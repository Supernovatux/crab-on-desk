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
    collections::BTreeSet,
    ffi::{CStr, CString, c_void},
    fmt::Write as _,
    ptr::NonNull,
};

use ash::{Device, Entry, Instance, ext, khr, vk};
use crab_common::gpu::{GpuChoice, GpuInfo, GpuKind};
use raw_window_handle::{RawDisplayHandle, RawWindowHandle};
use snafu::{OptionExt, ResultExt};

use super::{
    NoDeviceSnafu, UnsupportedSnafu, VkSnafu, VulkanError,
    driver::{self, Driver},
};

const BC7: vk::Format = vk::Format::BC7_UNORM_BLOCK;
const PAGE_SIZE: u64 = 4096;
const MISSING_GPU: &str = "No compatible GPU";

#[derive(Clone, Copy)]
pub enum Platform {
    Wayland(NonNull<c_void>),
    Xcb {
        connection: NonNull<c_void>,
        visual: u32,
    },
}

enum SurfaceLoader {
    Wayland(khr::wayland_surface::Instance),
    Xcb(khr::xcb_surface::Instance),
}

pub struct Context {
    _entry: Entry,
    pub instance: Instance,
    pub surface: khr::surface::Instance,
    pub platform: Platform,
    loader: SurfaceLoader,
    _driver: Option<Driver>,
}

pub struct Candidate {
    pub physical: vk::PhysicalDevice,
    pub family: u32,
    pub info: GpuInfo,
    pub host_image_copy: Option<vk::ImageLayout>,
    pub external_memory_host: bool,
    pub incremental_present: bool,
}

pub struct Gpu {
    pub device: Device,
    pub physical: vk::PhysicalDevice,
    pub queue: vk::Queue,
    pub family: u32,
    pub memory: vk::PhysicalDeviceMemoryProperties,
    pub host_image_copy: Option<(ext::host_image_copy::Device, vk::ImageLayout)>,
    pub external_memory_host: Option<ext::external_memory_host::Device>,
    pub incremental_present: bool,
}

impl Platform {
    pub fn new(display: RawDisplayHandle, visual: Option<u32>) -> Result<Self, VulkanError> {
        match display {
            RawDisplayHandle::Wayland(handle) => Ok(Self::Wayland(handle.display)),
            RawDisplayHandle::Xcb(handle) => Ok(Self::Xcb {
                connection: handle.connection.context(UnsupportedSnafu {
                    thing: "an XCB display without a connection",
                })?,
                visual: visual.unwrap_or_default(),
            }),
            _ => UnsupportedSnafu {
                thing: "this display server",
            }
            .fail(),
        }
    }

    const fn extension(self) -> &'static CStr {
        match self {
            Self::Wayland(_) => khr::wayland_surface::NAME,
            Self::Xcb { .. } => khr::xcb_surface::NAME,
        }
    }

    pub const fn present_modes(self) -> [vk::PresentModeKHR; 2] {
        match self {
            Self::Wayland(_) => [vk::PresentModeKHR::MAILBOX, vk::PresentModeKHR::FIFO],
            Self::Xcb { .. } => [vk::PresentModeKHR::IMMEDIATE, vk::PresentModeKHR::FIFO],
        }
    }

    pub const fn allows_opaque(self) -> bool {
        matches!(self, Self::Xcb { .. })
    }
}

impl Context {
    pub fn open(
        platform: Platform,
        compositor: Option<u64>,
        choice: &GpuChoice,
    ) -> Result<(Self, Candidate), VulkanError> {
        let entry = unsafe { Entry::load() }?;
        let compositor_pci = compositor.and_then(driver::pci_address);
        let choice = match choice {
            GpuChoice::Pci(address) if !driver::pci_exists(address) => {
                eprintln!("{MISSING_GPU} {choice}, using the integrated GPU");
                &GpuChoice::Integrated
            }
            choice => choice,
        };
        let restricted = match choice {
            GpuChoice::Integrated => compositor.and_then(Driver::for_render_node),
            GpuChoice::Pci(address) => Driver::for_pci(address),
            GpuChoice::Discrete | GpuChoice::Uuid(_) => None,
        };
        if let Some(driver) = restricted
            && let Ok(context) = Self::create(entry.clone(), platform, Some(driver))
            && let Ok(mut candidates) = context.candidates(compositor_pci.as_deref())
            && let Some(index) = satisfies(choice, &candidates)
        {
            let candidate = candidates.swap_remove(index);
            return Ok((context, candidate));
        }
        let context = Self::create(entry, platform, None)?;
        let mut candidates = context.candidates(compositor_pci.as_deref())?;
        let index = pick(choice, &candidates).context(NoDeviceSnafu)?;
        let candidate = candidates.swap_remove(index);
        Ok((context, candidate))
    }

