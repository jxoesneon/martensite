//! Window assembly — the same distilled pipeline as `morph_viewer`,
//! shared by both catalog targets:
//!
//! * **Desktop** — winit + `RenderOrchestrator` (Vello with TinySkia
//!   fallback), the platform AccessKit adapter (`accesskit_winit`), and
//!   the dev-channel socket (`MARTENSITE_DEV_CHANNEL=1`) so
//!   `cargo martensite mcp` can inspect and drive the running catalog.
//! * **Web (`wasm32-unknown-unknown`)** — the same `App` driving a
//!   `<canvas>` through `martensite-web`: the window is created from
//!   `WebWindowAttributes`, the GPU probe runs asynchronously
//!   (`WebGpu` → Vello, `WebGl2`/no adapter → TinySkia CPU raster), the
//!   real `AccessKitAdapter` tree feeds a `WebA11yBridge` DOM mirror,
//!   and the opt-in `web-dev` feature serves the dev channel over the
//!   local `cargo martensite dev-web` relay (ADR-0042).
//!
//! Wall-clock timing goes through [`now_ms`]: `Instant::now` traps on
//! wasm, so the web arm reads `performance.now()` instead. Everything
//! `std::env`/process/thread-bound stays behind the non-wasm cfg.

use std::sync::{Arc, Mutex};
use std::time::Duration;

use accesskit::ActionRequest;
#[cfg(not(all(target_arch = "wasm32", target_os = "unknown")))]
use accesskit::TreeUpdate;
use glam::Vec2;
use martensite::access::actions::dispatch_a11y_action;
use martensite::access::adapter::AccessKitAdapter;
use martensite::core::{HotNode, LayoutContext, NodeFlags, PaintList, Rect, WidgetId};
#[cfg(any(
    not(all(target_arch = "wasm32", target_os = "unknown")),
    feature = "web-dev"
))]
use martensite::devtools::dev_session::SignalAdapter;
use martensite::focus::FocusManager;
#[cfg(any(
    not(all(target_arch = "wasm32", target_os = "unknown")),
    feature = "web-dev"
))]
use martensite::reactive::Signal;
use martensite::theme::ThemeDictionary;
use martensite::window::event::MouseButton as MButton;
use martensite::window::{EventRouter, ModifierKeys, PointerEvent, PointerId, PointerKind};
use martensite_wgpu::{
    BackdropMode, GpuContext, OrchestratorConfig, PresentModePreference, RecoveryMachine,
    RenderOrchestrator, SurfaceWrapper,
};
use winit::application::ApplicationHandler;
use winit::event::{ButtonSource, ElementState, MouseButton, WindowEvent};
use winit::event_loop::ActiveEventLoop;
use winit::window::{Window, WindowId};

#[cfg(not(all(target_arch = "wasm32", target_os = "unknown")))]
use winit::dpi::LogicalSize;
#[cfg(not(all(target_arch = "wasm32", target_os = "unknown")))]
use winit::event_loop::{ControlFlow, EventLoop};
#[cfg(not(all(target_arch = "wasm32", target_os = "unknown")))]
use winit::window::WindowAttributes;

#[cfg(all(target_arch = "wasm32", target_os = "unknown"))]
use std::cell::RefCell;
#[cfg(all(target_arch = "wasm32", target_os = "unknown"))]
use std::rc::Rc;

use crate::pages::all_pages;
use crate::view::CatalogView;

/// Min interval between full a11y tree emissions (milliseconds).
/// `build_update` emits the entire tree — hundreds of nodes — so
/// running it per frame saturates both the app and the AT client.
/// 100 ms (10 Hz) is well under any AT polling cadence, while
/// `a11y_force` (AT action dispatched or focus moved) still emits
/// immediately so interactive latency stays one frame.
const A11Y_EMIT_MS: f64 = 100.0;

/// Logical (CSS-pixel) size the web canvas is laid out at. The backing
/// store is `LOGICAL_* × devicePixelRatio` physical pixels.
#[cfg(all(target_arch = "wasm32", target_os = "unknown"))]
pub(crate) const LOGICAL_WIDTH: u32 = 1280;
#[cfg(all(target_arch = "wasm32", target_os = "unknown"))]
pub(crate) const LOGICAL_HEIGHT: u32 = 860;

/// Wall clock in milliseconds.
///
/// The web arm reads `performance.now()`: `std::time::Instant::now`
/// traps on `wasm32-unknown-unknown` (there is no OS clock behind it),
/// so every frame-time measurement in the shared app path goes through
/// this helper. The native arm keeps `Instant` semantics by anchoring
/// to a process-lifetime epoch.
#[cfg(not(all(target_arch = "wasm32", target_os = "unknown")))]
fn now_ms() -> f64 {
    static EPOCH: std::sync::OnceLock<std::time::Instant> = std::sync::OnceLock::new();
    EPOCH
        .get_or_init(std::time::Instant::now)
        .elapsed()
        .as_secs_f64()
        * 1000.0
}

/// Wall clock in milliseconds — `performance.now()` on the web.
#[cfg(all(target_arch = "wasm32", target_os = "unknown"))]
fn now_ms() -> f64 {
    web_sys::window()
        .and_then(|window| window.performance())
        .map(|performance| performance.now())
        .unwrap_or(0.0)
}

