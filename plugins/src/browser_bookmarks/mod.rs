// SPDX-License-Identifier: GPL-3.0-only
// Copyright © 2021 System76

use pop_launcher::*;

use std::{fs, path::PathBuf};

use futures::StreamExt;
use pop_launcher::{async_stdin, async_stdout, json_input_stream};
use rusqlite::Connection;

use anyhow::{bail, Result};
use tokio::io::AsyncWrite;

use crate::*;

pub async fn main() {
    let mut app = App::new(async_stdout());

    let mut requests = json_input_stream(async_stdin());

    while let Some(result) = requests.next().await {
        match result {
            Ok(request) => match request {
                Request::Activate(id) => app.activate(id).await,
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
    tx: W,
    bookmarks: Vec<Bookmark>,
}

impl<W: AsyncWrite + Unpin> App<W> {
    fn new(tx: W) -> Self {
        let bookmarks = match bookmarks() {
            Ok(bookmarks) => bookmarks,
            Err(e) => {
                tracing::error!("{e}");
                Vec::new()
            }
        };

        Self { tx, bookmarks }
    }

    async fn activate(&mut self, id: u32) {
        if let Some(bookmark) = self.bookmarks.get(id as usize) {
            crate::xdg_open(&bookmark.url);
        }

        crate::send(&mut self.tx, PluginResponse::Close).await;
    }

    async fn search(&mut self, query: &str) {
        let query = query.to_lowercase();

        for (id, bookmark) in self.bookmarks.iter().enumerate() {
            if query.is_empty() || bookmark.match_query(&query) > 0.6 {
                let response = PluginResponse::Append(PluginSearchResult {
                    id: id as u32,
                    name: bookmark
                        .bookmark_name
                        .as_ref()
                        .map(|e| e.to_string())
                        .unwrap_or_default(),
                    description: bookmark
                        .description
                        .as_ref()
                        .map(|e| e.to_string())
                        .unwrap_or_default(),
                    ..Default::default()
                });

                send(&mut self.tx, response).await;
            }
        }

        send(&mut self.tx, PluginResponse::Finished).await;
    }
}

fn firefox_db_path() -> Result<PathBuf> {
    let home = std::env::var("HOME")?;

    let mut home = PathBuf::from(home);

    home.push(".mozilla");
    home.push("firefox");

    if !home.is_dir() {
        bail!("no firefox directory detected")
    }

    if let Ok(entries) = fs::read_dir(home) {
        for entry in entries.flatten() {
            let file_name = entry.path();
            if let Some(name) = file_name.to_str() {
                if name.ends_with(".default-release") {
                    return Ok(file_name.join("places.sqlite"));
                }
            }
        }
    }

    bail!("no db found")
}

fn open_db() -> Result<Connection> {
    let firefox_db_path = firefox_db_path()?;

    let tmp_db_path = "/tmp/places_backup.sqlite";

    fs::copy(firefox_db_path, tmp_db_path)?;

    let conn =
        Connection::open_with_flags(tmp_db_path, rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY)?;

    Ok(conn)
}

// do not change order!
#[derive(Debug)]
struct Bookmark {
    pub bookmark_name: Option<String>,
    pub _last_modified_date: Option<u64>,
    pub url: String,
    pub title: Option<String>,
    pub _last_visite_date: Option<u64>,
    pub description: Option<String>,
    pub _preview_image_url: Option<String>,
}

impl Bookmark {
    fn match_query(&self, query: &str) -> f64 {
        let mut normalized_values = Vec::new();

        if let Some(bookmark_name) = &self.bookmark_name {
            normalized_values.push(bookmark_name.to_lowercase());
        }

        normalized_values.push(self.url.to_lowercase());

        if let Some(title) = &self.title {
            normalized_values.push(title.to_lowercase());
        }
        if let Some(description) = &self.description {
            normalized_values.push(description.to_lowercase());
        }

        normalized_values
            .into_iter()
            .map(|de| strsim::jaro_winkler(query, &de))
            .max_by(|e1, e2| e1.total_cmp(e2))
            .unwrap_or(0.0)
    }
}

fn bookmarks() -> Result<Vec<Bookmark>> {
    let conn = open_db()?;

    eprintln!("connected");

    let query = r#"
        SELECT b.title, b.lastModified, p.url, p.title, p.last_visit_date, p.description, p.preview_image_url
        FROM moz_bookmarks AS b
        JOIN moz_places AS p ON b.fk = p.id;
    "#;

    let mut stmt = conn.prepare(query)?;
    let bookmarks = stmt
        .query_map([], |row| {
            Ok(Bookmark {
                bookmark_name: row.get(0)?,
                _last_modified_date: row.get(1)?,
                url: row.get(2)?,
                title: row.get(3)?,
                _last_visite_date: row.get(4)?,
                description: row.get(5)?,
                _preview_image_url: row.get(6)?,
            })
        })?
        .filter_map(|e| match e {
            Ok(e) => Some(e),
            Err(e) => {
                dbg!(e);
                None
            }
        })
        .collect::<Vec<_>>();

    Ok(bookmarks)
}
