// Copyright 2021 System76 <info@system76.com>
// SPDX-License-Identifier: MPL-2.0

mod client;
mod plugins;
mod priority;
mod recent;

pub use client::*;
pub use plugins::config;
pub use plugins::external::load;

use crate::plugins::{
    ExternalPlugin, HelpPlugin, Plugin, PluginConfig, PluginConnector, PluginPriority, PluginQuery,
};
use crate::priority::Priority;
use crate::recent::RecentUseStorage;
use flume::{Receiver, Sender};
use futures::{future, SinkExt, Stream, StreamExt};
use pop_launcher::{
    json_input_stream, plugin_paths, ContextOption, IconSource, Indice, PluginResponse,
    PluginSearchResult, Request, Response, SearchResult,
};
use regex::Regex;
use slab::Slab;
use std::{
    cmp::Ordering,
    collections::{HashMap, HashSet},
    io::{self, Write},
    path::PathBuf,
};

pub type PluginKey = usize;

pub enum Event {
    Request(Request),
    Response((PluginKey, PluginResponse)),
    PluginExit(PluginKey),
    Help(async_oneshot::Sender<Slab<PluginHelp>>),
}

pub struct PluginHelp {
    pub name: String,
    pub description: String,
    pub help: Option<String>,
}

pub fn ensure_cache_path() -> Result<PathBuf, Box<dyn std::error::Error>> {
    let cachepath = dirs::home_dir()
        .ok_or("failed to find home dir")?
        .join(".cache/pop-launcher");
    std::fs::create_dir_all(&cachepath)?;
    Ok(cachepath.join("recent"))
}

pub fn store_cache(storage: &RecentUseStorage) {
    let write_recent = || -> Result<(), Box<dyn std::error::Error>> {
        let cachepath = ensure_cache_path()?;
        Ok(serde_json::to_writer(
            std::fs::File::create(cachepath)?,
            storage,
        )?)
    };
    if let Err(e) = write_recent() {
        eprintln!("could not write to cache file\n{}", e);
    }
}

pub async fn main() {
    let cachepath = ensure_cache_path();
    let read_recent = || -> Result<RecentUseStorage, Box<dyn std::error::Error>> {
        let cachepath = std::fs::File::open(cachepath?)?;
        Ok(serde_json::from_reader(cachepath)?)
    };
    let recent = match read_recent() {
        Ok(r) => r,
        Err(e) => {
            eprintln!("could not read cache file\n{}", e);
            RecentUseStorage::default()
        }
    };

    // Listens for a stream of requests from stdin.
    let input_stream = json_input_stream(tokio::io::stdin()).filter_map(|result| {
        future::ready(match result {
            Ok(request) => Some(request),
            Err(why) => {
                tracing::error!("malformed JSON input: {}", why);
                None
            }
        })
    });

    let (output_tx, output_rx) = flume::bounded(16);

    // Service will operate for as long as it is being awaited
    let service = Service::new(output_tx.into_sink(), recent).exec(input_stream);

    // Responses from the service will be streamed to stdout
    let responder = async move {
        let stdout = io::stdout();
        let stdout = &mut stdout.lock();

        while let Ok(response) = output_rx.recv_async().await {
            serialize_out(stdout, &response);
        }
    };

    futures::future::join(service, responder).await;
}

pub struct Service<O> {
    active_search: Vec<(PluginKey, PluginSearchResult)>,
    associated_list: HashMap<Indice, Indice>,
    awaiting_results: HashSet<PluginKey>,
    last_query: String,
    no_sort: bool,
    output: O,
    plugins: Slab<PluginConnector>,
    search_scheduled: bool,
    recent: RecentUseStorage,
}

impl<O: futures::Sink<Response> + Unpin> Service<O> {
    pub fn new(output: O, recent: RecentUseStorage) -> Self {
        Self {
            active_search: Vec::new(),
            associated_list: HashMap::new(),
            awaiting_results: HashSet::new(),
            last_query: String::new(),
            output,
            no_sort: false,
            plugins: Slab::new(),
            search_scheduled: false,
            recent,
        }
    }

