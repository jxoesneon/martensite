//! Tier-1 async GPU→CPU readback: `RenderingDevice::texture_get_data_async`
//! wrappers that turn `PackedByteArray` callbacks into [`FrameMsg`]s and
//! push them through a [`FrameTransport`].
//!
//! # Threading contract
//!
//! Everything here is **single-threaded on purpose**. The async readback
//! callable is built with [`Callable::from_fn`], which Godot may only
//! invoke on the thread that created it — the extension always creates
//! it on the main thread (from `MartensiteViewport`'s `process`), which
//! is where Godot runs the readback callbacks at the frame boundary.
//! Shared state therefore lives in `Rc<RefCell<…>>`, not `Arc<Mutex<…>>`.
//!
//! If a future Godot version invokes these callbacks on the render
//! thread, the correct fix is `godot`'s `experimental-threads` feature
//! plus `Callable::from_sync_fn` — not silent data races.

use std::cell::RefCell;
use std::rc::Rc;
use std::time::Instant;

use godot::builtin::{Callable, PackedByteArray, Rid, Variant};
use godot::classes::RenderingDevice;
use godot::global::Error as GodotError;

use crate::transport::{FrameMsg, FrameTransport};

/// Hard cap on [`FrameMsg`]-sized allocations defended by the transport
/// codec; re-exported here so the readback side shares the bound.
pub use martensite_engine_bridge::MAX_FRAME_DIM;

/// Snapshot of readback accounting, surfaced to GDScript via
/// `MartensiteViewport.stats()` — the honesty instrumentation: every
/// frame counts a copy and every millisecond of latency is measurable.
#[derive(Copy, Clone, Debug, Default)]
pub struct ReadbackStats {
    /// Frames successfully handed to the transport.
    pub sent: u64,
    /// Frames dropped because the pixel payload failed validation.
    pub dropped: u64,
    /// Errors from `texture_get_data_async` or the transport.
    pub errors: u64,
    /// Async reads currently outstanding.
    pub in_flight: u32,
    /// Mean request→callback latency in microseconds (0 when `sent == 0`).
    pub avg_readback_us: u64,
}

struct Shared {
    transport: Option<Box<dyn FrameTransport>>,
    in_flight: u32,
    max_in_flight: u32,
    seq: u64,
    sent: u64,
    dropped: u64,
    errors: u64,
    total_readback_us: u64,
}

/// Manages outstanding `texture_get_data_async` requests for one
/// viewport. Cloneable (the clone shares state — used internally to
/// hand the callback a handle); not `Send`, matching the single-thread
/// contract above.
#[derive(Clone)]
pub struct Readback {
    shared: Rc<RefCell<Shared>>,
}

impl Default for Readback {
    fn default() -> Self {
        Self::new(2)
    }
}

impl Readback {
    /// Creates a readback manager allowing `max_in_flight` concurrent
    /// async reads (Godot's `texture_get_data_async` latency is the
    /// driver frame-queue depth, so a small bound already pipelines well;
    /// 2 is the default).
    pub fn new(max_in_flight: u32) -> Self {
        Self {
            shared: Rc::new(RefCell::new(Shared {
                transport: None,
                in_flight: 0,
                max_in_flight,
                seq: 0,
                sent: 0,
                dropped: 0,
                errors: 0,
                total_readback_us: 0,
            })),
        }
    }

    /// Installs (or replaces) the transport frames are sent on.
    pub fn set_transport(&mut self, transport: Option<Box<dyn FrameTransport>>) {
        self.shared.borrow_mut().transport = transport;
    }

    /// Adjusts the bound on outstanding async reads (clamped to ≥ 1).
    pub fn set_max_in_flight(&mut self, max_in_flight: u32) {
        self.shared.borrow_mut().max_in_flight = max_in_flight.max(1);
    }

    /// `true` when a transport is connected — without one, requests are
    /// still issued (the callback fires) but frames count as dropped.
    pub fn has_transport(&self) -> bool {
        self.shared.borrow().transport.is_some()
    }

    /// `true` when fewer than `max_in_flight` reads are outstanding.
    pub fn has_capacity(&self) -> bool {
        let s = self.shared.borrow();
        s.in_flight < s.max_in_flight
    }

    /// Current statistics snapshot.
    pub fn stats(&self) -> ReadbackStats {
        let s = self.shared.borrow();
        ReadbackStats {
            sent: s.sent,
            dropped: s.dropped,
            errors: s.errors,
            in_flight: s.in_flight,
            avg_readback_us: s.total_readback_us.checked_div(s.sent).unwrap_or(0),
        }
    }

