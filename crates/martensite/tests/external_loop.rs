//! Adversarial headless integration tests for the v0.14.0 external-surface
//! loop (`martensite-engine-bridge` + `martensite::widgets::external`).
//!
//! These tests drive the producer/consumer contract without a GPU:
//!
//! ```text
//! engine publishes → ring.mark_ready_* → take_front → composite → release
//!                  → drain_released → Engine::release (exactly once)
//! ```
//!
//! Coverage: mailbox lifecycle, stalled/overwriting producers, panic
//! quarantine, `InvalidPayload` validation, watermark bounds, and the
//! `cpu_frame_for` TinySkia-fallback resolver.
//!
//! NOTE: `ExternalEngines::render_frame`/`drive_frame` require an
//! `EngineContext` (`&wgpu::Device`/`&wgpu::Queue`). `wgpu::Device::noop`
//! only exists behind the `test-noop` feature, which no crate in
//! martensite's test dependency graph enables, so no context can be
//! constructed here. Panic-quarantine coverage therefore exercises the
//! context-free call sites (`drain_released`, `cpu_frame_for`); the
//! publish side is driven through the registry exactly as
//! `Engine::render` would drive it internally.
#![forbid(unsafe_code)]

use martensite::widgets::external::{BindError, ExternalEngine, ExternalEngines, FramePoll};
use martensite_engine_bridge::{
    BridgeError, BridgeHandle, BridgeRegistry, CpuFrame, Engine, EngineContext, EngineEvent, Frame,
    FrameSync, FrameToken, NativeFrame, PointerButton, SourceAlpha, SurfaceId, SurfaceRing,
    Viewport, MAX_FRAME_DIM,
};
use martensite_wgpu::wgpu;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};

// ───────────────────────────── helpers ─────────────────────────────

/// A `Frame` implementation with no GPU texture — stands in for
/// CPU-only/cross-device publications (and lets tests publish frames
/// without a `wgpu::Device`).
struct StubFrame {
    token: FrameToken,
    size: (u32, u32),
}

impl Frame for StubFrame {
    fn token(&self) -> FrameToken {
        self.token
    }
    fn same_device_texture(&self) -> Option<&wgpu::Texture> {
        None
    }
    fn native_handle(&self) -> Option<NativeFrame> {
        None
    }
    fn sync(&self) -> FrameSync {
        FrameSync::None
    }
    fn size(&self) -> (u32, u32) {
        self.size
    }
    fn alpha_mode(&self) -> SourceAlpha {
        SourceAlpha::Premultiplied
    }
}

/// An engine that records every released token. `render` returns `None`;
/// tests drive the publish step through the registry directly — the same
/// `acquire`/`mark_ready_*` calls a real engine makes inside `render`.
struct Rec {
    released: Arc<Mutex<Vec<FrameToken>>>,
}

impl Engine for Rec {
    fn render(&mut self, _c: &mut EngineContext, _v: Viewport) -> Option<Box<dyn Frame>> {
        None
    }
    fn release(&mut self, t: FrameToken) {
        self.released.lock().unwrap().push(t);
    }
}

/// An engine whose `release` always panics — the quarantine trigger.
struct FragileRelease {
    calls: Arc<AtomicUsize>,
}

impl Engine for FragileRelease {
    fn render(&mut self, _c: &mut EngineContext, _v: Viewport) -> Option<Box<dyn Frame>> {
        None
    }
    fn release(&mut self, _t: FrameToken) {
        self.calls.fetch_add(1, Ordering::SeqCst);
        panic!("producer release exploded");
    }
}

/// An engine that records every `on_event` input event (plus released
/// tokens, like `Rec`) — the `forward_event` delivery observer.
struct RecEvents {
    events: Arc<Mutex<Vec<EngineEvent>>>,
    released: Arc<Mutex<Vec<FrameToken>>>,
}

impl Engine for RecEvents {
    fn render(&mut self, _c: &mut EngineContext, _v: Viewport) -> Option<Box<dyn Frame>> {
        None
    }
    fn release(&mut self, t: FrameToken) {
        self.released.lock().unwrap().push(t);
    }
    fn on_event(&mut self, event: &EngineEvent) {
        self.events.lock().unwrap().push(event.clone());
    }
}

/// An engine whose `on_event` always panics — the input-path quarantine
/// trigger. `release` is well-behaved so tests can observe that the
/// quarantine skips it too.
struct FragileEvent {
    event_calls: Arc<AtomicUsize>,
    release_calls: Arc<AtomicUsize>,
}

impl Engine for FragileEvent {
    fn render(&mut self, _c: &mut EngineContext, _v: Viewport) -> Option<Box<dyn Frame>> {
        None
    }
    fn release(&mut self, _t: FrameToken) {
        self.release_calls.fetch_add(1, Ordering::SeqCst);
    }
    fn on_event(&mut self, _e: &EngineEvent) {
        self.event_calls.fetch_add(1, Ordering::SeqCst);
        panic!("producer on_event exploded");
    }
}