    pub async fn exec(mut self, input: impl Stream<Item = Request>) {
        let (service_tx, service_rx) = flume::bounded(1);
        let stream = plugins::external::load::from_paths();

        futures::pin_mut!(stream);

        while let Some((exec, config, regex)) = stream.next().await {
            tracing::info!("found plugin \"{}\"", exec.display());
            if self
                .plugins
                .iter()
                .any(|(_, p)| p.config.name == config.name)
            {
                tracing::info!("ignoring plugin");
                continue;
            }

            let name = String::from(config.name.as_ref());

            self.register_plugin(service_tx.clone(), config, regex, move |id, tx| {
                ExternalPlugin::new(id, name.clone(), exec.clone(), Vec::new(), tx)
            });
        }

        self.register_plugin(
            service_tx.clone(),
            plugins::help::CONFIG,
            Some(Regex::new(plugins::help::REGEX.as_ref()).expect("failed to compile help regex")),
            HelpPlugin::new,
        );

        let f1 = request_handler(input, service_tx);
        let f2 = self.response_handler(service_rx);

        futures::pin_mut!(f1);
        futures::pin_mut!(f2);

        futures::future::select(f1, f2).await.factor_first();
    }

    async fn response_handler(&mut self, service_rx: Receiver<Event>) {
        while let Ok(event) = service_rx.recv_async().await {
            match event {
                Event::Request(request) => {
                    match request {
                        Request::Search(query) => self.search(query).await,
                        Request::Interrupt => self.interrupt().await,
                        Request::Activate(id) => self.activate(id).await,
                        Request::ActivateContext { id, context } => {
                            self.activate_context(id, context).await;
                        }
                        Request::Complete(id) => self.complete(id).await,
                        Request::Context(id) => self.context(id).await,
                        Request::Quit(id) => self.quit(id).await,

                        // When requested to exit, the service will forward that
                        // request to all of its plugins before exiting itself
                        Request::Exit => {
                            for (_key, plugin) in self.plugins.iter_mut() {
                                let tx = plugin.sender_exec();
                                let _res = tx.send_async(Request::Exit).await;
                            }

                            break;
                        }
                    }
                }

                Event::Response((plugin, response)) => match response {
                    PluginResponse::Append(item) => self.append(plugin, item),
                    PluginResponse::Clear => self.clear(),
                    PluginResponse::Close => self.close().await,
                    PluginResponse::Context { id, options } => {
                        self.context_response(id, options).await;
                    }
                    PluginResponse::Fill(text) => self.fill(text).await,
                    PluginResponse::Finished => self.finished(plugin).await,
                    PluginResponse::DesktopEntry {
                        path,
                        gpu_preference,
                        action_name,
                    } => {
                        self.respond(Response::DesktopEntry {
                            path,
                            gpu_preference,
                            action_name,
                        })
                        .await;
                    }
                    // Report the plugin as finished and remove it from future polling
                    PluginResponse::Deactivate => {
                        self.finished(plugin).await;
                        let _res = self.plugins.remove(plugin);
                    }
                },

                // When a plugin has exited, the sender attached to the plugin will be dropped
                Event::PluginExit(plugin_id) => {
                    if let Some(plugin) = self.plugins.get_mut(plugin_id) {
                        plugin.sender_drop();
                    }
                }

                Event::Help(mut sender) => {
                    let mut details = Slab::new();

                    for (_, plugin) in self.plugins.iter() {
                        details.insert(plugin.details());
                    }

                    let _ = sender.send(details);
                }
            }
        }
    }

