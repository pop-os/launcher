// Copyright 2021 System76 <info@system76.com>
// SPDX-License-Identifier: MPL-2.0

use pop_launcher_toolkit::plugins;
use pop_launcher_toolkit::service;

use mimalloc::MiMalloc;
use pop_launcher_toolkit::service::ensure_cache_path;

#[global_allocator]
static GLOBAL: MiMalloc = MiMalloc;

#[tokio::main(flavor = "current_thread")]
async fn main() {
    if let Some(plugin) = std::env::args().next() {
        let start = plugin.rfind('/').map(|v| v + 1).unwrap_or(0);
        let cmd = &plugin.as_str()[start..];

        init_logging(cmd);

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
            "cosmic-toplevel" => plugins::cosmic_toplevel::main().await,
            unknown => {
                eprintln!("unknown cmd: {}", unknown);
            }
        }
    }
}

fn init_logging(cmd: &str) {
    let logdir = ensure_cache_path().expect("failed to get cache path for saving logs");

    let logfile = std::fs::OpenOptions::new()
        .create(true)
        .truncate(true)
        .write(true)
        .open(logdir.join([cmd, ".log"].concat().as_str()).as_path());

    if let Ok(file) = logfile {
        use tracing_subscriber::{fmt, EnvFilter};
        fmt()
            .with_env_filter(EnvFilter::from_default_env())
            .with_writer(file)
            .init();
    }
}