/// An engine whose `to_pixmap` always panics — quarantine trigger for
/// the `cpu_frame_for` resolver path.
struct FragilePixmap {
    release_calls: Arc<AtomicUsize>,
}

impl Engine for FragilePixmap {
    fn render(&mut self, _c: &mut EngineContext, _v: Viewport) -> Option<Box<dyn Frame>> {
        None
    }
    fn release(&mut self, _t: FrameToken) {
        self.release_calls.fetch_add(1, Ordering::SeqCst);
    }
    fn to_pixmap(&self, _t: FrameToken) -> Option<CpuFrame> {
        panic!("producer to_pixmap exploded");
    }
}

/// An engine that rasterizes on the CPU (`to_pixmap`) — the TinySkia
/// fallback producer.
struct RasterEngine {
    color: [u8; 4],
    pixmap_calls: Arc<AtomicUsize>,
}

impl Engine for RasterEngine {
    fn render(&mut self, _c: &mut EngineContext, _v: Viewport) -> Option<Box<dyn Frame>> {
        None
    }
    fn release(&mut self, _t: FrameToken) {}
    fn to_pixmap(&self, _t: FrameToken) -> Option<CpuFrame> {
        self.pixmap_calls.fetch_add(1, Ordering::SeqCst);
        let mut pixels = vec![0u8; 2 * 2 * 4];
        for px in pixels.as_chunks_mut::<4>().0 {
            px.copy_from_slice(&self.color);
        }
        Some(CpuFrame::new(2, 2, pixels))
    }
}

/// Registers a surface on a fresh handle and binds `engine` to it.
fn bound(
    handle: &BridgeHandle,
    engines: &mut ExternalEngines,
    engine: Box<dyn Engine>,
) -> SurfaceId {
    let surface = handle.lock().register();
    engines.bind(handle.clone(), surface, engine).unwrap();
    surface
}

/// Runs one full publish → ready → take → release cycle on `surface`,
/// returning the produced token.
fn publish_and_composite(
    handle: &BridgeHandle,
    surface: SurfaceId,
    size: (u32, u32),
) -> FrameToken {
    let mut reg = handle.lock();
    let (slot, token) = reg.acquire(surface).unwrap();
    reg.mark_ready_full(
        surface,
        slot,
        Some(Box::new(StubFrame { token, size })),
        None,
    )
    .unwrap();
    let (front, front_token) = reg.take_front(surface).unwrap().unwrap();
    assert_eq!(front_token, token);
    reg.release(surface, front).unwrap();
    token
}

// ──────────────── 1. full mailbox lifecycle ────────────────

#[test]
fn mailbox_lifecycle_release_fires_exactly_once_per_token() {
    let handle = BridgeHandle::new();
    let seen = Arc::new(Mutex::new(Vec::new()));
    let mut engines = ExternalEngines::new();
    let surface = bound(
        &handle,
        &mut engines,
        Box::new(Rec {
            released: Arc::clone(&seen),
        }),
    );

    // Producer publishes a frame payload + records readiness.
    let (slot, token) = handle.lock().acquire(surface).unwrap();
    handle
        .lock()
        .mark_ready_full(
            surface,
            slot,
            Some(Box::new(StubFrame {
                token,
                size: (64, 64),
            })),
            None,
        )
        .unwrap();

    // Damage signal: the ready event names the surface.
    assert_eq!(engines.drain_ready(), vec![surface]);
    assert!(engines.drain_ready().is_empty());

    // Host composites: take_front_frame moves the payload out of the
    // ring so the registry lock isn't held across the GPU pass.
    let taken = handle
        .lock()
        .take_front_frame(surface)
        .unwrap()
        .expect("front frame must be ready");
    assert_eq!(taken.token, token);
    let frame = taken.frame.expect("mark_ready_full published a frame");
    assert_eq!(frame.token(), token);
    assert_eq!(frame.size(), (64, 64));
    // The ring no longer holds the payload — it was moved, not copied.
    assert!(handle.lock().front_frame(surface).unwrap().is_none());
    drop(frame); // composite consumed it

    handle.lock().release(surface, taken.slot).unwrap();

    // Producer recycling: Engine::release fires exactly once.
    engines.drain_released();
    assert_eq!(*seen.lock().unwrap(), vec![token]);
    // A second drain must not double-release.
    engines.drain_released();
    assert_eq!(seen.lock().unwrap().len(), 1);
}

#[test]
fn mailbox_lifecycle_repeated_frames_release_in_order() {
    let handle = BridgeHandle::new();
    let seen = Arc::new(Mutex::new(Vec::new()));
    let mut engines = ExternalEngines::new();
    let surface = bound(
        &handle,
        &mut engines,
        Box::new(Rec {
            released: Arc::clone(&seen),
        }),
    );

    let t0 = publish_and_composite(&handle, surface, (64, 64));
    let t1 = publish_and_composite(&handle, surface, (64, 64));
    let t2 = publish_and_composite(&handle, surface, (64, 64));
    assert!(t0 < t1 && t1 < t2, "tokens must be strictly increasing");

    engines.drain_released();
    assert_eq!(*seen.lock().unwrap(), vec![t0, t1, t2]);
}

