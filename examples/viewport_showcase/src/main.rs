//! `viewport_showcase` — the v0.15.0 engine-adapter verification app.
//!
//! One Martensite window, two live external engines, ordinary UI around
//! both. On the left a [`martensite_bevy::BevyEngine`] runs a headless
//! Bevy `App` whose `Camera3d` renders a PBR cube into ring-slot
//! `wgpu::Texture`s on the host's own device (zero-copy composite). On
//! the right a [`martensite_godot::host::GodotEngine`] drains `FrameMsg`s
//! off a loopback TCP listener fed by the Godot GDExtension — and shows
//! an honest placeholder until a Godot editor actually connects:
//!
//! ```text
//! ┌─ Martensite — Engine Showcase (v0.15.0) ──────────────── winit chrome ┐
//! │ header strip (ordinary PaintList content)                            │
//! │ ┌─ Bevy viewport ────────┐  ┌─ Godot viewport ────────────────────┐ │
//! │ │ PaintCommand::External │  │ placeholder fill + status text OR   │ │
//! │ │ → bridge ring front    │  │ PaintCommand::External once frames  │ │
//! │ │   frame (wgpu texture) │  │ arrive over TCP                     │ │
//! │ └────────────────────────┘  └─────────────────────────────────────┘ │
//! │ footer strip (painted OVER both External markers — z-order proof)    │
//! └──────────────────────────────────────────────────────────────────────┘
//! ```
//!
//! Input: `CursorMoved`/`MouseInput`/`MouseWheel` hit-test against the
//! laid-out panel rects and forward surface-local [`EngineEvent`]s;
//! `KeyboardInput` goes to the panel that last received a pointer press.

use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use martensite::core::{HotNode, LayoutContext, Rect, Widget};
use martensite::widgets::external::{ExternalEngine, ExternalEngines};
use martensite_bevy::bevy::prelude::{Mesh3d, Query, Res, Time, Transform, With};
use martensite_bevy::{bevy, BevyEngine};
use martensite_engine_bridge::{
    BridgeHandle, EngineContext, EngineEvent, FrameToken, PointerButton, SurfaceId,
};
use martensite_godot::host::GodotEngine;
use martensite_render::{PaintList, Point, Rect as PaintRect};
use martensite_wgpu::{
    BackdropMode, GpuContext, OrchestratorConfig, PresentModePreference, RecoveryMachine,
    RenderOrchestrator, SurfaceWrapper,
};
use winit::application::ApplicationHandler;
use winit::dpi::{PhysicalPosition, PhysicalSize};
use winit::event::{ButtonSource, ElementState, MouseButton, MouseScrollDelta, WindowEvent};
use winit::event_loop::{ActiveEventLoop, ControlFlow, EventLoop};
use winit::keyboard::{NativeKeyCode, PhysicalKey};
use winit::window::{Window, WindowAttributes, WindowId};

/// Window margin around the panel layout, in physical pixels.
const MARGIN: f32 = 24.0;
/// Height of the header strip above the panels.
const HEADER_H: f32 = 44.0;
/// Gap under the header reserved for per-panel captions.
const CAPTION_H: f32 = 22.0;
/// Height of the footer strip below the panels.
const FOOTER_H: f32 = 28.0;
/// Horizontal gap between the two viewports.
const GUTTER: f32 = 20.0;
/// Loopback address the host listens on for Godot `FrameMsg`s. Launch
/// the editor-side extension pointed at this address to stream frames.
const GODOT_ADDR: &str = "127.0.0.1:7890";
/// Line-delta → physical-pixel conversion for `EngineEvent::Scroll`
/// (the bridge contract is physical pixels; a line is ~16 px).
const LINE_HEIGHT_PX: f32 = 16.0;

/// Computes the laid-out bounds of the two viewports for a window of
/// `size` — Bevy on the left, Godot on the right, split evenly across
/// the content area between the caption band and the footer.
fn panel_bounds(size: PhysicalSize<u32>) -> (Rect, Rect) {
    let w = size.width as f32;
    let h = size.height as f32;
    let top = MARGIN + HEADER_H + CAPTION_H;
    let height = (h - top - FOOTER_H - MARGIN).max(1.0);
    let width = ((w - MARGIN * 2.0 - GUTTER) / 2.0).max(1.0);
    let left = Rect::new(MARGIN, top, width, height);
    let right = Rect::new(MARGIN + width + GUTTER, top, width, height);
    (left, right)
}

