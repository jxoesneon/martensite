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
use martensite::motion::{AnimationDriver, AnimationId};
use martensite::prelude::*;
use martensite::render::{PaintList, Point};
use martensite::theme::{ThemeDictionary, ThemeDiff, ThemeMode, ThemeToken};
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
use winit::event_loop::{ActiveEventLoop, ControlFlow, EventLoop};
use winit::keyboard::{Key, ModifiersState, NamedKey};
use winit::window::{Window, WindowAttributes};

#[cfg(feature = "devtools")]
use martensite::devtools::hud::{DiagnosticHud, FrameTiming};

use crate::model::{build_dock_tree, Palette};
use crate::panels::{fmt_count, EditorPanel, GridPanel, MediaPanel, TelemetryPanel};
use crate::text::TextPainter;
use crate::toolbar::{Toolbar, TOOLBAR_H};

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

/// The user's theme selection — `System` follows the OS appearance.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ThemeChoice {
    Dark,
    Light,
    System,
}

/// The application. GPU/window state is created lazily inside
/// `can_create_surfaces` per the winit 0.31 lifecycle.
struct App {
    // Built eagerly — no window needed.
    scale: Signal<f32>,
    cpu: Signal<f64>,
    mem: Signal<f64>,
    /// Toolbar outcome cells — shared with the `Toolbar` widget and the
    /// panels that consume them (pause/rate → Telemetry, text → Grid).
    paused: Signal<bool>,
    glow: Signal<bool>,
    tick_ms: Signal<f64>,
    /// Theme selection as a `toolbar::THEME_OPTIONS` index — the
    /// dropdown writes it, `redraw` applies it, `T` writes it back.
    theme_sel: Signal<usize>,
    /// Grid filter text — `GridPanel` folds it into its `RowFilter`.
    filter_text: Signal<String>,
    /// Clipboard payload sink — `GridPanel` publishes on a context-menu
    /// commit; `redraw` writes it to the OS clipboard (the platform
    /// backend isn't `Send`, so it stays app-side).
    clipboard_out: Signal<Option<String>>,
    /// OS clipboard backend — `None` where no native backend exists.
    clipboard: Option<Box<dyn martensite_clipboard_platform::ClipboardBackend>>,
    /// The toolbar strip's arena node (not a dock panel — a fixed band
    /// under the header).
    toolbar: Option<WidgetId>,
    pal: Palette,
    /// Theme state — the dictionary ships both themes; `choice` is the
    /// selection (System resolves through winit each frame), and the
    /// driver + fade animate the switch through `ThemeDiff` Oklab
    /// interpolation on the ordinary paint path.
    themes: ThemeDictionary,
    theme_choice: ThemeChoice,
    theme_anim: AnimationDriver,
    theme_fade: Option<(AnimationId, ThemeDiff)>,
    installed_mode: ThemeMode,
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
    last_frame: Instant,
    started: Instant,
    frame_ms: f64,
    #[cfg(feature = "devtools")]
    hud: DiagnosticHud,
}

