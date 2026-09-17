//! The windowed application shell: winit event loop → `EventRouter` →
//! `WidgetArena` → `RenderOrchestrator`, with `DockTree` as the panel
//! geometry authority, `FocusManager` keyboard traversal, a live
//! AccessKit tree (real AT semantics — VoiceOver reads the panels), the
//! advisory paint-compliance audit, and the feature-gated devtools HUD.
//!
//! This is deliberately the production assembly path, not a shortcut:
//! `can_create_surfaces` wires exactly what `docs/tutorials` walks a
//! consumer through, plus the pieces the tutorial leaves as "larger
//! surface" (GPU pipeline, accessibility, focus).

use std::sync::Arc;
use std::time::Instant;

use accesskit::{ActionRequest, TreeUpdate};
use parking_lot::Mutex;

use glam::Vec2;
use martensite::access::actions::dispatch_a11y_action;
use martensite::access::adapter::AccessKitAdapter;
use martensite::core::{
    ColdNode, HotNode, LayoutContext, NodeFlags, Rect, WidgetArena, WidgetEvent, WidgetId,
};
use martensite::focus::{FocusManager, TabNavigation};
use martensite::prelude::*;
use martensite::render::{PaintList, Point};
use martensite::wgpu::{
    BackdropMode, GpuContext, OrchestratorConfig, PresentModePreference, RecoveryMachine,
    RenderOrchestrator, SurfaceWrapper,
};
use martensite::window::event::{
    EventRouter, ModifierKeys, MouseButton as MButton, PointerEvent, PointerId, PointerKind,
    PointerState,
};
use martensite::window::WindowId;

use winit::application::ApplicationHandler;
use winit::dpi::{PhysicalPosition, PhysicalSize};
use winit::event::{ElementState, MouseScrollDelta, WindowEvent};
use winit::event_loop::{ActiveEventLoop, ControlFlow, EventLoop, EventLoopProxy};
use winit::keyboard::{Key, ModifiersState, NamedKey};
use winit::window::{Window, WindowAttributes};

#[cfg(feature = "devtools")]
use martensite::devtools::hud::{DiagnosticHud, FrameTiming};

use crate::model::{build_dock_tree, Palette};
use crate::panels::{fmt_count, EditorPanel, GridPanel, MediaPanel, TelemetryPanel};
use crate::text::TextPainter;

/// Header strip height (logical pt × scale), status bar likewise.
const HEADER_PT: f32 = 52.0;
const STATUS_PT: f32 = 30.0;
const MARGIN_PT: f32 = 12.0;
const GAP_PT: f32 = 10.0;

/// AccessKit wiring. The winit adapter must exist before the window is
/// first shown; the initial tree is staged through `initial` so the
/// activation handler can hand it to the platform the moment AT attaches.
struct A11y {
    adapter: accesskit_winit::Adapter,
    tree: AccessKitAdapter,
}

struct InitialTree(Arc<Mutex<Option<TreeUpdate>>>);
impl accesskit::ActivationHandler for InitialTree {
    fn request_initial_tree(&mut self) -> Option<TreeUpdate> {
        self.0.lock().take()
    }
}

/// AT actions arrive on the platform's callback thread — queue them and
/// let the frame loop drain + decode + dispatch on the event thread.
struct QueueActions(Arc<Mutex<Vec<ActionRequest>>>);
impl accesskit::ActionHandler for QueueActions {
    fn do_action(&mut self, request: ActionRequest) {
        self.0.lock().push(request);
    }
}

struct NoopDeactivate;
impl accesskit::DeactivationHandler for NoopDeactivate {
    fn deactivate_accessibility(&mut self) {}
}

