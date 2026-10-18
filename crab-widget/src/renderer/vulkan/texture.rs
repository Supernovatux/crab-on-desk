// crab-on-desk, a Rust based desktop pet for coding agents.
//     Copyright (C) 2026  Supernovatux thulashitharan.d@gmail.com
//
//     This program is free software: you can redistribute it and/or modify
//     it under the terms of the GNU Affero General Public License as
//     published by the Free Software Foundation, either version 3 of the
//     License, or (at your option) any later version.
//
//     This program is distributed in the hope that it will be useful,
//     but WITHOUT ANY WARRANTY; without even the implied warranty of
//     MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE.  See the
//     GNU Affero General Public License for more details.
//
//     You should have received a copy of the GNU Affero General Public License
//     along with this program.  If not, see <https://www.gnu.org/licenses/>.

use std::{
    os::fd::OwnedFd,
    ptr::{self, NonNull},
    slice,
};

use ash::vk;
use rustix::{
    fs::{AtFlags, Mode, OFlags, StatxFlags},
    io::Errno,
    mm::{MapFlags, ProtFlags},
};
use snafu::{OptionExt, ResultExt, ensure};

use super::{
    MemorySnafu, ReadSnafu, ShortReadSnafu, VkSnafu, VulkanError, context::Gpu,
    swapchain::color_range,
};
use crate::theme::{Animation, Layer};

const FORMAT: vk::Format = vk::Format::BC7_UNORM_BLOCK;
const BLOCK_BYTES: u64 = 16;
const PAGE_SIZE: u64 = 4096;
const CHUNK_BYTES: u64 = 2 << 20;
const SLOTS: usize = 2;

pub struct Textures {
    images: Vec<vk::Image>,
    pub views: Vec<vk::ImageView>,
    memory: vk::DeviceMemory,
    pub layout: vk::ImageLayout,
}

enum Route {
    Direct { files: Vec<OwnedFd>, align: u64 },
    HostCopy(vk::ImageLayout),
    Staging,
}

struct Job {
    layer: usize,
    first: u32,
    frames: u32,
    offset: u64,
    bytes: u64,
}

struct Ring {
    buffer: vk::Buffer,
    memory: vk::DeviceMemory,
    host: NonNull<u8>,
    chunk: u64,
    anonymous: bool,
}

struct Slot {
    command: vk::CommandBuffer,
    fence: vk::Fence,
}

impl Textures {
    pub fn upload(
        gpu: &Gpu,
        pool: vk::CommandPool,
        animation: &Animation,
    ) -> Result<Self, VulkanError> {
        let route = route(gpu, animation);
        let mut textures = Self {
            images: Vec::new(),
            views: Vec::new(),
            memory: vk::DeviceMemory::null(),
            layout: match route {
                Route::HostCopy(layout) => layout,
                Route::Direct { .. } | Route::Staging => vk::ImageLayout::SHADER_READ_ONLY_OPTIMAL,
            },
        };
        match textures.fill(gpu, pool, animation, route) {
            Ok(()) => Ok(textures),
            Err(error) => {
                textures.destroy(gpu);
                Err(error)
            }
        }
    }

