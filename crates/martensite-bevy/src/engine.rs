//! [`BevyEngine`] — the [`Engine`](martensite_engine_bridge::Engine)
//! implementation driving a headless Bevy [`App`](bevy::app::App) on its own
//! thread.
//!
//! `bevy::app::App` is `!Send`/`!Sync`, so the `App`, the slot-texture pool,
//! and the ring publish all live on a dedicated render thread spawned by
//! [`BevyEngine::new`]. `Engine` calls cross the boundary as commands on a
//! channel; the produced [`Frame`](martensite_engine_bridge::Frame) handles
//! travel back because `wgpu` 30 textures are `Send + Sync` shared objects.

use std::collections::HashMap;
use std::sync::mpsc::{self, Receiver, Sender};
use std::thread::JoinHandle;

use bevy::app::App;
use martensite_engine_bridge::{
    BridgeHandle, CpuFrame, Engine, EngineContext, EngineEvent, Frame, FrameSync, FrameToken,
    NativeFrame, SharedTexture, SourceAlpha, SurfaceId, Viewport,
};
use martensite_wgpu::GpuContext;

use crate::app::{RenderHandles, build_app};
use crate::viewport::{CAMERA_VIEW_HANDLE, SlotPool};

/// Errors constructing or reaching a [`BevyEngine`].
///
/// # Examples
///
/// ```
/// use martensite_bevy::BevyAdapterError;
///
/// let err = BevyAdapterError::Init("boom".to_string());
/// assert!(err.to_string().contains("boom"));
/// ```
#[derive(Debug)]
pub enum BevyAdapterError {
    /// The Bevy app panicked or failed during startup on the render thread.
    /// The payload is the panic message.
    Init(String),
    /// The render thread could not be spawned.
    Spawn(std::io::Error),
    /// The render thread is gone — every command channel is disconnected.
    /// Terminal for this engine; the host should drop and recreate it.
    Terminated,
}

impl std::fmt::Display for BevyAdapterError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Init(msg) => write!(f, "bevy app failed to initialize: {msg}"),
            Self::Spawn(err) => write!(f, "failed to spawn the bevy render thread: {err}"),
            Self::Terminated => write!(f, "the bevy render thread has terminated"),
        }
    }
}

impl std::error::Error for BevyAdapterError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Spawn(err) => Some(err),
            _ => None,
        }
    }
}

/// One frame rendered by the embedded Bevy app — a same-device
/// [`SharedTexture`] the host composites without any copy.
///
/// The frame is published into the surface ring by
/// [`Engine::render`]; the returned value is the same frame as a cheap
/// inspection handle, per the [`Engine`] contract.
///
/// # Examples
///
/// ```no_run
/// use martensite_bevy::BevyFrame;
/// use martensite_engine_bridge::{Frame, FrameToken};
/// # let texture: std::sync::Arc<wgpu::Texture> = todo!();
///
/// let frame = BevyFrame::new(FrameToken(1), texture, (640, 480));
/// assert_eq!(frame.size(), (640, 480));
/// assert!(frame.same_device_texture().is_some());
/// ```
#[derive(Clone)]
pub struct BevyFrame {
    token: FrameToken,
    texture: SharedTexture,
    size: (u32, u32),
}

impl BevyFrame {
    /// Creates a frame handle for `token` wrapping `texture` of `size`.
    pub fn new(token: FrameToken, texture: SharedTexture, size: (u32, u32)) -> Self {
        Self {
            token,
            texture,
            size,
        }
    }
}

impl std::fmt::Debug for BevyFrame {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("BevyFrame")
            .field("token", &self.token)
            .field("size", &self.size)
            .finish_non_exhaustive()
    }
}

impl Frame for BevyFrame {
    fn token(&self) -> FrameToken {
        self.token
    }

    fn same_device_texture(&self) -> Option<&wgpu::Texture> {
        Some(&self.texture)
    }

    fn native_handle(&self) -> Option<NativeFrame> {
        None
    }

    fn sync(&self) -> FrameSync {
        // Same device, same queue: serial submission ordering — the render
        // thread's `queue.submit` happens-before the host's composite submit.
        FrameSync::None
    }