/// Lays both widgets into their panel rects and refreshes the hit-test
/// map. `layout` also pushes each panel's physical bounds + DPI to the
/// bridge (`BridgeRegistry::set_viewport`) — the producers' size
/// contract for the next `drive_frame`.
fn layout_both(
    bevy_panel: &mut ExternalEngine,
    godot_panel: &mut ExternalEngine,
    bevy_surface: SurfaceId,
    godot_surface: SurfaceId,
    panels: &mut HashMap<SurfaceId, Rect>,
    size: PhysicalSize<u32>,
) {
    let (left, right) = panel_bounds(size);
    let mut hot = HotNode::default();
    bevy_panel.layout(&mut LayoutContext { hot: &mut hot }, left);
    godot_panel.layout(&mut LayoutContext { hot: &mut hot }, right);
    panels.insert(bevy_surface, left);
    panels.insert(godot_surface, right);
}

/// Core `Rect` (origin + size) → `kurbo::Rect` (corner pair).
fn to_paint_rect(rect: Rect) -> PaintRect {
    PaintRect::new(
        f64::from(rect.min_x()),
        f64::from(rect.min_y()),
        f64::from(rect.max_x()),
        f64::from(rect.max_y()),
    )
}

/// `winit` button source → bridge [`PointerButton`], or `None` for
/// sources the bridge cannot represent (touch contacts, tablet tools).
fn pointer_button(source: ButtonSource) -> Option<PointerButton> {
    match source {
        ButtonSource::Mouse(button) => Some(match button {
            MouseButton::Left => PointerButton::Primary,
            MouseButton::Right => PointerButton::Secondary,
            MouseButton::Middle => PointerButton::Middle,
            MouseButton::Back => PointerButton::Back,
            MouseButton::Forward => PointerButton::Forward,
            // `Button6..=Button29` carry an explicit discriminant — it is
            // the platform button index, exactly `PointerButton::Other`.
            other => PointerButton::Other(other as u16),
        }),
        ButtonSource::Unknown(code) => Some(PointerButton::Other(code)),
        _ => None,
    }
}

/// `winit` scroll delta → physical-pixel `[x, y]` (the bridge contract),
/// or `None` for delta kinds added to the non-exhaustive enum later.
fn scroll_pixels(delta: MouseScrollDelta) -> Option<[f32; 2]> {
    match delta {
        MouseScrollDelta::LineDelta(x, y) => Some([x * LINE_HEIGHT_PX, y * LINE_HEIGHT_PX]),
        MouseScrollDelta::PixelDelta(pos) => Some([pos.x as f32, pos.y as f32]),
        _ => None,
    }
}

/// Best-effort stable key code for `EngineEvent::Key`.
///
/// winit 0.31 no longer exposes the platform scancode on `KeyEvent`, so
/// identified keys carry the `KeyCode` discriminant instead — still a
/// stable per-key value, which is what press/release pairing needs.
/// `Unidentified` keys forward their native platform code unchanged.
fn scancode_of(key: PhysicalKey) -> u32 {
    match key {
        PhysicalKey::Code(code) => code as u32,
        PhysicalKey::Unidentified(native) => match native {
            NativeKeyCode::Android(code) | NativeKeyCode::Xkb(code) => code,
            NativeKeyCode::MacOS(code) | NativeKeyCode::Windows(code) => u32::from(code),
            // `NativeKeyCode` is non-exhaustive; `Unidentified` and any
            // future variants collapse to a sentinel.
            _ => u32::MAX,
        },
    }
}