    fn fill(
        &mut self,
        gpu: &Gpu,
        pool: vk::CommandPool,
        animation: &Animation,
        route: Route,
    ) -> Result<(), VulkanError> {
        let device = &gpu.device;
        let usage = match route {
            Route::HostCopy(_) => {
                vk::ImageUsageFlags::SAMPLED | vk::ImageUsageFlags::HOST_TRANSFER_EXT
            }
            Route::Direct { .. } | Route::Staging => {
                vk::ImageUsageFlags::SAMPLED | vk::ImageUsageFlags::TRANSFER_DST
            }
        };
        for layer in &animation.layers {
            let info = vk::ImageCreateInfo::default()
                .image_type(vk::ImageType::TYPE_2D)
                .format(FORMAT)
                .extent(extent(layer))
                .mip_levels(1)
                .array_layers(animation.frame_count)
                .samples(vk::SampleCountFlags::TYPE_1)
                .tiling(vk::ImageTiling::OPTIMAL)
                .usage(usage)
                .sharing_mode(vk::SharingMode::EXCLUSIVE)
                .initial_layout(vk::ImageLayout::UNDEFINED);
            self.images.push(
                unsafe { device.create_image(&info, None) }.context(VkSnafu {
                    thing: "creating a texture",
                })?,
            );
        }
        self.bind_memory(gpu)?;
        match route {
            Route::HostCopy(layout) => self.copy_from_host(gpu, animation, layout)?,
            Route::Direct { files, align } => {
                let jobs = jobs(animation, align, Some(&files));
                let chunk = chunk_bytes(animation, align);
                let mut ring = Ring::imported(gpu, chunk)?;
                let copied =
                    self.copy_through(gpu, pool, &mut ring, animation, &jobs, |slot, job| {
                        read_direct(slot, &files, job, align)
                    });
                ring.destroy(gpu);
                copied?;
            }
            Route::Staging => {
                let jobs = jobs(animation, 1, None);
                let chunk = chunk_bytes(animation, 1);
                let mut ring = Ring::staging(gpu, chunk)?;
                let copied =
                    self.copy_through(gpu, pool, &mut ring, animation, &jobs, |slot, job| {
                        copy_mapped(slot, animation, job)
                    });
                ring.destroy(gpu);
                copied?;
            }
        }
        for image in &self.images {
            let info = vk::ImageViewCreateInfo::default()
                .image(*image)
                .view_type(vk::ImageViewType::TYPE_2D_ARRAY)
                .format(FORMAT)
                .subresource_range(color_range(animation.frame_count));
            self.views.push(
                unsafe { device.create_image_view(&info, None) }.context(VkSnafu {
                    thing: "creating a texture view",
                })?,
            );
        }
        Ok(())
    }

    fn bind_memory(&mut self, gpu: &Gpu) -> Result<(), VulkanError> {
        let device = &gpu.device;
        let mut offsets = Vec::with_capacity(self.images.len());
        let mut size: u64 = 0;
        let mut bits = u32::MAX;
        for image in &self.images {
            let requirements = unsafe { device.get_image_memory_requirements(*image) };
            let offset = size.next_multiple_of(requirements.alignment.max(1));
            offsets.push(offset);
            size = offset + requirements.size;
            bits &= requirements.memory_type_bits;
        }
        let memory_type = gpu
            .memory_type(
                bits,
                vk::MemoryPropertyFlags::DEVICE_LOCAL,
                vk::MemoryPropertyFlags::HOST_VISIBLE,
            )
            .context(MemorySnafu { thing: "textures" })?;
        self.memory = unsafe {
            device.allocate_memory(
                &vk::MemoryAllocateInfo::default()
                    .allocation_size(size)
                    .memory_type_index(memory_type),
                None,
            )
        }
        .context(VkSnafu {
            thing: "allocating texture memory",
        })?;
        for (image, offset) in self.images.iter().zip(offsets) {
            unsafe { device.bind_image_memory(*image, self.memory, offset) }.context(VkSnafu {
                thing: "binding texture memory",
            })?;
        }
        Ok(())
    }

    fn copy_from_host(
        &self,
        gpu: &Gpu,
        animation: &Animation,
        layout: vk::ImageLayout,
    ) -> Result<(), VulkanError> {
        let (loader, _) = gpu
            .host_image_copy
            .as_ref()
            .context(MemorySnafu { thing: "host copy" })?;
        for (layer, image) in animation.layers.iter().zip(&self.images) {
            let transition = vk::HostImageLayoutTransitionInfoEXT::default()
                .image(*image)
                .old_layout(vk::ImageLayout::UNDEFINED)
                .new_layout(layout)
                .subresource_range(color_range(animation.frame_count));
            unsafe { loader.transition_image_layout(&[transition]) }.context(VkSnafu {
                thing: "preparing a texture for host copies",
            })?;
            let regions = [vk::MemoryToImageCopyEXT::default()
                .host_pointer(layer.blocks().as_ptr().cast())
                .image_subresource(layers(0, animation.frame_count))
                .image_extent(extent(layer))];
            let info = vk::CopyMemoryToImageInfoEXT::default()
                .dst_image(*image)
                .dst_image_layout(layout)
                .regions(&regions);
            unsafe { loader.copy_memory_to_image(&info) }.context(VkSnafu {
                thing: "copying a texture from the host",
            })?;
        }
        Ok(())
    }