    fn size(&self) -> (u32, u32) {
        self.size
    }

    fn alpha_mode(&self) -> SourceAlpha {
        SourceAlpha::Premultiplied
    }
}

/// Commands the render thread executes, one per `Engine` call.
enum Command {
    /// Render one frame into a ring slot; reply carries the published frame
    /// (or `None` when the ring was exhausted / the update failed).
    Render {
        /// The laid-out viewport for this frame.
        viewport: Viewport,
        /// One-shot reply channel.
        reply: Sender<Option<BevyFrame>>,
    },
    /// Run a host closure against the app (scene spawning, queries, extra
    /// systems). The closure owns its own reply channel.
    WithApp(Box<dyn FnOnce(&mut App) + Send>),
    /// Stop the render thread (engine `Drop`).
    Shutdown,
}

/// An [`Engine`] that produces frames by rendering a headless Bevy 3D scene
/// into `wgpu` slot textures on the host's device.
///
/// Construct with [`BevyEngine::new`]; the Bevy [`App`] is created inside a
/// dedicated render thread (it is `!Send` and cannot cross threads). Each
/// [`Engine::render`] acquires a ring slot, re-registers that slot's texture
/// as the [`ManualTextureView`](bevy::render::texture::ManualTextureView)
/// behind [`CAMERA_VIEW_HANDLE`], pumps one `App::update`, and publishes the
/// slot — the host composites the texture with zero copies.
///
/// # Scene setup
///
/// The app starts with a single `Camera3d` (at `(0, 0, 5)` looking at the
/// origin) targeting [`CAMERA_VIEW_HANDLE`]. Hosts spawn meshes, lights, and
/// additional systems through [`BevyEngine::with_app`], which runs a closure
/// on the render thread:
///
/// ```no_run
/// # use martensite_bevy::{bevy::prelude::*, BevyEngine};
/// # fn demo(engine: &BevyEngine) {
/// engine.with_app(|app| {
///     app.world_mut().spawn((
///         PointLight::default(),
///         Transform::from_xyz(4.0, 8.0, 4.0),
///     ));
/// });
/// # }
/// ```
pub struct BevyEngine {
    cmd: Sender<Command>,
    events: Sender<EngineEvent>,
    handle: BridgeHandle,
    surface: SurfaceId,
    /// Host-device clones retained for [`Engine::to_pixmap`] readbacks —
    /// `wgpu` resources are `Send + Sync`, so the readback can run on the
    /// caller's thread.
    device: wgpu::Device,
    queue: wgpu::Queue,
    /// Whether frames are `Bgra*` (needs a B/R swizzle on CPU readback).
    bgra: bool,
    /// Published-token → texture retention so `to_pixmap` can read back a
    /// frame that is still live, mirroring `MockEngine`'s pending map.
    pending: HashMap<FrameToken, SharedTexture>,
    /// Tokens the host released back to this engine (test accounting).
    released: Vec<FrameToken>,
    /// Frames published into the ring (test accounting).
    published: u64,
    /// The render thread handle, joined on `Drop`.
    thread: Option<JoinHandle<()>>,
}