// ──────────────── 2. stalled producer ────────────────

#[test]
fn stalled_producer_reclaim_frees_slots_and_queues_tokens() {
    let handle = BridgeHandle::new();
    let seen = Arc::new(Mutex::new(Vec::new()));
    let mut engines = ExternalEngines::new();
    let surface = bound(
        &handle,
        &mut engines,
        Box::new(Rec {
            released: Arc::clone(&seen),
        }),
    );

    // Producer acquires both slots and vanishes without mark_ready.
    let stalled = {
        let mut reg = handle.lock();
        let (s0, t0) = reg.acquire(surface).unwrap();
        let (s1, t1) = reg.acquire(surface).unwrap();
        // Ring is now exhausted — a third acquire fails.
        assert!(matches!(
            reg.acquire(surface),
            Err(BridgeError::RingExhausted)
        ));
        // Host declares the producer stalled and reclaims the ring.
        assert_eq!(reg.reclaim_stalled(surface).unwrap(), 2);
        let _ = (s0, s1);
        [t0, t1]
    };

    // Slots are free again: a fresh acquire succeeds.
    let (slot, _t) = handle.lock().acquire(surface).unwrap();
    handle.lock().force_release(surface, slot).unwrap();

    // The abandoned tokens land in `released` — the producer recycles
    // its per-token resources instead of leaking them.
    engines.drain_released();
    let got = seen.lock().unwrap().clone();
    assert!(
        got.contains(&stalled[0]) && got.contains(&stalled[1]),
        "got {got:?}"
    );
}

#[test]
fn force_release_on_compositing_slot_is_refused() {
    // The host may still be sampling a Compositing slot's texture —
    // force_release must not free it.
    let mut reg = BridgeRegistry::new();
    let id = reg.register();
    let (slot, token) = reg.acquire(id).unwrap();
    reg.mark_ready(id, slot).unwrap();
    let (front, front_token) = reg.take_front(id).unwrap().unwrap();
    assert_eq!(front_token, token);

    assert!(!reg.force_release(id, slot).unwrap());
    // reclaim_stalled must leave the Compositing slot alone too.
    assert_eq!(reg.reclaim_stalled(id).unwrap(), 0);

    // The only valid path out of Compositing is release.
    reg.release(id, front).unwrap();
    assert_eq!(reg.drain_released(id).unwrap(), vec![token]);
}

#[test]
fn reclaim_stalled_frees_ready_but_not_compositing() {
    let mut reg = BridgeRegistry::new();
    let id = reg.register();
    // Slot A: full publish → compositing (host owns it now).
    let (sa, ta) = reg.acquire(id).unwrap();
    reg.mark_ready(id, sa).unwrap();
    reg.take_front(id).unwrap();
    // Slot B: acquired but producer stalled mid-write.
    let (_sb, tb) = reg.acquire(id).unwrap();
    assert!(matches!(reg.acquire(id), Err(BridgeError::RingExhausted)));

    // Only the Writing slot is reclaimed; the Compositing slot survives.
    assert_eq!(reg.reclaim_stalled(id).unwrap(), 1);
    assert_eq!(reg.drain_released(id).unwrap(), vec![tb]);
    // The composited slot still releases normally afterwards.
    reg.release(id, sa).unwrap();
    assert_eq!(reg.drain_released(id).unwrap(), vec![ta]);
}

// ──────────────── 3. overwritten ready frame ────────────────

#[test]
fn overwritten_ready_frame_token_is_released_not_leaked() {
    let handle = BridgeHandle::new();
    let seen = Arc::new(Mutex::new(Vec::new()));
    let mut engines = ExternalEngines::new();
    let surface = bound(
        &handle,
        &mut engines,
        Box::new(Rec {
            released: Arc::clone(&seen),
        }),
    );

    // Producer outpaces the host: two publishes before a single take.
    let (t0, t1) = {
        let mut reg = handle.lock();
        let (s0, t0) = reg.acquire(surface).unwrap();
        reg.mark_ready(surface, s0).unwrap();
        let (s1, t1) = reg.acquire(surface).unwrap();
        reg.mark_ready(surface, s1).unwrap(); // mailbox drops s0's frame
        (t0, t1)
    };

    // Ready events dedupe consecutive same-surface signals — the host
    // gets one "surface has newer frame" notification, not two.
    assert_eq!(engines.drain_ready(), vec![surface]);

    // The host composites only the freshest frame.
    let (front, front_token) = handle.lock().take_front(surface).unwrap().unwrap();
    assert_eq!(front_token, t1);
    handle.lock().release(surface, front).unwrap();

    // Both tokens are recycled: the dropped frame's token was queued to
    // `released` (before the composited one), not leaked.
    engines.drain_released();
    assert_eq!(*seen.lock().unwrap(), vec![t0, t1]);
}

