// Copyright 2021 System76 <info@system76.com>
// SPDX-License-Identifier: MPL-2.0

use pop_launcher_plugins as plugins;
use pop_launcher_service as service;
use std::io;

#[tokio::main(flavor = "current_thread")]
async fn main() {
    tracing_subscriber::fmt()
        .with_writer(io::stderr)
        .with_env_filter(tracing_subscriber::EnvFilter::from_default_env())
        .init();

    if let Some(plugin) = std::env::args().next() {
        let start = plugin.rfind('/').map(|v| v + 1).unwrap_or(0);
        let cmd = &plugin.as_str()[start..];
        match cmd {
            "calc" => plugins::calc::main().await,
            "desktop-entries" => plugins::desktop_entries::main().await,
            "find" => plugins::find::main().await,
            "files" => plugins::files::main().await,
            "pop-launcher" => service::main().await,
            "pop-shell" => plugins::pop_shell::main().await,
            "pulse" => plugins::pulse::main().await,
            "recent" => plugins::recent::main().await,
            "scripts" => plugins::scripts::main().await,
            "terminal" => plugins::terminal::main().await,
            "web" => plugins::web::main().await,
            unknown => {
                eprintln!("unknown cmd: {}", unknown);
            }
        }
    }
}