impl BevyEngine {
    /// Creates a `BevyEngine` bound to `surface` on `handle`, rendering
    /// `size` physical pixels in `format` (match the host's surface format —
    /// typically `Bgra8UnormSrgb` or `Rgba8UnormSrgb`).
    ///
    /// `gpu` must be the [`GpuContext`] whose `device`/`queue` the host hands
    /// to [`Engine::render`] via [`EngineContext`] — the adapter clones those
    /// objects into Bevy's `RenderCreation::Manual`, which is what makes every
    /// published frame a same-device texture.
    ///
    /// # Errors
    ///
    /// * [`BevyAdapterError::Spawn`] if the render thread fails to launch;
    /// * [`BevyAdapterError::Init`] if Bevy app construction panics;
    /// * [`BevyAdapterError::Terminated`] if the thread dies before
    ///   acknowledging startup.
    ///
    /// # Examples
    ///
    /// ```no_run
    /// use martensite_bevy::BevyEngine;
    /// use martensite_engine_bridge::BridgeHandle;
    /// use martensite_wgpu::GpuContext;
    ///
    /// let gpu = GpuContext::new().expect("GPU available");
    /// let handle = BridgeHandle::new();
    /// let surface = handle.lock().register();
    /// let engine = BevyEngine::new(
    ///     &gpu,
    ///     handle,
    ///     surface,
    ///     (1280, 720),
    ///     wgpu::TextureFormat::Bgra8UnormSrgb,
    /// )
    /// .expect("bevy engine booted");
    /// ```
    pub fn new(
        gpu: &GpuContext,
        handle: BridgeHandle,
        surface: SurfaceId,
        size: (u32, u32),
        format: wgpu::TextureFormat,
    ) -> Result<Self, BevyAdapterError> {
        let (cmd_tx, cmd_rx) = mpsc::channel::<Command>();
        let (event_tx, event_rx) = mpsc::channel::<EngineEvent>();
        let (ready_tx, ready_rx) = mpsc::channel::<Result<(), String>>();

        let device = (*gpu.device).clone();
        let queue = (*gpu.queue).clone();
        let render_handles = RenderHandles::from_gpu(gpu);
        let thread_handle = handle.clone();

        let thread = std::thread::Builder::new()
            .name("martensite-bevy-render".into())
            .spawn(move || {
                let boot = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                    let pool = SlotPool::new(&render_handles.device, size, format);
                    // Slot 0's texture seeds the manual view so the camera
                    // resolves a target before the first render() call; every
                    // subsequent frame re-registers the acquired slot.
                    let view = pool.make_view(
                        pool.slot(0)
                            .create_view(&wgpu::TextureViewDescriptor::default()),
                    );
                    let app = build_app(render_handles, event_rx, view);
                    (app, pool)
                }));
                match boot {
                    Ok((mut app, mut pool)) => {
                        let _ = ready_tx.send(Ok(()));
                        run(&mut app, &mut pool, &thread_handle, surface, cmd_rx);
                    }
                    Err(payload) => {
                        let _ = ready_tx.send(Err(panic_message(&*payload)));
                    }
                }
            })
            .map_err(BevyAdapterError::Spawn)?;

        match ready_rx.recv() {
            Ok(Ok(())) => {}
            Ok(Err(msg)) => {
                // The thread exited after reporting the failure; join it so
                // the handle does not linger.
                let _ = thread.join();
                return Err(BevyAdapterError::Init(msg));
            }
            Err(_) => {
                let _ = thread.join();
                return Err(BevyAdapterError::Terminated);
            }
        }

        Ok(Self {
            cmd: cmd_tx,
            events: event_tx,
            handle,
            surface,
            device,
            queue,
            bgra: matches!(
                format,
                wgpu::TextureFormat::Bgra8Unorm | wgpu::TextureFormat::Bgra8UnormSrgb
            ),
            pending: HashMap::new(),
            released: Vec::new(),
            published: 0,
            thread: Some(thread),
        })
    }

    /// The surface this engine publishes into.
    ///
    /// # Examples
    ///
    /// ```no_run
    /// # use martensite_bevy::BevyEngine;
    /// # use martensite_engine_bridge::BridgeHandle;
    /// # use martensite_wgpu::GpuContext;
    /// # let gpu = GpuContext::new().unwrap();
    /// # let handle = BridgeHandle::new();
    /// # let surface = handle.lock().register();
    /// let engine = BevyEngine::new(&gpu, handle, surface, (64, 64), wgpu::TextureFormat::Bgra8UnormSrgb).unwrap();
    /// assert_eq!(engine.surface(), surface);
    /// ```
    pub fn surface(&self) -> SurfaceId {
        self.surface
    }

    /// Frames successfully published into the ring so far.
    pub fn published(&self) -> u64 {
        self.published
    }

    /// Tokens the host released back to this engine (test accounting).
    pub fn released_tokens(&self) -> &[FrameToken] {
        &self.released
    }

    /// Drains the bridge's released-token queue for this surface and calls
    /// [`Engine::release`] for each — the producer half of the recycling
    /// contract. Call once per host frame.
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

    /// Runs `f` against the Bevy [`App`] on the render thread and returns
    /// its result — the scene-authoring escape hatch.
    ///
    /// Use it to spawn entities, insert resources, or add systems:
    ///
    /// ```no_run
    /// # use martensite_bevy::{bevy::prelude::*, BevyEngine};
    /// # fn demo(engine: &BevyEngine) {
    /// let world_len = engine.with_app(|app| app.world().entities().len());
    /// # let _ = world_len;
    /// # }
    /// ```
    ///
    /// Returns `None` when the render thread is gone. `f` runs between
    /// [`Engine::render`] calls — never concurrently with a frame — so it is
    /// safe to mutate the world freely.
    ///
    /// `f` executes **on** the render thread; calling `with_app` (or
    /// [`Engine::render`]) from inside `f` would deadlock the channel — keep
    /// closures self-contained.
    pub fn with_app<R>(&self, f: impl FnOnce(&mut App) -> R + Send + 'static) -> Option<R>
    where
        R: Send + 'static,
    {
        let (tx, rx) = mpsc::channel::<R>();
        self.cmd
            .send(Command::WithApp(Box::new(move |app| {
                let _ = tx.send(f(app));
            })))
            .ok()?;
        rx.recv().ok()
    }
}