// ──────────────── 4. panic quarantine ────────────────
//
// `render_frame` needs an EngineContext (wgpu device/queue) which cannot
// be built headlessly here — `wgpu::Device::noop` is gated behind the
// `test-noop` feature that no martensite test dependency enables. The
// quarantine paths that take no context — `drain_released` and
// `cpu_frame_for` — are exercised instead.

#[test]
fn panicking_release_engine_is_quarantined() {
    let calls = Arc::new(AtomicUsize::new(0));
    let handle = BridgeHandle::new();
    let mut engines = ExternalEngines::new();
    let surface = bound(
        &handle,
        &mut engines,
        Box::new(FragileRelease {
            calls: Arc::clone(&calls),
        }),
    );

    // First released token: release() panics → quarantined.
    publish_and_composite(&handle, surface, (8, 8));
    engines.drain_released();
    assert_eq!(calls.load(Ordering::SeqCst), 1);

    // Subsequent tokens: the callback is skipped entirely, but the
    // registry's released queue is still drained so it stays bounded.
    publish_and_composite(&handle, surface, (8, 8));
    publish_and_composite(&handle, surface, (8, 8));
    engines.drain_released();
    assert_eq!(
        calls.load(Ordering::SeqCst),
        1,
        "quarantined engine must never be called again"
    );
    assert!(
        handle.lock().drain_released(surface).unwrap().is_empty(),
        "tokens must still be drained for a quarantined engine"
    );
}

#[test]
fn panicking_to_pixmap_quarantines_engine() {
    let release_calls = Arc::new(AtomicUsize::new(0));
    let handle = BridgeHandle::new();
    let mut engines = ExternalEngines::new();
    let surface = bound(
        &handle,
        &mut engines,
        Box::new(FragilePixmap {
            release_calls: Arc::clone(&release_calls),
        }),
    );

    // Publish without a ring CpuFrame → resolver falls back to
    // to_pixmap → panic → quarantined, resolver returns None.
    let token = publish_and_composite(&handle, surface, (8, 8));
    assert!(engines.cpu_frame_for(surface, token).is_none());

    // Quarantine sticks: a second resolver call doesn't retry the
    // engine, and drain_released skips its release callback while
    // still draining the token queue.
    publish_and_composite(&handle, surface, (8, 8));
    assert!(engines.cpu_frame_for(surface, FrameToken(999)).is_none());
    engines.drain_released();
    assert_eq!(release_calls.load(Ordering::SeqCst), 0);
    assert!(handle.lock().drain_released(surface).unwrap().is_empty());
}

#[test]
fn quarantine_is_per_surface_siblings_unaffected() {
    // A panicking engine on one surface must not take down a healthy
    // engine bound to another surface on the same registry.
    let fragile_calls = Arc::new(AtomicUsize::new(0));
    let seen = Arc::new(Mutex::new(Vec::new()));
    let handle = BridgeHandle::new();
    let mut engines = ExternalEngines::new();
    let bad = bound(
        &handle,
        &mut engines,
        Box::new(FragileRelease {
            calls: Arc::clone(&fragile_calls),
        }),
    );
    let good = bound(
        &handle,
        &mut engines,
        Box::new(Rec {
            released: Arc::clone(&seen),
        }),
    );

    let good_token = publish_and_composite(&handle, good, (8, 8));
    publish_and_composite(&handle, bad, (8, 8)); // quarantines `bad`
    let good_token2 = publish_and_composite(&handle, good, (8, 8));

    engines.drain_released();
    assert_eq!(fragile_calls.load(Ordering::SeqCst), 1);
    // The healthy engine still receives every release, in order.
    assert_eq!(*seen.lock().unwrap(), vec![good_token, good_token2]);
}

// ──────────────── 5. InvalidPayload validation ────────────────

#[test]
fn cpu_frame_with_wrong_pixel_len_rejected() {
    let mut reg = BridgeRegistry::new();
    let id = reg.register();
    // Build via struct literal: CpuFrame::new debug_asserts the length,
    // which would panic instead of exercising the validation path.
    let bad = CpuFrame {
        width: 2,
        height: 2,
        pixels: vec![0u8; 15], // expected 16
    };
    let (slot, _) = reg.acquire(id).unwrap();
    assert!(matches!(
        reg.mark_ready_full(id, slot, None, Some(bad.clone())),
        Err(BridgeError::InvalidPayload)
    ));
    // Same rejection through the dedicated CPU path.
    let (slot2, _) = reg.acquire(id).unwrap();
    assert!(matches!(
        reg.mark_ready_cpu(id, slot2, bad),
        Err(BridgeError::InvalidPayload)
    ));
    // Rejected payloads push no ready event.
    assert!(reg.drain_ready().is_empty());
    // And nothing became compositable.
    assert!(reg.take_front(id).unwrap().is_none());
}

