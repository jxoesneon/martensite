//! Window assembly — a distilled `engine_embed`/`industrial_dashboard`
//! pipeline: winit + `RenderOrchestrator` (Vello with TinySkia
//! fallback), one `WidgetArena` rooted at [`MorphViewer`], real
//! pointer routing through `EventRouter`, and the dev-channel socket
//! (`MARTENSITE_DEV_CHANNEL=1`) so `cargo martensite mcp` can inspect
//! and drive the running showcase.

use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use glam::Vec2;
use martensite::core::{HotNode, LayoutContext, NodeFlags, PaintList, Rect, WidgetId};
use martensite::devtools::dev_session::SignalAdapter;
use martensite::focus::FocusManager;
use martensite::reactive::Signal;
use martensite::theme::ThemeDictionary;
use martensite::window::event::MouseButton as MButton;
use martensite::window::{EventRouter, ModifierKeys, PointerEvent, PointerId, PointerKind};
use martensite_wgpu::{
    BackdropMode, GpuContext, OrchestratorConfig, PresentModePreference, RecoveryMachine,
    RenderOrchestrator, SurfaceWrapper,
};
use winit::application::ApplicationHandler;
use winit::dpi::LogicalSize;
use winit::event::{ButtonSource, ElementState, MouseButton, WindowEvent};
use winit::event_loop::{ActiveEventLoop, ControlFlow, EventLoop};
use winit::window::{Window, WindowAttributes, WindowId};

use crate::viewer::{MorphViewer, ViewerSignals};

/// The windowed app state. GPU/window-bound members are created lazily
/// in `can_create_surfaces` per the winit 0.31 lifecycle.
struct App {
    signals: ViewerSignals,
    arena: Option<Arc<Mutex<martensite::core::WidgetArena>>>,
    root: Option<WidgetId>,
    router: EventRouter,
    /// Focus sink for the single-widget app: the viewer holds focus so
    /// the dev session's `event_dispatch` can route `key_press` /
    /// `text_input` to it.
    focus: Arc<Mutex<FocusManager>>,
    mods: winit::keyboard::ModifiersState,
    window: Option<Arc<dyn Window>>,
    gpu: Option<GpuContext>,
    surface: Option<SurfaceWrapper<'static>>,
    orchestrator: Option<RenderOrchestrator>,
    recovery: RecoveryMachine,
    dev_session: Option<Arc<martensite::devtools::dev_session::DevSession>>,
    /// Held only for its `Drop` (stops the listener, unlinks the sock).
    _dev_server: Option<martensite::dev_channel::DevChannelServer>,
    needs_layout: bool,
    last_frame: Instant,
}

impl App {
    fn new() -> Self {
        Self {
            signals: ViewerSignals {
                pack: Signal::new(0u32),
                scroll: Signal::new(0u32),
                paused: Signal::new(false),
                select: Signal::new(0u32),
                base: Signal::new(-1i64),
                target: Signal::new(-1i64),
                filter: Signal::new(String::new()),
                sort: Signal::new(String::new()),
                category: Signal::new(String::new()),
            },
            arena: None,
            root: None,
            router: EventRouter::new(),
            focus: Arc::new(Mutex::new(FocusManager::new())),
            mods: winit::keyboard::ModifiersState::empty(),
            window: None,
            gpu: None,
            surface: None,
            orchestrator: None,
            recovery: RecoveryMachine::new(),
            dev_session: None,
            _dev_server: None,
            needs_layout: true,
            last_frame: Instant::now(),
        }
    }