/// The application. GPU/window state is created lazily inside
/// `can_create_surfaces` per the winit 0.31 lifecycle.
struct App {
    // Built eagerly — no window needed.
    scale: Signal<f32>,
    cpu: Signal<f64>,
    mem: Signal<f64>,
    pal: Palette,
    dock: martensite::blessed::DockTree,
    /// arena is built in `can_create_surfaces` once the real scale
    /// factor is known (F18 — widgets get scale through the signal).
    arena: Option<WidgetArena>,
    root: Option<WidgetId>,
    panels: [Option<WidgetId>; 4],
    panel_names: [&'static str; 4],
    router: EventRouter,
    focus: FocusManager,
    mods: ModifiersState,
    actions: Arc<Mutex<Vec<ActionRequest>>>,
    initial_tree: Arc<Mutex<Option<TreeUpdate>>>,

    // Windowed state.
    window: Option<Arc<dyn Window>>,
    gpu: Option<GpuContext>,
    surface: Option<SurfaceWrapper<'static>>,
    orchestrator: Option<RenderOrchestrator>,
    a11y: Option<A11y>,
    chrome: TextPainter,
    recovery: RecoveryMachine,
    needs_layout: bool,
    cursor: PhysicalPosition<f64>,
    last_frame: Instant,
    started: Instant,
    frame_ms: f64,
    #[cfg(feature = "devtools")]
    hud: DiagnosticHud,
}

impl App {
    fn new(_proxy: EventLoopProxy) -> Self {
        Self {
            scale: Signal::new(1.0f32),
            cpu: Signal::new(0.42f64),
            mem: Signal::new(0.61f64),
            pal: Palette::dark(),
            dock: martensite::blessed::DockTree::with_capacity(8),
            arena: None,
            root: None,
            panels: [None, None, None, None],
            panel_names: ["Process Grid", "Telemetry", "Editor", "Media"],
            router: EventRouter::new(),
            focus: FocusManager::new(),
            mods: ModifiersState::empty(),
            actions: Arc::new(Mutex::new(Vec::new())),
            initial_tree: Arc::new(Mutex::new(None)),
            window: None,
            gpu: None,
            surface: None,
            orchestrator: None,
            a11y: None,
            chrome: TextPainter::new(),
            recovery: RecoveryMachine::new(),
            needs_layout: true,
            cursor: PhysicalPosition::new(0.0, 0.0),
            last_frame: Instant::now(),
            started: Instant::now(),
            frame_ms: 0.0,
            #[cfg(feature = "devtools")]
            hud: DiagnosticHud::new(),
        }
    }

    /// Builds the arena: a transparent root plus the four panel widgets
    /// as focusable arena children, then the BSP dock tree keyed by each
    /// child's `WidgetId` (F4 — the u64 bridge is still manual).
    fn build_arena(&mut self) {
        let mut arena = WidgetArena::new();

        // Transparent root — paints nothing itself; children get their
        // bounds from the dock tree, not Taffy (the dock BSP is the
        // panel-geometry authority this demo exists to exercise).
        // VISIBLE is required: `build_paint_list` skips any subtree
        // whose root lacks it, which would hide every panel.
        let mut root_hot = HotNode::default();
        root_hot.flags |= NodeFlags::VISIBLE;
        let root = arena.insert_with_widget(root_hot, Box::new(Container::new()));

        let mut focus = FocusManager::new();
        focus.set_root(root);

        let scale = self.scale.clone();
        let pal = self.pal;
        let mut panels: [Option<WidgetId>; 4] = [None, None, None, None];
        let widgets: [Box<dyn martensite::core::Widget>; 4] = [
            Box::new(GridPanel::new(scale.clone(), pal)),
            Box::new(TelemetryPanel::new(
                scale.clone(),
                pal,
                self.cpu.clone(),
                self.mem.clone(),
            )),
            Box::new(EditorPanel::new(scale.clone(), pal)),
            Box::new(MediaPanel::new(scale.clone(), pal)),
        ];
        for (i, widget) in widgets.into_iter().enumerate() {
            let mut hot = HotNode::default();
            hot.flags |= NodeFlags::FOCUSABLE | NodeFlags::VISIBLE;
            let mut cold = ColdNode::new(widget);
            cold.debug_name = Some(self.panel_names[i]);
            let id = arena.insert(hot, cold);
            arena.append_child(root, id).expect("append panel");
            panels[i] = Some(id);
        }

        let ids: Vec<u64> = panels.iter().map(|p| p.unwrap().to_u64()).collect();
        self.dock = build_dock_tree(&ids.try_into().expect("4 panels"));
        self.arena = Some(arena);
        self.root = Some(root);
        self.panels = panels;
        self.focus = focus;
        // Focus lands on the grid first — it is the hero panel.
        if let (Some(arena), Some(first)) = (&mut self.arena, panels[0]) {
            self.focus.try_set_focus(arena, first);
        }
    }

