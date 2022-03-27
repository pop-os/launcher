// SPDX-License-Identifier: GPL-3.0-only
// Copyright © 2021 System76

use crate::*;
use pop_launcher::*;

use flume::Sender;
use futures::{AsyncBufReadExt, StreamExt};
use smol::process::{Command, Stdio};
use std::collections::VecDeque;
use std::{
    io,
    path::{Path, PathBuf},
};

const LOCAL_PATH: &str = ".local/share/pop-launcher/scripts";
const SYSTEM_ADMIN_PATH: &str = "/etc/pop-launcher/scripts";
const DISTRIBUTION_PATH: &str = "/usr/lib/pop-launcher/scripts";

pub async fn main() {
    let mut requests = json_input_stream(async_stdin());

    let mut app = App::new();

    app.reload().await;

    while let Some(result) = requests.next().await {
        match result {
            Ok(request) => match request {
                Request::Activate(id) => app.activate(id).await,
                Request::Search(query) => app.search(&query).await,
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
    scripts: Vec<ScriptInfo>,
    out: smol::Unblock<io::Stdout>,
}

impl App {
    fn new() -> Self {
        App {
            scripts: Vec::with_capacity(16),
            out: async_stdout(),
        }
    }

    async fn activate(&mut self, id: u32) {
        if let Some(script) = self.scripts.get(id as usize) {
            let interpreter = script.interpreter.as_deref().unwrap_or("sh");
            send(&mut self.out, PluginResponse::Close).await;

            let _ = Command::new(interpreter)
                .arg(script.path.as_os_str())
                .stdin(Stdio::null())
                .stdout(Stdio::null())
                .stderr(Stdio::null())
                .spawn();
        }
    }

    async fn reload(&mut self) {
        let (tx, rx) = flume::bounded::<ScriptInfo>(8);

        let mut queue = VecDeque::new();

        queue.push_back(
            dirs::home_dir()
                .expect("user does not have home dir")
                .join(LOCAL_PATH),
        );
        queue.push_back(Path::new(SYSTEM_ADMIN_PATH).to_owned());
        queue.push_back(Path::new(DISTRIBUTION_PATH).to_owned());

        let script_sender = async move {
            while let Some(path) = queue.pop_front() {
                load_from(&path, &mut queue, tx.clone()).await;
            }
        };

        let script_receiver = async {
            'outer: while let Ok(script) = rx.recv_async().await {
                tracing::debug!("appending script: {:?}", script);
                for cached_script in &self.scripts {
                    if cached_script.name == script.name {
                        continue 'outer;
                    }
                }
                self.scripts.push(script);
            }
        };

        futures::future::join(script_sender, script_receiver).await;
    }

    async fn search(&mut self, query: &str) {
        let &mut Self {
            ref scripts,
            ref mut out,
            ..
        } = self;
        for (id, script) in scripts.iter().enumerate() {
            let should_include = script.name.to_ascii_lowercase().contains(query)
                || script.description.to_ascii_lowercase().contains(query)
                || script.keywords.iter().any(|k| k.contains(query));

            if should_include {
                send(
                    out,
                    PluginResponse::Append(PluginSearchResult {
                        id: id as u32,
                        name: script.name.clone(),
                        description: script.description.clone(),
                        icon: script
                            .icon
                            .as_ref()
                            .map(|icon| IconSource::Name(icon.clone().into())),
                        keywords: Some(script.keywords.clone()),
                        ..Default::default()
                    }),
                )
                .await;
            }
        }

        send(out, PluginResponse::Finished).await;
    }
}

#[derive(Debug, Default)]
struct ScriptInfo {
    interpreter: Option<String>,
    name: String,
    icon: Option<String>,
    path: PathBuf,
    keywords: Vec<String>,
    description: String,
}

async fn load_from(path: &Path, paths: &mut VecDeque<PathBuf>, tx: Sender<ScriptInfo>) {
    if let Ok(directory) = path.read_dir() {
        for entry in directory.filter_map(Result::ok) {
            let tx = tx.clone();
            let path = entry.path();

            if path.is_dir() {
                paths.push_back(path);
                continue;
            }

            smol::spawn(async move {
                let mut file = match smol::fs::File::open(&path).await {
                    Ok(file) => futures::io::BufReader::new(file).lines(),
                    Err(why) => {
                        tracing::error!("cannot open script at {}: {}", path.display(), why);
                        return;
                    }
                };

                let mut info = ScriptInfo {
                    path,
                    ..Default::default()
                };

                let mut first = true;

                while let Some(Ok(line)) = file.next().await {
                    if !line.starts_with('#') {
                        break;
                    }

                    let line = line[1..].trim();

                    if first {
                        first = false;
                        if let Some(interpreter) = line.strip_prefix('!') {
                            info.interpreter = Some(interpreter.to_owned());
                            continue;
                        }
                    }

                    if let Some(stripped) = line.strip_prefix("name:") {
                        info.name = stripped.trim_start().to_owned();
                    } else if let Some(stripped) = line.strip_prefix("description:") {
                        info.description = stripped.trim_start().to_owned();
                    } else if let Some(stripped) = line.strip_prefix("icon:") {
                        info.icon = Some(stripped.trim_start().to_owned());
                    } else if let Some(stripped) = line.strip_prefix("keywords:") {
                        info.keywords =
                            stripped.trim_start().split(' ').map(String::from).collect();
                    }
                }

                let _ = tx.send_async(info).await;
            })
            .detach();
        }
    }
}
