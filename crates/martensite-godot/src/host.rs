//! The Martensite-side engine: drains [`FrameMsg`]s arriving over the
//! transport and publishes them into the `martensite-engine-bridge`
//! surface ring.
//!
//! **This module never touches the `godot` crate's engine-facing APIs.**
//! When `martensite-godot` is linked as an `rlib` by a host app (the
//! `libgodot`-style embedding or the standalone receiver process),
//! nothing here requires a running Godot runtime — frames arrive through
//! [`FrameSource`] as plain bytes.
//!
//! # What happens per frame (Tier 1 honesty accounting)
//!
//! 1. Godot has already done a GPU→CPU readback
//!    (`texture_get_data_async`); the pixels arrived here through a
//!    socket or channel (one CPU copy on the wire).
//! 2. [`Engine::render`] uploads them to a host-owned `wgpu::Texture`
//!    via `queue.write_texture` — a CPU→GPU upload.
//! 3. The same pixels are also published as a [`CpuFrame`] so the
//!    TinySkia fallback path composites without a GPU texture.
//!
//! Two GPU boundary crossings per frame — that is the documented cost of
//! embedding Godot without engine patches.

use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use std::thread::JoinHandle;

use martensite_engine_bridge::{
    BridgeHandle, CpuFrame, Engine, EngineContext, Frame, FrameToken, SharedTexture, SurfaceId,
    TextureFrame, Viewport,
};

use crate::transport::{FrameMsg, FrameSource};

/// Resources the host has not yet released for a published token.
///
/// The texture is retained so the ring dropping its `TextureFrame` on
/// release is never the last owner (same pattern as the bridge's
/// `MockEngine`); the CPU copy serves [`Engine::to_pixmap`].
struct PendingFrame {
    texture: SharedTexture,
    cpu: CpuFrame,
}

/// An [`Engine`] that composites frames produced by a Godot
/// `SubViewport` and shipped over the [`crate::transport`] channel.
///
/// Construct with one of the `new_*` constructors matching the chosen
/// transport, then register it like any other `Engine` — the bridge
/// ring does the mailbox bookkeeping; this type does the GPU upload and
/// slot-texture recycling.
pub struct GodotEngine {
    handle: BridgeHandle,
    surface: SurfaceId,
    /// Mailbox: the receiver thread always overwrites — the host
    /// composites only the freshest frame, matching `PresentMode::
    /// Mailbox` semantics of the ring itself.
    latest: Arc<Mutex<Option<FrameMsg>>>,
    /// Background thread draining the transport into `latest`.
    receiver: Mutex<Option<JoinHandle<()>>>,
    /// Textures+CPU copies keyed by token, retained until `release`.
    pending: HashMap<FrameToken, PendingFrame>,
    /// Freed slot textures available for reuse.
    pool: Vec<SharedTexture>,
    /// Tokens handed back via `release` (accounting/testing).
    released: Vec<FrameToken>,
    /// Frames that arrived but could not be published (ring exhausted or
    /// publish error) — surfaced for instrumentation.
    dropped: u64,
    /// Frames successfully published to the ring.
    published: u64,
}

impl GodotEngine {
    /// Creates a `GodotEngine` draining `source` on a background thread.
    ///
    /// The thread loops on [`FrameSource::recv`]; socket sources
    /// transparently re-accept when a producer disconnects, and a hard
    /// error ends the thread (the engine then keeps compositing the last
    /// published frame).
    pub fn new(handle: BridgeHandle, surface: SurfaceId, source: FrameSource) -> Self {
        let latest = Arc::new(Mutex::new(None::<FrameMsg>));
        let thread_latest = Arc::clone(&latest);
        let receiver = std::thread::Builder::new()
            .name("martensite-godot-recv".into())
            .spawn(move || {
                let mut source = source;
                while let Ok(msg) = source.recv() {
                    // Mailbox: a stale not-yet-consumed frame is simply
                    // overwritten — the host never wants the older one.
                    *thread_latest.lock().unwrap_or_else(|e| e.into_inner()) = Some(msg);
                }
            })
            .expect("spawn martensite-godot receiver thread");

        Self {
            handle,
            surface,
            latest,
            receiver: Mutex::new(Some(receiver)),
            pending: HashMap::new(),
            pool: Vec::new(),
            released: Vec::new(),
            dropped: 0,
            published: 0,
        }
    }