    fn register_plugin<P: Plugin, I: Fn(usize, Sender<Event>) -> P + Send + Sync + 'static>(
        &mut self,
        service_tx: Sender<Event>,
        config: PluginConfig,
        regex: Option<regex::Regex>,
        init: I,
    ) {
        let entry = self.plugins.vacant_entry();
        let id = entry.key();

        let init = std::sync::Arc::new(init);

        let isolate_with = config
            .query
            .isolate_with
            .as_ref()
            .and_then(|expr| Regex::new(expr).ok());

        entry.insert(PluginConnector::new(
            config,
            regex,
            isolate_with,
            Box::new(move || {
                let (request_tx, request_rx) = flume::bounded(8);

                let init = init.clone();
                let service_tx = service_tx.clone();
                tokio::spawn(async move {
                    init(id, service_tx).run(request_rx).await;
                });

                request_tx
            }),
        ));
    }

    async fn activate(&mut self, id: Indice) {
        let mut ex = None;
        if let Some((plugin, meta)) = self.search_result(id as usize) {
            ex = meta.cache_identifier();
            let _res = plugin
                .sender_exec()
                .send_async(Request::Activate(meta.id))
                .await;
        }
        if let Some(e) = ex {
            self.recent.add(&e);
            store_cache(&self.recent);
        }
    }

    async fn activate_context(&mut self, id: Indice, context: Indice) {
        let mut ex = None;
        if let Some((plugin, meta)) = self.search_result(id as usize) {
            ex = meta.cache_identifier();
            let _res = plugin
                .sender_exec()
                .send_async(Request::ActivateContext {
                    id: meta.id,
                    context,
                })
                .await;
        }
        if let Some(e) = ex {
            self.recent.add(&e);
            store_cache(&self.recent);
        }
    }

    fn append(&mut self, plugin: PluginKey, append: PluginSearchResult) {
        self.active_search.push((plugin, append));
    }

    fn clear(&mut self) {
        self.active_search.clear();
    }

    async fn close(&mut self) {
        self.respond(Response::Close).await;
    }

    async fn context_response(&mut self, id: Indice, options: Vec<ContextOption>) {
        if let Some(id) = self.associated_list.get(&id) {
            let id = *id;
            self.respond(Response::Context { id, options }).await;
        }
    }

    async fn complete(&mut self, id: Indice) {
        if let Some((plugin, meta)) = self.search_result(id as usize) {
            let _res = plugin
                .sender_exec()
                .send_async(Request::Complete(meta.id))
                .await;
        }
    }

    async fn context(&mut self, id: Indice) {
        if let Some((plugin, meta)) = self.search_result(id as usize) {
            let _res = plugin
                .sender_exec()
                .send_async(Request::Context(meta.id))
                .await;
        }
    }

    async fn fill(&mut self, text: String) {
        self.respond(Response::Fill(text)).await;
    }

    async fn finished(&mut self, plugin: PluginKey) {
        self.awaiting_results.remove(&plugin);
        if !self.awaiting_results.is_empty() {
            return;
        }

        if self.search_scheduled {
            self.search(String::new()).await;
            return;
        }

        let search_list = self.sort();

        self.respond(Response::Update(search_list)).await;
    }

    async fn interrupt(&mut self) {
        for (_, plugin) in self.plugins.iter_mut() {
            if let Some(sender) = plugin.sender.as_mut() {
                let _res = sender.send_async(Request::Interrupt).await;
            }
        }
    }

    async fn quit(&mut self, id: Indice) {
        if let Some((plugin, meta)) = self.search_result(id as usize) {
            let _res = plugin
                .sender_exec()
                .send_async(Request::Quit(meta.id))
                .await;
        }
    }

    async fn respond(&mut self, event: Response) {
        let _res = self.output.send(event).await;
    }

    async fn search(&mut self, query: String) {
        if !self.awaiting_results.is_empty() {
            tracing::debug!("backing off from search until plugins are ready");
            if !self.search_scheduled {
                self.interrupt().await;
                self.search_scheduled = true;
                self.last_query = query;
            }

            return;
        }

        self.active_search.clear();

        if !self.search_scheduled {
            self.last_query = query;
        }

        self.search_scheduled = false;
        let query = self.last_query.as_str();

        let mut query_queue = Vec::new();
        let mut isolated = None;

        let requires_persistence = query.is_empty();

        for (key, plugin) in self.plugins.iter_mut() {
            // Avoid sending queries to plugins which are not matched
            if let Some(regex) = plugin.regex.as_ref() {
                if !regex.is_match(query) {
                    continue;
                }
            }

            if requires_persistence && !plugin.config.query.persistent {
                continue;
            }

            if plugin.config.query.isolate {
                isolated = Some(key);
                break;
            }

            if let Some(regex) = plugin.isolate_regex.as_ref() {
                if regex.is_match(query) {
                    isolated = Some(key);
                    break;
                }
            }

            query_queue.push(key);
        }

        if let Some(isolated) = isolated {
            if let Some(plugin) = self.plugins.get_mut(isolated) {
                if plugin
                    .sender_exec()
                    .send_async(Request::Search(query.to_owned()))
                    .await
                    .is_ok()
                {
                    self.awaiting_results.insert(isolated);
                    self.no_sort = plugin.config.query.no_sort;
                }
            }
        } else {
            for plugin_id in query_queue {
                if let Some(plugin) = self.plugins.get_mut(plugin_id) {
                    if plugin
                        .sender_exec()
                        .send_async(Request::Search(query.to_owned()))
                        .await
                        .is_ok()
                    {
                        self.awaiting_results.insert(plugin_id);
                    }
                }
            }
        }
    }

    /// From a given position ID, fetch the search result and its associated plugin
    fn search_result(
        &mut self,
        id: usize,
    ) -> Option<(&mut PluginConnector, &mut PluginSearchResult)> {
        let &mut Self {
            ref mut active_search,
            ref mut plugins,
            ..
        } = self;

        active_search
            .get_mut(id)
            .and_then(move |(plugin_id, meta)| {
                plugins.get_mut(*plugin_id).map(|plugin| (plugin, meta))
            })
    }

    #[allow(clippy::too_many_lines)]
    fn sort(&mut self) -> Vec<SearchResult> {
        let &mut Self {
            ref mut active_search,
            ref mut associated_list,
            ref mut no_sort,
            ref last_query,
            ref plugins,
            ref recent,
            ..
        } = self;

        let query = &last_query.to_ascii_lowercase();

        if *no_sort {
            *no_sort = false;
        } else {
            active_search.sort_by(|a, b| {
                // Weight is calculated between 0.0 and 1.0, with higher values being most similar
                fn calculate_weight(meta: &PluginSearchResult, query: &str) -> f64 {
                    let mut weight: f64 = 0.0;

                    let name = meta.name.to_ascii_lowercase();
                    let description = meta.description.to_ascii_lowercase();
                    let exec = meta
                        .exec
                        .as_ref()
                        .map(|exec| exec.to_ascii_lowercase())
                        .unwrap_or_default();

                    for name in name.split_ascii_whitespace().flat_map(|x| x.split('_')) {
                        if name.starts_with(query) {
                            return 1.0;
                        }
                    }

                    if exec.contains(query) {
                        if exec.starts_with(query) {
                            return 1.0;
                        }

                        weight = strsim::jaro_winkler(query, &exec) - 0.1;
                    }

                    weight
                        .max(strsim::jaro_winkler(&name, query))
                        .max(strsim::jaro_winkler(&description, query) - 0.1)
                        .max(match meta.keywords.as_ref() {
                            Some(keywords) => keywords
                                .iter()
                                .flat_map(|word| word.split_ascii_whitespace())
                                .fold(0.0, |acc, keyword| {
                                    let keyword = keyword.to_ascii_lowercase();
                                    acc.max(strsim::jaro_winkler(query, &keyword) - 0.1)
                                }),
                            None => 0.0,
                        })
                }

                let plug1 = match plugins.get(a.0) {
                    Some(plug) => plug,
                    None => return Ordering::Greater,
                };

                let plug2 = match plugins.get(b.0) {
                    Some(plug) => plug,
                    None => return Ordering::Less,
                };

                let get_prio = |sr: &PluginSearchResult, plg: &PluginConnector| -> Priority {
                    let ex = sr.cache_identifier();
                    Priority {
                        plugin_priority: plg.config.query.priority,
                        match_score: calculate_weight(sr, query),
                        recent_score: ex.as_ref().map(|s| recent.get_recent(s)).unwrap_or(0.),
                        freq_score: ex.as_ref().map(|s| recent.get_freq(s)).unwrap_or(0.),
                        execlen: sr.name.len(),
                    }
                };

                get_prio(&b.1, plug2).cmp(&get_prio(&a.1, plug1))
            });
        }

        let take = if last_query.starts_with('/') | last_query.starts_with('~') {
            100
        } else {
            8
        };

        let mut windows = Vec::with_capacity(take);
        let mut non_windows = Vec::with_capacity(take);
        associated_list.clear();

        let search_results =
            active_search
                .iter()
                .take(take)
                .enumerate()
                .map(|(id, (plugin, meta))| {
                    associated_list.insert(meta.id, id as u32);
                    SearchResult {
                        id: id as u32,
                        name: meta.name.clone(),
                        description: meta.description.clone(),
                        icon: meta.icon.clone(),
                        category_icon: plugins
                            .get(*plugin)
                            .and_then(|conn| conn.config.icon.clone()),
                        window: meta.window,
                    }
                });

        for result in search_results {
            if result.window.is_some() {
                windows.push(result);
            } else {
                non_windows.push(result);
            }
        }

        windows.append(&mut non_windows);
        windows
    }
}

/// Handles Requests received from a frontend
async fn request_handler(input: impl Stream<Item = Request>, tx: Sender<Event>) {
    let mut requested_to_exit = false;

    futures::pin_mut!(input);

    while let Some(request) = input.next().await {
        if let Request::Exit = request {
            requested_to_exit = true;
        }

        let _res = tx.send_async(Event::Request(request)).await;

        if requested_to_exit {
            break;
        }
    }

    tracing::debug!("no longer listening for requests");
}

/// Serializes the launcher's response to stdout
fn serialize_out<E: serde::Serialize>(output: &mut io::StdoutLock, event: &E) {
    if let Ok(mut vec) = serde_json::to_vec(event) {
        vec.push(b'\n');
        let _res = output.write_all(&vec);
    }
}