    /// Applies the dock tree's panel rects to the arena — the docking
    /// model is the geometry authority (no Taffy on panels; its
    /// two-pass path is exercised by the header strip and by the
    /// headless mode).
    fn apply_dock_layout(&mut self) {
        let (Some(arena), Some(window)) = (&mut self.arena, &self.window) else {
            return;
        };
        let s = self.scale.get();
        let size = window.surface_size();
        let (w, h) = (f64::from(size.width), f64::from(size.height));
        let m = f64::from(MARGIN_PT * s);
        let gap = f64::from(GAP_PT * s);
        let top = m + f64::from(HEADER_PT * s) + gap;
        let bottom = h - m - f64::from(STATUS_PT * s) - gap;

        // Root covers the window so routing always has a hit target.
        let root = self.root.expect("arena built");
        if let Some(hot) = arena.get_hot_mut(root) {
            hot.bounds = Rect::new(0.0, 0.0, w as f32, h as f32);
        }

        // blessed::Rect is (x, y, width, height) — the dock area's size
        // is `bottom - top`, not `bottom`.
        let area =
            martensite::blessed::Rect::new(m, top, (w - 2.0 * m).max(1.0), (bottom - top).max(1.0));
        let rects: Vec<_> = self.dock.panel_rects(area).collect();
        for (node_id, rect) in rects {
            let Some(martensite::blessed::DockNode::Leaf { panel }) = self.dock.node(node_id)
            else {
                continue;
            };
            let Some(wid) = WidgetId::from_u64(panel.widget_id()) else {
                continue;
            };
            // Inset each leaf by half the gutter on all sides — adjacent
            // panels then share one `gap` between them symmetrically.
            let gi = gap * 0.5;
            let r = Rect::new(
                (rect.x + gi) as f32,
                (rect.y + gi) as f32,
                (rect.width - gap).max(1.0) as f32,
                (rect.height - gap).max(1.0) as f32,
            );
            if let Some((hot, cold)) = arena.get_both_mut(wid) {
                hot.bounds = r;
                cold.widget.layout(&mut LayoutContext { hot }, r);
            }
        }
    }