/// Scene authoring for the Bevy panel — runs inside
/// [`BevyEngine::with_app`] on the adapter's render thread (a `bevy::App`
/// is `!Send`, so it can never leave that thread). The adapter has
/// already spawned a `Camera3d` at `(0, 0, 5)` looking at the origin
/// and targeting `CAMERA_VIEW_HANDLE`; this adds the subject matter.
fn spawn_showcase_scene(app: &mut bevy::app::App) {
    use martensite_bevy::bevy::prelude::*;

    app.insert_resource(ClearColor(Color::srgb(0.025, 0.03, 0.05)));

    let mesh = app
        .world_mut()
        .resource_mut::<Assets<Mesh>>()
        .add(Cuboid::new(1.6, 1.6, 1.6));
    let material = app
        .world_mut()
        .resource_mut::<Assets<StandardMaterial>>()
        .add(StandardMaterial {
            base_color: Color::srgb(0.32, 0.58, 0.96),
            metallic: 0.7,
            perceptual_roughness: 0.3,
            ..Default::default()
        });
    app.world_mut().spawn((
        Mesh3d(mesh),
        MeshMaterial3d(material),
        Transform::IDENTITY,
        Name::new("showcase-cube"),
    ));
    app.world_mut().spawn((
        PointLight {
            intensity: 900_000.0,
            shadow_maps_enabled: true,
            ..Default::default()
        },
        Transform::from_xyz(4.0, 6.0, 4.0),
    ));

    // One `Update` system keeps the cube tumbling — one `App::update`
    // runs per `Engine::render`, so animation cadence follows the host's
    // redraw pump.
    app.add_systems(Update, spin_showcase_meshes);
}

/// Bevy `Update` system: slow tumbling rotation on every spawned mesh
/// (this scene has exactly one). A `With<Mesh3d>` filter stands in for a
/// marker component — Bevy's `Component` derive resolves its own crate
/// names from the *depending* crate's manifest, which this example does
/// not have, so a plain query filter is the honest option.
fn spin_showcase_meshes(time: Res<Time>, mut meshes: Query<&mut Transform, With<Mesh3d>>) {
    let dt = time.delta_secs();
    for mut transform in &mut meshes {
        transform.rotate_y(dt * 0.9);
        transform.rotate_x(dt * 0.4);
    }
}

/// The demo app. Everything GPU/window-bound is created lazily inside
/// `can_create_surfaces`, per the winit 0.31 lifecycle.
struct App {
    /// One bridge registry shared by both producers, both widgets, and
    /// the compositor — each panel pair references its own `SurfaceId`.
    bridge: BridgeHandle,
    /// Ring surface for the Bevy panel (left).
    bevy_surface: SurfaceId,
    /// Ring surface for the Godot panel (right).
    godot_surface: SurfaceId,
    /// Bound producers. Behind a mutex because the orchestrator's
    /// CPU-frame resolver also calls `cpu_frame_for` inside `render`.
    engines: Arc<Mutex<ExternalEngines>>,

    /// Laid-out panel bounds per surface — the hit-test map for input
    /// forwarding, refreshed on every layout.
    panel_rects: HashMap<SurfaceId, Rect>,
    /// Panel that holds keyboard focus (the last one pointer-pressed).
    focused_panel: Option<SurfaceId>,
    /// Last cursor position in window physical pixels.
    cursor: PhysicalPosition<f64>,
    /// Status line drawn inside the Godot panel while no frames have
    /// arrived — also reports a listener-bind failure.
    godot_status: String,
    /// Latches true once the Godot widget has observed a real frame;
    /// the placeholder is only painted while this is false.
    godot_streaming: bool,

    window: Option<Arc<dyn Window>>,
    gpu: Option<GpuContext>,
    surface: Option<SurfaceWrapper<'static>>,
    orchestrator: Option<RenderOrchestrator>,
    bevy_widget: Option<ExternalEngine>,
    godot_widget: Option<ExternalEngine>,

    /// Device-loss FSM driving the GPU → TinySkia fallback decision.
    recovery: RecoveryMachine,
    /// Set on init/resize/scale change; cleared once layout runs.
    needs_layout: bool,
}

impl App {
    fn new() -> Self {
        let bridge = BridgeHandle::new();
        let bevy_surface = bridge.lock().register();
        let godot_surface = bridge.lock().register();

        let mut engines = ExternalEngines::new();
        // The Godot receiver needs no GPU: it binds a loopback listener
        // and spawns its transport-draining thread now. A bind failure
        // (e.g. the port is taken) is reported in the placeholder rather
        // than killing the demo — the Bevy half still verifies v0.15.0.
        let godot_status = match GodotEngine::new_tcp(bridge.clone(), godot_surface, GODOT_ADDR) {
            Ok(engine) => {
                engines
                    .bind(bridge.clone(), godot_surface, Box::new(engine))
                    .expect("first binding on the godot surface");
                format!("awaiting GDExtension connection on {GODOT_ADDR}")
            }
            Err(err) => format!("listener on {GODOT_ADDR} failed: {err}"),
        };

        Self {
            bridge,
            bevy_surface,
            godot_surface,
            engines: Arc::new(Mutex::new(engines)),
            panel_rects: HashMap::new(),
            focused_panel: None,
            cursor: PhysicalPosition::new(0.0, 0.0),
            godot_status,
            godot_streaming: false,
            window: None,
            gpu: None,
            surface: None,
            orchestrator: None,
            bevy_widget: None,
            godot_widget: None,
            recovery: RecoveryMachine::new(),
            needs_layout: true,
        }
    }

