//! Canonical windowed application runner.
//!
//! Composes the full native pipeline — winit [`ApplicationHandler`] →
//! [`GpuContext`](martensite_wgpu::GpuContext) +
//! [`SurfaceWrapper`](martensite_wgpu::SurfaceWrapper) +
//! [`RenderOrchestrator`](martensite_wgpu::RenderOrchestrator) →
//! [`WidgetArena`](martensite_core::WidgetArena) +
//! [`EventRouter`](martensite_window::event::EventRouter) — so
//! applications provide a root widget and window configuration instead
//! of re-implementing the event loop. This is the pipeline the
//! `examples/` binaries used to hand-roll (~500 lines each); it is
//! exercised there and by the craft suite.
//!
//! ```no_run
//! use martensite::runner::{self, RunnerConfig};
//! use martensite::core::{HotNode, NodeFlags};
//!
//! runner::run(RunnerConfig::new("My App"), |arena, _scale| {
//!     let mut hot = HotNode::default();
//!     hot.flags |= NodeFlags::VISIBLE | NodeFlags::HIT_TEST_ENABLED;
//!     arena.insert_with_widget(hot, Box::new(martensite::widgets::Text::new("hi")))
//! })
//! .expect("app runs");
//! ```
//!
//! [`ApplicationHandler`]: winit::application::ApplicationHandler

use std::sync::Arc;

use glam::Vec2;
use martensite_core::{LayoutContext, PaintList, Rect, WidgetArena, WidgetId};
use martensite_dnd::DndPlatform;
use martensite_focus::FocusManager;
use martensite_wgpu::wgpu;
use martensite_wgpu::{
    BackdropMode, GpuContext, OrchestratorConfig, PresentModePreference, RecoveryMachine,
    RenderOrchestrator, SurfaceWrapper,
};
use martensite_window::event::{
    convert_drop_event, convert_modifiers_state, convert_window_event, ime_event_for_winit,
    EventRouter,
};
use martensite_window::DpiScale;
use web_time::{Duration, Instant};
use winit::application::ApplicationHandler;
use winit::data_transfer::TypeHint;
use winit::dpi::LogicalSize;
use winit::event::{ElementState, MouseScrollDelta, WindowEvent};
use winit::event_loop::{ActiveEventLoop, AsyncRequestSerial, ControlFlow, EventLoop};
use winit::icon::Icon;
use winit::window::{Window, WindowAttributes, WindowId};

/// Per-frame drain hook — runs after `WidgetArena::tick`, before the
/// paint list is built. Apps use this to pump control channels,
/// background-task completions, and engine notifications into widgets.
pub type AppDrain = Box<dyn FnMut(&mut WidgetArena, WidgetId) + Send>;

/// Boxed future a [`GpuFactory`] returns.
pub type GpuFactoryFuture = std::pin::Pin<
    Box<dyn std::future::Future<Output = Result<GpuContext, GpuFactoryError>> + Send>,
>;

/// Error a [`GpuFactory`] may return.
pub type GpuFactoryError = Box<dyn std::error::Error + Send + Sync>;

/// Root-widget factory passed to [`run`]. Called once with the prepared
/// [`WidgetArena`] and the window's scale factor; returns the [`WidgetId`]
/// of the mounted root.
pub type RootFactory = Box<dyn FnOnce(&mut WidgetArena, f32) -> WidgetId + Send>;

/// Factory for the GPU context, given the already-created instance and
/// surface. Applications that need crash-safe backend selection (e.g. a
/// crash sentinel / software fallback policy) install their own;
/// `None` uses [`GpuContext::for_surface`].
pub type GpuFactory =
    Box<dyn FnOnce(&wgpu::Instance, &wgpu::Surface<'static>) -> GpuFactoryFuture + Send>;

/// Errors the runner can produce before or during [`run`].
///
/// [`LaunchError::GpuInit`] is distinct from windowing failures so
/// callers implementing GPU crash-sentinels can tell "the renderer
/// could not start" (preserve crash evidence, try software fallback)
/// from "the window/event loop failed" (restore prior evidence).
#[derive(Debug)]
#[non_exhaustive]
pub enum LaunchError {
    /// The winit event loop could not be created or run.
    EventLoop(winit::error::EventLoopError),
    /// The platform window could not be created.
    Window(winit::error::RequestError),
    /// GPU initialization failed inside the factory or the default
    /// `GpuContext::for_surface` path.
    GpuInit(String),
    /// Surface configuration failed after the device was acquired.
    SurfaceConfig(String),
}

impl std::fmt::Display for LaunchError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::EventLoop(e) => write!(f, "event loop: {e}"),
            Self::Window(e) => write!(f, "window: {e}"),
            Self::GpuInit(m) => write!(f, "gpu initialization: {m}"),
            Self::SurfaceConfig(m) => write!(f, "surface configuration: {m}"),
        }
    }
}