    fn copy_through(
        &self,
        gpu: &Gpu,
        pool: vk::CommandPool,
        ring: &mut Ring,
        animation: &Animation,
        jobs: &[Job],
        mut fill: impl FnMut(&mut [u8], &Job) -> Result<u64, VulkanError>,
    ) -> Result<(), VulkanError> {
        let device = &gpu.device;
        let commands = unsafe {
            device.allocate_command_buffers(
                &vk::CommandBufferAllocateInfo::default()
                    .command_pool(pool)
                    .level(vk::CommandBufferLevel::PRIMARY)
                    .command_buffer_count(SLOTS as u32),
            )
        }
        .context(VkSnafu {
            thing: "allocating upload commands",
        })?;
        let mut slots = Vec::with_capacity(SLOTS);
        for command in &commands {
            match unsafe {
                device.create_fence(
                    &vk::FenceCreateInfo::default().flags(vk::FenceCreateFlags::SIGNALED),
                    None,
                )
            } {
                Ok(fence) => slots.push(Slot {
                    command: *command,
                    fence,
                }),
                Err(result) => {
                    release(gpu, pool, &commands, &slots);
                    return Err(result).context(VkSnafu {
                        thing: "creating an upload fence",
                    });
                }
            }
        }
        let copied = self.copy_jobs(gpu, ring, animation, jobs, &slots, &mut fill);
        let fences: Vec<vk::Fence> = slots.iter().map(|slot| slot.fence).collect();
        let waited = unsafe { device.wait_for_fences(&fences, true, u64::MAX) };
        release(gpu, pool, &commands, &slots);
        copied?;
        waited.context(VkSnafu {
            thing: "waiting for uploads",
        })
    }

    fn copy_jobs(
        &self,
        gpu: &Gpu,
        ring: &mut Ring,
        animation: &Animation,
        jobs: &[Job],
        slots: &[Slot],
        fill: &mut impl FnMut(&mut [u8], &Job) -> Result<u64, VulkanError>,
    ) -> Result<(), VulkanError> {
        let device = &gpu.device;
        let last = jobs.len().saturating_sub(1);
        for (index, job) in jobs.iter().enumerate() {
            let slot_index = index % SLOTS;
            let Some(slot) = slots.get(slot_index) else {
                continue;
            };
            unsafe { device.wait_for_fences(&[slot.fence], true, u64::MAX) }.context(VkSnafu {
                thing: "waiting for an upload slot",
            })?;
            let within = fill(ring.slot(slot_index), job)?;
            let image = *self.images.get(job.layer).context(MemorySnafu {
                thing: "upload target",
            })?;
            let layer = animation.layers.get(job.layer).context(MemorySnafu {
                thing: "upload layer",
            })?;
            let region = vk::BufferImageCopy::default()
                .buffer_offset(ring.chunk * slot_index as u64 + within)
                .image_subresource(layers(job.first, job.frames))
                .image_extent(extent(layer));
            unsafe {
                device.reset_fences(&[slot.fence]).context(VkSnafu {
                    thing: "resetting an upload fence",
                })?;
                device
                    .begin_command_buffer(
                        slot.command,
                        &vk::CommandBufferBeginInfo::default()
                            .flags(vk::CommandBufferUsageFlags::ONE_TIME_SUBMIT),
                    )
                    .context(VkSnafu {
                        thing: "recording an upload",
                    })?;
                if index == 0 {
                    self.barrier(
                        device,
                        slot.command,
                        animation.frame_count,
                        Transition::ToCopy,
                    );
                }
                device.cmd_copy_buffer_to_image(
                    slot.command,
                    ring.buffer,
                    image,
                    vk::ImageLayout::TRANSFER_DST_OPTIMAL,
                    &[region],
                );
                if index == last {
                    self.barrier(
                        device,
                        slot.command,
                        animation.frame_count,
                        Transition::ToSampled,
                    );
                }
                device.end_command_buffer(slot.command).context(VkSnafu {
                    thing: "recording an upload",
                })?;
                let commands =
                    [vk::CommandBufferSubmitInfo::default().command_buffer(slot.command)];
                let submit = vk::SubmitInfo2::default().command_buffer_infos(&commands);
                device
                    .queue_submit2(gpu.queue, &[submit], slot.fence)
                    .context(VkSnafu {
                        thing: "submitting an upload",
                    })?;
            }
        }
        Ok(())
    }