    /// Builds the arena: dark theme, shared text painter, platform
    /// preferences, the viewer root, and — when
    /// `MARTENSITE_DEV_CHANNEL=1` — the session socket with the four
    /// viewer signals registered for `signal_trigger`.
    fn build_arena(&mut self, scale: f32) {
        let mut arena = martensite::core::WidgetArena::new();
        arena.set_theme(ThemeDictionary::new().dark_theme().clone());
        arena.set_scale_factor(scale);
        arena.set_text_painter(martensite::text_paint::shared_painter());
        martensite::window::prefs::apply_platform_preferences(&mut arena);

        let mut hot = HotNode::default();
        hot.flags |= NodeFlags::VISIBLE | NodeFlags::HIT_TEST_ENABLED;
        let viewer = MorphViewer::new(self.signals.clone());
        let root = arena.insert_with_widget(hot, Box::new(viewer));
        // The viewer is the app's only key target — the session's
        // `key_press`/`text_input` dispatches resolve through this.
        self.focus.lock().unwrap().set_focus_unchecked(root);

        let arena = Arc::new(Mutex::new(arena));
        match martensite::dev_channel::serve_dev_session_from_env(Arc::clone(&arena)) {
            Ok(Some((server, session))) => {
                session.attach_focus_manager(Arc::clone(&self.focus));
                for adapter in [
                    signal_adapter("pack", self.signals.pack.clone()),
                    signal_adapter("scroll", self.signals.scroll.clone()),
                    signal_adapter("paused", self.signals.paused.clone()),
                    signal_adapter("select", self.signals.select.clone()),
                    signal_adapter("base", self.signals.base.clone()),
                    signal_adapter("target", self.signals.target.clone()),
                    signal_adapter("filter", self.signals.filter.clone()),
                    signal_adapter("sort", self.signals.sort.clone()),
                    signal_adapter("category", self.signals.category.clone()),
                ] {
                    session.register_signal_adapter(adapter);
                }
                self.dev_session = Some(session);
                self._dev_server = Some(server);
                eprintln!(
                    "morph_viewer: dev channel serving (pid {})",
                    std::process::id()
                );
            }
            Ok(None) => {}
            Err(err) => eprintln!("morph_viewer: dev channel disabled: {err}"),
        }
        self.arena = Some(arena);
        self.root = Some(root);
    }

    /// Lays the root widget across the full window — single-surface
    /// layout, no dock split.
    fn layout(&mut self, w: u32, h: u32) {
        let Some(arena) = &self.arena else { return };
        let Some(root) = self.root else { return };
        let mut arena = arena.lock().unwrap();
        let scale = arena.scale_factor();
        let r = Rect::new(0.0, 0.0, w as f32, h as f32);
        // Overlay popups (the filter popover) clamp against this.
        arena.overlay_mut().set_viewport(r);
        if let Some((hot, cold)) = arena.get_both_mut(root) {
            hot.bounds = r;
            cold.widget.layout(&mut LayoutContext { hot, scale }, r);
        }
    }

    /// One frame: tick springs, rebuild the paint list, feed the dev
    /// session, render. Returns the built list (live-headless path
    /// renders nothing but still feeds the session).
    fn frame(&mut self, dt: Duration) {
        let Some(arena) = self.arena.clone() else {
            return;
        };
        let Some(root) = self.root else { return };
        arena.lock().unwrap().tick(dt);
        let mut list = PaintList::new();
        arena.lock().unwrap().build_paint_list(root, &mut list);
        if let Some(session) = &self.dev_session {
            session.on_frame(&list);
            session.absorb_events(self.router.event_ledger());
        }

        let Self {
            window: Some(window),
            gpu: Some(gpu),
            surface: Some(surface),
            orchestrator: Some(orchestrator),
            recovery,
            ..
        } = self
        else {
            return;
        };
        orchestrator.render(&list, recovery);
        if let Err(err) = orchestrator.render_to_surface(&gpu.device, &gpu.queue, surface) {
            recovery.handle_surface_error(err);
            let size = window.surface_size();
            if let Err(err) = surface.resize(&gpu.device, size.width.max(1), size.height.max(1)) {
                eprintln!("morph_viewer: surface resize failed: {err}");
            }
        }
        window.request_redraw();
    }

    /// winit pointer event → `EventRouter` → arena dispatch.
    fn dispatch_pointer(
        &mut self,
        position: winit::dpi::PhysicalPosition<f64>,
        state: martensite::window::PointerState,
        button: Option<MButton>,
    ) {
        let mut mods = ModifierKeys::empty();
        if self.mods.shift_key() {
            mods |= ModifierKeys::SHIFT;
        }
        if self.mods.control_key() {
            mods |= ModifierKeys::CONTROL;
        }
        let ev = PointerEvent {
            pointer_id: PointerId::PRIMARY,
            kind: PointerKind::Mouse,
            position: Vec2::new(position.x as f32, position.y as f32),
            state,
            button,
            modifiers: mods,
        };
        if let (Some(arena), Some(root), Some(window)) =
            (self.arena.as_ref(), self.root, self.window.as_ref())
        {
            self.router
                .dispatch_pointer_event(&mut arena.lock().unwrap(), root, window.id(), &ev);
        }
    }
}

