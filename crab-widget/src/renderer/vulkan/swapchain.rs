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

use ash::{khr, vk};
use snafu::{OptionExt, ResultExt, ensure};

use super::{
    UnsupportedSnafu, VkSnafu, VulkanError,
    context::{Context, Gpu},
};

const FORMATS: [vk::Format; 2] = [vk::Format::B8G8R8A8_UNORM, vk::Format::R8G8B8A8_UNORM];
const MIN_IMAGES: u32 = 2;

pub struct Swapchain {
    pub handle: vk::SwapchainKHR,
    pub images: Vec<vk::Image>,
    pub views: Vec<vk::ImageView>,
    pub rendered: Vec<vk::Semaphore>,
    pub commands: Vec<vk::CommandBuffer>,
    pub extent: vk::Extent2D,
    pub format: vk::Format,
}

impl Swapchain {
    pub fn new(
        context: &Context,
        gpu: &Gpu,
        loader: &khr::swapchain::Device,
        surface: vk::SurfaceKHR,
        pool: vk::CommandPool,
        size: (u32, u32),
        old: vk::SwapchainKHR,
    ) -> Result<Self, VulkanError> {
        let physical = gpu.physical;
        let surfaces = &context.surface;
        let supported =
            unsafe { surfaces.get_physical_device_surface_support(physical, gpu.family, surface) }
                .context(VkSnafu {
                    thing: "checking presentation support",
                })?;
        ensure!(
            supported,
            UnsupportedSnafu {
                thing: "presenting to this surface"
            }
        );
        let capabilities =
            unsafe { surfaces.get_physical_device_surface_capabilities(physical, surface) }
                .context(VkSnafu {
                    thing: "reading surface capabilities",
                })?;
        let formats = unsafe { surfaces.get_physical_device_surface_formats(physical, surface) }
            .context(VkSnafu {
                thing: "reading surface formats",
            })?;
        let format = FORMATS
            .into_iter()
            .find_map(|wanted| {
                formats.iter().find(|format| {
                    format.format == wanted
                        && format.color_space == vk::ColorSpaceKHR::SRGB_NONLINEAR
                })
            })
            .context(UnsupportedSnafu {
                thing: "an 8-bit UNORM surface format",
            })?;
        let modes =
            unsafe { surfaces.get_physical_device_surface_present_modes(physical, surface) }
                .context(VkSnafu {
                    thing: "reading present modes",
                })?;
        let mode = context
            .platform
            .present_modes()
            .into_iter()
            .find(|mode| modes.contains(mode))
            .unwrap_or(vk::PresentModeKHR::FIFO);
        let alpha = composite_alpha(context, capabilities.supported_composite_alpha)?;
        let extent = if capabilities.current_extent.width == u32::MAX {
            vk::Extent2D {
                width: size.0.clamp(
                    capabilities.min_image_extent.width,
                    capabilities.max_image_extent.width,
                ),
                height: size.1.clamp(
                    capabilities.min_image_extent.height,
                    capabilities.max_image_extent.height,
                ),
            }
        } else {
            capabilities.current_extent
        };
        let count = capabilities.min_image_count.max(MIN_IMAGES);
        let count = if capabilities.max_image_count == 0 {
            count
        } else {
            count.min(capabilities.max_image_count)
        };
        let info = vk::SwapchainCreateInfoKHR::default()
            .surface(surface)
            .min_image_count(count)
            .image_format(format.format)
            .image_color_space(format.color_space)
            .image_extent(extent)
            .image_array_layers(1)
            .image_usage(vk::ImageUsageFlags::COLOR_ATTACHMENT)
            .image_sharing_mode(vk::SharingMode::EXCLUSIVE)
            .pre_transform(capabilities.current_transform)
            .composite_alpha(alpha)
            .present_mode(mode)
            .clipped(true)
            .old_swapchain(old);
        let handle = unsafe { loader.create_swapchain(&info, None) }.context(VkSnafu {
            thing: "creating the swapchain",
        })?;
        let mut swapchain = Self {
            handle,
            images: Vec::new(),
            views: Vec::new(),
            rendered: Vec::new(),
            commands: Vec::new(),
            extent,
            format: format.format,
        };
        if let Err(error) = swapchain.populate(gpu, loader, pool) {
            swapchain.destroy(gpu, loader, pool);
            return Err(error);
        }
        Ok(swapchain)
    }

    fn populate(
        &mut self,
        gpu: &Gpu,
        loader: &khr::swapchain::Device,
        pool: vk::CommandPool,
    ) -> Result<(), VulkanError> {
        let device = &gpu.device;
        self.images = unsafe { loader.get_swapchain_images(self.handle) }.context(VkSnafu {
            thing: "listing swapchain images",
        })?;
        for image in &self.images {
            let info = vk::ImageViewCreateInfo::default()
                .image(*image)
                .view_type(vk::ImageViewType::TYPE_2D)
                .format(self.format)
                .subresource_range(color_range(1));
            self.views.push(
                unsafe { device.create_image_view(&info, None) }.context(VkSnafu {
                    thing: "creating a swapchain view",
                })?,
            );
            self.rendered.push(
                unsafe { device.create_semaphore(&vk::SemaphoreCreateInfo::default(), None) }
                    .context(VkSnafu {
                        thing: "creating a semaphore",
                    })?,
            );
        }
        self.commands = unsafe {
            device.allocate_command_buffers(
                &vk::CommandBufferAllocateInfo::default()
                    .command_pool(pool)
                    .level(vk::CommandBufferLevel::PRIMARY)
                    .command_buffer_count(self.images.len() as u32),
            )
        }
        .context(VkSnafu {
            thing: "allocating command buffers",
        })?;
        Ok(())
    }

    pub fn destroy(&mut self, gpu: &Gpu, loader: &khr::swapchain::Device, pool: vk::CommandPool) {
        let device = &gpu.device;
        unsafe {
            if !self.commands.is_empty() {
                device.free_command_buffers(pool, &self.commands);
            }
            for semaphore in self.rendered.drain(..) {
                device.destroy_semaphore(semaphore, None);
            }
            for view in self.views.drain(..) {
                device.destroy_image_view(view, None);
            }
            loader.destroy_swapchain(self.handle, None);
        }
        self.commands.clear();
        self.images.clear();
    }
}

pub fn color_range(layers: u32) -> vk::ImageSubresourceRange {
    vk::ImageSubresourceRange::default()
        .aspect_mask(vk::ImageAspectFlags::COLOR)
        .level_count(1)
        .layer_count(layers)
}

fn composite_alpha(
    context: &Context,
    supported: vk::CompositeAlphaFlagsKHR,
) -> Result<vk::CompositeAlphaFlagsKHR, VulkanError> {
    [
        vk::CompositeAlphaFlagsKHR::PRE_MULTIPLIED,
        vk::CompositeAlphaFlagsKHR::INHERIT,
    ]
    .into_iter()
    .chain(
        context
            .platform
            .allows_opaque()
            .then_some(vk::CompositeAlphaFlagsKHR::OPAQUE),
    )
    .find(|alpha| supported.contains(*alpha))
    .context(UnsupportedSnafu {
        thing: "transparent presentation",
    })
}