impl std::error::Error for LaunchError {}

/// Window and runtime configuration for [`run`].
///
/// # Examples
///
/// ```
/// use martensite::runner::RunnerConfig;
///
/// let cfg = RunnerConfig::new("Editor")
///     .with_size(1280.0, 800.0)
///     .with_min_size(640.0, 480.0)
///     .with_decorations(true);
/// assert_eq!(cfg.title, "Editor");
/// ```
pub struct RunnerConfig {
    /// Window title.
    pub title: String,
    /// Optional window icon (build with `winit::icon::RgbaIcon`).
    pub icon: Option<Icon>,
    /// Initial logical surface size.
    pub size: LogicalSize<f32>,
    /// Minimum logical size, if constrained.
    pub min_size: Option<LogicalSize<f32>>,
    /// Whether the window is user-resizable.
    pub resizable: bool,
    /// Whether the platform draws title-bar decorations.
    pub decorations: bool,
    /// Whether the window framebuffer is transparent.
    pub transparent: bool,
    /// Whether to start maximized.
    pub maximized: bool,
    /// Backdrop compositing mode (opaque vs. transparent/vibrancy).
    pub backdrop: BackdropMode,
    /// Presentation pacing preference for the surface.
    pub present_mode: PresentModePreference,
    /// Event loop control flow — `Poll` keeps continuous redraws
    /// (animation cadence); `Wait` sleeps between events.
    pub control_flow: ControlFlow,
    /// Orchestrator policy: software fallback, CPU preference.
    pub orchestrator: OrchestratorConfig,
    /// Custom GPU context factory (adapter ranking, crash-safe backend
    /// selection). `None` uses [`GpuContext::for_surface`].
    pub gpu_factory: Option<GpuFactory>,
    /// Extra instance descriptor overrides, used when the default
    /// [`GpuContext::instance_descriptor`] is not suitable (e.g.
    /// restricting `wgpu::Backends` after a driver crash).
    pub instance_descriptor: Option<wgpu::InstanceDescriptor>,
    /// Per-frame drain callbacks, run in order after `tick`.
    pub drains: Vec<AppDrain>,
    /// Called once after the first successful frame is presented —
    /// the point where "the window really came up" is known.
    pub on_started: Option<Box<dyn FnOnce() + Send>>,
    /// Whether OS file/URI drops are fetched and delivered to widgets
    /// as [`martensite_core::WidgetEvent::Dropped`]. Default `true`.
    pub accept_drops: bool,
    /// When `MARTENSITE_CPU` is set, prefer the CPU renderer even when
    /// a GPU adapter was acquired. Default `true`.
    pub honor_cpu_env: bool,
}

impl RunnerConfig {
    /// Creates a configuration with the given window title.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite::runner::RunnerConfig;
    ///
    /// let cfg = RunnerConfig::new("Tool");
    /// assert_eq!(cfg.title, "Tool");
    /// ```
    #[must_use]
    pub fn new(title: impl Into<String>) -> Self {
        Self {
            title: title.into(),
            icon: None,
            size: LogicalSize::new(1024.0, 768.0),
            min_size: None,
            resizable: true,
            decorations: true,
            transparent: false,
            maximized: false,
            backdrop: BackdropMode::Opaque,
            present_mode: PresentModePreference::LowLatency,
            control_flow: ControlFlow::Poll,
            orchestrator: OrchestratorConfig::new(true, false),
            gpu_factory: None,
            instance_descriptor: None,
            drains: Vec::new(),
            on_started: None,
            accept_drops: true,
            honor_cpu_env: true,
        }
    }

    /// Sets the initial window size in logical pixels.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite::runner::RunnerConfig;
    ///
    /// let cfg = RunnerConfig::new("x").with_size(800.0, 600.0);
    /// assert_eq!(cfg.size.width, 800.0);
    /// ```
    #[must_use]
    pub fn with_size(mut self, width: f32, height: f32) -> Self {
        self.size = LogicalSize::new(width, height);
        self
    }

