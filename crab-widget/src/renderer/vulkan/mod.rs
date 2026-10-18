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

mod context;
mod driver;
mod pipeline;
mod swapchain;
mod texture;

use std::io;

use ash::{khr, vk};
use crab_common::{atlas::Rect, gpu::GpuInfo};
use raw_window_handle::{RawDisplayHandle, RawWindowHandle};
use snafu::{ResultExt, Snafu};

use crate::{
    renderer::{LayerOffset, Renderer, RendererError, Target},
    theme::Animation,
};
use context::{Context, Gpu, Platform};
use pipeline::Pipeline;
use swapchain::{Swapchain, color_range};
use texture::Textures;

const DAMAGE_MARGIN: f64 = 1.0;
const CLEAR: vk::ClearValue = vk::ClearValue {
    color: vk::ClearColorValue {
        float32: [0.0, 0.0, 0.0, 0.0],
    },
};

#[derive(Debug, Snafu)]
pub enum VulkanError {
    #[snafu(display("Unable to load the Vulkan loader"))]
    #[snafu(context(false))]
    Load { source: ash::LoadingError },
    #[snafu(display("Vulkan error while {thing}"))]
    Vk { source: vk::Result, thing: String },
    #[snafu(display("No compatible Vulkan device"))]
    NoDevice,
    #[snafu(display("Vulkan does not support {thing}"))]
    Unsupported { thing: String },
    #[snafu(display("No usable memory for the {thing}"))]
    Memory { thing: String },
    #[snafu(display("Invalid SPIR-V"))]
    Shader { source: io::Error },
    #[snafu(display("Error while {thing}"))]
    Read {
        source: rustix::io::Errno,
        thing: String,
    },
    #[snafu(display("A texture file is shorter than its manifest says"))]
    ShortRead,
}

pub struct Vulkan {
    gpu: Gpu,
    swapchain_loader: khr::swapchain::Device,
    surface: vk::SurfaceKHR,
    swapchain: Option<Swapchain>,
    pipeline: Pipeline,
    pool: vk::CommandPool,
    acquired: vk::Semaphore,
    in_flight: vk::Fence,
    textures: Option<Textures>,
    layers: Vec<Rect>,
    canvas: (u32, u32),
    size: (u32, u32),
    stale: bool,
    drawn: Option<vk::Rect2D>,
    damage: Option<vk::Rect2D>,
    context: Context,
}

pub fn list(
    display: RawDisplayHandle,
    visual: Option<u32>,
    compositor: Option<u64>,
) -> Result<Vec<GpuInfo>, VulkanError> {
    Context::list(Platform::new(display, visual)?, compositor)
}

impl Renderer for Vulkan {
    fn setup(target: Target) -> Result<Box<Self>, RendererError> {
        let visual = match target.window {
            RawWindowHandle::Xcb(handle) => handle.visual_id.map(std::num::NonZero::get),
            _ => None,
        };
        let platform = Platform::new(target.display, visual)?;
        let (context, candidate) = Context::open(platform, target.device, &target.gpu)?;
        let gpu = context.device(&candidate)?;
        let partial = Partial::new(context, gpu);
        Ok(Box::new(partial.finish(&target)?))
    }

    fn set_animation(&mut self, animation: &Animation) -> Result<(), RendererError> {
        self.wait()?;
        if let Some(mut textures) = self.textures.take() {
            textures.destroy(&self.gpu);
        }
        self.layers.clear();
        let textures = Textures::upload(&self.gpu, self.pool, animation)?;
        self.pipeline
            .bind_frames(&self.gpu.device, &textures.views, textures.layout);
        self.textures = Some(textures);
        self.layers = animation.layers.iter().map(|layer| layer.bounds).collect();
        self.canvas = (animation.width, animation.height);
        self.record()?;
        Ok(())
    }