    /// Paints the app-level chrome: header strip with title + live KPI
    /// chips, status bar with key hints + focus + frame stats.
    fn paint_chrome(&mut self, list: &mut PaintList, w: f64, h: f64) {
        let pal = self.pal;
        let s = self.scale.get();
        let sd = f64::from(s);
        let m = MARGIN_PT as f64 * sd;
        let uptime = fmt_uptime(self.started.elapsed().as_secs());
        let focused_name = self
            .focus
            .current_focus()
            .and_then(|id| self.panels.iter().position(|p| *p == Some(id)))
            .map(|i| self.panel_names[i])
            .unwrap_or("none");
        let text = &mut self.chrome;

        // Header.
        let header_h = HEADER_PT as f64 * sd;
        list.push_fill_rect(
            martensite::render::Rect::new(m, m, w - m, m + header_h),
            pal.raised,
        );

        // KPI chips — measured first so the title/subtitle get the real
        // remaining width instead of a fraction guess (wrap collisions).
        let kpis = [
            ("CPU", format!("{:.0}%", self.cpu.get() * 100.0), pal.accent),
            (
                "MEM",
                format!("{:.0}%", self.mem.get() * 100.0),
                pal.accent2,
            ),
            ("PROCS", fmt_count(1_000_000), pal.text),
            ("UPTIME", uptime, pal.text_muted),
        ];
        let chips_w: f64 = kpis
            .iter()
            .map(|(label, value, _)| {
                f64::from(
                    text.measure(value, 14.0 * s)
                        .max(text.measure(label, 12.0 * s)),
                ) + 20.0 * sd
                    + 8.0 * sd
            })
            .sum();
        let text_w = ((w - m - 16.0 * sd - chips_w) - (m + 16.0 * sd) - 8.0 * sd).max(1.0);
        text.push(
            list,
            Point::new(m + 16.0 * sd, m + 9.0 * sd),
            "MARTENSITE — INDUSTRIAL WORKSTATION",
            14.0 * s,
            pal.text,
            Some(text_w as f32),
        );
        text.push(
            list,
            Point::new(m + 16.0 * sd, m + 28.0 * sd),
            "windowed dogfood — dock · grid · telemetry · editor · media",
            12.0 * s,
            Palette::alpha(pal.text, 200),
            Some(text_w as f32),
        );

        let mut kx = w - m - 16.0 * sd;
        for (label, value, color) in kpis.iter().rev() {
            let vw = text.measure(value, 14.0 * s);
            let lw = text.measure(label, 12.0 * s);
            let chip_w = f64::from(vw.max(lw)) + 20.0 * sd;
            kx -= chip_w;
            list.push_fill_rect(
                martensite::render::Rect::new(
                    kx,
                    m + 8.0 * sd,
                    kx + chip_w,
                    m + header_h - 8.0 * sd,
                ),
                pal.surface,
            );
            let label_w = text.measure(label, 12.0 * s);
            text.push(
                list,
                Point::new(kx + (chip_w - f64::from(label_w)) / 2.0, m + 11.0 * sd),
                label,
                12.0 * s,
                pal.text_muted,
                None,
            );
            let value_w = text.measure(value, 14.0 * s);
            text.push(
                list,
                Point::new(kx + (chip_w - f64::from(value_w)) / 2.0, m + 26.0 * sd),
                value,
                14.0 * s,
                *color,
                None,
            );
            kx -= 8.0 * sd;
        }

        // Status bar.
        let sb_h = STATUS_PT as f64 * sd;
        let sb_y = h - m - sb_h;
        list.push_fill_rect(
            martensite::render::Rect::new(m, sb_y, w - m, sb_y + sb_h),
            pal.raised,
        );
        let hints = format!(
            "Tab focus · click sort/select · F alerts · Space pause · focus: {focused_name} · {:.1}ms",
            self.frame_ms
        );
        // Reserve room on the right for the devtools label (when built
        // with `--features devtools`) so the two never collide.
        let hints_w = (w - 2.0 * m - 24.0 * sd - 260.0 * sd).max(1.0);
        text.push(
            list,
            Point::new(m + 12.0 * sd, sb_y + 6.0 * sd),
            &hints,
            12.0 * s,
            Palette::alpha(pal.text, 200),
            Some(hints_w as f32),
        );
        #[cfg(feature = "devtools")]
        if self.hud.is_enabled() {
            let label = format!(
                "devtools · {} frames · avg {:.2}ms",
                self.hud.histogram().len(),
                self.hud.histogram().average().total_time_ns as f64 / 1e6
            );
            let lw = text.measure(&label, 12.0 * s);
            text.push(
                list,
                Point::new(w - m - 12.0 * sd - f64::from(lw), sb_y + 6.0 * sd),
                &label,
                12.0 * s,
                pal.accent,
                None,
            );
        }
    }

