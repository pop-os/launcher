// SPDX-License-Identifier: GPL-3.0-only
// Copyright © 2021 System76

use async_pidfd::AsyncPidFd;
use futures_lite::prelude::*;
use pop_launcher::*;
use smol::Unblock;
use std::io;

struct Selection {
    pub id: u32,
    pub name: String,
    pub description: String,
}

pub struct App {
    selections: Vec<Selection>,
    out: Unblock<io::Stdout>,
}

impl Default for App {
    fn default() -> Self {
        Self {
            out: async_stdout(),
            selections: vec![
                Selection {
                    id: 0,
                    name: "Toggle Mute".into(),
                    description: "Silence and unsilence the default audio sink".into(),
                },
                Selection {
                    id: 1,
                    name: "Volume Up".into(),
                    description: "Raise volume 5%".into(),
                },
                Selection {
                    id: 2,
                    name: "Volume Down".into(),
                    description: "Lower volume 5%".into(),
                },
            ],
        }
    }
}

pub async fn main() {
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
        let (cmd, arg1, arg2) = match id {
            0 => ("pactl", "set-sink-mute", "toggle"),
            1 => ("pactl", "set-sink-volume", "+5%"),
            2 => ("pactl", "set-sink-volume", "-5%"),
            _ => return,
        };

        let mut handles = Vec::new();

        let mut sinks = pactl_sinks();

        use postage::prelude::Stream;
        while let Some(id) = sinks.recv().await {
            handles.push(smol::spawn(async move {
                let args = &[arg1, id.as_str(), arg2];
                let _ = command_spawn(cmd, args).await;
            }));
        }

        for handle in handles {
            let _ = handle.await;
        }
    }

    async fn search(&mut self, query: String) {
        if !query.is_empty() {
            for selection in filter(&self.selections, &query.to_ascii_lowercase()) {
                crate::send(
                    &mut self.out,
                    PluginResponse::Append(PluginSearchResult {
                        id: selection.id,
                        name: selection.name.clone(),
                        description: selection.description.clone(),
                        ..Default::default()
                    }),
                )
                .await;
            }
        }

        crate::send(&mut self.out, PluginResponse::Finished).await;
    }
}

fn filter<'a>(
    selections: &'a [Selection],
    query: &'a str,
) -> impl Iterator<Item = &'a Selection> + 'a {
    selections.iter().filter_map(move |selection| {
        if selection.name.to_ascii_lowercase().contains(query)
            || selection.description.to_ascii_lowercase().contains(query)
        {
            Some(selection)
        } else {
            None
        }
    })
}

async fn command_spawn(cmd: &str, args: &[&str]) -> io::Result<()> {
    use std::process::{Command, Stdio};

    let child = Command::new(cmd)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .args(args)
        .spawn()?;

    let _ = AsyncPidFd::from_pid(child.id() as i32)?.wait().await;

    Ok(())
}

fn pactl_sinks() -> postage::mpsc::Receiver<String> {
    let (mut tx, rx) = postage::mpsc::channel(4);

    smol::spawn(async move {
        let child = smol::process::Command::new("pactl")
            .env("LANG", "C")
            .args(&["list", "sinks"])
            .stdout(smol::process::Stdio::piped())
            .spawn();

        if let Ok(mut child) = child {
            if let Some(stdout) = child.stdout.take() {
                let mut lines = futures_lite::io::BufReader::new(stdout).lines();
                while let Some(Ok(line)) = lines.next().await {
                    if let Some(stripped) = line.strip_prefix("Sink #") {
                        use postage::prelude::Sink;
                        let _ = tx.send(stripped.trim().to_owned()).await;
                    }
                }
            }
        }
    })
    .detach();

    rx
}
