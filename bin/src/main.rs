// Copyright 2021 System76 <info@system76.com>
// SPDX-License-Identifier: MPL-2.0

use pop_launcher_plugins as plugins;
use pop_launcher_service as service;
use smol::block_on;
use std::io;

fn main() {
    tracing_subscriber::fmt()
        .with_writer(io::stderr)
        .with_env_filter(tracing_subscriber::EnvFilter::from_default_env())
        .init();

    if let Some(plugin) = std::env::args().next() {
        let start = plugin.rfind('/').map(|v| v + 1).unwrap_or(0);
        let cmd = &plugin.as_str()[start..];
        match cmd {
            "calc" => block_on(plugins::calc::main()),
            "desktop-entries" => block_on(plugins::desktop_entries::main()),
            "find" => block_on(plugins::find::main()),
            "files" => block_on(plugins::files::main()),
            "pop-launcher" => block_on(service::main()),
            "pop-shell" => block_on(plugins::pop_shell::main()),
            "pulse" => block_on(plugins::pulse::main()),
            "recent" => block_on(plugins::recent::main()),
            "scripts" => block_on(plugins::scripts::main()),
            "terminal" => block_on(plugins::terminal::main()),
            "web" => block_on(plugins::web::main()),
            unknown => {
                eprintln!("unknown cmd: {}", unknown);
            }
        }
    }
}
