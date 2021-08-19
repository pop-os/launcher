use futures_lite::prelude::*;
use pop_launcher::*;
use smol::Unblock;
use std::{borrow::Cow, collections::BTreeMap, io, path::PathBuf};

#[derive(Clone)]
struct Item {
    path: PathBuf,
    name: String,
    description: String,
    icon: IconSource,
}

pub async fn main() {
    let mut requests = json_input_stream(async_stdin());

    let mut app = App::default();

    while let Some(result) = requests.next().await {
        match result {
            Ok(request) => match request {
                Request::Activate(id) => app.activate(id).await,
                Request::Complete(id) => app.complete(id).await,
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

pub struct App {
    entries: BTreeMap<PathBuf, Vec<Item>>,
    home: PathBuf,
    out: Unblock<io::Stdout>,
    search_results: Vec<Item>,
}

impl Default for App {
    fn default() -> Self {
        Self {
            entries: BTreeMap::default(),
            home: std::env::home_dir().expect("no home dir"),
            out: async_stdout(),
            search_results: Vec::with_capacity(100),
        }
    }
}

impl App {
    pub async fn activate(&mut self, id: u32) {
        if let Some(selected) = self.search_results.get(id as usize) {
            crate::xdg_open(&selected.path);
            crate::send(&mut self.out, PluginResponse::Close).await;
        }
    }

    pub async fn complete(&mut self, id: u32) {
        if let Some(selected) = self.search_results.get(id as usize) {
            let path = match selected.path.strip_prefix(&self.home) {
                Ok(path) => path,
                Err(_) => &selected.path,
            };

            if let Some(string) = path.to_str() {
                let prefix = if path.is_absolute() { "" } else { "~/" };
                let suffix = if path.is_dir() { "/" } else { "" };
                let fill = [prefix, string, suffix].concat();

                crate::send(&mut self.out, PluginResponse::Fill(fill)).await;
            }
        }
    }

    pub async fn search(&mut self, query: String) {
        let path = if let Some(stripped) = query.strip_prefix("~/") {
            self.home.join(stripped)
        } else {
            PathBuf::from(query)
        };

        let mut show_hidden = false;
        let mut base = String::new();

        if let Some(filename) = path.file_name().and_then(|s| s.to_str()) {
            show_hidden = filename.starts_with('.');
            base = filename.to_ascii_lowercase();
        }

        self.search_results.clear();

        let search_path = if path.is_dir() {
            Some(path.as_path())
        } else if let Some(parent) = path.parent() {
            Some(parent)
        } else {
            None
        };

        if let Some(parent) = search_path {
            let items = self.entries.entry(parent.to_owned()).or_insert_with(|| {
                let mut items = Vec::new();
                if let Ok(dir) = parent.read_dir() {
                    for entry in dir.filter_map(Result::ok) {
                        let path = entry.path();
                        if let Some(name) = path.file_name().and_then(|x| x.to_str()) {
                            items.push(Item {
                                icon: IconSource::Mime(if path.is_dir() {
                                    Cow::Borrowed("inode/directory")
                                } else if let Some(guess) = new_mime_guess::from_path(&path).first()
                                {
                                    Cow::Owned(guess.essence_str().to_owned())
                                } else {
                                    Cow::Borrowed("text/plain")
                                }),
                                name: name.to_owned(),
                                description: path
                                    .metadata()
                                    .ok()
                                    .map(|meta| {
                                        human_format::Formatter::new()
                                            .with_scales(human_format::Scales::Binary())
                                            .with_units("B")
                                            .format(meta.len() as f64)
                                    })
                                    .unwrap_or_else(|| String::from("N/A")),
                                path,
                            });
                        }
                    }
                }

                items
            });

            for item in items {
                if !show_hidden && item.name.starts_with('.') {
                    continue;
                }

                self.search_results.push(item.clone());
            }
        }

        use std::cmp::Ordering;

        self.search_results.sort_by(|a, b| {
            let a_name = a.name.to_ascii_lowercase();
            let b_name = b.name.to_ascii_lowercase();

            let a_contains = a_name.contains(&base);
            let b_contains = b_name.contains(&base);

            if (a_contains && b_contains) || (!a_contains && !b_contains) {
                if a_name.starts_with(&base) {
                    Ordering::Less
                } else if b_name.starts_with(&base) {
                    Ordering::Greater
                } else {
                    human_sort::compare(&a_name, &b_name)
                }
            } else if a_contains {
                Ordering::Less
            } else if b_contains {
                Ordering::Equal
            } else {
                Ordering::Greater
            }
        });

        for (id, selection) in self.search_results.iter().enumerate() {
            crate::send(
                &mut self.out,
                PluginResponse::Append(PluginSearchResult {
                    id: id as u32,
                    name: selection.name.clone(),
                    description: selection.description.clone(),
                    icon: Some(selection.icon.clone()),
                    ..Default::default()
                }),
            )
            .await;

            if id == 19 {
                break;
            }
        }

        crate::send(&mut self.out, PluginResponse::Finished).await;
    }
}