impl Engine for BevyEngine {
    fn render(&mut self, _ctx: &mut EngineContext, viewport: Viewport) -> Option<Box<dyn Frame>> {
        // Clamp before touching the ring: an oversized viewport would fail
        // `mark_ready_full` with InvalidPayload anyway.
        let (w, h) = viewport.size;
        if w == 0
            || h == 0
            || w > martensite_engine_bridge::MAX_FRAME_DIM
            || h > martensite_engine_bridge::MAX_FRAME_DIM
        {
            return None;
        }
        let (tx, rx) = mpsc::channel();
        self.cmd
            .send(Command::Render {
                viewport,
                reply: tx,
            })
            .ok()?;
        let frame = rx.recv().ok()??;
        self.pending.insert(frame.token(), frame.texture.clone());
        self.published += 1;
        Some(Box::new(frame))
    }

    fn release(&mut self, token: FrameToken) {
        self.released.push(token);
        self.pending.remove(&token);
    }

    fn to_pixmap(&self, token: FrameToken) -> Option<CpuFrame> {
        let texture = self.pending.get(&token)?;
        readback(&self.device, &self.queue, texture, self.bgra)
    }

    fn on_event(&mut self, event: &EngineEvent) {
        // Queue for the next `PreUpdate` drain. A dead render thread drops
        // the receiver; the send error is intentionally ignored — input for
        // a dead engine is dropped, not fatal.
        let _ = self.events.send(event.clone());
    }
}

impl Drop for BevyEngine {
    fn drop(&mut self) {
        let _ = self.cmd.send(Command::Shutdown);
        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
    }
}

/// The render-thread loop: one `App::update` per `Render` command, host
/// closures in between, until `Shutdown` (or channel disconnect).
fn run(
    app: &mut App,
    pool: &mut SlotPool,
    handle: &BridgeHandle,
    surface: SurfaceId,
    rx: Receiver<Command>,
) {
    while let Ok(cmd) = rx.recv() {
        match cmd {
            Command::Render { viewport, reply } => {
                // A panicking Bevy update must not kill the thread — the host
                // gets `None` (keeps the last frame) and may retry next frame.
                let frame = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                    render_frame(app, pool, handle, surface, viewport)
                }))
                .unwrap_or_default();
                let _ = reply.send(frame);
            }
            Command::WithApp(f) => f(app),
            Command::Shutdown => break,
        }
    }
}

