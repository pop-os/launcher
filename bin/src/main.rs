// Copyright 2021 System76 <info@system76.com>
// SPDX-License-Identifier: MPL-2.0

use pop_launcher_toolkit::plugins;
use pop_launcher_toolkit::service;

use mimalloc::MiMalloc;

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
        .open(logdir.join([cmd, ".log"].concat().as_str()).as_path());

    if let Ok(file) = logfile {
        use tracing_subscriber::{fmt, EnvFilter};
        fmt()
            .with_env_filter(EnvFilter::from_default_env())
            .with_writer(file)
            .init();
    }
}