    /// One frame: tick widgets, paint, composite, present, feed AT.
    fn redraw(&mut self) {
        if self.window.is_none() || self.orchestrator.is_none() || self.arena.is_none() {
            return;
        }
        let now = Instant::now();
        let dt = now - self.last_frame;
        self.last_frame = now;

        // 1. Widget ticks — telemetry advances its signals, media pumps
        //    its pacing queue; dirty widgets mark repaint.
        let t0 = Instant::now();
        self.arena.as_mut().expect("checked").tick(dt);

        // 2. Layout — dock rects into arena nodes.
        if self.needs_layout {
            self.apply_dock_layout();
            self.needs_layout = false;
        }

        // 3. Record paint: window bg → widget tree → chrome overlays.
        let t1 = Instant::now();
        let size = self.window.as_ref().expect("checked").surface_size();
        let (w, h) = (f64::from(size.width), f64::from(size.height));
        let mut list = PaintList::new();
        list.push_fill_rect(martensite::render::Rect::new(0.0, 0.0, w, h), self.pal.bg);
        {
            let root = self.root.expect("checked");
            let arena = self.arena.as_ref().expect("checked");
            arena.build_paint_list(root, &mut list);
        }
        self.paint_chrome(&mut list, w, h);
        let t2 = Instant::now();

        // 4. Composite + present. The paint audit runs inside `render`.
        {
            let (Some(gpu), Some(surface), Some(orchestrator)) =
                (&self.gpu, &mut self.surface, &mut self.orchestrator)
            else {
                return;
            };
            orchestrator.render(&list, &self.recovery);
            if let Err(err) = orchestrator.render_to_surface(&gpu.device, &gpu.queue, surface) {
                self.recovery.handle_surface_error(err);
                let _ = surface.resize(&gpu.device, size.width.max(1), size.height.max(1));
            }
        }
        let t3 = Instant::now();
        self.frame_ms = (t3 - t0).as_secs_f64() * 1000.0;

        // 5. Devtools HUD — real measured timings.
        #[cfg(feature = "devtools")]
        self.hud.record_frame(FrameTiming::from_ms(
            (t1 - t0).as_secs_f64() * 1000.0,
            (t2 - t1).as_secs_f64() * 1000.0,
            (t3 - t2).as_secs_f64() * 1000.0,
            self.frame_ms,
        ));
        #[cfg(not(feature = "devtools"))]
        let _ = (t1, t2);

        // 6. Assistive tech: drain queued AT actions into the arena,
        //    then emit the tree (only when a client is attached).
        if let Some(a11y) = &mut self.a11y {
            let arena = self.arena.as_mut().expect("checked");
            for request in std::mem::take(&mut *self.actions.lock()) {
                if let Some(action) = a11y.tree.decode_action(arena, &request) {
                    dispatch_a11y_action(arena, &action);
                }
            }
            a11y.tree.set_focus(self.focus.current_focus());
            a11y.adapter
                .update_if_active(|| a11y.tree.build_update(arena));
        }

        // 7. Telemetry animates — keep the loop alive.
        self.window.as_ref().expect("checked").request_redraw();
    }
}

fn fmt_uptime(secs: u64) -> String {
    let (h, m, s) = (secs / 3600, (secs / 60) % 60, secs % 60);
    if h > 0 {
        format!("{h}h{m:02}m")
    } else {
        format!("{m}:{s:02}")
    }
}

impl ApplicationHandler for App {
    fn can_create_surfaces(&mut self, event_loop: &dyn ActiveEventLoop) {
        if self.window.is_some() {
            return;
        }

        // Invisible first — the AccessKit adapter must be created before
        // the window is shown (platform contract), then we reveal.
        let window: Arc<dyn Window> = event_loop
            .create_window(
                WindowAttributes::default()
                    .with_title("Martensite — Industrial Workstation")
                    .with_surface_size(PhysicalSize::new(1680, 980))
                    .with_visible(false),
            )
            .expect("create window")
            .into();

        self.scale.set(window.scale_factor() as f32);

        let instance = wgpu::Instance::default();
        let raw_surface = instance
            .create_surface(Arc::clone(&window))
            .expect("create surface");
        let gpu = pollster::block_on(GpuContext::for_surface(&instance, &raw_surface))
            .expect("request surface-compatible GPU context");

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

        let mut orchestrator = RenderOrchestrator::new(
            size.width.max(1),
            size.height.max(1),
            OrchestratorConfig::new(true, false),
        )
        .expect("orchestrator");
        let notify_window = Arc::clone(&window);
        orchestrator.set_pre_present_notify(Some(Box::new(move || {
            notify_window.pre_present_notify();
        })));
        orchestrator.set_audit_scale_factor(window.scale_factor());

        // Arena + first layout + initial a11y tree — all before show.
        self.build_arena();
        self.apply_dock_layout();

        let a11y = {
            let root = self.root.expect("arena built");
            let mut tree = AccessKitAdapter::new(root);
            tree.set_toolkit_name("Martensite");
            tree.set_toolkit_version(env!("CARGO_PKG_VERSION"));
            let arena = self.arena.as_mut().expect("arena built");
            tree.set_focus(self.focus.current_focus());
            *self.initial_tree.lock() = Some(tree.build_update(arena));
            A11y {
                adapter: accesskit_winit::Adapter::with_direct_handlers(
                    event_loop,
                    window.as_ref(),
                    InitialTree(Arc::clone(&self.initial_tree)),
                    QueueActions(Arc::clone(&self.actions)),
                    NoopDeactivate,
                ),
                tree,
            }
        };
        self.a11y = Some(a11y);

        self.window = Some(window);
        self.gpu = Some(gpu);
        self.surface = Some(surface);
        self.orchestrator = Some(orchestrator);

        if let Some(window) = &self.window {
            window.set_visible(true);
            window.request_redraw();
        }
    }