    pub fn list(platform: Platform, compositor: Option<u64>) -> Result<Vec<GpuInfo>, VulkanError> {
        let entry = unsafe { Entry::load() }?;
        let context = Self::create(entry, platform, None)?;
        let compositor_pci = compositor.and_then(driver::pci_address);
        Ok(context
            .candidates(compositor_pci.as_deref())?
            .into_iter()
            .map(|candidate| candidate.info)
            .collect())
    }

    fn create(
        entry: Entry,
        platform: Platform,
        driver: Option<Driver>,
    ) -> Result<Self, VulkanError> {
        let application = vk::ApplicationInfo::default().api_version(vk::API_VERSION_1_3);
        let mut extensions = vec![khr::surface::NAME.as_ptr(), platform.extension().as_ptr()];
        let drivers: Vec<vk::DirectDriverLoadingInfoLUNARG<'_>> = driver
            .iter()
            .map(|driver| {
                vk::DirectDriverLoadingInfoLUNARG::default()
                    .pfn_get_instance_proc_addr(Some(driver.entry()))
            })
            .collect();
        let mut direct = vk::DirectDriverLoadingListLUNARG::default()
            .mode(vk::DirectDriverLoadingModeLUNARG::EXCLUSIVE)
            .drivers(&drivers);
        let mut info = vk::InstanceCreateInfo::default().application_info(&application);
        if !drivers.is_empty() {
            extensions.push(vk::LUNARG_DIRECT_DRIVER_LOADING_NAME.as_ptr());
            info = info.push_next(&mut direct);
        }
        let info = info.enabled_extension_names(&extensions);
        let instance = unsafe { entry.create_instance(&info, None) }.context(VkSnafu {
            thing: "creating the instance",
        })?;
        let loader = match platform {
            Platform::Wayland(_) => {
                SurfaceLoader::Wayland(khr::wayland_surface::Instance::new(&entry, &instance))
            }
            Platform::Xcb { .. } => {
                SurfaceLoader::Xcb(khr::xcb_surface::Instance::new(&entry, &instance))
            }
        };
        Ok(Self {
            surface: khr::surface::Instance::new(&entry, &instance),
            _entry: entry,
            instance,
            platform,
            loader,
            _driver: driver,
        })
    }

    pub fn create_surface(&self, window: RawWindowHandle) -> Result<vk::SurfaceKHR, VulkanError> {
        let surface = match (&self.loader, self.platform, window) {
            (
                SurfaceLoader::Wayland(loader),
                Platform::Wayland(display),
                RawWindowHandle::Wayland(handle),
            ) => {
                let info = vk::WaylandSurfaceCreateInfoKHR::default()
                    .display(display.as_ptr().cast())
                    .surface(handle.surface.as_ptr().cast());
                unsafe { loader.create_wayland_surface(&info, None) }
            }
            (
                SurfaceLoader::Xcb(loader),
                Platform::Xcb { connection, .. },
                RawWindowHandle::Xcb(handle),
            ) => {
                let info = vk::XcbSurfaceCreateInfoKHR::default()
                    .connection(connection.as_ptr().cast())
                    .window(handle.window.get());
                unsafe { loader.create_xcb_surface(&info, None) }
            }
            _ => {
                return UnsupportedSnafu {
                    thing: "this window handle",
                }
                .fail();
            }
        };
        surface.context(VkSnafu {
            thing: "creating the surface",
        })
    }