    /// Hit-test `pos` (window physical pixels) against the laid-out
    /// panel rects; returns the surface and the surface-local position
    /// — panel origin subtracted, per the `EngineEvent` contract.
    fn panel_at(&self, pos: PhysicalPosition<f64>) -> Option<(SurfaceId, [f32; 2])> {
        let (x, y) = (pos.x as f32, pos.y as f32);
        for (surface, rect) in &self.panel_rects {
            if x >= rect.min_x() && x < rect.max_x() && y >= rect.min_y() && y < rect.max_y() {
                return Some((*surface, [x - rect.min_x(), y - rect.min_y()]));
            }
        }
        None
    }

    /// Routes one event to the engine bound to `surface`.
    /// `ExternalEngines::forward_event` quarantines a panicking engine,
    /// so a hostile event stream cannot take down the frame loop.
    fn forward(&mut self, surface: SurfaceId, event: &EngineEvent) {
        self.engines
            .lock()
            .expect("engines mutex")
            .forward_event(surface, event);
    }

    /// One frame of the producer → ring → composite loop.
    fn redraw(&mut self) {
        let Self {
            window: Some(window),
            gpu: Some(gpu),
            surface: Some(surface),
            orchestrator: Some(orchestrator),
            bevy_widget: Some(bevy_panel),
            godot_widget: Some(godot_panel),
            engines,
            recovery,
            bevy_surface,
            godot_surface,
            panel_rects,
            godot_streaming,
            godot_status,
            needs_layout,
            ..
        } = self
        else {
            return;
        };

        // 1. Layout — forwards each widget's physical bounds + DPI as
        //    its bridge viewport and refreshes the input hit-test map.
        if *needs_layout {
            layout_both(
                bevy_panel,
                godot_panel,
                *bevy_surface,
                *godot_surface,
                panel_rects,
                window.surface_size(),
            );
            *needs_layout = false;
        }

        // 2. Drive producers: `drive_frame` renders each bound engine at
        //    its stored viewport (Bevy pumps one `App::update`; Godot
        //    uploads the newest transport frame if one arrived) and
        //    drains the ready queues.
        let ready = {
            let mut ctx = EngineContext {
                device: &gpu.device,
                queue: &gpu.queue,
            };
            engines.lock().expect("engines mutex").drive_frame(&mut ctx)
        };

        // 3. Consume FramePolls: a size change re-runs layout, a new
        //    frame warrants repaint. The Godot panel flips to streaming
        //    the first time a real frame sets its intrinsic size.
        let bevy_poll = bevy_panel.poll_frame();
        let godot_poll = godot_panel.poll_frame();
        if godot_panel.intrinsic_size().x > 0.0 {
            *godot_streaming = true;
        }
        if bevy_poll.needs_layout() || godot_poll.needs_layout() {
            layout_both(
                bevy_panel,
                godot_panel,
                *bevy_surface,
                *godot_surface,
                panel_rects,
                window.surface_size(),
            );
        }
        if !ready.is_empty() || bevy_poll.needs_redraw() || godot_poll.needs_redraw() {
            window.request_redraw();
        }

        // 4. Record paint — ordinary Martensite commands under, between,
        //    and over the two `PaintCommand::External` markers.
        let size = window.surface_size();
        let (w, h) = (f64::from(size.width), f64::from(size.height));
        let m = f64::from(MARGIN);
        let bevy_bounds = panel_rects.get(bevy_surface).copied().unwrap_or_default();
        let godot_bounds = panel_rects.get(godot_surface).copied().unwrap_or_default();

        let mut list = PaintList::new();
        list.push_fill_rect(PaintRect::new(0.0, 0.0, w, h), [15, 17, 23, 255]);
        // Header strip + title.
        list.push_fill_rect(
            PaintRect::new(m, m, w - m, m + f64::from(HEADER_H)),
            [24, 27, 36, 255],
        );
        list.push_text(
            Point::new(m + 14.0, m + 13.0),
            "Engine Showcase — real engine adapters, one bridge".to_string(),
            18.0,
            [208, 214, 230, 255],
        );
        // Panel captions (drawn above each viewport's top edge).
        list.push_text(
            Point::new(
                f64::from(bevy_bounds.min_x()),
                f64::from(bevy_bounds.min_y()) - 18.0,
            ),
            "Bevy — PBR viewport (same-device wgpu, zero-copy)".to_string(),
            13.0,
            [140, 180, 235, 255],
        );
        list.push_text(
            Point::new(
                f64::from(godot_bounds.min_x()),
                f64::from(godot_bounds.min_y()) - 18.0,
            ),
            "Godot — SubViewport frames via transport".to_string(),
            13.0,
            [140, 235, 180, 255],
        );
        // Panel outlines.
        list.push_stroke_rect(to_paint_rect(bevy_bounds), 1.0, [70, 110, 180, 255]);
        list.push_stroke_rect(to_paint_rect(godot_bounds), 1.0, [70, 160, 110, 255]);
        // Honest placeholder for the Godot half: painted UNDER the
        // External marker, only until the first real frame lands.
        if !*godot_streaming {
            list.push_fill_rect(to_paint_rect(godot_bounds), [22, 24, 32, 255]);
            let gx = f64::from(godot_bounds.min_x());
            let gy = f64::from(godot_bounds.min_y());
            let gh = f64::from(godot_bounds.height());
            list.push_text(
                Point::new(gx + 16.0, gy + gh * 0.5 - 12.0),
                format!("Godot — {godot_status}"),
                15.0,
                [150, 160, 180, 255],
            );
            list.push_text(
                Point::new(gx + 16.0, gy + gh * 0.5 + 12.0),
                "build the cdylib: cargo build -p martensite-godot".to_string(),
                12.0,
                [110, 118, 138, 255],
            );
        }
        // The two External markers in z-order: Bevy first, Godot second.
        bevy_panel.record_paint(&mut list);
        godot_panel.record_paint(&mut list);
        // Footer painted AFTER both markers — ordinary content
        // composited over the external frames.
        list.push_fill_rect(
            PaintRect::new(0.0, h - f64::from(FOOTER_H), w, h),
            [20, 22, 30, 255],
        );
        list.push_text(
            Point::new(m, h - f64::from(FOOTER_H) + 7.0),
            "click a viewport to focus input — pointer, wheel and keys forward as EngineEvents"
                .to_string(),
            13.0,
            [120, 128, 150, 255],
        );

        // 5. Composite + present. `render` splits the list at both
        //    External markers into Vello segments; `render_to_surface`
        //    takes each ring's front frame and composites it in exact
        //    paint order, submits, releases the slots, fires
        //    `pre_present_notify`, and presents.
        orchestrator.render(&list, recovery);
        if let Err(err) = orchestrator.render_to_surface(&gpu.device, &gpu.queue, surface) {
            recovery.handle_surface_error(err);
            let _ = surface.resize(&gpu.device, size.width.max(1), size.height.max(1));
        }

        // 6. Producer recycling: released ring-slot tokens flow back to
        //    both engines after `queue.submit`.
        engines.lock().expect("engines mutex").drain_released();

        // 7. Bevy produces every tick — keep the loop animating.
        window.request_redraw();
    }
}