impl ApplicationHandler for App {
    fn can_create_surfaces(&mut self, event_loop: &dyn ActiveEventLoop) {
        if self.window.is_some() {
            return;
        }
        let window: Arc<dyn Window> = event_loop
            .create_window(
                WindowAttributes::default()
                    .with_title("Martensite — morph viewer")
                    .with_surface_size(LogicalSize::new(1320.0, 860.0)),
            )
            .expect("create window")
            .into();

        // `create_instance` carries the noncompliant-adapter allowance
        // (hasvk/ANV report conformance 0.0.0.0 — hidden otherwise).
        let instance = GpuContext::create_instance();
        let raw_surface = instance
            .create_surface(Arc::clone(&window))
            .expect("create surface");
        let gpu = pollster::block_on(GpuContext::for_surface(&instance, &raw_surface))
            .expect("request GPU context");
        gpu.device.on_uncaptured_error(std::sync::Arc::new(|err| {
            eprintln!("wgpu uncaptured error: {err}");
        }));

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
        let prefer_cpu = std::env::var("MARTENSITE_CPU").is_ok();
        let mut orchestrator = RenderOrchestrator::new(
            size.width.max(1),
            size.height.max(1),
            OrchestratorConfig::new(true, prefer_cpu),
        )
        .expect("orchestrator");
        let notify_window = Arc::clone(&window);
        orchestrator.set_pre_present_notify(Some(Box::new(move || {
            notify_window.pre_present_notify();
        })));

        self.build_arena(window.scale_factor() as f32);
        self.needs_layout = true;

        self.window = Some(window);
        self.gpu = Some(gpu);
        self.surface = Some(surface);
        self.orchestrator = Some(orchestrator);

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
                        if let Err(err) = surface.resize(&gpu.device, size.width, size.height) {
                            eprintln!("morph_viewer: surface resize failed: {err}");
                        }
                    }
                    self.needs_layout = true;
                }
            }
            WindowEvent::ScaleFactorChanged { .. } => {
                if let Some(window) = &self.window {
                    if let Some(arena) = &self.arena {
                        arena
                            .lock()
                            .unwrap()
                            .set_scale_factor(window.scale_factor() as f32);
                    }
                }
                self.needs_layout = true;
            }
            WindowEvent::ModifiersChanged(m) => {
                self.mods = m.state();
            }
            WindowEvent::PointerMoved {
                position, primary, ..
            } => {
                if primary {
                    self.dispatch_pointer(position, martensite::window::PointerState::Moved, None);
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
                let btn = match button {
                    ButtonSource::Mouse(MouseButton::Left) => Some(MButton::Left),
                    ButtonSource::Mouse(MouseButton::Right) => Some(MButton::Right),
                    ButtonSource::Mouse(MouseButton::Middle) => Some(MButton::Middle),
                    _ => None,
                };
                self.dispatch_pointer(
                    position,
                    if state == ElementState::Pressed {
                        martensite::window::PointerState::Pressed
                    } else {
                        martensite::window::PointerState::Released
                    },
                    btn,
                );
            }
            WindowEvent::MouseWheel { delta, .. } => {
                // Match the dashboard convention: lines → 48px each,
                // pixel deltas pass through unchanged.
                let s = self
                    .arena
                    .as_ref()
                    .map(|a| a.lock().unwrap().scale_factor())
                    .unwrap_or(1.0);
                let d = match delta {
                    winit::event::MouseScrollDelta::LineDelta(x, y) => {
                        Vec2::new(x * 48.0 * s, y * 48.0 * s)
                    }
                    winit::event::MouseScrollDelta::PixelDelta(p) => {
                        Vec2::new(p.x as f32, p.y as f32)
                    }
                    _ => return,
                };
                if let (Some(arena), Some(window)) = (self.arena.as_ref(), self.window.as_ref()) {
                    self.router
                        .dispatch_scroll_event(&mut arena.lock().unwrap(), window.id(), d);
                }
            }
            WindowEvent::KeyboardInput { event, .. } => {
                let pressed = event.state == ElementState::Pressed;
                let key_name = match &event.logical_key {
                    winit::keyboard::Key::Named(n) => format!("{n:?}"),
                    winit::keyboard::Key::Character(c) => c.to_string(),
                    _ => String::new(),
                };
                let key_name = match key_name.as_str() {
                    "Space" => " ".to_string(),
                    other => other.to_string(),
                };
                if !key_name.is_empty() {
                    if let (Some(arena), Some(root)) = (&self.arena, self.root) {
                        let mut arena = arena.lock().unwrap();
                        self.router.dispatch_keyboard_event(
                            &mut arena,
                            Some(root),
                            &key_name,
                            pressed,
                            event.repeat,
                        );
                        // Committed text rides its own event (same
                        // split as `WindowEvent::Ime` delivery).
                        if pressed && !event.repeat {
                            if let Some(text) = &event.text {
                                let t = text.to_string();
                                if !t.is_empty() {
                                    arena.dispatch_event(
                                        root,
                                        &martensite::core::WidgetEvent::ImeCommitted { text: t },
                                    );
                                }
                            }
                        }
                    }
                }
            }
            WindowEvent::Ime(ime) => {
                if let Some(ev) = martensite::window::event::ime_event_for_winit(&ime) {
                    if let (Some(arena), Some(root)) = (&self.arena, self.root) {
                        self.router
                            .dispatch_ime_event(&mut arena.lock().unwrap(), Some(root), &ev);
                    }
                }
            }
            WindowEvent::RedrawRequested => {
                let now = Instant::now();
                let dt = now - self.last_frame;
                self.last_frame = now;
                if self.needs_layout {
                    if let Some(window) = &self.window {
                        let size = window.surface_size();
                        self.layout(size.width, size.height);
                    }
                    self.needs_layout = false;
                }
                self.frame(dt);
            }
            _ => {}
        }
    }
}

