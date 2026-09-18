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
use martensite::blessed::{
    DockDragSession, DockDropZone, DockNode, DockPanel, NodeId, SplitDirection,
};
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
use crate::panels::{fmt_count, EditorPanel, GridPanel, MediaPanel, TelemetryPanel, TITLE_H};
use crate::statusbar::{build_l10n, StatusBar, LOCALE_CODES, STATUSBAR_W};
use crate::text::TextPainter;
use crate::toolbar::{Toolbar, TOOLBAR_H};

/// Header strip height (logical pt × scale), status bar likewise.
const HEADER_PT: f32 = 52.0;
pub(crate) const STATUS_PT: f32 = 30.0;
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

/// An in-flight title-bar drag. Created as a *candidate* on press;
/// `active` flips once the pointer leaves the dead-zone, at which
/// point `session.target_zone` drives the translucent drop preview
/// painted by `paint_chrome`. Releasing applies the rearrangement to
/// the BSP tree — over no leaf (or over the source leaf) it cancels.
struct DockDrag {
    /// Leaf the drag started on. Stale after `apply_dock_drop`'s
    /// `remove` — targets are re-resolved by widget id.
    source: NodeId,
    /// The panel being moved — inserted into the tree on drop.
    panel: DockPanel,
    /// Press position for the dead-zone test.
    start: Vec2,
    /// Latest pointer position — the ghost chip rides it.
    pos: Vec2,
    /// Framework-side drag state: floating panel + hit-tested target.
    session: DockDragSession,
    /// Past the dead-zone — a real rearrange gesture, not a click.
    active: bool,
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
    /// Fluent catalog backing the status-bar locale dropdown — the
    /// en/es/fr/de bundles compile in via `include_str!` and the
    /// chrome labels re-resolve through it on every paint.
    l10n: martensite_l10n::reactive::L10n,
    /// Locale selection as a `statusbar::LOCALE_CODES` index — the
    /// status-bar dropdown writes it, `redraw` applies it.
    locale_sel: Signal<usize>,
    /// The `LOCALE_CODES` index last applied to `l10n` — the redraw
    /// drain compares the signal against it to detect a switch.
    locale_idx: usize,
    /// Clipboard payload sink — `GridPanel` publishes on a context-menu
    /// commit; `redraw` writes it to the OS clipboard (the platform
    /// backend isn't `Send`, so it stays app-side).
    clipboard_out: Signal<Option<String>>,
    /// OS clipboard backend — `None` where no native backend exists.
    clipboard: Option<Box<dyn martensite_clipboard_platform::ClipboardBackend>>,
    /// The toolbar strip's arena node (not a dock panel — a fixed band
    /// under the header).
    toolbar: Option<WidgetId>,
    /// The status strip's arena node — a fixed band at the window
    /// bottom, positioned by `apply_dock_layout` like the toolbar.
    statusbar: Option<WidgetId>,
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
    /// Title-bar drag-to-dock gesture — `Some` from a title-band press
    /// until the primary button releases (a click that never leaves
    /// the dead-zone stays `!active` and changes nothing).
    dock_drag: Option<DockDrag>,
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
    /// `--audit-locale` — opt-in `MissingLocale` lint: flag painted
    /// strings the shipped FTL resources don't cover.
    audit_locale: bool,
    last_frame: Instant,
    started: Instant,
    frame_ms: f64,
    #[cfg(feature = "devtools")]
    hud: DiagnosticHud,
}

