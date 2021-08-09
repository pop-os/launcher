mod config;
pub(crate) mod external;
pub mod help;

pub use self::config::{PluginBinary, PluginConfig, PluginQuery};
pub use self::external::ExternalPlugin;
pub use self::help::{HelpPlugin, PluginHelp};

use crate::Request;
use async_trait::async_trait;
use flume::{Receiver, Sender};
use regex::Regex;

#[async_trait]
pub trait Plugin
where
    Self: Sized + Send,
{
    /// Activate the selected ID from this plugin
    async fn activate(&mut self, id: u32);

    async fn complete(&mut self, id: u32);

    fn exit(&mut self);

    async fn interrupt(&mut self);

    fn name(&self) -> &str;

    async fn search(&mut self, query: &str);

    async fn quit(&mut self, id: u32);

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
                Request::Complete(id) => self.complete(id).await,
                Request::Quit(id) => self.quit(id).await,
                Request::Exit => {
                    self.exit();
                    break;
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

    /// A compiled regular expression that a query must match
    /// for the launcher service to justify spawning and sending
    /// queries to this plugin
    pub regex: Option<Regex>,

    /// The sender of the spawned background service that will be
    /// forwarded to the launncher service
    pub sender: Option<Sender<Request>>,
}

impl PluginConnector {
    pub fn new(
        config: PluginConfig,
        regex: Option<Regex>,
        init: Box<dyn Fn() -> Sender<Request> + Send>,
    ) -> Self {
        Self {
            config,
            init,
            regex,
            sender: None,
        }
    }

    pub fn details(&self) -> PluginHelp {
        PluginHelp {
            name: self.config.name.as_ref().to_owned(),
            description: self.config.description.as_ref().to_owned(),
            help: self
                .config
                .query
                .help
                .as_ref()
                .map(|x| x.as_ref().to_owned()),
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

        sender.get_or_insert_with(|| init())
    }

    /// Drops the sender, which will subsequently drop the plugin forwarder attached to it
    pub fn sender_drop(&mut self) {
        self.sender = None;
    }
}