    fn candidates(&self, compositor_pci: Option<&str>) -> Result<Vec<Candidate>, VulkanError> {
        let physicals = unsafe { self.instance.enumerate_physical_devices() }.context(VkSnafu {
            thing: "listing devices",
        })?;
        Ok(physicals
            .into_iter()
            .filter_map(|physical| self.candidate(physical, compositor_pci))
            .collect())
    }

    fn candidate(
        &self,
        physical: vk::PhysicalDevice,
        compositor_pci: Option<&str>,
    ) -> Option<Candidate> {
        let instance = &self.instance;
        let properties = unsafe { instance.get_physical_device_properties(physical) };
        if properties.api_version < vk::API_VERSION_1_3 {
            return None;
        }
        let extensions =
            unsafe { instance.enumerate_device_extension_properties(physical) }.ok()?;
        let extensions: BTreeSet<CString> = extensions
            .iter()
            .filter_map(|extension| extension.extension_name_as_c_str().ok())
            .map(CStr::to_owned)
            .collect();
        let has = |name: &CStr| extensions.contains(name);
        if !has(khr::swapchain::NAME) {
            return None;
        }
        let host_copy_extension = has(ext::host_image_copy::NAME);
        let mut features13 = vk::PhysicalDeviceVulkan13Features::default();
        let mut host_copy = vk::PhysicalDeviceHostImageCopyFeaturesEXT::default();
        let mut features = vk::PhysicalDeviceFeatures2::default().push_next(&mut features13);
        if host_copy_extension {
            features = features.push_next(&mut host_copy);
        }
        unsafe { instance.get_physical_device_features2(physical, &mut features) };
        let core = features.features;
        if core.texture_compression_bc == vk::FALSE
            || core.shader_sampled_image_array_dynamic_indexing == vk::FALSE
            || features13.dynamic_rendering == vk::FALSE
            || features13.synchronization2 == vk::FALSE
        {
            return None;
        }
        let mut formats3 = vk::FormatProperties3::default();
        let mut formats = vk::FormatProperties2::default().push_next(&mut formats3);
        unsafe { instance.get_physical_device_format_properties2(physical, BC7, &mut formats) };
        let sampled = vk::FormatFeatureFlags2::SAMPLED_IMAGE
            | vk::FormatFeatureFlags2::SAMPLED_IMAGE_FILTER_LINEAR;
        if !formats3.optimal_tiling_features.contains(sampled) {
            return None;
        }
        let transfer = formats3
            .optimal_tiling_features
            .contains(vk::FormatFeatureFlags2::TRANSFER_DST);
        let family = self.present_family(physical)?;
        let host_image_copy = (host_copy.host_image_copy == vk::TRUE
            && formats3
                .optimal_tiling_features
                .contains(vk::FormatFeatureFlags2::HOST_IMAGE_TRANSFER_EXT))
        .then(|| self.host_copy_layout(physical))
        .flatten();
        if !transfer && host_image_copy.is_none() {
            return None;
        }
        let host_memory_extension = has(ext::external_memory_host::NAME);
        let (id, driver, import_alignment) = self.identity(
            physical,
            has(ext::pci_bus_info::NAME),
            host_memory_extension,
        );
        let compositor = matches!((&id, compositor_pci), (GpuChoice::Pci(address), Some(compositor)) if address == compositor);
        Some(Candidate {
            physical,
            family,
            info: GpuInfo {
                id,
                name: properties
                    .device_name_as_c_str()
                    .map(|name| name.to_string_lossy().into_owned())
                    .unwrap_or_default(),
                kind: kind(properties.device_type),
                driver,
                compositor,
            },
            host_image_copy,
            external_memory_host: transfer
                && host_memory_extension
                && import_alignment <= PAGE_SIZE,
            incremental_present: has(khr::incremental_present::NAME),
        })
    }

