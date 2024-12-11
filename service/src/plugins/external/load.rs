// Copyright 2021 System76 <info@system76.com>
// SPDX-License-Identifier: MPL-2.0

use crate::PluginConfig;

use futures::{stream, Stream, StreamExt};
use std::path::PathBuf;
use tracing::error;

/// Fetches plugins installed on the system in parallel.
///
/// Searches plugin paths from highest to least priority. User plugins will override
/// distribution plugins. Plugins are loaded in the order they are found.
pub fn from_paths() -> impl Stream<Item = PluginConfig> {
    stream::iter(crate::plugin_paths())
        .flat_map(|path| from_path(path.to_path_buf()))
        .map(|(source, config)| {
            tokio::task::spawn_blocking(move || PluginConfig::from_desktop_entry(&source, &config))
        })
        .buffered(num_cpus::get())
        .filter_map(|x| async move {
            match x {
                Ok(plugin) => match plugin {
                    Ok(plugin) => Some(plugin),
                    Err(e) => {
                        error!("{e:?}");
                        None
                    }
                },
                Err(e) => {
                    error!("{e:?}");
                    None
                }
            }
        })
}

/// Loads all plugin information found in the given path.
pub fn from_path(path: PathBuf) -> impl Stream<Item = (PathBuf, PathBuf)> {
    gen_z::gen_z(move |mut z| async move {
        if let Ok(readdir) = path.read_dir() {
            for entry in readdir.filter_map(Result::ok) {
                let source = entry.path();
                if !source.is_dir() {
                    continue;
                }

                let config = source.join("plugin.desktop");
                if !config.exists() {
                    continue;
                }

                z.send((source, config)).await;
            }
        }
    })
}
