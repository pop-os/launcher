// SPDX-License-Identifier: GPL-3.0-only
// Copyright © 2021 System76

mod config;

use self::config::{Config, Definition};
use futures_lite::StreamExt;
use pop_launcher::*;

use smol::Unblock;
use std::io;

pub async fn main() {
    let mut app = App::default();

    let mut requests = json_input_stream(async_stdin());

    while let Some(result) = requests.next().await {
        match result {
            Ok(request) => match request {
                Request::Activate(id) => app.activate(id).await,
                Request::Search(query) => app.search(query).await,
                Request::Exit => break,
                _ => (),
            },
            Err(why) => tracing::error!("malformed JSON input: {}", why),
        }
    }
}

pub struct App {
    config: Config,
    queries: Vec<String>,
    out: Unblock<io::Stdout>,
}

impl Default for App {
    fn default() -> Self {
        Self {
            config: config::load(),
            queries: Vec::new(),
            out: async_stdout(),
        }
    }
}

impl App {
    pub async fn activate(&mut self, id: u32) {
        if let Some(query) = self.queries.get(id as usize) {
            eprintln!("got query: {}", query);
            crate::xdg_open(query);
        }

        crate::send(&mut self.out, PluginResponse::Close).await;
    }

    pub async fn search(&mut self, query: String) {
        self.queries.clear();
        if let Some(word) = query.split_ascii_whitespace().next() {
            if let Some(defs) = self.config.get(word) {
                for (id, def) in defs.iter().enumerate() {
                    let (_, mut query) = query.split_at(word.len());
                    query = query.trim();
                    let encoded = build_query(def, query);

                    crate::send(
                        &mut self.out,
                        PluginResponse::Append(PluginSearchResult {
                            id: id as u32,
                            name: [&def.name, ": ", query].concat(),
                            description: encoded.clone(),
                            ..Default::default()
                        }),
                    )
                    .await;

                    self.queries.push(encoded);
                }
            }
        }

        crate::send(&mut self.out, PluginResponse::Finished).await;
    }
}

fn build_query(definition: &Definition, query: &str) -> String {
    let prefix = if definition.query.starts_with("https://") {
        ""
    } else {
        "https://"
    };

    [prefix, &*definition.query, &*urlencoding::encode(query)].concat()
}
