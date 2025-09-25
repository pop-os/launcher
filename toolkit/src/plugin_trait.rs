// Copyright 2021 System76 <info@system76.com>
// SPDX-License-Identifier: MPL-2.0

use futures::StreamExt;
use pop_launcher::{Indice, PluginResponse, Request, async_stdin, async_stdout, json_input_stream};

pub use async_trait::async_trait;
use pop_launcher_plugins as plugins;

/// Re-export of the tracing crate, use this to add custom logs to your plugin
pub use tracing;

/// A helper trait to create `pop-launcher` plugins.
#[async_trait]
pub trait PluginExt
where
    Self: Sized + Send,
{
    /// The name of our plugin, currently this is used internally to create the plugin log file at
    /// `$XDG_STATE_HOME/pop-launcher/{name}.log`
    fn name(&self) -> &str;

    /// Handle a [`Request::Search`] issued by `pop-launcher`.
    /// To send search result back use [`PluginResponse::Append`].
    /// Once finished [`PluginResponse::Finished`] is expected to notify the search result are ready to be displayed.
    async fn search(&mut self, query: &str);

    /// Define how the plugin should handle [`Request::Activate`] request.
    /// Typically run the requested entry (for instance using [`super::plugins::xdg_open`])
    /// and close the client with a [`PluginResponse::Close`]
    async fn activate(&mut self, id: Indice);

    /// Define how the plugin should handle [`Request::ActivateContext`] request.
    /// Typically run the requested entry with the provided context (for instance using [`super::plugins::xdg_open`])
    /// and close the client with a [`PluginResponse::Close`]
    async fn activate_context(&mut self, _id: Indice, _context: Indice) {}

    /// Handle an autocompletion request from the client
    async fn complete(&mut self, _id: Indice) {}

    /// `pop-launcher` request the context for the given [`SearchResult`] id.
    /// to send the requested context use [`PluginResponse::Context`]
    ///
    /// [`SearchResult`]: pop_launcher::SearchResult
    async fn context(&mut self, _id: Indice) {}

    /// This is automatically called after `pop-launcher` requests the plugin to exit.
    /// Use this only if your plugin does not need to perform specific clean ups.
    fn exit(&mut self) {}

    /// Whenever a new search query is issued, `pop-launcher` will send a [`Request::Interrupt`]
    /// so we can stop any ongoing computation before handling the next query.
    /// This is especially useful for plugins that rely on external services
    /// to get their search results (a HTTP endpoint for instance)
    async fn interrupt(&mut self) {}

    /// The launcher is asking us to quit a specific item.
    async fn quit(&mut self, _id: Indice) {}

    /// A helper function to send [`PluginResponse`] back to `pop-launcher`
    async fn respond_with(&self, response: PluginResponse) {
        plugins::send(&mut async_stdout(), response).await
    }

    /// Run the plugin
    async fn run(&mut self) {
        self.init_logging();
        let mut receiver = json_input_stream(async_stdin());
        while let Some(request) = receiver.next().await {
            tracing::event!(
                tracing::Level::DEBUG,
                "{}: received {:?}",
                self.name(),
                request
            );

            match request {
                Ok(request) => match request {
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
                        break;
                    }
                },
                Err(why) => tracing::error!("Malformed json request: {why}"),
            }
        }

        tracing::event!(tracing::Level::DEBUG, "{}: exiting plugin", self.name());
    }

    fn init_logging(&self) {
        let logdir = match dirs::state_dir() {
            Some(dir) => dir.join("pop-launcher/"),
            None => dirs::home_dir()
                .expect("home directory required")
                .join(".cache/pop-launcher"),
        };

        let _ = std::fs::create_dir_all(&logdir);

        let logfile = std::fs::OpenOptions::new()
            .create(true)
            .truncate(true)
            .write(true)
            .open(
                logdir
                    .join([self.name(), ".log"].concat().as_str())
                    .as_path(),
            );

        if let Ok(file) = logfile {
            use tracing_subscriber::{EnvFilter, fmt};
            fmt()
                .with_env_filter(EnvFilter::from_default_env())
                .with_writer(file)
                .init();
        }
    }
}
