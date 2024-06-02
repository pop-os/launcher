// SPDX-License-Identifier: GPL-3.0-only
// Copyright © 2021 System76

use crate::*;
use freedesktop_desktop_entry::{default_paths, get_languages_from_env, DesktopEntry, Iter as DesktopIter, PathSource};
use futures::StreamExt;
use pop_launcher::*;
use utils::path_string;
use std::borrow::Cow;
use std::hash::{Hash, Hasher};
use std::path::PathBuf;
use tokio::io::AsyncWrite;


pub(crate) mod utils;

#[derive(Debug, Eq)]
struct Item {
    appid: String,
    description: String,
    exec: String,
    icon: Option<String>,
    keywords: Option<Vec<String>>,
    name: String,
    path: PathBuf,
    prefers_non_default_gpu: bool,
    src: PathSource,
    actions: Vec<String>,
}

impl Hash for Item {
    fn hash<H: Hasher>(&self, state: &mut H) {
        self.appid.hash(state);
    }
}

impl PartialEq for Item {
    fn eq(&self, other: &Self) -> bool {
        self.appid == other.appid
    }
}

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
    entries: Vec<Item>,
    locale: Option<String>,
    tx: W,
    gpus: Option<Vec<switcheroo_control::Gpu>>,
}

impl<W: AsyncWrite + Unpin> App<W> {
    fn new(tx: W) -> Self {
        let lang = std::env::var("LANG").ok();

        Self {
            entries: Vec::new(),
            locale: lang
                .as_ref()
                .and_then(|l| l.split('.').next())
                .map(String::from),
            tx,
            gpus: None,
        }
    }

    async fn reload(&mut self) {
        self.entries.clear();

        let locale = self.locale.as_ref().map(String::as_ref);

        let mut deduplicator = std::collections::HashSet::new();

        let current = current_desktop();
        let current = current
            .as_ref()
            .map(|x| x.split(':').collect::<Vec<&str>>());

        for path in DesktopIter::new(default_paths()) {
            let src = PathSource::guess_from(&path);
            if let Ok(bytes) = std::fs::read_to_string(&path) {
                if let Ok(entry) = DesktopEntry::decode_from_str(&path, &bytes, &get_languages_from_env()) {
                    // Do not show if our desktop is defined in `NotShowIn`.
                    if let Some(not_show_in) = entry.desktop_entry("NotShowIn") {
                        let current = ward::ward!(current.as_ref(), else { continue });

                        let matched = not_show_in
                            .to_ascii_lowercase()
                            .split(';')
                            .any(|desktop| current.iter().any(|c| *c == desktop));

                        if matched {
                            continue;
                        }
                    }

                    // Track this condition so that we can override `NoDisplay` if this is true.
                    let mut only_show_in = false;

                    // Do not show if our desktop is not defined in `OnlyShowIn`.
                    if let Some(desktops) = entry.only_show_in() {
                        let current = ward::ward!(current.as_ref(), else { continue });

                        only_show_in = desktops
                            .to_ascii_lowercase()
                            .split(';')
                            .any(|desktop| current.iter().any(|c| *c == desktop));

                        if !only_show_in {
                            continue;
                        }
                    }

                    // Avoid showing the GNOME Shell entry entirely
                    if entry
                        .name(None)
                        .map_or(false, |v| EXCLUSIONS.contains(&v.as_ref()))
                    {
                        continue;
                    }

                    // And also avoid showing anything that's set as `NoDisplay`
                    if !only_show_in && entry.no_display() {
                        continue;
                    }

                    if let Some((name, exec)) = entry.name(locale).zip(entry.exec()) {
                        if let Some(exec) = exec.split_ascii_whitespace().next() {
                            if exec == "false" {
                                continue;
                            }

                            let item = Item {
                                appid: entry.appid.to_string(),
                                name: name.to_string(),
                                description: entry
                                    .comment(locale)
                                    .as_deref()
                                    .unwrap_or("")
                                    .to_owned(),
                                keywords: entry.keywords().map(|keywords| {
                                    keywords.split(';').map(String::from).collect()
                                }),
                                icon: Some(
                                    entry
                                        .icon()
                                        .map(|x| x.to_owned())
                                        .unwrap_or_else(|| "application-x-executable".to_string()),
                                ),
                                exec: exec.to_owned(),
                                path: path.clone(),
                                prefers_non_default_gpu: entry.prefers_non_default_gpu(),
                                src,
                                actions: entry
                                    .actions()
                                    .map(|actions| {
                                        actions
                                            .split(';')
                                            .filter_map(|action| {
                                                entry.action_entry_localized(action, "Name", None)
                                            })
                                            .map(Cow::into_owned)
                                            .collect::<Vec<_>>()
                                    })
                                    .unwrap_or_default(),
                            };

                            deduplicator.insert(item);
                        }
                    }
                }
            }
        }

        self.entries.extend(deduplicator);

        self.gpus = try_get_gpus().await;
    }

    async fn activate(&mut self, id: u32) {
        if let Some(entry) = self.entries.get(id as usize) {
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
                } else {
                    if entry.prefers_non_default_gpu {
                        GpuPreference::NonDefault
                    } else {
                        GpuPreference::Default
                    }
                }
            } else {
                if !entry.prefers_non_default_gpu {
                    GpuPreference::NonDefault
                } else {
                    GpuPreference::Default
                }
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
                        .any(|query| search_interest.contains(&*query))
                    || strsim::jaro_winkler(&*query, &*search_interest) > 0.6;

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
                        (i == default_idx).then_some(" (default)").unwrap_or("")
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

fn current_desktop() -> Option<String> {
    std::env::var("XDG_CURRENT_DESKTOP").ok().map(|x| {
        let x = x.to_ascii_lowercase();
        if x == "unity" {
            "gnome".to_owned()
        } else {
            x
        }
    })
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
