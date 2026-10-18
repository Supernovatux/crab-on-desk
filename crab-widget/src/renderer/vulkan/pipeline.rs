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

use std::{io::Cursor, mem::size_of, ptr::NonNull, slice};

use ash::{Device, util::read_spv, vk};
use snafu::{OptionExt, ResultExt};

use super::{MemorySnafu, ShaderSnafu, VkSnafu, VulkanError, context::Gpu};

const VERTEX_SHADER: &[u8] = include_bytes!("../../../shaders/quad.vert.spv");
const FRAGMENT_SHADER: &[u8] = include_bytes!("../../../shaders/quad.frag.spv");
const ENTRY_POINT: &std::ffi::CStr = c"main";
const FRAMES_BINDING: u32 = 0;
const UNIFORM_BINDING: u32 = 1;
const FLOATS_PER_VEC4: usize = 4;

pub struct Pipeline {
    pub layout: vk::PipelineLayout,
    pub handle: vk::Pipeline,
    pub format: vk::Format,
    pub set: vk::DescriptorSet,
    set_layout: vk::DescriptorSetLayout,
    pool: vk::DescriptorPool,
    sampler: vk::Sampler,
    uniform: vk::Buffer,
    uniform_memory: vk::DeviceMemory,
    mapped: Option<NonNull<f32>>,
    layers: usize,
}

impl Pipeline {
    pub fn new(gpu: &Gpu, layers: usize, format: vk::Format) -> Result<Self, VulkanError> {
        let mut pipeline = Self {
            layout: vk::PipelineLayout::null(),
            handle: vk::Pipeline::null(),
            format,
            set: vk::DescriptorSet::null(),
            set_layout: vk::DescriptorSetLayout::null(),
            pool: vk::DescriptorPool::null(),
            sampler: vk::Sampler::null(),
            uniform: vk::Buffer::null(),
            uniform_memory: vk::DeviceMemory::null(),
            mapped: None,
            layers,
        };
        match pipeline.build(gpu) {
            Ok(()) => Ok(pipeline),
            Err(error) => {
                pipeline.destroy(&gpu.device);
                Err(error)
            }
        }
    }

    fn build(&mut self, gpu: &Gpu) -> Result<(), VulkanError> {
        self.create_descriptors(&gpu.device)?;
        self.create_uniform(gpu)?;
        self.handle = create_pipeline(&gpu.device, self.layout, self.layers, self.format)?;
        self.uniforms().fill(0.0);
        Ok(())
    }

    fn create_descriptors(&mut self, device: &Device) -> Result<(), VulkanError> {
        let sampler_info = vk::SamplerCreateInfo::default()
            .mag_filter(vk::Filter::LINEAR)
            .min_filter(vk::Filter::LINEAR)
            .mipmap_mode(vk::SamplerMipmapMode::NEAREST)
            .address_mode_u(vk::SamplerAddressMode::CLAMP_TO_EDGE)
            .address_mode_v(vk::SamplerAddressMode::CLAMP_TO_EDGE)
            .address_mode_w(vk::SamplerAddressMode::CLAMP_TO_EDGE)
            .max_lod(0.0);
        self.sampler = unsafe { device.create_sampler(&sampler_info, None) }.context(VkSnafu {
            thing: "creating the sampler",
        })?;
        let samplers = vec![self.sampler; self.layers];
        let bindings = [
            vk::DescriptorSetLayoutBinding::default()
                .binding(FRAMES_BINDING)
                .descriptor_type(vk::DescriptorType::COMBINED_IMAGE_SAMPLER)
                .descriptor_count(self.layers as u32)
                .stage_flags(vk::ShaderStageFlags::FRAGMENT)
                .immutable_samplers(&samplers),
            vk::DescriptorSetLayoutBinding::default()
                .binding(UNIFORM_BINDING)
                .descriptor_type(vk::DescriptorType::UNIFORM_BUFFER)
                .descriptor_count(1)
                .stage_flags(vk::ShaderStageFlags::VERTEX | vk::ShaderStageFlags::FRAGMENT),
        ];
        self.set_layout = unsafe {
            device.create_descriptor_set_layout(
                &vk::DescriptorSetLayoutCreateInfo::default().bindings(&bindings),
                None,
            )
        }
        .context(VkSnafu {
            thing: "creating the descriptor set layout",
        })?;
        let sizes = [
            vk::DescriptorPoolSize::default()
                .ty(vk::DescriptorType::COMBINED_IMAGE_SAMPLER)
                .descriptor_count(self.layers as u32),
            vk::DescriptorPoolSize::default()
                .ty(vk::DescriptorType::UNIFORM_BUFFER)
                .descriptor_count(1),
        ];
        self.pool = unsafe {
            device.create_descriptor_pool(
                &vk::DescriptorPoolCreateInfo::default()
                    .max_sets(1)
                    .pool_sizes(&sizes),
                None,
            )
        }
        .context(VkSnafu {
            thing: "creating the descriptor pool",
        })?;
        let set_layouts = [self.set_layout];
        self.set = unsafe {
            device.allocate_descriptor_sets(
                &vk::DescriptorSetAllocateInfo::default()
                    .descriptor_pool(self.pool)
                    .set_layouts(&set_layouts),
            )
        }
        .context(VkSnafu {
            thing: "allocating the descriptor set",
        })?
        .into_iter()
        .next()
        .context(MemorySnafu {
            thing: "descriptor set",
        })?;
        let push_constants = [vk::PushConstantRange::default()
            .stage_flags(vk::ShaderStageFlags::VERTEX | vk::ShaderStageFlags::FRAGMENT)
            .size(size_of::<u32>() as u32)];
        self.layout = unsafe {
            device.create_pipeline_layout(
                &vk::PipelineLayoutCreateInfo::default()
                    .set_layouts(&set_layouts)
                    .push_constant_ranges(&push_constants),
                None,
            )
        }
        .context(VkSnafu {
            thing: "creating the pipeline layout",
        })?;
        Ok(())
    }