#[test]
fn cpu_frame_with_oversized_dims_rejected() {
    let mut reg = BridgeRegistry::new();
    let id = reg.register();
    let oversized = CpuFrame {
        width: MAX_FRAME_DIM + 1,
        height: 1,
        pixels: vec![],
    };
    let (slot, _) = reg.acquire(id).unwrap();
    assert!(matches!(
        reg.mark_ready_full(id, slot, None, Some(oversized)),
        Err(BridgeError::InvalidPayload)
    ));
    let tall = CpuFrame {
        width: 1,
        height: MAX_FRAME_DIM + 1,
        pixels: vec![],
    };
    let (slot2, _) = reg.acquire(id).unwrap();
    assert!(matches!(
        reg.mark_ready_cpu(id, slot2, tall),
        Err(BridgeError::InvalidPayload)
    ));
}

#[test]
fn frame_with_oversized_dims_rejected() {
    let mut reg = BridgeRegistry::new();
    let id = reg.register();
    let (slot, token) = reg.acquire(id).unwrap();
    assert!(matches!(
        reg.mark_ready_frame(
            id,
            slot,
            Box::new(StubFrame {
                token,
                size: (MAX_FRAME_DIM + 1, 64),
            }),
        ),
        Err(BridgeError::InvalidPayload)
    ));
    // Boundary: exactly MAX_FRAME_DIM is accepted.
    let (slot2, token2) = reg.acquire(id).unwrap();
    reg.mark_ready_frame(
        id,
        slot2,
        Box::new(StubFrame {
            token: token2,
            size: (MAX_FRAME_DIM, 1),
        }),
    )
    .unwrap();
}

#[test]
fn failed_publish_leaves_slot_writing_and_reusable() {
    // After an InvalidPayload rejection the slot stays in `Writing` —
    // the producer can recover by publishing a valid payload into it.
    let mut reg = BridgeRegistry::new();
    let id = reg.register();
    let (slot, token) = reg.acquire(id).unwrap();
    let bad = CpuFrame {
        width: 4,
        height: 4,
        pixels: vec![0u8; 3],
    };
    assert!(matches!(
        reg.mark_ready_cpu(id, slot, bad),
        Err(BridgeError::InvalidPayload)
    ));
    // Recovery on the same slot: valid payload → mark_ready works.
    reg.mark_ready_cpu(id, slot, CpuFrame::new(1, 1, vec![7u8; 4]))
        .unwrap();
    let (_front, front_token) = reg.take_front(id).unwrap().unwrap();
    assert_eq!(front_token, token);
}

// ──────────────── 6. cpu_frame_for resolver ────────────────

#[test]
fn cpu_frame_for_falls_back_to_engine_pixmap() {
    // Engine publishes no ring CpuFrame (GPU-path publish) but can
    // rasterize: the resolver must ask the engine.
    let pixmap_calls = Arc::new(AtomicUsize::new(0));
    let handle = BridgeHandle::new();
    let mut engines = ExternalEngines::new();
    let surface = bound(
        &handle,
        &mut engines,
        Box::new(RasterEngine {
            color: [10, 20, 30, 255],
            pixmap_calls: Arc::clone(&pixmap_calls),
        }),
    );

    let token = publish_and_composite(&handle, surface, (32, 32));
    let cpu = engines
        .cpu_frame_for(surface, token)
        .expect("to_pixmap must supply the raster");
    assert_eq!((cpu.width, cpu.height), (2, 2));
    assert_eq!(cpu.pixels, [10, 20, 30, 255].repeat(4));
    assert_eq!(pixmap_calls.load(Ordering::SeqCst), 1);
}

#[test]
fn cpu_frame_for_prefers_ring_published_payload() {
    // A CpuFrame already sitting in the ring wins — the resolver must
    // not even call to_pixmap (the engine's raster may be stale).
    let pixmap_calls = Arc::new(AtomicUsize::new(0));
    let handle = BridgeHandle::new();
    let mut engines = ExternalEngines::new();
    let surface = bound(
        &handle,
        &mut engines,
        Box::new(RasterEngine {
            color: [10, 20, 30, 255],
            pixmap_calls: Arc::clone(&pixmap_calls),
        }),
    );

    let token = {
        let mut reg = handle.lock();
        let (slot, token) = reg.acquire(surface).unwrap();
        reg.mark_ready_cpu(surface, slot, CpuFrame::new(2, 2, vec![9u8; 16]))
            .unwrap();
        token
    };
    let cpu = engines.cpu_frame_for(surface, token).unwrap();
    assert_eq!(cpu.pixels, vec![9u8; 16]);
    assert_eq!(
        pixmap_calls.load(Ordering::SeqCst),
        0,
        "ring payload must short-circuit the engine fallback"
    );
}

#[test]
fn cpu_frame_for_unknown_surface_returns_none() {
    let mut engines = ExternalEngines::new();
    // No engines bound at all.
    assert!(engines.cpu_frame_for(SurfaceId(1), FrameToken(1)).is_none());
    // Bound to a different surface → still None for the stranger.
    let handle = BridgeHandle::new();
    let _surface = bound(
        &handle,
        &mut engines,
        Box::new(RasterEngine {
            color: [0; 4],
            pixmap_calls: Arc::new(AtomicUsize::new(0)),
        }),
    );
    assert!(engines
        .cpu_frame_for(SurfaceId(999), FrameToken(1))
        .is_none());
}