    /// Sets the minimum window size in logical pixels.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite::runner::RunnerConfig;
    ///
    /// let cfg = RunnerConfig::new("x").with_min_size(320.0, 240.0);
    /// assert!(cfg.min_size.is_some());
    /// ```
    #[must_use]
    pub fn with_min_size(mut self, width: f32, height: f32) -> Self {
        self.min_size = Some(LogicalSize::new(width, height));
        self
    }

    /// Sets whether the window is resizable.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite::runner::RunnerConfig;
    ///
    /// assert!(!RunnerConfig::new("x").with_resizable(false).resizable);
    /// ```
    #[must_use]
    pub fn with_resizable(mut self, resizable: bool) -> Self {
        self.resizable = resizable;
        self
    }

    /// Sets whether the platform draws window decorations.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite::runner::RunnerConfig;
    ///
    /// assert!(!RunnerConfig::new("x").with_decorations(false).decorations);
    /// ```
    #[must_use]
    pub fn with_decorations(mut self, decorations: bool) -> Self {
        self.decorations = decorations;
        self
    }

    /// Sets the window icon.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite::runner::RunnerConfig;
    ///
    /// let icon = winit::icon::RgbaIcon::new(vec![255; 4 * 4 * 4], 4, 4).unwrap();
    /// assert!(RunnerConfig::new("x").with_icon(Some(icon.into())).icon.is_some());
    /// ```
    #[must_use]
    pub fn with_icon(mut self, icon: Option<Icon>) -> Self {
        self.icon = icon;
        self
    }

    /// Adds a per-frame drain callback.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite::runner::RunnerConfig;
    ///
    /// let cfg = RunnerConfig::new("x").with_drain(Box::new(|_arena, _root| {}));
    /// assert_eq!(cfg.drains.len(), 1);
    /// ```
    #[must_use]
    pub fn with_drain(mut self, drain: AppDrain) -> Self {
        self.drains.push(drain);
        self
    }

    /// Builds the [`WindowAttributes`] for this configuration.
    fn window_attributes(&self) -> WindowAttributes {
        let mut attrs = WindowAttributes::default()
            .with_title(self.title.clone())
            .with_surface_size(self.size)
            .with_resizable(self.resizable)
            .with_decorations(self.decorations)
            .with_transparent(self.transparent)
            .with_maximized(self.maximized);
        if let Some(min) = self.min_size {
            attrs = attrs.with_min_surface_size(min);
        }
        if let Some(icon) = self.icon.clone() {
            attrs = attrs.with_window_icon(Some(icon));
        }
        attrs
    }
}

impl std::fmt::Debug for RunnerConfig {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("RunnerConfig")
            .field("title", &self.title)
            .field("size", &self.size)
            .field("min_size", &self.min_size)
            .field("resizable", &self.resizable)
            .field("decorations", &self.decorations)
            .field("transparent", &self.transparent)
            .field("maximized", &self.maximized)
            .field("accept_drops", &self.accept_drops)
            .finish_non_exhaustive()
    }
}

/// The running application state. GPU and window members are created
/// lazily in `can_create_surfaces` per the winit 0.31 lifecycle.
struct Runner {
    config: RunnerConfig,
    make_root: Option<RootFactory>,
    arena: Option<WidgetArena>,
    root: Option<WidgetId>,
    router: EventRouter,
    focus: FocusManager,
    mods: winit::keyboard::ModifiersState,
    window: Option<Arc<dyn Window>>,
    gpu: Option<GpuContext>,
    surface: Option<SurfaceWrapper<'static>>,
    orchestrator: Option<RenderOrchestrator>,
    recovery: RecoveryMachine,
    /// Serials of in-flight `fetch_data_transfer` requests so a later
    /// `DataTransferReceived` can be correlated to this app.
    pending_drops: Vec<AsyncRequestSerial>,
    /// Shared cell [`run`] reads after `run_app` consumes the handler,
    /// so a GPU bring-up failure reaches the caller classified.
    gpu_init_error: Arc<std::sync::Mutex<Option<String>>>,
    needs_layout: bool,
    last_frame: Instant,
}

impl Runner {
    fn scale(&self) -> f32 {
        self.window
            .as_ref()
            .map(|w| w.scale_factor() as f32)
            .unwrap_or(1.0)
    }

    fn layout(&mut self, w: u32, h: u32) {
        let (Some(arena), Some(root)) = (self.arena.as_mut(), self.root) else {
            return;
        };
        let scale = arena.scale_factor();
        let r = Rect::new(0.0, 0.0, w as f32, h as f32);
        arena.overlay_mut().set_viewport(r);
        if let Some((hot, cold)) = arena.get_both_mut(root) {
            hot.bounds = r;
            cold.widget.layout(&mut LayoutContext { hot, scale }, r);
        }
    }

