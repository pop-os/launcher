// SPDX-License-Identifier: GPL-3.0-only
// Copyright © 2021 System76

mod graphics;

use crate::desktop_entries::graphics::Gpus;
use crate::*;
use freedesktop_desktop_entry::{default_paths, DesktopEntry, Iter as DesktopIter, PathSource};
use futures::StreamExt;
use once_cell::sync::Lazy;
use pop_launcher::*;
use std::borrow::Cow;
use std::hash::{Hash, Hasher};
use std::process::Command;
use tokio::io::AsyncWrite;
use tracing::error;

static GPUS: Lazy<Gpus> = Lazy::new(Gpus::load);

#[derive(Debug, Eq)]
struct Item {
    appid: String,
    description: String,
    context: Vec<ContextAction>,
    prefer_non_default_gpu: bool,
    exec: String,
    icon: Option<String>,
    keywords: Option<Vec<String>>,
    name: String,
    src: PathSource,
}

#[derive(Debug, PartialEq, Eq)]
pub enum ContextAction {
    Action(Action),
    GpuPreference(GpuPreference),
}

impl Item {
    fn run(&self, action_idx: Option<Indice>) {
        match action_idx {
            // No action provided just run the desktop entry with the default gpu
            None => run_exec_command(&self.exec, self.prefer_non_default_gpu),
            // Run the provided action
            Some(idx) => match self.context.get(idx as usize) {
                None => error!("Could not find context action at index {idx}"),
                Some(action) => match action {
                    ContextAction::Action(action) => {
                        run_exec_command(&action.exec, self.prefer_non_default_gpu)
                    }
                    ContextAction::GpuPreference(pref) => match pref {
                        GpuPreference::Default => run_exec_command(&self.exec, false),
                        GpuPreference::NonDefault => run_exec_command(&self.exec, true),
                    },
                },
            },
        }
    }
}

fn run_exec_command(exec: &str, discrete_graphics: bool) {
    let cmd = shell_words::split(exec);
    let cmd: Vec<String> = cmd.unwrap();

    let args = cmd
        .iter()
        // Filter desktop entries field code. Is this needed ?
        // see: https://specifications.freedesktop.org/desktop-entry-spec/desktop-entry-spec-latest.html
        .filter(|arg| !arg.starts_with('%'))
        .collect::<Vec<&String>>();

    let mut cmd = Command::new(&args[0]);
    let mut cmd = cmd.args(&args[1..]);

    let gpu = if discrete_graphics {
        GPUS.non_default()
    } else {
        GPUS.get_default()
    };

    if let Some(gpu) = gpu {
        for (opt, value) in gpu.launch_options() {
            cmd = cmd.env(opt, value);
        }
    }

    if let Err(err) = cmd.spawn() {
        error!("Failed to run desktop entry: {err}");
    }
}

#[derive(Debug, PartialEq, Eq)]
pub struct Action {
    pub name: String,
    pub description: String,
    pub exec: String,
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
                if let Ok(entry) = DesktopEntry::decode(&path, &bytes) {
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

                            let mut actions = vec![];

                            if let Some(entries) = entry.actions() {
                                for action in entries.split(';') {
                                    let action =
                                        entry.action_name(action, locale).and_then(|name| {
                                            entry.action_exec(action).map(|exec| Action {
                                                name: name.to_string(),
                                                description: action.to_string(),
                                                exec: exec.to_string(),
                                            })
                                        });

                                    if let Some(action) = action {
                                        actions.push(action);
                                    }
                                }
                            }

                            let actions = actions
                                .into_iter()
                                .map(|action| ContextAction::Action(action));

                            let is_switchable = GPUS.is_switchable();
                            let entry_prefers_non_default_gpu = entry.prefers_non_default_gpu();
                            let prefer_non_default_gpu =
                                entry_prefers_non_default_gpu && is_switchable;
                            let prefer_default_gpu =
                                !entry_prefers_non_default_gpu && is_switchable;

                            let context: Vec<ContextAction> = if prefer_non_default_gpu {
                                vec![ContextAction::GpuPreference(GpuPreference::Default)]
                            } else if prefer_default_gpu {
                                vec![ContextAction::GpuPreference(GpuPreference::NonDefault)]
                            } else {
                                vec![]
                            }
                            .into_iter()
                            .chain(actions)
                            .collect();

                            let item = Item {
                                appid: entry.appid.to_owned(),
                                name: name.to_string(),
                                description: entry
                                    .comment(locale)
                                    .as_deref()
                                    .unwrap_or("")
                                    .to_owned(),
                                keywords: entry.keywords().map(|keywords| {
                                    keywords.split(';').map(String::from).collect()
                                }),
                                icon: entry.icon().map(|x| x.to_owned()),
                                exec: exec.to_owned(),
                                src,
                                context,
                                prefer_non_default_gpu: entry_prefers_non_default_gpu,
                            };

                            deduplicator.insert(item);
                        }
                    }
                }
            }
        }

        self.entries.extend(deduplicator)
    }

    async fn activate(&mut self, id: u32) {
        send(&mut self.tx, PluginResponse::Close).await;

        if let Some(entry) = self.entries.get(id as usize) {
            entry.run(None);
        } else {
            error!("Desktop entry not found at index {id}");
        }

        std::process::exit(0);
    }

    async fn activate_context(&mut self, id: u32, context: u32) {
        send(&mut self.tx, PluginResponse::Close).await;

        if let Some(entry) = self.entries.get(id as usize) {
            entry.run(Some(context))
        }
    }

    async fn context(&mut self, id: u32) {
        if let Some(entry) = self.entries.get(id as usize) {
            let mut options = Vec::new();

            for (idx, action) in entry.context.iter().enumerate() {
                match action {
                    ContextAction::Action(action) => options.push(ContextOption {
                        id: idx as u32,
                        name: action.name.to_owned(),
                        description: action.description.to_owned(),
                        exec: Some(action.exec.to_string()),
                    }),
                    ContextAction::GpuPreference(pref) => match pref {
                        GpuPreference::Default => options.push(ContextOption {
                            id: 0,
                            name: "Integrated Graphics".to_owned(),
                            description: "Launch Using Integrated Graphics Card".to_owned(),
                            exec: None,
                        }),
                        GpuPreference::NonDefault => options.push(ContextOption {
                            id: 0,
                            name: "Discrete Graphics".to_owned(),
                            description: "Launch Using Discrete Graphics Card".to_owned(),
                            exec: None,
                        }),
                    },
                }
            }

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

fn path_string(source: &PathSource) -> Cow<'static, str> {
    match source {
        PathSource::Local | PathSource::LocalDesktop => "Local".into(),
        PathSource::LocalFlatpak => "Flatpak".into(),
        PathSource::LocalNix => "Nix".into(),
        PathSource::Nix => "Nix (System)".into(),
        PathSource::System => "System".into(),
        PathSource::SystemLocal => "Local (System)".into(),
        PathSource::SystemFlatpak => "Flatpak (System)".into(),
        PathSource::SystemSnap => "Snap (System)".into(),
        PathSource::Other(other) => Cow::Owned(other.clone()),
    }
}