    fn draw(&mut self, w: i32, h: i32, frame: u32, mirrored: bool, offsets: &[LayerOffset]) {
        if self.wait().is_err() {
            return;
        }
        let surface_aspect = f64::from(w) / f64::from(h);
        let (canvas_width, canvas_height) = (f64::from(self.canvas.0), f64::from(self.canvas.1));
        let frame_aspect = canvas_width / canvas_height;
        let direction = if mirrored { -1.0 } else { 1.0 };
        let scale_x = direction * (frame_aspect / surface_aspect).min(1.0);
        let scale_y = (surface_aspect / frame_aspect).min(1.0);
        let canvas = (canvas_width, canvas_height);
        let rects = self
            .layers
            .iter()
            .zip(
                offsets
                    .iter()
                    .copied()
                    .chain(std::iter::repeat(LayerOffset::default())),
            )
            .map(|(bounds, offset)| layer_rect(*bounds, offset, canvas));
        let drawn = self.swapchain.as_ref().and_then(|swapchain| {
            rects
                .clone()
                .map(|rect| pixel_rect(rect, (scale_x, scale_y), swapchain.extent))
                .reduce(union)
        });
        self.damage = self
            .drawn
            .zip(drawn)
            .map(|(previous, current)| union(previous, current));
        self.drawn = drawn;
        let uniforms = self.pipeline.uniforms();
        let header = [scale_x as f32, scale_y as f32, f64::from(frame) as f32, 0.0];
        for (slot, value) in uniforms.iter_mut().zip(
            header
                .into_iter()
                .chain(rects.flatten().map(|value| value as f32)),
        ) {
            *slot = value;
        }
    }

    fn replace_window(
        &mut self,
        window: RawWindowHandle,
        w: u32,
        h: u32,
    ) -> Result<(), RendererError> {
        self.idle()?;
        self.destroy_swapchain();
        unsafe { self.context.surface.destroy_surface(self.surface, None) };
        self.surface = self.context.create_surface(window)?;
        self.size = (w, h);
        self.rebuild()?;
        Ok(())
    }

    fn swapbuffers(&mut self) -> Result<(), RendererError> {
        if self.stale || self.swapchain.is_none() {
            self.rebuild()?;
        }
        let index = match self.acquire() {
            Err(vk::Result::ERROR_OUT_OF_DATE_KHR) => {
                self.rebuild()?;
                self.acquire()
            }
            acquired => acquired,
        }
        .context(VkSnafu {
            thing: "acquiring an image",
        })?;
        let Some(swapchain) = &self.swapchain else {
            return Ok(());
        };
        let (Some(command), Some(rendered)) = (
            swapchain.commands.get(index as usize),
            swapchain.rendered.get(index as usize),
        ) else {
            return Ok(());
        };
        let device = &self.gpu.device;
        let waits = [vk::SemaphoreSubmitInfo::default()
            .semaphore(self.acquired)
            .stage_mask(vk::PipelineStageFlags2::COLOR_ATTACHMENT_OUTPUT)];
        let signals = [vk::SemaphoreSubmitInfo::default()
            .semaphore(*rendered)
            .stage_mask(vk::PipelineStageFlags2::COLOR_ATTACHMENT_OUTPUT)];
        let commands = [vk::CommandBufferSubmitInfo::default().command_buffer(*command)];
        let submit = vk::SubmitInfo2::default()
            .wait_semaphore_infos(&waits)
            .command_buffer_infos(&commands)
            .signal_semaphore_infos(&signals);
        unsafe {
            device.reset_fences(&[self.in_flight]).context(VkSnafu {
                thing: "resetting the frame fence",
            })?;
            device
                .queue_submit2(self.gpu.queue, &[submit], self.in_flight)
                .context(VkSnafu {
                    thing: "submitting a frame",
                })?;
        }
        let wait_semaphores = [*rendered];
        let swapchains = [swapchain.handle];
        let indices = [index];
        let rectangles: Vec<vk::RectLayerKHR> = self
            .damage
            .filter(|_| self.gpu.incremental_present)
            .map(|damage| vk::RectLayerKHR {
                offset: damage.offset,
                extent: damage.extent,
                layer: 0,
            })
            .into_iter()
            .collect();
        let regions = [vk::PresentRegionKHR::default().rectangles(&rectangles)];
        let mut incremental = vk::PresentRegionsKHR::default().regions(&regions);
        let mut present = vk::PresentInfoKHR::default()
            .wait_semaphores(&wait_semaphores)
            .swapchains(&swapchains)
            .image_indices(&indices);
        if !rectangles.is_empty() {
            present = present.push_next(&mut incremental);
        }
        match unsafe {
            self.swapchain_loader
                .queue_present(self.gpu.queue, &present)
        } {
            Ok(suboptimal) => self.stale |= suboptimal,
            Err(vk::Result::ERROR_OUT_OF_DATE_KHR) => self.stale = true,
            Err(result) => {
                return Err(VulkanError::Vk {
                    source: result,
                    thing: "presenting".to_owned(),
                }
                .into());
            }
        }
        Ok(())
    }