impl ApplicationHandler for App {
    fn can_create_surfaces(&mut self, event_loop: &dyn ActiveEventLoop) {
        if self.window.is_some() {
            return;
        }

        // Native chrome: winit's ordinary decorated window (OS title bar,
        // OS resize grips) — Martensite content starts at the surface
        // bounds. The panels below are not child windows or platform
        // views; they are paint markers inside this one surface.
        let window: Arc<dyn Window> = event_loop
            .create_window(
                WindowAttributes::default()
                    .with_title("Martensite — Engine Showcase (v0.15.0)")
                    .with_surface_size(PhysicalSize::new(1600, 940)),
            )
            .expect("create window")
            .into();

        // GPU: surface first, then a compatible adapter — the adapter is
        // guaranteed to be able to present to this surface. The surface
        // and adapter must come from the same instance (wgpu panics on
        // foreign-instance surfaces), so `for_surface` gets the instance
        // that created it.
        let instance = wgpu::Instance::default();
        let raw_surface = instance
            .create_surface(Arc::clone(&window))
            .expect("create surface");
        let gpu = pollster::block_on(GpuContext::for_surface(&instance, &raw_surface))
            .expect("request surface-compatible GPU context");

        // Swapchain — low-latency pacing for streaming content.
        let size = window.surface_size();
        let mut surface = SurfaceWrapper::new(raw_surface);
        surface.set_pacing(PresentModePreference::LowLatency);
        surface
            .configure(
                &gpu.device,
                &gpu.adapter,
                size.width.max(1),
                size.height.max(1),
                BackdropMode::Opaque,
            )
            .expect("configure surface");

        // Orchestrator — Vello GPU path with TinySkia fallback allowed
        // (the cpu_frame_resolver below feeds it `Engine::to_pixmap`).
        let mut orchestrator = RenderOrchestrator::new(
            size.width.max(1),
            size.height.max(1),
            OrchestratorConfig::new(true, false),
        )
        .expect("orchestrator");

        // Bridge install: `PaintCommand::External` markers now resolve
        // through the rings — `composite_front` takes each surface's
        // freshest frame zero-copy in paint order.
        orchestrator.set_bridge(self.bridge.clone());

        // winit pacing contract — notify the compositor before present.
        let notify_window = Arc::clone(&window);
        orchestrator.set_pre_present_notify(Some(Box::new(move || {
            notify_window.pre_present_notify();
        })));

        // CPU-fallback raster source: the bound engines' `to_pixmap`
        // (Godot ships a CpuFrame alongside every frame; Bevy reads back
        // the slot texture on demand).
        let resolver_engines = Arc::clone(&self.engines);
        orchestrator.set_cpu_frame_resolver(Some(Box::new(move |surface, token| {
            resolver_engines
                .lock()
                .expect("engines mutex")
                .cpu_frame_for(surface, FrameToken(token))
        })));

        // Producer → host wake: every published frame schedules a
        // repaint. The waker runs under the registry lock — it must only
        // signal, never re-lock the bridge.
        let waker_window = Arc::clone(&window);
        self.bridge.set_ready_waker(Some(Box::new(move |_surface| {
            waker_window.request_redraw();
        })));

        // The Bevy engine: `RenderCreation::Manual` inside the adapter
        // clones THIS GpuContext's device/queue, so every published
        // frame is already a same-device texture for the composite pass.
        // The format must match the negotiated swapchain format.
        let format = surface
            .configuration()
            .expect("surface is configured")
            .format;
        let (bevy_bounds, _) = panel_bounds(size);
        let bevy_engine = BevyEngine::new(
            &gpu,
            self.bridge.clone(),
            self.bevy_surface,
            (
                bevy_bounds.width().max(1.0) as u32,
                bevy_bounds.height().max(1.0) as u32,
            ),
            format,
        )
        .expect("bevy engine booted");
        // Scene authoring crosses to the render thread as a closure.
        let _ = bevy_engine.with_app(spawn_showcase_scene);
        self.engines
            .lock()
            .expect("engines mutex")
            .bind(
                self.bridge.clone(),
                self.bevy_surface,
                Box::new(bevy_engine),
            )
            .expect("first binding on the bevy surface");

        // The widgets — retained leaves whose paint output is an
        // External marker at each one's laid-out rect.
        self.bevy_widget = Some(
            ExternalEngine::new(self.bridge.clone(), self.bevy_surface)
                .with_scale_factor(window.scale_factor())
                .with_aspect_ratio(16.0 / 9.0)
                .with_label("Bevy 3D viewport"),
        );
        self.godot_widget = Some(
            ExternalEngine::new(self.bridge.clone(), self.godot_surface)
                .with_scale_factor(window.scale_factor())
                .with_label("Godot viewport"),
        );

        self.window = Some(window);
        self.gpu = Some(gpu);
        self.surface = Some(surface);
        self.orchestrator = Some(orchestrator);
        self.needs_layout = true;

        // Kick the first frame; the loop is self-sustaining from here
        // (ready-waker + trailing request_redraw).
        if let Some(window) = &self.window {
            window.request_redraw();
        }
    }

