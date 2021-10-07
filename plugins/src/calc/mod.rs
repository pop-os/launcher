// SPDX-License-Identifier: GPL-3.0-only
// Copyright © 2021 System76

use futures_lite::{AsyncBufReadExt, AsyncWriteExt, StreamExt};
use pop_launcher::*;
use regex::Regex;
use smol::{
    process::{Command, Stdio},
    Unblock,
};
use std::{borrow::Cow, io};

pub async fn main() {
    let mut requests = json_input_stream(async_stdin());

    let mut app = App {
        decimal_comma: uses_decimal_comma().await,
        ..Default::default()
    };

    while let Some(result) = requests.next().await {
        match result {
            Ok(request) => match request {
                Request::Activate(_) => app.activate().await,
                Request::ActivateContext { .. } => app.activate_context().await,
                Request::Context(_) => app.context().await,
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
    pub decimal_comma: bool,
    out: Unblock<io::Stdout>,
    outcome: Option<String>,
    regex: Regex,
}

impl Default for App {
    fn default() -> Self {
        Self {
            decimal_comma: false,
            out: async_stdout(),
            outcome: None,
            regex: Regex::new("\\x1B\\[(?:;?[0-9]{1,3})+[mGK]").expect("bad regex for qalc"),
        }
    }
}

impl App {
    pub async fn activate(&mut self) {
        if let Some(mut outcome) = self.outcome.take() {
            outcome = ["= ", outcome.as_str()].concat();
            crate::send(&mut self.out, PluginResponse::Fill(outcome)).await;
        }
    }

    pub async fn activate_context(&mut self) {
        crate::xdg_open("https://qalculate.github.io/manual/qalc.html");
        crate::send(&mut self.out, PluginResponse::Close).await;
    }

    pub async fn context(&mut self) {
        let options = vec![ContextOption {
            id: 0,
            name: "Qalculate! Manual".into(),
        }];

        crate::send(&mut self.out, PluginResponse::Context { id: 0, options }).await;
    }

    pub async fn search(&mut self, query: &str) {
        if let Some(mut search) = query.strip_prefix('=') {
            search = search.trim();
            self.outcome = qcalc(&mut self.regex, search, self.decimal_comma).await;

            crate::send(
                &mut self.out,
                PluginResponse::Append(PluginSearchResult {
                    id: 0,
                    name: self
                        .outcome
                        .clone()
                        .unwrap_or_else(|| [search, " x = ?"].concat()),
                    description: String::new(),
                    icon: Some(IconSource::Name(Cow::Borrowed("accessories-calculator"))),
                    ..Default::default()
                }),
            )
            .await;
        }

        crate::send(&mut self.out, PluginResponse::Finished).await;
    }
}

async fn qcalc(regex: &mut Regex, expression: &str, decimal_comma: bool) -> Option<String> {
    let mut command = Command::new("qalc");

    if decimal_comma {
        command.args(&["-set", "decimal comma on"]);
    }

    let spawn = command
        .env("LANG", "C")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn();

    let mut child = match spawn {
        Ok(child) => child,
        Err(why) => {
            return Some(if why.kind() == io::ErrorKind::NotFound {
                String::from("qalc command is not installed")
            } else {
                format!("qalc command failed to spawn: {}", why)
            })
        }
    };

    if let Some(mut stdin) = child.stdin.take() {
        let _ = stdin
            .write_all([expression, "\n"].concat().as_bytes())
            .await;
    }

    let stdout = match child.stdout.take() {
        Some(stdout) => stdout,
        None => {
            return Some(String::from(
                "qalc lacks stdout pipe: did you get hit by a cosmic ray?",
            ));
        }
    };

    let mut reader = smol::io::BufReader::new(stdout).lines().skip(2);
    let mut output = String::new();

    fn has_issue(line: &str) -> bool {
        line.starts_with("error") || line.starts_with("warning")
    }

    while let Some(Ok(line)) = reader.next().await {
        let line = line.trim();

        if line.is_empty() {
            break;
        }

        let normalized = regex.replace_all(line, "");
        let mut normalized = normalized.as_ref();

        if has_issue(normalized) {
            return None;
        } else {
            if !output.is_empty() {
                output.push(' ');
            }

            if normalized.starts_with('(') {
                let mut level = 1;
                for (byte_pos, character) in normalized[1..].char_indices() {
                    if character == '(' {
                        level += 1;
                    } else if character == ')' {
                        level -= 1;

                        if level == 0 {
                            normalized = normalized[byte_pos + 2..].trim_start();
                            break;
                        }
                    }
                }
            }

            let cut = if let Some(pos) = normalized.find('=') {
                pos + 1
            } else if let Some(pos) = normalized.find('≈') {
                pos + '≈'.len_utf8()
            } else {
                return None;
            };

            normalized = normalized[cut..].trim_start();
            if normalized.starts_with('(') && normalized.ends_with(')') {
                normalized = &normalized[1..normalized.len() - 1];
            }

            output.push_str(normalized);
        };
    }

    Some(output)
}

pub async fn uses_decimal_comma() -> bool {
    let spawn_result = Command::new("locale")
        .arg("-ck")
        .arg("decimal_point")
        .stderr(Stdio::null())
        .output()
        .await;

    if let Ok(output) = spawn_result {
        if let Ok(string) = String::from_utf8(output.stdout) {
            return string.contains("decimal_point=\",\"");
        }
    }

    false
}
