use freedesktop_desktop_entry::{default_paths, DesktopEntry, Iter as DesktopIter, PathSource};
use futures_lite::{AsyncWrite, StreamExt};
use pop_launcher::*;
use pop_launcher_plugins::*;
use std::borrow::Cow;
use std::hash::{Hash, Hasher};
use std::path::PathBuf;

#[derive(Debug, Eq)]
struct Item {
    appid: String,
    description: String,
    exec: String,
    icon: Option<String>,
    keywords: Option<Vec<String>>,
    name: String,
    path: PathBuf,
    prefers_non_default_gpu: bool,
    src: PathSource,
    terminal_command: bool,
}

impl Hash for Item {
    fn hash<H: Hasher>(&self, state: &mut H) {
        self.name.hash(state);
        self.src.hash(state);
    }
}

impl PartialEq for Item {
    fn eq(&self, other: &Self) -> bool {
        self.name == other.name && self.src == other.src
    }
}

pub async fn main() {
    let mut app = DesktopEntryPlugin::new(async_stdout());
    app.reload().await;

    let mut requests = json_input_stream(async_stdin());

    while let Some(result) = requests.next().await {
        match result {
            Ok(request) => {
                tracing::debug!("received request: {:?}", request);
                match request {
                    Request::Activate(id) => app.activate(id).await,
                    Request::Search(query) => app.search(&query).await,
                    Request::Exit => break,
                    _ => (),
                }
            }

            Err(why) => {
                tracing::error!("malformed JSON request: {}", why);
            }
        }
    }
}

struct DesktopEntryPlugin<W> {
    entries: Vec<Item>,
    locale: Option<String>,
    tx: W,
}

impl<W: AsyncWrite + Unpin> DesktopEntryPlugin<W> {
    fn new(tx: W) -> Self {
        let lang = std::env::var("LANG").ok();

        Self {
            entries: Vec::new(),
            locale: lang
                .as_ref()
                .and_then(|l| l.split('.').next())
                .map(String::from),
            tx,
        }
    }

    async fn reload(&mut self) {
        self.entries.clear();

        let locale = self.locale.as_ref().map(String::as_ref);

        let mut deduplicator = std::collections::HashSet::new();

        let current = current_desktop();
        let current = current
            .as_ref()
            .map(|x| x.split(':').collect::<Vec<&str>>());

        for (src, path) in DesktopIter::new(default_paths()) {
            if let Ok(bytes) = std::fs::read_to_string(&path) {
                if let Ok(entry) = DesktopEntry::decode(&path, &bytes) {
                    if entry.no_display() {
                        let matched = current
                            .as_ref()
                            .zip(entry.only_show_in())
                            .map(|(current, desktops)| {
                                !desktops
                                    .to_ascii_lowercase()
                                    .split(';')
                                    .any(|desktop| current.iter().any(|c| *c == desktop))
                            })
                            .unwrap_or(false);

                        if matched {
                            continue;
                        }
                    }

                    if let Some((name, exec)) = entry.name(locale).zip(entry.exec()) {
                        if let Some(exec) = exec.split_ascii_whitespace().next() {
                            let item = Item {
                                appid: entry.appid.to_owned(),
                                name: name.to_owned(),
                                description: entry.comment(locale).unwrap_or("").to_owned(),
                                keywords: entry.keywords().map(|keywords| {
                                    keywords.split(';').map(String::from).collect()
                                }),
                                icon: entry.icon().map(|x| x.to_owned()),
                                exec: exec.to_owned(),
                                path: path.clone(),
                                terminal_command: entry.terminal(),
                                prefers_non_default_gpu: entry.prefers_non_default_gpu(),
                                src,
                            };

                            deduplicator.insert(item);
                        }
                    }
                }
            }
        }

        self.entries.extend(deduplicator)
    }

    async fn activate(&mut self, id: u32) {
        tracing::debug!("activate {} from {:?}", id, self.entries);
        if let Some(entry) = self.entries.get(id as usize) {
            let response = PluginResponse::DesktopEntry(entry.path.clone());
            send(&mut self.tx, response).await;
        }
    }

    async fn search(&mut self, query: &str) {
        let query = query.to_ascii_lowercase();

        let &mut Self {
            ref entries,
            ref mut tx,
            ..
        } = self;

        let mut items = Vec::with_capacity(16);

        for (id, entry) in entries.iter().enumerate() {
            items.extend(entry.name.split_ascii_whitespace());

            if let Some(keywords) = entry.keywords.as_ref() {
                items.extend(keywords.iter().map(String::as_str));
            }

            items.push(entry.exec.as_str());

            for search_interest in items.drain(..) {
                let search_interest = search_interest.to_ascii_lowercase();
                let append = search_interest.starts_with(&*query)
                    || search_interest.contains(&*query)
                    || strsim::damerau_levenshtein(&*query, &*search_interest) < 3;

                if append {
                    let response = PluginResponse::Append(SearchMeta {
                        id: id as u32,
                        name: entry.name.clone(),
                        description: format!("{} - {}", path_string(&entry.src), entry.description),
                        keywords: entry.keywords.clone(),
                        icon: entry.icon.clone().map(Cow::Owned).map(IconSource::Name),
                        exec: Some(entry.exec.clone()),
                        ..Default::default()
                    });

                    send(tx, response).await;

                    break;
                }
            }
        }

        send(tx, PluginResponse::Finished).await;
    }
}

fn current_desktop() -> Option<String> {
    std::env::var("XDG_CURRENT_DESKTOP").ok().map(|x| {
        let x = x.to_ascii_lowercase();
        if x == "unity" {
            "gnome".to_owned()
        } else {
            x
        }
    })
}

fn path_string(source: &PathSource) -> Cow<'static, str> {
    match source {
        PathSource::Local | PathSource::LocalDesktop => "Local".into(),
        PathSource::LocalFlatpak => "Flatpak".into(),
        PathSource::System => "System".into(),
        PathSource::SystemFlatpak => "Flatpak (System)".into(),
        PathSource::SystemSnap => "Snap (System)".into(),
        PathSource::Other(other) => Cow::Owned(other.clone()),
    }
}
