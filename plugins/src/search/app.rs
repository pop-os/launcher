use flume::Receiver;
use regex::Regex;
use std::cell::Cell;
use std::io;
use std::rc::Rc;
use tokio::io::{AsyncBufReadExt, BufReader, Lines};
use tokio::process::ChildStdout;

use pop_launcher::{async_stdout, PluginResponse, PluginSearchResult};

use crate::search::config::Definition;
use crate::search::util::{interpolate_result, interpolate_run_command};

use super::config::{load, Config};
use super::util::{
    exec, interpolate_query_command, split_query_by_regex, split_query_by_shell_words,
};

/// Maintains state for search requests
pub struct App {
    pub config: Config,

    // Indicates if a search is being performed in the background.
    pub active: Rc<Cell<bool>>,

    // Flume channel where we can send interrupt
    pub cancel: Option<flume::Receiver<()>>,

    pub out: tokio::io::Stdout,
    pub search_results: Vec<Vec<String>>,
}

impl Default for App {
    fn default() -> Self {
        Self {
            config: load(),
            search_results: Vec::with_capacity(128),
            active: Rc::new(Cell::new(false)),
            cancel: None,
            out: async_stdout(),
        }
    }
}

impl App {
    pub async fn make_listener(
        &mut self,
        stdout: &mut Lines<BufReader<ChildStdout>>,
        defn: &Definition,
        query_string: &str,
        keywords: &[String],
    ) {
        let mut id = 0;
        let mut output_line;

        'stream: loop {
            let interrupt = async {
                let x: Option<&Receiver<()>> = self.cancel.as_ref();

                if let Some(cancel) = x {
                    let _ = cancel.recv_async().await;
                } else {
                    tracing::error!("no interrupt receiver");
                }
                Ok(None)
            };

            match crate::or(interrupt, stdout.next_line()).await {
                Ok(Some(line)) => output_line = line,
                Ok(None) => {
                    break 'stream;
                }
                Err(why) => {
                    tracing::error!("error on stdout line read: {}", why);
                    break 'stream;
                }
            }

            self.append(id, &output_line, defn, query_string, keywords)
                .await;

            id += 1;

            if id == 10 {
                break 'stream;
            }
        }
    }

    /// Appends a new search result to the context.
    pub async fn append<'a>(
        &mut self,
        id: u32,
        output_line: &'a str,
        defn: &'a Definition,
        query_string: &'a str,
        keywords: &'a [String],
    ) {
        if let Ok(re) = Regex::new(&defn.output_captures) {
            if let Some(captures) = re.captures(&output_line) {
                let interpolate = |result_line: &'a str| -> Option<String> {
                    let interpolated = interpolate_result(
                        result_line,
                        output_line,
                        query_string,
                        keywords,
                        &captures,
                    );
                    if let Ok(interpolated) = interpolated {
                        Some(interpolated)
                    } else {
                        tracing::error!(
                            "unable to interpolate result: {}, {}",
                            result_line,
                            output_line
                        );
                        None
                    }
                };

                let result_name: Option<String> = interpolate(&defn.result_name);
                let result_desc: Option<String> = interpolate(&defn.result_desc);
                let run_command_parts = interpolate_run_command(
                    &defn.run_command,
                    output_line,
                    query_string,
                    keywords,
                    &captures,
                );
                eprintln!("run command: {:?}", run_command_parts);

                if let Some(name) = result_name {
                    if let Some(description) = result_desc {
                        if let Ok(run_command_parts) = run_command_parts {
                            let response = PluginResponse::Append(PluginSearchResult {
                                id,
                                name: name.to_owned(),
                                description: description.to_owned(),
                                ..Default::default()
                            });

                            crate::send(&mut self.out, response).await;
                            self.search_results.push(run_command_parts);
                        }
                    }
                }
            }
        }
    }

    // Given a query string, identify whether or not it matches one of the rules in our definition set, and
    // if so, execute the corresponding query_command.
    pub async fn search(&mut self, query_string: String) {
        self.search_results.clear();

        if let Some(rule) = self.config.match_rule(&query_string).cloned() {
            if let Some(keywords) = match rule.split {
                Some(re) => split_query_by_regex(&query_string, &re),
                None => split_query_by_shell_words(&query_string),
            } {
                eprintln!("keywords: {:?}", keywords);
                if let Some(parts) =
                    interpolate_query_command(&rule.action.query_command, &query_string, &keywords)
                        .ok()
                {
                    eprintln!("query command: {:?}", parts);
                    if let Some((program, args)) = parts.split_first() {
                        // We're good to exec the command!

                        let (mut child, mut stdout) = match exec(program, args, true).await {
                            Ok((child, stdout)) => {
                                (child, tokio::io::BufReader::new(stdout).lines())
                            }
                            Err(why) => {
                                tracing::error!("failed to spawn process: {}", why);

                                let _ = crate::send(
                                    &mut self.out,
                                    PluginResponse::Append(PluginSearchResult {
                                        id: 0,
                                        name: if why.kind() == io::ErrorKind::NotFound {
                                            String::from("command not found")
                                        } else {
                                            format!("failed to spawn process: {}", why)
                                        },
                                        ..Default::default()
                                    }),
                                )
                                .await;

                                return;
                            }
                        };

                        let timeout = async {
                            tokio::time::sleep(std::time::Duration::from_secs(10)).await;
                        };

                        let listener =
                            self.make_listener(&mut stdout, &rule.action, &query_string, &keywords);

                        futures::pin_mut!(timeout);
                        futures::pin_mut!(listener);

                        let _ = futures::future::select(timeout, listener).await;

                        let _ = child.kill().await;
                        let _ = child.wait().await;
                    }
                } else {
                    tracing::error!("can't interpolate query command");
                }
            } else {
                tracing::error!("can't split search keywords");
            }
        }
    }
}