// Copyright 2021 System76 <info@system76.com>
// SPDX-License-Identifier: MPL-2.0

use crate::PluginConfig;

use futures::{stream, Stream, StreamExt};
use regex::Regex;
use std::path::PathBuf;

/// Fetches plugins installed on the system in parallel.
///
/// Searches plugin paths from highest to least priority. User plugins will override
/// distribution plugins. Plugins are loaded in the order they are found.
pub fn from_paths() -> impl Stream<Item = (PathBuf, PluginConfig, Option<Regex>)> {
    stream::iter(crate::plugin_paths())
        .flat_map(|path| from_path(path.to_path_buf()))
        .map(|(source, config)| {
            smol::unblock(move || crate::plugins::config::load(&source, &config))
        })
        .buffered(num_cpus::get())
        .filter_map(|x| async move { x })
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

                let config = source.join("plugin.ron");
                if !config.exists() {
                    continue;
                }

                z.send((source, config)).await;
            }
        }
    })
}
