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
//! `examples/` binaries used to hand-roll (~500 lines each);
//! `examples/runner_smoke` is the in-tree reference consumer.
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
//! # Limitations
//!
//! - No accessibility adapter is wired into the runner yet; the
//!   widget-catalog path has one, so runner-hosted apps are not exposed
//!   to assistive technology.
//! - OS drag-and-drop is delivered through the arena's hit-test
//!   ([`martensite_window::event::EventRouter::dispatch_drop_event`]) to
//!   the widget under the pointer, not through the `martensite-dnd`
//!   `DropBridge` registry.
//!
//! [`ApplicationHandler`]: winit::application::ApplicationHandler

use std::io::{self, Read};
use std::sync::Arc;

use glam::Vec2;
use martensite_core::{
    DropPayload, LayoutConstraints, LayoutContext, PaintList, Rect, WidgetArena, WidgetId,
};
use martensite_dnd::DndPlatform;
use martensite_focus::FocusManager;
use martensite_wgpu::wgpu;
use martensite_wgpu::{
    BackdropMode, GpuContext, OrchestratorConfig, PresentModePreference, RecoveryMachine,
    RenderOrchestrator, SurfaceError, SurfaceWrapper,
};
use martensite_window::event::{
    convert_drop_event, convert_modifiers_state, convert_window_event, ime_event_for_winit,
    DropEvent, EventRouter, PointerEvent,
};
use martensite_window::DpiScale;
use web_time::{Duration, Instant};
use winit::application::ApplicationHandler;
use winit::data_transfer::{DataTransferId, TypeHint, TypedData};
use winit::dpi::LogicalSize;
use winit::event::{ElementState, MouseScrollDelta, WindowEvent};
use winit::event_loop::{ActiveEventLoop, AsyncRequestSerial, ControlFlow, EventLoop};
use winit::icon::Icon;
use winit::window::{Window, WindowAttributes, WindowId};

/// Largest drop payload, in bytes, the runner reads into memory. Larger
/// transfers are rejected and dispatched as a leave.
const MAX_DROP_PAYLOAD_BYTES: usize = 64 * 1024 * 1024;

/// Maximum in-flight `fetch_data_transfer` requests; the oldest is
/// evicted beyond this so abandoned drags cannot grow the list.
const MAX_PENDING_DROPS: usize = 8;

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
///
/// # Examples
///
/// ```
/// use martensite::runner::LaunchError;
///
/// let err = LaunchError::GpuInit("no adapter".into());
/// assert_eq!(err.to_string(), "gpu initialization: no adapter");
/// let is_gpu = match err {
///     LaunchError::GpuInit(_) => true,
///     _ => false,
/// };
/// assert!(is_gpu);
/// ```
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
#[non_exhaustive]
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
    /// Event loop control flow. `Poll` (default) redraws continuously.
    /// `Wait` repaints only in response to window events, so animations
    /// and `drains` advance only on events; use `Poll` for animated UIs
    /// or async drains.
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

/// Outcome of presenting one frame, decided without touching the GPU.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum PresentAction {
    /// The frame reached the surface.
    Presented,
    /// The surface must be reconfigured at its current size.
    Reconfigure,
    /// Nothing to do this frame; device-loss recovery owns the next step.
    Skip,
}

/// Classifies a present result and advances the recovery state machine.
fn after_present(
    recovery: &mut RecoveryMachine,
    result: Result<(), SurfaceError>,
) -> PresentAction {
    match result {
        Ok(()) => {
            // Marks `Restored`/`FallbackCpu` recoveries complete; a no-op while `Active`.
            recovery.repaint_completed();
            PresentAction::Presented
        }
        Err(err) => {
            if recovery.handle_surface_error(err) {
                PresentAction::Reconfigure
            } else {
                PresentAction::Skip
            }
        }
    }
}

/// Whether the frame loop requests redraws itself. `Wait` does not: it
/// repaints only in response to window events.
fn wants_continuous_redraw(control_flow: ControlFlow) -> bool {
    control_flow == ControlFlow::Poll
}

/// What the shell must do after a window event has been handled.
#[derive(Debug, Default, PartialEq, Eq)]
struct Effects {
    /// The window should repaint.
    redraw: bool,
    /// The event loop should exit.
    exit: bool,
    /// Raw physical size the surface and orchestrator adopt; the shell clamps it.
    resize: Option<(u32, u32)>,
}

/// A drag-and-drop offer, as far as the runner distinguishes types.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Offer {
    /// A type with a cross-platform hint.
    Hint(TypeHint),
    /// A platform-specific type, addressed by its position in the offer list.
    Opaque(usize),
}

/// The drag-and-drop platform surface the runner drives. [`WinitDrop`] is
/// the production implementation; tests substitute a recorder.
trait DropPlatform {
    /// Advertised types for `id`, or `None` when they cannot be enumerated.
    fn offered(&self, id: DataTransferId) -> Option<Vec<Offer>>;
    /// Requests the payload for `id` as `ty`.
    fn fetch(&self, id: DataTransferId, ty: Offer) -> Result<AsyncRequestSerial, String>;
    /// Registers copy and move as the accepted drop actions for `id`.
    fn accept(&self, id: DataTransferId) -> Result<(), String>;
}

/// [`DropPlatform`] backed by the live winit event loop.
struct WinitDrop<'a>(&'a dyn ActiveEventLoop);

impl DropPlatform for WinitDrop<'_> {
    fn offered(&self, id: DataTransferId) -> Option<Vec<Offer>> {
        let transfer = self.0.data_transfer(id).ok()?;
        Some(
            transfer
                .available_types()
                .iter()
                .enumerate()
                .map(|(index, ty)| match ty.hint() {
                    Some(hint) => Offer::Hint(hint),
                    None => Offer::Opaque(index),
                })
                .collect(),
        )
    }

    fn fetch(&self, id: DataTransferId, ty: Offer) -> Result<AsyncRequestSerial, String> {
        let result = match ty {
            Offer::Hint(hint) => self.0.fetch_data_transfer(id, &hint),
            Offer::Opaque(index) => {
                let transfer = self.0.data_transfer(id).map_err(|e| e.to_string())?;
                let types = transfer.available_types();
                let ty = types.get(index).ok_or("offer index out of range")?;
                self.0.fetch_data_transfer(id, *ty)
            }
        };
        result.map_err(|e| e.to_string())
    }

    fn accept(&self, id: DataTransferId) -> Result<(), String> {
        let platform = martensite_dnd::platform::WinitDndPlatform::new(self.0);
        platform
            .set_accepted_actions(
                martensite_dnd::PlatformTransferId::new(id.into_raw()),
                &[
                    martensite_dnd::DropEffect::Copy,
                    martensite_dnd::DropEffect::Move,
                ],
            )
            .map_err(|e| e.to_string())
    }
}

/// Window- and GPU-free runner state: the widget arena, event routing,
/// and drop bookkeeping. [`Runner`] owns the window and GPU stack around it.
struct RunnerCore {
    make_root: Option<RootFactory>,
    drains: Vec<AppDrain>,
    accept_drops: bool,
    arena: Option<WidgetArena>,
    root: Option<WidgetId>,
    router: EventRouter,
    focus: FocusManager,
    mods: winit::keyboard::ModifiersState,
    /// Serials of in-flight `fetch_data_transfer` requests so a later
    /// `DataTransferReceived` can be correlated to this app.
    pending_drops: Vec<AsyncRequestSerial>,
    /// Transfer whose accepted drop actions were last set, so
    /// `DragPosition` does not re-register them on every move.
    accepted_transfer: Option<DataTransferId>,
    /// Rebuilt each frame — retained so `build_paint_list` reuses its
    /// command-buffer capacity instead of reallocating.
    paint_list: PaintList,
    needs_layout: bool,
}

impl RunnerCore {
    fn new(make_root: Option<RootFactory>, drains: Vec<AppDrain>, accept_drops: bool) -> Self {
        Self {
            make_root,
            drains,
            accept_drops,
            arena: None,
            root: None,
            router: EventRouter::new(),
            focus: FocusManager::new(),
            mods: winit::keyboard::ModifiersState::empty(),
            pending_drops: Vec::new(),
            accepted_transfer: None,
            paint_list: PaintList::new(),
            needs_layout: true,
        }
    }

