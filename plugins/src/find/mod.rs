// SPDX-License-Identifier: GPL-3.0-only
// Copyright © 2021 System76

use futures_lite::*;
use pop_launcher::*;
use postage::mpsc;
use postage::prelude::{Sink, Stream};
use smol::process::{Child, ChildStdout, Command, Stdio};
use std::cell::Cell;
use std::io;
use std::path::PathBuf;
use std::rc::Rc;

enum Event {
    Activate(u32),
    Search(String),
}

pub async fn main() {
    let (mut event_tx, mut event_rx) = mpsc::channel::<Event>(8);

    // Channel for cancelling searches that are in progress.
    let (interrupt_tx, interrupt_rx) = mpsc::channel::<()>(1);

    // Indicates if a search is being performed in the background.
    let active = Rc::new(Cell::new(false));

    let mut app = SearchContext {
        search_results: Vec::with_capacity(128),
        active: active.clone(),
        interrupt_rx,
        out: async_stdout(),
    };

    // Manages the external process, tracks search results, and executes activate requests
    let search_handler = async move {
        while let Some(search) = event_rx.recv().await {
            match search {
                Event::Activate(id) => {
                    if let Some(selection) = app.search_results.get(id as usize) {
                        let path = selection.clone();
                        let handle = smol::spawn(async move {
                            crate::xdg_open(&path);
                        });

                        handle.detach();

                        crate::send(&mut app.out, PluginResponse::Close).await;
                    }
                }

                Event::Search(search) => {
                    app.search(search).await;
                    app.active.set(false);
                    crate::send(&mut app.out, PluginResponse::Finished).await;
                }
            }
        }
    };

    // Forwards requests to the search handler, and performs an interrupt as necessary.
    let request_handler = async move {
        let interrupt = || {
            let active = active.clone();
            let mut tx = interrupt_tx.clone();
            async move {
                if active.get() {
                    tracing::debug!("sending interrupt");
                    let _ = tx.try_send(());
                }
            }
        };

        let mut requests = json_input_stream(async_stdin());

        while let Some(result) = requests.next().await {
            match result {
                Ok(request) => match request {
                    // Launch the default application with the selected file
                    Request::Activate(id) => {
                        event_tx.send(Event::Activate(id)).await?;
                    }

                    // Interrupt any active searches being performed
                    Request::Interrupt => interrupt().await,

                    // Schedule a new search process to be launched
                    Request::Search(query) => {
                        interrupt().await;

                        let query = match query.find(' ') {
                            Some(pos) => query[pos..].trim_start(),
                            None => &query,
                        };

                        event_tx.send(Event::Search(query.to_owned())).await?;
                        active.set(true);
                    }

                    _ => (),
                },

                Err(why) => {
                    tracing::error!("malformed JSON input: {}", why);
                }
            }
        }

        Ok::<(), postage::sink::SendError<Event>>(())
    };

    let _ = future::zip(request_handler, search_handler).await;
}

/// Maintains state for search requests
struct SearchContext {
    pub active: Rc<Cell<bool>>,
    pub interrupt_rx: mpsc::Receiver<()>,
    pub out: smol::Unblock<io::Stdout>,
    pub search_results: Vec<PathBuf>,
}

impl SearchContext {
    /// Appends a new search result to the context.
    async fn append(&mut self, id: u32, line: String) {
        let name = line
            .rfind('/')
            .map(|pos| line[pos + 1..].to_owned())
            .unwrap_or_else(|| line.clone());

        let description = ["~/", line.as_str()].concat();

        let path = PathBuf::from(line);

        let response = PluginResponse::Append(PluginSearchResult {
            id,
            description,
            name,
            icon: Some(IconSource::Mime(crate::mime_from_path(&path))),
            ..Default::default()
        });

        crate::send(&mut self.out, response).await;
        self.search_results.push(path);
    }

    /// Submits the query to `fdfind` and actively monitors the search results while handling interrupts.
    async fn search(&mut self, search: String) {
        self.search_results.clear();
        tracing::debug!("searching for {}", search);

        let (mut child, mut stdout) = match query(&search).await {
            Ok((child, stdout)) => (child, futures_lite::io::BufReader::new(stdout).lines()),
            Err(why) => {
                tracing::error!("failed to spawn fdfind process: {}", why);

                let _ = crate::send(
                    &mut self.out,
                    PluginResponse::Append(PluginSearchResult {
                        id: 0,
                        name: if why.kind() == io::ErrorKind::NotFound {
                            String::from("fdfind command is not installed")
                        } else {
                            format!("failed to spawn fdfind process: {}", why)
                        },
                        ..Default::default()
                    }),
                )
                .await;

                return;
            }
        };

        let mut id = 0;
        let mut append;

        'stream: loop {
            let interrupt = async {
                let _ = self.interrupt_rx.recv().await;
                None
            };

            match interrupt.or(stdout.next()).await {
                Some(result) => match result {
                    Ok(line) => append = line,
                    Err(why) => {
                        tracing::error!("error on stdout line read: {}", why);
                        break 'stream;
                    }
                },

                None => break 'stream,
            }

            self.append(id, append).await;

            id += 1;

            if id == 10 {
                break 'stream;
            }
        }

        let _ = child.kill();
        let _ = child.status().await;
    }
}

/// Submits the search query to `fdfind`, and returns its stdout pipe. Falls
/// back to fdfind if it cannot be spawned.
async fn query(arg: &str) -> io::Result<(Child, ChildStdout)> {
    // Closure to spawn the process
    let spawn = |cmd: &str| -> io::Result<Child> {
        Command::new(cmd)
            .arg("-i")
            .arg(arg)
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .spawn()
    };

    // Try fdfind first, then fall back to fd
    let mut child = match spawn("fdfind") {
        Err(why) if why.kind() == io::ErrorKind::NotFound => spawn("fd"),
        result => result,
    }?;

    child
        .stdout
        .take()
        .map(move |stdout| (child, stdout))
        .ok_or_else(|| io::Error::new(io::ErrorKind::BrokenPipe, "stdout pipe is missing"))
}
