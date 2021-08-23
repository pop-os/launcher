use futures_lite::prelude::*;
use pop_launcher::*;
use smol::Unblock;
use std::io;

pub struct App {
    out: Unblock<io::Stdout>,
}

impl Default for App {
    fn default() -> Self {
        Self {
            out: async_stdout(),
        }
    }
}

pub async fn main() {
    let mut requests = json_input_stream(async_stdin());

    let mut app = App::default();

    while let Some(result) = requests.next().await {
        match result {
            Ok(request) => match request {
                Request::Activate(id) => app.activate(id).await,
                Request::Search(query) => app.search(query).await,
                Request::Exit => break,
                _ => (),
            },
            Err(why) => {
                tracing::error!("malformed JSON input: {}", why);
            }
        }
    }
}

impl App {
    async fn activate(&mut self, id: u32) {}

    async fn search(&mut self, query: String) {

    }
}
