mod toplevel_handler;

use cctk::cosmic_protocols::toplevel_info::v1::client::zcosmic_toplevel_handle_v1::State;
use cctk::toplevel_info::ToplevelInfo;
use cctk::wayland_client::Proxy;
use cctk::sctk::reexports::calloop;
use fde::DesktopEntry;
use freedesktop_desktop_entry as fde;
use std::collections::HashSet;
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
    IconSource, PluginResponse, PluginSearchResult, Request, WorkspaceFilter, async_stdin,
    async_stdout, json_input_stream,
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
                            app.search(&query, WorkspaceFilter::All).await;
                            app.ids_to_ignore.clear();
                        }
                        Request::SearchFiltered {
                            query,
                            workspace_filter,
                        } => {
                            debug!("searching {query} with workspace filter {workspace_filter:?}");
                            app.search(&query, workspace_filter).await;
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
                        ToplevelUpdate::Info {
                            info,
                            workspace_coordinates,
                        } => {
                            let entry = ToplevelEntry {
                                info,
                                workspace_coordinates,
                            };
                            if let Some(pos) = app
                                .toplevels
                                .iter()
                                .position(|t| t.info.foreign_toplevel == entry.info.foreign_toplevel)
                            {
                                if entry.info.state.contains(&State::Activated) {
                                    app.toplevels.remove(pos);
                                    app.toplevels.push(entry);
                                } else {
                                    app.toplevels[pos] = entry;
                                }
                            } else {
                                app.toplevels.push(entry);
                            }
                        }
                        ToplevelUpdate::Remove(foreign_toplevel) => {
                            if let Some(pos) = app
                                .toplevels
                                .iter()
                                .position(|t| t.info.foreign_toplevel == foreign_toplevel)
                            {
                                app.toplevels.remove(pos);
                                app.ids_to_ignore.push(foreign_toplevel.id().protocol_id());
                            } else {
                                warn!("no toplevel to remove");
                            }
                        }
                        ToplevelUpdate::ActiveWorkspaces(active_workspace_coordinates) => {
                            app.active_workspace_coordinates = active_workspace_coordinates;
                            if let Some(query) = app.pending_workspace_search.take() {
                                app.search(&query, WorkspaceFilter::Current).await;
                            }
                        }
                    }
                }
            }
            _ => break,
        }
    }
}

struct ToplevelEntry {
    info: ToplevelInfo,
    workspace_coordinates: HashSet<Vec<u32>>,
}

