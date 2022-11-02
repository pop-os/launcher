// SPDX-License-Identifier: GPL-3.0-only
// Copyright © 2021 System76

use futures::prelude::*;
use pop_launcher::*;
use crate::detect_terminal;

pub struct App {
    last_query: Option<String>,
    out: tokio::io::Stdout,
    shell_only: bool,
}

impl Default for App {
    fn default() -> Self {
        Self {
            last_query: None,
            out: async_stdout(),
            shell_only: false,
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
    async fn activate(&mut self, _id: u32) {
        let exe = match self.last_query.take() {
            Some(cmd) => cmd,
            None => return,
        };

        use fork::{daemon, Fork};

        crate::send(&mut self.out, PluginResponse::Close).await;

        if let Ok(Fork::Child) = daemon(true, false) {
            use std::os::unix::process::CommandExt;
            use std::process::Command;

            let mut cmd;

            if self.shell_only {
                cmd = Command::new("sh");
                cmd.args(&["-c", &exe]);
            } else {
                let (terminal, arg) = detect_terminal();
                cmd = Command::new(terminal);
                cmd.args(&[
                    arg,
                    "sh",
                    "-c",
                    &format!("{}; echo \"Press Enter to exit\"; read t", exe),
                ]);
            }

            let _ = cmd.exec();
        }

        std::process::exit(0);
    }

    async fn search(&mut self, query: String) {
        self.splice_input(&query).await;
        crate::send(&mut self.out, PluginResponse::Finished).await;
    }

    async fn splice_input(&mut self, mut query: &str) {
        if let Some(q) = query.strip_prefix(':') {
            self.shell_only = true;
            query = q.trim();
            self.last_query = Some(query.to_owned());
        } else {
            self.shell_only = false;

            let query = if let Some(query) = query.strip_prefix("t:") {
                query.trim()
            } else if let Some(pos) = query.find(' ') {
                query[pos + 1..].trim()
            } else {
                return;
            };

            self.last_query = Some(query.to_owned());
        }

        crate::send(
            &mut self.out,
            PluginResponse::Append(PluginSearchResult {
                id: 0,
                name: query.to_owned(),
                description: String::from("run command in terminal"),
                ..Default::default()
            }),
        )
        .await;
    }
}