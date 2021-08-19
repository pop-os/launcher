mod plugins;

use crate::plugins::*;
use futures_lite::{future, StreamExt};
use pop_launcher::*;
use postage::mpsc;
use postage::prelude::*;
use regex::Regex;
use slab::Slab;
use std::{
    collections::{HashMap, HashSet},
    io::{self, Write},
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

pub async fn main() {
    let stdout = io::stdout();
    Service::new(stdout.lock()).exec().await
}

pub struct Service<O> {
    active_search: Vec<(PluginKey, PluginSearchResult)>,
    associated_list: HashMap<Indice, Indice>,
    awaiting_results: HashSet<PluginKey>,
    last_query: String,
    output: O,
    plugins: Slab<PluginConnector>,
    no_sort: bool,
    search_scheduled: bool,
}

impl<O: Write> Service<O> {
    pub fn new(output: O) -> Self {
        Self {
            active_search: Vec::new(),
            associated_list: HashMap::new(),
            awaiting_results: HashSet::new(),
            last_query: String::new(),
            output,
            plugins: Slab::new(),
            no_sort: false,
            search_scheduled: false,
        }
    }

    pub async fn exec(mut self) {
        let (service_tx, service_rx) = mpsc::channel(1);

        let stream = plugins::external::load::from_paths();

        futures_lite::pin!(stream);

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
            move |id, tx| HelpPlugin::new(id, tx),
        );

        let f1 = request_handler(service_tx);
        let f2 = self.response_handler(service_rx);

        future::zip(f1, f2).await;
    }

    async fn response_handler(&mut self, mut service_rx: mpsc::Receiver<Event>) {
        while let Some(event) = service_rx.recv().await {
            match event {
                Event::Request(request) => {
                    match request {
                        Request::Search(query) => self.search(query).await,
                        Request::Interrupt => self.interrupt().await,
                        Request::Activate(id) => self.activate(id).await,
                        Request::ActivateContext { id, context } => {
                            self.activate_context(id, context).await
                        }
                        Request::Complete(id) => self.complete(id).await,
                        Request::Context(id) => self.context(id).await,
                        Request::Quit(id) => self.quit(id).await,

                        // When requested to exit, the service will forward that
                        // request to all of its plugins before exiting itself
                        Request::Exit => {
                            for (_key, plugin) in self.plugins.iter_mut() {
                                let tx = plugin.sender_exec();
                                let _ = tx.send(Request::Exit).await;
                            }

                            break;
                        }
                    }
                }

                Event::Response((plugin, response)) => match response {
                    PluginResponse::Append(item) => self.append(plugin, item),
                    PluginResponse::Clear => self.clear(),
                    PluginResponse::Close => self.close(),
                    PluginResponse::Context { id, options } => self.context_response(id, options),
                    PluginResponse::Fill(text) => self.fill(text),
                    PluginResponse::Finished => self.finished(plugin).await,
                    PluginResponse::DesktopEntry {
                        path,
                        gpu_preference,
                    } => {
                        self.respond(&Response::DesktopEntry {
                            path,
                            gpu_preference,
                        });
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

    fn register_plugin<
        P: Plugin,
        I: Fn(usize, mpsc::Sender<Event>) -> P + Send + Sync + 'static,
    >(
        &mut self,
        service_tx: mpsc::Sender<Event>,
        config: PluginConfig,
        regex: Option<regex::Regex>,
        init: I,
    ) {
        let entry = self.plugins.vacant_entry();
        let id = entry.key();

        let init = std::sync::Arc::new(init);

        entry.insert(PluginConnector::new(
            config,
            regex,
            Box::new(move || {
                let (request_tx, request_rx) = mpsc::channel(8);

                let init = init.clone();
                let service_tx = service_tx.clone();
                smol::spawn(async move {
                    init(id, service_tx).run(request_rx).await;
                })
                .detach();

                request_tx
            }),
        ));
    }

    async fn activate(&mut self, id: Indice) {
        if let Some((plugin, meta)) = self.search_result(id as usize) {
            let _ = plugin.sender_exec().send(Request::Activate(meta.id)).await;
        }
    }

    async fn activate_context(&mut self, id: Indice, context: Indice) {
        if let Some((plugin, meta)) = self.search_result(id as usize) {
            let _ = plugin
                .sender_exec()
                .send(Request::ActivateContext {
                    id: meta.id,
                    context,
                })
                .await;
        }
    }

    fn append(&mut self, plugin: PluginKey, append: PluginSearchResult) {
        self.active_search.push((plugin, append));
    }

    fn clear(&mut self) {
        self.active_search.clear();
    }

    fn close(&mut self) {
        self.respond(&Response::Close);
    }

    fn context_response(&mut self, id: Indice, options: Vec<ContextOption>) {
        if let Some(id) = self.associated_list.get(&id) {
            let id = *id;
            self.respond(&Response::Context { id, options });
        }
    }

    async fn complete(&mut self, id: Indice) {
        if let Some((plugin, meta)) = self.search_result(id as usize) {
            let _ = plugin.sender_exec().send(Request::Complete(meta.id)).await;
        }
    }

    async fn context(&mut self, id: Indice) {
        if let Some((plugin, meta)) = self.search_result(id as usize) {
            let _ = plugin.sender_exec().send(Request::Context(meta.id)).await;
        }
    }

    fn fill(&mut self, text: String) {
        self.respond(&Response::Fill(text));
    }

    async fn finished(&mut self, plugin: PluginKey) {
        self.awaiting_results.remove(&plugin);
        if self.awaiting_results.is_empty() {
            if self.search_scheduled {
                self.search(String::new()).await;
                return;
            }

            let search_list = self.sort();
            self.respond(&Response::Update(search_list));
        }
    }

    async fn interrupt(&mut self) {
        for (_, plugin) in self.plugins.iter_mut() {
            if let Some(sender) = plugin.sender.as_mut() {
                let _ = sender.send(Request::Interrupt).await;
            }
        }
    }

    async fn quit(&mut self, id: Indice) {
        if let Some((plugin, meta)) = self.search_result(id as usize) {
            let _ = plugin.sender_exec().send(Request::Quit(meta.id)).await;
        }
    }

    /// Serializes the launcher's response to stdout
    fn respond<E: serde::Serialize>(&mut self, event: &E) {
        if let Ok(mut vec) = serde_json::to_vec(event) {
            vec.push(b'\n');
            let _ = self.output.write_all(&vec);
        }
    }

    async fn search(&mut self, query: String) {
        if !self.awaiting_results.is_empty() {
            tracing::debug!("backing off from search until plugins are ready");
            if !self.search_scheduled {
                self.interrupt().await;
                self.search_scheduled = true;
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

            query_queue.push(key);
        }

        if let Some(isolated) = isolated {
            if let Some(plugin) = self.plugins.get_mut(isolated) {
                if plugin
                    .sender_exec()
                    .send(Request::Search(query.to_owned()))
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
                        .send(Request::Search(query.to_owned()))
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

    fn sort(&mut self) -> Vec<SearchResult> {
        let &mut Self {
            ref mut active_search,
            ref mut associated_list,
            ref mut no_sort,
            ref last_query,
            ref plugins,
            ..
        } = self;

        let query = &last_query.to_ascii_lowercase();

        use std::cmp::Ordering;

        if *no_sort {
            *no_sort = false;
        } else {
            active_search.sort_by(|a, b| {
                fn calculate_weight(meta: &PluginSearchResult, query: &str) -> usize {
                    let mut weight = 0;

                    let name = meta.name.to_ascii_lowercase();
                    let description = meta.description.to_ascii_lowercase();
                    let exec = meta
                        .exec
                        .as_ref()
                        .map(|exec| exec.to_ascii_lowercase())
                        .unwrap_or_default();

                    if !name.starts_with(query) {
                        weight = 1;

                        if !name.contains(query) {
                            weight = strsim::damerau_levenshtein(&name, query)
                                .min(strsim::damerau_levenshtein(&description, query));

                            if let Some(keywords) = meta.keywords.as_ref() {
                                for keyword in keywords.iter() {
                                    let keyword = keyword.to_ascii_lowercase();
                                    weight = if keyword.starts_with(query)
                                        || keyword.contains(query)
                                    {
                                        1
                                    } else {
                                        weight.min(strsim::damerau_levenshtein(query, &keyword) + 1)
                                    }
                                }
                            }
                        }
                    }

                    if exec.contains(query) {
                        weight = if exec.starts_with(query) {
                            weight.min(2)
                        } else {
                            weight.min(strsim::damerau_levenshtein(query, &exec))
                        }
                    }

                    weight
                }

                let a_weight = calculate_weight(&a.1, query);
                let b_weight = calculate_weight(&b.1, query);

                match a_weight.cmp(&b_weight) {
                    Ordering::Equal => {
                        let a_len = a.1.name.len();
                        let b_len = b.1.name.len();

                        a_len.cmp(&b_len)
                    }
                    other => other,
                }
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
                non_windows.push(result)
            }
        }

        windows.append(&mut non_windows);
        windows
    }
}

/// Handles Requests received from a frontend
async fn request_handler(mut tx: mpsc::Sender<Event>) {
    let mut requested_to_exit = false;
    let mut request_stream = json_input_stream(async_stdin());

    while let Some(result) = request_stream.next().await {
        match result {
            Ok(request) => {
                if let Request::Exit = request {
                    requested_to_exit = true
                }

                let _ = tx.send(Event::Request(request)).await;

                if requested_to_exit {
                    break;
                }
            }

            Err(why) => {
                tracing::error!("Request JSON is malformed: {}", why);
            }
        }
    }

    tracing::debug!("no longer listening for requests")
}
