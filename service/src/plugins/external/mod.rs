// Copyright 2021 System76 <info@system76.com>
// SPDX-License-Identifier: MPL-2.0

pub mod load;

use std::{
    io,
    path::PathBuf,
    process::Stdio,
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc,
    },
};

use crate::{Event, Indice, Plugin, PluginResponse, Request};
use async_oneshot::oneshot;
use flume::Sender;
use futures::StreamExt;
use tokio::{
    io::AsyncWriteExt,
    process::{Child, Command},
    task::JoinHandle,
};
use tracing::{event, Level};

pub struct ExternalPlugin {
    id: usize,
    tx: Sender<Event>,
    name: String,
    pub cmd: PathBuf,
    pub args: Vec<String>,
    process: Option<(JoinHandle<()>, Child, async_oneshot::Sender<()>)>,
    detached: Arc<AtomicBool>,
    searching: Arc<AtomicBool>,
}

impl ExternalPlugin {
    pub fn new(
        id: usize,
        name: String,
        cmd: PathBuf,
        args: Vec<String>,
        tx: Sender<Event>,
    ) -> Self {
        Self {
            id,
            name,
            tx,
            cmd,
            args,
            process: None,
            detached: Arc::default(),
            searching: Arc::default(),
        }
    }

    pub fn launch(&mut self) -> Option<&mut (JoinHandle<()>, Child, async_oneshot::Sender<()>)> {
        event!(Level::DEBUG, "{}: launching plugin", self.name());

        let child = Command::new(&self.cmd)
            .args(&self.args)
            .stdout(Stdio::piped())
            .stdin(Stdio::piped())
            .stderr(Stdio::inherit())
            .spawn()
            .ok();

        if let Some(mut child) = child {
            if let Some(stdout) = child.stdout.take() {
                let detached = self.detached.clone();
                let searching = self.searching.clone();
                let (trip_tx, trip_rx) = oneshot::<()>();
                let tx = self.tx.clone();
                let name = self.name().to_owned();
                let id = self.id;

                // Spawn a background task to forward JSON responses from the child process.
                let task = tokio::spawn(async move {
                    let tx_ = tx.clone();
                    let searching_ = searching.clone();
                    let name_ = name.clone();

                    // Future for directly handling the JSON output from the process.
                    let responder = async move {
                        let mut requests = crate::json_input_stream(stdout);

                        while let Some(result) = requests.next().await {
                            match result {
                                Ok(response) => {
                                    if let PluginResponse::Finished = response {
                                        searching_.store(false, Ordering::SeqCst);
                                    }

                                    let _ = tx_.send_async(Event::Response((id, response))).await;
                                }
                                Err(why) => {
                                    tracing::error!("{}: serde error: {:?}", name_, why);
                                }
                            }
                        }

                        tracing::debug!("{}: exiting from responder", name_);
                    };

                    let trip = async move {
                        let _ = trip_rx.await;
                    };

                    futures::pin_mut!(responder);
                    futures::pin_mut!(trip);

                    futures::future::select(responder, trip)
                        .await
                        .factor_first();

                    // Ensure that a task that was searching sends a finished signal if it dies.
                    if searching.swap(false, Ordering::SeqCst) {
                        let _ = tx
                            .send_async(Event::Response((id, PluginResponse::Finished)))
                            .await;
                    }

                    detached.store(true, Ordering::SeqCst);

                    event!(Level::DEBUG, "{}: detached plugin", name);
                });

                self.process = Some((task, child, trip_tx));
            }
        }

        self.process.as_mut()
    }

    pub async fn process_check(&mut self) {
        if let Some(mut child) = self.process.take() {
            match child.1.try_wait() {
                Err(_) | Ok(Some(_)) => {
                    child.0.abort();
                }
                Ok(None) => self.process = Some(child),
            }

            if self.detached.swap(false, Ordering::SeqCst) {
                self.process = None;
            }
        }
    }

    pub async fn query(&mut self, event: &Request) -> io::Result<()> {
        self.process_check().await;

        if self.process.is_none() {
            tracing::debug!("{}: relaunching process", self.name());
            self.launch();
        }

        if let Some((_, child, _)) = self.process.as_mut() {
            if let Some(stdin) = child.stdin.as_mut() {
                if let Ok(mut serialized) = serde_json::to_vec(event) {
                    serialized.push(b'\n');
                    stdin.write_all(&serialized).await?;
                    tracing::debug!("{}: sent message to external process", self.name());
                }

                return Ok(());
            }
        }

        Err(io::Error::new(
            io::ErrorKind::NotConnected,
            "child process could not be reached",
        ))
    }
}

#[async_trait::async_trait]
impl Plugin for ExternalPlugin {
    async fn activate(&mut self, id: Indice) {
        let _ = self.query(&Request::Activate(id)).await;
    }

    async fn activate_context(&mut self, id: Indice, context: Indice) {
        let _ = self.query(&Request::ActivateContext { id, context }).await;
    }

    async fn complete(&mut self, id: Indice) {
        let _ = self.query(&Request::Complete(id)).await;
    }

    async fn context(&mut self, id: Indice) {
        let _ = self.query(&Request::Context(id)).await;
    }

    fn exit(&mut self) {
        if let Some((_, _, mut trigger)) = self.process.take() {
            let _ = trigger.send(());
        }
    }

    async fn interrupt(&mut self) {
        let _ = self.query(&Request::Interrupt).await;
    }

    fn name(&self) -> &str {
        &self.name
    }

    async fn search(&mut self, query: &str) {
        if self.query(&Request::Search(query.to_owned())).await.is_ok() {
            self.searching.store(true, Ordering::SeqCst);
        } else {
            let _ = self
                .tx
                .send_async(Event::Response((self.id, PluginResponse::Finished)))
                .await;
        }
    }
    async fn quit(&mut self, id: Indice) {
        let _ = self.query(&Request::Quit(id)).await;
    }
}