    fn barrier(
        &self,
        device: &ash::Device,
        command: vk::CommandBuffer,
        frames: u32,
        transition: Transition,
    ) {
        let barriers: Vec<vk::ImageMemoryBarrier2> = self
            .images
            .iter()
            .map(|image| {
                let barrier = vk::ImageMemoryBarrier2::default()
                    .image(*image)
                    .subresource_range(color_range(frames));
                match transition {
                    Transition::ToCopy => barrier
                        .dst_stage_mask(vk::PipelineStageFlags2::COPY)
                        .dst_access_mask(vk::AccessFlags2::TRANSFER_WRITE)
                        .old_layout(vk::ImageLayout::UNDEFINED)
                        .new_layout(vk::ImageLayout::TRANSFER_DST_OPTIMAL),
                    Transition::ToSampled => barrier
                        .src_stage_mask(vk::PipelineStageFlags2::COPY)
                        .src_access_mask(vk::AccessFlags2::TRANSFER_WRITE)
                        .dst_stage_mask(vk::PipelineStageFlags2::FRAGMENT_SHADER)
                        .dst_access_mask(vk::AccessFlags2::SHADER_SAMPLED_READ)
                        .old_layout(vk::ImageLayout::TRANSFER_DST_OPTIMAL)
                        .new_layout(vk::ImageLayout::SHADER_READ_ONLY_OPTIMAL),
                }
            })
            .collect();
        unsafe {
            device.cmd_pipeline_barrier2(
                command,
                &vk::DependencyInfo::default().image_memory_barriers(&barriers),
            );
        }
    }

    pub fn destroy(&mut self, gpu: &Gpu) {
        let device = &gpu.device;
        unsafe {
            for view in self.views.drain(..) {
                device.destroy_image_view(view, None);
            }
            for image in self.images.drain(..) {
                device.destroy_image(image, None);
            }
            device.free_memory(self.memory, None);
        }
        self.memory = vk::DeviceMemory::null();
    }
}

#[derive(Clone, Copy)]
enum Transition {
    ToCopy,
    ToSampled,
}

impl Ring {
    fn imported(gpu: &Gpu, chunk: u64) -> Result<Self, VulkanError> {
        let loader = gpu.external_memory_host.as_ref().context(MemorySnafu {
            thing: "host import",
        })?;
        let size = chunk * SLOTS as u64;
        let host = unsafe {
            rustix::mm::mmap_anonymous(
                ptr::null_mut(),
                size as usize,
                ProtFlags::READ | ProtFlags::WRITE,
                MapFlags::PRIVATE,
            )
        }
        .context(ReadSnafu {
            thing: "mapping the upload buffer",
        })?;
        let unmap = || unsafe {
            let _ = rustix::mm::munmap(host, size as usize);
        };
        let Some(host) = NonNull::new(host.cast::<u8>()) else {
            return MemorySnafu {
                thing: "upload buffer",
            }
            .fail();
        };
        let handle = vk::ExternalMemoryHandleTypeFlags::HOST_ALLOCATION_EXT;
        let mut properties = vk::MemoryHostPointerPropertiesEXT::default();
        let imported = unsafe {
            (loader.fp().get_memory_host_pointer_properties_ext)(
                gpu.device.handle(),
                handle,
                host.as_ptr().cast(),
                &raw mut properties,
            )
        }
        .result()
        .context(VkSnafu {
            thing: "importing the upload buffer",
        })
        .and_then(|()| {
            let mut external = vk::ExternalMemoryBufferCreateInfo::default().handle_types(handle);
            let mut import = vk::ImportMemoryHostPointerInfoEXT::default()
                .handle_type(handle)
                .host_pointer(host.as_ptr().cast());
            buffer_with_memory(
                gpu,
                size,
                properties.memory_type_bits,
                &mut external,
                Some(&mut import),
            )
        });
        match imported {
            Ok((buffer, memory)) => Ok(Self {
                buffer,
                memory,
                host,
                chunk,
                anonymous: true,
            }),
            Err(error) => {
                unmap();
                Err(error)
            }
        }
    }

