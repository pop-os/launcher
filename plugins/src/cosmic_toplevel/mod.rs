mod toplevel_handler;

use cctk::cosmic_protocols::toplevel_info::v1::client::zcosmic_toplevel_handle_v1::State;
use cctk::wayland_client::Proxy;
use cctk::{cosmic_protocols, sctk::reexports::calloop, toplevel_info::ToplevelInfo};
use cosmic_protocols::toplevel_info::v1::client::zcosmic_toplevel_handle_v1::ZcosmicToplevelHandleV1;
use fde::DesktopEntry;
use freedesktop_desktop_entry as fde;
use toplevel_handler::TopLevelsUpdate;
use tracing::{debug, error, info, warn};

use crate::desktop_entries::utils::{get_description, is_session_cosmic};
use crate::send;
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
use std::iter;
use tokio::io::{AsyncWrite, AsyncWriteExt};

use self::toplevel_handler::{toplevel_handler, ToplevelAction};

pub async fn main() {
    let mut tx = async_stdout();

    if !is_session_cosmic() {
        send(&mut tx, PluginResponse::Deactivate).await;
        return;
    }

    let (mut app, mut toplevel_rx) = App::new(tx);

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
                            debug!("activating {id}");
                            app.activate(id);
                        }
                        Request::Quit(id) => app.quit(id),
                        Request::Search(query) => {
                            debug!("searching {query}");
                            app.search(&query).await;
                            // clear the ids to ignore, as all just sent are valid
                            app.ids_to_ignore.clear();
                        }
                        Request::Exit => break,
                        _ => (),
                    },
                    Err(why) => {
                        error!("malformed JSON request: {}", why);
                    }
                };
            }
            Either::Right((Some(updates), second_to_next_request)) => {
                next_event = toplevel_rx.next();
                next_request = second_to_next_request;

                for (handle, info) in updates {
                    match info {
                        Some(info) => {
                            if let Some(pos) = app.toplevels.iter().position(|t| t.0 == handle) {
                                if info.state.contains(&State::Activated) {
                                    app.toplevels.remove(pos);
                                    app.toplevels.push((handle, Box::new(info)));
                                } else {
                                    app.toplevels[pos].1 = Box::new(info);
                                }
                            } else {
                                app.toplevels.push((handle, Box::new(info)));
                            }
                        }
                        // no info means remove
                        None => {
                            if let Some(pos) = app.toplevels.iter().position(|t| t.0 == handle) {
                                app.toplevels.remove(pos);
                                // ignore requests for this id until after the next search
                                app.ids_to_ignore.push(handle.id().protocol_id());
                            } else {
                                warn!("no toplevel to remove");
                            }
                        }
                    }
                }
            }
            _ => break,
        }
    }
}

struct App<W> {
    locales: Vec<String>,
    desktop_entries: Vec<DesktopEntry<'static>>,
    ids_to_ignore: Vec<u32>,
    toplevels: Vec<(ZcosmicToplevelHandleV1, Box<ToplevelInfo>)>,
    calloop_tx: calloop::channel::Sender<ToplevelAction>,
    tx: W,
}

impl<W: AsyncWrite + Unpin> App<W> {
    fn new(tx: W) -> (Self, mpsc::UnboundedReceiver<TopLevelsUpdate>) {
        let (toplevels_tx, toplevel_rx) = mpsc::unbounded();
        let (calloop_tx, calloop_rx) = calloop::channel::channel();
        let _handle = std::thread::spawn(move || toplevel_handler(toplevels_tx, calloop_rx));

        let locales = fde::get_languages_from_env();

        let paths = fde::Iter::new(fde::default_paths());

        let desktop_entries = DesktopEntry::from_paths(paths, &locales)
            .filter_map(|e| e.ok())
            .collect::<Vec<_>>();

        (
            Self {
                locales,
                desktop_entries,
                ids_to_ignore: Vec::new(),
                toplevels: Vec::new(),
                calloop_tx,
                tx,
            },
            toplevel_rx,
        )
    }

    fn activate(&mut self, id: u32) {
        info!("requested to activate: {id}");
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
            info!("activating: {id}");
            let _res = self.calloop_tx.send(ToplevelAction::Activate(handle));
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
            let _res = self.calloop_tx.send(ToplevelAction::Close(handle));
        }
    }

    async fn search(&mut self, query: &str) {
        let query = query.to_ascii_lowercase();

        for (handle, info) in self.toplevels.iter().rev() {
            let entry = if query.is_empty() {
                fde::matching::get_best_match(
                    &[&info.app_id, &info.title],
                    &self.desktop_entries,
                    fde::matching::MatchAppIdOptions::default(),
                )
            } else {
                let lowercase_title = info.title.to_lowercase();
                let window_words = lowercase_title
                    .split_whitespace()
                    .chain(iter::once(info.app_id.as_str()))
                    .chain(iter::once(info.title.as_str()))
                    .collect::<Vec<_>>();

                fde::matching::get_best_match(
                    &window_words,
                    &self.desktop_entries,
                    fde::matching::MatchAppIdOptions::default(),
                )
                .and_then(|de| {
                    let score =
                        fde::matching::get_entry_score(&query, de, &self.locales, &window_words);

                    if score > 0.8 {
                        Some(de)
                    } else {
                        None
                    }
                })
            };

            if let Some(de) = entry {
                let icon_name = if let Some(icon) = de.icon() {
                    Cow::Owned(icon.to_owned())
                } else {
                    Cow::Borrowed("application-x-executable")
                };

                let response = PluginResponse::Append(PluginSearchResult {
                    // XXX protocol id may be re-used later
                    id: handle.id().protocol_id(),
                    window: Some((0, handle.id().clone().protocol_id())),
                    description: info.title.clone(),
                    name: get_description(de, &self.locales),
                    icon: Some(IconSource::Name(icon_name)),
                    ..Default::default()
                });

                send(&mut self.tx, response).await;
            }
        }

        send(&mut self.tx, PluginResponse::Finished).await;
        let _ = self.tx.flush().await;
    }
}
