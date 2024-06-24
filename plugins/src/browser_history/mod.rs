// SPDX-License-Identifier: GPL-3.0-only
// Copyright © 2024 wiiznokes

use browser_bookmarks::utils::{open_firefox_db_ro, Browser, F64Ord};
use btreemultimap::BTreeMultiMap;
use pop_launcher::*;

use futures::StreamExt;
use pop_launcher::{async_stdin, async_stdout, json_input_stream};

use anyhow::Result;
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
    history: Vec<HistoryEntry>,
}

impl<W: AsyncWrite + Unpin> App<W> {
    fn new(tx: W) -> Self {
        let history = match Browser::get_default_browser() {
            Browser::Unknown => Vec::new(),
            Browser::Firefox => match firefox_history() {
                Ok(history) => history,
                Err(e) => {
                    tracing::error!("{e}");
                    Vec::new()
                }
            },
        };

        Self { tx, history }
    }

    async fn activate(&mut self, id: u32) {
        if let Some(bookmark) = self.history.get(id as usize) {
            crate::xdg_open(&bookmark.url);
        }

        crate::send(&mut self.tx, PluginResponse::Close).await;
    }

    async fn search(&mut self, query: &str) {
        let query = query.strip_prefix("h: ").unwrap_or("");

        if query.is_empty() {
            for (id, h) in self.history.iter().enumerate() {
                send(&mut self.tx, h.map_to_plugin_response(id)).await;
            }
        } else {
            let query = query.to_lowercase();

            let mut tree: BTreeMultiMap<F64Ord, (usize, &HistoryEntry)> = BTreeMultiMap::new();

            for (id, history) in self.history.iter().enumerate() {
                let score = history.match_query(&query);

                if score > 0.6 {
                    tree.insert(F64Ord(score), (id, history));
                }
            }

            for (_, books) in tree {
                for (id, h) in books {
                    send(&mut self.tx, h.map_to_plugin_response(id)).await;
                }
            }
        }

        send(&mut self.tx, PluginResponse::Finished).await;
    }
}

// do not change order!
#[derive(Debug)]
struct HistoryEntry {
    pub url: String,
    pub title: Option<String>,
    pub description: Option<String>,
}

impl HistoryEntry {
    fn match_query(&self, query: &str) -> f64 {
        let mut normalized_values = Vec::new();

        normalized_values.push(self.url.to_lowercase());

        if let Some(title) = &self.title {
            normalized_values.push(title.to_lowercase());
        }
        if let Some(description) = &self.description {
            normalized_values.push(description.to_lowercase());
        }

        normalized_values
            .into_iter()
            .map(|de| textdistance::str::lcsstr(query, &de) as f64 / query.len() as f64)
            .max_by(|e1, e2| e1.total_cmp(e2))
            .unwrap_or(0.0)
    }

    fn map_to_plugin_response(&self, id: usize) -> PluginResponse {
        PluginResponse::Append(PluginSearchResult {
            id: id as u32,
            name: self.url.clone(),
            description: self
                .description
                .as_ref()
                .map(|e| e.to_string())
                .unwrap_or_default(),
            ..Default::default()
        })
    }
}

fn firefox_history() -> Result<Vec<HistoryEntry>> {
    let conn = open_firefox_db_ro()?;

    // on my PC, i have 59875 history entries
    // which takes ~1s in release mode to display the search result.
    // Let's limit it a bit.
    let query_history = r#"
        SELECT p.url, p.title, p.description
        FROM moz_historyvisits AS h
        INNER JOIN moz_places AS p ON h.place_id = p.id
        ORDER BY h.visit_date DESC
        LIMIT 2000;
    "#;

    let mut stmt = conn.prepare(query_history)?;
    let history = stmt
        .query_map([], |row| {
            Ok(HistoryEntry {
                url: row.get(0)?,
                title: row.get(1)?,
                description: row.get(2)?,
            })
        })?
        .filter_map(|e| match e {
            Ok(e) => Some(e),
            Err(e) => {
                tracing::debug!("{e}");
                None
            }
        })
        .collect::<Vec<_>>();

    Ok(history)
}

#[cfg(test)]
mod test {
    use btreemultimap::BTreeMultiMap;

    use crate::{
        browser_bookmarks::utils::F64Ord,
        browser_history::{firefox_history, HistoryEntry},
    };

    #[ignore]
    #[test]
    fn test_history_query() {
        let query = "cosmic-comp";

        let history = firefox_history().unwrap();

        println!("nb: {}", history.len());

        let mut tree: BTreeMultiMap<F64Ord, (usize, &HistoryEntry)> = BTreeMultiMap::new();

        for (id, bookmark) in history.iter().enumerate() {
            println!("{}", bookmark.url);

            let score = bookmark.match_query(query);

            if score > 0.6 {
                tree.insert(F64Ord(score), (id, bookmark));
            }
        }

        println!("tree: {}", tree.len());

        for (score, books) in tree {
            for (_, b) in books {
                println!("{}-----------{}", score.0, b.url);
            }
        }
    }
}