    /// Builds the arena with platform preferences and the caller's root
    /// widget. Runs once: the arena survives suspend/resume.
    fn init_arena(&mut self, scale: f32) -> Result<(), LaunchError> {
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
        self.arena = Some(arena);
        self.root = Some(root);
        self.needs_layout = true;
        Ok(())
    }

    fn set_scale(&mut self, scale: f32) {
        if let Some(arena) = self.arena.as_mut() {
            arena.set_scale_factor(scale);
        }
        self.needs_layout = true;
    }

    fn layout(&mut self, w: u32, h: u32) {
        self.needs_layout = false;
        let (Some(arena), Some(root)) = (self.arena.as_mut(), self.root) else {
            return;
        };
        // Manual layout bypasses `LayoutEngine` — install the ambient
        // intl and measurer guards so widget `measure` calls see the
        // same direction/locale and glyph metrics the paint pass will
        // use (otherwise text lays out zero-height).
        let _intl = arena.install_ambient_intl();
        let _measurer = arena
            .text_painter_shared()
            .map(martensite_core::paint::install_ambient_measurer);
        let scale = arena.scale_factor();
        let r = Rect::new(0.0, 0.0, w as f32, h as f32);
        arena.overlay_mut().set_viewport(r);
        if let Some((hot, cold)) = arena.get_both_mut(root) {
            // `Flex` sizes its children in `measure`; `layout` alone
            // leaves every row at zero height.
            cold.widget.measure(
                &mut LayoutContext { hot, scale },
                LayoutConstraints {
                    min_size: Vec2::ZERO,
                    max_size: Vec2::new(r.width(), r.height()),
                },
            );
            hot.bounds = r;
            cold.widget.layout(&mut LayoutContext { hot, scale }, r);
        }
    }

    /// Ticks the arena, runs drains, applies focus requests, and rebuilds
    /// the paint list. Returns `None` before the arena exists.
    fn prepare_frame(&mut self, dt: Duration) -> Option<&PaintList> {
        let (Some(arena), Some(root)) = (self.arena.as_mut(), self.root) else {
            return None;
        };
        arena.tick(dt);
        for drain in &mut self.drains {
            drain(arena, root);
        }
        if let Some(id) = self.router.take_focus_request() {
            self.focus.apply_focus_request(arena, id);
        }
        self.paint_list.clear();
        arena.build_paint_list(root, &mut self.paint_list);
        Some(&self.paint_list)
    }