    fn window_event(
        &mut self,
        event_loop: &dyn ActiveEventLoop,
        _id: WindowId,
        event: WindowEvent,
    ) {
        match event {
            WindowEvent::CloseRequested => event_loop.exit(),
            WindowEvent::SurfaceResized(size) => {
                if size.width > 0 && size.height > 0 {
                    if let (Some(surface), Some(gpu)) = (&mut self.surface, &self.gpu) {
                        let _ = surface.resize(&gpu.device, size.width, size.height);
                    }
                    self.needs_layout = true;
                }
            }
            WindowEvent::ScaleFactorChanged { .. } => {
                // Push the new DPI to both widgets — the next layout
                // re-pushes the bridge viewports at the new scale.
                if let Some(window) = &self.window {
                    let scale = window.scale_factor();
                    if let Some(widget) = &mut self.bevy_widget {
                        widget.set_scale_factor(scale);
                    }
                    if let Some(widget) = &mut self.godot_widget {
                        widget.set_scale_factor(scale);
                    }
                }
                self.needs_layout = true;
                if let Some(window) = &self.window {
                    window.request_redraw();
                }
            }
            // ── Input forwarding (v0.15.0) ────────────────────────────
            // Pointer events hit-test `panel_rects` and are forwarded
            // with the panel origin subtracted (surface-local physical
            // pixels). Keyboard events go to the focused panel — the one
            // that last received a pointer press. Only `primary` pointer
            // events are forwarded: the bridge event model is
            // mouse-shaped, so secondary touch contacts stay out.
            WindowEvent::PointerMoved {
                position, primary, ..
            } => {
                self.cursor = position;
                if primary {
                    if let Some((surface, local)) = self.panel_at(position) {
                        self.forward(surface, &EngineEvent::PointerMove { position: local });
                    }
                }
            }
            WindowEvent::PointerButton {
                state,
                position,
                button,
                primary,
                ..
            } => {
                if !primary {
                    return;
                }
                let Some(button) = pointer_button(button) else {
                    return;
                };
                if let Some((surface, local)) = self.panel_at(position) {
                    let pressed = state == ElementState::Pressed;
                    if pressed {
                        self.focused_panel = Some(surface);
                    }
                    self.forward(
                        surface,
                        &EngineEvent::PointerButton {
                            position: local,
                            button,
                            pressed,
                        },
                    );
                }
            }
            WindowEvent::MouseWheel { delta, .. } => {
                // Wheel events carry no position — the last reported
                // cursor position decides which panel scrolls.
                let Some(delta) = scroll_pixels(delta) else {
                    return;
                };
                if let Some((surface, local)) = self.panel_at(self.cursor) {
                    self.forward(
                        surface,
                        &EngineEvent::Scroll {
                            position: local,
                            delta,
                        },
                    );
                }
            }
            WindowEvent::KeyboardInput { event, .. } => {
                if let Some(surface) = self.focused_panel {
                    let pressed = event.state == ElementState::Pressed;
                    self.forward(
                        surface,
                        &EngineEvent::Key {
                            scancode: scancode_of(event.physical_key),
                            pressed,
                        },
                    );
                    // Committed text rides its own event so engines can
                    // keep key state and text input separate.
                    if pressed && !event.repeat {
                        if let Some(text) = &event.text {
                            let text = text.to_string();
                            if !text.is_empty() {
                                self.forward(surface, &EngineEvent::TextInput { text });
                            }
                        }
                    }
                }
            }
            WindowEvent::Focused(focused) => {
                // Focus belongs to the focused panel, not the cursor
                // position — on loss the engine clears its pressed-key
                // caches (KeyboardFocusLost on the Bevy side).
                if let Some(surface) = self.focused_panel {
                    self.forward(surface, &EngineEvent::Focus { focused });
                }
            }
            WindowEvent::RedrawRequested => self.redraw(),
            _ => {}
        }
    }
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let event_loop = EventLoop::new()?;
    // Poll keeps the producer cadence continuous; each RedrawRequested
    // re-arms the next, and Fifo-present acquisition paces the loop.
    event_loop.set_control_flow(ControlFlow::Poll);
    event_loop.run_app(App::new())?;
    Ok(())
}