impl App {
    fn new(initial_choice: ThemeChoice, audit_locale: bool) -> Self {
        Self {
            scale: Signal::new(1.0f32),
            cpu: Signal::new(0.42f64),
            mem: Signal::new(0.61f64),
            paused: Signal::new(false),
            glow: Signal::new(true),
            tick_ms: Signal::new(100.0f64),
            theme_sel: Signal::new(Self::theme_index(initial_choice)),
            filter_text: Signal::new(String::new()),
            l10n: build_l10n(),
            locale_sel: Signal::new(0),
            locale_idx: 0,
            clipboard_out: Signal::new(None),
            clipboard: martensite_clipboard_platform::native_backend(),
            toolbar: None,
            statusbar: None,
            pal: Palette::dark(),
            themes: ThemeDictionary::new(),
            theme_choice: initial_choice,
            theme_anim: AnimationDriver::new(),
            theme_fade: None,
            installed_mode: ThemeMode::Dark,
            dock: martensite::blessed::DockTree::with_capacity(8),
            dock_drag: None,
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
            audit_locale,
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

        // The status strip — last child so it follows the panels in
        // Tab order and the paint walk; its geometry is a fixed band
        // at the window bottom (the strip region `dock_area`'s bottom
        // already reserves), positioned by `apply_dock_layout`.
        {
            let mut hot = HotNode::default();
            hot.flags |= NodeFlags::FOCUSABLE | NodeFlags::VISIBLE | NodeFlags::HIT_TEST_ENABLED;
            let mut cold = ColdNode::new(Box::new(StatusBar::new(
                scale.clone(),
                self.locale_sel.clone(),
            )));
            cold.debug_name = Some("StatusBar");
            let id = arena.insert(hot, cold);
            arena.append_child(root, id).expect("append statusbar");
            self.statusbar = Some(id);
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

    /// The rectangle the dock BSP subdivides — below the header +
    /// toolbar bands, above the status bar, inside the window margin.
    /// Shared by `apply_dock_layout` and every dock hit-test so the
    /// pointer and the painted panels always agree. Device px (the
    /// same space as `panel_rects` output); a degenerate 1×1 rect
    /// without a window so headless callers degrade instead of
    /// panicking.
    fn dock_area(&self) -> martensite::blessed::Rect {
        let s = self.scale.get();
        let size = self
            .window
            .as_ref()
            .map(|w| w.surface_size())
            .unwrap_or_default();
        let (w, h) = (f64::from(size.width), f64::from(size.height));
        let m = f64::from(MARGIN_PT * s);
        let gap = f64::from(GAP_PT * s);
        let top = m + f64::from(HEADER_PT * s) + gap;
        let bottom = h - m - f64::from(STATUS_PT * s) - gap;
        // Toolbar band under the header — same collapse rule as
        // `apply_dock_layout`.
        let tb_h = f64::from(TOOLBAR_H * s).min((bottom - top).max(0.0));
        let top = top + tb_h + gap;
        // blessed::Rect is (x, y, width, height) — the dock area's
        // height is `bottom - top`, not `bottom`.
        martensite::blessed::Rect::new(m, top, (w - 2.0 * m).max(1.0), (bottom - top).max(1.0))
    }

    /// The leaf whose `panel_rects` rect contains `(x, y)` — device
    /// px, same space as winit `PhysicalPosition`.
    fn leaf_at(&self, x: f64, y: f64) -> Option<(NodeId, martensite::blessed::Rect)> {
        let area = self.dock_area();
        self.dock.panel_rects(area).find(|(_, r)| r.contains(x, y))
    }

    /// Applies the dock tree's panel rects to the arena — the docking
    /// model is the sole geometry authority in windowed mode (header
    /// and status strips are hand-painted chrome; Taffy's two-pass
    /// path is exercised only by `--headless`).
    fn apply_dock_layout(&mut self) {
        // Computed before the arena borrow — `dock_area` is the shared
        // authority for the rect the BSP subdivides (the pointer
        // hit-tests use it too).
        let area = self.dock_area();
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

        // Status strip at the window bottom — `dock_area`'s `bottom`
        // already leaves it free, so the band is available for the
        // locale dropdown. Right-aligned inside the strip at full
        // strip height — the widget fills this segment itself (the
        // chrome fill stops at its left edge: chrome paints after
        // widgets and would cover the dropdown face otherwise).
        if let Some(id) = self.statusbar {
            let sb_h = f64::from(STATUS_PT * s);
            let sb_y = (h - m - sb_h).max(0.0);
            // Clamp the segment to what's left of the margin so a
            // too-narrow window degrades to a zero-width widget
            // instead of an inverted rect.
            let sb_w = f64::from(STATUSBAR_W * s).min((w - m).max(0.0));
            let r = Rect::new(
                (w - m - sb_w).max(m) as f32,
                sb_y as f32,
                sb_w as f32,
                sb_h as f32,
            );
            if let Some((hot, cold)) = arena.get_both_mut(id) {
                hot.bounds = r;
                cold.widget.layout(&mut LayoutContext { hot, scale: s }, r);
            }
        }

        // Root covers the window so routing always has a hit target.
        let root = self.root.expect("arena built");
        if let Some(hot) = arena.get_hot_mut(root) {
            hot.bounds = Rect::new(0.0, 0.0, w as f32, h as f32);
        }

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

    /// The active drag's preview: `(drop-zone rect, pointer pos,
    /// panel title)` — resolved before `paint_chrome` mutably borrows
    /// `self.chrome`.
    fn dock_drag_preview(&self) -> Option<(martensite::blessed::Rect, Vec2, String)> {
        let drag = self.dock_drag.as_ref()?;
        if !drag.active {
            return None;
        }
        let (target, zone) = drag.session.target_zone?;
        let area = self.dock_area();
        let (_, rect) = self.dock.panel_rects(area).find(|(id, _)| *id == target)?;
        Some((
            drag.session.preview_rect(rect, zone),
            drag.pos,
            drag.panel.title().to_string(),
        ))
    }

    /// Paints the app-level chrome: header strip with title + live KPI
    /// chips, status bar with key hints + focus + frame stats.
    fn paint_chrome(&mut self, list: &mut PaintList, w: f64, h: f64) {
        let pal = self.pal;
        let s = self.scale.get();
        let sd = f64::from(s);
        let m = MARGIN_PT as f64 * sd;
        let uptime = fmt_uptime(self.started.elapsed().as_secs());
        let drag_preview = self.dock_drag_preview();
        // The focus readout names whichever arena node holds focus —
        // panels by title, the two chrome strips by role, `none`
        // otherwise (e.g. the transparent root).
        let focused_id = self.focus.current_focus();
        let focused_name = focused_id
            .map(|id| {
                if let Some(i) = self.panels.iter().position(|p| *p == Some(id)) {
                    self.panel_names[i]
                } else if self.toolbar == Some(id) {
                    "toolbar"
                } else if self.statusbar == Some(id) {
                    "status bar"
                } else {
                    "none"
                }
            })
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
        // Labels resolve through Fluent so they follow the status-bar
        // locale dropdown; the English literals stay as fallbacks.
        let chip =
            |key: &str, fallback: &str| self.l10n.get(key).unwrap_or_else(|| fallback.to_string());
        let mut kpis = vec![
            (chip("kpi-uptime", "UPTIME"), uptime, pal.text_muted),
            (chip("kpi-procs", "PROCS"), fmt_count(1_000_000), pal.text),
            (
                chip("kpi-mem", "MEM"),
                format!("{:.0}%", self.mem.get() * 100.0),
                pal.accent2,
            ),
            (
                chip("kpi-cpu", "CPU"),
                format!("{:.0}%", self.cpu.get() * 100.0),
                pal.accent,
            ),
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
        let sb_y = (h - m - sb_h).max(0.0);
        // The fill stops at the StatusBar widget's left edge — chrome
        // paints after `build_paint_list`, so a full-width fill here
        // would cover the dropdown face and focus ring. The widget
        // fills its own segment with the same token.
        list.push_fill_rect(
            martensite::render::Rect::new(
                m,
                sb_y,
                (w - m - f64::from(STATUSBAR_W * s)).max(m),
                sb_y + sb_h,
            ),
            pal.raised,
        );
        // Hints + focused-widget readout resolve through Fluent so the
        // strip follows the locale dropdown live; the English literals
        // stay as fallbacks when a bundle lacks the key.
        let hints = self.l10n.get("sb-hints").unwrap_or_else(|| {
            "Tab focus · drag title to dock · click sort/select · F alerts · Space pause"
                .to_string()
        });
        let focus_label = self
            .l10n
            .get_with_args("sb-focus", &[("name", focused_name)])
            .unwrap_or_else(|| format!("focus: {focused_name}"));
        let hints = format!("{hints} · {focus_label} · {:.1}ms", self.frame_ms);
        // Reserve room on the right for the locale dropdown (always —
        // it owns the rightmost `STATUSBAR_W` of the strip) plus the
        // devtools label (only when built with `--features devtools`)
        // so the hints never collide with either.
        let dd_reserve = f64::from(STATUSBAR_W * s) + 8.0 * sd;
        #[cfg(feature = "devtools")]
        let reserve = dd_reserve + 260.0 * sd;
        #[cfg(not(feature = "devtools"))]
        let reserve = dd_reserve;
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
                // Left of the locale dropdown — the rightmost band is
                // the widget's, not the label's.
                Point::new(w - m - dd_reserve - f64::from(lw), sb_y + 6.0 * sd),
                &label,
                12.0 * s,
                pal.accent,
                None,
            );
        }

        // Dock-rearrange preview — a translucent drop-zone fill over
        // the target leaf plus a ghost chip with the dragged panel's
        // title riding the pointer. Paints last so it sits above all
        // panel chrome.
        if let Some((preview, pos, title)) = drag_preview {
            let pr = martensite::render::Rect::new(
                preview.x,
                preview.y,
                preview.x + preview.width,
                preview.y + preview.height,
            );
            list.push_fill_rect(pr, Palette::alpha(pal.accent, 50));
            list.push_stroke_rect(pr, (1.5 * s).max(1.0), pal.accent);
            let label = text.fit(&title, 12.0 * s, (160.0 * sd) as f32);
            let lw = f64::from(text.measure(&label, 12.0 * s));
            let chip_w = lw + 20.0 * sd;
            let chip_h = 22.0 * sd;
            let cx = f64::from(pos.x) + 12.0 * sd;
            let cy = f64::from(pos.y) + 10.0 * sd;
            let chip = martensite::render::Rect::new(cx, cy, cx + chip_w, cy + chip_h);
            list.push_fill_rect(chip, Palette::alpha(pal.raised, 230));
            list.push_stroke_rect(chip, s.max(1.0), pal.accent);
            text.push(
                list,
                Point::new(cx + 10.0 * sd, cy + 5.0 * sd),
                &label,
                12.0 * s,
                pal.text,
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

        // Locale dropdown → Fluent: the status bar publishes its
        // `LOCALE_CODES` index; switching re-points the catalog so the
        // next `paint_chrome` resolves the new bundle. No layout flag
        // needed — chrome text re-shapes every frame anyway.
        let idx = self.locale_sel.get().min(LOCALE_CODES.len() - 1);
        if idx != self.locale_idx {
            self.locale_idx = idx;
            self.l10n
                .set_locale(LOCALE_CODES[idx].parse().expect("valid langid"))
                .expect("LOCALE_CODES are registered bundles");
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

/// Maps a pointer position inside a target leaf's rect to a drop zone:
/// the outer quarter of each edge is a directional split, the interior
/// is a Center swap. Left/right zones win at the corners.
fn drop_zone(rect: martensite::blessed::Rect, pos: Vec2) -> DockDropZone {
    const EDGE: f64 = 0.25;
    let fx = (f64::from(pos.x) - rect.x) / rect.width.max(f64::EPSILON);
    let fy = (f64::from(pos.y) - rect.y) / rect.height.max(f64::EPSILON);
    if fx < EDGE {
        DockDropZone::Left
    } else if fx > 1.0 - EDGE {
        DockDropZone::Right
    } else if fy < EDGE {
        DockDropZone::Top
    } else if fy > 1.0 - EDGE {
        DockDropZone::Bottom
    } else {
        DockDropZone::Center
    }
}

/// Applies a completed drag to the BSP tree.
///
/// `Center`/`Tab` swap the two leaves' panels in place — no structural
/// change. Directional zones remove the source leaf (collapsing its
/// parent split) and re-split the target at 0.5. `DockTree::remove`
/// promotes the sibling into the parent's slot, so the target id can
/// go stale mid-operation — it is re-resolved by widget id afterward.
/// `split_leaf` always lands the new panel on the right/bottom, so a
/// `Left`/`Top` drop finishes by swapping the two new child leaves.
fn apply_dock_drop(
    dock: &mut martensite::blessed::DockTree,
    source: NodeId,
    target: NodeId,
    zone: DockDropZone,
    dragged: DockPanel,
) {
    // A directional drop onto the source leaf would `remove` it and
    // then fail re-resolution — silently deleting the panel. Callers
    // already exclude the source during hit-testing; this guards
    // against misuse.
    if source == target {
        return;
    }
    match zone {
        DockDropZone::Center | DockDropZone::Tab => {
            let Some(DockNode::Leaf {
                panel: target_panel,
            }) = dock.node(target)
            else {
                return;
            };
            let target_panel = target_panel.clone();
            // Validate the source leaf before either write — if the
            // first `node_mut` landed and the second couldn't, the
            // dragged panel would exist in two leaves.
            let Some(DockNode::Leaf { .. }) = dock.node(source) else {
                return;
            };
            if let Some(DockNode::Leaf { panel }) = dock.node_mut(target) {
                *panel = dragged;
            }
            if let Some(DockNode::Leaf { panel }) = dock.node_mut(source) {
                *panel = target_panel;
            }
        }
        DockDropZone::Left | DockDropZone::Right | DockDropZone::Top | DockDropZone::Bottom => {
            let Some(DockNode::Leaf {
                panel: target_panel,
            }) = dock.node(target)
            else {
                return;
            };
            let target_wid = target_panel.widget_id();
            dock.remove(source);
            // The sibling's slot moved during `remove` — re-find the
            // target leaf by its widget id rather than trusting `target`.
            let Some((target_id, _)) = dock.panels().find(|(_, p)| p.widget_id() == target_wid)
            else {
                return;
            };
            let direction = match zone {
                DockDropZone::Left | DockDropZone::Right => SplitDirection::Vertical,
                _ => SplitDirection::Horizontal,
            };
            let Ok((orig, new)) = dock.split_leaf(target_id, direction, 0.5, dragged) else {
                return;
            };
            // The dragged panel landed on the right/bottom half; for a
            // Left/Top drop swap the two children so it reads as the
            // leading half.
            if matches!(zone, DockDropZone::Left | DockDropZone::Top) {
                let (Some(DockNode::Leaf { panel: a }), Some(DockNode::Leaf { panel: b })) =
                    (dock.node(orig).cloned(), dock.node(new).cloned())
                else {
                    return;
                };
                if let Some(DockNode::Leaf { panel }) = dock.node_mut(orig) {
                    *panel = b;
                }
                if let Some(DockNode::Leaf { panel }) = dock.node_mut(new) {
                    *panel = a;
                }
            }
        }
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
        if self.audit_locale {
            orchestrator.set_audit_locale_probe(Some(crate::statusbar::build_locale_probe(
                self.filter_text.clone(),
            )));
        }

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
                        // `apply_tab` advances *and* dispatches
                        // FocusLost/FocusGained. (The raw `tab` +
                        // `apply_focus_request` pairing was a silent
                        // no-op — `tab` commits the transition before
                        // returning, so nothing ever dispatched.)
                        self.focus.apply_tab(arena, dir);
                    }
                    return;
                }
                #[cfg(feature = "devtools")]
                if pressed && event.logical_key == Key::Named(NamedKey::F1) {
                    self.hud.toggle();
                    return;
                }
                // T cycles theme Dark → Light → System until the toolbar
                // dropdown lands; both drive `set_theme_choice`. Cmd is
                // excluded alongside Ctrl — Cmd+T is browser-adjacent
                // muscle memory, not a theme request.
                if pressed
                    && !self.mods.control_key()
                    && !self.mods.meta_key()
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
                // Cmd/Ctrl+Z → "Undo", +Shift → "Redo", Ctrl+Y →
                // "Redo", plus the text-editing set A/X/C/V →
                // "SelectAll"/"Cut"/"Copy"/"Paste".
                // `WidgetEvent::KeyPressed` carries no modifier
                // state (F17), so the chords are synthesized as
                // framework key names and routed through the normal
                // dispatch — the focused widget, not the app, decides
                // whether to consume them. The original event is
                // swallowed so "z"/"y"/etc. never reach ImeCommitted.
                if pressed && (self.mods.meta_key() || self.mods.control_key()) {
                    let synthetic: Option<&'static str> = match &event.logical_key {
                        Key::Character(c) if c.eq_ignore_ascii_case("z") => {
                            Some(if self.mods.shift_key() {
                                "Redo"
                            } else {
                                "Undo"
                            })
                        }
                        Key::Character(c) if c.eq_ignore_ascii_case("y") => Some("Redo"),
                        Key::Character(c) if c.eq_ignore_ascii_case("a") => Some("SelectAll"),
                        Key::Character(c) if c.eq_ignore_ascii_case("x") => Some("Cut"),
                        Key::Character(c) if c.eq_ignore_ascii_case("c") => Some("Copy"),
                        Key::Character(c) if c.eq_ignore_ascii_case("v") => Some("Paste"),
                        _ => None,
                    };
                    if let Some(name) = synthetic {
                        let focused = self.focus.current_focus();
                        if let Some(arena) = &mut self.arena {
                            self.router.dispatch_keyboard_event(
                                arena,
                                focused,
                                name,
                                true,
                                event.repeat,
                            );
                            // Balanced release — widgets tracking
                            // press/release pairs (GridPanel's shift
                            // state) must not see a stuck key.
                            self.router
                                .dispatch_keyboard_event(arena, focused, name, false, false);
                        }
                        self.sync_focus();
                        return;
                    }
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
    /// `true` when `pos` lands inside an open overlay popup's resolved
    /// bounds — such presses belong to the popup (menu item, dropdown
    /// option), which hit-tests before window content, so they must
    /// not seed a dock-drag candidate. Mirrors `dispatch_event`'s
    /// resolve-before-hit-test so a popup opened this frame (synthetic
    /// event streams can press before the next `tick`) still counts.
    fn press_over_overlay(&mut self, pos: Vec2) -> bool {
        let Some(arena) = self.arena.as_mut() else {
            return false;
        };
        let overlay = arena.overlay_mut();
        if overlay.viewport().width() > 0.0 && overlay.viewport().height() > 0.0 {
            overlay.layout_pass();
        }
        overlay.entries().any(|e| e.bounds().contains(pos))
    }

    /// Left-press inside a leaf's title band seeds a drag candidate.
    /// The band is the top `TITLE_H·scale` of the *inset* leaf rect —
    /// the same inset `apply_dock_layout` applies, so the hit target
    /// matches the painted title bar exactly.
    fn dock_press_candidate(&self, pos: PhysicalPosition<f64>) -> Option<DockDrag> {
        let s = self.scale.get();
        let gi = f64::from(GAP_PT * s) * 0.5;
        let title_h = f64::from(TITLE_H * s);
        let (id, rect) = self.leaf_at(pos.x, pos.y)?;
        let in_band = pos.y >= rect.y + gi
            && pos.y < rect.y + gi + title_h
            && pos.x >= rect.x + gi
            && pos.x < rect.x + rect.width - gi;
        if !in_band {
            return None;
        }
        let Some(DockNode::Leaf { panel }) = self.dock.node(id) else {
            return None;
        };
        let start = Vec2::new(pos.x as f32, pos.y as f32);
        Some(DockDrag {
            source: id,
            panel: panel.clone(),
            start,
            pos: start,
            session: DockDragSession::new(panel.clone()),
            active: false,
        })
    }

    /// The `(leaf, zone)` under `pos`, excluding `source` — dropping
    /// back on the dragged leaf is a cancel, not a valid target.
    /// Shared by the Moved path (preview) and the release path so a
    /// drop is computed from the release position, not a stale cached
    /// target.
    fn dock_target_at(
        &self,
        pos: PhysicalPosition<f64>,
        source: NodeId,
    ) -> Option<(NodeId, DockDropZone)> {
        let p = Vec2::new(pos.x as f32, pos.y as f32);
        let (id, rect) = self.leaf_at(pos.x, pos.y)?;
        (id != source).then(|| (id, drop_zone(rect, p)))
    }

    /// Moves the drag forward: activates past the dead-zone, then
    /// hit-tests `pos` against the *other* leaves to maintain
    /// `session.target_zone` for the preview.
    fn update_dock_drag(&mut self, pos: PhysicalPosition<f64>) {
        if self.dock_drag.is_none() {
            return;
        }
        let p = Vec2::new(pos.x as f32, pos.y as f32);
        let source = self.dock_drag.as_ref().expect("checked").source;
        let target = self.dock_target_at(pos, source);
        let dead = 4.0 * self.scale.get();
        let drag = self.dock_drag.as_mut().expect("checked");
        drag.pos = p;
        if !drag.active && p.distance(drag.start) <= dead {
            return;
        }
        drag.active = true;
        match target {
            Some((id, zone)) => drag.session.set_target(id, zone),
            None => drag.session.clear_target(),
        }
    }

    /// Ends the gesture: an active drag re-hit-tests the *release*
    /// position (a release with no preceding move — window-edge
    /// releases, synthetic events — must not apply a stale target);
    /// anything else is a cancel. `dock_drag` clears regardless so a
    /// stale candidate never survives a release.
    fn finish_dock_drag(&mut self, pos: PhysicalPosition<f64>) {
        let Some(drag) = self.dock_drag.take() else {
            return;
        };
        if !drag.active {
            return;
        }
        let target = self.dock_target_at(pos, drag.source);
        if let Some((target, zone)) = target {
            apply_dock_drop(&mut self.dock, drag.source, target, zone, drag.panel);
            // Relayout now — `redraw` repaints every frame regardless.
            self.apply_dock_layout();
        }
    }

    fn dispatch_pointer(
        &mut self,
        pos: PhysicalPosition<f64>,
        state: PointerState,
        button: Option<MButton>,
    ) {
        // Dock-rearrange bookkeeping runs before arena dispatch: the
        // press seeds a candidate while the widget still sees the
        // event, so clicks that never leave the dead-zone behave
        // exactly as before. Presses consumed by an open overlay popup
        // (menus, dropdown lists — the overlay hit-tests first) never
        // seed a candidate.
        match state {
            PointerState::Pressed if button == Some(MButton::Left) => {
                let p = Vec2::new(pos.x as f32, pos.y as f32);
                self.dock_drag = if self.press_over_overlay(p) {
                    None
                } else {
                    self.dock_press_candidate(pos)
                };
            }
            PointerState::Moved => self.update_dock_drag(pos),
            PointerState::Released if button == Some(MButton::Left) => {
                self.finish_dock_drag(pos);
            }
            _ => {}
        }
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
/// `audit_locale` (the `--audit-locale` flag) opts the paint audit into
/// the `MissingLocale` lint — user-visible strings the shipped FTL
/// resources don't cover are reported through the same lint channel.
pub fn run(
    initial_choice: ThemeChoice,
    audit_locale: bool,
) -> Result<(), Box<dyn std::error::Error>> {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env().unwrap_or_else(|_| "warn".into()),
        )
        .init();
    let event_loop = EventLoop::new()?;
    event_loop.set_control_flow(ControlFlow::Poll);
    event_loop.run_app(App::new(initial_choice, audit_locale))?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn area() -> martensite::blessed::Rect {
        martensite::blessed::Rect::new(0.0, 0.0, 1000.0, 600.0)
    }

    /// Leaf id of the panel holding `wid` — the widget id is the only
    /// stable key across `remove`/`split` (slab indices move).
    fn leaf_id(dock: &martensite::blessed::DockTree, wid: u64) -> NodeId {
        dock.panels()
            .find(|(_, p)| p.widget_id() == wid)
            .map(|(id, _)| id)
            .expect("widget docked")
    }

    /// The `panel_rects` rect of the leaf holding `wid`.
    fn rect_of(dock: &martensite::blessed::DockTree, wid: u64) -> martensite::blessed::Rect {
        dock.panel_rects(area())
            .find(|(id, _)| {
                matches!(dock.node(*id), Some(DockNode::Leaf { panel }) if panel.widget_id() == wid)
            })
            .map(|(_, r)| r)
            .expect("widget docked")
    }

    /// The panel stored in leaf `id` (what a press would capture).
    fn panel_of(dock: &martensite::blessed::DockTree, id: NodeId) -> DockPanel {
        match dock.node(id) {
            Some(DockNode::Leaf { panel }) => panel.clone(),
            _ => panic!("expected leaf"),
        }
    }

    #[test]
    fn drop_zone_edges_and_center() {
        let r = martensite::blessed::Rect::new(0.0, 0.0, 100.0, 100.0);
        assert_eq!(drop_zone(r, Vec2::new(10.0, 50.0)), DockDropZone::Left);
        assert_eq!(drop_zone(r, Vec2::new(90.0, 50.0)), DockDropZone::Right);
        assert_eq!(drop_zone(r, Vec2::new(50.0, 10.0)), DockDropZone::Top);
        assert_eq!(drop_zone(r, Vec2::new(50.0, 90.0)), DockDropZone::Bottom);
        assert_eq!(drop_zone(r, Vec2::new(50.0, 50.0)), DockDropZone::Center);
        // Corners resolve to the left/right zone first.
        assert_eq!(drop_zone(r, Vec2::new(5.0, 5.0)), DockDropZone::Left);
    }

    #[test]
    fn dock_drop_center_swaps_panels() {
        let mut dock = build_dock_tree(&[1, 2, 3, 4]);
        let (src, dst) = (leaf_id(&dock, 1), leaf_id(&dock, 2));
        let dragged = panel_of(&dock, src);
        apply_dock_drop(&mut dock, src, dst, DockDropZone::Center, dragged);
        assert_eq!(dock.panel_count(), 4);
        // Same leaves, exchanged payloads.
        assert_eq!(panel_of(&dock, src).widget_id(), 2);
        assert_eq!(panel_of(&dock, dst).widget_id(), 1);
    }

    #[test]
    fn dock_drop_right_lands_right_half() {
        let mut dock = build_dock_tree(&[1, 2, 3, 4]);
        let (src, dst) = (leaf_id(&dock, 1), leaf_id(&dock, 2));
        let dragged = panel_of(&dock, src);
        apply_dock_drop(&mut dock, src, dst, DockDropZone::Right, dragged);
        assert_eq!(dock.panel_count(), 4);
        let (dragged, target) = (rect_of(&dock, 1), rect_of(&dock, 2));
        // Same band, dragged on the right half of the target's old rect.
        assert!((dragged.y - target.y).abs() < 1e-6);
        assert!((dragged.height - target.height).abs() < 1e-6);
        assert!(dragged.x >= target.x + target.width - 1e-6);
    }

    #[test]
    fn dock_drop_left_lands_left_half() {
        let mut dock = build_dock_tree(&[1, 2, 3, 4]);
        let (src, dst) = (leaf_id(&dock, 1), leaf_id(&dock, 2));
        let dragged = panel_of(&dock, src);
        apply_dock_drop(&mut dock, src, dst, DockDropZone::Left, dragged);
        let (dragged, target) = (rect_of(&dock, 1), rect_of(&dock, 2));
        assert!((dragged.y - target.y).abs() < 1e-6);
        assert!(dragged.x + dragged.width <= target.x + 1e-6);
    }

    #[test]
    fn dock_drop_top_stacks_above() {
        let mut dock = build_dock_tree(&[1, 2, 3, 4]);
        let (src, dst) = (leaf_id(&dock, 1), leaf_id(&dock, 2));
        let dragged = panel_of(&dock, src);
        apply_dock_drop(&mut dock, src, dst, DockDropZone::Top, dragged);
        let (dragged, target) = (rect_of(&dock, 1), rect_of(&dock, 2));
        assert!((dragged.x - target.x).abs() < 1e-6);
        assert!((dragged.width - target.width).abs() < 1e-6);
        assert!(dragged.y + dragged.height <= target.y + 1e-6);
    }

    #[test]
    fn dock_drop_bottom_stacks_below() {
        let mut dock = build_dock_tree(&[1, 2, 3, 4]);
        let (src, dst) = (leaf_id(&dock, 1), leaf_id(&dock, 2));
        let dragged = panel_of(&dock, src);
        apply_dock_drop(&mut dock, src, dst, DockDropZone::Bottom, dragged);
        let (dragged, target) = (rect_of(&dock, 1), rect_of(&dock, 2));
        assert!((dragged.x - target.x).abs() < 1e-6);
        assert!((dragged.width - target.width).abs() < 1e-6);
        assert!(dragged.y >= target.y + target.height - 1e-6);
    }

    /// A press landing inside an open popup belongs to the popup —
    /// `press_over_overlay` is the gate that keeps it from also
    /// seeding a dock-drag candidate (menu commits AND a rearrange
    /// would both fire otherwise).
    #[test]
    fn press_over_overlay_matches_popup_bounds() {
        struct Sized;
        impl martensite::core::Widget for Sized {
            fn measure(
                &mut self,
                _cx: &mut LayoutContext,
                _c: martensite::core::LayoutConstraints,
            ) -> Vec2 {
                Vec2::new(200.0, 100.0)
            }
            fn layout(&mut self, _cx: &mut LayoutContext, _b: Rect) {}
        }

        let mut app = App::new(ThemeChoice::Dark, false);
        app.build_arena();
        {
            let overlay = app.arena.as_mut().expect("arena").overlay_mut();
            overlay.set_viewport(Rect::new(0.0, 0.0, 1000.0, 600.0));
            overlay.open(
                Box::new(Sized),
                martensite::core::overlay::OverlayAnchor::Bounds(Rect::new(
                    100.0, 100.0, 200.0, 20.0,
                )),
            );
        }
        // The popup resolves below its anchor → x∈[100,300],
        // y∈[120,220]. Inside counts as an overlay press; outside
        // doesn't.
        assert!(app.press_over_overlay(Vec2::new(150.0, 150.0)));
        assert!(!app.press_over_overlay(Vec2::new(400.0, 400.0)));
    }

    /// Tab must visit every chrome element AND every panel — the
    /// `tab` + `apply_focus_request` pairing left `FocusGained`
    /// undispatched, so focus *visually* never left the chin bar
    /// even though `current_focus` advanced.
    #[test]
    fn tab_traversal_visits_panels_and_chrome() {
        let mut app = App::new(ThemeChoice::Dark, false);
        app.build_arena();
        let arena = app.arena.as_mut().expect("arena");
        let mut visited = std::collections::HashSet::new();
        // One full cycle — six FOCUSABLE nodes: toolbar, four panels,
        // status bar.
        for _ in 0..6 {
            if let Some(id) = app.focus.apply_tab(arena, TabNavigation::Forward) {
                visited.insert(id.to_u64());
            }
        }
        for p in app.panels.iter().flatten() {
            assert!(
                visited.contains(&p.to_u64()),
                "panel never received Tab focus"
            );
        }
        assert!(visited.contains(&app.toolbar.expect("toolbar").to_u64()));
        assert!(visited.contains(&app.statusbar.expect("statusbar").to_u64()));
        assert_eq!(visited.len(), 6);
    }

    #[test]
    fn dock_drop_onto_sibling_re_resolves_target() {
        // Editor (3) and Media (4) are siblings — removing 3 promotes
        // 4 into the parent slot, invalidating its NodeId mid-drop.
        let mut dock = build_dock_tree(&[1, 2, 3, 4]);
        let (src, dst) = (leaf_id(&dock, 3), leaf_id(&dock, 4));
        let dragged = panel_of(&dock, src);
        apply_dock_drop(&mut dock, src, dst, DockDropZone::Right, dragged);
        assert_eq!(dock.panel_count(), 4);
        let (dragged, target) = (rect_of(&dock, 3), rect_of(&dock, 4));
        assert!((dragged.y - target.y).abs() < 1e-6);
        assert!(dragged.x >= target.x + target.width - 1e-6);
    }

    /// The semantic tree VoiceOver consumes: panels expose real roles
    /// and live labels, chrome is named, and the editor publishes its
    /// buffer as its value — the content path, headlessly.
    #[test]
    fn a11y_tree_exposes_roles_labels_and_values() {
        let mut app = App::new(ThemeChoice::Dark, false);
        app.build_arena();
        let arena = app.arena.as_mut().expect("arena");
        let mut adapter = AccessKitAdapter::new(app.root.expect("root"));
        let update = adapter.build_update(arena);

        let roles: Vec<accesskit::Role> = update.nodes.iter().map(|(_, n)| n.role()).collect();
        for want in [
            accesskit::Role::Table,              // Process Grid
            accesskit::Role::Image,              // Telemetry
            accesskit::Role::MultilineTextInput, // Editor
        ] {
            assert!(roles.contains(&want), "missing role {want:?}");
        }

        let labels: Vec<&str> = update.nodes.iter().filter_map(|(_, n)| n.label()).collect();
        for want in ["Process Grid", "Telemetry", "Editor"] {
            assert!(
                labels.iter().any(|l| l.contains(want)),
                "missing label {want}"
            );
        }
        assert!(
            update
                .nodes
                .iter()
                .any(|(_, n)| n.value().is_some_and(|v| !v.is_empty())),
            "editor publishes no value"
        );
    }
}