    fn window_event(
        &mut self,
        event_loop: &dyn ActiveEventLoop,
        _id: WindowId,
        event: WindowEvent,
    ) {
        // AT sees every event first (focus tracking, event filtering).
        if let (Some(a), Some(w)) = (&mut self.a11y, &self.window) {
            a.adapter.process_event(w.as_ref(), &event);
        }

        match event {
            WindowEvent::CloseRequested => event_loop.exit(),
            WindowEvent::SurfaceResized(size) => {
                if size.width > 0 && size.height > 0 {
                    if let (Some(surface), Some(gpu)) = (&mut self.surface, &self.gpu) {
                        let _ = surface.resize(&gpu.device, size.width, size.height);
                    }
                    if let Some(o) = &mut self.orchestrator {
                        o.set_frame_size(size.width, size.height);
                    }
                    self.needs_layout = true;
                }
            }
            WindowEvent::ScaleFactorChanged { .. } => {
                if let Some(window) = &self.window {
                    let scale = window.scale_factor();
                    self.scale.set(scale as f32);
                    if let Some(o) = &mut self.orchestrator {
                        o.set_audit_scale_factor(scale);
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
                self.cursor = position;
                if primary {
                    self.dispatch_pointer(position, PointerState::Moved, None);
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
                    winit::event::ButtonSource::Mouse(winit::event::MouseButton::Left) => {
                        Some(MButton::Left)
                    }
                    winit::event::ButtonSource::Mouse(winit::event::MouseButton::Right) => {
                        Some(MButton::Right)
                    }
                    winit::event::ButtonSource::Mouse(winit::event::MouseButton::Middle) => {
                        Some(MButton::Middle)
                    }
                    _ => None,
                };
                self.dispatch_pointer(
                    position,
                    if state == ElementState::Pressed {
                        PointerState::Pressed
                    } else {
                        PointerState::Released
                    },
                    btn,
                );
            }
            WindowEvent::MouseWheel { delta, .. } => {
                let s = self.scale.get();
                let d = match delta {
                    MouseScrollDelta::LineDelta(x, y) => Vec2::new(x * 48.0 * s, y * 48.0 * s),
                    MouseScrollDelta::PixelDelta(p) => Vec2::new(p.x as f32, p.y as f32),
                    _ => return,
                };
                if let (Some(arena), Some(window)) = (&mut self.arena, &self.window) {
                    self.router.dispatch_scroll_event(arena, window.id(), d);
                }
                self.sync_focus();
            }
            WindowEvent::KeyboardInput { event, .. } => {
                let pressed = event.state == ElementState::Pressed;
                // Tab traversal is app-level chrome — panels never see it.
                if pressed && event.logical_key == Key::Named(NamedKey::Tab) {
                    let dir = if self.mods.shift_key() {
                        TabNavigation::Reverse
                    } else {
                        TabNavigation::Forward
                    };
                    if let Some(arena) = &mut self.arena {
                        self.focus.tab(arena, dir);
                    }
                    return;
                }
                #[cfg(feature = "devtools")]
                if pressed && event.logical_key == Key::Named(NamedKey::F1) {
                    self.hud.toggle();
                    return;
                }
                let key_name = match &event.logical_key {
                    Key::Named(n) => format!("{n:?}"),
                    Key::Character(c) => c.to_string(),
                    _ => return,
                };
                // Remap winit's debug names to the framework's names.
                let key_name = match key_name.as_str() {
                    "Space" => " ".to_string(),
                    other => other.to_string(),
                };
                let focused = self.focus.current_focus();
                if let Some(arena) = &mut self.arena {
                    self.router.dispatch_keyboard_event(
                        arena,
                        focused,
                        &key_name,
                        pressed,
                        event.repeat,
                    );
                    // Committed text rides its own event.
                    if pressed && !event.repeat {
                        if let Some(text) = &event.text {
                            let t = text.to_string();
                            if !t.is_empty() {
                                if let Some(id) = focused {
                                    arena
                                        .dispatch_event(id, &WidgetEvent::ImeCommitted { text: t });
                                }
                            }
                        }
                    }
                }
                self.sync_focus();
            }
            WindowEvent::RedrawRequested => self.redraw(),
            _ => {}
        }
    }

    fn proxy_wake_up(&mut self, _event_loop: &dyn ActiveEventLoop) {
        if let Some(window) = &self.window {
            window.request_redraw();
        }
    }
}

impl App {
    fn dispatch_pointer(
        &mut self,
        pos: PhysicalPosition<f64>,
        state: PointerState,
        button: Option<MButton>,
    ) {
        let mods = {
            let mut k = ModifierKeys::empty();
            if self.mods.shift_key() {
                k |= ModifierKeys::SHIFT;
            }
            if self.mods.control_key() {
                k |= ModifierKeys::CONTROL;
            }
            k
        };
        let ev = PointerEvent {
            pointer_id: PointerId::PRIMARY,
            kind: PointerKind::Mouse,
            position: Vec2::new(pos.x as f32, pos.y as f32),
            state,
            button,
            modifiers: mods,
        };
        if let (Some(arena), Some(root), Some(window)) = (&mut self.arena, self.root, &self.window)
        {
            self.router
                .dispatch_pointer_event(arena, root, window.id(), &ev);
        }
        self.sync_focus();
    }

    /// `CaptureFocus` responses land in the router's pending slot; feed
    /// them to the FocusManager so Tab order and click focus agree.
    fn sync_focus(&mut self) {
        if let (Some(arena), Some(id)) = (&mut self.arena, self.router.take_focus_request()) {
            self.focus.try_set_focus(arena, id);
        }
    }
}

/// Runs the windowed workstation. `main` calls this unless `--headless`
/// was passed.
pub fn run() -> Result<(), Box<dyn std::error::Error>> {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env().unwrap_or_else(|_| "warn".into()),
        )
        .init();
    let event_loop = EventLoop::new()?;
    event_loop.set_control_flow(ControlFlow::Poll);
    let proxy = event_loop.create_proxy();
    event_loop.run_app(App::new(proxy))?;
    Ok(())
}