    fn frame(&mut self, dt: Duration) {
        let (Some(arena), Some(root)) = (self.arena.as_mut(), self.root) else {
            return;
        };
        arena.tick(dt);
        for drain in &mut self.config.drains {
            drain(arena, root);
        }
        if let Some(id) = self.router.take_focus_request() {
            self.focus.apply_focus_request(arena, id);
        }
        let mut list = PaintList::new();
        arena.build_paint_list(root, &mut list);

        let (Some(window), Some(gpu), Some(surface), Some(orchestrator)) = (
            self.window.as_ref(),
            self.gpu.as_ref(),
            self.surface.as_mut(),
            self.orchestrator.as_mut(),
        ) else {
            return;
        };
        orchestrator.render(&list, &self.recovery);
        if let Err(err) = orchestrator.render_to_surface(&gpu.device, &gpu.queue, surface) {
            self.recovery.handle_surface_error(err);
            let size = window.surface_size();
            let _ = surface.resize(&gpu.device, size.width.max(1), size.height.max(1));
        }
        window.request_redraw();
        if let Some(started) = self.config.on_started.take() {
            started();
        }
    }

    fn create_surfaces(&mut self, event_loop: &dyn ActiveEventLoop) -> Result<(), LaunchError> {
        if self.window.is_some() {
            return Ok(());
        }
        let window: Arc<dyn Window> = event_loop
            .create_window(self.config.window_attributes())
            .map_err(LaunchError::Window)?
            .into();

        let descriptor = self
            .config
            .instance_descriptor
            .take()
            .unwrap_or_else(GpuContext::instance_descriptor);
        let instance = wgpu::Instance::new(descriptor);
        let raw_surface = instance
            .create_surface(Arc::clone(&window))
            .map_err(|e| LaunchError::SurfaceConfig(e.to_string()))?;
        let gpu = match self.config.gpu_factory.take() {
            Some(factory) => pollster::block_on(factory(&instance, &raw_surface))
                .map_err(|e| LaunchError::GpuInit(e.to_string()))?,
            None => pollster::block_on(GpuContext::for_surface(&instance, &raw_surface))
                .map_err(|e| LaunchError::GpuInit(e.to_string()))?,
        };
        gpu.device.on_uncaptured_error(Arc::new(|err| {
            tracing::error!("wgpu uncaptured error: {err}");
        }));

        let size = window.surface_size();
        let mut surface = SurfaceWrapper::new(raw_surface);
        surface.set_pacing(self.config.present_mode);
        surface
            .configure(
                &gpu.device,
                &gpu.adapter,
                size.width.max(1),
                size.height.max(1),
                self.config.backdrop,
            )
            .map_err(|e| LaunchError::SurfaceConfig(e.to_string()))?;
        if self.config.honor_cpu_env && std::env::var("MARTENSITE_CPU").is_ok() {
            self.config.orchestrator.prefer_cpu = true;
        }
        let mut orchestrator = RenderOrchestrator::new(
            size.width.max(1),
            size.height.max(1),
            self.config.orchestrator.clone(),
        )
        .map_err(|e| LaunchError::GpuInit(e.to_string()))?;
        let notify_window = Arc::clone(&window);
        orchestrator.set_pre_present_notify(Some(Box::new(move || {
            notify_window.pre_present_notify();
        })));

        // Arena: platform preferences + caller's root widget.
        let scale = window.scale_factor() as f32;
        let mut arena = WidgetArena::new();
        arena.set_theme(
            martensite_theme::ThemeDictionary::new()
                .dark_theme()
                .clone(),
        );
        arena.set_scale_factor(scale);
        arena.set_text_painter(crate::text_paint::shared_painter());
        martensite_window::prefs::apply_platform_preferences(&mut arena);
        let make_root = self.make_root.take().ok_or_else(|| {
            LaunchError::GpuInit("runner already consumed its root factory".into())
        })?;
        let root = make_root(&mut arena, scale);
        self.focus.set_root(root);
        self.focus.set_focus_unchecked(root);

        self.window = Some(window);
        self.gpu = Some(gpu);
        self.surface = Some(surface);
        self.orchestrator = Some(orchestrator);
        self.arena = Some(arena);
        self.root = Some(root);
        self.needs_layout = true;
        if let Some(window) = &self.window {
            window.request_redraw();
        }
        Ok(())
    }
}

