// SPDX-License-Identifier: GPL-3.0-only
// Copyright © 2021 System76

use crate::*;

use freedesktop_desktop_entry as fde;
use freedesktop_desktop_entry::DesktopEntry;
use futures::StreamExt;
use pop_launcher::*;
use std::borrow::Cow;
use tokio::io::AsyncWrite;
use utils::get_description;

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
    desktop_entries: Vec<DesktopEntry<'static>>,
    locales: Vec<String>,
    tx: W,
    gpus: Option<Vec<switcheroo_control::Gpu>>,
}

impl<W: AsyncWrite + Unpin> App<W> {
    fn new(tx: W) -> Self {
        let current_desktop = fde::current_desktop();
        Self {
            current_desktop: fde::current_desktop(),
            is_desktop_cosmic: current_desktop
                .unwrap_or_default()
                .iter()
                .any(|e| e == "cosmic"),
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

        let paths = fde::Iter::new(fde::default_paths());

        let desktop_entries = DesktopEntry::from_paths(paths, &locales)
            .filter_map(|de| {
                de.ok().and_then(|de| {
                    if deduplicator.contains(de.appid.as_ref()) {
                        return None;
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
                    } else {
                        // And also avoid showing anything that's set as `NoDisplay`
                        if de.no_display() {
                            return None;
                        }
                    }
                    deduplicator.insert(de.appid.to_string());
                    Some(de)
                })
            })
            .collect::<Vec<_>>();

        self.desktop_entries = desktop_entries;

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
            let score = fde::matching::get_entry_score(query, entry, &self.locales, &[]);

            if score > 0.6 {
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
        }

        send(&mut self.tx, PluginResponse::Finished).await;
    }

    async fn gnome_context(&self, entry: &DesktopEntry<'_>) -> Vec<ContextOption> {
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

    async fn cosmic_context(&self, entry: &DesktopEntry<'_>) -> Vec<ContextOption> {
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
