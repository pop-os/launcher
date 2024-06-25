// SPDX-License-Identifier: GPL-3.0-only
// Copyright © 2021 System76

use app::App;
use futures::*;
use pop_launcher::{async_stdin, json_input_stream, PluginResponse, Request};

use crate::search::util::exec;

mod app;
mod config;
mod util;

#[derive(Debug)]
enum Event {
    Activate(u32),
    Search(String),
}

pub async fn main() {
    let (event_tx, event_rx) = flume::bounded::<Event>(8);

    // Channel for cancelling searches that are in progress.
    let (interrupt_tx, interrupt_rx) = flume::bounded::<()>(1);

    let mut app = App::default();

    app.cancel = Some(interrupt_rx);

    let active = app.active.clone();

    // Manages the external process, tracks search results, and executes activate requests
    let search_handler = async move {
        while let Ok(search) = event_rx.recv_async().await {
            match search {
                Event::Activate(id) => {
                    if let Some(selection) = app.search_results.get(id as usize) {
                        let run_command_parts = selection.clone();
                        tokio::spawn(async move {
                            if let Some((program, args)) = run_command_parts.split_first() {
                                // We're good to exec the command!
                                let _ = exec(program, args, false).await;
                            }
                        });

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
            let tx = interrupt_tx.clone();
            async move {
                if active.get() {
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
                        event_tx.send_async(Event::Activate(id)).await?;
                    }

                    // Interrupt any active searches being performed
                    Request::Interrupt => interrupt().await,

                    // Schedule a new search process to be launched
                    Request::Search(query) => {
                        interrupt().await;

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

    let _ = futures::future::join(request_handler, search_handler).await;
}