    fn create_uniform(&mut self, gpu: &Gpu) -> Result<(), VulkanError> {
        let device = &gpu.device;
        let size = (uniform_floats(self.layers) * size_of::<f32>()) as u64;
        self.uniform = unsafe {
            device.create_buffer(
                &vk::BufferCreateInfo::default()
                    .size(size)
                    .usage(vk::BufferUsageFlags::UNIFORM_BUFFER),
                None,
            )
        }
        .context(VkSnafu {
            thing: "creating the uniform buffer",
        })?;
        let requirements = unsafe { device.get_buffer_memory_requirements(self.uniform) };
        let memory_type = gpu
            .memory_type(
                requirements.memory_type_bits,
                vk::MemoryPropertyFlags::HOST_VISIBLE | vk::MemoryPropertyFlags::HOST_COHERENT,
                vk::MemoryPropertyFlags::empty(),
            )
            .context(MemorySnafu {
                thing: "uniform buffer",
            })?;
        self.uniform_memory = unsafe {
            device.allocate_memory(
                &vk::MemoryAllocateInfo::default()
                    .allocation_size(requirements.size)
                    .memory_type_index(memory_type),
                None,
            )
        }
        .context(VkSnafu {
            thing: "allocating the uniform buffer",
        })?;
        unsafe { device.bind_buffer_memory(self.uniform, self.uniform_memory, 0) }.context(
            VkSnafu {
                thing: "binding the uniform buffer",
            },
        )?;
        let mapped =
            unsafe { device.map_memory(self.uniform_memory, 0, size, vk::MemoryMapFlags::empty()) }
                .context(VkSnafu {
                    thing: "mapping the uniform buffer",
                })?;
        self.mapped = Some(NonNull::new(mapped.cast::<f32>()).context(MemorySnafu {
            thing: "uniform mapping",
        })?);
        let buffer_info = [vk::DescriptorBufferInfo::default()
            .buffer(self.uniform)
            .range(vk::WHOLE_SIZE)];
        let write = vk::WriteDescriptorSet::default()
            .dst_set(self.set)
            .dst_binding(UNIFORM_BINDING)
            .descriptor_type(vk::DescriptorType::UNIFORM_BUFFER)
            .buffer_info(&buffer_info);
        unsafe { device.update_descriptor_sets(&[write], &[]) };
        Ok(())
    }

    pub fn set_format(&mut self, device: &Device, format: vk::Format) -> Result<(), VulkanError> {
        if format == self.format {
            return Ok(());
        }
        let handle = create_pipeline(device, self.layout, self.layers, format)?;
        unsafe { device.destroy_pipeline(self.handle, None) };
        self.handle = handle;
        self.format = format;
        Ok(())
    }

    pub fn bind_frames(&self, device: &Device, views: &[vk::ImageView], layout: vk::ImageLayout) {
        let Some(first) = views.first() else {
            return;
        };
        let images: Vec<vk::DescriptorImageInfo> = (0..self.layers)
            .map(|index| {
                vk::DescriptorImageInfo::default()
                    .image_view(*views.get(index).unwrap_or(first))
                    .image_layout(layout)
            })
            .collect();
        let write = vk::WriteDescriptorSet::default()
            .dst_set(self.set)
            .dst_binding(FRAMES_BINDING)
            .descriptor_type(vk::DescriptorType::COMBINED_IMAGE_SAMPLER)
            .image_info(&images);
        unsafe { device.update_descriptor_sets(&[write], &[]) };
    }

    pub const fn uniforms(&mut self) -> &mut [f32] {
        match self.mapped {
            Some(mapped) => unsafe {
                slice::from_raw_parts_mut(mapped.as_ptr(), uniform_floats(self.layers))
            },
            None => &mut [],
        }
    }