    /// Creates a `GodotEngine` reading frames from a loopback-TCP
    /// listener bound to `addr` (e.g. `"127.0.0.1:9177"`).
    ///
    /// # Errors
    ///
    /// Propagates `TcpListener::bind` errors.
    pub fn new_tcp(
        handle: BridgeHandle,
        surface: SurfaceId,
        addr: impl std::net::ToSocketAddrs,
    ) -> std::io::Result<Self> {
        Ok(Self::new(handle, surface, FrameSource::bind_tcp(addr)?))
    }

    /// Creates a `GodotEngine` reading frames from a Unix-socket
    /// listener at `path`.
    ///
    /// # Errors
    ///
    /// Propagates listener bind errors.
    #[cfg(unix)]
    pub fn new_unix(
        handle: BridgeHandle,
        surface: SurfaceId,
        path: impl AsRef<std::path::Path>,
    ) -> std::io::Result<Self> {
        Ok(Self::new(handle, surface, FrameSource::bind_unix(path)?))
    }

    /// Creates a `GodotEngine` reading from an in-process channel — the
    /// `libgodot`-embedding path. Obtain `rx` via
    /// [`crate::transport::take_channel_receiver`] after the extension
    /// registered its [`crate::transport::ChannelTransport`].
    pub fn new_channel(
        handle: BridgeHandle,
        surface: SurfaceId,
        rx: std::sync::mpsc::Receiver<FrameMsg>,
    ) -> Self {
        Self::new(handle, surface, FrameSource::channel(rx))
    }

    /// The surface this engine publishes into.
    pub fn surface(&self) -> SurfaceId {
        self.surface
    }

    /// Number of frames published into the ring so far.
    pub fn published(&self) -> u64 {
        self.published
    }

    /// Frames dropped because the `mark_ready_full` publish failed.
    /// Ring exhaustion defers a frame instead — it is not counted here.
    pub fn dropped(&self) -> u64 {
        self.dropped
    }

    /// Tokens the host released back to this engine (test accounting).
    pub fn released_tokens(&self) -> &[FrameToken] {
        &self.released
    }

    /// Drains the bridge's released-token queue for this surface and
    /// calls [`Engine::release`] for each — the producer half of the
    /// recycling contract. Call once per host frame.
    pub fn drain_released(&mut self) {
        let tokens = self
            .handle
            .lock()
            .drain_released(self.surface)
            .unwrap_or_default();
        for token in tokens {
            self.release(token);
        }
    }

    /// Reuses a pooled texture of matching size, or creates a fresh
    /// `Rgba8Unorm` slot texture on the host device.
    fn make_texture(&mut self, device: &wgpu::Device, size: (u32, u32)) -> SharedTexture {
        let size = (size.0.max(1), size.1.max(1));
        if let Some(pos) = self
            .pool
            .iter()
            .rposition(|t| t.width() == size.0 && t.height() == size.1)
        {
            return self.pool.remove(pos);
        }
        Arc::new(device.create_texture(&wgpu::TextureDescriptor {
            label: Some("martensite-godot-frame"),
            size: wgpu::Extent3d {
                width: size.0,
                height: size.1,
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            // The readback produces non-sRGB RGBA8 bytes; the host's
            // composite pass applies the source-alpha pipeline.
            format: wgpu::TextureFormat::Rgba8Unorm,
            usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
            view_formats: &[],
        }))
    }
}

impl Engine for GodotEngine {
    fn render(&mut self, ctx: &mut EngineContext, _viewport: Viewport) -> Option<Box<dyn Frame>> {
        let msg = self
            .latest
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .take()?;
        let (w, h) = (msg.width, msg.height);

        let (slot, token) = match self.handle.lock().acquire(self.surface) {
            Ok(pair) => pair,
            Err(_) => {
                // Ring exhausted (host hasn't released yet): put the
                // frame back — it is still the freshest — and try again
                // next render call. Not counted as dropped: it is merely
                // deferred and will be published on a later pass.
                *self.latest.lock().unwrap_or_else(|e| e.into_inner()) = Some(msg);
                return None;
            }
        };

        let texture = self.make_texture(ctx.device, (w, h));
        ctx.queue.write_texture(
            wgpu::TexelCopyTextureInfo {
                texture: texture.as_ref(),
                mip_level: 0,
                origin: wgpu::Origin3d::ZERO,
                aspect: wgpu::TextureAspect::All,
            },
            &msg.pixels,
            wgpu::TexelCopyBufferLayout {
                offset: 0,
                bytes_per_row: Some(w.saturating_mul(4)),
                rows_per_image: Some(h),
            },
            wgpu::Extent3d {
                width: w,
                height: h,
                depth_or_array_layers: 1,
            },
        );
        // Flush before publishing: same-queue ordering then guarantees
        // the host's composite pass sees the populated texture.
        ctx.queue.submit([]);

        // Publish the GPU frame AND the CPU raster: `mark_ready_full`
        // keeps the texture alive in the ring slot while `cpu` feeds the
        // TinySkia fallback path. We keep our own `Arc` + CPU copy in
        // `pending` until `release` so the ring's drop is never the last
        // owner and `to_pixmap` can still answer for released-pending
        // tokens.
        let cpu = CpuFrame::new(w, h, msg.pixels);
        self.pending.insert(
            token,
            PendingFrame {
                texture: Arc::clone(&texture),
                cpu: cpu.clone(),
            },
        );
        let published = self.handle.lock().mark_ready_full(
            self.surface,
            slot,
            Some(Box::new(TextureFrame::new(
                token,
                (*texture).clone(),
                (w, h),
            ))),
            Some(cpu),
        );
        if published.is_err() {
            self.pending.remove(&token);
            self.handle.lock().force_release(self.surface, slot).ok();
            self.dropped += 1;
            return None;
        }
        self.published += 1;
        Some(Box::new(TextureFrame::new(
            token,
            (*texture).clone(),
            (w, h),
        )))
    }

