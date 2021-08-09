use pop_launcher::Service;
use std::io;

fn main() {
    tracing_subscriber::fmt()
        .with_writer(io::stderr)
        .with_env_filter(tracing_subscriber::EnvFilter::from_default_env())
        .init();

    let stdout = io::stdout();
    smol::block_on(Service::new(stdout.lock()).exec());
}