    /// Issues one async readback of `texture` (an RD `Rid` — already
    /// mapped through `texture_get_rd_texture`) at `w`×`h` RGBA8.
    ///
    /// Returns `false` (and records a drop) when the request was skipped:
    /// no capacity, or `texture_get_data_async` itself failed. The
    /// callback ships the resulting [`FrameMsg`] on the transport when it
    /// fires — typically `frame_queue_size` frames later.
    ///
    /// When `flip_y` is set, rows are swapped so row 0 of the shipped
    /// frame is the image's bottom row — Godot renders with a top-left
    /// origin while the host composites top-down.
    pub fn request(
        &mut self,
        rd: &mut RenderingDevice,
        texture: Rid,
        w: u32,
        h: u32,
        flip_y: bool,
    ) -> bool {
        {
            let mut s = self.shared.borrow_mut();
            if s.in_flight >= s.max_in_flight {
                s.dropped += 1;
                return false;
            }
            s.in_flight += 1;
        }

        let shared = Rc::clone(&self.shared);
        let seq = self.shared.borrow().seq;
        self.shared.borrow_mut().seq = seq + 1;
        let requested_at = Instant::now();

        let callback = Callable::from_fn(
            "martensite_godot_readback",
            move |args: &[&Variant]| -> Variant {
                let mut s = shared.borrow_mut();
                s.in_flight = s.in_flight.saturating_sub(1);
                s.total_readback_us += requested_at.elapsed().as_micros() as u64;

                let Some(msg) = args
                    .first()
                    .and_then(|v| v.try_to::<PackedByteArray>().ok())
                    .and_then(|bytes| frame_msg(&bytes, w, h, seq, flip_y))
                else {
                    s.dropped += 1;
                    return Variant::nil();
                };

                match s.transport.as_mut().map(|t| t.send(&msg)) {
                    Some(Ok(())) => s.sent += 1,
                    Some(Err(_)) => s.errors += 1,
                    None => s.dropped += 1, // no transport connected
                }
                Variant::nil()
            },
        );

        let err = rd.texture_get_data_async(texture, 0, &callback);
        if err != GodotError::OK {
            let mut s = self.shared.borrow_mut();
            s.in_flight = s.in_flight.saturating_sub(1);
            s.errors += 1;
            return false;
        }
        true
    }

    /// Synchronous fallback: `texture_get_data` blocks the GPU until the
    /// pixels land. Available for debugging/bring-up; the async path is
    /// the production one.
    pub fn request_blocking(
        &mut self,
        rd: &mut RenderingDevice,
        texture: Rid,
        w: u32,
        h: u32,
        flip_y: bool,
    ) -> bool {
        let bytes = rd.texture_get_data(texture, 0);
        let seq = {
            let mut s = self.shared.borrow_mut();
            let seq = s.seq;
            s.seq += 1;
            seq
        };
        let Some(msg) = frame_msg(&bytes, w, h, seq, flip_y) else {
            self.shared.borrow_mut().dropped += 1;
            return false;
        };
        let mut s = self.shared.borrow_mut();
        match s.transport.as_mut().map(|t| t.send(&msg)) {
            Some(Ok(())) => {
                s.sent += 1;
                true
            }
            Some(Err(_)) => {
                s.errors += 1;
                false
            }
            None => {
                s.dropped += 1;
                false
            }
        }
    }
}

/// Converts a `texture_get_data*` payload into a [`FrameMsg`],
/// validating `width * height * 4` against the byte count and applying
/// the optional vertical flip. `None` when the payload doesn't match —
/// e.g. Godot returned an empty array for an invalid `Rid`.
pub fn frame_msg(
    bytes: &PackedByteArray,
    width: u32,
    height: u32,
    seq: u64,
    flip_y: bool,
) -> Option<FrameMsg> {
    let mut pixels = bytes.as_slice().to_vec();
    if flip_y {
        flip_rows(&mut pixels, width as usize, height as usize);
    }
    FrameMsg::rgba8(width, height, seq, pixels).ok()
}

/// Swaps image rows in place so row 0 becomes the last row — RGBA8,
/// tightly packed. No-op on malformed lengths (callers validate).
pub fn flip_rows(pixels: &mut [u8], width: usize, height: usize) {
    let row_len = width * 4;
    if height < 2 || pixels.len() != row_len * height {
        return;
    }
    for y in 0..height / 2 {
        let top = y * row_len;
        let bottom = (height - 1 - y) * row_len;
        for i in 0..row_len {
            pixels.swap(top + i, bottom + i);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn flip_rows_swaps_top_and_bottom() {
        // 2x2: rows [1,2,3,4] and [5,6,7,8] (u8 pixels, w=1px = 4 bytes).
        let mut px = vec![1, 1, 1, 1, 2, 2, 2, 2];
        flip_rows(&mut px, 1, 2);
        assert_eq!(px, vec![2, 2, 2, 2, 1, 1, 1, 1]);
    }

    #[test]
    fn flip_rows_noop_on_bad_len() {
        let mut px = vec![0u8; 10];
        flip_rows(&mut px, 3, 3);
        assert_eq!(px, vec![0u8; 10]);
    }

    #[test]
    fn flip_rows_single_row_noop() {
        let mut px = vec![9u8; 8];
        flip_rows(&mut px, 2, 1);
        assert_eq!(px, vec![9u8; 8]);
    }

    #[test]
    fn stats_default_zero() {
        let rb = Readback::default();
        let s = rb.stats();
        assert_eq!(s.sent, 0);
        assert_eq!(s.in_flight, 0);
        assert!(!rb.has_transport());
        assert!(rb.has_capacity());
    }
}
