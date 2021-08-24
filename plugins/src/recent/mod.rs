use futures_lite::prelude::*;
use gtk::prelude::*;
use pop_launcher::*;
use slab::Slab;
use smol::Unblock;
use std::{borrow::Cow, io};

pub struct App {
    manager: gtk::RecentManager,
    out: Unblock<io::Stdout>,
    uris: Slab<String>,
}

impl Default for App {
    fn default() -> Self {
        Self {
            manager: gtk::RecentManager::new(),
            out: async_stdout(),
            uris: Slab::new(),
        }
    }
}

pub async fn main() {
    if gtk::init().is_err() {
        tracing::error!("failed to initialize GTK");
        return;
    }

    let mut requests = json_input_stream(async_stdin());

    let mut app = App::default();

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
        if let Some(query) = normalized(&query) {
            for item in self.manager.items() {
                if let Some(name) = item.display_name() {
                    if name.to_ascii_lowercase().contains(&query) {
                        if let Some((mime, uri)) = item.mime_type().zip(item.uri()) {
                            let id = self.uris.insert(uri.to_string());
                            crate::send(
                                &mut self.out,
                                PluginResponse::Append(PluginSearchResult {
                                    id: id as u32,
                                    name: name.to_string(),
                                    description: item
                                        .uri_display()
                                        .map(String::from)
                                        .unwrap_or_default(),
                                    icon: Some(IconSource::Mime(Cow::Owned(mime.to_string()))),
                                    ..Default::default()
                                }),
                            )
                            .await;
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
