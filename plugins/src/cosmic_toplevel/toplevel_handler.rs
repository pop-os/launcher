use std::collections::HashSet;

use cctk::{
    cosmic_protocols,
    screencopy::{
        CaptureFrame, CaptureSession, FailureReason, Formats, Frame, ScreencopyHandler,
        ScreencopyState,
    },
    toplevel_info::{ToplevelInfo, ToplevelInfoHandler, ToplevelInfoState},
    toplevel_management::{ToplevelManagerHandler, ToplevelManagerState},
    wayland_client::{self, Proxy, WEnum, protocol::wl_shm},
    wayland_protocols::ext::foreign_toplevel_list::v1::client::ext_foreign_toplevel_handle_v1::ExtForeignToplevelHandleV1,
};
use sctk::{
    self,
    reexports::{
        calloop, calloop_wayland_source::WaylandSource, client::protocol::wl_seat::WlSeat,
    },
    seat::{SeatHandler, SeatState},
    shm::{Shm, ShmHandler, slot::SlotPool},
};

use cosmic_protocols::{
    toplevel_info::v1::client::zcosmic_toplevel_handle_v1::ZcosmicToplevelHandleV1,
    toplevel_management::v1::client::zcosmic_toplevel_manager_v1,
};
use futures::channel::mpsc::UnboundedSender;
use sctk::registry::{ProvidesRegistryState, RegistryState};
use tracing::{debug, warn};
use wayland_client::{Connection, QueueHandle, globals::registry_queue_init};

#[derive(Debug, Clone)]
pub enum ToplevelAction {
    Activate(ExtForeignToplevelHandleV1),
    Close(ExtForeignToplevelHandleV1),
    RefreshThumbnail(ExtForeignToplevelHandleV1),
}

#[derive(Clone)]
pub struct ToplevelEntry {
    pub info: ToplevelInfo,
    pub thumbnail: Option<pop_launcher::ThumbnailData>,
}

pub enum ToplevelUpdate {
    Info(ToplevelEntry),
    ThumbnailReady {
        window_id: u32,
        thumbnail: pop_launcher::ThumbnailData,
    },
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
    thumbnail_capture: super::thumbnail_capture::ThumbnailCaptureState,
    shm_state: Shm,
    screencopy_state: ScreencopyState,
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
        self.thumbnail_capture
            .invalidate(toplevel.id().protocol_id());
        self.pending_update.insert(toplevel.clone());
    }

    fn info_done(&mut self, conn: &Connection, qh: &QueueHandle<Self>) {
        let res = self
            .pending_update
            .drain()
            .map(|handle| match self.toplevel_info_state.info(&handle) {
                Some(info) => {
                    let capturer = self.screencopy_state.capturer().clone();

                    self.thumbnail_capture
                        .request_capture(&info.foreign_toplevel, &capturer, qh);

                    if let Err(err) = conn.flush() {
                        self.thumbnail_capture
                            .clear_in_flight(info.foreign_toplevel.id().protocol_id());
                        warn!("flush failed after request_capture: error={err:?}");
                    }
                    ToplevelUpdate::Info(ToplevelEntry {
                        info: info.clone(),
                        thumbnail: self.thumbnail_capture.thumbnail_for(&info.foreign_toplevel),
                    })
                }
                None => {
                    self.thumbnail_capture.invalidate(handle.id().protocol_id());
                    ToplevelUpdate::Remove(handle)
                }
            })
            .collect();

        if let Err(err) = self.tx.unbounded_send(res) {
            warn!("{err}");
        }
    }
}

impl ShmHandler for AppData {
    fn shm_state(&mut self) -> &mut Shm {
        &mut self.shm_state
    }
}

impl ScreencopyHandler for AppData {
    fn screencopy_state(&mut self) -> &mut ScreencopyState {
        &mut self.screencopy_state
    }

