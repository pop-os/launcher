// Copyright 2021 System76 <info@system76.com>
// SPDX-License-Identifier: MPL-2.0

//! # pop-launcher-toolkit
//!
//! A toolkit to write pop-launcher client and plugin.
//!
//! ## Crates
//!  - **[`launcher`]:** re-export the pop-launcher crate, containing all the IPC message struct and
//!    some utility functions to locate plugins.
//!  - **[`service`]:** re-export the pop-launcher-service crate, containing deserializable plugin config struct.
//!    This is useful if your client needs to read user defined plugins configs.
//!  - **[`plugins`]:** re-export pop-launcher-plugins which defines all the default pop-launcher plugins.
//!    Useful if your client needs to read default plugin configs
//!
//! ## Writing a plugin
//!
//! Add the following to your Cargo.toml :
//!
//! ```toml
//! [dependencies]
//! tokio = { version = "1.18.2", features = ["rt"] }
//! pop-launcher-toolkit = { git = "https://github.com/pop-os/launcher" }
//! ```
//!
//! And implement the [`PluginExt`] trait:
//!
//! [`PluginExt`]: plugin_trait::PluginExt
//!
//! ```rust
//! use pop_launcher_toolkit::launcher::{Indice, PluginResponse, PluginSearchResult};
//! use pop_launcher_toolkit::plugin_trait::{async_trait, PluginExt};
//! use pop_launcher_toolkit::plugins;
//!
//! // The plugin struct, here it holds the search result
//! pub struct MyPlugin {
//!   data: Vec<String>
//! }
//!
//! #[async_trait]
//! impl PluginExt for MyPlugin {
//!
//!   // Define the name of you plugin, this will be used
//!   // to generate a logfile in $XDG_STATE_HOME at runtime.
//!   fn name(&self) -> &str {
//!       "my_awesome_plugin"
//!   }
//!
//!   // Respond to `pop-launcher` 'search' query
//!   async fn search(&mut self, query: &str) {
//!      // `pop-launcher` dispatches request to plugins according to the regex defined in
//!      // the `plugin.ron` config file, here we get rid of the prefix
//!      // before processing the request.
//!      let query = query.strip_prefix("plug ").unwrap();
//!
//!      // Iterate through our internal search results with their indices.
//!      let search_results = self.data.iter()
//!         .enumerate()
//!         .filter(|(idx, data)| data.contains(query));
//!
//!      // Send our search results to `pop-launcher` using their indices as id.
//!      for (idx, search_result) in search_results {
//!         self.respond_with(PluginResponse::Append(PluginSearchResult {
//!             id: idx as u32,
//!             name: search_result.clone(),
//!             description: "".to_string(),
//!             keywords: None,
//!             icon: None,
//!             exec: None,
//!             window: None,
//!         })).await;
//!      }
//!
//!     // tell `pop-launcher` we are done with this request
//!     self.respond_with(PluginResponse::Finished).await;
//!   }
//!
//!   // Respond to `pop-launcher` 'activate' query
//!   async fn activate(&mut self, id: Indice) {
//!       // Get the selected entry
//!       let entry = self.data.get(id as usize).unwrap();
//!       // Here we use xdg_open to run the entry but this could be anything
//!       plugins::xdg_open(entry);
//!       // Tell pop launcher we are done
//!       self.respond_with(PluginResponse::Finished);
//!   }
//!
//!   // Respond to `pop-launcher` 'close' request.
//!   async fn quit(&mut self, id: Indice) {
//!       self.respond_with(PluginResponse::Close).await;
//!   }
//! }
//!
//! #[tokio::main(flavor = "current_thread")]
//! pub async fn main() {
//!
//!     // Here we declare our plugin with dummy values, and never mutate them.
//!     // In a real plugin we would probably use some kind of mutable shared reference to
//!     // update our search results.
//!     let mut plugin = MyPlugin {
//!         data: vec!["https://crates.io".to_string(), "https://en.wikipedia.org".to_string()],
//!     };
//!
//!     /// If you need to debug your plugin or display error messages use `tcracing` macros.
//!     tracing::info!("Starting my_awsome_plugin");
//!
//!     // Call the plugin entry point function to start
//!     // talking with pop_launcherc
//!     plugin.run().await;
//! }
//! ```

pub use pop_launcher as launcher;
pub use pop_launcher_plugins as plugins;
pub use pop_launcher_service::{
    self as service, load::from_path as load_plugin_from_path,
    load::from_paths as load_plugins_from_paths,
};

/// A helper trait to quickly create `pop-launcher` plugins
pub mod plugin_trait;
