// Copyright 2022 System76 <info@system76.com>
// SPDX-License-Identifier: GPL-3.0-only

use std::collections::HashSet;
use std::hash::{Hash, Hasher};
use std::io;
use std::ops::Deref;
use std::path::PathBuf;

#[derive(Debug, Default)]
pub struct Gpus {
    devices: Vec<Dev>,
    default: Option<Dev>,
}

impl Gpus {
    // Get gpus via udev
    pub fn load() -> Self {
        let drivers = get_gpus();

        let mut gpus = Gpus::default();
        for dev in drivers.unwrap() {
            if dev.is_default {
                gpus.default = Some(dev)
            } else {
                gpus.devices.push(dev)
            }
        }

        gpus
    }

    /// `true` if there is at least one non default gpu
    pub fn is_switchable(&self) -> bool {
        self.default.is_some() && !self.devices.is_empty()
    }

    /// Return the default gpu
    pub fn get_default(&self) -> Option<&Dev> {
        self.default.as_ref()
    }

    /// Get the first non-default gpu, the current `PreferNonDefaultGpu` specification
    /// Does not tell us which one should be used. Anyway most machine out there should have
    /// only one discrete graphic card.
    /// see: https://gitlab.freedesktop.org/xdg/xdg-specs/-/issues/59
    pub fn non_default(&self) -> Option<&Dev> {
        self.devices.first()
    }
}

#[derive(Debug)]
pub struct Dev {
    id: usize,
    driver: Driver,
    is_default: bool,
    parent_path: PathBuf,
}

impl Dev {
    /// Get the environment variable to launch a program with the correct gpu settings
    pub fn launch_options(&self) -> Vec<(String, String)> {
        let dev_num = self.id.to_string();
        let mut options = vec![];

        match self.driver {
            Driver::Unknown | Driver::Amd(_) | Driver::Intel => {
                options.push(("DRI_PRIME".into(), dev_num))
            }
            Driver::Nvidia => {
                options.push(("__GLX_VENDOR_LIBRARY_NAME".into(), "nvidia".into()));
                options.push(("__NV_PRIME_RENDER_OFFLOAD".into(), "1".into()));
                options.push((" __VK_LAYER_NV_optimus".into(), "NVIDIA_only".into()));
            }
        }

        match self.get_vulkan_icd_paths() {
            Ok(vulkan_icd_paths) if !vulkan_icd_paths.is_empty() => {
                options.push(("VK_ICD_FILENAMES".into(), vulkan_icd_paths.join(":")))
            }
            Err(err) => eprintln!("Failed to open vulkan icd paths: {err}"),
            _ => {}
        }

        options
    }

    // Lookup vulkan icd files and return the ones matching the driver in use
    fn get_vulkan_icd_paths(&self) -> io::Result<Vec<String>> {
        let vulkan_icd_paths = dirs::data_dir()
            .expect("local data dir does not exists")
            .join("vulkan/icd.d");
        let vulkan_icd_paths = &[PathBuf::from("/usr/share/vulkan/icd.d"), vulkan_icd_paths];

        let mut icd_paths = vec![];
        if let Some(driver) = self.driver.as_str() {
            for path in vulkan_icd_paths {
                if path.exists() {
                    for entry in path.read_dir()? {
                        let entry = entry?;
                        let path = entry.path();
                        if path.is_file() {
                            let path_str = path.to_string_lossy();
                            if path_str.contains(driver) {
                                icd_paths.push(path_str.to_string())
                            }
                        }
                    }
                }
            }
        }

        Ok(icd_paths)
    }
}

// Ensure we filter out "render" devices having the same parent as the card
impl Hash for Dev {
    fn hash<H: Hasher>(&self, state: &mut H) {
        state.write(self.parent_path.to_string_lossy().as_bytes());
        state.finish();
    }
}

impl PartialEq<Dev> for Dev {
    fn eq(&self, other: &Self) -> bool {
        self.parent_path == other.parent_path
    }
}

impl Eq for Dev {}

#[derive(Debug)]
enum Driver {
    Intel,
    Amd(String),
    Nvidia,
    Unknown,
}

impl Driver {
    fn from_udev<S: Deref<Target=str>>(driver: Option<S>) -> Driver {
        match driver.as_deref() {
            // For amd devices we need the name of the driver to get vulkan icd files
            Some("radeon") => Driver::Amd("radeon".to_string()),
            Some("amdgpu") => Driver::Amd("amdgpu".to_string()),
            Some("nvidia") => Driver::Nvidia,
            Some("iris") | Some("i915") | Some("i965") => Driver::Intel,
            _ => Driver::Unknown,
        }
    }

    fn as_str(&self) -> Option<&str> {
        match self {
            Driver::Intel => Some("intel"),
            Driver::Amd(driver) => Some(driver.as_str()),
            Driver::Nvidia => Some("nvidia"),
            Driver::Unknown => None,
        }
    }
}

fn get_gpus() -> io::Result<HashSet<Dev>> {
    let mut enumerator = udev::Enumerator::new()?;
    let mut dev_map = HashSet::new();
    let mut drivers: Vec<Dev> = enumerator
        .scan_devices()?
        .into_iter()
        .filter(|dev| {
            dev.devnode()
                .map(|path| path.starts_with("/dev/dri"))
                .unwrap_or(false)
        })
        .filter_map(|dev| {
            dev.parent().and_then(|parent| {
                let id = dev.sysnum();
                let parent_path = parent.syspath().to_path_buf();
                let driver = parent.driver().map(|d| d.to_string_lossy().to_string());
                let driver = Driver::from_udev(driver);

                let is_default = parent
                    .attribute_value("boot_vga")
                    .map(|v| v == "1")
                    .unwrap_or(false);

                id.map(|id| Dev {
                    id,
                    driver,
                    is_default,
                    parent_path,
                })
            })
        })
        .collect();

    // Sort the devices by sysnum so we get card0, card1 first and ignore the other 3D devices
    drivers.sort_by(|a, b| a.id.cmp(&b.id));

    for dev in drivers {
        dev_map.insert(dev);
    }

    Ok(dev_map)
}
