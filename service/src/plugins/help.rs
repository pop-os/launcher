use crate::*;
use pop_launcher::*;
use flume::Sender;
use slab::Slab;
use std::borrow::Cow;

pub const REGEX: Cow<'static, str> = Cow::Borrowed("^(\\?).*");

pub const CONFIG: PluginConfig = PluginConfig {
    name: Cow::Borrowed("Help"),
    description: Cow::Borrowed("Show available plugin prefixes"),
    bin: None,
    query: PluginQuery {
        help: None,
        isolate: true,
        no_sort: true,
        persistent: false,
        regex: None,
    },
    icon: Some(IconSource::Name(Cow::Borrowed("system-help-symbolic"))),
};
pub struct HelpPlugin {
    pub details: Slab<PluginHelp>,
    pub internal: Sender<Event>,
    pub tx: Sender<PluginResponse>,
}

impl HelpPlugin {
    pub fn new(internal: Sender<Event>, tx: Sender<PluginResponse>) -> Self {
        Self {
            details: Slab::new(),
            internal,
            tx,
        }
    }

    async fn reload(&mut self) {
        let (tx, rx) = async_oneshot::oneshot();
        let _ = self.internal.send_async(Event::Help(tx)).await;
        self.details = rx.await.expect("internal error fetching help info");
    }
}

#[async_trait::async_trait]
impl Plugin for HelpPlugin {
    async fn activate(&mut self, id: u32) {
        if let Some(detail) = self.details.get(id as usize) {
            if let Some(help) = detail.help.as_ref() {
                let _ = self.tx.send_async(PluginResponse::Fill(help.clone())).await;
            }
        }
    }

    async fn complete(&mut self, id: u32) {
        self.activate(id).await
    }

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
                let _ = self
                    .tx
                    .send_async(PluginResponse::Append(PluginSearchResult {
                        id: id as u32,
                        name: detail.name.clone(),
                        description: detail.description.clone(),
                        ..Default::default()
                    }))
                    .await;
            }
        }

        let _ = self.tx.send_async(PluginResponse::Finished).await;
    }

    async fn quit(&mut self, _id: u32) {}
}
