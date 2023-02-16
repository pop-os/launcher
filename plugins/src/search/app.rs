use flume::Receiver;
use regex::Regex;
use std::cell::Cell;
// use std::future::Future;
use std::io;
// use std::ops::Deref;
use std::rc::Rc;
// use std::sync::Arc;
use tokio::io::{AsyncBufReadExt, BufReader, Lines};
use tokio::process::ChildStdout;

use pop_launcher::{async_stdout, PluginResponse, PluginSearchResult};

use crate::search::config::Definition;
use crate::search::util::interpolate_result;

use super::config::{load, Config, DisplayLine};
use super::util::{exec, interpolate_command};

/// Maintains state for search requests
pub struct App {
    pub config: Config,

    // Indicates if a search is being performed in the background.
    pub active: Rc<Cell<bool>>,

    // Flume channel where we can send interrupt
    pub cancel: Option<flume::Receiver<()>>,

    pub out: tokio::io::Stdout,
    pub search_results: Vec<String>,
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
        defn: &Definition,
        stdout: &mut Lines<BufReader<ChildStdout>>,
        search_terms: &[String],
    ) {
        let mut id = 0;
        let mut append;
        eprintln!("start listener");

        'stream: loop {
            let interrupt = async {
                let x: Option<&Receiver<()>> = self.cancel.as_ref();
                // let x: Option<&Receiver<()>> = (*cancel).as_ref();

                if let Some(cancel) = x {
                    let _ = cancel.recv_async().await;
                } else {
                    eprintln!("no interrupt receiver");
                    tracing::error!("no interrupt receiver");
                }
                Ok(None)
            };

            match crate::or(interrupt, stdout.next_line()).await {
                Ok(Some(line)) => {
                    eprintln!("append line: {}", line);
                    append = line
                }
                Ok(None) => {
                    eprintln!("listener; break stream");
                    break 'stream;
                }
                Err(why) => {
                    eprintln!("error on stdout line read: {}", why);
                    tracing::error!("error on stdout line read: {}", why);
                    break 'stream;
                }
            }

            self.append(id, &append, defn, search_terms).await;

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
        line: &'a str,
        defn: &'a Definition,
        vars: &'a [String],
    ) {
        eprintln!("append: {:?} {:?}", id, line);

        let match_display_line = |display_line: &'a DisplayLine| -> Option<String> {
            match display_line {
                DisplayLine::Label(label) => Some(label.clone()),
                DisplayLine::Capture(pattern) => {
                    if let Ok(re) = Regex::new(&pattern) {
                        re.captures(&line)
                            .and_then(|caps| caps.get(1))
                            .map(|cap| cap.as_str().to_owned())
                    } else {
                        tracing::error!("failed to build Capture regex: {}", pattern);

                        None
                    }
                }
                DisplayLine::Replace(pattern, replace) => {
                    if let Ok(re) = Regex::new(&pattern) {
                        if let Some(capture) = re
                            .captures(&line)
                            .and_then(|caps| caps.get(1))
                            .map(|cap| cap.as_str())
                        {
                            let replacement = interpolate_result(replace, &vars, capture);
                            if let Ok(replacement) = replacement {
                                Some(replacement)
                            } else {
                                tracing::error!(
                                    "unable to interpolate Replace: {}, {}",
                                    pattern,
                                    replace
                                );

                                None
                            }
                        } else {
                            None
                        }
                    } else {
                        tracing::error!("failed to build Replace regex: {}", pattern);

                        None
                    }
                }
            }
        };

        let title: Option<String> = match_display_line(&defn.title);
        let detail: Option<String> = match_display_line(&defn.detail);

        if let Some(title) = title {
            if let Some(detail) = detail {
                let response = PluginResponse::Append(PluginSearchResult {
                    id,
                    name: title.to_owned(),
                    description: detail.to_owned(),
                    ..Default::default()
                });

                eprintln!("append; send response {:?}", response);

                crate::send(&mut self.out, response).await;
                self.search_results.push(line.to_string());
            }
        }
    }

    /// Submits the query to `fdfind` and actively monitors the search results while handling interrupts.
    pub async fn search(&mut self, search: String) {
        eprintln!("config: {:?}", self.config);

        self.search_results.clear();

        if let Some(search_terms) = shell_words::split(&search).ok().as_deref() {
            if let Some(word) = search_terms.first() {
                eprintln!("look for word: '{}'", word);

                let word_defn: Option<Definition> = self.config.get(word).cloned();

                if let Some(defn) = word_defn {
                    if let Some(parts) = interpolate_command(&defn.query, search_terms).ok() {
                        eprintln!("search parts: {:?}", parts);

                        if let Some((program, args)) = parts.split_first() {
                            // We're good to exec the command!

                            let (mut child, mut stdout) = match exec(program, args).await {
                                Ok((child, stdout)) => {
                                    eprintln!("spawned process");
                                    (child, tokio::io::BufReader::new(stdout).lines())
                                }
                                Err(why) => {
                                    eprintln!("failed to spawn process: {}", why);
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
                                tokio::time::sleep(std::time::Duration::from_secs(3)).await;
                            };

                            let listener = self.make_listener(&defn, &mut stdout, search_terms);

                            futures::pin_mut!(timeout);
                            futures::pin_mut!(listener);

                            let _ = futures::future::select(timeout, listener).await;

                            let _ = child.kill().await;
                            let _ = child.wait().await;
                        }
                    } else {
                        eprintln!("can't interpolate command");
                    }
                } else {
                    eprintln!("no matching definition");
                }
            } else {
                eprintln!("search term has no head word");
            }
        } else {
            eprintln!("can't split search terms");
        }
    }
}
