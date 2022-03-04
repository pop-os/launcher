// Copyright 2022 System76 <info@system76.com>
// SPDX-License-Identifier: GPL-3.0-only

use anyhow::Context;
use sysfs_class::{PciDevice, SysClass};

/// Checks if the system has switchable graphics.
///
/// A system is considered switchable if multiple graphics card devices are found.
pub fn is_switchable() -> bool {
    let main = || -> anyhow::Result<bool> {
        let devices = PciDevice::all().context("cannot get PCI devices")?;

        let mut amd_graphics = 0;
        let mut intel_graphics = 0;
        let mut nvidia_graphics = 0;

        for dev in devices {
            let c = dev.class().context("cannot get class of device")?;
            if let 0x03 = (c >> 16) & 0xFF {
                match dev.vendor().context("cannot get vendor of device")? {
                    0x1002 => amd_graphics += 1,
                    0x10DE => nvidia_graphics += 1,
                    0x8086 => intel_graphics += 1,
                    _ => (),
                }
            }
        }

        let switchable = (nvidia_graphics > 0 && (intel_graphics > 0 || amd_graphics > 0))
            || (intel_graphics > 0 && amd_graphics > 0);

        Ok(switchable)
    };

    match main() {
        Ok(value) => value,
        Err(why) => {
            tracing::error!("{}", why);
            false
        }
    }
}