    fn staging(gpu: &Gpu, chunk: u64) -> Result<Self, VulkanError> {
        let size = chunk * SLOTS as u64;
        let mut external = vk::ExternalMemoryBufferCreateInfo::default();
        let (buffer, memory) = buffer_with_memory(gpu, size, u32::MAX, &mut external, None)?;
        let mapped = unsafe {
            gpu.device
                .map_memory(memory, 0, size, vk::MemoryMapFlags::empty())
        };
        match mapped.map(|mapped| NonNull::new(mapped.cast::<u8>())) {
            Ok(Some(host)) => Ok(Self {
                buffer,
                memory,
                host,
                chunk,
                anonymous: false,
            }),
            mapped => {
                unsafe {
                    gpu.device.destroy_buffer(buffer, None);
                    gpu.device.free_memory(memory, None);
                }
                mapped.context(VkSnafu {
                    thing: "mapping the staging buffer",
                })?;
                MemorySnafu {
                    thing: "staging buffer",
                }
                .fail()
            }
        }
    }

    const fn slot(&mut self, index: usize) -> &mut [u8] {
        let chunk = self.chunk as usize;
        unsafe { slice::from_raw_parts_mut(self.host.as_ptr().add(chunk * index), chunk) }
    }

    fn destroy(&self, gpu: &Gpu) {
        unsafe {
            gpu.device.destroy_buffer(self.buffer, None);
            gpu.device.free_memory(self.memory, None);
            if self.anonymous {
                let _ = rustix::mm::munmap(self.host.as_ptr().cast(), self.chunk as usize * SLOTS);
            }
        }
    }
}

fn buffer_with_memory(
    gpu: &Gpu,
    size: u64,
    allowed: u32,
    external: &mut vk::ExternalMemoryBufferCreateInfo<'_>,
    import: Option<&mut vk::ImportMemoryHostPointerInfoEXT<'_>>,
) -> Result<(vk::Buffer, vk::DeviceMemory), VulkanError> {
    let device = &gpu.device;
    let mut info = vk::BufferCreateInfo::default()
        .size(size)
        .usage(vk::BufferUsageFlags::TRANSFER_SRC)
        .sharing_mode(vk::SharingMode::EXCLUSIVE);
    if import.is_some() {
        info = info.push_next(external);
    }
    let buffer = unsafe { device.create_buffer(&info, None) }.context(VkSnafu {
        thing: "creating an upload buffer",
    })?;
    let requirements = unsafe { device.get_buffer_memory_requirements(buffer) };
    let required = if import.is_some() {
        vk::MemoryPropertyFlags::empty()
    } else {
        vk::MemoryPropertyFlags::HOST_VISIBLE | vk::MemoryPropertyFlags::HOST_COHERENT
    };
    let memory_type = gpu.memory_type(
        requirements.memory_type_bits & allowed,
        required,
        vk::MemoryPropertyFlags::empty(),
    );
    let Some(memory_type) = memory_type else {
        unsafe { device.destroy_buffer(buffer, None) };
        return MemorySnafu {
            thing: "upload buffer",
        }
        .fail();
    };
    let mut allocate = vk::MemoryAllocateInfo::default()
        .allocation_size(if import.is_some() {
            size
        } else {
            requirements.size
        })
        .memory_type_index(memory_type);
    if let Some(import) = import {
        allocate = allocate.push_next(import);
    }
    let memory = match unsafe { device.allocate_memory(&allocate, None) } {
        Ok(memory) => memory,
        Err(result) => {
            unsafe { device.destroy_buffer(buffer, None) };
            return Err(result).context(VkSnafu {
                thing: "allocating an upload buffer",
            });
        }
    };
    if let Err(result) = unsafe { device.bind_buffer_memory(buffer, memory, 0) } {
        unsafe {
            device.destroy_buffer(buffer, None);
            device.free_memory(memory, None);
        }
        return Err(result).context(VkSnafu {
            thing: "binding an upload buffer",
        });
    }
    Ok((buffer, memory))
}

fn release(gpu: &Gpu, pool: vk::CommandPool, commands: &[vk::CommandBuffer], slots: &[Slot]) {
    unsafe {
        for slot in slots {
            gpu.device.destroy_fence(slot.fence, None);
        }
        gpu.device.free_command_buffers(pool, commands);
    }
}

fn route(gpu: &Gpu, animation: &Animation) -> Route {
    if gpu.external_memory_host.is_some()
        && let Some((files, align)) = direct_files(animation)
    {
        return Route::Direct { files, align };
    }
    gpu.host_image_copy
        .as_ref()
        .map_or(Route::Staging, |(_, layout)| Route::HostCopy(*layout))
}