    fn resize(&mut self, w: u32, h: u32) -> Result<(), RendererError> {
        if (w, h) != self.size {
            self.size = (w, h);
            self.stale = true;
        }
        Ok(())
    }
}

impl Vulkan {
    fn acquire(&mut self) -> Result<u32, vk::Result> {
        let Some(swapchain) = &self.swapchain else {
            return Err(vk::Result::ERROR_OUT_OF_DATE_KHR);
        };
        let (index, suboptimal) = unsafe {
            self.swapchain_loader.acquire_next_image(
                swapchain.handle,
                u64::MAX,
                self.acquired,
                vk::Fence::null(),
            )
        }?;
        self.stale |= suboptimal;
        Ok(index)
    }

    fn wait(&self) -> Result<(), VulkanError> {
        unsafe {
            self.gpu
                .device
                .wait_for_fences(&[self.in_flight], true, u64::MAX)
        }
        .context(VkSnafu {
            thing: "waiting for the last frame",
        })
    }

    fn idle(&self) -> Result<(), VulkanError> {
        unsafe { self.gpu.device.device_wait_idle() }.context(VkSnafu {
            thing: "waiting for the device",
        })
    }

    fn rebuild(&mut self) -> Result<(), VulkanError> {
        self.idle()?;
        let old = self.swapchain.take();
        let swapchain = Swapchain::new(
            &self.context,
            &self.gpu,
            &self.swapchain_loader,
            self.surface,
            self.pool,
            self.size,
            old.as_ref()
                .map_or(vk::SwapchainKHR::null(), |old| old.handle),
        );
        if let Some(mut old) = old {
            old.destroy(&self.gpu, &self.swapchain_loader, self.pool);
        }
        let swapchain = swapchain?;
        self.pipeline
            .set_format(&self.gpu.device, swapchain.format)?;
        self.swapchain = Some(swapchain);
        self.stale = false;
        self.drawn = None;
        self.damage = None;
        self.record()
    }

    fn destroy_swapchain(&mut self) {
        if let Some(mut swapchain) = self.swapchain.take() {
            swapchain.destroy(&self.gpu, &self.swapchain_loader, self.pool);
        }
    }

    fn record(&self) -> Result<(), VulkanError> {
        let Some(swapchain) = &self.swapchain else {
            return Ok(());
        };
        let device = &self.gpu.device;
        for ((command, image), view) in swapchain
            .commands
            .iter()
            .zip(&swapchain.images)
            .zip(&swapchain.views)
        {
            unsafe {
                device
                    .reset_command_buffer(*command, vk::CommandBufferResetFlags::empty())
                    .context(VkSnafu {
                        thing: "resetting a command buffer",
                    })?;
                device
                    .begin_command_buffer(*command, &vk::CommandBufferBeginInfo::default())
                    .context(VkSnafu {
                        thing: "recording a frame",
                    })?;
                self.record_frame(*command, *image, *view, swapchain.extent);
                device.end_command_buffer(*command).context(VkSnafu {
                    thing: "recording a frame",
                })?;
            }
        }
        Ok(())
    }