    /// Routes one window event and reports what the shell must do next.
    /// `RedrawRequested` is handled by the shell and is a no-op here.
    fn handle_event(
        &mut self,
        event: &WindowEvent,
        window_id: WindowId,
        scale: f32,
        platform: &dyn DropPlatform,
    ) -> Effects {
        let mut fx = Effects::default();
        match event {
            WindowEvent::CloseRequested => fx.exit = true,
            WindowEvent::SurfaceResized(size) => {
                if size.width > 0 && size.height > 0 {
                    fx.resize = Some((size.width, size.height));
                    self.needs_layout = true;
                    fx.redraw = true;
                }
            }
            WindowEvent::ScaleFactorChanged { scale_factor, .. } => {
                self.set_scale(*scale_factor as f32);
                fx.redraw = true;
            }
            WindowEvent::ModifiersChanged(m) => {
                self.mods = m.state();
            }
            WindowEvent::PointerMoved { .. } | WindowEvent::PointerButton { .. } => {
                if let Some(mut pe) = pointer_event(event) {
                    pe.modifiers = convert_modifiers_state(&self.mods);
                    if let (Some(arena), Some(root)) = (self.arena.as_mut(), self.root) {
                        self.router
                            .dispatch_pointer_event(arena, root, window_id, &pe);
                    }
                    fx.redraw = true;
                }
            }
            WindowEvent::MouseWheel { delta, .. } => {
                // Lines → 48 px each (matching the widget-catalog
                // convention); pixel deltas pass through unchanged.
                let d = match delta {
                    MouseScrollDelta::LineDelta(x, y) => {
                        Vec2::new(x * 48.0 * scale, y * 48.0 * scale)
                    }
                    MouseScrollDelta::PixelDelta(p) => Vec2::new(p.x as f32, p.y as f32),
                    _ => return fx,
                };
                if let Some(arena) = self.arena.as_mut() {
                    self.router.dispatch_scroll_event(arena, window_id, d);
                }
                fx.redraw = true;
            }
            WindowEvent::KeyboardInput { event: key, .. } => {
                fx.redraw = true;
                let pressed = key.state == ElementState::Pressed;
                let Some(key_name) = martensite_window::event::key_name(&key.logical_key) else {
                    return fx;
                };
                let (Some(arena), Some(root)) = (&mut self.arena, self.root) else {
                    return fx;
                };
                let focused = self.focus.current_focus().or(Some(root));
                self.router
                    .dispatch_keyboard_event(arena, focused, &key_name, pressed, key.repeat);
                // Committed text rides its own event (same split as
                // `WindowEvent::Ime` delivery).
                if pressed && !key.repeat {
                    if let Some(text) = &key.text {
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
                fx.redraw = true;
                if let Some(ev) = ime_event_for_winit(ime) {
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
                if !self.accept_drops {
                    return fx;
                }
                fx.redraw = true;
                // Accept copy+move so the OS shows a valid drop cursor.
                if let WindowEvent::DragEntered { id, .. }
                | WindowEvent::DragPosition { id, .. }
                | WindowEvent::DragDropped { id, .. } = event
                {
                    self.accept_transfer(
                        platform,
                        *id,
                        matches!(event, WindowEvent::DragEntered { .. }),
                    );
                }
                let mut dropped = drop_event(event);
                // On `DragDropped` the payload fetch completes the
                // delivery via `DataTransferReceived`; without an
                // accepted fetch the drop can never complete, so it is
                // reported as a leave to clear hover state.
                if let WindowEvent::DragDropped { id, .. } = event {
                    if !self.fetch_drop_payload(platform, *id) {
                        dropped = Some(DropEvent::Left);
                    }
                }
                if matches!(
                    event,
                    WindowEvent::DragLeft { .. } | WindowEvent::DragDropped { .. }
                ) {
                    self.accepted_transfer = None;
                }
                if let (Some(dropped), Some(arena), Some(root)) =
                    (dropped, self.arena.as_mut(), self.root)
                {
                    self.router
                        .dispatch_drop_event(arena, root, window_id, &dropped);
                }
            }
            WindowEvent::DataTransferReceived { value, serial, .. } => {
                if !self.pending_drops.contains(serial) {
                    return fx;
                }
                let payload = match drop_payload(value.as_ref()) {
                    Ok(payload) => Some(payload),
                    // Winit asks for a retry on the next
                    // `DataTransferReceived` carrying this serial.
                    Err(err) if is_pending(&err) => return fx,
                    Err(err) => {
                        tracing::warn!("martensite runner: drop payload unreadable: {err}");
                        None
                    }
                };
                self.pending_drops.retain(|s| s != serial);
                fx.redraw = true;
                if let (Some(arena), Some(root)) = (self.arena.as_mut(), self.root) {
                    match payload {
                        Some(payload) => {
                            self.router
                                .dispatch_drop_payload(arena, root, window_id, payload);
                        }
                        None => {
                            self.router.dispatch_drop_event(
                                arena,
                                root,
                                window_id,
                                &DropEvent::Left,
                            );
                        }
                    }
                }
            }
            WindowEvent::Focused(_)
            | WindowEvent::PointerEntered { .. }
            | WindowEvent::PointerLeft { .. } => fx.redraw = true,
            _ => {}
        }
        fx
    }

    /// Registers copy+move as accepted for `id` on `DragEntered` and
    /// whenever the drag moves to a different transfer.
    fn accept_transfer(&mut self, platform: &dyn DropPlatform, id: DataTransferId, entered: bool) {
        if !entered && self.accepted_transfer == Some(id) {
            return;
        }
        self.accepted_transfer = Some(id);
        if let Err(err) = platform.accept(id) {
            tracing::debug!("martensite runner: set_accepted_actions failed: {err}");
        }
    }

    /// Starts the payload fetch for a dropped transfer, trying URIs,
    /// then plain text, then the first remaining advertised type.
    /// Returns `false` when the platform accepted no fetch.
    fn fetch_drop_payload(&mut self, platform: &dyn DropPlatform, id: DataTransferId) -> bool {
        let offered = platform.offered(id);
        let is_offered = |hint: TypeHint| {
            offered
                .as_ref()
                .is_none_or(|types| types.contains(&Offer::Hint(hint)))
        };
        let mut candidates = Vec::new();
        if is_offered(TypeHint::UriList) {
            candidates.push(Offer::Hint(TypeHint::UriList));
        }
        if is_offered(TypeHint::Plaintext) {
            candidates.push(Offer::Hint(TypeHint::Plaintext));
        }
        if let Some(types) = &offered {
            candidates.extend(
                types
                    .iter()
                    .copied()
                    .find(|ty| !matches!(ty, Offer::Hint(TypeHint::UriList | TypeHint::Plaintext))),
            );
        }
        for ty in candidates {
            match platform.fetch(id, ty) {
                Ok(serial) => {
                    push_bounded(&mut self.pending_drops, serial);
                    return true;
                }
                Err(err) => tracing::debug!("martensite runner: fetch {ty:?} rejected: {err}"),
            }
        }
        false
    }
}

/// The running application state: the window-free [`RunnerCore`] plus
/// the window and GPU stack, which are created lazily in
/// `can_create_surfaces` per the winit 0.31 lifecycle.
struct Runner {
    config: RunnerConfig,
    core: RunnerCore,
    window: Option<Arc<dyn Window>>,
    gpu: Option<GpuContext>,
    surface: Option<SurfaceWrapper<'static>>,
    orchestrator: Option<RenderOrchestrator>,
    recovery: RecoveryMachine,
    /// Shared cell [`run`] reads after `run_app` consumes the handler,
    /// so a classified launch failure (window, GPU, or surface
    /// configuration) reaches the caller instead of looking like a
    /// clean exit.
    launch_error: Arc<std::sync::Mutex<Option<LaunchError>>>,
    last_frame: Instant,
}

impl Runner {
    fn scale(&self) -> f32 {
        self.window
            .as_ref()
            .map(|w| w.scale_factor() as f32)
            .unwrap_or(1.0)
    }

    /// Clamps a physical extent to the GPU's `max_texture_dimension_2d`
    /// (proportionally, preserving aspect). Downlevel native adapters
    /// can cap the surface extent below a HiDPI window's device-pixel
    /// size — the same oversize class the web clamp covers.
    fn clamped_extent(&self, w: u32, h: u32) -> (u32, u32) {
        match &self.gpu {
            Some(gpu) => martensite_wgpu::surface::clamp_extent(
                w,
                h,
                gpu.device.limits().max_texture_dimension_2d,
            ),
            None => (w, h),
        }
    }

    /// Requests a repaint. Under `Wait` this is what produces a frame after
    /// an event; under `Poll` the frame loop already requests one.
    fn request_redraw(&self) {
        if let Some(window) = &self.window {
            window.request_redraw();
        }
    }

    /// Carries out the shell-side part of an [`Effects`] value.
    fn apply(&mut self, event_loop: &dyn ActiveEventLoop, fx: Effects) {
        if fx.exit {
            event_loop.exit();
        }
        if let Some((w, h)) = fx.resize {
            let (w, h) = self.clamped_extent(w, h);
            if let (Some(surface), Some(gpu)) = (&mut self.surface, &self.gpu) {
                if let Err(err) = surface.resize(&gpu.device, w, h) {
                    tracing::warn!("martensite runner: surface resize failed: {err}");
                }
            }
            if let Some(o) = &mut self.orchestrator {
                o.set_frame_size(w, h);
            }
        }
        if fx.redraw {
            self.request_redraw();
        }
    }

    fn frame(&mut self, dt: Duration) {
        let Some(paint) = self.core.prepare_frame(dt) else {
            return;
        };
        // `create_surfaces` requests a redraw when the GPU stack returns on resume.
        let (Some(window), Some(gpu), Some(surface), Some(orchestrator)) = (
            self.window.as_ref(),
            self.gpu.as_ref(),
            self.surface.as_mut(),
            self.orchestrator.as_mut(),
        ) else {
            return;
        };
        orchestrator.render(paint, &self.recovery);
        let result = orchestrator.render_to_surface(&gpu.device, &gpu.queue, surface);
        match after_present(&mut self.recovery, result) {
            PresentAction::Presented => {
                // "Started" means a frame actually presented — not
                // merely attempted.
                if let Some(started) = self.config.on_started.take() {
                    started();
                }
            }
            PresentAction::Reconfigure => {
                let size = window.surface_size();
                let (w, h) = martensite_wgpu::surface::clamp_extent(
                    size.width.max(1),
                    size.height.max(1),
                    gpu.device.limits().max_texture_dimension_2d,
                );
                if let Err(err) = surface.resize(&gpu.device, w, h) {
                    tracing::warn!(
                        "martensite runner: surface resize after acquire failure: {err}"
                    );
                }
            }
            PresentAction::Skip => {}
        }
        if wants_continuous_redraw(self.config.control_flow) {
            window.request_redraw();
        }
    }

    fn create_surfaces(&mut self, event_loop: &dyn ActiveEventLoop) -> Result<(), LaunchError> {
        // The window and arena are one-shot: they survive a suspend/resume
        // cycle, so `make_root` (an `FnOnce`) runs exactly once. Only the
        // GPU stack is rebuilt on resume.
        if self.window.is_none() {
            let window: Arc<dyn Window> = event_loop
                .create_window(self.config.window_attributes())
                .map_err(LaunchError::Window)?
                .into();
            self.core.init_arena(window.scale_factor() as f32)?;
            self.window = Some(window);
        }

        // GPU/surface/orchestrator come up whenever any is missing —
        // `destroy_surfaces` (Android suspend) drops them under a live
        // window and the first `can_create_surfaces` after resume
        // rebuilds them here.
        if self.gpu.is_some() && self.surface.is_some() && self.orchestrator.is_some() {
            return Ok(());
        }
        let window = Arc::clone(self.window.as_ref().expect("window created above"));
        // A custom `gpu_factory`/`instance_descriptor` applies to the
        // first bring-up only; a resume rebuild uses the default
        // `GpuContext::for_surface` path.
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
        // Clamp to the device's texture extent — downlevel adapters can
        // cap `max_texture_dimension_2d` below the window's device-pixel
        // size; `configure`/`get_current_texture` would fail otherwise.
        let (w, h) = martensite_wgpu::surface::clamp_extent(
            size.width.max(1),
            size.height.max(1),
            gpu.device.limits().max_texture_dimension_2d,
        );
        let mut surface = SurfaceWrapper::new(raw_surface);
        surface.set_pacing(self.config.present_mode);
        surface
            .configure(&gpu.device, &gpu.adapter, w, h, self.config.backdrop)
            .map_err(|e| LaunchError::SurfaceConfig(e.to_string()))?;
        if self.config.honor_cpu_env && std::env::var("MARTENSITE_CPU").is_ok() {
            self.config.orchestrator.prefer_cpu = true;
        }
        let mut orchestrator = RenderOrchestrator::new(w, h, self.config.orchestrator.clone())
            .map_err(|e| LaunchError::GpuInit(e.to_string()))?;
        let notify_window = Arc::clone(&window);
        orchestrator.set_pre_present_notify(Some(Box::new(move || {
            notify_window.pre_present_notify();
        })));

        self.gpu = Some(gpu);
        self.surface = Some(surface);
        self.orchestrator = Some(orchestrator);
        self.core.needs_layout = true;
        window.request_redraw();
        Ok(())
    }
}

impl ApplicationHandler for Runner {
    fn can_create_surfaces(&mut self, event_loop: &dyn ActiveEventLoop) {
        if let Err(err) = self.create_surfaces(event_loop) {
            tracing::error!("martensite runner: {err}");
            if let Ok(mut cell) = self.launch_error.lock() {
                *cell = Some(err);
            }
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

    fn window_event(&mut self, event_loop: &dyn ActiveEventLoop, id: WindowId, event: WindowEvent) {
        if let WindowEvent::RedrawRequested = event {
            let now = Instant::now();
            let dt = now.saturating_duration_since(self.last_frame);
            self.last_frame = now;
            if self.core.needs_layout {
                if let Some(window) = &self.window {
                    let size = window.surface_size();
                    let (w, h) = self.clamped_extent(size.width.max(1), size.height.max(1));
                    self.core.layout(w, h);
                }
            }
            self.frame(dt);
            return;
        }
        let scale = self.scale();
        let fx = self
            .core
            .handle_event(&event, id, scale, &WinitDrop(event_loop));
        self.apply(event_loop, fx);
    }
}

/// Converts a pointer event at unit scale: arena bounds are laid out in
/// physical pixels, so winit's physical positions are used unchanged.
fn pointer_event(event: &WindowEvent) -> Option<PointerEvent> {
    convert_window_event(event, &DpiScale::new(1.0))
}

/// Converts a drag event at unit scale for the same reason as
/// [`pointer_event`].
fn drop_event(event: &WindowEvent) -> Option<DropEvent> {
    convert_drop_event(event, &DpiScale::new(1.0))
}

/// Whether a payload read failure is transient: winit reports
/// `WouldBlock`/`Deadlock` and expects the read to be retried.
fn is_pending(err: &io::Error) -> bool {
    matches!(
        err.kind(),
        io::ErrorKind::WouldBlock | io::ErrorKind::Deadlock
    )
}

/// Rejects a drop payload whose total size exceeds `limit`.
fn check_drop_len(len: usize, limit: usize) -> io::Result<()> {
    if len > limit {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "drop payload exceeds limit",
        ));
    }
    Ok(())
}

/// Reads at most `limit` + 1 bytes from `reader`, so an oversize stream
/// is detected without buffering all of it.
fn read_capped(reader: Box<dyn io::BufRead>, limit: usize) -> io::Result<Vec<u8>> {
    let mut bytes = Vec::new();
    reader.take(limit as u64 + 1).read_to_end(&mut bytes)?;
    check_drop_len(bytes.len(), limit)?;
    Ok(bytes)
}

/// Reads a fetched transfer as the payload its type describes.
fn drop_payload(value: &dyn TypedData) -> io::Result<DropPayload> {
    match value.type_().hint() {
        Some(TypeHint::UriList) => match value.try_as_file_paths() {
            Ok(paths) => {
                let total: usize = paths.iter().map(|p| p.as_os_str().len()).sum();
                check_drop_len(total, MAX_DROP_PAYLOAD_BYTES)?;
                Ok(DropPayload::Files(paths))
            }
            Err(err) if is_pending(&err) => Err(err),
            Err(_) => {
                let uris = value.try_as_uris()?;
                check_drop_len(uris.iter().map(String::len).sum(), MAX_DROP_PAYLOAD_BYTES)?;
                Ok(DropPayload::Uris(uris))
            }
        },
        Some(TypeHint::Plaintext | TypeHint::Html) => match value.try_read() {
            Some(reader) => {
                let bytes = read_capped(reader, MAX_DROP_PAYLOAD_BYTES)?;
                String::from_utf8(bytes)
                    .map(DropPayload::Text)
                    .map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e))
            }
            None => {
                let text = value.try_as_string()?;
                check_drop_len(text.len(), MAX_DROP_PAYLOAD_BYTES)?;
                Ok(DropPayload::Text(text))
            }
        },
        _ => match value.try_read() {
            Some(reader) => read_capped(reader, MAX_DROP_PAYLOAD_BYTES).map(DropPayload::Bytes),
            None => {
                let bytes = value.try_as_bytes()?;
                check_drop_len(bytes.len(), MAX_DROP_PAYLOAD_BYTES)?;
                Ok(DropPayload::Bytes(bytes))
            }
        },
    }
}

/// Pushes `serial`, evicting the oldest entry once `MAX_PENDING_DROPS`
/// fetches are outstanding.
fn push_bounded<T>(pending: &mut Vec<T>, serial: T) {
    while pending.len() >= MAX_PENDING_DROPS {
        pending.remove(0);
        tracing::debug!("martensite runner: evicting oldest pending drop fetch");
    }
    pending.push(serial);
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
    mut config: RunnerConfig,
    make_root: impl FnOnce(&mut WidgetArena, f32) -> WidgetId + Send + 'static,
) -> Result<(), LaunchError> {
    let event_loop = EventLoop::new().map_err(LaunchError::EventLoop)?;
    event_loop.set_control_flow(config.control_flow);
    let launch_error = Arc::new(std::sync::Mutex::new(None::<LaunchError>));
    let make_root: RootFactory = Box::new(make_root);
    let drains = std::mem::take(&mut config.drains);
    let core = RunnerCore::new(Some(make_root), drains, config.accept_drops);
    let runner = Runner {
        config,
        core,
        window: None,
        gpu: None,
        surface: None,
        orchestrator: None,
        recovery: RecoveryMachine::new(),
        launch_error: Arc::clone(&launch_error),
        last_frame: Instant::now(),
    };
    event_loop.run_app(runner).map_err(LaunchError::EventLoop)?;
    // `run_app` consumes the handler; read the classified launch
    // failure back out of the shared cell.
    if let Some(err) = launch_error.lock().ok().and_then(|mut g| g.take()) {
        return Err(err);
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use martensite_core::{DummyWidget, EventResponse, HotNode, NodeFlags, Rect};
    use martensite_core::{EventContext, Widget, WidgetEvent};
    use std::cell::RefCell;
    use std::sync::Weak;
    use winit::data_transfer::TransferType;
    use winit::dpi::PhysicalSize;
    use winit::event::{
        ElementState, KeyEvent, Modifiers, MouseScrollDelta, PointerSource, SurfaceSizeWriter,
        TouchPhase,
    };
    use winit::keyboard::{
        Key, KeyCode, KeyLocation, ModifiersState, NativeKey, PhysicalKey, SmolStr,
    };

    use martensite_window::WindowId;
    use winit::dpi::PhysicalPosition;
    use winit::event::{ButtonSource, MouseButton as WinitMouseButton};

    fn pressed_at(x: f64, y: f64) -> WindowEvent {
        WindowEvent::PointerButton {
            device_id: None,
            state: ElementState::Pressed,
            position: PhysicalPosition::new(x, y),
            primary: true,
            button: ButtonSource::Mouse(WinitMouseButton::Left),
            is_macos_activation_click: false,
        }
    }

    fn drag_at(x: f64, y: f64) -> WindowEvent {
        WindowEvent::DragPosition {
            id: DataTransferId::from_raw(1),
            position: PhysicalPosition::new(x, y),
            proposed_action: None,
        }
    }

    /// Root widget covering physical pixels 300..400 on both axes.
    fn physical_root(arena: &mut WidgetArena) -> WidgetId {
        let hot = HotNode {
            bounds: Rect::new(300.0, 300.0, 100.0, 100.0),
            flags: NodeFlags::VISIBLE | NodeFlags::HIT_TEST_ENABLED,
            ..HotNode::default()
        };
        arena.insert_with_widget(hot, Box::new(DummyWidget))
    }

    #[test]
    fn pointer_event_keeps_physical_position_at_scale_two() {
        let pe = pointer_event(&pressed_at(350.0, 350.0)).expect("pointer button converts");
        assert_eq!(pe.position, Vec2::new(350.0, 350.0));
        // The scaled conversion would halve the position at 2.0 — the
        // mismatch the unit-scale helper avoids.
        let scaled = convert_window_event(&pressed_at(350.0, 350.0), &DpiScale::new(2.0))
            .expect("pointer button converts");
        assert_eq!(scaled.position, Vec2::new(175.0, 175.0));
    }

    #[test]
    fn drop_event_keeps_physical_position_at_scale_two() {
        let drop = drop_event(&drag_at(350.0, 350.0)).expect("drag position converts");
        assert_eq!(drop.position(), Some(Vec2::new(350.0, 350.0)));
    }

    #[test]
    fn pointer_event_hits_physically_laid_out_widget() {
        let mut arena = WidgetArena::new();
        let root = physical_root(&mut arena);
        let mut router = EventRouter::new();
        let win = WindowId::from_raw(1);

        let pe = pointer_event(&pressed_at(350.0, 350.0)).expect("pointer button converts");
        assert_eq!(
            router.dispatch_pointer_event(&mut arena, root, win, &pe),
            Some(EventResponse::Ignored)
        );

        let halved = convert_window_event(&pressed_at(350.0, 350.0), &DpiScale::new(2.0))
            .expect("pointer button converts");
        assert_eq!(
            router.dispatch_pointer_event(&mut arena, root, win, &halved),
            None
        );
    }

    #[test]
    fn drop_event_hovers_physically_laid_out_widget() {
        let mut arena = WidgetArena::new();
        let root = physical_root(&mut arena);
        let mut router = EventRouter::new();
        let win = WindowId::from_raw(1);

        let moved = drop_event(&drag_at(350.0, 350.0)).expect("drag position converts");
        assert_eq!(
            router.dispatch_drop_event(&mut arena, root, win, &moved),
            Some(EventResponse::Ignored)
        );

        let halved = convert_drop_event(&drag_at(350.0, 350.0), &DpiScale::new(2.0))
            .expect("drag position converts");
        assert_eq!(
            router.dispatch_drop_event(&mut arena, root, win, &halved),
            None
        );
    }

    /// Fake transfer whose conversions are scripted per test.
    #[derive(Debug)]
    struct FakeData {
        hint: TypeHint,
        pending: bool,
        uris: Vec<String>,
        text: String,
        stream: Option<Vec<u8>>,
        bytes: Option<Vec<u8>>,
    }

    impl TypedData for FakeData {
        fn type_(&self) -> &dyn TransferType {
            &self.hint
        }

        fn try_read(&self) -> Option<Box<dyn io::BufRead>> {
            let bytes = self.stream.clone()?;
            Some(Box::new(io::Cursor::new(bytes)))
        }

        fn try_as_bytes(&self) -> io::Result<Vec<u8>> {
            self.bytes
                .clone()
                .ok_or_else(|| io::Error::other("not readable as bytes"))
        }

        fn try_as_uris(&self) -> io::Result<Vec<String>> {
            if self.pending {
                return Err(io::ErrorKind::WouldBlock.into());
            }
            Ok(self.uris.clone())
        }

        fn try_as_string(&self) -> io::Result<String> {
            if self.pending {
                return Err(io::ErrorKind::WouldBlock.into());
            }
            Ok(self.text.clone())
        }
    }

    fn fake(hint: TypeHint) -> FakeData {
        FakeData {
            hint,
            pending: false,
            uris: Vec::new(),
            text: String::new(),
            stream: None,
            bytes: None,
        }
    }

    #[test]
    fn drop_payload_uri_list_prefers_file_paths() {
        let data = FakeData {
            uris: vec!["file:///tmp/a.txt".into()],
            ..fake(TypeHint::UriList)
        };
        assert!(matches!(
            drop_payload(&data),
            Ok(DropPayload::Files(paths)) if paths.len() == 1
        ));
    }

    #[test]
    fn drop_payload_non_file_uri_falls_back_to_uris() {
        let data = FakeData {
            uris: vec!["https://example.com/a".into()],
            ..fake(TypeHint::UriList)
        };
        assert!(matches!(
            drop_payload(&data),
            Ok(DropPayload::Uris(uris)) if uris == ["https://example.com/a"]
        ));
    }

    #[test]
    fn drop_payload_plaintext_becomes_text() {
        let data = FakeData {
            text: "hello".into(),
            ..fake(TypeHint::Plaintext)
        };
        assert!(matches!(
            drop_payload(&data),
            Ok(DropPayload::Text(t)) if t == "hello"
        ));
    }

    #[test]
    fn drop_payload_would_block_is_pending() {
        let data = FakeData {
            pending: true,
            ..fake(TypeHint::Plaintext)
        };
        let err = drop_payload(&data).expect_err("pending read");
        assert!(is_pending(&err));
    }

    #[test]
    fn drop_payload_unreadable_bytes_is_final_error() {
        let err = drop_payload(&fake(TypeHint::Image {
            extension_hint: None,
        }))
        .expect_err("no byte source");
        assert!(!is_pending(&err));
    }

    #[test]
    fn read_capped_rejects_stream_over_limit() {
        let over: Box<dyn io::BufRead> = Box::new(io::Cursor::new(vec![0u8; 5]));
        let err = read_capped(over, 4).expect_err("oversize payload");
        assert_eq!(err.kind(), io::ErrorKind::InvalidData);
        assert!(!is_pending(&err));
        let at: Box<dyn io::BufRead> = Box::new(io::Cursor::new(vec![0u8; 4]));
        assert_eq!(read_capped(at, 4).expect("at limit").len(), 4);
    }

    #[test]
    fn drop_payload_stream_under_limit_is_read() {
        let data = FakeData {
            stream: Some(b"hi".to_vec()),
            ..fake(TypeHint::Plaintext)
        };
        assert!(matches!(
            drop_payload(&data),
            Ok(DropPayload::Text(t)) if t == "hi"
        ));
    }

    #[test]
    fn drop_payload_invalid_utf8_text_is_rejected() {
        let data = FakeData {
            stream: Some(vec![0xff, 0xfe]),
            ..fake(TypeHint::Plaintext)
        };
        let err = drop_payload(&data).expect_err("invalid utf-8");
        assert_eq!(err.kind(), io::ErrorKind::InvalidData);
    }

    #[test]
    fn push_bounded_evicts_oldest_beyond_limit() {
        let mut pending = Vec::new();
        for serial in 0..(MAX_PENDING_DROPS as u32 + 2) {
            push_bounded(&mut pending, serial);
        }
        assert_eq!(pending.len(), MAX_PENDING_DROPS);
        assert_eq!(pending.first(), Some(&2));
        assert_eq!(pending.last(), Some(&(MAX_PENDING_DROPS as u32 + 1)));
    }

    #[derive(Clone, Default)]
    struct Log(Arc<std::sync::Mutex<Vec<String>>>);

    impl Log {
        fn push(&self, entry: impl Into<String>) {
            self.0.lock().expect("log lock").push(entry.into());
        }
        fn entries(&self) -> Vec<String> {
            self.0.lock().expect("log lock").clone()
        }
        fn has(&self, needle: &str) -> bool {
            self.entries().iter().any(|e| e.contains(needle))
        }
    }

    /// Records measure, layout, and every delivered event.
    struct Probe {
        log: Log,
    }

    impl Widget for Probe {
        fn measure(&mut self, _cx: &mut LayoutContext, _constraints: LayoutConstraints) -> Vec2 {
            self.log.push("measure");
            Vec2::new(10.0, 20.0)
        }
        fn layout(&mut self, _cx: &mut LayoutContext, bounds: Rect) {
            self.log
                .push(format!("layout {}x{}", bounds.width(), bounds.height()));
        }
        fn event(&mut self, cx: &mut EventContext) -> EventResponse {
            match cx.event {
                WidgetEvent::Scroll { delta, .. } => {
                    self.log.push(format!("scroll {} {}", delta.x, delta.y));
                }
                other => self.log.push(format!("{other:?}")),
            }
            EventResponse::Handled
        }
    }

    fn probe_root(log: &Log) -> RootFactory {
        let log = log.clone();
        Box::new(move |arena: &mut WidgetArena, _scale: f32| {
            let hot = HotNode {
                bounds: Rect::new(300.0, 300.0, 100.0, 100.0),
                flags: NodeFlags::VISIBLE | NodeFlags::HIT_TEST_ENABLED,
                ..HotNode::default()
            };
            arena.insert_with_widget(hot, Box::new(Probe { log }))
        })
    }

    fn core_with(log: &Log, accept_drops: bool) -> RunnerCore {
        let mut core = RunnerCore::new(Some(probe_root(log)), Vec::new(), accept_drops);
        core.init_arena(1.0).expect("arena initializes");
        core
    }

    fn ready_core(log: &Log) -> RunnerCore {
        core_with(log, true)
    }

    fn win() -> WindowId {
        WindowId::from_raw(1)
    }

    fn pointer_moved_at(x: f64, y: f64) -> WindowEvent {
        WindowEvent::PointerMoved {
            device_id: None,
            position: PhysicalPosition::new(x, y),
            primary: true,
            source: PointerSource::Mouse,
        }
    }

    fn wheel(delta: MouseScrollDelta) -> WindowEvent {
        WindowEvent::MouseWheel {
            device_id: None,
            delta,
            phase: TouchPhase::Started,
        }
    }

    fn key(text: Option<&str>, state: ElementState, repeat: bool) -> WindowEvent {
        key_with(Key::Character(SmolStr::new("a")), text, state, repeat)
    }

    fn key_with(
        logical: Key,
        text: Option<&str>,
        state: ElementState,
        repeat: bool,
    ) -> WindowEvent {
        WindowEvent::KeyboardInput {
            device_id: None,
            event: KeyEvent {
                physical_key: PhysicalKey::Code(KeyCode::KeyA),
                logical_key: logical.clone(),
                text: text.map(SmolStr::new),
                text_with_all_modifiers: text.map(SmolStr::new),
                location: KeyLocation::Standard,
                state,
                repeat,
                key_without_modifiers: logical,
            },
            is_synthetic: false,
        }
    }

    fn drag_entered(raw: i64) -> WindowEvent {
        WindowEvent::DragEntered {
            id: DataTransferId::from_raw(raw),
            position: None,
        }
    }

    fn drag_position(raw: i64, x: f64, y: f64) -> WindowEvent {
        WindowEvent::DragPosition {
            id: DataTransferId::from_raw(raw),
            position: PhysicalPosition::new(x, y),
            proposed_action: None,
        }
    }

    fn drag_dropped(raw: i64) -> WindowEvent {
        WindowEvent::DragDropped {
            id: DataTransferId::from_raw(raw),
            proposed_action: None,
        }
    }

    fn drag_left(raw: i64) -> WindowEvent {
        WindowEvent::DragLeft {
            id: DataTransferId::from_raw(raw),
        }
    }

    fn data_received(serial: AsyncRequestSerial, value: Arc<dyn TypedData>) -> WindowEvent {
        WindowEvent::DataTransferReceived {
            id: DataTransferId::from_raw(1),
            serial,
            value,
        }
    }

    /// Scripted drop platform: records every call and fails the fetches
    /// and accepts it is told to.
    #[derive(Default)]
    struct FakeDrop {
        offered: Option<Vec<Offer>>,
        rejected: Vec<Offer>,
        accept_fails: bool,
        fetches: RefCell<Vec<Offer>>,
        accepts: RefCell<Vec<i64>>,
    }

    impl DropPlatform for FakeDrop {
        fn offered(&self, _id: DataTransferId) -> Option<Vec<Offer>> {
            self.offered.clone()
        }
        fn fetch(&self, _id: DataTransferId, ty: Offer) -> Result<AsyncRequestSerial, String> {
            self.fetches.borrow_mut().push(ty);
            if self.rejected.contains(&ty) {
                Err("rejected".into())
            } else {
                Ok(AsyncRequestSerial::get())
            }
        }
        fn accept(&self, id: DataTransferId) -> Result<(), String> {
            self.accepts.borrow_mut().push(id.into_raw());
            if self.accept_fails {
                Err("accept refused".into())
            } else {
                Ok(())
            }
        }
    }

    #[test]
    fn pointer_press_reaches_widget_under_physical_position() {
        let log = Log::default();
        let mut core = ready_core(&log);
        let fx = core.handle_event(&pressed_at(350.0, 350.0), win(), 2.0, &FakeDrop::default());
        assert!(fx.redraw);
        assert!(log.has("PointerPressed"));
    }

    #[test]
    fn line_wheel_scales_by_48_per_line_and_pixel_wheel_passes_through() {
        let log = Log::default();
        let mut core = ready_core(&log);
        let platform = FakeDrop::default();
        core.handle_event(&pointer_moved_at(350.0, 350.0), win(), 2.0, &platform);
        let fx = core.handle_event(
            &wheel(MouseScrollDelta::LineDelta(0.0, 1.0)),
            win(),
            2.0,
            &platform,
        );
        assert!(fx.redraw);
        assert!(log.has("scroll 0 96"));
        core.handle_event(
            &wheel(MouseScrollDelta::PixelDelta(PhysicalPosition::new(
                3.0, -4.0,
            ))),
            win(),
            2.0,
            &platform,
        );
        assert!(log.has("scroll 3 -4"));
    }

    #[test]
    fn key_press_delivers_key_and_committed_text_and_repeat_does_not_commit() {
        let log = Log::default();
        let mut core = ready_core(&log);
        let platform = FakeDrop::default();
        let fx = core.handle_event(
            &key(Some("a"), ElementState::Pressed, false),
            win(),
            1.0,
            &platform,
        );
        assert!(fx.redraw);
        assert!(log.has("KeyPressed"));
        assert!(log.has("ImeCommitted"));
        let before = log.entries().len();
        core.handle_event(
            &key(Some("a"), ElementState::Pressed, true),
            win(),
            1.0,
            &platform,
        );
        assert!(!log.entries()[before..]
            .iter()
            .any(|e| e.contains("ImeCommitted")));
    }

    #[test]
    fn ime_commit_reaches_focused_widget() {
        let log = Log::default();
        let mut core = ready_core(&log);
        let ime = WindowEvent::Ime(winit::event::Ime::Commit("é".into()));
        let fx = core.handle_event(&ime, win(), 1.0, &FakeDrop::default());
        assert!(fx.redraw);
        assert!(log.has("ImeCommitted"));
    }

    #[test]
    fn modifiers_changed_updates_tracked_state_without_redraw() {
        let log = Log::default();
        let mut core = ready_core(&log);
        let event = WindowEvent::ModifiersChanged(Modifiers::from(ModifiersState::SHIFT));
        let fx = core.handle_event(&event, win(), 1.0, &FakeDrop::default());
        assert_eq!(fx, Effects::default());
        assert_eq!(core.mods, ModifiersState::SHIFT);
    }

    #[test]
    fn close_requested_exits() {
        let log = Log::default();
        let mut core = ready_core(&log);
        let fx = core.handle_event(
            &WindowEvent::CloseRequested,
            win(),
            1.0,
            &FakeDrop::default(),
        );
        assert_eq!(
            fx,
            Effects {
                exit: true,
                ..Effects::default()
            }
        );
    }

    #[test]
    fn surface_resize_requests_resize_layout_and_redraw_but_ignores_zero_size() {
        let log = Log::default();
        let mut core = ready_core(&log);
        let platform = FakeDrop::default();
        core.layout(1, 1);
        let fx = core.handle_event(
            &WindowEvent::SurfaceResized(PhysicalSize::new(800, 600)),
            win(),
            1.0,
            &platform,
        );
        assert_eq!(
            fx,
            Effects {
                redraw: true,
                exit: false,
                resize: Some((800, 600)),
            }
        );
        assert!(core.needs_layout);
        core.layout(1, 1);
        let zero = core.handle_event(
            &WindowEvent::SurfaceResized(PhysicalSize::new(0, 600)),
            win(),
            1.0,
            &platform,
        );
        assert_eq!(zero, Effects::default());
        assert!(!core.needs_layout);
    }

    #[test]
    fn scale_factor_change_updates_arena_and_requests_layout() {
        let log = Log::default();
        let mut core = ready_core(&log);
        core.layout(1, 1);
        let event = WindowEvent::ScaleFactorChanged {
            scale_factor: 2.0,
            surface_size_writer: SurfaceSizeWriter::new(Weak::new()),
        };
        let fx = core.handle_event(&event, win(), 1.0, &FakeDrop::default());
        assert!(fx.redraw);
        assert!(core.needs_layout);
        assert_eq!(core.arena.as_ref().expect("arena").scale_factor(), 2.0);
    }

    #[test]
    fn focus_and_pointer_enter_leave_only_request_redraw() {
        let log = Log::default();
        let mut core = ready_core(&log);
        let events = [
            WindowEvent::Focused(true),
            WindowEvent::PointerLeft {
                device_id: None,
                primary: true,
                position: None,
                kind: winit::event::PointerKind::Mouse,
            },
        ];
        for event in &events {
            let fx = core.handle_event(event, win(), 1.0, &FakeDrop::default());
            assert_eq!(
                fx,
                Effects {
                    redraw: true,
                    ..Effects::default()
                }
            );
        }
    }

    #[test]
    fn drops_are_ignored_when_not_accepted() {
        let log = Log::default();
        let mut core = core_with(&log, false);
        let platform = FakeDrop::default();
        let fx = core.handle_event(&drag_entered(1), win(), 1.0, &platform);
        assert_eq!(fx, Effects::default());
        core.handle_event(&drag_dropped(1), win(), 1.0, &platform);
        assert!(platform.accepts.borrow().is_empty());
        assert!(platform.fetches.borrow().is_empty());
    }

    #[test]
    fn accept_runs_once_per_transfer_and_resets_on_leave() {
        let log = Log::default();
        let mut core = ready_core(&log);
        let platform = FakeDrop::default();
        core.handle_event(&drag_entered(1), win(), 1.0, &platform);
        core.handle_event(&drag_position(1, 350.0, 350.0), win(), 1.0, &platform);
        core.handle_event(&drag_position(2, 350.0, 350.0), win(), 1.0, &platform);
        core.handle_event(&drag_left(2), win(), 1.0, &platform);
        core.handle_event(&drag_position(2, 350.0, 350.0), win(), 1.0, &platform);
        assert_eq!(*platform.accepts.borrow(), vec![1, 2, 2]);
    }

    #[test]
    fn drag_leave_dispatches_hover_leave_to_hovered_widget() {
        let log = Log::default();
        let mut core = ready_core(&log);
        let platform = FakeDrop::default();
        core.handle_event(&drag_position(1, 350.0, 350.0), win(), 1.0, &platform);
        assert!(log.has("DropHover"));
        core.handle_event(&drag_left(1), win(), 1.0, &platform);
        assert!(log.has("DropHoverLeave"));
    }

    #[test]
    fn accept_failure_is_logged_and_does_not_block_drop() {
        let log = Log::default();
        let mut core = ready_core(&log);
        let platform = FakeDrop {
            accept_fails: true,
            ..FakeDrop::default()
        };
        core.handle_event(&drag_entered(1), win(), 1.0, &platform);
        assert_eq!(*platform.accepts.borrow(), vec![1]);
        assert_eq!(core.accepted_transfer, Some(DataTransferId::from_raw(1)));
    }

    #[test]
    fn drop_fetches_uri_list_then_plaintext_when_offered() {
        let log = Log::default();
        let mut core = ready_core(&log);
        let platform = FakeDrop {
            offered: Some(vec![
                Offer::Hint(TypeHint::Plaintext),
                Offer::Hint(TypeHint::UriList),
                Offer::Opaque(0),
            ]),
            rejected: vec![Offer::Hint(TypeHint::UriList)],
            ..FakeDrop::default()
        };
        core.handle_event(&drag_dropped(1), win(), 1.0, &platform);
        assert_eq!(
            *platform.fetches.borrow(),
            vec![
                Offer::Hint(TypeHint::UriList),
                Offer::Hint(TypeHint::Plaintext)
            ]
        );
        assert_eq!(core.pending_drops.len(), 1);
    }

    #[test]
    fn drop_falls_back_to_first_other_advertised_type() {
        let log = Log::default();
        let mut core = ready_core(&log);
        let platform = FakeDrop {
            offered: Some(vec![Offer::Opaque(0)]),
            ..FakeDrop::default()
        };
        core.handle_event(&drag_dropped(1), win(), 1.0, &platform);
        assert_eq!(*platform.fetches.borrow(), vec![Offer::Opaque(0)]);
    }

    #[test]
    fn drop_skips_unoffered_hints_when_choosing_fallback() {
        let log = Log::default();
        let mut core = ready_core(&log);
        let platform = FakeDrop {
            offered: Some(vec![Offer::Hint(TypeHint::Html), Offer::Opaque(1)]),
            ..FakeDrop::default()
        };
        core.handle_event(&drag_dropped(1), win(), 1.0, &platform);
        assert_eq!(
            *platform.fetches.borrow(),
            vec![Offer::Hint(TypeHint::Html)]
        );
    }

    #[test]
    fn drop_with_every_fetch_rejected_dispatches_leave_and_queues_nothing() {
        let log = Log::default();
        let mut core = ready_core(&log);
        let platform = FakeDrop {
            rejected: vec![
                Offer::Hint(TypeHint::UriList),
                Offer::Hint(TypeHint::Plaintext),
            ],
            ..FakeDrop::default()
        };
        core.handle_event(&drag_position(1, 350.0, 350.0), win(), 1.0, &platform);
        core.handle_event(&drag_dropped(1), win(), 1.0, &platform);
        assert_eq!(
            *platform.fetches.borrow(),
            vec![
                Offer::Hint(TypeHint::UriList),
                Offer::Hint(TypeHint::Plaintext)
            ]
        );
        assert!(core.pending_drops.is_empty());
        assert!(log.has("DropHoverLeave"));
    }

    #[test]
    fn pending_drop_fetches_are_bounded() {
        let log = Log::default();
        let mut core = ready_core(&log);
        let platform = FakeDrop::default();
        for _ in 0..(MAX_PENDING_DROPS + 1) {
            core.handle_event(&drag_dropped(1), win(), 1.0, &platform);
        }
        assert_eq!(core.pending_drops.len(), MAX_PENDING_DROPS);
    }

    #[test]
    fn unknown_serial_is_ignored() {
        let log = Log::default();
        let mut core = ready_core(&log);
        let value: Arc<dyn TypedData> = Arc::new(fake(TypeHint::Plaintext));
        let fx = core.handle_event(
            &data_received(AsyncRequestSerial::get(), value),
            win(),
            1.0,
            &FakeDrop::default(),
        );
        assert_eq!(fx, Effects::default());
        assert!(!log.has("Dropped"));
    }

    #[test]
    fn pending_read_keeps_serial_for_retry() {
        let log = Log::default();
        let mut core = ready_core(&log);
        let platform = FakeDrop::default();
        core.handle_event(&drag_dropped(1), win(), 1.0, &platform);
        let serial = core.pending_drops[0];
        let value: Arc<dyn TypedData> = Arc::new(FakeData {
            pending: true,
            ..fake(TypeHint::Plaintext)
        });
        let fx = core.handle_event(&data_received(serial, value), win(), 1.0, &platform);
        assert_eq!(fx, Effects::default());
        assert_eq!(core.pending_drops, vec![serial]);
        assert!(!log.has("Dropped"));
    }

    #[test]
    fn unreadable_payload_dispatches_leave_and_releases_serial() {
        let log = Log::default();
        let mut core = ready_core(&log);
        let platform = FakeDrop::default();
        core.handle_event(&drag_position(1, 350.0, 350.0), win(), 1.0, &platform);
        core.handle_event(&drag_dropped(1), win(), 1.0, &platform);
        let serial = core.pending_drops[0];
        let value: Arc<dyn TypedData> = Arc::new(fake(TypeHint::Image {
            extension_hint: None,
        }));
        let fx = core.handle_event(&data_received(serial, value), win(), 1.0, &platform);
        assert!(fx.redraw);
        assert!(core.pending_drops.is_empty());
        assert!(log.has("DropHoverLeave"));
    }

    #[test]
    fn readable_payload_is_delivered_to_hovered_widget() {
        let log = Log::default();
        let mut core = ready_core(&log);
        let platform = FakeDrop::default();
        core.handle_event(&drag_position(1, 350.0, 350.0), win(), 1.0, &platform);
        core.handle_event(&drag_dropped(1), win(), 1.0, &platform);
        let serial = core.pending_drops[0];
        let value: Arc<dyn TypedData> = Arc::new(FakeData {
            text: "hi".into(),
            ..fake(TypeHint::Plaintext)
        });
        let fx = core.handle_event(&data_received(serial, value), win(), 1.0, &platform);
        assert!(fx.redraw);
        assert!(core.pending_drops.is_empty());
        assert!(log.has("Dropped"));
        assert!(log.has("Text(\"hi\")"));
    }

    #[test]
    fn layout_measures_before_laying_out_root() {
        let log = Log::default();
        let mut core = ready_core(&log);
        core.layout(800, 600);
        let entries = log.entries();
        let measure = entries.iter().position(|e| e == "measure");
        let layout = entries.iter().position(|e| e == "layout 800x600");
        assert!(measure.is_some() && layout.is_some());
        assert!(measure < layout);
        assert!(!core.needs_layout);
    }

    #[test]
    fn prepare_frame_runs_drains_in_order_and_is_none_without_arena() {
        let log = Log::default();
        let drains: Vec<AppDrain> = (1..=2)
            .map(|n| {
                let log = log.clone();
                Box::new(move |_arena: &mut WidgetArena, _root: WidgetId| {
                    log.push(format!("drain {n}"));
                }) as AppDrain
            })
            .collect();
        let mut core = RunnerCore::new(Some(probe_root(&log)), drains, true);
        assert!(core.prepare_frame(Duration::ZERO).is_none());
        core.init_arena(1.0).expect("arena initializes");
        assert!(core.prepare_frame(Duration::ZERO).is_some());
        let drained: Vec<String> = log
            .entries()
            .into_iter()
            .filter(|e| e.starts_with("drain"))
            .collect();
        assert_eq!(drained, vec!["drain 1", "drain 2"]);
    }

    #[test]
    fn init_arena_passes_scale_and_refuses_a_second_root() {
        let log = Log::default();
        let root_log = log.clone();
        let root: RootFactory = Box::new(move |arena: &mut WidgetArena, scale: f32| {
            root_log.push(format!("scale {scale}"));
            let hot = HotNode {
                flags: NodeFlags::VISIBLE | NodeFlags::HIT_TEST_ENABLED,
                ..HotNode::default()
            };
            arena.insert_with_widget(hot, Box::new(Probe { log: root_log }))
        });
        let mut core = RunnerCore::new(Some(root), Vec::new(), true);
        core.init_arena(2.0).expect("arena initializes");
        assert!(log.has("scale 2"));
        assert_eq!(core.arena.as_ref().expect("arena").scale_factor(), 2.0);
        let err = core.init_arena(2.0).expect_err("root already consumed");
        assert!(matches!(err, LaunchError::GpuInit(_)));
    }

    #[test]
    fn presented_frame_keeps_recovery_active() {
        let mut recovery = RecoveryMachine::new();
        assert_eq!(
            after_present(&mut recovery, Ok(())),
            PresentAction::Presented
        );
        assert!(recovery.is_active());
    }

    #[test]
    fn transient_surface_error_requests_reconfigure() {
        let mut recovery = RecoveryMachine::new();
        let action = after_present(&mut recovery, Err(SurfaceError::Outdated));
        assert_eq!(action, PresentAction::Reconfigure);
        assert!(recovery.is_active());
    }

    #[test]
    fn device_loss_skips_frame_and_enters_recovery() {
        let mut recovery = RecoveryMachine::new();
        let action = after_present(&mut recovery, Err(SurfaceError::Lost));
        assert_eq!(action, PresentAction::Skip);
        assert!(!recovery.is_active());
    }

    #[test]
    fn only_poll_requests_continuous_redraw() {
        assert!(wants_continuous_redraw(ControlFlow::Poll));
        assert!(!wants_continuous_redraw(ControlFlow::Wait));
    }

    #[test]
    fn runner_config_builders_set_fields() {
        let cfg = RunnerConfig::new("t")
            .with_size(800.0, 600.0)
            .with_min_size(320.0, 240.0)
            .with_resizable(false)
            .with_decorations(false)
            .with_icon(None)
            .with_drain(Box::new(|_arena: &mut WidgetArena, _root: WidgetId| {}));
        assert_eq!(cfg.title, "t");
        assert_eq!((cfg.size.width, cfg.size.height), (800.0, 600.0));
        let min = cfg.min_size.expect("min size set");
        assert_eq!((min.width, min.height), (320.0, 240.0));
        assert!(!cfg.resizable);
        assert!(!cfg.decorations);
        assert!(cfg.icon.is_none());
        assert_eq!(cfg.drains.len(), 1);
        let _ = cfg.window_attributes();
        assert!(format!("{cfg:?}").contains("accept_drops"));
    }

    #[test]
    fn keyboard_without_arena_only_requests_redraw() {
        let mut core = RunnerCore::new(None, Vec::new(), true);
        let fx = core.handle_event(
            &key(Some("a"), ElementState::Pressed, false),
            win(),
            1.0,
            &FakeDrop::default(),
        );
        assert_eq!(
            fx,
            Effects {
                redraw: true,
                ..Effects::default()
            }
        );
    }

    #[test]
    fn unidentified_key_requests_redraw_without_dispatch() {
        let log = Log::default();
        let mut core = ready_core(&log);
        let event = key_with(
            Key::Unidentified(NativeKey::Unidentified),
            None,
            ElementState::Pressed,
            false,
        );
        let fx = core.handle_event(&event, win(), 1.0, &FakeDrop::default());
        assert_eq!(
            fx,
            Effects {
                redraw: true,
                ..Effects::default()
            }
        );
        assert!(!log.has("KeyPressed"));
    }

    #[test]
    fn text_less_key_press_dispatches_key_without_commit() {
        let log = Log::default();
        let mut core = ready_core(&log);
        core.handle_event(
            &key(None, ElementState::Pressed, false),
            win(),
            1.0,
            &FakeDrop::default(),
        );
        assert!(log.has("KeyPressed"));
        assert!(!log.has("ImeCommitted"));
    }

    #[test]
    fn ime_enabled_requests_redraw_without_delivery() {
        let log = Log::default();
        let mut core = ready_core(&log);
        let event = WindowEvent::Ime(winit::event::Ime::Enabled);
        let fx = core.handle_event(&event, win(), 1.0, &FakeDrop::default());
        assert_eq!(
            fx,
            Effects {
                redraw: true,
                ..Effects::default()
            }
        );
        assert!(!log.has("Ime"));
    }

    #[test]
    fn redraw_requests_are_left_to_the_shell() {
        let log = Log::default();
        let mut core = ready_core(&log);
        let fx = core.handle_event(
            &WindowEvent::RedrawRequested,
            win(),
            1.0,
            &FakeDrop::default(),
        );
        assert_eq!(fx, Effects::default());
    }

    #[test]
    fn layout_without_arena_only_clears_the_flag() {
        let mut core = RunnerCore::new(None, Vec::new(), true);
        core.layout(10, 10);
        assert!(!core.needs_layout);
    }

    #[test]
    fn focus_request_from_widget_is_applied_on_next_frame() {
        let log = Log::default();
        let mut core = ready_core(&log);
        let root = core.root.expect("root mounted");
        core.arena.as_mut().expect("arena").request_focus(root);
        core.handle_event(
            &pointer_moved_at(350.0, 350.0),
            win(),
            1.0,
            &FakeDrop::default(),
        );
        assert!(core.prepare_frame(Duration::ZERO).is_some());
        assert_eq!(core.focus.current_focus(), Some(root));
    }

    #[test]
    fn drop_payload_bytes_stream_under_limit_is_read() {
        let data = FakeData {
            stream: Some(vec![1, 2, 3]),
            ..fake(TypeHint::Image {
                extension_hint: None,
            })
        };
        assert!(matches!(
            drop_payload(&data),
            Ok(DropPayload::Bytes(bytes)) if bytes == [1u8, 2, 3]
        ));
    }

    #[test]
    fn drop_payload_bytes_fallback_over_limit_is_rejected() {
        let data = FakeData {
            bytes: Some(vec![0u8; MAX_DROP_PAYLOAD_BYTES + 1]),
            ..fake(TypeHint::Image {
                extension_hint: None,
            })
        };
        let err = drop_payload(&data).expect_err("oversize bytes");
        assert_eq!(err.kind(), io::ErrorKind::InvalidData);
    }

    #[test]
    fn drop_payload_pending_uri_list_stays_pending() {
        let data = FakeData {
            pending: true,
            ..fake(TypeHint::UriList)
        };
        let err = drop_payload(&data).expect_err("pending read");
        assert!(is_pending(&err));
    }

    #[test]
    fn drop_payload_bytes_fallback_under_limit_is_read() {
        let data = FakeData {
            bytes: Some(vec![7, 8]),
            ..fake(TypeHint::Image {
                extension_hint: None,
            })
        };
        assert!(matches!(
            drop_payload(&data),
            Ok(DropPayload::Bytes(bytes)) if bytes == [7u8, 8]
        ));
    }

    #[test]
    fn launch_error_display_names_each_stage() {
        let event_loop = winit::error::EventLoopError::RecreationAttempt;
        assert_eq!(
            LaunchError::EventLoop(event_loop).to_string(),
            format!(
                "event loop: {}",
                winit::error::EventLoopError::RecreationAttempt
            )
        );
        assert!(LaunchError::Window(winit::error::RequestError::Ignored)
            .to_string()
            .starts_with("window: "));
        assert_eq!(
            LaunchError::SurfaceConfig("no format".into()).to_string(),
            "surface configuration: no format"
        );
        assert!(LaunchError::GpuInit("x".into())
            .to_string()
            .starts_with("gpu initialization: "));
    }

    #[test]
    fn windowless_shell_helpers_fall_back_to_unit_scale() {
        let log = Log::default();
        let mut runner = Runner {
            config: RunnerConfig::new("t"),
            core: ready_core(&log),
            window: None,
            gpu: None,
            surface: None,
            orchestrator: None,
            recovery: RecoveryMachine::new(),
            launch_error: Arc::new(std::sync::Mutex::new(None)),
            last_frame: Instant::now(),
        };
        assert_eq!(runner.scale(), 1.0);
        assert_eq!(runner.clamped_extent(800, 600), (800, 600));
        runner.request_redraw();
        runner.frame(Duration::ZERO);
        assert!(runner.recovery.is_active());
    }
}