    fn release(&mut self, token: FrameToken) {
        self.released.push(token);
        if let Some(p) = self.pending.remove(&token) {
            self.pool.push(p.texture);
        }
    }

    fn to_pixmap(&self, token: FrameToken) -> Option<CpuFrame> {
        self.pending.get(&token).map(|p| p.cpu.clone())
    }
}

impl Drop for GodotEngine {
    fn drop(&mut self) {
        // Detach the mailbox first so a blocked `recv` exiting later
        // cannot touch freed state; then join briefly. A socket `recv`
        // blocks in `accept`/`read` — we cannot interrupt it without a
        // shutdown mechanism, so a still-blocked thread is intentionally
        // detached rather than joined forever.
        if let Some(handle) = self
            .receiver
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .take()
            && handle.is_finished()
        {
            handle.join().ok();
        }
        // Otherwise: leave the thread running; it holds only an Arc
        // to the (still-alive) `latest` cell.
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::transport::{ChannelTransport, FrameTransport};

    #[test]
    fn mailbox_drains_into_latest() {
        let handle = BridgeHandle::new();
        let surface = handle.lock().register();
        let mut tx = ChannelTransport::new_named("host-test-mailbox");
        let rx = crate::transport::take_channel_receiver("host-test-mailbox").unwrap();
        let engine = GodotEngine::new_channel(handle, surface, rx);
        tx.send(&FrameMsg::rgba8(2, 2, 1, vec![7u8; 16]).unwrap())
            .unwrap();
        // Give the receiver thread a moment to drain.
        for _ in 0..100 {
            if engine.latest.lock().unwrap().is_some() {
                break;
            }
            std::thread::yield_now();
        }
        let msg = engine.latest.lock().unwrap().take().unwrap();
        assert_eq!((msg.width, msg.height, msg.seq), (2, 2, 1));
        assert_eq!(msg.pixels, vec![7u8; 16]);
    }

    /// Exercises `render` end-to-end (texture upload + ring publish)
    /// on wgpu's `noop` backend — no real GPU required.
    #[cfg(feature = "test-noop")]
    #[test]
    fn render_publishes_into_ring() {
        let handle = BridgeHandle::new();
        let surface = handle.lock().register();
        let mut tx = ChannelTransport::new_named("host-test-render");
        let rx = crate::transport::take_channel_receiver("host-test-render").unwrap();
        let mut engine = GodotEngine::new_channel(handle.clone(), surface, rx);
        tx.send(&FrameMsg::rgba8(4, 4, 1, vec![9u8; 64]).unwrap())
            .unwrap();

        // `Device::noop` short-circuits the Instance/Adapter dance: a
        // fully validating no-op backend, no GPU required.
        let (device, queue) = wgpu::Device::noop(&wgpu::DeviceDescriptor::default());
        let mut ctx = EngineContext {
            device: &device,
            queue: &queue,
        };

        let frame = engine
            .render(&mut ctx, Viewport::new(4, 4, 1.0))
            .expect("a frame is produced");
        assert_eq!(frame.size(), (4, 4));
        assert!(frame.same_device_texture().is_some());
        let reg = handle.lock();
        assert_eq!(reg.front_size(surface).unwrap(), Some((4, 4)));
        let cpu = reg.front_cpu_frame(surface).unwrap().unwrap();
        assert_eq!(cpu.pixels, vec![9u8; 64]);
    }
}
