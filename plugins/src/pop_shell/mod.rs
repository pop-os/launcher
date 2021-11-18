// SPDX-License-Identifier: GPL-3.0-only
// Copyright © 2021 System76

use crate::*;
use freedesktop_desktop_entry as fde;
use futures_lite::{AsyncWrite, AsyncWriteExt, StreamExt};
use pop_launcher::*;
use serde::{Deserialize, Serialize};
use std::{convert::TryFrom, fs, path::PathBuf};
use zbus::Connection;
use zvariant::{Signature, Type};

const DEST: &str = "com.System76.PopShell";
const PATH: &str = "/com/System76/PopShell";

#[derive(Debug, Deserialize)]
struct Item {
    entity: (u32, u32),
    name: String,
    description: String,
    desktop_entry: String,
}

impl Type for Item {
    fn signature() -> Signature<'static> {
        Signature::try_from("((uu)sss)").expect("bad dbus signature")
    }
}

pub async fn main() {
    let connection = match Connection::new_session() {
        Ok(conn) => conn,
        Err(_) => {
            let mut out = async_stdout();
            let _ = crate::send(&mut out, PluginResponse::Deactivate);
            return;
        }
    };

    let mut app = App::new(connection, async_stdout());
    app.reload().await;

    let mut requests = json_input_stream(async_stdin());
    while let Some(request) = requests.next().await {
        match request {
            Ok(request) => match request {
                Request::Activate(id) => app.activate(id).await,
                Request::Quit(_id) => (),
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

struct App<W> {
    desktop_entries: Vec<(fde::PathSource, PathBuf)>,
    entries: Vec<Item>,
    connection: Connection,
    tx: W,
}

impl<W: AsyncWrite + Unpin> App<W> {
    fn new(connection: Connection, tx: W) -> Self {
        Self {
            desktop_entries: fde::Iter::new(fde::default_paths()).collect(),
            entries: Vec::new(),
            connection,
            tx,
        }
    }

    fn call_method<A: Serialize + Type>(
        &mut self,
        method: &str,
        args: &A,
    ) -> zbus::Result<zbus::Message> {
        self.connection
            .call_method(Some(DEST), PATH, Some(DEST), method, args)
    }

    async fn reload(&mut self) {
        if let Ok(message) = self.call_method("WindowList", &()) {
            self.entries = message
                .body::<Vec<Item>>()
                .expect("pop-shell returned invalid WindowList response");
        }
    }

    async fn activate(&mut self, id: u32) {
        if let Some(id) = self.entries.get(id as usize) {
            let entity = id.entity;
            let _ = self.call_method("WindowFocus", &(entity,));
        }
    }

    async fn search(&mut self, query: &str) {
        let query = query.to_ascii_lowercase();
        let haystack = query.split_ascii_whitespace().collect::<Vec<&str>>();

        fn contains_pattern(needle: &str, haystack: &[&str]) -> bool {
            let needle = needle.to_ascii_lowercase();
            haystack.iter().all(|h| needle.contains(h))
        }

        for (id, item) in self.entries.iter().enumerate() {
            let retain = contains_pattern(&item.name, &haystack)
                || contains_pattern(&item.description, &haystack);

            if !retain {
                continue;
            }

            let mut icon_name = Cow::Borrowed("application-x-executable");

            if let Some(desktop_entry) = item.desktop_entry.strip_suffix(".desktop") {
                for (_, path) in &self.desktop_entries {
                    if let Some(name) = path.file_stem() {
                        if desktop_entry == name {
                            if let Ok(data) = fs::read_to_string(path) {
                                if let Ok(entry) = fde::DesktopEntry::decode(path, &data) {
                                    if let Some(icon) = entry.icon() {
                                        icon_name = Cow::Owned(icon.to_owned());
                                    }
                                }
                            }

                            break;
                        }
                    }
                }
            }

            send(
                &mut self.tx,
                PluginResponse::Append(PluginSearchResult {
                    id: id as u32,
                    name: item.name.clone(),
                    description: item.description.clone(),
                    icon: Some(IconSource::Name(icon_name)),
                    window: Some(item.entity),
                    ..Default::default()
                }),
            )
            .await;
        }

        send(&mut self.tx, PluginResponse::Finished).await;
        let _ = self.tx.flush();
    }
}
