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

    let mut app = App::default();

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
    out: Unblock<io::Stdout>,
    outcome: Option<String>,
    regex: Regex,
}

impl Default for App {
    fn default() -> Self {
        Self {
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
            name: "Qalc Manual".into(),
        }];

        crate::send(&mut self.out, PluginResponse::Context { id: 0, options }).await;
    }

    pub async fn search(&mut self, query: &str) {
        if let Some(mut search) = query.strip_prefix("=") {
            search = search.trim();
            self.outcome = qcalc(&mut self.regex, search).await;

            crate::send(
                &mut self.out,
                PluginResponse::Append(PluginSearchResult {
                    id: 0,
                    name: self
                        .outcome
                        .clone()
                        .unwrap_or_else(|| [search, " x = ?"].concat()),
                    description: "Math expressions by Qalc".to_owned(),
                    icon: Some(IconSource::Name(Cow::Borrowed("accessories-calculator"))),
                    ..Default::default()
                }),
            )
            .await;

            crate::send(&mut self.out, PluginResponse::Finished).await;
        }
    }
}

async fn qcalc(regex: &mut Regex, expression: &str) -> Option<String> {
    let mut child = Command::new("qalc")
        .env("LANG", "C")
        .arg("-t")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .ok()?;

    if let Some(mut stdin) = child.stdin.take() {
        let _ = stdin
            .write_all([expression, "\n"].concat().as_bytes())
            .await;
    }

    if let Some(stdout) = child.stdout.take() {
        let mut reader = smol::io::BufReader::new(stdout).lines().skip(2);
        let mut output = String::new();

        while let Some(Ok(line)) = reader.next().await {
            let line = line.trim();

            if line.is_empty() {
                break;
            }

            let normalized = regex.replace_all(line, "");

            if normalized.starts_with("error") {
                return None;
            } else {
                if !output.is_empty() {
                    output.push(' ');
                }

                output.push_str(normalized.as_ref());
            };
        }

        return Some(output);
    }

    None
}
