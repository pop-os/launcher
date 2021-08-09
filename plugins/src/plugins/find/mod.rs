use futures_lite::*;
use pop_launcher::*;
use pop_launcher_plugins::send;
use smol::process::{ChildStdout, Command, Stdio};
use std::borrow::Cow;
use std::cell::Cell;
use std::io;
use std::path::{Path, PathBuf};
use std::rc::Rc;

enum Event {
    Activate(u32),
    Search(String),
}

pub async fn main() {
    let (event_tx, event_rx) = flume::unbounded::<Event>();

    // Channel for cancelling searches that are in progress.
    let (interrupt_tx, interrupt_rx) = flume::bounded::<()>(0);

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
        while let Ok(search) = event_rx.recv_async().await {
            match search {
                Event::Activate(id) => {
                    if let Some(selection) = app.search_results.get(id as usize) {
                        let path = selection.clone();
                        let handle = smol::spawn(async move {
                            xdg_open(&path).await;
                        });

                        handle.detach();

                        send(&mut app.out, PluginResponse::Close).await;
                    }
                }

                Event::Search(search) => app.search(search).await,
            }
        }
    };

    // Forwards requests to the search handler, and performs an interrupt as necessary.
    let request_handler = async move {
        let interrupt = || async {
            if active.get() && !interrupt_tx.is_full() {
                tracing::debug!("sending interrupt");
                let _ = interrupt_tx.send_async(()).await;
            }
        };

        let mut requests = json_input_stream(async_stdin());

        while let Some(result) = requests.next().await {
            match result {
                Ok(request) => match request {
                    // Launch the default application with the selected file
                    Request::Activate(id) => {
                        event_tx.send_async(Event::Activate(id)).await?;
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

                        event_tx.send_async(Event::Search(query.to_owned())).await?;
                        active.set(true);
                    }

                    _ => (),
                },

                Err(why) => {
                    tracing::error!("malformed JSON input: {}", why);
                }
            }
        }

        Ok::<(), flume::SendError<Event>>(())
    };

    let _ = future::zip(request_handler, search_handler).await;
}

/// Maintains state for search requests
struct SearchContext {
    pub active: Rc<Cell<bool>>,
    pub interrupt_rx: flume::Receiver<()>,
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

        let response = PluginResponse::Append(SearchMeta {
            id,
            description,
            name,
            icon: Some(IconSource::Mime(if path.is_dir() {
                Cow::Borrowed("inode/directory")
            } else if let Some(guess) = new_mime_guess::from_path(&path).first() {
                Cow::Owned(guess.essence_str().to_owned())
            } else {
                Cow::Borrowed("text/plain")
            })),
            ..Default::default()
        });

        send(&mut self.out, response).await;
        self.search_results.push(path);
    }

    /// Submits the query to `fdfind` and actively monitors the search results while handling interrupts.
    async fn search(&mut self, search: String) {
        tracing::debug!("searching for {}", search);

        let mut stdout = match query(&search).await {
            Ok(stdout) => futures_lite::io::BufReader::new(stdout).lines(),
            Err(why) => {
                tracing::error!("failed to spawn fdfind process: {}", why);
                self.active.set(false);
                return;
            }
        };

        self.search_results.clear();
        let mut id = 0;
        let mut append;

        'stream: loop {
            let interrupt = async {
                let _ = self.interrupt_rx.recv_async().await;
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

        send(&mut self.out, PluginResponse::Finished).await;
    }
}

/// Submits the search query to `fdfind`, and returns its stdout pipe.
async fn query(arg: &str) -> io::Result<ChildStdout> {
    let mut child = Command::new("fdfind")
        .arg(arg)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()?;

    match child.stdout.take() {
        Some(stdout) => Ok(stdout),
        None => Err(io::Error::new(
            io::ErrorKind::BrokenPipe,
            "stdout pipe is missing",
        )),
    }
}

/// Launches a file with its default appplication via `xdg-open`.
async fn xdg_open(file: &Path) {
    let _ = Command::new("xdg-open").arg(file).spawn();
}
