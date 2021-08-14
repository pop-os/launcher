use crate::PluginConfig;

use flume::Sender;
use futures_lite::{Stream, StreamExt};
use regex::Regex;
use std::path::{Path, PathBuf};

/// Fetches plugins installed on the system in parallel.
pub async fn from_paths(tx: Sender<(PathBuf, PluginConfig, Option<Regex>)>) {
    const PLUGIN_PATHS: &[&str] = &[
        // User plugins
        ".local/share/pop-launcher/plugins/",
        // System plugins configured by admin
        "/etc/pop-launcher/plugins/",
        // Distribution plugins
        "/usr/lib/pop-launcher/plugins/",
    ];

    let mut futures = Vec::new();

    // Searches plugin paths from highest to least priority.
    // User plugins will override distribution plugins.
    for path in PLUGIN_PATHS {
        let path_buf;
        #[allow(deprecated)]
        let path = if !path.starts_with('/') {
            path_buf = std::env::home_dir()
                .expect("user does not have home dir")
                .join(path);
            path_buf.as_path()
        } else {
            Path::new(&path)
        };

        let loadable_plugins = from_path(path);
        futures_lite::pin!(loadable_plugins);

        // Spawn a background task to parse the config for each plugin found.
        while let Some((source, config)) = loadable_plugins.next().await {
            let tx = tx.clone();
            let future = smol::unblock(move || {
                if let Some(plugin) = crate::plugins::config::load(&source, &config) {
                    let _ = tx.send(plugin);
                }
            });

            futures.push(smol::spawn(future))
        }

        // Ensures that plugins are loaded in the order that they were spawned.
        for future in futures.drain(..) {
            future.await;
        }
    }
}

/// Loads all plugin information found in the given path.
pub fn from_path(path: &Path) -> impl Stream<Item = (PathBuf, PathBuf)> + '_ {
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