/// AccessKit wiring helpers. The winit adapter must exist before the
/// window is first shown (platform contract); the initial tree is
/// staged through `initial` so the activation handler can hand it to
/// the platform the moment AT attaches.
#[cfg(not(all(target_arch = "wasm32", target_os = "unknown")))]
struct InitialTree(Arc<Mutex<Option<TreeUpdate>>>);
#[cfg(not(all(target_arch = "wasm32", target_os = "unknown")))]
impl accesskit::ActivationHandler for InitialTree {
    fn request_initial_tree(&mut self) -> Option<TreeUpdate> {
        self.0.lock().unwrap().take()
    }
}

/// AT actions arrive on the platform's callback thread — queue them and
/// let the frame loop drain + decode + dispatch on the event thread.
#[cfg(not(all(target_arch = "wasm32", target_os = "unknown")))]
struct QueueActions(Arc<Mutex<Vec<ActionRequest>>>);
#[cfg(not(all(target_arch = "wasm32", target_os = "unknown")))]
impl accesskit::ActionHandler for QueueActions {
    fn do_action(&mut self, request: ActionRequest) {
        self.0.lock().unwrap().push(request);
    }
}

#[cfg(not(all(target_arch = "wasm32", target_os = "unknown")))]
struct NoopDeactivate;
#[cfg(not(all(target_arch = "wasm32", target_os = "unknown")))]
impl accesskit::DeactivationHandler for NoopDeactivate {
    fn deactivate_accessibility(&mut self) {}
}

/// The windowed app state. GPU/window-bound members are created lazily
/// in `can_create_surfaces` per the winit 0.31 lifecycle; on the web the
/// adapter probe resolves asynchronously, so `web_init` lands the
/// `GpuContext`/surface/font fixtures at the first frame after both
/// legs complete (`try_finish_web_init`).
pub(crate) struct App {
    arena: Option<Arc<Mutex<martensite::core::WidgetArena>>>,
    root: Option<WidgetId>,
    router: EventRouter,
    /// Focus sink for the single-root app: the catalog holds focus so
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
    #[cfg(not(all(target_arch = "wasm32", target_os = "unknown")))]
    _dev_server: Option<martensite::dev_channel::DevChannelServer>,
    /// Web dev-channel WebSocket leg (ADR-0042) — held for its `Drop`
    /// (closes the relay socket). Present only when the `web-dev`
    /// feature is on *and* the page URL carried `?dev_token=`.
    #[cfg(all(target_arch = "wasm32", target_os = "unknown", feature = "web-dev"))]
    _web_channel: Option<martensite::devtools::web_channel::WebDevChannel>,
    /// In-memory log ring attached to the dev session — powers
    /// `martensite_logs` (paint-lint findings included). Only present
    /// where a consumer exists: the native tracing layer and the
    /// `web-dev` session.
    #[cfg(any(
        not(all(target_arch = "wasm32", target_os = "unknown")),
        feature = "web-dev"
    ))]
    log_ring: Arc<martensite::devtools::dev_session::log_ring::LogRing>,
    /// Fallback viewport size when no window exists — set from
    /// `MARTENSITE_HEADLESS_SIZE` in `run_live_headless`.
    headless_size: (u32, u32),
    needs_layout: bool,
    /// Wall-clock ms of the last rendered frame — see [`now_ms`].
    last_frame_ms: f64,
    /// Semantic-tree producer for both targets: walks the catalog arena
    /// and emits real `TreeUpdate`s.
    a11y_tree: Option<AccessKitAdapter>,
    /// AT action requests queued by the platform adapter (native) or
    /// the DOM mirror (web), drained on the event thread each frame.
    a11y_actions: Arc<Mutex<Vec<ActionRequest>>>,
    /// Last time the a11y tree update was emitted (ms) — throttles
    /// full-tree rebuilds to `A11Y_EMIT_MS`.
    a11y_last_emit_ms: f64,
    /// Force an a11y emit on the next pump (action dispatched or the
    /// focus moved).
    a11y_force: bool,
    a11y_last_focus: Option<WidgetId>,
    /// Staged initial tree for the native `accesskit_winit` adapter's
    /// activation handler.
    #[cfg(not(all(target_arch = "wasm32", target_os = "unknown")))]
    a11y_initial: Arc<Mutex<Option<TreeUpdate>>>,
    /// Native platform AccessKit adapter (AT-SPI / UI Automation /
    /// NSAccessibility).
    #[cfg(not(all(target_arch = "wasm32", target_os = "unknown")))]
    a11y_platform: Option<accesskit_winit::Adapter>,
    /// Web DOM/ARIA mirror — off by default; activated by its hidden
    /// "Enable accessibility" button or the `martensite_catalog_enable_a11y`
    /// JS export (used by the browser gate).
    #[cfg(all(target_arch = "wasm32", target_os = "unknown"))]
    a11y_bridge: Option<martensite_web::access::WebA11yBridge>,
    /// Async web bootstrap state (GPU probe, font fetch, canvas).
    #[cfg(all(target_arch = "wasm32", target_os = "unknown"))]
    pub(crate) web_init: Rc<RefCell<crate::web::WebInit>>,
    /// Hidden `<input>` hosting IME composition for the canvas — kept
    /// for its DOM listeners; events land in `ime_queue`.
    #[cfg(all(target_arch = "wasm32", target_os = "unknown"))]
    _ime: Option<martensite_web::window::HiddenImeInput>,
    /// IME events captured on the hidden input, drained each frame
    /// into `EventRouter::dispatch_ime_event`.
    #[cfg(all(target_arch = "wasm32", target_os = "unknown"))]
    ime_queue: Rc<RefCell<Vec<martensite::window::event::ImeEvent>>>,
    /// Keeps the fetched-font fixture override installed for the app's
    /// lifetime — lazily-created `FontManager`s on the wasm main
    /// thread consult it on construction.
    #[cfg(all(target_arch = "wasm32", target_os = "unknown"))]
    _font_guard: Option<martensite::text::font::TestFontsGuard>,
    /// The `wgpu::Instance` the surface and adapter were created
    /// from — held for the app's lifetime so wgpu's wasm-side external
    /// handle for it can never be dropped under the live surface.
    #[cfg(all(target_arch = "wasm32", target_os = "unknown"))]
    _web_instance: Option<martensite_web::wgpu::Instance>,
    /// F12 diagnostic HUD — web-dev builds only.
    #[cfg(all(target_arch = "wasm32", target_os = "unknown", feature = "web-dev"))]
    hud: martensite::devtools::hud::DiagnosticHud,
    /// Inspector overlay state — toggled alongside the HUD.
    #[cfg(all(target_arch = "wasm32", target_os = "unknown", feature = "web-dev"))]
    inspector: martensite::devtools::inspector::InspectorState,
}