    fn identity(
        &self,
        physical: vk::PhysicalDevice,
        pci_extension: bool,
        host_memory_extension: bool,
    ) -> (GpuChoice, String, u64) {
        let mut driver = vk::PhysicalDeviceDriverProperties::default();
        let mut id = vk::PhysicalDeviceIDProperties::default();
        let mut pci = vk::PhysicalDevicePCIBusInfoPropertiesEXT::default();
        let mut host_memory = vk::PhysicalDeviceExternalMemoryHostPropertiesEXT::default();
        let mut properties = vk::PhysicalDeviceProperties2::default()
            .push_next(&mut driver)
            .push_next(&mut id);
        if pci_extension {
            properties = properties.push_next(&mut pci);
        }
        if host_memory_extension {
            properties = properties.push_next(&mut host_memory);
        }
        unsafe {
            self.instance
                .get_physical_device_properties2(physical, &mut properties);
        }
        let id = if pci_extension {
            GpuChoice::Pci(format!(
                "{:04x}:{:02x}:{:02x}.{:x}",
                pci.pci_domain, pci.pci_bus, pci.pci_device, pci.pci_function
            ))
        } else {
            GpuChoice::Uuid(id.device_uuid.iter().fold(String::new(), |mut hex, byte| {
                let _ = write!(hex, "{byte:02x}");
                hex
            }))
        };
        let driver = driver
            .driver_name_as_c_str()
            .map(|name| name.to_string_lossy().into_owned())
            .unwrap_or_default();
        (id, driver, host_memory.min_imported_host_pointer_alignment)
    }

    fn host_copy_layout(&self, physical: vk::PhysicalDevice) -> Option<vk::ImageLayout> {
        let mut properties = vk::PhysicalDeviceHostImageCopyPropertiesEXT::default();
        let mut properties2 = vk::PhysicalDeviceProperties2::default().push_next(&mut properties);
        unsafe {
            self.instance
                .get_physical_device_properties2(physical, &mut properties2);
        }
        let mut layouts =
            vec![vk::ImageLayout::UNDEFINED; properties.copy_dst_layout_count as usize];
        let mut properties =
            vk::PhysicalDeviceHostImageCopyPropertiesEXT::default().copy_dst_layouts(&mut layouts);
        let mut properties2 = vk::PhysicalDeviceProperties2::default().push_next(&mut properties);
        unsafe {
            self.instance
                .get_physical_device_properties2(physical, &mut properties2);
        }
        [
            vk::ImageLayout::SHADER_READ_ONLY_OPTIMAL,
            vk::ImageLayout::GENERAL,
        ]
        .into_iter()
        .find(|layout| layouts.contains(layout))
    }

    fn present_family(&self, physical: vk::PhysicalDevice) -> Option<u32> {
        let families = unsafe {
            self.instance
                .get_physical_device_queue_family_properties(physical)
        };
        (0..families.len() as u32)
            .zip(families)
            .filter(|(_, family)| family.queue_flags.contains(vk::QueueFlags::GRAPHICS))
            .map(|(index, _)| index)
            .find(|index| self.presents(physical, *index))
    }

    fn presents(&self, physical: vk::PhysicalDevice, family: u32) -> bool {
        match (&self.loader, self.platform) {
            (SurfaceLoader::Wayland(loader), Platform::Wayland(display)) => unsafe {
                loader.get_physical_device_wayland_presentation_support(
                    physical,
                    family,
                    &mut *display.as_ptr().cast(),
                )
            },
            (SurfaceLoader::Xcb(loader), Platform::Xcb { connection, visual }) => unsafe {
                loader.get_physical_device_xcb_presentation_support(
                    physical,
                    family,
                    &mut *connection.as_ptr().cast(),
                    visual,
                )
            },
            _ => false,
        }
    }