// ──────────────── adversarial ring/registry edges ────────────────

#[test]
fn released_queue_watermark_drops_oldest_tokens() {
    // RELEASED_WATERMARK is 128: flood past it and the oldest tokens
    // are dropped so the queue stays bounded — a producer that never
    // drains loses recycling info but cannot stall the host.
    let mut ring = SurfaceRing::new();
    const FLOOD: u64 = 200;
    for _ in 0..FLOOD {
        let (slot, _) = ring.acquire().unwrap();
        assert!(ring.force_release(slot));
    }
    let released = ring.drain_released();
    assert_eq!(released.len(), 128);
    // Oldest dropped: the queue keeps the LAST 128 tokens (73..=200).
    assert_eq!(released.first().copied(), Some(FrameToken(FLOOD - 127)));
    assert_eq!(released.last().copied(), Some(FrameToken(FLOOD)));
    assert!(ring.drain_released().is_empty());
}

#[test]
fn ready_event_watermark_bounds_queue() {
    // READY_EVENTS_WATERMARK is 1024: a producer publishing across many
    // surfaces without the host draining cannot grow the queue forever.
    let mut reg = BridgeRegistry::new();
    let mut surfaces = Vec::new();
    for _ in 0..1100 {
        surfaces.push(reg.register());
    }
    for &s in &surfaces {
        let (slot, _) = reg.acquire(s).unwrap();
        reg.mark_ready(s, slot).unwrap();
    }
    let events = reg.drain_ready();
    assert_eq!(events.len(), 1024);
    // The newest events survive — the oldest were dropped.
    assert_eq!(events.last().copied(), Some(*surfaces.last().unwrap()));
    assert!(reg.drain_ready().is_empty());
}

#[test]
fn invalid_transitions_and_slots_rejected() {
    let mut ring = SurfaceRing::new();
    // mark_ready/release on a Free slot.
    assert!(matches!(
        ring.mark_ready(0),
        Err(BridgeError::InvalidTransition)
    ));
    assert!(matches!(
        ring.release(0),
        Err(BridgeError::InvalidTransition)
    ));
    // Out-of-range slot indices.
    assert!(matches!(
        ring.mark_ready(2),
        Err(BridgeError::InvalidSlot(2))
    ));
    assert!(matches!(ring.release(9), Err(BridgeError::InvalidSlot(9))));
    assert!(!ring.force_release(7));
    // Double-release is an invalid transition, not a second token.
    let (slot, token) = ring.acquire().unwrap();
    ring.mark_ready(slot).unwrap();
    let (front, _) = ring.take_front().unwrap();
    ring.release(front).unwrap();
    assert!(matches!(
        ring.release(front),
        Err(BridgeError::InvalidTransition)
    ));
    assert_eq!(ring.drain_released(), vec![token]);
}

#[test]
fn operations_on_unknown_surface_rejected() {
    let mut reg = BridgeRegistry::new();
    let ghost = SurfaceId(999);
    assert!(matches!(
        reg.acquire(ghost),
        Err(BridgeError::UnknownSurface(s)) if s == ghost
    ));
    assert!(matches!(
        reg.take_front(ghost),
        Err(BridgeError::UnknownSurface(s)) if s == ghost
    ));
    assert!(matches!(
        reg.drain_released(ghost),
        Err(BridgeError::UnknownSurface(s)) if s == ghost
    ));
    assert!(matches!(
        reg.reclaim_stalled(ghost),
        Err(BridgeError::UnknownSurface(s)) if s == ghost
    ));
    assert!(matches!(
        reg.force_release(ghost, 0),
        Err(BridgeError::UnknownSurface(s)) if s == ghost
    ));
}

#[test]
fn unregister_discards_in_flight_released_tokens() {
    // Documented contract: unregistering a surface discards in-flight
    // tokens — the producer simply never sees those releases.
    let mut reg = BridgeRegistry::new();
    let id = reg.register();
    let (slot, _token) = reg.acquire(id).unwrap();
    reg.mark_ready(id, slot).unwrap();
    let (front, _) = reg.take_front(id).unwrap().unwrap();
    reg.release(id, front).unwrap(); // token queued, never drained
    reg.unregister(id);
    assert!(matches!(
        reg.drain_released(id),
        Err(BridgeError::UnknownSurface(_))
    ));
    assert!(matches!(
        reg.acquire(id),
        Err(BridgeError::UnknownSurface(_))
    ));
}

#[test]
fn drain_ready_preserves_unbound_surface_events() {
    // ExternalEngines must not consume ready events for surfaces it
    // doesn't own — a widget-only surface's redraw signal survives.
    let handle = BridgeHandle::new();
    let mut engines = ExternalEngines::new();
    let bound_surface = bound(
        &handle,
        &mut engines,
        Box::new(Rec {
            released: Arc::new(Mutex::new(Vec::new())),
        }),
    );
    let unbound = handle.lock().register();

    {
        let mut reg = handle.lock();
        for s in [unbound, bound_surface] {
            let (slot, _) = reg.acquire(s).unwrap();
            reg.mark_ready(s, slot).unwrap();
        }
    }
    // Only the bound surface's event is drained…
    assert_eq!(engines.drain_ready(), vec![bound_surface]);
    // …and the unbound surface's event is still in the registry queue.
    assert_eq!(handle.lock().drain_ready(), vec![unbound]);
}

