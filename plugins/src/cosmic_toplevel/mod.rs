mod toplevel_handler;

use cctk::wayland_client::Proxy;
use cctk::{cosmic_protocols, sctk::reexports::calloop, toplevel_info::ToplevelInfo};
use cosmic_protocols::toplevel_info::v1::client::zcosmic_toplevel_handle_v1::ZcosmicToplevelHandleV1;

use crate::send;
use freedesktop_desktop_entry as fde;
use futures::{
    channel::mpsc,
    future::{select, Either},
    StreamExt,
};
use pop_launcher::{
    async_stdin, async_stdout, json_input_stream, IconSource, PluginResponse, PluginSearchResult,
    Request,
};
use std::borrow::Cow;
use std::{ffi::OsString, fs, path::PathBuf};
use tokio::io::{AsyncWrite, AsyncWriteExt};

use self::toplevel_handler::{toplevel_handler, ToplevelAction, ToplevelEvent};

pub async fn main() {
    tracing::info!("starting cosmic-toplevel");

    let (mut app, mut toplevel_rx) = App::new(async_stdout());

    let mut requests = json_input_stream(async_stdin());
    let mut next_request = requests.next();
    let mut next_event = toplevel_rx.next();
    loop {
        let event = select(next_request, next_event).await;
        match event {
            Either::Left((Some(request), second_to_next_event)) => {
                next_event = second_to_next_event;
                next_request = requests.next();
                match request {
                    Ok(request) => match request {
                        Request::Activate(id) => {
                            tracing::info!("activating {id}");
                            app.activate(id);
                        }
                        Request::Quit(id) => app.quit(id),
                        Request::Search(query) => {
                            tracing::info!("searching {query}");
                            app.search(&query).await;
                            // clear the ids to ignore, as all just sent are valid
                            app.ids_to_ignore.clear();
                        }
                        Request::Exit => break,
                        _ => (),
                    },
                    Err(why) => {
                        tracing::error!("malformed JSON request: {}", why);
                    }
                };
            }
            Either::Right((Some(event), second_to_next_request)) => {
                next_event = toplevel_rx.next();
                next_request = second_to_next_request;
                match event {
                    ToplevelEvent::Add(handle, info) => {
                        tracing::info!("{}", &info.app_id);
                        app.toplevels.retain(|t| t.0 != handle);
                        app.toplevels.push((handle, info));
                    }
                    ToplevelEvent::Remove(handle) => {
                        app.toplevels.retain(|t| t.0 != handle);
                        // ignore requests for this id until after the next search
                        app.ids_to_ignore.push(handle.id().protocol_id());
                    }
                    ToplevelEvent::Update(handle, info) => {
                        if let Some(t) = app.toplevels.iter_mut().find(|t| t.0 == handle) {
                            t.1 = info;
                        }
                    }
                }
            }
            _ => break,
        }
    }
}

struct App<W> {
    desktop_entries: Vec<(fde::PathSource, PathBuf)>,
    ids_to_ignore: Vec<u32>,
    toplevels: Vec<(ZcosmicToplevelHandleV1, ToplevelInfo)>,
    calloop_tx: calloop::channel::Sender<ToplevelAction>,
    tx: W,
}

impl<W: AsyncWrite + Unpin> App<W> {
    fn new(tx: W) -> (Self, mpsc::UnboundedReceiver<ToplevelEvent>) {
        let (toplevels_tx, toplevel_rx) = mpsc::unbounded();
        let (calloop_tx, calloop_rx) = calloop::channel::channel();
        let _ = std::thread::spawn(move || toplevel_handler(toplevels_tx, calloop_rx));

        (
            Self {
                ids_to_ignore: Vec::new(),
                desktop_entries: fde::Iter::new(fde::default_paths())
                    .map(|path| (fde::PathSource::guess_from(&path), path))
                    .collect(),
                toplevels: Vec::new(),
                calloop_tx,
                tx,
            },
            toplevel_rx,
        )
    }

    fn activate(&mut self, id: u32) {
        tracing::info!("requested to activate: {id}");
        if self.ids_to_ignore.contains(&id) {
            return;
        }
        if let Some(handle) = self.toplevels.iter().find_map(|t| {
            if t.0.id().protocol_id() == id {
                Some(t.0.clone())
            } else {
                None
            }
        }) {
            tracing::info!("activating: {id}");
            let _ = self.calloop_tx.send(ToplevelAction::Activate(handle));
        }
    }

    fn quit(&mut self, id: u32) {
        if self.ids_to_ignore.contains(&id) {
            return;
        }
        if let Some(handle) = self.toplevels.iter().find_map(|t| {
            if t.0.id().protocol_id() == id {
                Some(t.0.clone())
            } else {
                None
            }
        }) {
            let _ = self.calloop_tx.send(ToplevelAction::Close(handle));
        }
    }

    async fn search(&mut self, query: &str) {
        fn contains_pattern(needle: &str, haystack: &[&str]) -> bool {
            let needle = needle.to_ascii_lowercase();
            haystack.iter().all(|h| needle.contains(h))
        }

        let query = query.to_ascii_lowercase();
        let haystack = query.split_ascii_whitespace().collect::<Vec<&str>>();

        for item in &self.toplevels {
            let retain = query.is_empty()
                || contains_pattern(&item.1.app_id, &haystack)
                || contains_pattern(&item.1.title, &haystack);

            if !retain {
                continue;
            }

            let mut icon_name = Cow::Borrowed("application-x-executable");

            for (_, path) in &self.desktop_entries {
                if let Some(name) = path.file_stem() {
                    let app_id: OsString = item.1.app_id.clone().into();
                    if app_id == name {
                        if let Ok(data) = fs::read_to_string(path) {
                            if let Ok(entry) = fde::DesktopEntry::decode(path, &data) {
                                if let Some(icon) = entry.icon() {
                                    icon_name = Cow::Owned(icon.to_owned());
                                }
                            }
                        }

                        break;
                    }
                }
            }

            send(
                &mut self.tx,
                PluginResponse::Append(PluginSearchResult {
                    // XXX protocol id may be re-used later
                    id: item.0.id().protocol_id(),
                    name: item.1.app_id.clone(),
                    description: item.1.title.clone(),
                    icon: Some(IconSource::Name(icon_name)),
                    ..Default::default()
                }),
            )
            .await;
        }

        send(&mut self.tx, PluginResponse::Finished).await;
        let _ = self.tx.flush();
    }
}
