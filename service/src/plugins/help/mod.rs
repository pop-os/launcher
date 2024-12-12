// Copyright 2021 System76 <info@system76.com>
// SPDX-License-Identifier: MPL-2.0

use crate::*;
use flume::Sender;
use pop_launcher::*;
use slab::Slab;

pub fn manifest() -> PluginConfig {
    PluginConfig::from_str(
        &PathBuf::default(),
        &PathBuf::default(),
        include_str!("plugin.desktop"),
    )
    .unwrap()
}

pub struct HelpPlugin {
    pub id: usize,
    pub details: Slab<PluginHelp>,
    pub tx: Sender<Event>,
}

impl HelpPlugin {
    pub fn new(id: usize, tx: Sender<Event>) -> Self {
        Self {
            id,
            details: Slab::new(),
            tx,
        }
    }

    async fn reload(&mut self) {
        let (tx, rx) = async_oneshot::oneshot();
        let _ = self.tx.send_async(Event::Help(tx)).await;
        self.details = rx.await.expect("internal error fetching help info");
    }
}

#[async_trait::async_trait]
impl Plugin for HelpPlugin {
    async fn activate(&mut self, id: u32) {
        if let Some(detail) = self.details.get(id as usize) {
            if let Some(help) = detail.help.as_ref() {
                let _ = self
                    .tx
                    .send_async(Event::Response((
                        self.id,
                        PluginResponse::Fill(help.clone()),
                    )))
                    .await;
            }
        }
    }

    async fn activate_context(&mut self, _: u32, _: u32) {}

    async fn complete(&mut self, id: u32) {
        self.activate(id).await
    }

    async fn context(&mut self, _: u32) {}

    fn exit(&mut self) {}

    async fn interrupt(&mut self) {}

    fn name(&self) -> &str {
        "help"
    }

    async fn search(&mut self, _query: &str) {
        if self.details.is_empty() {
            self.reload().await;
        }
        for (id, detail) in self.details.iter() {
            if detail.help.is_some() {
                let response = PluginResponse::Append(PluginSearchResult {
                    id: id as u32,
                    name: detail.name.clone(),
                    description: detail.description.clone(),
                    ..Default::default()
                });

                let _ = self
                    .tx
                    .send_async(Event::Response((self.id, response)))
                    .await;
            }
        }

        let _ = self
            .tx
            .send_async(Event::Response((self.id, PluginResponse::Finished)))
            .await;
    }

    async fn quit(&mut self, _id: u32) {}
}