    pub fn destroy(&self, device: &Device) {
        unsafe {
            device.destroy_pipeline(self.handle, None);
            device.destroy_pipeline_layout(self.layout, None);
            device.destroy_descriptor_pool(self.pool, None);
            device.destroy_descriptor_set_layout(self.set_layout, None);
            device.destroy_sampler(self.sampler, None);
            device.destroy_buffer(self.uniform, None);
            device.free_memory(self.uniform_memory, None);
        }
    }
}

const fn uniform_floats(layers: usize) -> usize {
    FLOATS_PER_VEC4 * (layers + 1)
}

fn create_pipeline(
    device: &Device,
    layout: vk::PipelineLayout,
    layers: usize,
    format: vk::Format,
) -> Result<vk::Pipeline, VulkanError> {
    let vertex = shader(device, VERTEX_SHADER)?;
    let fragment = shader(device, FRAGMENT_SHADER);
    let fragment = match fragment {
        Ok(fragment) => fragment,
        Err(error) => {
            unsafe { device.destroy_shader_module(vertex, None) };
            return Err(error);
        }
    };
    let layer_count = (layers as i32).to_ne_bytes();
    let entries = [vk::SpecializationMapEntry::default()
        .constant_id(0)
        .size(layer_count.len())];
    let specialization = vk::SpecializationInfo::default()
        .map_entries(&entries)
        .data(&layer_count);
    let stages = [
        vk::PipelineShaderStageCreateInfo::default()
            .stage(vk::ShaderStageFlags::VERTEX)
            .module(vertex)
            .name(ENTRY_POINT)
            .specialization_info(&specialization),
        vk::PipelineShaderStageCreateInfo::default()
            .stage(vk::ShaderStageFlags::FRAGMENT)
            .module(fragment)
            .name(ENTRY_POINT)
            .specialization_info(&specialization),
    ];
    let vertex_input = vk::PipelineVertexInputStateCreateInfo::default();
    let assembly = vk::PipelineInputAssemblyStateCreateInfo::default()
        .topology(vk::PrimitiveTopology::TRIANGLE_STRIP);
    let viewport = vk::PipelineViewportStateCreateInfo::default()
        .viewport_count(1)
        .scissor_count(1);
    let rasterization = vk::PipelineRasterizationStateCreateInfo::default()
        .polygon_mode(vk::PolygonMode::FILL)
        .cull_mode(vk::CullModeFlags::NONE)
        .line_width(1.0);
    let multisample = vk::PipelineMultisampleStateCreateInfo::default()
        .rasterization_samples(vk::SampleCountFlags::TYPE_1);
    let attachments = [vk::PipelineColorBlendAttachmentState::default()
        .blend_enable(true)
        .src_color_blend_factor(vk::BlendFactor::ONE)
        .dst_color_blend_factor(vk::BlendFactor::ONE_MINUS_SRC_ALPHA)
        .color_blend_op(vk::BlendOp::ADD)
        .src_alpha_blend_factor(vk::BlendFactor::ONE)
        .dst_alpha_blend_factor(vk::BlendFactor::ONE_MINUS_SRC_ALPHA)
        .alpha_blend_op(vk::BlendOp::ADD)
        .color_write_mask(vk::ColorComponentFlags::RGBA)];
    let blend = vk::PipelineColorBlendStateCreateInfo::default().attachments(&attachments);
    let dynamic_states = [vk::DynamicState::VIEWPORT, vk::DynamicState::SCISSOR];
    let dynamic = vk::PipelineDynamicStateCreateInfo::default().dynamic_states(&dynamic_states);
    let formats = [format];
    let mut rendering =
        vk::PipelineRenderingCreateInfo::default().color_attachment_formats(&formats);
    let info = vk::GraphicsPipelineCreateInfo::default()
        .stages(&stages)
        .vertex_input_state(&vertex_input)
        .input_assembly_state(&assembly)
        .viewport_state(&viewport)
        .rasterization_state(&rasterization)
        .multisample_state(&multisample)
        .color_blend_state(&blend)
        .dynamic_state(&dynamic)
        .layout(layout)
        .push_next(&mut rendering);
    let pipelines =
        unsafe { device.create_graphics_pipelines(vk::PipelineCache::null(), &[info], None) };
    unsafe {
        device.destroy_shader_module(vertex, None);
        device.destroy_shader_module(fragment, None);
    }
    pipelines
        .map_err(|(_, result)| result)
        .context(VkSnafu {
            thing: "creating the pipeline",
        })?
        .into_iter()
        .next()
        .context(MemorySnafu { thing: "pipeline" })
}

fn shader(device: &Device, bytes: &[u8]) -> Result<vk::ShaderModule, VulkanError> {
    let code = read_spv(&mut Cursor::new(bytes)).context(ShaderSnafu)?;
    unsafe { device.create_shader_module(&vk::ShaderModuleCreateInfo::default().code(&code), None) }
        .context(VkSnafu {
            thing: "creating a shader module",
        })
}