struct App<W> {
    locales: Vec<String>,
    desktop_entries: Vec<DesktopEntry>,
    ids_to_ignore: Vec<u32>,
    toplevels: Vec<ToplevelEntry>,
    active_workspace_coordinates: HashSet<Vec<u32>>,
    pending_workspace_search: Option<String>,
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
                active_workspace_coordinates: HashSet::new(),
                pending_workspace_search: None,
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
            if t.info.foreign_toplevel.id().protocol_id() == id {
                Some(t.info.foreign_toplevel.clone())
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
            if t.info.foreign_toplevel.id().protocol_id() == id {
                Some(t.info.foreign_toplevel.clone())
            } else {
                None
            }
        }) {
            let _res = self.calloop_tx.send(ToplevelAction::Close(handle));
        }
    }

    fn matches_workspace_filter(
        &self,
        entry: &ToplevelEntry,
        workspace_filter: WorkspaceFilter,
    ) -> bool {
        matches_workspace_filter(
            &self.active_workspace_coordinates,
            &entry.workspace_coordinates,
            workspace_filter,
        )
    }

    async fn search(&mut self, query: &str, workspace_filter: WorkspaceFilter) {
        if workspace_filter == WorkspaceFilter::Current
            && self.active_workspace_coordinates.is_empty()
        {
            debug!(
                "deferring workspace-filtered search until active workspaces are known"
            );
            self.pending_workspace_search = Some(query.to_owned());
            send(&mut self.tx, PluginResponse::Finished).await;
            let _ = self.tx.flush().await;
            return;
        }

        self.pending_workspace_search = None;

        let matched = self
            .toplevels
            .iter()
            .filter(|t| self.matches_workspace_filter(t, workspace_filter))
            .count();
        debug!(
            "workspace search: filter={workspace_filter:?} active_coords={:?} toplevels={} matched={}",
            self.active_workspace_coordinates,
            self.toplevels.len(),
            matched
        );

        fn contains_pattern(needle: &str, haystack: &[&str]) -> bool {
            let needle = needle.to_ascii_lowercase();
            haystack.iter().all(|h| needle.contains(h))
        }

        let query = query.to_ascii_lowercase();
        let haystack = query.split_ascii_whitespace().collect::<Vec<&str>>();

        for toplevel in &self.toplevels {
            if !self.matches_workspace_filter(toplevel, workspace_filter) {
                continue;
            }

            let info = &toplevel.info;
            let retain = query.is_empty()
                || contains_pattern(&info.app_id, &haystack)
                || contains_pattern(&info.title, &haystack);

            if !retain {
                continue;
            }

            let appid = fde::unicase::Ascii::new(info.app_id.as_str());

            let desktop_entry = fde::find_app_by_id(&self.desktop_entries, appid)
                .map(ToOwned::to_owned)
                .unwrap_or_else(|| fde::DesktopEntry::from_appid(appid.to_string()).to_owned());

            let icon_name = if let Some(icon) = desktop_entry.icon() {
                Cow::Owned(icon.to_owned())
            } else {
                Cow::Borrowed("application-x-executable")
            };

            let response = PluginResponse::Append(PluginSearchResult {
                id: info.foreign_toplevel.id().protocol_id(),
                window: Some((0, info.foreign_toplevel.id().protocol_id())),
                description: info.title.clone(),
                name: get_description(&desktop_entry, &self.locales),
                icon: Some(IconSource::Name(icon_name)),
                ..Default::default()
            });

            send(&mut self.tx, response).await;
        }

        send(&mut self.tx, PluginResponse::Finished).await;
        let _ = self.tx.flush().await;
    }
}

fn matches_workspace_filter(
    active_workspace_coordinates: &HashSet<Vec<u32>>,
    entry_workspace_coordinates: &HashSet<Vec<u32>>,
    workspace_filter: WorkspaceFilter,
) -> bool {
    if workspace_filter == WorkspaceFilter::All {
        return true;
    }

    if active_workspace_coordinates.is_empty() || entry_workspace_coordinates.is_empty() {
        return false;
    }

    entry_workspace_coordinates
        .iter()
        .any(|coords| active_workspace_coordinates.contains(coords))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn coords(values: &[&[u32]]) -> HashSet<Vec<u32>> {
        values.iter().map(|coords| (*coords).to_vec()).collect()
    }

    #[test]
    fn all_filter_matches_everything() {
        let active = coords(&[&[1]]);
        let entry = coords(&[&[2]]);
        assert!(matches_workspace_filter(
            &active,
            &entry,
            WorkspaceFilter::All
        ));
    }

    #[test]
    fn current_filter_matches_shared_coordinates() {
        let active = coords(&[&[1, 2]]);
        let entry = coords(&[&[1, 2], &[3]]);
        assert!(matches_workspace_filter(
            &active,
            &entry,
            WorkspaceFilter::Current
        ));
    }

    #[test]
    fn current_filter_rejects_other_workspaces() {
        let active = coords(&[&[1]]);
        let entry = coords(&[&[2]]);
        assert!(!matches_workspace_filter(
            &active,
            &entry,
            WorkspaceFilter::Current
        ));
    }

    #[test]
    fn current_filter_rejects_missing_metadata() {
        let active = coords(&[&[1]]);
        let entry = coords(&[]);
        assert!(!matches_workspace_filter(
            &active,
            &entry,
            WorkspaceFilter::Current
        ));
        assert!(!matches_workspace_filter(
            &coords(&[]),
            &coords(&[&[1]]),
            WorkspaceFilter::Current
        ));
    }
}