/// One frame on the render thread: acquire a ring slot, point the camera's
/// manual view at that slot's texture, pump the app, wait for the GPU, and
/// publish.
fn render_frame(
    app: &mut App,
    pool: &mut SlotPool,
    handle: &BridgeHandle,
    surface: SurfaceId,
    viewport: Viewport,
) -> Option<BevyFrame> {
    pool.ensure_size(viewport.size);
    let (slot, token) = handle.lock().acquire(surface).ok()?;
    pool.register_view(app.world_mut(), CAMERA_VIEW_HANDLE, slot);
    app.update();
    // Ensure the render submission is complete before the host can composite:
    // same-queue ordering is sufficient for correctness, but polling here also
    // runs wgpu callbacks (maps, cleanups) so the adapter's own to_pixmap
    // readback cannot queue unboundedly behind live frames.
    let _ = pool.device().poll(wgpu::PollType::Wait {
        submission_index: None,
        timeout: None,
    });
    let frame = BevyFrame::new(token, std::sync::Arc::clone(pool.slot(slot)), pool.size());
    if handle
        .lock()
        .mark_ready_full(surface, slot, Some(Box::new(frame.clone())), None)
        .is_err()
    {
        // Should not happen (slot was just acquired), but never strand a
        // `Writing` slot.
        let _ = handle.lock().force_release(surface, slot);
        return None;
    }
    Some(frame)
}

/// Extracts a human-readable message from a caught panic payload.
fn panic_message(payload: &dyn std::any::Any) -> String {
    if let Some(s) = payload.downcast_ref::<&'static str>() {
        (*s).to_string()
    } else if let Some(s) = payload.downcast_ref::<String>() {
        s.clone()
    } else {
        "non-string panic payload".to_string()
    }
}

/// Blocking `COPY_SRC` → `MAP_READ` readback for [`Engine::to_pixmap`].
///
/// This is the explicit CPU-fallback path — it never runs on the streaming
/// path. `bgra` swizzles B/R channels so the returned `CpuFrame` is always
/// RGBA8 regardless of the swapchain-oriented texture format.
fn readback(
    device: &wgpu::Device,
    queue: &wgpu::Queue,
    texture: &wgpu::Texture,
    bgra: bool,
) -> Option<CpuFrame> {
    let (w, h) = (texture.width(), texture.height());
    if w == 0 || h == 0 {
        return None;
    }
    let bytes_per_row = w.checked_mul(4)?;
    let padded = bytes_per_row.div_ceil(wgpu::COPY_BYTES_PER_ROW_ALIGNMENT)
        * wgpu::COPY_BYTES_PER_ROW_ALIGNMENT;
    let buffer = device.create_buffer(&wgpu::BufferDescriptor {
        label: Some("martensite-bevy-readback"),
        size: u64::from(padded) * u64::from(h),
        usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
        mapped_at_creation: false,
    });
    let mut encoder = device.create_command_encoder(&wgpu::CommandEncoderDescriptor {
        label: Some("martensite-bevy-readback"),
    });
    encoder.copy_texture_to_buffer(
        wgpu::TexelCopyTextureInfo {
            texture,
            mip_level: 0,
            origin: wgpu::Origin3d::ZERO,
            aspect: wgpu::TextureAspect::All,
        },
        wgpu::TexelCopyBufferInfo {
            buffer: &buffer,
            layout: wgpu::TexelCopyBufferLayout {
                offset: 0,
                bytes_per_row: Some(padded),
                rows_per_image: Some(h),
            },
        },
        wgpu::Extent3d {
            width: w,
            height: h,
            depth_or_array_layers: 1,
        },
    );
    let submission = queue.submit([encoder.finish()]);

    let (tx, rx) = mpsc::channel();
    buffer
        .slice(..)
        .map_async(wgpu::MapMode::Read, move |result| {
            let _ = tx.send(result);
        });
    device
        .poll(wgpu::PollType::Wait {
            submission_index: Some(submission),
            timeout: None,
        })
        .ok()?;
    rx.recv().ok()?.ok()?;

    let mut pixels = vec![0u8; w as usize * h as usize * 4];
    {
        let view = buffer.slice(..).get_mapped_range().ok()?;
        for row in 0..h as usize {
            let src = &view[row * padded as usize..row * padded as usize + w as usize * 4];
            let dst = &mut pixels[row * w as usize * 4..(row + 1) * w as usize * 4];
            dst.copy_from_slice(src);
        }
    }
    buffer.unmap();

    if bgra {
        for px in pixels.as_chunks_mut::<4>().0 {
            px.swap(0, 2);
        }
    }
    Some(CpuFrame::new(w, h, pixels))
}

#[cfg(test)]
mod tests {
    use super::*;
    #[cfg(feature = "test-noop")]
    use bevy::math::Vec2;