    unsafe fn record_frame(
        &self,
        command: vk::CommandBuffer,
        image: vk::Image,
        view: vk::ImageView,
        extent: vk::Extent2D,
    ) {
        let device = &self.gpu.device;
        let to_attachment = [vk::ImageMemoryBarrier2::default()
            .src_stage_mask(vk::PipelineStageFlags2::COLOR_ATTACHMENT_OUTPUT)
            .dst_stage_mask(vk::PipelineStageFlags2::COLOR_ATTACHMENT_OUTPUT)
            .dst_access_mask(vk::AccessFlags2::COLOR_ATTACHMENT_WRITE)
            .old_layout(vk::ImageLayout::UNDEFINED)
            .new_layout(vk::ImageLayout::COLOR_ATTACHMENT_OPTIMAL)
            .image(image)
            .subresource_range(color_range(1))];
        let to_present = [vk::ImageMemoryBarrier2::default()
            .src_stage_mask(vk::PipelineStageFlags2::COLOR_ATTACHMENT_OUTPUT)
            .src_access_mask(vk::AccessFlags2::COLOR_ATTACHMENT_WRITE)
            .old_layout(vk::ImageLayout::COLOR_ATTACHMENT_OPTIMAL)
            .new_layout(vk::ImageLayout::PRESENT_SRC_KHR)
            .image(image)
            .subresource_range(color_range(1))];
        let attachments = [vk::RenderingAttachmentInfo::default()
            .image_view(view)
            .image_layout(vk::ImageLayout::COLOR_ATTACHMENT_OPTIMAL)
            .load_op(vk::AttachmentLoadOp::CLEAR)
            .store_op(vk::AttachmentStoreOp::STORE)
            .clear_value(CLEAR)];
        let area = vk::Rect2D::default().extent(extent);
        let rendering = vk::RenderingInfo::default()
            .render_area(area)
            .layer_count(1)
            .color_attachments(&attachments);
        let viewport = vk::Viewport::default()
            .width(f64::from(extent.width) as f32)
            .height(f64::from(extent.height) as f32)
            .max_depth(1.0);
        unsafe {
            device.cmd_pipeline_barrier2(
                command,
                &vk::DependencyInfo::default().image_memory_barriers(&to_attachment),
            );
            device.cmd_begin_rendering(command, &rendering);
            if !self.layers.is_empty() {
                device.cmd_set_viewport(command, 0, &[viewport]);
                device.cmd_set_scissor(command, 0, &[area]);
                device.cmd_bind_pipeline(
                    command,
                    vk::PipelineBindPoint::GRAPHICS,
                    self.pipeline.handle,
                );
                device.cmd_bind_descriptor_sets(
                    command,
                    vk::PipelineBindPoint::GRAPHICS,
                    self.pipeline.layout,
                    0,
                    &[self.pipeline.set],
                    &[],
                );
                for layer in 0..self.layers.len() as u32 {
                    device.cmd_push_constants(
                        command,
                        self.pipeline.layout,
                        vk::ShaderStageFlags::VERTEX | vk::ShaderStageFlags::FRAGMENT,
                        0,
                        &layer.to_ne_bytes(),
                    );
                    device.cmd_draw(command, 4, 1, 0, 0);
                }
            }
            device.cmd_end_rendering(command);
            device.cmd_pipeline_barrier2(
                command,
                &vk::DependencyInfo::default().image_memory_barriers(&to_present),
            );
        }
    }
}

fn layer_rect(bounds: Rect, offset: LayerOffset, (width, height): (f64, f64)) -> [f64; 4] {
    let half_width = f64::from(bounds.width) / 2.0 * offset.stretch_x;
    let center_x = f64::from(bounds.x) + f64::from(bounds.width) / 2.0 + offset.x;
    let top = f64::from(bounds.y) + offset.y;
    [
        (center_x - half_width) / width,
        top / height,
        (center_x + half_width) / width,
        (top + f64::from(bounds.height)) / height,
    ]
}

fn pixel_rect(
    [left, top, right, bottom]: [f64; 4],
    (scale_x, scale_y): (f64, f64),
    extent: vk::Extent2D,
) -> vk::Rect2D {
    let (width, height) = (f64::from(extent.width), f64::from(extent.height));
    let to_x = |canvas: f64| {
        (canvas.mul_add(2.0, -1.0).mul_add(scale_x, 1.0) / 2.0 * width).clamp(0.0, width)
    };
    let to_y = |canvas: f64| {
        (canvas.mul_add(2.0, -1.0).mul_add(scale_y, 1.0) / 2.0 * height).clamp(0.0, height)
    };
    let (x0, x1) = (to_x(left), to_x(right));
    let (y0, y1) = (to_y(top), to_y(bottom));
    let x = (x0.min(x1) - DAMAGE_MARGIN).floor().max(0.0);
    let y = (y0.min(y1) - DAMAGE_MARGIN).floor().max(0.0);
    let right = (x0.max(x1) + DAMAGE_MARGIN).ceil().min(width);
    let bottom = (y0.max(y1) + DAMAGE_MARGIN).ceil().min(height);
    vk::Rect2D {
        offset: vk::Offset2D {
            x: x as i32,
            y: y as i32,
        },
        extent: vk::Extent2D {
            width: (right - x).max(0.0) as u32,
            height: (bottom - y).max(0.0) as u32,
        },
    }
}