    fn init_done(
        &mut self,
        conn: &Connection,
        qh: &QueueHandle<Self>,
        session: &CaptureSession,
        formats: &Formats,
    ) {
        let Some(data) = session.data::<super::thumbnail_capture::ThumbnailSessionData>() else {
            warn!("init_done received without thumbnail session data");
            return;
        };

        let window_id = data.window_id();

        if !formats.shm_formats.contains(&wl_shm::Format::Abgr8888) {
            self.thumbnail_capture.clear_in_flight(window_id);
            debug!("capture skipped: window_id={window_id}, Abgr8888 not supported");
            return;
        }

        let (width, height) = formats.buffer_size;
        if width == 0 || height == 0 {
            self.thumbnail_capture.clear_in_flight(window_id);
            debug!(
                "capture skipped: window_id={window_id}, invalid buffer size=({width}, {height})"
            );
            return;
        }

        let stride = width * 4;
        let size = stride * height;

        let mut pool = match SlotPool::new(size as usize, &self.shm_state) {
            Ok(pool) => pool,
            Err(err) => {
                self.thumbnail_capture.clear_in_flight(window_id);
                warn!("SlotPool::new failed: window_id={window_id}, error={err:?}");
                return;
            }
        };
        let buffer = match pool.create_buffer(
            width as i32,
            height as i32,
            stride as i32,
            wl_shm::Format::Abgr8888,
        ) {
            Ok((buffer, canvas)) => {
                canvas.fill(0);
                buffer
            }
            Err(err) => {
                self.thumbnail_capture.clear_in_flight(window_id);
                warn!("create_buffer failed: window_id={window_id}, error={err:?}");
                return;
            }
        };

        if let Err(err) = buffer.activate() {
            self.thumbnail_capture.clear_in_flight(window_id);
            warn!("buffer activate failed: window_id={window_id}, error={err:?}");
            return;
        }

        session.capture(
            buffer.wl_buffer(),
            &[],
            qh,
            super::thumbnail_capture::ThumbnailFrameData::new(window_id),
        );

        self.thumbnail_capture.mark_capturing(
            window_id,
            session.clone(),
            pool,
            buffer,
            width,
            height,
            stride,
        );

        if let Err(err) = conn.flush() {
            self.thumbnail_capture.clear_in_flight(window_id);
            warn!("flush failed after capture: window_id={window_id}, error={err:?}");
        }
    }

    fn stopped(&mut self, _conn: &Connection, _qh: &QueueHandle<Self>, session: &CaptureSession) {
        if let Some(data) = session.data::<super::thumbnail_capture::ThumbnailSessionData>() {
            self.thumbnail_capture.clear_in_flight(data.window_id());
            debug!("thumbnail capture stopped: window_id={}", data.window_id());
        } else {
            debug!("thumbnail capture stopped without session data");
        }
    }

    fn ready(
        &mut self,
        _conn: &Connection,
        _qh: &QueueHandle<Self>,
        frame: &CaptureFrame,
        _metadata: Frame,
    ) {
        if let Some(data) = frame.data::<super::thumbnail_capture::ThumbnailFrameData>() {
            let window_id = data.window_id();
            if let Some(thumbnail) = self.thumbnail_capture.mark_ready_from_capture(window_id)
                && let Err(err) = self.tx.unbounded_send(vec![ToplevelUpdate::ThumbnailReady {
                    window_id,
                    thumbnail,
                }])
            {
                warn!("{err}");
            }
        } else {
            warn!("ready received without thumbnail frame data");
        }
    }

    fn failed(
        &mut self,
        _conn: &Connection,
        _qh: &QueueHandle<Self>,
        frame: &CaptureFrame,
        reason: WEnum<FailureReason>,
    ) {
        if let Some(data) = frame.data::<super::thumbnail_capture::ThumbnailFrameData>() {
            self.thumbnail_capture.clear_in_flight(data.window_id());
            debug!(
                "thumbnail capture failed: window_id={}, reason={reason:?}",
                data.window_id()
            );
        } else {
            debug!("thumbnail capture failed without frame data: reason={reason:?}");
        }
    }
}

pub(crate) fn toplevel_handler(
    tx: UnboundedSender<Vec<ToplevelUpdate>>,
    rx: calloop::channel::Channel<ToplevelAction>,
) -> anyhow::Result<()> {
    let conn = Connection::connect_to_env()?;
    let conn_for_actions = conn.clone();

    let (globals, event_queue) = registry_queue_init(&conn)?;
    let mut event_loop = calloop::EventLoop::<AppData>::try_new()?;
    let qh = event_queue.handle();
    let qh_for_actions = qh.clone();

    let wayland_source = WaylandSource::new(conn, event_queue);
    let handle = event_loop.handle();

    handle.insert_source(wayland_source, |_, q, state| q.dispatch_pending(state))?;

    let _ = handle.insert_source(rx, move |event, _, state| match event {
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
            ToplevelAction::RefreshThumbnail(handle) => {
                let window_id = handle.id().protocol_id();

                state.thumbnail_capture.invalidate(window_id);

                let capturer = state.screencopy_state.capturer().clone();

                state
                    .thumbnail_capture
                    .request_capture(&handle, &capturer, &qh_for_actions);

                if let Err(err) = conn_for_actions.flush() {
                    state.thumbnail_capture.clear_in_flight(window_id);
                    warn!(
                        "flush failed after RefreshThumbnail: window_id={window_id}, error={err:?}"
                    );
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
        thumbnail_capture: super::thumbnail_capture::ThumbnailCaptureState::new(),
        shm_state: Shm::bind(&globals, &qh).expect("failed to bind shm"),
        screencopy_state: ScreencopyState::new(&globals, &qh),
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
sctk::delegate_shm!(AppData);
cctk::delegate_screencopy!(AppData);