impl App {
    fn new() -> Self {
        Self {
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
            #[cfg(not(all(target_arch = "wasm32", target_os = "unknown")))]
            _dev_server: None,
            #[cfg(all(target_arch = "wasm32", target_os = "unknown", feature = "web-dev"))]
            _web_channel: None,
            #[cfg(any(
                not(all(target_arch = "wasm32", target_os = "unknown")),
                feature = "web-dev"
            ))]
            log_ring: Arc::new(martensite::devtools::dev_session::log_ring::LogRing::new(
                512,
            )),
            headless_size: (1280, 860),
            needs_layout: true,
            last_frame_ms: now_ms(),
            a11y_tree: None,
            a11y_actions: Arc::new(Mutex::new(Vec::new())),
            a11y_last_emit_ms: now_ms(),
            a11y_force: true,
            a11y_last_focus: None,
            #[cfg(not(all(target_arch = "wasm32", target_os = "unknown")))]
            a11y_initial: Arc::new(Mutex::new(None)),
            #[cfg(not(all(target_arch = "wasm32", target_os = "unknown")))]
            a11y_platform: None,
            #[cfg(all(target_arch = "wasm32", target_os = "unknown"))]
            a11y_bridge: None,
            #[cfg(all(target_arch = "wasm32", target_os = "unknown"))]
            web_init: Rc::new(RefCell::new(crate::web::WebInit::default())),
            #[cfg(all(target_arch = "wasm32", target_os = "unknown"))]
            _ime: None,
            #[cfg(all(target_arch = "wasm32", target_os = "unknown"))]
            ime_queue: Rc::new(RefCell::new(Vec::new())),
            #[cfg(all(target_arch = "wasm32", target_os = "unknown"))]
            _font_guard: None,
            #[cfg(all(target_arch = "wasm32", target_os = "unknown"))]
            _web_instance: None,
            #[cfg(all(target_arch = "wasm32", target_os = "unknown", feature = "web-dev"))]
            hud: martensite::devtools::hud::DiagnosticHud::new(),
            #[cfg(all(target_arch = "wasm32", target_os = "unknown", feature = "web-dev"))]
            inspector: martensite::devtools::inspector::InspectorState::new(),
        }
    }

    /// Web entry state: same `App`, pre-bound to the page canvas the
    /// `#[wasm_bindgen(start)]` shim looked up.
    #[cfg(all(target_arch = "wasm32", target_os = "unknown"))]
    pub(crate) fn new_web(canvas: web_sys::HtmlCanvasElement) -> Self {
        let app = Self::new();
        app.web_init.borrow_mut().canvas = Some(canvas);
        app
    }

    /// Builds the arena: dark theme, shared text painter, platform
    /// preferences, the catalog root, and — when the dev channel is
    /// enabled for the target — the session with the signal adapters
    /// registered for `signals_list`/`signal_trigger`/`signal_set`.
    fn build_arena(&mut self, scale: f32) {
        let mut arena = martensite::core::WidgetArena::new();
        arena.set_theme(ThemeDictionary::new().dark_theme().clone());
        arena.set_scale_factor(scale);
        arena.set_text_painter(martensite::text_paint::shared_painter());
        martensite::window::prefs::apply_platform_preferences(&mut arena);

        let mut hot = HotNode::default();
        hot.flags |= NodeFlags::VISIBLE | NodeFlags::HIT_TEST_ENABLED;
        let view = CatalogView::new(all_pages());
        // Clone the signal handles before the view moves into the arena —
        // the dev session's writes land in `reconcile` drains. Both dev
        // legs (the host socket and the web relay) consume the same
        // adapter set; a wasm build without `web-dev` skips it entirely.
        #[cfg(any(
            not(all(target_arch = "wasm32", target_os = "unknown")),
            feature = "web-dev"
        ))]
        let signal_adapters = vec![
            signal_adapter("sel", view.sig_sel.clone()),
            signal_adapter("search", view.sig_search.clone()),
            signal_adapter("stage_theme", view.sig_theme.clone()),
            signal_adapter("rtl", view.sig_rtl.clone()),
            signal_adapter("stage_locale", view.sig_locale.clone()),
            signal_adapter("frame", view.sig_frame.clone()),
            signal_adapter("zoom", view.sig_zoom.clone()),
            signal_adapter("prop", view.sig_prop.clone()),
        ];
        let root = arena.insert_with_widget(hot, Box::new(view));
        // The catalog root is the app's only key target — the session's
        // `key_press`/`text_input` dispatches resolve through this.
        self.focus.lock().unwrap().set_focus_unchecked(root);

        let arena = Arc::new(Mutex::new(arena));
        #[cfg(not(all(target_arch = "wasm32", target_os = "unknown")))]
        match martensite::dev_channel::serve_dev_session_from_env(Arc::clone(&arena)) {
            Ok(Some((server, session))) => {
                session.attach_focus_manager(Arc::clone(&self.focus));
                session.set_log_ring(Arc::clone(&self.log_ring));
                for adapter in signal_adapters {
                    session.register_signal_adapter(adapter);
                }
                self.dev_session = Some(session);
                self._dev_server = Some(server);
                eprintln!(
                    "widget_catalog: dev channel serving (pid {})",
                    std::process::id()
                );
            }
            Ok(None) => {}
            Err(err) => eprintln!("widget_catalog: dev channel disabled: {err}"),
        }
        // The web leg (ADR-0042) dials out to the `cargo martensite
        // dev-web` relay instead of binding a socket — and only when the
        // feature is compiled in *and* the page URL carries `?dev_token=`.
        #[cfg(all(target_arch = "wasm32", target_os = "unknown", feature = "web-dev"))]
        if let Some((session, channel)) = crate::web_dev::serve(
            Arc::clone(&arena),
            Arc::clone(&self.focus),
            Arc::clone(&self.log_ring),
            signal_adapters,
        ) {
            self.dev_session = Some(session);
            self._web_channel = Some(channel);
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
        // Manual layout bypasses `LayoutEngine` — install the ambient
        // measurer so widget `measure` calls see the same glyph metrics
        // the paint pass will use (otherwise text lays out zero-height).
        let _measurer = arena
            .text_painter_shared()
            .map(martensite::core::paint::install_ambient_measurer);
        let scale = arena.scale_factor();
        let r = Rect::new(0.0, 0.0, w as f32, h as f32);
        // Overlay popups clamp against this.
        arena.overlay_mut().set_viewport(r);
        if let Some((hot, cold)) = arena.get_both_mut(root) {
            // `Flex` sizes its children in `measure`; `layout` alone
            // leaves every row at zero height.
            cold.widget.measure(
                &mut LayoutContext { hot, scale },
                martensite::core::LayoutConstraints {
                    min_size: Vec2::ZERO,
                    max_size: Vec2::new(r.width(), r.height()),
                },
            );
            hot.bounds = r;
            cold.widget.layout(&mut LayoutContext { hot, scale }, r);
        }
    }

    /// Last laid-out viewport size — the headless path relayouts to it
    /// every frame.
    fn layout_size(&self) -> (u32, u32) {
        self.window
            .as_ref()
            .map(|w| {
                let s = w.surface_size();
                (s.width, s.height)
            })
            .unwrap_or(self.headless_size)
    }

    /// Lets the [`CatalogView`] drain its controls, signals, and
    /// staged-widget event queues once per frame, then re-lays out —
    /// the catalog is small enough that a relayout per frame is
    /// cheaper than diffing which children changed.
    fn reconcile_view(&mut self) {
        let Some(arena) = self.arena.clone() else {
            return;
        };
        let Some(root) = self.root else { return };
        {
            let mut arena = arena.lock().unwrap();
            if let Some((_hot, cold)) = arena.get_both_mut(root) {
                if let Some(view) = cold
                    .widget
                    .as_any_mut()
                    .and_then(|a| a.downcast_mut::<CatalogView>())
                {
                    view.reconcile();
                }
            }
        }
        let (w, h) = self.layout_size();
        self.layout(w, h);
    }

    /// Drains queued AT-action requests into the arena (decode →
    /// dispatch), refreshes focus, and emits a `TreeUpdate` at the
    /// `A11Y_EMIT_MS` cadence — to the platform adapter on native, to
    /// the `WebA11yBridge` DOM mirror on the web.
    fn pump_a11y(&mut self) {
        let Some(tree) = &mut self.a11y_tree else {
            return;
        };
        let Some(arena) = &self.arena else { return };
        let mut arena = arena.lock().unwrap();
        let mut dispatched = false;
        for request in std::mem::take(&mut *self.a11y_actions.lock().unwrap()) {
            if let Some(action) = tree.decode_action(&arena, &request) {
                dispatch_a11y_action(&mut arena, &action);
                dispatched = true;
            }
        }
        let focus = self.focus.lock().unwrap().current_focus();
        tree.set_focus(focus);
        if dispatched || focus != self.a11y_last_focus {
            self.a11y_force = true;
        }
        // The programmatic-enable hook (browser gate / console) is
        // consumed outside the emit throttle so enabling lands
        // immediately even between emits.
        #[cfg(all(target_arch = "wasm32", target_os = "unknown"))]
        if crate::web::take_a11y_enable_request() {
            if let Some(bridge) = &mut self.a11y_bridge {
                if let Err(err) = bridge.set_enabled(true) {
                    crate::web::log(&format!("a11y mirror enable failed: {err}"));
                }
            }
        }
        let now = now_ms();
        if !(self.a11y_force || now - self.a11y_last_emit_ms >= A11Y_EMIT_MS) {
            return;
        }
        #[cfg(not(all(target_arch = "wasm32", target_os = "unknown")))]
        if let Some(adapter) = &mut self.a11y_platform {
            adapter.update_if_active(|| tree.build_update(&mut arena));
        }
        // `WebA11yBridge::update` also accumulates while disabled — the
        // pending tree materializes atomically on activation, so the
        // mirror always reflects the latest emitted state.
        #[cfg(all(target_arch = "wasm32", target_os = "unknown"))]
        if let Some(bridge) = &mut self.a11y_bridge {
            let update = tree.build_update(&mut arena);
            if let Err(err) = bridge.update(&update) {
                crate::web::log(&format!("a11y tree update failed: {err}"));
            }
        }
        self.a11y_force = false;
        self.a11y_last_emit_ms = now;
        self.a11y_last_focus = focus;
    }

    /// Completes the deferred web bootstrap once the async legs (GPU
    /// probe, font fetch) have both landed: swapchain, orchestrator,
    /// arena, and the a11y mirror all come up in one shot. Until then
    /// the canvas keeps requesting frames so the loop stays alive.
    #[cfg(all(target_arch = "wasm32", target_os = "unknown"))]
    fn try_finish_web_init(&mut self) {
        if self.arena.is_some() {
            return;
        }
        let ready = {
            let init = self.web_init.borrow();
            init.gpu_ready && init.font_ready
        };
        if !ready {
            return;
        }
        let Some(window) = self.window.clone() else {
            return;
        };
        let (gpu, instance, raw_surface, prefer_cpu) = self.web_init.borrow_mut().take_gpu();
        self._web_instance = instance;
        let size = window.surface_size();
        let w = size.width.max(1);
        let h = size.height.max(1);
        if let (Some(gpu), Some(raw_surface)) = (gpu, raw_surface) {
            let mut surface = SurfaceWrapper::new(raw_surface);
            surface.set_pacing(PresentModePreference::LowLatency);
            if let Err(err) =
                surface.configure(&gpu.device, &gpu.adapter, w, h, BackdropMode::Opaque)
            {
                crate::web::log(&format!("surface configure failed: {err}"));
            } else {
                self.surface = Some(surface);
            }
            if let Ok(mut orchestrator) =
                RenderOrchestrator::new(w, h, OrchestratorConfig::new(true, prefer_cpu))
            {
                let notify_window = Arc::clone(&window);
                orchestrator.set_pre_present_notify(Some(Box::new(move || {
                    notify_window.pre_present_notify();
                })));
                self.orchestrator = Some(orchestrator);
            } else {
                crate::web::log("render orchestrator init failed");
            }
            self.gpu = Some(gpu);
        }
        // Arena + a11y come up even without a GPU adapter — the mirror
        // and dev session still work against a blank canvas.
        let scale = web_sys::window()
            .map(|window| window.device_pixel_ratio() as f32)
            .unwrap_or(1.0);
        self.build_arena(scale);
        self.needs_layout = true;
        self.init_web_a11y();
        // Keep the fixture-font override alive for the app's lifetime.
        if let Some(guard) = self.web_init.borrow_mut().font_guard.take() {
            self._font_guard = Some(guard);
        }
        window.request_redraw();
    }

    /// Builds the `WebA11yBridge` mirror, wires its action requests back
    /// into the arena through the same `decode_action` path the native
    /// adapter uses, and stages the initial tree. Off by default — the
    /// bridge's own activation button (or the JS export) turns it on.
    #[cfg(all(target_arch = "wasm32", target_os = "unknown"))]
    fn init_web_a11y(&mut self) {
        let Some(root) = self.root else { return };
        let Some(arena) = &self.arena else { return };
        let mut tree = AccessKitAdapter::new(root);
        tree.set_toolkit_name("Martensite");
        tree.set_toolkit_version(env!("CARGO_PKG_VERSION"));
        tree.set_focus(self.focus.lock().unwrap().current_focus());
        match martensite_web::access::WebA11yBridge::new() {
            Ok(mut bridge) => {
                let queue = Arc::clone(&self.a11y_actions);
                bridge.set_action_handler(move |request| {
                    queue.lock().unwrap().push(request);
                });
                let update = tree.build_update(&mut arena.lock().unwrap());
                if let Err(err) = bridge.update(&update) {
                    crate::web::log(&format!("a11y tree update failed: {err}"));
                }
                self.a11y_bridge = Some(bridge);
            }
            Err(err) => crate::web::log(&format!("a11y bridge unavailable: {err}")),
        }
        self.a11y_tree = Some(tree);
    }

    /// Drains IME events captured on the hidden input overlay into the
    /// widget event pipeline (composition runs on the DOM element, not
    /// through winit's `WindowEvent::Ime` on the web).
    #[cfg(all(target_arch = "wasm32", target_os = "unknown"))]
    fn drain_ime_queue(&mut self) {
        let events = std::mem::take(&mut *self.ime_queue.borrow_mut());
        if events.is_empty() {
            return;
        }
        if let (Some(arena), Some(root)) = (&self.arena, self.root) {
            let mut arena = arena.lock().unwrap();
            for event in &events {
                self.router
                    .dispatch_ime_event(&mut arena, Some(root), event);
            }
        }
    }

    /// One frame: tick springs, reconcile the catalog view, rebuild
    /// the paint list, feed the dev session, pump the a11y tree, render.
    /// Returns early without GPU pieces (live-headless path renders
    /// nothing but still feeds the session and the a11y mirror).
    fn frame(&mut self, dt: Duration) {
        #[cfg(all(target_arch = "wasm32", target_os = "unknown"))]
        {
            self.try_finish_web_init();
            if self.arena.is_none() {
                // Async legs still in flight — keep the rAF loop alive.
                if let Some(window) = &self.window {
                    window.request_redraw();
                }
                return;
            }
        }
        let Some(arena) = self.arena.clone() else {
            return;
        };
        let Some(root) = self.root else { return };
        arena.lock().unwrap().tick(dt);
        self.reconcile_view();
        #[cfg(all(target_arch = "wasm32", target_os = "unknown"))]
        self.drain_ime_queue();
        let mut list = PaintList::new();
        arena.lock().unwrap().build_paint_list(root, &mut list);
        if let Some(session) = &self.dev_session {
            session.on_frame(&list);
            session.absorb_events(self.router.event_ledger());
        }
        self.pump_a11y();

        #[cfg(all(target_arch = "wasm32", target_os = "unknown", feature = "web-dev"))]
        {
            self.hud
                .record_frame(martensite::devtools::hud::FrameTiming::from_ms(
                    0.0,
                    0.0,
                    0.0,
                    dt.as_secs_f64() * 1000.0,
                ));
            if let Some(arena) = &self.arena {
                let (used, capacity) = {
                    let arena = arena.lock().unwrap();
                    (arena.len(), arena.capacity())
                };
                self.hud.update_arena_telemetry(
                    martensite::devtools::hud::ArenaTelemetry::from_slots(capacity, used, 0),
                );
            }
            if self.hud.is_enabled() {
                self.hud.render_hud(&mut list);
            }
        }

        match (
            &self.window,
            &self.gpu,
            &mut self.surface,
            &mut self.orchestrator,
        ) {
            (Some(window), Some(gpu), Some(surface), Some(orchestrator)) => {
                orchestrator.render(&list, &self.recovery);
                if let Err(err) = orchestrator.render_to_surface(&gpu.device, &gpu.queue, surface) {
                    self.recovery.handle_surface_error(err);
                    let size = window.surface_size();
                    if let Err(err) =
                        surface.resize(&gpu.device, size.width.max(1), size.height.max(1))
                    {
                        eprintln!("widget_catalog: surface resize failed: {err}");
                    }
                }
                window.request_redraw();
            }
            _ => {
                // No present target (headless, or the web GPU probe
                // found no adapter) — still keep the loop alive so the
                // session and mirror keep pumping.
                if let Some(window) = &self.window {
                    window.request_redraw();
                }
            }
        }
    }

    /// Tick + reconcile + paint-list build + dev-session feed — the
    /// windowless half of `frame` (no orchestrator exists in headless
    /// mode).
    #[cfg(not(all(target_arch = "wasm32", target_os = "unknown")))]
    fn frame_headless(&mut self, dt: Duration) {
        let Some(arena) = self.arena.clone() else {
            return;
        };
        let Some(root) = self.root else { return };
        arena.lock().unwrap().tick(dt);
        self.reconcile_view();
        let mut list = PaintList::new();
        arena.lock().unwrap().build_paint_list(root, &mut list);
        if let Some(session) = &self.dev_session {
            session.on_frame(&list);
            session.absorb_events(self.router.event_ledger());
        }
        self.pump_a11y();
    }

    /// winit pointer event → `EventRouter` → arena dispatch.
    /// Identical on both targets: `ClickTracker` inside
    /// `dispatch_pointer_event` runs on a wasm-safe clock.
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

    /// Native surface bring-up: window (invisible until the a11y
    /// adapter exists), synchronous GPU/swapchain creation, arena,
    /// AccessKit staging, then reveal.
    #[cfg(not(all(target_arch = "wasm32", target_os = "unknown")))]
    fn create_surfaces_native(&mut self, event_loop: &dyn ActiveEventLoop) {
        // Invisible first — the AccessKit adapter must be created
        // before the window is shown (platform contract).
        let window: Arc<dyn Window> = event_loop
            .create_window(
                WindowAttributes::default()
                    .with_title("Martensite — Widget Catalog")
                    .with_surface_size(LogicalSize::new(1280.0, 860.0))
                    .with_visible(false),
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

        // Arena + initial a11y tree — all before show.
        self.build_arena(window.scale_factor() as f32);
        self.needs_layout = true;
        self.layout(size.width.max(1), size.height.max(1));

        let root = self.root.expect("arena built");
        let mut tree = AccessKitAdapter::new(root);
        tree.set_toolkit_name("Martensite");
        tree.set_toolkit_version(env!("CARGO_PKG_VERSION"));
        tree.set_focus(self.focus.lock().unwrap().current_focus());
        {
            let arena = self.arena.as_ref().expect("arena built");
            *self.a11y_initial.lock().unwrap() =
                Some(tree.build_update(&mut arena.lock().unwrap()));
        }
        self.a11y_platform = Some(accesskit_winit::Adapter::with_direct_handlers(
            event_loop,
            window.as_ref(),
            InitialTree(Arc::clone(&self.a11y_initial)),
            QueueActions(Arc::clone(&self.a11y_actions)),
            NoopDeactivate,
        ));
        self.a11y_tree = Some(tree);

        self.window = Some(window);
        self.gpu = Some(gpu);
        self.surface = Some(surface);
        self.orchestrator = Some(orchestrator);

        if let Some(window) = &self.window {
            window.set_visible(true);
            window.request_redraw();
        }
    }

    /// Web surface bring-up: binds the page's `<canvas>` to a winit
    /// window via `WebWindowAttributes`, installs the hidden IME input,
    /// and requests the first frame. GPU/surface/arena construction
    /// completes in `try_finish_web_init` once the async legs land.
    #[cfg(all(target_arch = "wasm32", target_os = "unknown"))]
    fn create_surfaces_web(&mut self, event_loop: &dyn ActiveEventLoop) {
        use martensite_web::window as win_web;

        let canvas = self
            .web_init
            .borrow()
            .canvas
            .clone()
            .expect("web canvas bound at start()");
        let attrs = win_web::WebWindowAttributes::new()
            .with_canvas(Some(canvas))
            .with_append(true)
            .with_focusable(true)
            .with_prevent_default(true)
            .apply(
                martensite_web::winit::window::WindowAttributes::default()
                    .with_title("Martensite — Widget Catalog"),
            );
        let window: Arc<dyn Window> = event_loop
            .create_window(attrs)
            .expect("create web window")
            .into();

        // Hidden IME overlay — composition events queue into the next
        // frame's dispatch (they cannot borrow `self` from a DOM
        // callback).
        if let Some(body) = web_sys::window()
            .and_then(|window| window.document())
            .and_then(|document| {
                document
                    .body()
                    .map(wasm_bindgen::JsCast::unchecked_into::<web_sys::Element>)
            })
        {
            match win_web::HiddenImeInput::new(&body) {
                Ok(ime) => {
                    let queue = Rc::clone(&self.ime_queue);
                    ime.set_on_ime(move |event| {
                        use martensite::window::event::ImeEvent as Ev;
                        use martensite_web::window::ImeEvent as WebEv;
                        let mapped = match event {
                            WebEv::CompositionStart => Some(Ev::Preedit {
                                text: String::new(),
                                cursor: None,
                            }),
                            WebEv::CompositionUpdate(text) => {
                                Some(Ev::Preedit { text, cursor: None })
                            }
                            WebEv::CompositionEnd(text) | WebEv::InsertText(text) => {
                                Some(Ev::Committed(text))
                            }
                            _ => None,
                        };
                        if let Some(mapped) = mapped {
                            queue.borrow_mut().push(mapped);
                        }
                    });
                    self._ime = Some(ime);
                }
                Err(err) => crate::web::log(&format!("ime overlay unavailable: {err}")),
            }
        }

        self.window = Some(window);
        if let Some(window) = &self.window {
            window.request_redraw();
        }
    }
}

