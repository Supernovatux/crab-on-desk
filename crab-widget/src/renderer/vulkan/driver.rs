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
    env,
    ffi::{OsString, c_char},
    fs,
    path::{Path, PathBuf},
};

use ash::vk;
use libloading::Library;

const ICD_DIR: &str = "vulkan/icd.d";
const ICD_ENTRY: &[u8] = b"vk_icdGetInstanceProcAddr\0";
const PCI_DEVICES: &str = "/sys/bus/pci/devices";
const DEFAULT_CONFIG_DIRS: &str = "/etc/xdg";
const DEFAULT_DATA_DIRS: &str = "/usr/local/share:/usr/share";
const SYSTEM_CONFIG_DIR: &str = "/etc";
const LOADER_OVERRIDES: [&str; 5] = [
    "VK_DRIVER_FILES",
    "VK_ICD_FILENAMES",
    "VK_ADD_DRIVER_FILES",
    "VK_LOADER_DRIVERS_SELECT",
    "VK_LOADER_DRIVERS_DISABLE",
];
const LIBRARIES: [(&str, &[&str]); 11] = [
    ("amdgpu", &["libvulkan_radeon"]),
    ("i915", &["libvulkan_intel", "libvulkan_intel_hasvk"]),
    ("xe", &["libvulkan_intel"]),
    ("nouveau", &["libvulkan_nouveau"]),
    ("nvidia", &["libGLX_nvidia"]),
    ("virtio_gpu", &["libvulkan_virtio"]),
    ("msm", &["libvulkan_freedreno"]),
    ("panfrost", &["libvulkan_panfrost"]),
    ("panthor", &["libvulkan_panfrost"]),
    ("v3d", &["libvulkan_broadcom"]),
    ("asahi", &["libvulkan_asahi"]),
];

pub type GetInstanceProcAddr =
    unsafe extern "system" fn(vk::Instance, *const c_char) -> vk::PFN_vkVoidFunction;

pub struct Driver {
    entry: GetInstanceProcAddr,
    _library: Library,
}

impl Driver {
    pub fn for_render_node(device: u64) -> Option<Self> {
        Self::for_sysfs(&render_node_device(device))
    }

    pub fn for_pci(address: &str) -> Option<Self> {
        Self::for_sysfs(&Path::new(PCI_DEVICES).join(address))
    }

    pub const fn entry(&self) -> GetInstanceProcAddr {
        self.entry
    }

    fn for_sysfs(device: &Path) -> Option<Self> {
        if LOADER_OVERRIDES
            .iter()
            .any(|name| env::var_os(name).is_some())
        {
            return None;
        }
        let kernel = fs::read_link(device.join("driver")).ok()?;
        let kernel = kernel.file_name()?.to_str()?;
        let (_, libraries) = LIBRARIES.iter().find(|(name, _)| *name == kernel)?;
        manifests()
            .into_iter()
            .find_map(|manifest| load(&manifest, libraries))
    }
}

pub fn pci_exists(address: &str) -> bool {
    Path::new(PCI_DEVICES).join(address).exists()
}

pub fn pci_address(device: u64) -> Option<String> {
    fs::canonicalize(render_node_device(device))
        .ok()?
        .file_name()?
        .to_str()
        .map(str::to_owned)
}

fn render_node_device(device: u64) -> PathBuf {
    PathBuf::from(format!(
        "/sys/dev/char/{}:{}/device",
        rustix::fs::major(device),
        rustix::fs::minor(device)
    ))
}

fn load(manifest: &Path, libraries: &[&str]) -> Option<Driver> {
    let text = fs::read(manifest).ok()?;
    let value: serde_json::Value = serde_json::from_slice(&text).ok()?;
    let library_path = value.get("ICD")?.get("library_path")?.as_str()?;
    let library_path = Path::new(library_path);
    let name = library_path.file_name()?.to_str()?;
    if !libraries.iter().any(|library| {
        name.strip_prefix(library)
            .is_some_and(|rest| rest.starts_with('.'))
    }) {
        return None;
    }
    let library_path = if library_path.is_relative() && library_path.components().count() > 1 {
        manifest.parent()?.join(library_path)
    } else {
        library_path.to_path_buf()
    };
    let library = unsafe { Library::new(&library_path) }.ok()?;
    let entry = *unsafe { library.get::<GetInstanceProcAddr>(ICD_ENTRY) }.ok()?;
    Some(Driver {
        entry,
        _library: library,
    })
}

fn manifests() -> Vec<PathBuf> {
    search_dirs()
        .into_iter()
        .filter_map(|dir| fs::read_dir(dir.join(ICD_DIR)).ok())
        .flat_map(|entries| {
            let mut paths: Vec<PathBuf> = entries
                .filter_map(Result::ok)
                .map(|entry| entry.path())
                .filter(|path| {
                    path.extension()
                        .is_some_and(|extension| extension == "json")
                })
                .collect();
            paths.sort();
            paths
        })
        .collect()
}

fn search_dirs() -> Vec<PathBuf> {
    let home = variable("HOME").map(PathBuf::from);
    let config_home = variable("XDG_CONFIG_HOME")
        .map(PathBuf::from)
        .or_else(|| home.as_ref().map(|home| home.join(".config")));
    let data_home = variable("XDG_DATA_HOME")
        .map(PathBuf::from)
        .or_else(|| home.as_ref().map(|home| home.join(".local/share")));
    let config_dirs = variable("XDG_CONFIG_DIRS").unwrap_or_else(|| DEFAULT_CONFIG_DIRS.into());
    let data_dirs = variable("XDG_DATA_DIRS").unwrap_or_else(|| DEFAULT_DATA_DIRS.into());
    config_home
        .into_iter()
        .chain(env::split_paths(&config_dirs))
        .chain([PathBuf::from(SYSTEM_CONFIG_DIR)])
        .chain(data_home)
        .chain(env::split_paths(&data_dirs))
        .collect()
}

fn variable(name: &str) -> Option<OsString> {
    env::var_os(name).filter(|value| !value.is_empty())
}
