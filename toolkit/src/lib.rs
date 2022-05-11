// Copyright 2021 System76 <info@system76.com>
// SPDX-License-Identifier: MPL-2.0

pub use pop_launcher_service::{
    self as service,
    load::from_path as load_plugin_from_path,
    load::from_paths as load_plugins_from_paths
};
pub use pop_launcher_plugins as plugins;
pub use pop_launcher as launcher;