impl ApplicationHandler for App {
    fn can_create_surfaces(&mut self, event_loop: &dyn ActiveEventLoop) {
        if self.window.is_some() {
            return;
        }
        #[cfg(not(all(target_arch = "wasm32", target_os = "unknown")))]
        self.create_surfaces_native(event_loop);
        #[cfg(all(target_arch = "wasm32", target_os = "unknown"))]
        self.create_surfaces_web(event_loop);
    }

    fn window_event(
        &mut self,
        event_loop: &dyn ActiveEventLoop,
        _id: WindowId,
        event: WindowEvent,
    ) {
        // AT sees every event first (focus tracking, event filtering).
        #[cfg(not(all(target_arch = "wasm32", target_os = "unknown")))]
        if let (Some(adapter), Some(window)) = (&mut self.a11y_platform, &self.window) {
            adapter.process_event(window.as_ref(), &event);
        }
        match event {
            WindowEvent::CloseRequested | WindowEvent::Destroyed => event_loop.exit(),
            WindowEvent::SurfaceResized(size) => {
                // winit-web reports the canvas's device-pixel content box:
                // `size` is already physical — reconfigure with it as-is.
                if size.width > 0 && size.height > 0 {
                    if let (Some(surface), Some(gpu)) = (&mut self.surface, &self.gpu) {
                        if let Err(err) = surface.resize(&gpu.device, size.width, size.height) {
                            eprintln!("widget_catalog: surface resize failed: {err}");
                        }
                    }
                    if let Some(o) = &mut self.orchestrator {
                        o.set_frame_size(size.width, size.height);
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
                // F12 toggles the dev HUD + inspector overlay — web-dev
                // builds only (the native dev channel has no overlay).
                #[cfg(all(target_arch = "wasm32", target_os = "unknown", feature = "web-dev"))]
                if pressed
                    && matches!(
                        event.logical_key,
                        winit::keyboard::Key::Named(winit::keyboard::NamedKey::F12)
                    )
                {
                    self.hud.toggle();
                    self.inspector.toggle_active();
                    return;
                }
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
                let now = now_ms();
                let dt = Duration::from_secs_f64(((now - self.last_frame_ms) / 1000.0).max(0.0));
                self.last_frame_ms = now;
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
/// `signal_trigger`/`signal_set` writes land in the handle the widget
/// polls. Compiled whenever a dev leg exists (host socket or the
/// `web-dev` relay).
#[cfg(any(
    not(all(target_arch = "wasm32", target_os = "unknown")),
    feature = "web-dev"
))]
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

/// tracing init shared by `run`/`run_live_headless`: stderr fmt
/// honors `RUST_LOG` (default `warn`); the dev-channel `LogRing` sees
/// every INFO+ record so `martensite_logs` can slice by level/target.
/// Also installs the dev panic hook so `martensite_runtime_errors`
/// reports panics with node context.
#[cfg(not(all(target_arch = "wasm32", target_os = "unknown")))]
fn init_tracing(log_ring: &Arc<martensite::devtools::dev_session::log_ring::LogRing>) {
    use tracing_subscriber::layer::SubscriberExt;
    use tracing_subscriber::util::SubscriberInitExt;
    use tracing_subscriber::Layer;
    martensite::devtools::error_surface::install_dev_panic_hook();
    tracing_subscriber::registry()
        .with(tracing_subscriber::fmt::layer().with_filter(
            tracing_subscriber::EnvFilter::try_from_default_env().unwrap_or_else(|_| "warn".into()),
        ))
        .with(
            log_ring
                .layer()
                .with_filter(tracing_subscriber::filter::LevelFilter::INFO),
        )
        .try_init()
        .ok();
}

/// Windowed entry — winit + Vello GPU rendering.
#[cfg(not(all(target_arch = "wasm32", target_os = "unknown")))]
pub fn run() -> Result<(), Box<dyn std::error::Error>> {
    let event_loop = EventLoop::new()?;
    // Poll keeps a continuous frame cadence.
    event_loop.set_control_flow(ControlFlow::Poll);
    let app = App::new();
    init_tracing(&app.log_ring);
    event_loop.run_app(app)?;
    Ok(())
}

/// `run` is the desktop entry — on wasm the showcase starts through
/// the `#[wasm_bindgen(start)]` shim in `crate::web` instead. The stub
/// keeps the host binary's signature (and its callers in `main.rs`)
/// type-checking on the wasm target, where it is never executed.
#[cfg(all(target_arch = "wasm32", target_os = "unknown"))]
pub fn run() -> Result<(), Box<dyn std::error::Error>> {
    Err("widget_catalog: the windowed runner is desktop-only — on wasm use the `start` wasm-bindgen entry".into())
}

/// `--live-headless`: the full widget tree + dev channel with no
/// window/GPU — the MCP/dev-tooling path. `capture_node` still
/// rasterizes real PNGs via the devtools TinySkia backend.
///
/// `MARTENSITE_HEADLESS_SCALE` (default 1.0) and
/// `MARTENSITE_HEADLESS_SIZE` (`WxH`, default `1280x860`) let harnesses
/// exercise HiDPI scale factors and arbitrary window sizes without a
/// display — the W/H are device px, matching `App::layout`.
#[cfg(not(all(target_arch = "wasm32", target_os = "unknown")))]
pub fn run_live_headless() -> Result<(), Box<dyn std::error::Error>> {
    use std::time::Instant;
    let mut app = App::new();
    init_tracing(&app.log_ring);
    let scale: f32 = std::env::var("MARTENSITE_HEADLESS_SCALE")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(1.0);
    let (w, h) = std::env::var("MARTENSITE_HEADLESS_SIZE")
        .ok()
        .and_then(|v| {
            let (w, h) = v.split_once('x')?;
            Some((w.parse().ok()?, h.parse().ok()?))
        })
        .unwrap_or((1280u32, 860u32));
    app.headless_size = (w, h);
    app.build_arena(scale);
    app.layout(w, h);
    if app.dev_session.is_some() {
        eprintln!(
            "widget_catalog: live-headless dev channel serving (pid {})",
            std::process::id()
        );
    }
    let mut last = Instant::now();
    loop {
        let now = Instant::now();
        let dt = now - last;
        last = now;
        app.frame_headless(dt);
        std::thread::sleep(Duration::from_millis(16));
    }
}

/// Headless mode is desktop tooling — see the `run` stub for why a
/// wasm-typed copy still exists.
#[cfg(all(target_arch = "wasm32", target_os = "unknown"))]
pub fn run_live_headless() -> Result<(), Box<dyn std::error::Error>> {
    Err("widget_catalog: live-headless mode is desktop-only".into())
}