    pub fn device(&self, candidate: &Candidate) -> Result<Gpu, VulkanError> {
        let priorities = [1.0];
        let queues = [vk::DeviceQueueCreateInfo::default()
            .queue_family_index(candidate.family)
            .queue_priorities(&priorities)];
        let mut extensions = vec![khr::swapchain::NAME.as_ptr()];
        if candidate.host_image_copy.is_some() {
            extensions.push(ext::host_image_copy::NAME.as_ptr());
        }
        if candidate.external_memory_host {
            extensions.push(ext::external_memory_host::NAME.as_ptr());
        }
        if candidate.incremental_present {
            extensions.push(khr::incremental_present::NAME.as_ptr());
        }
        let features = vk::PhysicalDeviceFeatures::default()
            .texture_compression_bc(true)
            .shader_sampled_image_array_dynamic_indexing(true);
        let mut features13 = vk::PhysicalDeviceVulkan13Features::default()
            .dynamic_rendering(true)
            .synchronization2(true);
        let mut host_copy =
            vk::PhysicalDeviceHostImageCopyFeaturesEXT::default().host_image_copy(true);
        let mut info = vk::DeviceCreateInfo::default()
            .queue_create_infos(&queues)
            .enabled_extension_names(&extensions)
            .enabled_features(&features)
            .push_next(&mut features13);
        if candidate.host_image_copy.is_some() {
            info = info.push_next(&mut host_copy);
        }
        let device = unsafe { self.instance.create_device(candidate.physical, &info, None) }
            .context(VkSnafu {
                thing: "creating the device",
            })?;
        Ok(Gpu {
            queue: unsafe { device.get_device_queue(candidate.family, 0) },
            host_image_copy: candidate.host_image_copy.map(|layout| {
                (
                    ext::host_image_copy::Device::new(&self.instance, &device),
                    layout,
                )
            }),
            external_memory_host: candidate
                .external_memory_host
                .then(|| ext::external_memory_host::Device::new(&self.instance, &device)),
            memory: unsafe {
                self.instance
                    .get_physical_device_memory_properties(candidate.physical)
            },
            physical: candidate.physical,
            family: candidate.family,
            incremental_present: candidate.incremental_present,
            device,
        })
    }
}

impl Drop for Context {
    fn drop(&mut self) {
        unsafe { self.instance.destroy_instance(None) };
    }
}

impl Gpu {
    pub fn memory_type(
        &self,
        bits: u32,
        required: vk::MemoryPropertyFlags,
        avoided: vk::MemoryPropertyFlags,
    ) -> Option<u32> {
        let usable: Vec<(u32, vk::MemoryPropertyFlags)> = (0..u32::BITS)
            .zip(self.memory.memory_types_as_slice())
            .filter(|(index, memory)| {
                bits & (1 << index) != 0 && memory.property_flags.contains(required)
            })
            .map(|(index, memory)| (index, memory.property_flags))
            .collect();
        usable
            .iter()
            .find(|(_, flags)| !flags.intersects(avoided))
            .or_else(|| usable.first())
            .map(|(index, _)| *index)
    }
}

const fn kind(device_type: vk::PhysicalDeviceType) -> GpuKind {
    match device_type {
        vk::PhysicalDeviceType::INTEGRATED_GPU => GpuKind::Integrated,
        vk::PhysicalDeviceType::DISCRETE_GPU => GpuKind::Discrete,
        vk::PhysicalDeviceType::VIRTUAL_GPU => GpuKind::Virtual,
        vk::PhysicalDeviceType::CPU => GpuKind::Cpu,
        _ => GpuKind::Other,
    }
}

fn satisfies(choice: &GpuChoice, candidates: &[Candidate]) -> Option<usize> {
    candidates.iter().position(|candidate| match choice {
        GpuChoice::Integrated => candidate.info.kind == GpuKind::Integrated,
        GpuChoice::Pci(_) => candidate.info.id == *choice,
        GpuChoice::Discrete | GpuChoice::Uuid(_) => false,
    })
}

fn pick(choice: &GpuChoice, candidates: &[Candidate]) -> Option<usize> {
    let of_kind = |wanted: GpuKind| {
        candidates
            .iter()
            .position(|candidate| candidate.info.kind == wanted)
    };
    let integrated = || {
        of_kind(GpuKind::Integrated)
            .or_else(|| {
                candidates
                    .iter()
                    .position(|candidate| candidate.info.compositor)
            })
            .or_else(|| (!candidates.is_empty()).then_some(0))
    };
    match choice {
        GpuChoice::Integrated => integrated(),
        GpuChoice::Discrete => of_kind(GpuKind::Discrete).or_else(integrated),
        GpuChoice::Pci(_) | GpuChoice::Uuid(_) => candidates
            .iter()
            .position(|candidate| candidate.info.id == *choice)
            .or_else(|| {
                eprintln!("{MISSING_GPU} {choice}, using the integrated GPU");
                integrated()
            }),
    }
}