#[test]
fn duplicate_binding_rejected_but_cross_registry_collision_allowed() {
    let seen = Arc::new(Mutex::new(Vec::new()));
    let handle = BridgeHandle::new();
    let surface = handle.lock().register();
    let mut engines = ExternalEngines::new();
    engines
        .bind(
            handle.clone(),
            surface,
            Box::new(Rec {
                released: Arc::clone(&seen),
            }),
        )
        .unwrap();
    // Same (registry, surface) pair → DuplicateBinding.
    assert!(matches!(
        engines.bind(
            handle.clone(),
            surface,
            Box::new(Rec {
                released: Arc::clone(&seen)
            })
        ),
        Err(BindError::DuplicateBinding)
    ));
    assert_eq!(engines.len(), 1);
    // A numerically-identical SurfaceId on a DIFFERENT registry is a
    // distinct binding — dedup keys on (registry_id, surface), not the
    // raw id.
    let other = BridgeHandle::new();
    engines
        .bind(
            other,
            surface,
            Box::new(Rec {
                released: Arc::clone(&seen),
            }),
        )
        .unwrap();
    assert_eq!(engines.len(), 2);
}

// ──────────────── widget-level adversarial checks ────────────────

#[test]
fn poll_frame_survives_evicted_front_and_unregister() {
    let handle = BridgeHandle::new();
    let surface = handle.lock().register();
    let mut widget = ExternalEngine::new(handle.clone(), surface);

    // Publish then evict the ready frame before the host composites:
    // poll_frame must report no observable frame rather than a stale one.
    {
        let mut reg = handle.lock();
        let (slot, _) = reg.acquire(surface).unwrap();
        reg.mark_ready_sized(surface, slot, (64, 64)).unwrap();
        assert!(reg.force_release(surface, slot).unwrap());
    }
    assert_eq!(widget.poll_frame(), FramePoll::None);

    // A fresh publish is still observed normally.
    {
        let mut reg = handle.lock();
        let (slot, _) = reg.acquire(surface).unwrap();
        reg.mark_ready_sized(surface, slot, (64, 64)).unwrap();
        reg.drain_ready();
    }
    assert_eq!(widget.poll_frame(), FramePoll::Resized);
    assert_eq!(widget.poll_frame(), FramePoll::None);

    // Surface unregistered out from under the widget → None, not panic.
    handle.lock().unregister(surface);
    assert_eq!(widget.poll_frame(), FramePoll::None);
}

#[test]
fn widget_drop_unregisters_surface() {
    let handle = BridgeHandle::new();
    let surface = handle.lock().register();
    assert_eq!(handle.lock().surface_count(), 1);
    {
        let _widget = ExternalEngine::new(handle.clone(), surface);
    } // widget dropped
    assert_eq!(handle.lock().surface_count(), 0);
    assert!(matches!(
        handle.lock().acquire(surface),
        Err(BridgeError::UnknownSurface(_))
    ));
}

#[test]
fn drain_ready_only_reports_bound_surfaces_on_each_registry() {
    // Two registries, each with a bound and an unbound surface: the
    // drain must report each registry's bound surfaces without
    // swallowing the unbound ones' events.
    let h1 = BridgeHandle::new();
    let h2 = BridgeHandle::new();
    let mut engines = ExternalEngines::new();
    let mk = || {
        Box::new(Rec {
            released: Arc::new(Mutex::new(Vec::new())),
        }) as Box<dyn Engine>
    };
    let b1 = bound(&h1, &mut engines, mk());
    let b2 = bound(&h2, &mut engines, mk());
    let u1 = h1.lock().register();
    let u2 = h2.lock().register();
    for (h, s) in [(&h1, u1), (&h1, b1), (&h2, u2), (&h2, b2)] {
        let mut reg = h.lock();
        let (slot, _) = reg.acquire(s).unwrap();
        reg.mark_ready(s, slot).unwrap();
    }
    let mut ready = engines.drain_ready();
    ready.sort();
    assert_eq!(ready, {
        let mut v = vec![b1, b2];
        v.sort();
        v
    });
    assert_eq!(h1.lock().drain_ready(), vec![u1]);
    assert_eq!(h2.lock().drain_ready(), vec![u2]);
}

// ──────────────── 7. input-event seam (v0.15.0) ────────────────