/// JSON codec adapter exposing one typed `Signal<T>` to the dev
/// session — `signals_list` reports its live value and
/// `signal_trigger` writes land in the handle the widget polls.
fn signal_adapter<T>(name: &str, sig: Signal<T>) -> SignalAdapter
where
    T: serde::Serialize + serde::de::DeserializeOwned + Clone + Send + Sync + 'static,
{
    let read = sig.clone();
    let write = sig;
    let wname = name.to_string();
    SignalAdapter {
        name: wname.clone(),
        signal_id: Some(read.id().raw()),
        read_json: Box::new(move || serde_json::to_value(read.get()).ok()),
        write_json: Box::new(move |v| {
            serde_json::from_value::<T>(v.clone())
                .map(|val| write.set(val))
                .map_err(|e| format!("`{wname}` expects {}: {e}", std::any::type_name::<T>()))
        }),
    }
}

/// Windowed entry — winit + Vello GPU rendering.
pub fn run() -> Result<(), Box<dyn std::error::Error>> {
    let event_loop = EventLoop::new()?;
    // Poll keeps the spring cadence continuous; each RedrawRequested
    // re-arms the next and Fifo present paces the loop.
    event_loop.set_control_flow(ControlFlow::Poll);
    event_loop.run_app(App::new())?;
    Ok(())
}

/// `--live-headless`: the full widget tree + dev channel with no
/// window/GPU — the MCP/dev-tooling path. `capture_node` still
/// rasterizes real PNGs via the devtools TinySkia backend.
pub fn run_live_headless() -> Result<(), Box<dyn std::error::Error>> {
    let mut app = App::new();
    app.build_arena(1.0);
    app.layout(1320, 860);
    let mut last = Instant::now();
    loop {
        let now = Instant::now();
        let dt = now - last;
        last = now;
        app.frame_headless(dt);
        std::thread::sleep(Duration::from_millis(16));
    }
}

impl App {
    /// Tick + paint-list build + dev-session feed — the windowless
    /// half of `frame` (no orchestrator exists in headless mode).
    fn frame_headless(&mut self, dt: Duration) {
        let Some(arena) = self.arena.clone() else {
            return;
        };
        let Some(root) = self.root else { return };
        arena.lock().unwrap().tick(dt);
        let mut list = PaintList::new();
        arena.lock().unwrap().build_paint_list(root, &mut list);
        if let Some(session) = &self.dev_session {
            session.on_frame(&list);
            session.absorb_events(self.router.event_ledger());
        }
    }
}
