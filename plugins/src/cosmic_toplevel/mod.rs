mod toplevel_handler;

use cctk::cosmic_protocols::toplevel_info::v1::client::zcosmic_toplevel_handle_v1::State;
use cctk::wayland_client::Proxy;
use cctk::{sctk::reexports::calloop, toplevel_info::ToplevelInfo};
use fde::DesktopEntry;
use freedesktop_desktop_entry as fde;
use toplevel_handler::ToplevelUpdate;
use tracing::{debug, error, info, warn};

use crate::desktop_entries::utils::{get_description, is_session_cosmic};
use crate::send;
use futures::{
    StreamExt,
    channel::mpsc,
    future::{Either, select},
};
use pop_launcher::{
    IconSource, PluginResponse, PluginSearchResult, Request, async_stdin, async_stdout,
    json_input_stream,
};
use std::borrow::Cow;
use tokio::io::{AsyncWrite, AsyncWriteExt};

use self::toplevel_handler::{ToplevelAction, toplevel_handler};

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

                for update in updates {
                    match update {
                        ToplevelUpdate::Info(info) => {
                            if let Some(pos) = app
                                .toplevels
                                .iter()
                                .position(|t| t.foreign_toplevel == info.foreign_toplevel)
                            {
                                if info.state.contains(&State::Activated) {
                                    app.toplevels.remove(pos);
                                    app.toplevels.push(Box::new(info));
                                } else {
                                    app.toplevels[pos] = Box::new(info);
                                }
                            } else {
                                app.toplevels.push(Box::new(info));
                            }
                        }
                        ToplevelUpdate::Remove(foreign_toplevel) => {
                            if let Some(pos) = app
                                .toplevels
                                .iter()
                                .position(|t| t.foreign_toplevel == foreign_toplevel)
                            {
                                app.toplevels.remove(pos);
                                // ignore requests for this id until after the next search
                                app.ids_to_ignore.push(foreign_toplevel.id().protocol_id());
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
    desktop_entries: Vec<DesktopEntry>,
    ids_to_ignore: Vec<u32>,
    toplevels: Vec<Box<ToplevelInfo>>,
    calloop_tx: calloop::channel::Sender<ToplevelAction>,
    tx: W,
}

impl<W: AsyncWrite + Unpin> App<W> {
    fn new(tx: W) -> (Self, mpsc::UnboundedReceiver<Vec<ToplevelUpdate>>) {
        let (toplevels_tx, toplevel_rx) = mpsc::unbounded();
        let (calloop_tx, calloop_rx) = calloop::channel::channel();
        let _handle = std::thread::spawn(move || toplevel_handler(toplevels_tx, calloop_rx));

        let locales = fde::get_languages_from_env();

        let desktop_entries = fde::Iter::new(fde::default_paths())
            .map(|path| DesktopEntry::from_path(path, Some(&locales)))
            .filter_map(Result::ok)
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
            if t.foreign_toplevel.id().protocol_id() == id {
                Some(t.foreign_toplevel.clone())
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
            if t.foreign_toplevel.id().protocol_id() == id {
                Some(t.foreign_toplevel.clone())
            } else {
                None
            }
        }) {
            let _res = self.calloop_tx.send(ToplevelAction::Close(handle));
        }
    }

    async fn search(&mut self, query: &str) {
        fn contains_pattern(needle: &str, haystack: &[&str]) -> bool {
            let needle = needle.to_ascii_lowercase();
            haystack.iter().all(|h| needle.contains(h))
        }

        let query = query.to_ascii_lowercase();
        let haystack = query.split_ascii_whitespace().collect::<Vec<&str>>();

        for info in &self.toplevels {
            let retain = query.is_empty()
                || contains_pattern(&info.app_id, &haystack)
                || contains_pattern(&info.title, &haystack);

            if !retain {
                continue;
            }

            let appid = fde::unicase::Ascii::new(info.app_id.as_str());

            let entry = fde::find_app_by_id(&self.desktop_entries, appid)
                .map(ToOwned::to_owned)
                .unwrap_or_else(|| fde::DesktopEntry::from_appid(appid.to_string()).to_owned());

            let icon_name = if let Some(icon) = entry.icon() {
                Cow::Owned(icon.to_owned())
            } else {
                Cow::Borrowed("application-x-executable")
            };

            let response = PluginResponse::Append(PluginSearchResult {
                // XXX protocol id may be re-used later
                id: info.foreign_toplevel.id().protocol_id(),
                window: Some((0, info.foreign_toplevel.id().protocol_id())),
                description: info.title.clone(),
                name: get_description(&entry, &self.locales),
                icon: Some(IconSource::Name(icon_name)),
                ..Default::default()
            });

            send(&mut self.tx, response).await;
        }

        send(&mut self.tx, PluginResponse::Finished).await;
        let _ = self.tx.flush();
    }
}