#[test]
fn forward_event_reaches_bound_engine() {
    let events = Arc::new(Mutex::new(Vec::new()));
    let released = Arc::new(Mutex::new(Vec::new()));
    let handle = BridgeHandle::new();
    let mut engines = ExternalEngines::new();
    let surface = bound(
        &handle,
        &mut engines,
        Box::new(RecEvents {
            events: Arc::clone(&events),
            released,
        }),
    );

    let move_ev = EngineEvent::PointerMove {
        position: [12.0, 34.0],
    };
    let button_ev = EngineEvent::PointerButton {
        position: [12.0, 34.0],
        button: PointerButton::Primary,
        pressed: true,
    };
    let key_ev = EngineEvent::Key {
        scancode: 30,
        pressed: true,
    };
    engines.forward_event(surface, &move_ev);
    engines.forward_event(surface, &button_ev);
    engines.forward_event(surface, &key_ev);

    assert_eq!(
        *events.lock().unwrap(),
        vec![move_ev, button_ev, key_ev],
        "every forwarded event must reach the bound engine, in order"
    );
}

#[test]
fn forward_event_quarantines_panicking_engine() {
    let event_calls = Arc::new(AtomicUsize::new(0));
    let release_calls = Arc::new(AtomicUsize::new(0));
    let handle = BridgeHandle::new();
    let mut engines = ExternalEngines::new();
    let surface = bound(
        &handle,
        &mut engines,
        Box::new(FragileEvent {
            event_calls: Arc::clone(&event_calls),
            release_calls: Arc::clone(&release_calls),
        }),
    );

    // First event: on_event panics → the engine is quarantined.
    engines.forward_event(surface, &EngineEvent::Focus { focused: true });
    assert_eq!(event_calls.load(Ordering::SeqCst), 1);

    // Subsequent events never reach it again.
    engines.forward_event(
        surface,
        &EngineEvent::PointerMove {
            position: [0.0, 0.0],
        },
    );
    assert_eq!(
        event_calls.load(Ordering::SeqCst),
        1,
        "quarantined engine must never be called again"
    );

    // The same quarantine applies to drain_released: the callback is
    // skipped while the token queue is still drained.
    publish_and_composite(&handle, surface, (8, 8));
    engines.drain_released();
    assert_eq!(release_calls.load(Ordering::SeqCst), 0);
    assert!(handle.lock().drain_released(surface).unwrap().is_empty());
}

#[test]
fn unbind_removes_engine_binding() {
    let events = Arc::new(Mutex::new(Vec::new()));
    let released = Arc::new(Mutex::new(Vec::new()));
    let handle = BridgeHandle::new();
    let mut engines = ExternalEngines::new();
    let surface = bound(
        &handle,
        &mut engines,
        Box::new(RecEvents {
            events: Arc::clone(&events),
            released: Arc::clone(&released),
        }),
    );

    // Ring stays usable while bound: publishes a ready event + a
    // released token, both still queued.
    let token = publish_and_composite(&handle, surface, (8, 8));

    assert!(engines.unbind(surface));
    assert_eq!(engines.len(), 0);
    assert!(!engines.unbind(surface), "second unbind finds nothing");

    // Events are dropped — no engine is bound to the surface.
    engines.forward_event(surface, &EngineEvent::TextInput { text: "x".into() });
    assert!(events.lock().unwrap().is_empty());

    // drain_ready/drain_released with no binding are no-ops: the ring's
    // queues are untouched (event + token still queued for later).
    assert!(engines.drain_ready().is_empty());
    engines.drain_released();
    assert!(released.lock().unwrap().is_empty());
    assert_eq!(
        handle.lock().drain_ready(),
        vec![surface],
        "unbinding must not discard the surface's queued ready event"
    );

    // Rebinding works — a fresh engine sees events, and the released
    // token that accumulated while unbound drains to it.
    let events2 = Arc::new(Mutex::new(Vec::new()));
    let released2 = Arc::new(Mutex::new(Vec::new()));
    engines
        .bind(
            handle.clone(),
            surface,
            Box::new(RecEvents {
                events: Arc::clone(&events2),
                released: Arc::clone(&released2),
            }),
        )
        .unwrap();
    engines.forward_event(surface, &EngineEvent::Focus { focused: true });
    assert_eq!(
        *events2.lock().unwrap(),
        vec![EngineEvent::Focus { focused: true }]
    );
    engines.drain_released();
    assert_eq!(*released2.lock().unwrap(), vec![token]);
}

#[test]
fn forward_event_unknown_surface_is_noop() {
    let mut engines = ExternalEngines::new();
    // No engines bound at all.
    engines.forward_event(
        SurfaceId(999),
        &EngineEvent::Scroll {
            position: [0.0, 0.0],
            delta: [1.0, -1.0],
        },
    );

    // A bound engine on a different surface must not observe it.
    let events = Arc::new(Mutex::new(Vec::new()));
    let handle = BridgeHandle::new();
    let _surface = bound(
        &handle,
        &mut engines,
        Box::new(RecEvents {
            events: Arc::clone(&events),
            released: Arc::new(Mutex::new(Vec::new())),
        }),
    );
    engines.forward_event(
        SurfaceId(999),
        &EngineEvent::PointerMove {
            position: [1.0, 2.0],
        },
    );
    assert!(events.lock().unwrap().is_empty());
}
