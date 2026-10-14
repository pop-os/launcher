use std::collections::HashMap;

use cctk::{
    GlobalData,
    screencopy::{
        CaptureOptions, CaptureSession, CaptureSource, Capturer, ScreencopyFrameData,
        ScreencopyFrameDataExt, ScreencopySessionData, ScreencopySessionDataExt,
    },
    wayland_client::{Dispatch, Proxy, QueueHandle},
    wayland_protocols::ext::{
        foreign_toplevel_list::v1::client::ext_foreign_toplevel_handle_v1::ExtForeignToplevelHandleV1,
        image_capture_source::v1::client::ext_image_capture_source_v1::ExtImageCaptureSourceV1,
        image_copy_capture::v1::client::ext_image_copy_capture_session_v1::ExtImageCopyCaptureSessionV1,
    },
};
use pop_launcher::ThumbnailData;
use sctk::shm::slot::{Buffer, SlotPool};
use tracing::warn;

const THUMBNAIL_TARGET_WIDTH: u32 = 160;

enum ThumbnailState {
    Pending {
        _session: CaptureSession,
    },
    Capturing {
        _session: CaptureSession,
        pool: SlotPool,
        buffer: Buffer,
        width: u32,
        height: u32,
        stride: u32,
    },
    Ready(ThumbnailData),
}

pub struct ThumbnailSessionData {
    window_id: u32,
    session_data: ScreencopySessionData,
}

impl ThumbnailSessionData {
    pub fn new(window_id: u32) -> Self {
        Self {
            window_id,
            session_data: ScreencopySessionData::default(),
        }
    }

    pub fn window_id(&self) -> u32 {
        self.window_id
    }
}

impl ScreencopySessionDataExt for ThumbnailSessionData {
    fn screencopy_session_data(&self) -> &ScreencopySessionData {
        &self.session_data
    }
}

pub struct ThumbnailCaptureState {
    cache: HashMap<u32, ThumbnailState>,
}

impl ThumbnailCaptureState {
    pub fn new() -> Self {
        Self {
            cache: HashMap::new(),
        }
    }

    pub fn thumbnail_for(
        &mut self,
        toplevel: &ExtForeignToplevelHandleV1,
    ) -> Option<ThumbnailData> {
        let id = toplevel.id().protocol_id();

        match self.cache.get(&id) {
            Some(ThumbnailState::Ready(thumbnail)) => Some(thumbnail.clone()),
            Some(ThumbnailState::Pending { .. })
            | Some(ThumbnailState::Capturing { .. })
            | None => None,
        }
    }

    pub fn request_capture<D>(
        &mut self,
        toplevel: &ExtForeignToplevelHandleV1,
        capturer: &Capturer,
        qh: &QueueHandle<D>,
    ) where
        D: 'static,
        D: Dispatch<ExtImageCaptureSourceV1, GlobalData>,
        D: Dispatch<ExtImageCopyCaptureSessionV1, ThumbnailSessionData>,
    {
        let id = toplevel.id().protocol_id();

        if self.cache.contains_key(&id) {
            return;
        }

        let source = CaptureSource::Toplevel(toplevel.clone());
        let options = CaptureOptions::empty();

        match capturer.create_session(&source, options, qh, ThumbnailSessionData::new(id)) {
            Ok(session) => {
                self.cache
                    .insert(id, ThumbnailState::Pending { _session: session });
            }
            Err(error) => {
                warn!("create_session failed: window_id={id}, error={error:?}");
            }
        }
    }

    pub fn mark_capturing(
        &mut self,
        window_id: u32,
        session: CaptureSession,
        pool: SlotPool,
        buffer: Buffer,
        width: u32,
        height: u32,
        stride: u32,
    ) {
        self.cache.insert(
            window_id,
            ThumbnailState::Capturing {
                _session: session,
                pool,
                buffer,
                width,
                height,
                stride,
            },
        );
    }

    pub fn mark_ready_from_capture(&mut self, window_id: u32) -> Option<ThumbnailData> {
        let Some(state) = self.cache.remove(&window_id) else {
            return None;
        };

        match state {
            ThumbnailState::Capturing {
                _session,
                mut pool,
                buffer,
                width,
                height,
                stride,
            } => {
                let raw_pixels = pool.raw_data_mut(&buffer.slot());

                let Some(thumbnail) =
                    build_thumbnail_rgba(raw_pixels, width, height, stride, THUMBNAIL_TARGET_WIDTH)
                else {
                    warn!("failed to build thumbnail for window_id={window_id}");
                    return None;
                };

                self.cache
                    .insert(window_id, ThumbnailState::Ready(thumbnail.clone()));
                Some(thumbnail)
            }
            other => {
                self.cache.insert(window_id, other);
                None
            }
        }
    }

    pub fn invalidate(&mut self, window_id: u32) {
        self.cache.remove(&window_id);
    }

    pub fn clear_in_flight(&mut self, window_id: u32) {
        if matches!(
            self.cache.get(&window_id),
            Some(ThumbnailState::Pending { .. } | ThumbnailState::Capturing { .. })
        ) {
            self.cache.remove(&window_id);
        }
    }
}

pub struct ThumbnailFrameData {
    window_id: u32,
    frame_data: ScreencopyFrameData,
}

impl ThumbnailFrameData {
    pub fn new(window_id: u32) -> Self {
        Self {
            window_id,
            frame_data: ScreencopyFrameData::default(),
        }
    }

    pub fn window_id(&self) -> u32 {
        self.window_id
    }
}

impl ScreencopyFrameDataExt for ThumbnailFrameData {
    fn screencopy_frame_data(&self) -> &ScreencopyFrameData {
        &self.frame_data
    }
}

fn build_thumbnail_rgba(
    raw_pixels: &[u8],
    src_width: u32,
    src_height: u32,
    src_stride: u32,
    target_width: u32,
) -> Option<ThumbnailData> {
    if src_width == 0 || src_height == 0 || target_width == 0 {
        return None;
    }

    let target_height =
        ((target_width as u64 * src_height as u64) / src_width as u64).max(1) as u32;

    let src_stride = src_stride as usize;
    let mut pixels = Vec::with_capacity((target_width * target_height * 4) as usize);

    for y in 0..target_height {
        let src_y = (y as u64 * src_height as u64 / target_height as u64) as usize;

        for x in 0..target_width {
            let src_x = (x as u64 * src_width as u64 / target_width as u64) as usize;
            let src_index = src_y * src_stride + src_x * 4;

            if src_index + 3 >= raw_pixels.len() {
                return None;
            }

            pixels.extend_from_slice(&raw_pixels[src_index..src_index + 4]);
        }
    }

    Some(ThumbnailData {
        width: target_width,
        height: target_height,
        pixels,
    })
}