fn union(a: vk::Rect2D, b: vk::Rect2D) -> vk::Rect2D {
    let left = a.offset.x.min(b.offset.x);
    let top = a.offset.y.min(b.offset.y);
    let right = (a.offset.x + a.extent.width as i32).max(b.offset.x + b.extent.width as i32);
    let bottom = (a.offset.y + a.extent.height as i32).max(b.offset.y + b.extent.height as i32);
    vk::Rect2D {
        offset: vk::Offset2D { x: left, y: top },
        extent: vk::Extent2D {
            width: (right - left) as u32,
            height: (bottom - top) as u32,
        },
    }
}

impl Drop for Vulkan {
    fn drop(&mut self) {
        let _ = self.idle();
        self.destroy_swapchain();
        if let Some(mut textures) = self.textures.take() {
            textures.destroy(&self.gpu);
        }
        let device = &self.gpu.device;
        self.pipeline.destroy(device);
        unsafe {
            device.destroy_fence(self.in_flight, None);
            device.destroy_semaphore(self.acquired, None);
            device.destroy_command_pool(self.pool, None);
            device.destroy_device(None);
            self.context.surface.destroy_surface(self.surface, None);
        }
    }
}

struct Partial {
    context: Context,
    gpu: Gpu,
    surface: vk::SurfaceKHR,
    pool: vk::CommandPool,
    acquired: vk::Semaphore,
    in_flight: vk::Fence,
}

impl Partial {
    const fn new(context: Context, gpu: Gpu) -> Self {
        Self {
            context,
            gpu,
            surface: vk::SurfaceKHR::null(),
            pool: vk::CommandPool::null(),
            acquired: vk::Semaphore::null(),
            in_flight: vk::Fence::null(),
        }
    }

    fn finish(mut self, target: &Target) -> Result<Vulkan, VulkanError> {
        let (swapchain_loader, swapchain, pipeline) = match self.prepare(target) {
            Ok(prepared) => prepared,
            Err(error) => {
                self.destroy();
                return Err(error);
            }
        };
        let renderer = Vulkan {
            gpu: self.gpu,
            context: self.context,
            swapchain_loader,
            surface: self.surface,
            swapchain: Some(swapchain),
            pipeline,
            pool: self.pool,
            acquired: self.acquired,
            in_flight: self.in_flight,
            textures: None,
            layers: Vec::new(),
            canvas: (1, 1),
            size: target.size,
            stale: false,
            drawn: None,
            damage: None,
        };
        renderer.record()?;
        Ok(renderer)
    }

    fn prepare(
        &mut self,
        target: &Target,
    ) -> Result<(khr::swapchain::Device, Swapchain, Pipeline), VulkanError> {
        let device = &self.gpu.device;
        self.surface = self.context.create_surface(target.window)?;
        self.pool = unsafe {
            device.create_command_pool(
                &vk::CommandPoolCreateInfo::default()
                    .flags(vk::CommandPoolCreateFlags::RESET_COMMAND_BUFFER)
                    .queue_family_index(self.gpu.family),
                None,
            )
        }
        .context(VkSnafu {
            thing: "creating the command pool",
        })?;
        self.acquired =
            unsafe { device.create_semaphore(&vk::SemaphoreCreateInfo::default(), None) }.context(
                VkSnafu {
                    thing: "creating a semaphore",
                },
            )?;
        self.in_flight = unsafe {
            device.create_fence(
                &vk::FenceCreateInfo::default().flags(vk::FenceCreateFlags::SIGNALED),
                None,
            )
        }
        .context(VkSnafu {
            thing: "creating the frame fence",
        })?;
        let swapchain_loader = khr::swapchain::Device::new(&self.context.instance, device);
        let mut swapchain = Swapchain::new(
            &self.context,
            &self.gpu,
            &swapchain_loader,
            self.surface,
            self.pool,
            target.size,
            vk::SwapchainKHR::null(),
        )?;
        match Pipeline::new(&self.gpu, target.layers.max(1), swapchain.format) {
            Ok(pipeline) => Ok((swapchain_loader, swapchain, pipeline)),
            Err(error) => {
                swapchain.destroy(&self.gpu, &swapchain_loader, self.pool);
                Err(error)
            }
        }
    }

    fn destroy(&self) {
        let device = &self.gpu.device;
        unsafe {
            device.destroy_fence(self.in_flight, None);
            device.destroy_semaphore(self.acquired, None);
            device.destroy_command_pool(self.pool, None);
            device.destroy_device(None);
            self.context.surface.destroy_surface(self.surface, None);
        }
    }
}
