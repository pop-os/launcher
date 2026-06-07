use std::collections::{HashMap, HashSet};

use cctk::{
    cosmic_protocols,
    toplevel_info::{ToplevelInfo, ToplevelInfoHandler, ToplevelInfoState},
    toplevel_management::{ToplevelManagerHandler, ToplevelManagerState},
    wayland_client::{self, Proxy, WEnum},
    wayland_protocols::ext::{
        foreign_toplevel_list::v1::client::ext_foreign_toplevel_handle_v1::ExtForeignToplevelHandleV1,
        workspace::v1::client::ext_workspace_handle_v1,
    },
    workspace::{WorkspaceHandler, WorkspaceState},
};
use sctk::{
    self,
    reexports::{
        calloop, calloop_wayland_source::WaylandSource, client::protocol::wl_seat::WlSeat,
    },
    seat::{SeatHandler, SeatState},
};

use cosmic_protocols::{
    toplevel_info::v1::client::zcosmic_toplevel_handle_v1::ZcosmicToplevelHandleV1,
    toplevel_management::v1::client::zcosmic_toplevel_manager_v1,
};
use futures::channel::mpsc::UnboundedSender;
use sctk::registry::{ProvidesRegistryState, RegistryState};
use tracing::warn;
use wayland_client::{Connection, QueueHandle, globals::registry_queue_init};

#[derive(Debug, Clone)]
pub enum ToplevelAction {
    Activate(ExtForeignToplevelHandleV1),
    Close(ExtForeignToplevelHandleV1),
}

pub enum ToplevelUpdate {
    Info {
        info: ToplevelInfo,
        workspace_coordinates: HashSet<Vec<u32>>,
    },
    Remove(ExtForeignToplevelHandleV1),
    /// Coordinates of currently active workspace(s).
    ActiveWorkspaces(HashSet<Vec<u32>>),
}

struct AppData {
    exit: bool,
    tx: UnboundedSender<Vec<ToplevelUpdate>>,
    registry_state: RegistryState,
    toplevel_info_state: ToplevelInfoState,
    toplevel_manager_state: ToplevelManagerState,
    workspace_state: WorkspaceState,
    seat_state: SeatState,
    pending_update: HashSet<ExtForeignToplevelHandleV1>,
    /// Workspace coordinates keyed by handle protocol id.
    ///
    /// Toplevel and workspace protocols may hand out different handles for the
    /// same workspace, so we track coordinates per handle as events arrive.
    workspace_coords_by_id: HashMap<u32, Vec<u32>>,
}

impl AppData {
    fn cosmic_toplevel_for_foreign(
        &self,
        foreign_toplevel: &ExtForeignToplevelHandleV1,
    ) -> Option<&ZcosmicToplevelHandleV1> {
        self.toplevel_info_state
            .info(foreign_toplevel)?
            .cosmic_toplevel
            .as_ref()
    }

    fn active_workspace_coordinates(&self) -> HashSet<Vec<u32>> {
        self.workspace_state
            .workspaces()
            .filter(|workspace| workspace.state.contains(ext_workspace_handle_v1::State::Active))
            .map(|workspace| workspace.coordinates.clone())
            .collect()
    }

    fn sync_workspace_coords(&mut self) {
        for workspace in self.workspace_state.workspaces() {
            self.workspace_coords_by_id.insert(
                workspace.handle.id().protocol_id(),
                workspace.coordinates.clone(),
            );
        }
    }

    fn workspace_coordinates(&self, info: &ToplevelInfo) -> HashSet<Vec<u32>> {
        let mut coordinates = info
            .workspace
            .iter()
            .filter_map(|handle| {
                self.workspace_state
                    .workspace_info(handle)
                    .map(|workspace| workspace.coordinates.clone())
                    .or_else(|| {
                        self.workspace_coords_by_id
                            .get(&handle.id().protocol_id())
                            .cloned()
                    })
            })
            .collect::<HashSet<_>>();

        if coordinates.is_empty() && !info.workspace.is_empty() {
            for workspace in self.workspace_state.workspaces() {
                if info.workspace.iter().any(|handle| {
                    workspace.handle.id().protocol_id() == handle.id().protocol_id()
                }) {
                    coordinates.insert(workspace.coordinates.clone());
                }
            }
        }

        coordinates
    }

    fn send_active_workspaces(&self) {
        if let Err(err) = self
            .tx
            .unbounded_send(vec![ToplevelUpdate::ActiveWorkspaces(
                self.active_workspace_coordinates(),
            )])
        {
            warn!("{err}");
        }
    }
}

impl ProvidesRegistryState for AppData {
    fn registry(&mut self) -> &mut RegistryState {
        &mut self.registry_state
    }

    sctk::registry_handlers!();
}

impl SeatHandler for AppData {
    fn seat_state(&mut self) -> &mut sctk::seat::SeatState {
        &mut self.seat_state
    }

    fn new_seat(&mut self, _: &Connection, _: &QueueHandle<Self>, _: WlSeat) {}

    fn new_capability(
        &mut self,
        _: &Connection,
        _: &QueueHandle<Self>,
        _: WlSeat,
        _: sctk::seat::Capability,
    ) {
    }