impl App {
    fn new(initial_choice: ThemeChoice) -> Self {
        Self {
            scale: Signal::new(1.0f32),
            cpu: Signal::new(0.42f64),
            mem: Signal::new(0.61f64),
            paused: Signal::new(false),
            glow: Signal::new(true),
            tick_ms: Signal::new(100.0f64),
            theme_sel: Signal::new(Self::theme_index(initial_choice)),
            filter_text: Signal::new(String::new()),
            clipboard_out: Signal::new(None),
            clipboard: martensite_clipboard_platform::native_backend(),
            toolbar: None,
            pal: Palette::dark(),
            themes: ThemeDictionary::new(),
            theme_choice: initial_choice,
            theme_anim: AnimationDriver::new(),
            theme_fade: None,
            installed_mode: ThemeMode::Dark,
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
        // Every widget's paint resolves tokens through PaintContext::theme.
        // The initial choice installs directly (no startup fade) so a
        // `--theme light` boot lands settled on frame one.
        let mode = self.effective_mode();
        arena.set_theme(self.themes.theme(mode).clone());
        arena.set_scale_factor(self.scale.get());
        // Ambient shaped-text painter — every facade widget (toolbar
        // button labels, dropdown face/options, …) emits real glyph
        // runs through this one shared FontManager.
        arena.set_text_painter(martensite::text_paint::shared_painter());
        self.installed_mode = mode;

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

        // The toolbar strip — first child so it precedes the panels in
        // Tab order and the paint walk; its geometry is a fixed band
        // under the header, not a dock leaf.
        let scale = self.scale.clone();
        {
            let mut hot = HotNode::default();
            hot.flags |= NodeFlags::FOCUSABLE | NodeFlags::VISIBLE | NodeFlags::HIT_TEST_ENABLED;
            let mut cold = ColdNode::new(Box::new(Toolbar::new(
                scale.clone(),
                self.paused.clone(),
                self.glow.clone(),
                self.tick_ms.clone(),
                self.theme_sel.clone(),
                self.filter_text.clone(),
            )));
            cold.debug_name = Some("Toolbar");
            let id = arena.insert(hot, cold);
            arena.append_child(root, id).expect("append toolbar");
            self.toolbar = Some(id);
        }

        let mut panels: [Option<WidgetId>; 4] = [None, None, None, None];
        let widgets: [Box<dyn martensite::core::Widget>; 4] = [
            Box::new(GridPanel::new(
                scale.clone(),
                self.filter_text.clone(),
                self.clipboard_out.clone(),
            )),
            Box::new(TelemetryPanel::new(
                scale.clone(),
                self.cpu.clone(),
                self.mem.clone(),
                self.paused.clone(),
                self.glow.clone(),
                self.tick_ms.clone(),
            )),
            Box::new(EditorPanel::new(scale.clone())),
            Box::new(MediaPanel::new(scale.clone())),
        ];
        for (i, widget) in widgets.into_iter().enumerate() {
            let mut hot = HotNode::default();
            // F21 — hit-testing is opt-in: without HIT_TEST_ENABLED the
            // router reports every node Unhandled and pointer/scroll
            // input silently dies. Required for click-sort, row
            // selection, caret placement, and wheel scrolling.
            hot.flags |= NodeFlags::FOCUSABLE | NodeFlags::VISIBLE | NodeFlags::HIT_TEST_ENABLED;
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
        // `apply_focus_request` (not `try_set_focus`) so FocusGained
        // actually dispatches to the widget (F22).
        if let (Some(arena), Some(first)) = (&mut self.arena, panels[0]) {
            self.focus.apply_focus_request(arena, first);
        }
    }

    /// Applies the dock tree's panel rects to the arena — the docking
    /// model is the sole geometry authority in windowed mode (header
    /// and status strips are hand-painted chrome; Taffy's two-pass
    /// path is exercised only by `--headless`).
    fn apply_dock_layout(&mut self) {
        let (Some(arena), Some(window)) = (&mut self.arena, &self.window) else {
            return;
        };
        let s = self.scale.get();
        let size = window.surface_size();
        let (w, h) = (f64::from(size.width), f64::from(size.height));
        // Popups clamp into the real window — without this the layer's
        // default zero viewport collapses every popup to a degenerate
        // rect at the origin.
        arena
            .overlay_mut()
            .set_viewport(Rect::new(0.0, 0.0, w as f32, h as f32));
        let m = f64::from(MARGIN_PT * s);
        let gap = f64::from(GAP_PT * s);
        let top = m + f64::from(HEADER_PT * s) + gap;
        let bottom = h - m - f64::from(STATUS_PT * s) - gap;

        // Toolbar band under the header — collapses to nothing when the
        // window is too short rather than eating the dock area.
        let tb_h = f64::from(TOOLBAR_H * s).min((bottom - top).max(0.0));
        if let Some(id) = self.toolbar {
            // Rect is (x, y, width, height) — not min/max corners.
            let r = Rect::new(m as f32, top as f32, (w - 2.0 * m) as f32, tb_h as f32);
            if let Some((hot, cold)) = arena.get_both_mut(id) {
                hot.bounds = r;
                cold.widget.layout(&mut LayoutContext { hot, scale: s }, r);
            }
        }
        let top = top + tb_h + gap;

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
                cold.widget.layout(&mut LayoutContext { hot, scale: s }, r);
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
        // remaining width instead of a fraction guess. At narrow widths
        // the least important chips drop off (UPTIME, then PROCS) rather
        // than squeezing the title to zero — graceful degradation.
        let mut kpis = vec![
            ("UPTIME", uptime, pal.text_muted),
            ("PROCS", fmt_count(1_000_000), pal.text),
            (
                "MEM",
                format!("{:.0}%", self.mem.get() * 100.0),
                pal.accent2,
            ),
            ("CPU", format!("{:.0}%", self.cpu.get() * 100.0), pal.accent),
        ];
        // Least important first — pop until the title keeps ~180pt.
        let min_title_w = 180.0 * sd;
        loop {
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
            let text_w = (w - m - 16.0 * sd - chips_w) - (m + 16.0 * sd) - 8.0 * sd;
            if text_w >= min_title_w || kpis.len() <= 1 {
                break;
            }
            kpis.remove(0);
        }
        kpis.reverse(); // back to CPU-first order for layout below
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
        let title = text.fit(
            "MARTENSITE — INDUSTRIAL WORKSTATION",
            14.0 * s,
            text_w as f32,
        );
        text.push(
            list,
            Point::new(m + 16.0 * sd, m + 9.0 * sd),
            &title,
            14.0 * s,
            pal.text,
            None,
        );
        let subtitle = text.fit(
            "windowed dogfood — dock · grid · telemetry · editor · media",
            12.0 * s,
            text_w as f32,
        );
        text.push(
            list,
            Point::new(m + 16.0 * sd, m + 28.0 * sd),
            &subtitle,
            12.0 * s,
            Palette::alpha(pal.text, 200),
            None,
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
        // Reserve room on the right for the devtools label (only when
        // built with `--features devtools`) so the two never collide.
        #[cfg(feature = "devtools")]
        let reserve = 260.0 * sd;
        #[cfg(not(feature = "devtools"))]
        let reserve = 0.0;
        let hints_w = (w - 2.0 * m - 24.0 * sd - reserve).max(1.0);
        // fit, not wrap — a wrapped second line would overflow the
        // bar's fixed height and collide with nothing to clip it.
        let hints = text.fit(&hints, 12.0 * s, hints_w as f32);
        text.push(
            list,
            Point::new(m + 12.0 * sd, sb_y + 6.0 * sd),
            &hints,
            12.0 * s,
            Palette::alpha(pal.text, 200),
            None,
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

    /// The effective mode — `System` follows the OS appearance reported
    /// by winit, defaulting to dark when the platform can't say.
    fn effective_mode(&self) -> ThemeMode {
        match self.theme_choice {
            ThemeChoice::Dark => ThemeMode::Dark,
            ThemeChoice::Light => ThemeMode::Light,
            ThemeChoice::System => match self.window.as_ref().and_then(|w| w.theme()) {
                Some(winit::window::Theme::Light) => ThemeMode::Light,
                _ => ThemeMode::Dark,
            },
        }
    }

    /// The `THEME_OPTIONS` index for a choice — dropdown order.
    fn theme_index(choice: ThemeChoice) -> usize {
        match choice {
            ThemeChoice::Dark => 0,
            ThemeChoice::Light => 1,
            ThemeChoice::System => 2,
        }
    }

    /// Selects a theme choice and starts the animated fade. No-op when
    /// the choice is unchanged. Also writes the dropdown index back so
    /// `T`-driven cycles stay visible in the toolbar.
    fn set_theme_choice(&mut self, choice: ThemeChoice) {
        self.theme_sel.set_if_changed(Self::theme_index(choice));
        if choice == self.theme_choice {
            return;
        }
        self.theme_choice = choice;
        self.start_theme_fade();
    }

    /// Begins (or re-targets) a fade from the *installed* theme to the
    /// effective target — interrupting a fade mid-flight diffs from
    /// the interpolated colors on screen, not the stale endpoint.
    fn start_theme_fade(&mut self) {
        let target = self.themes.theme(self.effective_mode()).clone();
        let Some(arena) = &mut self.arena else {
            return;
        };
        let diff = ThemeDiff::from_themes(arena.theme(), &target);
        if diff.deltas().is_empty() {
            arena.set_theme(target);
            self.theme_fade = None;
            return;
        }
        // Critically damped (ζ≈1, ~250 ms settle): a smooth ease with
        // no overshoot — extrapolated Oklab past t=1 clips the gamut.
        let spring = SpringConfig::new(1.0, 220.0, 29.7).expect("valid spring");
        let id = self.theme_anim.start(spring, 0.0, 1.0);
        self.theme_fade = Some((id, diff));
    }

    /// One frame: tick widgets, paint, composite, present, feed AT.
    fn redraw(&mut self) {
        if self.window.is_none() || self.orchestrator.is_none() || self.arena.is_none() {
            return;
        }
        let now = Instant::now();
        let dt = now - self.last_frame;
        self.last_frame = now;

        // Dropdown → choice: the toolbar publishes its index; apply it.
        let sel = self.theme_sel.get();
        let wanted = match sel {
            1 => ThemeChoice::Light,
            2 => ThemeChoice::System,
            _ => ThemeChoice::Dark,
        };
        if wanted != self.theme_choice {
            self.set_theme_choice(wanted);
        }

        // Context menu → OS clipboard: the grid publishes the payload;
        // the backend write happens here, app-side.
        if let Some(payload) = self.clipboard_out.get() {
            if let Some(cb) = self.clipboard.as_mut() {
                cb.write("text/plain;charset=utf-8", payload.as_bytes());
            }
            self.clipboard_out.set(None);
        }

        // 0. Theme — advance any in-flight fade and install the
        //    resolved theme; widgets resolve it through
        //    `PaintContext::theme`, chrome through `self.pal`.
        {
            let mode = self.effective_mode();
            let target = self.themes.theme(mode).clone();
            let arena = self.arena.as_mut().expect("checked");
            if let Some((id, diff)) = self.theme_fade.take() {
                self.theme_anim.advance(dt.as_secs_f32());
                let t = self.theme_anim.position(id).unwrap_or(1.0);
                if self.theme_anim.is_settled(id) || t >= 0.999 {
                    arena.set_theme(target);
                    self.theme_anim.remove_settled();
                } else {
                    // Clone `to` so non-color tokens land at the
                    // destination; color deltas blend through Oklab.
                    let mut theme = target;
                    for (key, color) in diff.interpolate(t.clamp(0.0, 1.0)) {
                        theme.set(key, ThemeToken::Color(color));
                    }
                    arena.set_theme(theme);
                    self.theme_fade = Some((id, diff));
                }
            } else if self.installed_mode != mode {
                // An OS-level flip under `System` with no fade queued.
                arena.set_theme(target);
            }
            self.installed_mode = mode;
            self.pal = Palette::from_theme(arena.theme());
        }

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
        // App chrome sits outside the widget tree — wrap it in a manual
        // provenance scope so audit findings still name a component.
        list.push_scope(
            None,
            "App Chrome",
            martensite::render::Rect::new(0.0, 0.0, w, h),
        );
        self.paint_chrome(&mut list, w, h);
        list.pop_scope();
        // Overlay popups (dropdown menus, context menus, tooltips) are
        // appended inside `build_paint_list` — a second explicit paint
        // here double-emits every popup command (fills covering the
        // first copy's text, self-overlapping glyph runs).
        let t2 = Instant::now();

        // Report the focused widget's rect (device px) so the audit can
        // verify a painted focus indicator exists (WCAG 2.4.7).
        let focus_rect = self
            .focus
            .current_focus()
            .and_then(|id| self.arena.as_ref().and_then(|a| a.get_hot(id)))
            .map(|h| {
                martensite::render::Rect::new(
                    f64::from(h.bounds.min_x()),
                    f64::from(h.bounds.min_y()),
                    f64::from(h.bounds.max_x()),
                    f64::from(h.bounds.max_y()),
                )
            });

        // 4. Composite + present. The paint audit runs inside `render`.
        {
            let (Some(gpu), Some(surface), Some(orchestrator)) =
                (&self.gpu, &mut self.surface, &mut self.orchestrator)
            else {
                return;
            };
            orchestrator.set_audit_focus_rect(focus_rect);
            if let Some(arena) = self.arena.as_ref() {
                orchestrator.audit_target_sizes(arena);
            }
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
                    if let Some(arena) = &mut self.arena {
                        arena.set_scale_factor(scale as f32);
                    }
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
                        // `tab` returns the next candidate without
                        // dispatching — `apply_focus_request` performs
                        // the FocusLost/FocusGained transition (F22).
                        if let Some(next) = self.focus.tab(arena, dir) {
                            self.focus.apply_focus_request(arena, next);
                        }
                    }
                    return;
                }
                #[cfg(feature = "devtools")]
                if pressed && event.logical_key == Key::Named(NamedKey::F1) {
                    self.hud.toggle();
                    return;
                }
                // T cycles theme Dark → Light → System until the toolbar
                // dropdown lands; both drive `set_theme_choice`.
                if pressed
                    && !self.mods.control_key()
                    && matches!(&event.logical_key, Key::Character(c) if c.eq_ignore_ascii_case("t"))
                {
                    let next = match self.theme_choice {
                        ThemeChoice::Dark => ThemeChoice::Light,
                        ThemeChoice::Light => ThemeChoice::System,
                        ThemeChoice::System => ThemeChoice::Dark,
                    };
                    self.set_theme_choice(next);
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
            self.focus.apply_focus_request(arena, id);
        }
    }
}

/// Runs the windowed workstation. `main` calls this unless `--headless`
/// was passed. `initial_choice` selects the boot theme (the app installs
/// it directly — no startup fade — so `--theme light` lands settled).
pub fn run(initial_choice: ThemeChoice) -> Result<(), Box<dyn std::error::Error>> {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env().unwrap_or_else(|_| "warn".into()),
        )
        .init();
    let event_loop = EventLoop::new()?;
    event_loop.set_control_flow(ControlFlow::Poll);
    event_loop.run_app(App::new(initial_choice))?;
    Ok(())
}