impl ApplicationHandler for Runner {
    fn can_create_surfaces(&mut self, event_loop: &dyn ActiveEventLoop) {
        if let Err(err) = self.create_surfaces(event_loop) {
            if let LaunchError::GpuInit(m) = &err {
                if let Ok(mut cell) = self.gpu_init_error.lock() {
                    *cell = Some(m.clone());
                }
            }
            tracing::error!("martensite runner: {err}");
            event_loop.exit();
        }
    }

    fn destroy_surfaces(&mut self, _event_loop: &dyn ActiveEventLoop) {
        // Android invalidates the native surface here; dropping the
        // wrapper keeps the winit window alive for recreation.
        self.surface = None;
        self.gpu = None;
        self.orchestrator = None;
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
                            tracing::warn!("martensite runner: surface resize failed: {err}");
                        }
                    }
                    if let Some(o) = &mut self.orchestrator {
                        o.set_frame_size(size.width, size.height);
                    }
                    self.needs_layout = true;
                }
            }
            WindowEvent::ScaleFactorChanged { .. } => {
                if let (Some(window), Some(arena)) = (&self.window, &mut self.arena) {
                    arena.set_scale_factor(window.scale_factor() as f32);
                }
                self.needs_layout = true;
            }
            WindowEvent::ModifiersChanged(m) => {
                self.mods = m.state();
            }
            WindowEvent::PointerMoved { .. } | WindowEvent::PointerButton { .. } => {
                let scale = DpiScale::new(self.scale() as f64);
                if let Some(mut pe) = convert_window_event(&event, &scale) {
                    pe.modifiers = convert_modifiers_state(&self.mods);
                    if let (Some(arena), Some(root), Some(window)) =
                        (self.arena.as_mut(), self.root, self.window.as_ref())
                    {
                        self.router
                            .dispatch_pointer_event(arena, root, window.id(), &pe);
                    }
                }
            }
            WindowEvent::MouseWheel { delta, .. } => {
                // Lines → 48 px each (matching the widget-catalog
                // convention); pixel deltas pass through unchanged.
                let s = self.scale();
                let d = match delta {
                    MouseScrollDelta::LineDelta(x, y) => Vec2::new(x * 48.0 * s, y * 48.0 * s),
                    MouseScrollDelta::PixelDelta(p) => Vec2::new(p.x as f32, p.y as f32),
                    _ => return,
                };
                if let (Some(arena), Some(window)) = (self.arena.as_mut(), self.window.as_ref()) {
                    self.router.dispatch_scroll_event(arena, window.id(), d);
                }
            }
            WindowEvent::KeyboardInput { event, .. } => {
                let pressed = event.state == ElementState::Pressed;
                let Some(key_name) = martensite_window::event::key_name(&event.logical_key) else {
                    return;
                };
                let (Some(arena), Some(root)) = (&mut self.arena, self.root) else {
                    return;
                };
                let focused = self.focus.current_focus().or(Some(root));
                self.router.dispatch_keyboard_event(
                    arena,
                    focused,
                    &key_name,
                    pressed,
                    event.repeat,
                );
                // Committed text rides its own event (same split as
                // `WindowEvent::Ime` delivery).
                if pressed && !event.repeat {
                    if let Some(text) = &event.text {
                        let t = text.to_string();
                        if !t.is_empty() {
                            arena.dispatch_event(
                                focused.unwrap_or(root),
                                &martensite_core::WidgetEvent::ImeCommitted { text: t },
                            );
                        }
                    }
                }
            }
            WindowEvent::Ime(ime) => {
                if let Some(ev) = ime_event_for_winit(&ime) {
                    if let (Some(arena), Some(root)) = (&mut self.arena, self.root) {
                        let focused = self.focus.current_focus().or(Some(root));
                        self.router.dispatch_ime_event(arena, focused, &ev);
                    }
                }
            }
            WindowEvent::DragEntered { .. }
            | WindowEvent::DragPosition { .. }
            | WindowEvent::DragDropped { .. }
            | WindowEvent::DragLeft { .. } => {
                if !self.config.accept_drops {
                    return;
                }
                // Accept copy+move so the OS shows a valid drop cursor;
                // on `DragDropped` kick off the async payload fetch
                // whose `DataTransferReceived` completes the delivery.
                if let WindowEvent::DragEntered { id, .. }
                | WindowEvent::DragPosition { id, .. }
                | WindowEvent::DragDropped { id, .. } = &event
                {
                    let platform = martensite_dnd::platform::WinitDndPlatform::new(event_loop);
                    let transfer_id = martensite_dnd::PlatformTransferId::new(id.into_raw());
                    let _ = platform.set_accepted_actions(
                        transfer_id,
                        &[
                            martensite_dnd::DropEffect::Copy,
                            martensite_dnd::DropEffect::Move,
                        ],
                    );
                    if matches!(event, WindowEvent::DragDropped { .. }) {
                        if let Ok(serial) = event_loop.fetch_data_transfer(*id, &TypeHint::UriList)
                        {
                            self.pending_drops.push(serial);
                        }
                    }
                }
                let scale = DpiScale::new(self.scale() as f64);
                if let (Some(drop), Some(arena), Some(root), Some(window)) = (
                    convert_drop_event(&event, &scale),
                    self.arena.as_mut(),
                    self.root,
                    self.window.as_ref(),
                ) {
                    self.router
                        .dispatch_drop_event(arena, root, window.id(), &drop);
                }
            }
            WindowEvent::DataTransferReceived { value, serial, .. } => {
                if !self.pending_drops.contains(&serial) {
                    return;
                }
                self.pending_drops.retain(|s| *s != serial);
                let payload = if let Ok(paths) = value.try_as_file_paths() {
                    martensite_core::DropPayload::Files(paths)
                } else if let Ok(uris) = value.try_as_uris() {
                    martensite_core::DropPayload::Uris(uris)
                } else if let Ok(text) = value.try_as_string() {
                    martensite_core::DropPayload::Text(text)
                } else {
                    martensite_core::DropPayload::Bytes(value.try_as_bytes().unwrap_or_default())
                };
                if let (Some(arena), Some(root), Some(window)) =
                    (self.arena.as_mut(), self.root, self.window.as_ref())
                {
                    self.router
                        .dispatch_drop_payload(arena, root, window.id(), payload);
                }
            }
            WindowEvent::RedrawRequested => {
                let now = Instant::now();
                let dt = now.saturating_duration_since(self.last_frame);
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

/// Runs the application until the window closes.
///
/// `make_root` is called with the prepared [`WidgetArena`] and the
/// window's scale factor; return the [`WidgetId`] of the mounted root
/// widget. Typical implementations insert their shell widget with
/// `NodeFlags::VISIBLE | NodeFlags::HIT_TEST_ENABLED`.
///
/// Returns [`LaunchError::GpuInit`] when the GPU/surface bring-up
/// failed — callers with a crash sentinel should treat this as GPU
/// evidence. All other variants are ordinary launch failures.
///
/// # Examples
///
/// ```no_run
/// use martensite::runner::{self, RunnerConfig};
/// use martensite::core::{HotNode, NodeFlags};
///
/// let config = RunnerConfig::new("My App").with_size(1280.0, 800.0);
/// let _ = runner::run(config, |arena, _scale| {
///     let mut hot = HotNode::default();
///     hot.flags |= NodeFlags::VISIBLE | NodeFlags::HIT_TEST_ENABLED;
///     arena.insert_with_widget(hot, Box::new(martensite::widgets::Text::new("hi")))
/// });
/// ```
pub fn run(
    config: RunnerConfig,
    make_root: impl FnOnce(&mut WidgetArena, f32) -> WidgetId + Send + 'static,
) -> Result<(), LaunchError> {
    let event_loop = EventLoop::new().map_err(LaunchError::EventLoop)?;
    event_loop.set_control_flow(config.control_flow);
    let gpu_init_error = Arc::new(std::sync::Mutex::new(None::<String>));
    let runner = Runner {
        make_root: Some(Box::new(make_root)),
        config,
        arena: None,
        root: None,
        router: EventRouter::new(),
        focus: FocusManager::new(),
        mods: winit::keyboard::ModifiersState::empty(),
        window: None,
        gpu: None,
        surface: None,
        orchestrator: None,
        recovery: RecoveryMachine::new(),
        pending_drops: Vec::new(),
        gpu_init_error: Arc::clone(&gpu_init_error),
        needs_layout: true,
        last_frame: Instant::now(),
    };
    event_loop.run_app(runner).map_err(LaunchError::EventLoop)?;
    // `run_app` consumes the handler; read the classified GPU-init
    // failure back out of the shared cell.
    if let Some(m) = gpu_init_error.lock().ok().and_then(|g| g.clone()) {
        return Err(LaunchError::GpuInit(m));
    }
    Ok(())
}