    fn remove_capability(
        &mut self,
        _: &Connection,
        _: &QueueHandle<Self>,
        _: WlSeat,
        _: sctk::seat::Capability,
    ) {
    }

    fn remove_seat(&mut self, _: &Connection, _: &QueueHandle<Self>, _: WlSeat) {}
}

impl ToplevelManagerHandler for AppData {
    fn toplevel_manager_state(&mut self) -> &mut cctk::toplevel_management::ToplevelManagerState {
        &mut self.toplevel_manager_state
    }

    fn capabilities(
        &mut self,
        _: &Connection,
        _: &QueueHandle<Self>,
        _: Vec<WEnum<zcosmic_toplevel_manager_v1::ZcosmicToplelevelManagementCapabilitiesV1>>,
    ) {
    }
}

impl ToplevelInfoHandler for AppData {
    fn toplevel_info_state(&mut self) -> &mut ToplevelInfoState {
        &mut self.toplevel_info_state
    }

    fn new_toplevel(
        &mut self,
        _conn: &Connection,
        _qh: &QueueHandle<Self>,
        toplevel: &ExtForeignToplevelHandleV1,
    ) {
        self.pending_update.insert(toplevel.clone());
    }

    fn update_toplevel(
        &mut self,
        _conn: &Connection,
        _qh: &QueueHandle<Self>,
        toplevel: &ExtForeignToplevelHandleV1,
    ) {
        self.pending_update.insert(toplevel.clone());
    }

    fn toplevel_closed(
        &mut self,
        _conn: &Connection,
        _qh: &QueueHandle<Self>,
        toplevel: &ExtForeignToplevelHandleV1,
    ) {
        self.pending_update.insert(toplevel.clone());
    }

    fn info_done(&mut self, _conn: &Connection, _qh: &QueueHandle<Self>) {
        self.sync_workspace_coords();
        let pending = self.pending_update.drain().collect::<Vec<_>>();
        let mut res = Vec::with_capacity(pending.len());
        for handle in pending {
            match self.toplevel_info_state.info(&handle) {
                Some(info) => {
                    let workspace_coordinates = self.workspace_coordinates(info);
                    res.push(ToplevelUpdate::Info {
                        info: info.clone(),
                        workspace_coordinates,
                    });
                }
                None => res.push(ToplevelUpdate::Remove(handle)),
            }
        }

        res.push(ToplevelUpdate::ActiveWorkspaces(
            self.active_workspace_coordinates(),
        ));

        if let Err(err) = self.tx.unbounded_send(res) {
            warn!("{err}");
        }
    }
}

impl WorkspaceHandler for AppData {
    fn workspace_state(&mut self) -> &mut WorkspaceState {
        &mut self.workspace_state
    }

    fn done(&mut self) {
        self.sync_workspace_coords();
        self.send_active_workspaces();
    }
}

pub(crate) fn toplevel_handler(
    tx: UnboundedSender<Vec<ToplevelUpdate>>,
    rx: calloop::channel::Channel<ToplevelAction>,
) -> anyhow::Result<()> {
    let conn = Connection::connect_to_env()?;
    let (globals, event_queue) = registry_queue_init(&conn)?;
    let mut event_loop = calloop::EventLoop::<AppData>::try_new()?;
    let qh = event_queue.handle();
    let wayland_source = WaylandSource::new(conn, event_queue);
    let handle = event_loop.handle();

    handle.insert_source(wayland_source, |_, q, state| q.dispatch_pending(state))?;

    let _ = handle.insert_source(rx, |event, _, state| match event {
        calloop::channel::Event::Msg(req) => match req {
            ToplevelAction::Activate(handle) => {
                let manager = &state.toplevel_manager_state.manager;
                // TODO Ashley how to choose the seat in a multi-seat setup?
                if let Some(cosmic_toplevel) = state.cosmic_toplevel_for_foreign(&handle) {
                    for s in state.seat_state.seats() {
                        manager.activate(cosmic_toplevel, &s);
                    }
                }
            }
            ToplevelAction::Close(handle) => {
                let manager = &state.toplevel_manager_state.manager;
                if let Some(cosmic_toplevel) = state.cosmic_toplevel_for_foreign(&handle) {
                    manager.close(cosmic_toplevel);
                }
            }
        },
        calloop::channel::Event::Closed => {
            state.exit = true;
        }
    });

    let registry_state = RegistryState::new(&globals);
    let mut app_data = AppData {
        exit: false,
        tx,
        seat_state: SeatState::new(&globals, &qh),
        toplevel_info_state: ToplevelInfoState::new(&registry_state, &qh),
        toplevel_manager_state: ToplevelManagerState::new(&registry_state, &qh),
        workspace_state: WorkspaceState::new(&registry_state, &qh),
        registry_state,
        pending_update: HashSet::new(),
        workspace_coords_by_id: HashMap::new(),
    };

    loop {
        if app_data.exit {
            break Ok(());
        }
        event_loop.dispatch(None, &mut app_data)?;
    }
}

sctk::delegate_seat!(AppData);
sctk::delegate_registry!(AppData);
cctk::delegate_toplevel_info!(AppData);
cctk::delegate_toplevel_manager!(AppData);
cctk::delegate_workspace!(AppData);