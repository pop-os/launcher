// SPDX-License-Identifier: GPL-3.0-only
// Copyright © 2021 System76

use fork::{daemon, Fork};
use pop_launcher::{Indice, PluginResponse, PluginSearchResult};
use pop_launcher_toolkit::plugin_trait::{async_trait, PluginExt};
use std::io;
use std::os::unix::process::CommandExt;
use std::path::PathBuf;
use std::process::{exit, Command};
use pop_launcher_plugins::detect_terminal;

// This example demonstrate how to write a pop-launcher plugin using the `PluginExt` helper trait.
// We are going to build a plugin to display man pages descriptions and open them on activation.
// To do that we will use `whatis`, a command that searches the manual page names and displays their descriptions.

// For instance running `whatis git` would output the following :
// ```
// git (1)              - the stupid content tracker
// Git (3pm)            - Perl interface to the Git version control system
// ```

// Run `whatis` and split the output line to get a man page name and its description
fn run_whatis(arg: &str) -> io::Result<Vec<(String, String)>> {
    let output = Command::new("whatis").arg(arg).output()?.stdout;

    Ok(String::from_utf8_lossy(&output)
        .lines()
        .filter_map(|entry| entry.split_once('-'))
        .map(|(man_page, description)| {
            (man_page.trim().to_string(), description.trim().to_string())
        })
        .collect())
}

// Open a new terminal and run `man` with the provided man page name
fn open_man_page(arg: &str) -> io::Result<()> {
    let (terminal, targ) = detect_terminal();

    if let Ok(Fork::Child) = daemon(true, false) {
        Command::new(terminal).args(&[targ, "man", arg]).exec();
    }

    exit(0);
}

// Our plugin struct, holding the search results.
#[derive(Default)]
pub struct WhatIsPlugin {
    entries: Vec<(String, String)>,
}

// This is the main part of our plugin, defining how it will react to pop-launcher requests.
#[async_trait]
impl PluginExt for WhatIsPlugin {
    // Define the name of our plugin, this is mainly used to write log
    // emitted by tracing macros to `$HOME/.local/state/pop-launcher/wathis.log.
    fn name(&self) -> &str {
        "whatis"
    }

    // Define how the plugin will react to pop-launcher search requests.
    // Note that we need to send `PluginResponse::Finished` once we are done,
    // otherwise pop-launcher will not display our search results and wait forever.
    async fn search(&mut self, query: &str) {
        // pop-launcher will only dispatch query matching the regex defined in our `plugin.ron`
        // file, can safely strip it out.
        let query = query.strip_prefix("whatis ");

        if let Some(query) = query {
            // Whenever we get a new query, pass the query to the `whatis` helper function
            // and update our plugin entries with the result.
            match run_whatis(query) {
                Ok(entries) => self.entries = entries,
                // If we need to produce log, we use the tracing macros.
                Err(err) => tracing::error!("Error while running 'whatis' command: {err}"),
            }

            // Now we send our entries back to the launcher. We also need a way to find our entry on activation
            // requests, here we use the entry index as an idendifier.
            for (idx, (cmd, description)) in self.entries.iter().enumerate() {
                self.respond_with(PluginResponse::Append(PluginSearchResult {
                    id: idx as u32,
                    name: format!("{cmd} - {description}"),
                    keywords: None,
                    description: description.clone(),
                    icon: None,
                    exec: None,
                    window: None,
                }))
                .await;
            }
        }

        // Tell pop-launcher we are done with this search request.
        self.respond_with(PluginResponse::Finished).await;
    }

    // pop-launcher is asking for an entry activation.
    async fn activate(&mut self, id: Indice) {
        // First we try to find the requested entry in the plugin struct
        if let Some((command, _description)) = self.entries.get(id as usize) {
            // Open a new terminal with the requested man page and exit the plugin.
            if let Err(err) = open_man_page(command) {
                tracing::error!("Failed to open man page for '{command}': {err}")
            }
        }
    }
}

// Now we just need to call the `run` function to start our plugin.
// You can test it by writing request to its stdin.
// For instance issuing a search request : `{ "Search": "whatis git" }`,
// or activate one of the search results : `{ "Activate": 0 }`
#[tokio::main(flavor = "current_thread")]
async fn main() {
    WhatIsPlugin::default().run().await
}
