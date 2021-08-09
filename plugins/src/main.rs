mod plugins;

use smol::block_on;
use std::io;

fn main() {
    tracing_subscriber::fmt()
        .with_writer(io::stderr)
        .with_env_filter(tracing_subscriber::EnvFilter::from_default_env())
        .init();

    std::env::args();

    if let Some(plugin) = std::env::args().next() {
        let start = plugin.rfind('/').map(|v| v + 1).unwrap_or(0);
        match &plugin.as_str()[start..] {
            "desktop-entries" => block_on(plugins::desktop_entries::main()),
            "pop-shell" => block_on(plugins::pop_shell::main()),
            "find" => block_on(plugins::find::main()),
            "scripts" => block_on(plugins::scripts::main()),
            unknown => {
                eprintln!("unknown cmd: {}", unknown);
            }
        }
    }
}
