// SPDX-License-Identifier: GPL-3.0-only
// Copyright © 2021 System76

use std::borrow::Cow;
use std::fs::OpenOptions;
use std::io;
use std::path::PathBuf;
use std::time::Duration;

use futures_lite::StreamExt;
use isahc::config::{Configurable, RedirectPolicy};
use isahc::{Body, Error, HttpClient, HttpClientBuilder, ReadResponseExt};
use smol::Unblock;
use url::Url;

use pop_launcher::*;

use crate::mime_from_path;

use self::config::{Config, Definition};
use isahc::http::header::CONTENT_TYPE;
use smol::io::AsyncReadExt;

mod config;

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
    client: HttpClient,
    cache: PathBuf,
}

const ALLOWED_FAVICON_MIME: [&str; 5] = [
    "image/vnd.microsoft.icon",
    "image/png",
    "image/gif",
    "image/svg+xml",
    "image/x-icon",
];

impl Default for App {
    fn default() -> Self {
        let cache = std::env::home_dir()
            .map(|cache| cache.join(".cache/pop-launcher"))
            .expect("no home dir");

        if !cache.exists() {
            std::fs::create_dir(&cache).expect("unable to create $HOME/.cache/pop-launcher")
        }

        Self {
            config: config::load(),
            queries: Vec::new(),
            out: async_stdout(),
            client: HttpClient::builder()
                .redirect_policy(RedirectPolicy::Follow)
                .timeout(Duration::from_secs(1))
                .build()
                .expect("failed to create http client"),
            cache,
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
                    let icon = self.get_favicon(&def.name, &encoded).await;

                    crate::send(
                        &mut self.out,
                        PluginResponse::Append(PluginSearchResult {
                            id: id as u32,
                            name: [&def.name, ": ", query].concat(),
                            description: encoded.clone(),
                            icon,
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

impl App {
    async fn get_favicon(&self, rule_name: &str, url: &str) -> Option<IconSource> {
        let url = Url::parse(url).expect("invalid url");

        let favicon_path = self.cache.join(format!("{}.ico", rule_name));

        if favicon_path.exists() {
            let favicon_path = favicon_path.to_string_lossy().into_owned();
            Some(IconSource::Name(Cow::Owned(favicon_path)))
        } else {
            self.fetch_icon_in_background(rule_name, url, &favicon_path)
                .await;
            None
        }
    }

    async fn fetch_icon_in_background(&self, rule_name: &str, url: Url, favicon_path: &PathBuf) {
        let client = self.client.clone();
        let rule_name = rule_name.to_string();
        let domain = url
            .domain()
            .map(|domain| domain.to_string())
            .expect("url have no domain");
        let favicon_path = favicon_path.clone();

        smol::spawn(async move {
            let response = client
                .get_async(format!(
                    "https://www.google.com/s2/favicons?domain={}&sz=32",
                    domain
                ))
                .await;

            match response {
                Err(err) => {
                    tracing::error!("error fetching favicon for {}: {}", rule_name, err);
                }
                Ok(mut response) => {
                    let content_type = response
                        .headers()
                        .get(CONTENT_TYPE)
                        .map(|header| header.to_str().ok())
                        .flatten()
                        .unwrap();

                    if !ALLOWED_FAVICON_MIME.contains(&content_type) {
                        tracing::error!(
                            "Got unexpected content-type '{}' type for {:?} favicon",
                            content_type,
                            favicon_path
                        );
                    };

                    let mut contents = vec![];
                    response.body_mut().read_to_end(&mut contents).await;
                    let copy = smol::fs::write(&favicon_path, contents).await;

                    if let Err(err) = copy {
                        tracing::error!("error writing favicon to {:?}: {}", &favicon_path, err);
                    }
                }
            }
        })
        .detach();
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
