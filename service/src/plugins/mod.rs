// Copyright 2021 System76 <info@system76.com>
// SPDX-License-Identifier: MPL-2.0

pub mod config;
pub(crate) mod external;
// pub mod help;

pub use self::config::{PluginConfig, PluginPriority};
pub use self::external::ExternalPlugin;

use crate::{Indice, PluginHelp, Request};
use async_trait::async_trait;
use flume::{Receiver, Sender};

#[async_trait]
pub trait Plugin
where
    Self: Sized + Send,
{
    /// Activate the selected ID from this plugin
    async fn activate(&mut self, id: Indice);

    async fn activate_context(&mut self, id: Indice, context: Indice);

    async fn complete(&mut self, id: Indice);

    async fn context(&mut self, id: Indice);

    fn exit(&mut self);

    async fn interrupt(&mut self);

    fn name(&self) -> &str;

    async fn search(&mut self, query: &str);

    async fn quit(&mut self, id: Indice);

    async fn run(&mut self, rx: Receiver<Request>) {
        while let Ok(request) = rx.recv_async().await {
            tracing::event!(
                tracing::Level::DEBUG,
                "{}: received {:?}",
                self.name(),
                request
            );
            match request {
                Request::Search(query) => self.search(&query).await,
                Request::Interrupt => self.interrupt().await,
                Request::Activate(id) => self.activate(id).await,
                Request::ActivateContext { id, context } => {
                    self.activate_context(id, context).await
                }
                Request::Complete(id) => self.complete(id).await,
                Request::Context(id) => self.context(id).await,
                Request::Quit(id) => self.quit(id).await,
                Request::Exit => {
                    self.exit();
                    break;
                }
                Request::Close => {
                    self.exit();
                }
            }
        }

        tracing::event!(tracing::Level::DEBUG, "{}: exiting plugin", self.name());
    }
}

/// Stores all information relevant for communicating with a plugin
///
/// Plugins may be requested to exit, and relaunched at any point in the future.
pub struct PluginConnector {
    /// The deserialized configuration file for this plugin
    pub config: PluginConfig,

    /// Code that is executed to prepare a new instance of
    /// this plugin to spawn as a background service
    pub init: Box<dyn Fn() -> Sender<Request>>,

    /// The sender of the spawned background service that will be
    /// forwarded to the launncher service
    pub sender: Option<Sender<Request>>,
}

impl PluginConnector {
    pub fn new(config: PluginConfig, init: Box<dyn Fn() -> Sender<Request> + Send>) -> Self {
        Self {
            config,
            init,
            sender: None,
        }
    }

    pub fn details(&self) -> PluginHelp {
        PluginHelp {
            name: self.config.name.to_string(),
            description: self.config.description.clone().unwrap_or_default(),
            help: self.config.generic_query.clone(),
        }
    }

    /// Obtains the sender for sending messages to this plugin.
    ///
    /// If the sender is absent, the plugin is relaunched with a new one.
    pub fn sender_exec(&mut self) -> &mut Sender<Request> {
        let &mut Self {
            ref mut sender,
            ref init,
            ..
        } = self;

        sender.get_or_insert_with(init)
    }

    /// Drops the sender, which will subsequently drop the plugin forwarder attached to it
    pub fn sender_drop(&mut self) {
        self.sender = None;
    }
}
