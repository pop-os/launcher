// SPDX-License-Identifier: GPL-3.0-only
// Copyright © 2021 System76

use crate::*;

use freedesktop_desktop_entry as fde;
use freedesktop_desktop_entry::{
    default_paths, get_languages_from_env, DesktopEntry, Iter as DesktopIter, PathSource,
};
use futures::StreamExt;
use pop_launcher::*;
use std::borrow::Cow;
use std::collections::HashSet;
use tokio::io::AsyncWrite;
use utils::path_string;

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
    desktop_entries: Vec<DesktopEntry<'static>>,
    locales: Vec<String>,
    tx: W,
}

impl<W: AsyncWrite + Unpin> App<W> {
    fn new(tx: W) -> Self {
        Self {
            current_desktop: fde::current_desktop(),
            desktop_entries: Vec::new(),
            locales: fde::get_languages_from_env(),
            tx,
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

                    // And also avoid showing anything that's set as `NoDisplay`
                    if de.no_display() {
                        return None;
                    }

                    // Do not show if our desktop is defined in `NotShowIn`.
                    if let Some(not_show_in) = de.not_show_in() {
                        if let Some(current_desktop) = &self.current_desktop {
                            if not_show_in.iter().any(|not_show| {
                                current_desktop.iter().any(|desktop| not_show == desktop)
                            }) {
                                return None;
                            }
                        }
                    }

                    // Do not show if our desktop is not defined in `OnlyShowIn`.
                    if let Some(only_show_in) = de.only_show_in() {
                        if let Some(current_desktop) = &self.current_desktop {
                            if !only_show_in.iter().any(|not_show| {
                                current_desktop.iter().any(|desktop| not_show == desktop)
                            }) {
                                return None;
                            }
                        }
                    }
                    deduplicator.insert(de.appid.to_string());
                    Some(de)
                })
            })
            .collect::<Vec<_>>();

        self.desktop_entries = desktop_entries;
      
    }

    async fn activate(&mut self, id: u32) {
        if let Some(entry) = self.desktop_entries.get(id as usize) {
            let response = PluginResponse::DesktopEntry {
                path: entry.path.clone(),
                gpu_preference: if entry.prefers_non_default_gpu {
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
        if let Some(entry) = self.entries.get(id as usize) {
            let is_cosmic = matches!(current_desktop().as_deref(), Some("cosmic"));
            let gpu_len = self.gpus.as_ref().map(Vec::len).unwrap_or(0) as u32;

            let gpu_preference = if is_cosmic {
                if context < gpu_len {
                    GpuPreference::SpecificIdx(context)
                } else if entry.prefers_non_default_gpu {
                    GpuPreference::NonDefault
                } else {
                    GpuPreference::Default
                }
            } else if !entry.prefers_non_default_gpu {
                GpuPreference::NonDefault
            } else {
                GpuPreference::Default
            };

            let response = PluginResponse::DesktopEntry {
                path: entry.path.clone(),
                gpu_preference,
                action_name: (is_cosmic && context >= gpu_len)
                    .then(|| entry.actions[(context - gpu_len) as usize].clone()),
            };

            send(&mut self.tx, response).await;
        }
    }

    async fn context(&mut self, id: u32) {
        if let Some(entry) = self.entries.get(id as usize) {
            let options = match current_desktop().as_deref() {
                Some("cosmic") => self.cosmic_context(entry).await,
                _ => self.gnome_context(entry).await,
            };

            if !options.is_empty() {
                let response = PluginResponse::Context { id, options };

                send(&mut self.tx, response).await;
            }
        }
    }

    async fn search(&mut self, query: &str) {
        let query = query.to_ascii_lowercase();

        let &mut Self {
            ref entries,
            ref mut tx,
            ..
        } = self;

        let mut items = Vec::with_capacity(16);

        for (id, entry) in entries.iter().enumerate() {
            items.extend(entry.name.split_ascii_whitespace());

            if let Some(keywords) = entry.keywords.as_ref() {
                items.extend(keywords.iter().map(String::as_str));
            }

            items.push(entry.exec.as_str());

            for search_interest in items.drain(..) {
                let search_interest = search_interest.to_ascii_lowercase();
                let append = search_interest.starts_with(&*query)
                    || query
                        .split_ascii_whitespace()
                        .any(|query| search_interest.contains(query))
                    || strsim::jaro_winkler(&query, &search_interest) > 0.6;

                if append {
                    let desc_source = path_string(&entry.src);

                    let response = PluginResponse::Append(PluginSearchResult {
                        id: id as u32,
                        name: entry.name.clone(),
                        description: if entry.description.is_empty() {
                            desc_source.to_string()
                        } else {
                            format!("{} - {}", desc_source, entry.description)
                        },
                        keywords: entry.keywords.clone(),
                        icon: entry.icon.clone().map(Cow::Owned).map(IconSource::Name),
                        exec: Some(entry.exec.clone()),
                        ..Default::default()
                    });

                    send(tx, response).await;

                    break;
                }
            }
        }

        send(tx, PluginResponse::Finished).await;
    }

    async fn gnome_context(&self, entry: &Item) -> Vec<ContextOption> {
        if self.gpus.is_some() {
            vec![ContextOption {
                id: 0,
                name: (if entry.prefers_non_default_gpu {
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

    async fn cosmic_context(&self, entry: &Item) -> Vec<ContextOption> {
        let mut options = Vec::new();

        if let Some(gpus) = self.gpus.as_ref() {
            let default_idx = if entry.prefers_non_default_gpu {
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
        for (i, action) in entry.actions.iter().enumerate() {
            options.push(ContextOption {
                id: (i + options_offset) as u32,
                name: action.clone(),
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