    #[test]
    fn adapter_error_display() {
        assert!(
            BevyAdapterError::Init("boom".into())
                .to_string()
                .contains("boom")
        );
        assert!(
            BevyAdapterError::Terminated
                .to_string()
                .contains("terminated")
        );
    }

    /// Builds a `GpuContext` on wgpu's validating `noop` backend — no GPU.
    /// The full instance/adapter/device path is exercised so
    /// `RenderCreation::Manual` gets genuine handles.
    #[cfg(feature = "test-noop")]
    pub(crate) fn noop_gpu() -> GpuContext {
        let instance = wgpu::Instance::new(wgpu::InstanceDescriptor {
            backends: wgpu::Backends::NOOP,
            backend_options: wgpu::BackendOptions {
                noop: wgpu::NoopBackendOptions::enabled(),
                ..Default::default()
            },
            ..wgpu::InstanceDescriptor::new_without_display_handle_from_env()
        });
        let adapter =
            pollster::block_on(instance.request_adapter(&wgpu::RequestAdapterOptions::default()))
                .expect("noop adapter");
        let adapter_info = adapter.get_info();
        let (device, queue) =
            pollster::block_on(adapter.request_device(&wgpu::DeviceDescriptor::default()))
                .expect("noop device");
        GpuContext {
            instance: std::sync::Arc::new(instance),
            adapter: std::sync::Arc::new(adapter),
            device: std::sync::Arc::new(device),
            queue: std::sync::Arc::new(queue),
            adapter_info,
        }
    }

    /// Boots a real headless Bevy app on the noop backend: plugin graph,
    /// `RenderCreation::Manual` injection, camera spawn, and one rendered
    /// frame published into the ring.
    #[cfg(feature = "test-noop")]
    #[test]
    fn engine_constructs_and_renders_a_frame() {
        let gpu = noop_gpu();
        let handle = BridgeHandle::new();
        let surface = handle.lock().register();
        let mut engine = BevyEngine::new(
            &gpu,
            handle.clone(),
            surface,
            (64, 64),
            wgpu::TextureFormat::Rgba8UnormSrgb,
        )
        .expect("bevy app boots on the noop backend");

        let (device, queue) = ((*gpu.device).clone(), (*gpu.queue).clone());
        let mut ctx = EngineContext {
            device: &device,
            queue: &queue,
        };
        let frame = engine
            .render(&mut ctx, Viewport::new(64, 64, 1.0))
            .expect("first frame produced");
        assert_eq!(frame.size(), (64, 64));
        assert!(frame.same_device_texture().is_some());
        assert_eq!(engine.published(), 1);

        let reg = handle.lock();
        assert_eq!(reg.front_size(surface).unwrap(), Some((64, 64)));
        assert!(reg.front_frame(surface).unwrap().is_some());
    }

    /// Input events queue on `on_event` and are drained by the plugin during
    /// the next `render()` — verified through the world state afterwards.
    #[cfg(feature = "test-noop")]
    #[test]
    fn event_queue_drains_into_bevy_inputs() {
        let gpu = noop_gpu();
        let handle = BridgeHandle::new();
        let surface = handle.lock().register();
        let mut engine = BevyEngine::new(
            &gpu,
            handle,
            surface,
            (32, 32),
            wgpu::TextureFormat::Rgba8UnormSrgb,
        )
        .expect("bevy app boots on the noop backend");

        engine.on_event(&EngineEvent::PointerMove {
            position: [10.0, 12.0],
        });
        engine.on_event(&EngineEvent::PointerButton {
            position: [10.0, 12.0],
            button: martensite_engine_bridge::PointerButton::Primary,
            pressed: true,
        });
        engine.on_event(&EngineEvent::Key {
            scancode: 30,
            pressed: true,
        });

        let (device, queue) = ((*gpu.device).clone(), (*gpu.queue).clone());
        let mut ctx = EngineContext {
            device: &device,
            queue: &queue,
        };
        engine.render(&mut ctx, Viewport::new(32, 32, 1.0));

        let (pointer_loc, mouse_left, key_pressed) = engine
            .with_app(|app| {
                let world = app.world_mut();
                let loc = world
                    .query::<&bevy::picking::pointer::PointerLocation>()
                    .iter(world)
                    .next()
                    .and_then(|p| p.location().map(|l| l.position));
                let mouse = world
                    .resource::<bevy::input::ButtonInput<bevy::input::mouse::MouseButton>>()
                    .pressed(bevy::input::mouse::MouseButton::Left);
                let key = world
                    .resource::<bevy::input::ButtonInput<bevy::input::keyboard::KeyCode>>()
                    .get_pressed()
                    .count();
                (loc, mouse, key)
            })
            .expect("render thread alive");
        assert_eq!(pointer_loc, Some(Vec2::new(10.0, 12.0)));
        assert!(mouse_left);
        assert_eq!(key_pressed, 1);
    }

