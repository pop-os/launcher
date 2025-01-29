// SPDX-License-Identifier: GPL-3.0-only
// Copyright © 2021 System76

use crate::*;

use freedesktop_desktop_entry as fde;
use freedesktop_desktop_entry::DesktopEntry;
use futures::StreamExt;
use pop_launcher::*;
use std::borrow::Cow;
use tokio::io::AsyncWrite;
use utils::{get_description, is_session_cosmic};

pub(crate) mod utils;

pub async fn main() {
    let mut app = App::new(async_stdout());
    app.reload().await;

    let mut requests = json_input_stream(async_stdin());

    while let Some(result) = requests.next().await {
        match result {
            Ok(request) => match request {
                Request::Activate(id) => app.activate(id).await,
                Request::ActivateContext { id, context } => app.activate_context(id, context).await,
                Request::Context(id) => app.context(id).await,
                Request::Search(query) => app.search(&query).await,
                Request::Exit => break,
                _ => (),
            },

            Err(why) => {
                tracing::error!("malformed JSON request: {}", why);
            }
        }
    }
}

/// Desktop entries to hard exclude.
const EXCLUSIONS: &[&str] = &["GNOME Shell", "Initial Setup"];

struct App<W> {
    current_desktop: Option<Vec<String>>,
    is_desktop_cosmic: bool,
    desktop_entries: Vec<DesktopEntry>,
    locales: Vec<String>,
    tx: W,
    gpus: Option<Vec<switcheroo_control::Gpu>>,
}

impl<W: AsyncWrite + Unpin> App<W> {
    fn new(tx: W) -> Self {
        Self {
            current_desktop: fde::current_desktop(),
            is_desktop_cosmic: is_session_cosmic(),
            desktop_entries: Vec::new(),
            locales: fde::get_languages_from_env(),
            tx,
            gpus: None,
        }
    }

    async fn reload(&mut self) {
        self.desktop_entries.clear();

        let mut deduplicator = std::collections::HashSet::new();
        let locales = fde::get_languages_from_env();

        self.desktop_entries = fde::Iter::new(fde::default_paths())
            .entries(Some(&locales))
            .filter_map(|de| {
                // Treat Flatpak and system apps differently in the cache so they don't
                // override each other
                let appid = de.flatpak().unwrap_or_else(|| de.appid.as_ref());
                if deduplicator.contains(appid) {
                    return None;
                }
                // Always cache already visited entries to allow overriding entries e.g. by
                // placing a modified copy in ~/.local/share/applications/
                deduplicator.insert(appid.to_owned());

                de.name(&self.locales)?;

                match de.exec() {
                    Some(exec) => match exec.split_ascii_whitespace().next() {
                        Some(exec) => {
                            if exec == "false" {
                                return None;
                            }
                        }
                        None => return None,
                    },
                    None => return None,
                }

                // Avoid showing the GNOME Shell entry entirely
                if de
                    .name(&[] as &[&str])
                    .map_or(false, |v| EXCLUSIONS.contains(&v.as_ref()))
                {
                    return None;
                }

                // Do not show if our desktop is defined in `NotShowIn`.
                if let Some(not_show_in) = de.not_show_in() {
                    if let Some(current_desktop) = &self.current_desktop {
                        if not_show_in.iter().any(|not_show| {
                            current_desktop
                                .iter()
                                .any(|desktop| &not_show.to_ascii_lowercase() == desktop)
                        }) {
                            return None;
                        }
                    }
                }

                // Do not show if our desktop is not defined in `OnlyShowIn`.
                if let Some(only_show_in) = de.only_show_in() {
                    if let Some(current_desktop) = &self.current_desktop {
                        if !only_show_in.iter().any(|show_in| {
                            current_desktop
                                .iter()
                                .any(|desktop| &show_in.to_ascii_lowercase() == desktop)
                        }) {
                            return None;
                        }
                    }
                }
                // Treat `OnlyShowIn` as an override otherwise do not show if `NoDisplay` is true
                // Some desktop environments set `OnlyShowIn` and `NoDisplay = true` to
                // indicate special entries
                else if de.no_display() {
                    return None;
                }

                Some(de)
            })
            .collect();

        self.gpus = try_get_gpus().await;
    }

    async fn activate(&mut self, id: u32) {
        if let Some(entry) = self.desktop_entries.get(id as usize) {
            let response = PluginResponse::DesktopEntry {
                path: entry.path.to_path_buf(),
                gpu_preference: if entry.prefers_non_default_gpu() {
                    GpuPreference::NonDefault
                } else {
                    GpuPreference::Default
                },
                action_name: None,
            };

            send(&mut self.tx, response).await;
        }
    }