fn direct_files(animation: &Animation) -> Option<(Vec<OwnedFd>, u64)> {
    let mut align = BLOCK_BYTES;
    let mut files = Vec::with_capacity(animation.layers.len());
    for layer in &animation.layers {
        if layer.data_offset() % BLOCK_BYTES != 0 {
            return None;
        }
        let file = rustix::fs::open(
            &layer.path,
            OFlags::RDONLY | OFlags::DIRECT | OFlags::CLOEXEC,
            Mode::empty(),
        )
        .ok()?;
        let stat = rustix::fs::statx(&file, "", AtFlags::EMPTY_PATH, StatxFlags::DIOALIGN).ok()?;
        let offset_align = u64::from(stat.stx_dio_offset_align);
        let memory_align = u64::from(stat.stx_dio_mem_align);
        if !StatxFlags::from_bits_retain(stat.stx_mask).contains(StatxFlags::DIOALIGN)
            || offset_align == 0
            || memory_align == 0
            || offset_align > PAGE_SIZE
            || memory_align > PAGE_SIZE
        {
            return None;
        }
        align = align.max(offset_align);
        files.push(file);
    }
    Some((files, align))
}

fn chunk_bytes(animation: &Animation, align: u64) -> u64 {
    let largest = animation
        .layers
        .iter()
        .map(Layer::frame_bytes)
        .max()
        .unwrap_or(0);
    CHUNK_BYTES.max((largest + 2 * align).next_multiple_of(PAGE_SIZE))
}

fn jobs(animation: &Animation, align: u64, files: Option<&[OwnedFd]>) -> Vec<Job> {
    let chunk = chunk_bytes(animation, align);
    let mut jobs = Vec::new();
    for (index, layer) in animation.layers.iter().enumerate() {
        let frame_bytes = layer.frame_bytes();
        let per_job = ((chunk - 2 * align) / frame_bytes).max(1) as u32;
        let base = if files.is_some() {
            layer.data_offset()
        } else {
            0
        };
        let mut first = 0;
        while first < animation.frame_count {
            let frames = per_job.min(animation.frame_count - first);
            jobs.push(Job {
                layer: index,
                first,
                frames,
                offset: base + u64::from(first) * frame_bytes,
                bytes: u64::from(frames) * frame_bytes,
            });
            first += frames;
        }
    }
    jobs
}

fn read_direct(
    slot: &mut [u8],
    files: &[OwnedFd],
    job: &Job,
    align: u64,
) -> Result<u64, VulkanError> {
    let file = files.get(job.layer).context(MemorySnafu {
        thing: "upload file",
    })?;
    let start = job.offset - job.offset % align;
    let end = (job.offset + job.bytes).next_multiple_of(align);
    let window = slot
        .get_mut(..(end - start) as usize)
        .context(MemorySnafu {
            thing: "upload slot",
        })?;
    let read = loop {
        match rustix::io::pread(file, &mut *window, start) {
            Err(Errno::INTR) => {}
            read => {
                break read.context(ReadSnafu {
                    thing: "reading a texture",
                })?;
            }
        }
    };
    let needed = job.offset + job.bytes - start;
    ensure!(read as u64 >= needed, ShortReadSnafu);
    Ok(job.offset - start)
}

fn copy_mapped(slot: &mut [u8], animation: &Animation, job: &Job) -> Result<u64, VulkanError> {
    let layer = animation.layers.get(job.layer).context(MemorySnafu {
        thing: "upload layer",
    })?;
    let range = job.offset as usize..(job.offset + job.bytes) as usize;
    let source = layer.blocks().get(range).context(ShortReadSnafu)?;
    slot.get_mut(..source.len())
        .context(MemorySnafu {
            thing: "upload slot",
        })?
        .copy_from_slice(source);
    Ok(0)
}

const fn extent(layer: &Layer) -> vk::Extent3D {
    vk::Extent3D {
        width: layer.bounds.width,
        height: layer.bounds.height,
        depth: 1,
    }
}

const fn layers(first: u32, count: u32) -> vk::ImageSubresourceLayers {
    vk::ImageSubresourceLayers {
        aspect_mask: vk::ImageAspectFlags::COLOR,
        mip_level: 0,
        base_array_layer: first,
        layer_count: count,
    }
}
