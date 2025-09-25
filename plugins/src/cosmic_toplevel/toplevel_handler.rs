use std::collections::HashSet;

use cctk::{
    cosmic_protocols,
    toplevel_info::{ToplevelInfo, ToplevelInfoHandler, ToplevelInfoState},
    toplevel_management::{ToplevelManagerHandler, ToplevelManagerState},
    wayland_client::{self, WEnum},
    wayland_protocols::ext::foreign_toplevel_list::v1::client::ext_foreign_toplevel_handle_v1::ExtForeignToplevelHandleV1,
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
    Info(ToplevelInfo),
    Remove(ExtForeignToplevelHandleV1),
}

struct AppData {
    exit: bool,
    tx: UnboundedSender<Vec<ToplevelUpdate>>,
    registry_state: RegistryState,
    toplevel_info_state: ToplevelInfoState,
    toplevel_manager_state: ToplevelManagerState,
    seat_state: SeatState,
    pending_update: HashSet<ExtForeignToplevelHandleV1>,
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
        let res = self
            .pending_update
            .drain()
            .map(|handle| match self.toplevel_info_state.info(&handle) {
                Some(info) => ToplevelUpdate::Info(info.clone()),
                None => ToplevelUpdate::Remove(handle),
            })
            .collect();

        if let Err(err) = self.tx.unbounded_send(res) {
            warn!("{err}");
        }
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
        registry_state,
        pending_update: HashSet::new(),
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