    /// A resized viewport re-allocates the slot textures and publishes at the
    /// new size.
    #[cfg(feature = "test-noop")]
    #[test]
    fn resize_recreates_slot_textures() {
        let gpu = noop_gpu();
        let handle = BridgeHandle::new();
        let surface = handle.lock().register();
        let mut engine = BevyEngine::new(
            &gpu,
            handle.clone(),
            surface,
            (16, 16),
            wgpu::TextureFormat::Rgba8UnormSrgb,
        )
        .expect("bevy app boots on the noop backend");
        let (device, queue) = ((*gpu.device).clone(), (*gpu.queue).clone());
        let mut ctx = EngineContext {
            device: &device,
            queue: &queue,
        };

        engine.render(&mut ctx, Viewport::new(16, 16, 1.0));
        let frame = engine
            .render(&mut ctx, Viewport::new(48, 24, 1.0))
            .expect("resized frame produced");
        assert_eq!(frame.size(), (48, 24));
        let reg = handle.lock();
        assert_eq!(reg.front_size(surface).unwrap(), Some((48, 24)));
    }

    /// Real-GPU smoke test: boots the engine on the host's actual adapter and
    /// renders + reads back a frame. Excluded from CI — run explicitly with
    /// `cargo test -p martensite-bevy -- --ignored` on a GPU machine.
    #[test]
    #[ignore = "requires a real GPU adapter"]
    fn real_gpu_frame_smoke() {
        let Some(gpu) = GpuContext::new().ok() else {
            eprintln!("no GPU adapter available; skipping");
            return;
        };
        let handle = BridgeHandle::new();
        let surface = handle.lock().register();
        let mut engine = BevyEngine::new(
            &gpu,
            handle.clone(),
            surface,
            (64, 64),
            wgpu::TextureFormat::Rgba8UnormSrgb,
        )
        .expect("bevy app boots on a real GPU");
        let (device, queue) = ((*gpu.device).clone(), (*gpu.queue).clone());
        let mut ctx = EngineContext {
            device: &device,
            queue: &queue,
        };
        let frame = engine
            .render(&mut ctx, Viewport::new(64, 64, 1.0))
            .expect("real-GPU frame produced");
        let pixmap = engine.to_pixmap(frame.token()).expect("readback");
        assert_eq!((pixmap.width, pixmap.height), (64, 64));
    }

    /// `to_pixmap` returns a CPU raster for a live token — zeroed pixels on
    /// the noop backend, correct dimensions.
    #[cfg(feature = "test-noop")]
    #[test]
    fn to_pixmap_reads_back_frame() {
        let gpu = noop_gpu();
        let handle = BridgeHandle::new();
        let surface = handle.lock().register();
        let mut engine = BevyEngine::new(
            &gpu,
            handle,
            surface,
            (8, 8),
            wgpu::TextureFormat::Rgba8UnormSrgb,
        )
        .expect("bevy app boots on the noop backend");
        let (device, queue) = ((*gpu.device).clone(), (*gpu.queue).clone());
        let mut ctx = EngineContext {
            device: &device,
            queue: &queue,
        };
        let frame = engine
            .render(&mut ctx, Viewport::new(8, 8, 1.0))
            .expect("frame produced");
        let pixmap = engine.to_pixmap(frame.token()).expect("readback works");
        assert_eq!((pixmap.width, pixmap.height), (8, 8));
        assert_eq!(pixmap.pixels.len(), 8 * 8 * 4);
    }
}