    async fn activate_context(&mut self, id: u32, context: u32) {
        if let Some(entry) = self.desktop_entries.get(id as usize) {
            let gpu_len = self.gpus.as_ref().map(Vec::len).unwrap_or(0) as u32;

            let gpu_preference = if self.is_desktop_cosmic {
                if context < gpu_len {
                    GpuPreference::SpecificIdx(context)
                } else if entry.prefers_non_default_gpu() {
                    GpuPreference::NonDefault
                } else {
                    GpuPreference::Default
                }
            } else if !entry.prefers_non_default_gpu() {
                GpuPreference::NonDefault
            } else {
                GpuPreference::Default
            };

            let response = PluginResponse::DesktopEntry {
                path: entry.path.to_path_buf(),
                gpu_preference,
                action_name: (self.is_desktop_cosmic && context >= gpu_len).then(|| {
                    entry.actions().unwrap_or_default()[(context - gpu_len) as usize].to_string()
                }),
            };

            send(&mut self.tx, response).await;
        }
    }

    async fn context(&mut self, id: u32) {
        if let Some(entry) = self.desktop_entries.get(id as usize) {
            let options = if self.is_desktop_cosmic {
                self.cosmic_context(entry).await
            } else {
                self.gnome_context(entry).await
            };

            if !options.is_empty() {
                let response = PluginResponse::Context { id, options };

                send(&mut self.tx, response).await;
            }
        }
    }

    async fn search(&mut self, query: &str) {
        for (id, entry) in self.desktop_entries.iter().enumerate() {
            let score = entry.match_query(query, &self.locales, &[]);

            if score < 0.6 {
                continue;
            }
            let response = PluginResponse::Append(PluginSearchResult {
                id: id as u32,
                name: entry.name(&self.locales).unwrap_or_default().to_string(),
                description: get_description(entry, &self.locales),
                keywords: entry
                    .keywords(&self.locales)
                    .map(|v| v.iter().map(|e| e.to_string()).collect()),
                icon: entry
                    .icon()
                    .map(|e| Cow::Owned(e.to_string()))
                    .map(IconSource::Name),
                exec: entry.exec().map(|e| e.to_string()),
                ..Default::default()
            });

            send(&mut self.tx, response).await;
        }

        send(&mut self.tx, PluginResponse::Finished).await;
    }

    async fn gnome_context(&self, entry: &DesktopEntry) -> Vec<ContextOption> {
        if self.gpus.is_some() {
            vec![ContextOption {
                id: 0,
                name: (if entry.prefers_non_default_gpu() {
                    "Launch Using Integrated Graphics Card"
                } else {
                    "Launch Using Discrete Graphics Card"
                })
                .to_owned(),
            }]
        } else {
            Vec::new()
        }
    }

    async fn cosmic_context(&self, entry: &DesktopEntry) -> Vec<ContextOption> {
        let mut options = Vec::new();

        if let Some(gpus) = self.gpus.as_ref() {
            let default_idx = if entry.prefers_non_default_gpu() {
                gpus.iter().position(|gpu| !gpu.default).unwrap_or(0)
            } else {
                gpus.iter().position(|gpu| gpu.default).unwrap_or(0)
            };
            for (i, gpu) in gpus.iter().enumerate() {
                options.push(ContextOption {
                    id: i as u32,
                    name: format!(
                        "Launch using {}{}",
                        gpu.name,
                        if i == default_idx { " (default)" } else { "" }
                    ),
                });
            }
        }

        let options_offset = self.gpus.as_ref().map(|gpus| gpus.len()).unwrap_or(0);
        for (i, action) in entry.actions().unwrap_or_default().iter().enumerate() {
            options.push(ContextOption {
                id: (i + options_offset) as u32,
                name: action.to_string(),
            });
        }

        options
    }
}

async fn try_get_gpus() -> Option<Vec<switcheroo_control::Gpu>> {
    let connection = zbus::Connection::system().await.ok()?;
    let proxy = switcheroo_control::SwitcherooControlProxy::new(&connection)
        .await
        .ok()?;

    if !proxy.has_dual_gpu().await.ok()? {
        return None;
    }

    let gpus = proxy.get_gpus().await.ok()?;
    if gpus.is_empty() {
        return None;
    }
    Some(gpus)
}
