// SPDX-License-Identifier: GPL-3.0-only
// Copyright © 2021 System76

use futures::prelude::*;
use pop_launcher::*;
use recently_used_xbel::{parse_file, RecentlyUsed};
use slab::Slab;
use std::borrow::Cow;

pub struct App {
    recent: Option<RecentlyUsed>,
    out: tokio::io::Stdout,
    uris: Slab<String>,
}

impl Default for App {
    fn default() -> Self {
        Self {
            recent: None,
            out: async_stdout(),
            uris: Slab::new(),
        }
    }
}

pub async fn main() {
    let mut requests = json_input_stream(async_stdin());

    let mut app = App::default();

    match parse_file() {
        Ok(recent) => app.recent = Some(recent),
        Err(why) => {
            tracing::error!("failed to parse recently used files: {}", why);
        }
    }

    while let Some(result) = requests.next().await {
        match result {
            Ok(request) => match request {
                Request::Activate(id) => app.activate(id).await,
                Request::Search(query) => app.search(query).await,
                Request::Exit => break,
                _ => (),
            },
            Err(why) => {
                tracing::error!("malformed JSON input: {}", why);
            }
        }
    }
}

impl App {
    async fn activate(&mut self, id: u32) {
        if let Some(uri) = self.uris.get(id as usize) {
            crate::xdg_open(uri);
            crate::send(&mut self.out, PluginResponse::Close).await;
        }
    }

    async fn search(&mut self, query: String) {
        self.uris.clear();
        if let Some((recent, query)) = self.recent.as_ref().zip(normalized(&query)) {
            for item in recent.bookmarks.iter().rev() {
                let display_uri = item.href.replace("%20", " ");

                let name = match display_uri.rfind('/') {
                    Some(pos) => &display_uri[pos + 1..],
                    None => &display_uri,
                };

                if name.to_ascii_lowercase().contains(&query) {
                    if let Some(mime) = new_mime_guess::from_path(&item.href).first() {
                        let id = self.uris.insert(item.href.clone());
                        crate::send(
                            &mut self.out,
                            PluginResponse::Append(PluginSearchResult {
                                id: id as u32,
                                name: name.to_owned(),
                                description: display_uri,
                                icon: Some(IconSource::Mime(Cow::Owned(mime.to_string()))),
                                ..Default::default()
                            }),
                        )
                        .await;

                        if id == 19 {
                            break;
                        }
                    }
                }
            }
        }

        crate::send(&mut self.out, PluginResponse::Finished).await;
    }
}

fn normalized(input: &str) -> Option<String> {
    input
        .find(' ')
        .map(|pos| input[pos + 1..].trim().to_ascii_lowercase())
}
