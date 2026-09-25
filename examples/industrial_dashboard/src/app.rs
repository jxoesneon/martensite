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
use std::time::{Duration, Instant};

use accesskit::{ActionRequest, TreeUpdate};
use parking_lot::Mutex;

use glam::Vec2;
use martensite::access::actions::dispatch_a11y_action;
use martensite::access::adapter::AccessKitAdapter;
use martensite::blessed::{
    DockDragSession, DockDropZone, DockNode, DockPanel, NodeId, SplitDirection,
};
use martensite::core::shape::Shape;
use martensite::core::{
    ColdNode, HotNode, LayoutContext, NodeFlags, Rect, TextStyle, WidgetArena, WidgetEvent,
    WidgetId,
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
    ime_event_for_winit, EventRouter, ModifierKeys, MouseButton as MButton, PointerEvent,
    PointerId, PointerKind, PointerState,
};
use martensite::window::WindowId;

use martensite_dialog::{FileDialogRequest, FileFilter, PlatformDialog};
use martensite_notify::{Notification, PlatformNotifier, Urgency};
use martensite_persist::{
    default_store_path, JsonFileStore, MemoryStore, PersistError, StateStore,
};
use martensite_print::{PlatformPrinter, PrintJob, PrintOutcome};
use martensite_share::{PlatformShare, ShareOutcome, ShareRequest};
use serde_json::Value;

use winit::application::ApplicationHandler;
use winit::dpi::{LogicalSize, PhysicalPosition};
use winit::event::{ElementState, MouseScrollDelta, WindowEvent};
use winit::event_loop::{ActiveEventLoop, ControlFlow, EventLoop};
use winit::keyboard::{Key, ModifiersState, NamedKey};
use winit::window::{Window, WindowAttributes};

#[cfg(feature = "devtools")]
use martensite::devtools::hud::{DiagnosticHud, FrameTiming};

use crate::model::{build_dock_tree, Palette};
use crate::overlays::{push_toast, ShellOverlays, ToastInbox};
use crate::panels::{
    fmt_count, EditorPanel, EditorSignals, GridPanel, MediaPanel, TelemetryPanel, TITLE_H,
};
use crate::statusbar::{build_l10n, StatusBar, LOCALE_CODES, STATUSBAR_W};
use crate::subwindow::SubWindow;
use crate::text::{TextPainter, TITLE_WEIGHT};
use crate::toolbar::{Toolbar, THEME_OPTIONS, TOOLBAR_H};

/// Header strip height (logical pt × scale), status bar likewise.
/// The header is sized for the B1 type scale: a 24 pt display-tier
/// KPI numeral under a caption label needs ~64 pt, not the old 52.
const HEADER_PT: f32 = 64.0;
pub(crate) const STATUS_PT: f32 = 30.0;
const MARGIN_PT: f32 = 12.0;
const GAP_PT: f32 = 12.0;

/// Title-tier chrome style (spec B1): semibold + 0.03 em tracking —
/// the header wordmark's declared style. `TITLE_WEIGHT` is the
/// dashboard's shared semibold constant (`FontWeight::SEMIBOLD`).
const TITLE_STYLE: TextStyle = TextStyle::REGULAR.weight(TITLE_WEIGHT).letter_spacing(0.03);
/// Display-tier numeral style (spec B1): 24 pt semibold — KPI chips.
const DISPLAY_STYLE: TextStyle = TextStyle::REGULAR.weight(TITLE_WEIGHT);

/// Min interval between full a11y tree emissions. `build_update` emits
/// the entire tree — thousands of nodes — so running it per frame at
/// 50+ Hz saturates both the app and System Events. 100 ms (10 Hz) is
/// well under any AT client's polling cadence, while `a11y_force`
/// (AT action dispatched or focus moved) still emits immediately so
/// interactive latency stays one frame.
const A11Y_EMIT_INTERVAL: Duration = Duration::from_millis(100);

/// Animation frame period — the dashboard beats at 30 Hz. Industrial
/// telemetry doesn't need 60 fps, and a paced loop leaves idle gaps on
/// the main thread for accessibility queries and input to be serviced
/// (an unpaced redraw loop starved AX reads to >1 s per attribute).
/// Input events set `frame_due` so interaction latency stays one event,
/// not one interval.
const FRAME_INTERVAL: Duration = Duration::from_millis(33);

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

/// Hysteresis-gated telemetry alarm watch — each channel notifies
/// once on entering its band and re-arms when it clears, so a noisy
/// signal cannot spam the OS notification facility. The toolbar
/// "alerts" switch is the master gate (the same cell the panel
/// banner reads).
#[derive(Default)]
struct AlarmWatch {
    /// Armed/disarmed edge — the arming transition itself notifies
    /// once so the backend's liveness is visible immediately.
    armed: bool,
    cpu: bool,
    mem: bool,
}

/// The preference set mirrored into `martensite-persist` — written
/// back (set + atomic flush) whenever a frame observes a change.
#[derive(Clone, PartialEq)]
struct Prefs {
    theme: ThemeChoice,
    locale: usize,
    editor_tab: usize,
    alerts: bool,
}

/// The application. GPU/window state is created lazily inside
/// `can_create_surfaces` per the winit 0.31 lifecycle.
pub(crate) struct App {
    // Built eagerly — no window needed.
    pub(crate) scale: Signal<f32>,
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
    /// Toolbar "alerts" switch ↔ `TelemetryPanel` banner.
    alerts_on: Signal<bool>,
    /// Toolbar "⌘ Commands" → navigate to Editor ▸ CHROME.
    commands_req: Signal<bool>,
    /// Toolbar "Alerts" → navigate to Media ▸ COMMS announcements.
    bell_req: Signal<bool>,
    /// Toolbar "About…" → `ShellOverlays` modal dialog.
    about_req: Signal<bool>,
    /// Toolbar "Inspector" → `ShellOverlays` edge drawer.
    inspector_req: Signal<bool>,
    /// Toolbar "Console" → opens the secondary OS window.
    console_req: Signal<bool>,
    /// Toolbar "Share" → dispatch the telemetry report through the OS
    /// share service.
    share_req: Signal<bool>,
    /// Toolbar "Print" → submit the same report to the OS spooler.
    print_req: Signal<bool>,
    /// Editor "open…" chip → OS open-file dialog request.
    open_req: Signal<bool>,
    /// Editor "save as…" chip → OS save-file dialog request.
    export_req: Signal<bool>,
    /// App → `EditorPanel`: document picked in the open dialog.
    open_in: Signal<Option<(String, String)>>,
    /// `EditorPanel` → app: the active tab's `(name, buffer)` — the
    /// save dialog's payload.
    doc_out: Signal<Option<(String, String)>>,
    /// `EditorPanel`'s active tab — persisted as a preference.
    editor_tab: Signal<usize>,
    /// Shared toast inbox — cloned into `ShellOverlays`; `redraw`
    /// enqueues toasts for real app actions (copies, theme changes).
    toast_inbox: ToastInbox,
    /// OS clipboard backend — `None` where no native backend exists.
    clipboard: Option<Box<dyn martensite_clipboard_platform::ClipboardBackend>>,
    /// Settings persistence — a `JsonFileStore` under the per-OS
    /// config dir, a volatile `MemoryStore` where none resolves.
    store: PrefStore,
    /// OS notification facility — a safe stub where no backend exists.
    notifier: Box<dyn PlatformNotifier>,
    /// OS file dialogs — a stub reporting `Cancelled` where no backend
    /// exists. Calls block on the event thread (native modals).
    dialogs: Box<dyn PlatformDialog>,
    /// OS share/reveal — a stub reporting `Unsupported` where no
    /// backend exists.
    share: Box<dyn PlatformShare>,
    /// OS print spooler — a stub where no backend exists.
    printer: Box<dyn PlatformPrinter>,
    /// Telemetry alarm watch → OS notifications on transitions only.
    alarm: AlarmWatch,
    /// The simulated-plant backend every zone widget binds to —
    /// seeded once, driven by the frame loop (`push_history`,
    /// `tick_acoustic`, `tick_minute`), read/written by `Bound`s.
    pub(crate) model: crate::domain::PlantModel,
    /// The last preference set written through — change detection so
    /// the store only flushes on real edits.
    saved_prefs: Prefs,
    /// `true` when `--theme` was passed on the CLI — the override is
    /// session-scoped and must not be written back to the store.
    theme_override: bool,
    /// Dedup flag for persist-failure toasts — one per failure streak,
    /// re-armed on the next successful flush.
    persist_error_shown: bool,
    /// The toolbar strip's arena node (not a dock panel — a fixed band
    /// under the header).
    toolbar: Option<WidgetId>,
    /// The status strip's arena node — a fixed band at the window
    /// bottom, positioned by `apply_dock_layout` like the toolbar.
    statusbar: Option<WidgetId>,
    /// The overlay owner's arena node — zero-bounds widget whose
    /// `sync_overlay` reconciles the dialog, drawer, and toast strip.
    shell_overlays: Option<WidgetId>,
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
    pub(crate) arena: Option<WidgetArena>,
    pub(crate) root: Option<WidgetId>,
    panels: [Option<WidgetId>; 4],
    panel_names: [&'static str; 4],
    router: EventRouter,
    focus: FocusManager,
    mods: ModifiersState,
    actions: Arc<Mutex<Vec<ActionRequest>>>,
    initial_tree: Arc<Mutex<Option<TreeUpdate>>>,

    // Windowed state.
    window: Option<Arc<dyn Window>>,
    /// The secondary "Console" window — real OS surface sharing `gpu`.
    subwindow: Option<SubWindow>,
    gpu: Option<GpuContext>,
    surface: Option<SurfaceWrapper<'static>>,
    orchestrator: Option<RenderOrchestrator>,
    a11y: Option<A11y>,
    /// Last time the a11y tree update was emitted — throttles the
    /// full-tree rebuild to `A11Y_EMIT_INTERVAL`.
    a11y_last_emit: Instant,
    /// Set when an AT action ran or focus moved — forces an emit on the
    /// next frame regardless of the throttle.
    a11y_force: bool,
    /// The focus value emitted in the last update — change detection.
    a11y_last_focus: Option<WidgetId>,
    /// Deadline for the next animation frame — `about_to_wait` sleeps
    /// the event loop until then (`FRAME_INTERVAL` pacing).
    next_frame: Instant,
    /// Set by any window event other than `RedrawRequested` — input
    /// gets a frame on the next loop iteration instead of waiting out
    /// the animation interval.
    frame_due: bool,
    chrome: TextPainter,
    recovery: RecoveryMachine,
    needs_layout: bool,
    /// `--audit-locale` — opt-in `MissingLocale` lint: flag painted
    /// strings the shipped FTL resources don't cover.
    audit_locale: bool,
    last_frame: Instant,
    started: Instant,
    frame_ms: f64,
    /// Accumulators driving the plant model: telemetry history samples
    /// at 4 Hz, the shift clock ticks one minute per real second.
    hist_acc: f64,
    minute_acc: f64,
    #[cfg(feature = "devtools")]
    hud: DiagnosticHud,
}

impl App {
    pub(crate) fn new(flag_choice: Option<ThemeChoice>, audit_locale: bool) -> Self {
        // Restore persisted preferences — an explicit `--theme` flag
        // wins over the store, which wins over the Dark default.
        let store = open_store();
        let choice = flag_choice
            .or_else(|| stored_theme(&store))
            .unwrap_or(ThemeChoice::Dark);
        let locale = stored_locale(&store);
        let editor_tab = stored_tab(&store);
        let alerts = stored_alerts(&store);
        // Shared signals — the PlantModel binds to these exact cells
        // so toolbar/panel writes are visible to every Bound widget.
        let cpu = Signal::new(0.42f64);
        let mem = Signal::new(0.61f64);
        let paused = Signal::new(false);
        let alerts_sig = Signal::new(alerts);
        let filter_text = Signal::new(String::new());
        let toast_inbox: ToastInbox = std::sync::Arc::new(std::sync::Mutex::new(Vec::new()));
        let mut model = crate::domain::PlantModel::seeded(
            cpu.clone(),
            mem.clone(),
            paused.clone(),
            alerts_sig.clone(),
            filter_text.clone(),
        );
        // The shell inbox IS the model's inbox — zones enqueue, the
        // ShellOverlays owner drains into the viewport ToastHost.
        model.toast_inbox = toast_inbox.clone();
        Self {
            scale: Signal::new(1.0f32),
            cpu: cpu.clone(),
            mem: mem.clone(),
            paused: paused.clone(),
            glow: Signal::new(true),
            tick_ms: Signal::new(100.0f64),
            theme_sel: Signal::new(Self::theme_index(choice)),
            filter_text: filter_text.clone(),
            l10n: build_l10n(),
            locale_sel: Signal::new(locale),
            // `locale_idx` tracks the applied locale, not the request —
            // a nonzero stored index applies through the first redraw.
            locale_idx: 0,
            clipboard_out: Signal::new(None),
            alerts_on: alerts_sig.clone(),
            commands_req: Signal::new(false),
            bell_req: Signal::new(false),
            about_req: Signal::new(false),
            inspector_req: Signal::new(false),
            console_req: Signal::new(false),
            share_req: Signal::new(false),
            print_req: Signal::new(false),
            open_req: Signal::new(false),
            export_req: Signal::new(false),
            open_in: Signal::new(None),
            doc_out: Signal::new(None),
            editor_tab: Signal::new(editor_tab),
            toast_inbox,
            clipboard: martensite_clipboard_platform::native_backend(),
            notifier: martensite_notify::default_platform_notifier(),
            dialogs: martensite_dialog::default_platform_dialog(),
            share: martensite_share::default_platform_share(),
            printer: martensite_print::default_platform_printer(),
            alarm: AlarmWatch::default(),
            model,
            saved_prefs: Prefs {
                theme: choice,
                locale,
                editor_tab,
                alerts,
            },
            theme_override: flag_choice.is_some(),
            persist_error_shown: false,
            store,
            toolbar: None,
            statusbar: None,
            shell_overlays: None,
            pal: Palette::dark(),
            themes: ThemeDictionary::new(),
            theme_choice: choice,
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
            subwindow: None,
            gpu: None,
            surface: None,
            orchestrator: None,
            a11y: None,
            a11y_last_emit: Instant::now(),
            a11y_force: true,
            a11y_last_focus: None,
            next_frame: Instant::now(),
            frame_due: true,
            chrome: TextPainter::new(),
            recovery: RecoveryMachine::new(),
            needs_layout: true,
            audit_locale,
            last_frame: Instant::now(),
            started: Instant::now(),
            frame_ms: 0.0,
            hist_acc: 0.0,
            minute_acc: 0.0,
            #[cfg(feature = "devtools")]
            hud: DiagnosticHud::new(),
        }
    }

    /// Builds the arena: a transparent root plus the four panel widgets
    /// as focusable arena children, then the BSP dock tree keyed by each
    /// child's `WidgetId` (F4 — the u64 bridge is still manual).
    pub(crate) fn build_arena(&mut self) {
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
                crate::toolbar::ToolbarSignals {
                    paused: self.paused.clone(),
                    glow_on: self.glow.clone(),
                    tick_ms: self.tick_ms.clone(),
                    theme_sel: self.theme_sel.clone(),
                    filter_text: self.filter_text.clone(),
                    alerts_on: self.alerts_on.clone(),
                    commands_req: self.commands_req.clone(),
                    bell_req: self.bell_req.clone(),
                    about_req: self.about_req.clone(),
                    inspector_req: self.inspector_req.clone(),
                    console_req: self.console_req.clone(),
                    share_req: self.share_req.clone(),
                    print_req: self.print_req.clone(),
                },
            )));
            cold.debug_name = Some("Toolbar");
            let id = arena.insert(hot, cold);
            arena.append_child(root, id).expect("append toolbar");
            self.toolbar = Some(id);
        }

        let mut panels: [Option<WidgetId>; 4] = [None, None, None, None];
        // Each operational panel is wrapped in a ZonePanel: the
        // operational view on top, domain-named zone pages below —
        // every zone widget is Bound to the shared PlantModel.
        let widgets: [Box<dyn martensite::core::Widget>; 4] = [
            Box::new(crate::zones::ZonePanel::new(
                Box::new(GridPanel::new(
                    scale.clone(),
                    self.filter_text.clone(),
                    self.clipboard_out.clone(),
                )),
                "Process Grid",
                scale.clone(),
                &self.model,
                0,
                crate::zones::grid::pages(&self.model),
            )),
            Box::new(crate::zones::ZonePanel::new(
                Box::new(TelemetryPanel::new(
                    scale.clone(),
                    self.cpu.clone(),
                    self.mem.clone(),
                    self.paused.clone(),
                    self.glow.clone(),
                    self.tick_ms.clone(),
                    self.alerts_on.clone(),
                )),
                "Telemetry",
                scale.clone(),
                &self.model,
                1,
                crate::zones::telemetry::pages(&self.model),
            )),
            Box::new(crate::zones::ZonePanel::new(
                Box::new(EditorPanel::new(
                    scale.clone(),
                    EditorSignals {
                        tab_sel: self.editor_tab.clone(),
                        open_in: self.open_in.clone(),
                        doc_out: self.doc_out.clone(),
                        open_req: self.open_req.clone(),
                        export_req: self.export_req.clone(),
                    },
                )),
                "Editor",
                scale.clone(),
                &self.model,
                2,
                crate::zones::editor::pages(&self.model),
            )),
            Box::new(crate::zones::ZonePanel::new(
                Box::new(MediaPanel::new(scale.clone())),
                "Media",
                scale.clone(),
                &self.model,
                3,
                crate::zones::media::pages(&self.model),
            )),
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
                self.cpu.clone(),
                self.paused.clone(),
            )));
            cold.debug_name = Some("StatusBar");
            let id = arena.insert(hot, cold);
            arena.append_child(root, id).expect("append statusbar");
            self.statusbar = Some(id);
        }

        // The overlay owner — an invisible zero-bounds widget whose
        // `sync_overlay` reconciles the About dialog, inspector drawer,
        // and toast strip against the layer. VIRTUAL (no HIT_TEST) so
        // it never intercepts input itself.
        {
            let mut hot = HotNode::default();
            hot.flags |= NodeFlags::VISIBLE;
            let mut cold = ColdNode::new(Box::new(ShellOverlays::new(
                self.about_req.clone(),
                self.inspector_req.clone(),
                self.alerts_on.clone(),
                self.toast_inbox.clone(),
            )));
            cold.debug_name = Some("ShellOverlays");
            let id = arena.insert(hot, cold);
            arena.append_child(root, id).expect("append overlays");
            self.shell_overlays = Some(id);
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
        let size = self
            .window
            .as_ref()
            .map(|w| w.surface_size())
            .unwrap_or_default();
        self.dock_area_at(size.width, size.height)
    }

    /// `dock_area` at an explicit surface size — see
    /// [`Self::apply_dock_layout_at`].
    fn dock_area_at(&self, width: u32, height: u32) -> martensite::blessed::Rect {
        let s = self.scale.get();
        let (w, h) = (f64::from(width), f64::from(height));
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
        let Some(window) = &self.window else {
            return;
        };
        let size = window.surface_size();
        self.apply_dock_layout_at(size.width, size.height);
    }

    /// `apply_dock_layout` with an explicit surface size — the real
    /// path derives `w`/`h` from the window; tests drive this directly
    /// to exercise geometry without a display server.
    pub(crate) fn apply_dock_layout_at(&mut self, width: u32, height: u32) {
        // Computed before the arena borrow — `dock_area` is the shared
        // authority for the rect the BSP subdivides (the pointer
        // hit-tests use it too).
        let area = self.dock_area_at(width, height);
        let Some(arena) = &mut self.arena else {
            return;
        };
        let s = self.scale.get();
        let (w, h) = (f64::from(width), f64::from(height));
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

        // The dock's manual path assigns bounds outside the layout
        // engine — re-evaluate underflow engagement for every node
        // (engage/release hysteresis lives inside), then relocate
        // focus if the focused node just became covered.
        arena.update_underflow_all();
        self.focus.revalidate(arena);
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
        list.push_fill_rect(kurbo::Rect::new(m, m, w - m, m + header_h), pal.raised);

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
            // Plant status objects — last in the vec so they're the
            // last chips dropped under width pressure (they're the
            // most operationally important).
            (
                chip("kpi-line", "LINE"),
                if self.model.line_running.get() {
                    chip("kpi-line-run", "RUN")
                } else {
                    chip("kpi-line-held", "HELD")
                },
                if self.model.line_running.get() {
                    pal.ok
                } else {
                    pal.warn
                },
            ),
            (
                chip("kpi-alarms", "ALARMS"),
                format!("{}", self.model.active_alarms().len()),
                if self.model.active_alarms().is_empty() {
                    pal.text_muted
                } else {
                    pal.error
                },
            ),
        ];
        // Least important first — pop until the title keeps ~180pt.
        let min_title_w = 180.0 * sd;
        loop {
            let chips_w: f64 = kpis
                .iter()
                .map(|(label, value, _)| {
                    f64::from(
                        text.measure_styled(value, 24.0 * s, DISPLAY_STYLE)
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
                    text.measure_styled(value, 24.0 * s, DISPLAY_STYLE)
                        .max(text.measure(label, 12.0 * s)),
                ) + 20.0 * sd
                    + 8.0 * sd
            })
            .sum();
        let text_w = ((w - m - 16.0 * sd - chips_w) - (m + 16.0 * sd) - 8.0 * sd).max(1.0);
        // Title-tier wordmark (spec B1 role→tier): 15 pt semibold with
        // the tier's +0.03 em tracking — `fit_styled`/`push_styled` must
        // shape with the same style or the ellipsis cut is measured
        // against the wrong widths.
        let title = text.fit_styled(
            "MARTENSITE — INDUSTRIAL WORKSTATION",
            15.0 * s,
            text_w as f32,
            TITLE_STYLE,
        );
        text.push_styled(
            list,
            Point::new(m + 16.0 * sd, m + 13.0 * sd),
            &title,
            15.0 * s,
            pal.text,
            None,
            TITLE_STYLE,
        );
        // Subtitle is secondary metadata — caption tier on the muted
        // token (spec B1: `text_muted` carries units/axis/secondary).
        let subtitle = text.fit(
            "windowed dogfood — dock · grid · telemetry · editor · media",
            12.0 * s,
            text_w as f32,
        );
        text.push(
            list,
            Point::new(m + 16.0 * sd, m + 36.0 * sd),
            &subtitle,
            12.0 * s,
            pal.text_muted,
            None,
        );

        // KPI chips — caption-tier label (12 pt muted) stacked over a
        // display-tier numeral (24 pt semibold, spec B1). The header
        // grew to 64 pt so the pair keeps its 8 pt chip inset.
        let mut kx = w - m - 16.0 * sd;
        for (label, value, color) in kpis.iter().rev() {
            let vw = text.measure_styled(value, 24.0 * s, DISPLAY_STYLE);
            let lw = text.measure(label, 12.0 * s);
            let chip_w = f64::from(vw.max(lw)) + 20.0 * sd;
            kx -= chip_w;
            list.push_fill_rect(
                kurbo::Rect::new(kx, m + 8.0 * sd, kx + chip_w, m + header_h - 8.0 * sd),
                pal.surface,
            );
            let label_w = text.measure(label, 12.0 * s);
            text.push(
                list,
                Point::new(kx + (chip_w - f64::from(label_w)) / 2.0, m + 10.0 * sd),
                label,
                12.0 * s,
                pal.text_muted,
                None,
            );
            let value_w = text.measure_styled(value, 24.0 * s, DISPLAY_STYLE);
            text.push_styled(
                list,
                Point::new(kx + (chip_w - f64::from(value_w)) / 2.0, m + 27.0 * sd),
                value,
                24.0 * s,
                *color,
                None,
                DISPLAY_STYLE,
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
            kurbo::Rect::new(
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
        // Caption-tier hints on the muted token — key hints, the focus
        // readout, and frame timing are secondary metadata (spec B1).
        let hints = text.fit(&hints, 12.0 * s, hints_w as f32);
        text.push(
            list,
            Point::new(m + 12.0 * sd, sb_y + 6.0 * sd),
            &hints,
            12.0 * s,
            pal.text_muted,
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
            let pr = kurbo::Rect::new(
                preview.x,
                preview.y,
                preview.x + preview.width,
                preview.y + preview.height,
            );
            let target = Shape::rounded(4.0 * s);
            list.push_fill_shape(pr, &target, Palette::alpha(pal.accent, 50));
            list.push_stroke_shape(pr, &target, (1.5 * s).max(1.0), pal.accent);
            let label = text.fit(&title, 12.0 * s, (160.0 * sd) as f32);
            let lw = f64::from(text.measure(&label, 12.0 * s));
            let chip_w = lw + 20.0 * sd;
            let chip_h = 22.0 * sd;
            let cx = f64::from(pos.x) + 12.0 * sd;
            let cy = f64::from(pos.y) + 10.0 * sd;
            let chip = kurbo::Rect::new(cx, cy, cx + chip_w, cy + chip_h);
            list.push_fill_shape(
                chip,
                &Shape::squircle((chip_h * 0.4) as f32),
                Palette::alpha(pal.raised, 230),
            );
            list.push_stroke_shape(
                chip,
                &Shape::squircle((chip_h * 0.4) as f32),
                s.max(1.0),
                pal.accent,
            );
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

    /// The report artifact the Share/Print actions dispatch — built
    /// from app-owned state so the flow needs no widget internals.
    fn report_text(&self) -> String {
        format!(
            "Industrial Workstation report\n\
             uptime {}\ncpu_load {:.1}%\nmem_pressure {:.1}%\n\
             paused {}\nalerts armed {}\nlocale {}\ntheme {:?}\nframe {:.2} ms\n",
            fmt_uptime(self.started.elapsed().as_secs()),
            self.cpu.get() * 100.0,
            self.mem.get() * 100.0,
            self.paused.get(),
            self.alerts_on.get(),
            LOCALE_CODES[self.locale_idx.min(LOCALE_CODES.len() - 1)],
            self.theme_choice,
            self.frame_ms,
        )
    }

    /// Drains the service request signals the toolbar and the editor
    /// chips publish — OS file dialogs, share dispatch, and print
    /// submission all run here, app-side (widgets stay OS-free). The
    /// dialog backend's `show` blocks on this thread; that's the
    /// native-modal contract, same as `DialogService` documents.
    fn drain_service_requests(&mut self) {
        // Toolbar chrome launchers → zone navigation. Zone/page
        // indices follow `zones::mod` (0=grid, 1=telemetry, 2=editor,
        // 3=media) and each zone's `pages()` order.
        if self.commands_req.get() {
            self.commands_req.set(false);
            self.model.request_page(2, 3); // Editor ▸ CHROME
        }
        if self.bell_req.get() {
            self.bell_req.set(false);
            self.model.request_page(3, 0); // Media ▸ COMMS
        }
        // Editor "open…" chip → open-file dialog → `open_in` (the
        // panel drains it into a new tab on its next tick).
        if self.open_req.get() {
            self.open_req.set(false);
            // A stub backend silently reports `Cancelled` — tell the
            // user instead of letting the click die invisibly.
            if self.dialogs.platform_name() == "stub" {
                push_toast(
                    &self.toast_inbox,
                    martensite::widgets::Severity::Warning,
                    "File dialogs unavailable on this platform",
                );
            } else {
                let outcome = self.dialogs.show(
                    &FileDialogRequest::open_file()
                        .title("Open into editor")
                        .filter(FileFilter::new("Text", ["toml", "rs", "txt", "md", "log"])),
                );
                if let Some(path) = outcome.path() {
                    // The buffer is held whole in memory — cap the read
                    // so a 2 GB log can't balloon the process.
                    let too_big = std::fs::metadata(path)
                        .map(|m| m.len() > 16 * 1024 * 1024)
                        .unwrap_or(false);
                    match if too_big {
                        Err(std::io::Error::new(
                            std::io::ErrorKind::FileTooLarge,
                            "file exceeds the 16 MiB editor cap",
                        ))
                    } else {
                        std::fs::read_to_string(path)
                    } {
                        Ok(text) => {
                            let name = path
                                .file_name()
                                .map(|n| n.to_string_lossy().into_owned())
                                .unwrap_or_else(|| "untitled".to_string());
                            self.open_in.set(Some((name.clone(), text)));
                            push_toast(
                                &self.toast_inbox,
                                martensite::widgets::Severity::Info,
                                format!("Opened {name}"),
                            );
                        }
                        Err(err) => push_toast(
                            &self.toast_inbox,
                            martensite::widgets::Severity::Error,
                            format!("Open failed: {err}"),
                        ),
                    }
                }
            }
        }
        // Editor "save as…" chip → save dialog → write the buffer the
        // panel published through `doc_out`, then reveal the file.
        if self.export_req.get() {
            self.export_req.set(false);
            if self.dialogs.platform_name() == "stub" {
                push_toast(
                    &self.toast_inbox,
                    martensite::widgets::Severity::Warning,
                    "File dialogs unavailable on this platform",
                );
            } else if let Some((name, text)) = self.doc_out.get() {
                let outcome = self.dialogs.show(
                    &FileDialogRequest::save_file()
                        .title("Export editor buffer")
                        .default_name(&name)
                        .filter(FileFilter::new("Text", ["toml", "rs", "txt", "md"])),
                );
                if let Some(path) = outcome.path() {
                    match std::fs::write(path, &text) {
                        Ok(()) => {
                            push_toast(
                                &self.toast_inbox,
                                martensite::widgets::Severity::Info,
                                format!("Exported {name}"),
                            );
                            let outcome = self.share.reveal(path);
                            if !outcome.is_shared() {
                                push_toast(
                                    &self.toast_inbox,
                                    martensite::widgets::Severity::Warning,
                                    format!("reveal: {outcome:?}"),
                                );
                            }
                        }
                        Err(err) => push_toast(
                            &self.toast_inbox,
                            martensite::widgets::Severity::Error,
                            format!("Export failed: {err}"),
                        ),
                    }
                }
            }
        }
        // Toolbar "Share" → dispatch the report to the OS share
        // handler (a mailto draft on the subprocess backends).
        if self.share_req.get() {
            self.share_req.set(false);
            let request =
                ShareRequest::text(self.report_text()).subject("Industrial Workstation report");
            match self.share.share(&request) {
                ShareOutcome::Shared => push_toast(
                    &self.toast_inbox,
                    martensite::widgets::Severity::Info,
                    "Report shared",
                ),
                other => push_toast(
                    &self.toast_inbox,
                    martensite::widgets::Severity::Warning,
                    format!("share: {other:?}"),
                ),
            }
        }
        // Toolbar "Print" → submit the same artifact to the spooler.
        if self.print_req.get() {
            self.print_req.set(false);
            let job = PrintJob::text("Industrial Workstation", self.report_text());
            match self.printer.print(&job) {
                PrintOutcome::Submitted { job_id } => push_toast(
                    &self.toast_inbox,
                    martensite::widgets::Severity::Info,
                    match job_id {
                        Some(id) => format!("Print submitted — {id}"),
                        None => "Print submitted".to_string(),
                    },
                ),
                PrintOutcome::Cancelled => {}
                PrintOutcome::Failed(err) => push_toast(
                    &self.toast_inbox,
                    martensite::widgets::Severity::Error,
                    format!("Print failed: {err}"),
                ),
            }
        }
    }

    /// Telemetry alarm watch — runs after `arena.tick` so the panel's
    /// fresh samples drive it. Armed by the toolbar "alerts" switch;
    /// disarming clears all state so re-arming reports a fresh breach
    /// rather than a stale one. Notifications fire on transitions
    /// only — the hysteresis band debounces a hovering signal.
    fn watch_alarms(&mut self) {
        if !self.alerts_on.get() {
            self.alarm = AlarmWatch::default();
            return;
        }
        let mut notes = Vec::new();
        if !self.alarm.armed {
            self.alarm.armed = true;
            notes.push(
                Notification::new("Telemetry watch armed")
                    .body("cpu_load / mem_pressure thresholds live")
                    .urgency(Urgency::Low),
            );
        }
        let cpu = self.cpu.get() * 100.0;
        let mem = self.mem.get() * 100.0;
        // Wide hysteresis bands: the telemetry feed is synthetic and
        // oscillates, so a narrow band would flap the alarm on every
        // crossing. 10/8 points keeps transitions meaningful.
        for (name, pct, hi, lo, state) in [
            ("cpu_load", cpu, 82.0, 72.0, &mut self.alarm.cpu),
            ("mem_pressure", mem, 76.0, 68.0, &mut self.alarm.mem),
        ] {
            if !*state && pct >= hi {
                *state = true;
                notes.push(
                    Notification::new("Telemetry alarm")
                        .body(format!("{name} at {pct:.0}% — above {hi:.0}%"))
                        .urgency(Urgency::Critical),
                );
            } else if *state && pct < lo {
                *state = false;
                notes.push(
                    Notification::new("Telemetry cleared")
                        .body(format!("{name} back under {lo:.0}%"))
                        .urgency(Urgency::Low),
                );
            }
        }
        for note in &notes {
            if let Err(err) = self.notifier.notify(note) {
                push_toast(
                    &self.toast_inbox,
                    martensite::widgets::Severity::Error,
                    format!("notification failed: {err}"),
                );
            }
        }
    }

    /// Preference write-through — compares the live values against the
    /// last set flushed and rewrites the store on change (the
    /// `JsonFileStore` flush is atomic, so a change mid-crash can't
    /// corrupt the file).
    fn persist_prefs(&mut self) {
        let prefs = Prefs {
            theme: self.theme_choice,
            locale: self.locale_idx,
            editor_tab: self.editor_tab.get(),
            alerts: self.alerts_on.get(),
        };
        if prefs == self.saved_prefs {
            return;
        }
        let theme = match prefs.theme {
            ThemeChoice::Dark => "dark",
            ThemeChoice::Light => "light",
            ThemeChoice::System => "system",
        };
        // A `--theme` CLI override is session-scoped — persisting it
        // would silently rewrite the user's stored preference.
        if !self.theme_override {
            self.store.set("ui.theme", theme);
        }
        self.store.set(
            "ui.locale",
            LOCALE_CODES[prefs.locale.min(LOCALE_CODES.len() - 1)],
        );
        // Tabs opened via "open…" live past `SOURCES` — persisting a
        // temp index would restore the wrong tab next launch.
        if prefs.editor_tab < crate::model::SOURCES.len() {
            self.store.set("ui.editor_tab", prefs.editor_tab as u64);
        }
        self.store.set("ui.alerts", prefs.alerts);
        match self.store.flush() {
            Ok(()) => {
                self.persist_error_shown = false;
                // Only mark the set as flushed on success — on failure
                // the next frame retries the write.
                self.saved_prefs = prefs;
            }
            Err(err) => {
                // One toast per failure streak — a dead disk would
                // otherwise flood the inbox every frame.
                if !self.persist_error_shown {
                    self.persist_error_shown = true;
                    push_toast(
                        &self.toast_inbox,
                        martensite::widgets::Severity::Error,
                        format!("settings not saved: {err}"),
                    );
                }
            }
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

        // Dropdown → choice: the toolbar publishes its index; apply it.
        let sel = self.theme_sel.get();
        let wanted = match sel {
            1 => ThemeChoice::Light,
            2 => ThemeChoice::System,
            _ => ThemeChoice::Dark,
        };
        if wanted != self.theme_choice {
            self.set_theme_choice(wanted);
            crate::overlays::push_toast(
                &self.toast_inbox,
                martensite::widgets::Severity::Info,
                format!("Theme: {}", THEME_OPTIONS[sel.min(2)]),
            );
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
        // the backend write happens here, app-side. The success toast
        // only fires when a native backend actually wrote — `write`
        // returns `()`, so an absent backend is the only failure mode
        // we can detect, and claiming "Copied" then would be a lie.
        if let Some(payload) = self.clipboard_out.get() {
            self.clipboard_out.set(None);
            if let Some(cb) = self.clipboard.as_mut() {
                cb.write("text/plain;charset=utf-8", payload.as_bytes());
                crate::overlays::push_toast(
                    &self.toast_inbox,
                    martensite::widgets::Severity::Info,
                    "Copied to clipboard",
                );
            } else {
                crate::overlays::push_toast(
                    &self.toast_inbox,
                    martensite::widgets::Severity::Warning,
                    "No clipboard backend on this platform",
                );
            }
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
                // Reduced-motion jumps to the destination — no
                // crossfade (a real HMI accessibility gate; zone
                // widgets consult the same signal for their own
                // animation loops).
                if self.model.reduced_motion.get() || self.theme_anim.is_settled(id) || t >= 0.999 {
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
            // The secondary window's arena tracks the same theme.
            if let Some(sub) = &mut self.subwindow {
                sub.set_theme(arena.theme());
            }
        }

        // 1. Widget ticks — telemetry advances its signals, media pumps
        //    its pacing queue; dirty widgets mark repaint.
        let t0 = Instant::now();
        self.arena.as_mut().expect("checked").tick(dt);

        // Drive the plant model AFTER the tick so history/acoustic
        // read this frame's fresh cpu/mem samples. History samples at
        // 4 Hz (240-sample ring ≈ 1 min window); the shift clock
        // advances one simulated minute per real second so the demo
        // stays visibly alive. The toolbar pause freezes the whole
        // simulation — not just the gauges — so both drivers gate on
        // it. `while` drains keep sampling cadence after a hitch
        // instead of dropping the accumulated time.
        if !self.paused.get() {
            self.hist_acc += dt.as_secs_f64();
            while self.hist_acc >= 0.25 {
                self.hist_acc -= 0.25;
                self.model.push_history();
            }
            self.minute_acc += dt.as_secs_f64();
            while self.minute_acc >= 1.0 {
                self.minute_acc -= 1.0;
                self.model.tick_minute();
            }
            // Acoustic monitoring is part of the sim — pause freezes
            // the spectrum/VU/tuner along with history and the clock.
            self.model
                .tick_acoustic(self.started.elapsed().as_secs_f64());
        }

        // Panel/toolbar service requests → OS dialogs, share, print.
        // The backends aren't `Send`, so every call happens here,
        // app-side; widgets only ever publish request Signals. This
        // runs AFTER the tick so `doc_out` (published inside
        // `EditorPanel::tick`) reflects this frame's buffer, not last
        // frame's — an export must contain the latest keystroke.
        self.drain_service_requests();

        // Alarm watch + preference write-through — after the tick so
        // the telemetry panel's fresh samples and the editor's latest
        // tab selection drive them.
        self.watch_alarms();
        self.persist_prefs();

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
        list.push_fill_rect(kurbo::Rect::new(0.0, 0.0, w, h), self.pal.bg);
        {
            let root = self.root.expect("checked");
            let arena = self.arena.as_ref().expect("checked");
            arena.build_paint_list(root, &mut list);
        }
        // App chrome sits outside the widget tree — wrap it in a manual
        // provenance scope so audit findings still name a component.
        list.push_scope(None, "App Chrome", kurbo::Rect::new(0.0, 0.0, w, h));
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
                kurbo::Rect::new(
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
                orchestrator.audit_underflow(arena);
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
        //    then emit the tree (only when a client is attached). The
        //    full-tree rebuild is throttled to `A11Y_EMIT_INTERVAL`;
        //    an AT action or a focus change forces an immediate emit.
        if let Some(a11y) = &mut self.a11y {
            let arena = self.arena.as_mut().expect("checked");
            let mut dispatched = false;
            for request in std::mem::take(&mut *self.actions.lock()) {
                if let Some(action) = a11y.tree.decode_action(arena, &request) {
                    dispatch_a11y_action(arena, &action);
                    dispatched = true;
                }
            }
            let focus = self.focus.current_focus();
            a11y.tree.set_focus(focus);
            if dispatched || focus != self.a11y_last_focus {
                self.a11y_force = true;
            }
            if self.a11y_force || self.a11y_last_emit.elapsed() >= A11Y_EMIT_INTERVAL {
                a11y.adapter
                    .update_if_active(|| a11y.tree.build_update(arena));
                self.a11y_force = false;
                self.a11y_last_emit = Instant::now();
                self.a11y_last_focus = focus;
            }
        }

        // 7. Pacing lives in `about_to_wait` — the loop wakes at the
        //    next frame deadline instead of re-drawing immediately.
        self.next_frame = now + FRAME_INTERVAL;
    }
}

/// The dashboard's settings store — `StateStore::set` is generic, so
/// the trait isn't dyn-compatible; this enum is the concrete seam the
/// compiler suggests. `JsonFileStore` under the per-OS config dir when
/// one resolves (and parses), else a volatile `MemoryStore` so the app
/// still runs where no config root exists.
enum PrefStore {
    Json(JsonFileStore),
    Mem(MemoryStore),
}

impl StateStore for PrefStore {
    fn get(&self, key: &str) -> Option<&Value> {
        match self {
            Self::Json(s) => s.get(key),
            Self::Mem(s) => s.get(key),
        }
    }
    fn set(&mut self, key: impl Into<String>, value: impl Into<Value>) {
        match self {
            Self::Json(s) => s.set(key, value),
            Self::Mem(s) => s.set(key, value),
        }
    }
    fn remove(&mut self, key: &str) -> Option<Value> {
        match self {
            Self::Json(s) => s.remove(key),
            Self::Mem(s) => s.remove(key),
        }
    }
    fn keys(&self) -> Vec<String> {
        match self {
            Self::Json(s) => s.keys(),
            Self::Mem(s) => s.keys(),
        }
    }
    fn flush(&mut self) -> Result<(), PersistError> {
        match self {
            Self::Json(s) => s.flush(),
            Self::Mem(s) => s.flush(),
        }
    }
}

/// Opens the settings store — see [`PrefStore`].
fn open_store() -> PrefStore {
    // Unit tests must not read (or clobber) the developer's real
    // settings — always in-memory under `cfg(test)`.
    if cfg!(test) {
        return PrefStore::Mem(MemoryStore::new());
    }
    let Some(path) = default_store_path("martensite", "industrial-dashboard") else {
        return PrefStore::Mem(MemoryStore::new());
    };
    match JsonFileStore::open(&path) {
        Ok(store) => PrefStore::Json(store),
        Err(PersistError::Corrupt(_)) => {
            // A corrupt file would otherwise fall back to memory
            // FOREVER — `MemoryStore::flush` is a no-op so nothing
            // ever rewrites it. Move it aside and start fresh so
            // persistence self-heals on the next flush.
            let _ = std::fs::rename(&path, path.with_extension("json.bad"));
            JsonFileStore::open(&path)
                .map_or_else(|_| PrefStore::Mem(MemoryStore::new()), PrefStore::Json)
        }
        Err(_) => PrefStore::Mem(MemoryStore::new()),
    }
}

/// The persisted theme preference, if the store holds a known value.
fn stored_theme(store: &PrefStore) -> Option<ThemeChoice> {
    match store.get("ui.theme").and_then(|v| v.as_str()) {
        Some("light") => Some(ThemeChoice::Light),
        Some("system") => Some(ThemeChoice::System),
        Some("dark") => Some(ThemeChoice::Dark),
        _ => None,
    }
}

/// The persisted locale as a `LOCALE_CODES` index (0 = English).
fn stored_locale(store: &PrefStore) -> usize {
    store
        .get("ui.locale")
        .and_then(|v| v.as_str())
        .and_then(|code| LOCALE_CODES.iter().position(|c| *c == code))
        .unwrap_or(0)
}

/// The persisted editor tab index (clamped by the panel at restore).
fn stored_tab(store: &PrefStore) -> usize {
    store
        .get("ui.editor_tab")
        .and_then(|v| v.as_u64())
        .unwrap_or(0) as usize
}

/// The persisted alerts-switch state.
fn stored_alerts(store: &PrefStore) -> bool {
    store
        .get("ui.alerts")
        .and_then(|v| v.as_bool())
        .unwrap_or(false)
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
                    // Logical, not physical — on a 2× display a
                    // physical 1680×980 opens at 840×490 pt, under
                    // every zone's 320×240 minimum.
                    .with_surface_size(LogicalSize::new(1600.0, 1000.0))
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
        // wgpu reports validation errors through `log`, which this app
        // never initializes — without this callback a broken GPU frame
        // is invisible (the black-window investigation ran blind).
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
        // MARTENSITE_CPU=1 forces the TinySkia→write_texture path,
        // bypassing Vello + the segment composite — used to bisect
        // presentation failures.
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

    fn window_event(&mut self, event_loop: &dyn ActiveEventLoop, id: WindowId, event: WindowEvent) {
        // A pending toolbar request opens the secondary window — needs
        // `ActiveEventLoop`, which only exists inside event callbacks.
        if self.console_req.get() {
            self.console_req.set(false);
            if self.subwindow.is_none() {
                if let (Some(gpu), Some(arena)) = (&self.gpu, &self.arena) {
                    self.subwindow =
                        SubWindow::open(event_loop, gpu, arena.theme(), self.cpu.clone());
                    if self.subwindow.is_some() {
                        push_toast(
                            &self.toast_inbox,
                            martensite::widgets::Severity::Info,
                            "Console window opened",
                        );
                    }
                }
            }
        }
        // Events addressed to the secondary window bypass the primary
        // window's arena, router, and a11y adapter entirely.
        if let Some(sub) = &mut self.subwindow {
            if sub.window_id() == id {
                if let Some(gpu) = &self.gpu {
                    if sub.handle_event(gpu, &event) {
                        self.subwindow = None;
                    }
                }
                return;
            }
        }

        // AT sees every event first (focus tracking, event filtering).
        if let (Some(a), Some(w)) = (&mut self.a11y, &self.window) {
            a.adapter.process_event(w.as_ref(), &event);
        }

        match event {
            WindowEvent::CloseRequested => {
                // Persist pending preference writes — the change
                // detector flushes on edit, this catches anything
                // queued in the final frame.
                let _ = self.store.flush();
                event_loop.exit();
            }
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
                // The toolbar theme dropdown is the canonical control —
                // the transitional bare-`T` shortcut was removed because
                // it intercepted the keystroke before dispatch, eating
                // "t" typed into the filter input or the editor.
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
            WindowEvent::Ime(ime) => {
                // IME composition stream — Preedit/Commit route to the
                // focused widget; Enabled/Disabled are host lifecycle
                // and drop out of `ime_event_for_winit` as `None`.
                if let Some(ev) = ime_event_for_winit(&ime) {
                    let focused = self.focus.current_focus();
                    if let Some(arena) = &mut self.arena {
                        self.router.dispatch_ime_event(arena, focused, &ev);
                    }
                    self.sync_focus();
                }
            }
            WindowEvent::RedrawRequested => self.redraw(),
            _ => {
                // Any other event may have mutated widget state — draw
                // it on the next iteration rather than waiting out the
                // animation interval.
                self.frame_due = true;
            }
        }
    }

    fn about_to_wait(&mut self, event_loop: &dyn ActiveEventLoop) {
        if self.window.is_none() {
            event_loop.set_control_flow(ControlFlow::Wait);
            return;
        }
        let now = Instant::now();
        // Queued assistive-technology actions are drained in `redraw` —
        // while paused there is no animation beat, so a pending request
        // must wake the loop itself or AXPress would never dispatch.
        let pending_at = !self.actions.lock().is_empty();
        if self.frame_due || pending_at || now >= self.next_frame {
            // Input arrived or the frame deadline passed — animate now.
            self.frame_due = false;
            self.next_frame = now + FRAME_INTERVAL;
            if let Some(w) = &self.window {
                w.request_redraw();
            }
            event_loop.set_control_flow(ControlFlow::Poll);
        } else if self.paused.get() {
            // Paused: no animation beat — input wakes us instantly, and
            // a 10 Hz wake polls the AT-action queue (accesskit_winit's
            // QueueActions doesn't post a user event, so without this an
            // AXPress issued while paused would sit until real input).
            event_loop.set_control_flow(ControlFlow::WaitUntil(now + A11Y_EMIT_INTERVAL));
        } else {
            event_loop.set_control_flow(ControlFlow::WaitUntil(self.next_frame));
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
/// was passed. `flag_choice` is the `--theme` override — `None` lets the
/// persisted preference restore (the app installs the resolved choice
/// directly, no startup fade, so it lands settled).
/// `audit_locale` (the `--audit-locale` flag) opts the paint audit into
/// the `MissingLocale` lint — user-visible strings the shipped FTL
/// resources don't cover are reported through the same lint channel.
pub fn run(
    flag_choice: Option<ThemeChoice>,
    audit_locale: bool,
) -> Result<(), Box<dyn std::error::Error>> {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env().unwrap_or_else(|_| "warn".into()),
        )
        .init();
    let event_loop = EventLoop::new()?;
    // Wait, not Poll: the trailing request_redraw() in `redraw` is
    // vsync-paced, so the loop still ticks ~60 Hz while animating but
    // sleeps between events instead of busy-spinning.
    event_loop.set_control_flow(ControlFlow::Wait);
    event_loop.run_app(App::new(flag_choice, audit_locale))?;
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

        let mut app = App::new(Some(ThemeChoice::Dark), false);
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
        let mut app = App::new(Some(ThemeChoice::Dark), false);
        app.build_arena();
        let arena = app.arena.as_mut().expect("arena");
        let mut visited = std::collections::HashSet::new();
        // One full cycle — six FOCUSABLE nodes: toolbar, four
        // panels, status bar.
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

    /// Scratch: for every OccludedText lint, identify the covering
    /// fill — walks the command stream like the audit does and reports
    /// the later opaque fills covering all five probe samples.
    fn dump_occluders(list: &PaintList, lints: &[martensite::access::paint_audit::PaintLint]) {
        use kurbo::Shape;
        use martensite::access::paint_audit::PaintLintKind;
        use martensite::core::PaintCommand;
        // Re-walk: record (idx, clip, scope) for fills and texts.
        let mut clip_stack: Vec<kurbo::Rect> = Vec::new();
        let mut scope_stack: Vec<(String, kurbo::Rect)> = Vec::new();
        #[derive(Debug)]
        struct Fill {
            idx: usize,
            rect: kurbo::Rect,
            color: [u8; 4],
            clip: Option<kurbo::Rect>,
            scope: String,
        }
        let mut fills: Vec<Fill> = Vec::new();
        let mut texts: Vec<(usize, kurbo::Rect, Option<kurbo::Rect>, String)> = Vec::new();
        for (i, cmd) in list.commands.iter().enumerate() {
            match cmd {
                PaintCommand::ClipRect(r) | PaintCommand::ClipRoundedRect(r, _) => {
                    clip_stack.push(*r)
                }
                PaintCommand::ClipPath(p) => {
                    let mut bb = kurbo::Rect::new(
                        f64::INFINITY,
                        f64::INFINITY,
                        f64::NEG_INFINITY,
                        f64::NEG_INFINITY,
                    );
                    for el in p.elements() {
                        for pt in match el {
                            kurbo::PathEl::MoveTo(p) | kurbo::PathEl::LineTo(p) => vec![*p],
                            kurbo::PathEl::QuadTo(a, b) => vec![*a, *b],
                            kurbo::PathEl::CurveTo(a, b, c) => vec![*a, *b, *c],
                            kurbo::PathEl::ClosePath => vec![],
                        } {
                            bb = bb.union(kurbo::Rect::new(pt.x, pt.y, pt.x, pt.y));
                        }
                    }
                    clip_stack.push(bb);
                }
                PaintCommand::PopClip => {
                    clip_stack.pop();
                }
                PaintCommand::PushScope { name, bounds, .. } => {
                    scope_stack.push((name.to_string(), *bounds))
                }
                PaintCommand::PopScope => {
                    scope_stack.pop();
                }
                _ => {}
            }
            let clip = clip_stack.iter().copied().reduce(|a, b| a.intersect(b));
            let scope = scope_stack
                .last()
                .map(|(n, _)| n.clone())
                .unwrap_or_default();
            match cmd {
                PaintCommand::FillRect(r, c) => fills.push(Fill {
                    idx: i,
                    rect: *r,
                    color: *c,
                    clip,
                    scope,
                }),
                PaintCommand::FillPath(p, c) => fills.push(Fill {
                    idx: i,
                    rect: p.bounding_box(),
                    color: *c,
                    clip,
                    scope,
                }),
                PaintCommand::DrawText(p, t, sz, _) => {
                    let w = (*sz as f64) * 0.6 * t.chars().count() as f64;
                    let b = kurbo::Rect::new(p.x, p.y, p.x + w.max(*sz as f64), p.y + *sz as f64);
                    texts.push((i, b, clip, scope));
                }
                PaintCommand::DrawGlyphRun(run) => {
                    if run.glyphs.is_empty() {
                        continue;
                    }
                    let x0 = run.glyphs.iter().map(|g| g.x).fold(f32::INFINITY, f32::min);
                    let x1 = run
                        .glyphs
                        .iter()
                        .map(|g| g.x + g.width)
                        .fold(f32::NEG_INFINITY, f32::max);
                    let base = run
                        .glyphs
                        .iter()
                        .map(|g| g.y)
                        .fold(f32::NEG_INFINITY, f32::max);
                    let b = kurbo::Rect::new(
                        x0 as f64,
                        (base - run.font_size * 0.8) as f64,
                        x1 as f64,
                        (base + run.font_size * 0.25) as f64,
                    );
                    texts.push((i, b, clip, scope));
                }
                _ => {}
            }
        }
        for l in lints
            .iter()
            .filter(|l| l.kind == PaintLintKind::OccludedText)
        {
            // Find the text rec nearest the anchor.
            let Some(&(ti, tb, tclip, ref tscope)) = texts.iter().min_by_key(|(_, b, _, _)| {
                let cx = (b.x0 + b.x1) / 2.0 - l.anchor.0;
                let cy = (b.y0 + b.y1) / 2.0 - l.anchor.1;
                (cx * cx + cy * cy) as i64
            }) else {
                continue;
            };
            let vis = tclip.map_or(tb, |c| tb.intersect(c));
            let (w, h) = (vis.width(), vis.height());
            let samples = [
                (vis.x0 + w * 0.5, vis.y0 + h * 0.5),
                (vis.x0 + w * 0.25, vis.y0 + h * 0.25),
                (vis.x1 - w * 0.25, vis.y0 + h * 0.25),
                (vis.x0 + w * 0.25, vis.y1 - h * 0.25),
                (vis.x1 - w * 0.25, vis.y1 - h * 0.25),
            ];
            let covers: Vec<String> = fills
                .iter()
                .filter(|f| {
                    f.idx > ti
                        && f.color[3] == 255
                        && samples.iter().all(|&(sx, sy)| {
                            f.rect.contains(kurbo::Point::new(sx, sy))
                                && f.clip.is_none_or(|c| c.contains(kurbo::Point::new(sx, sy)))
                        })
                })
                .map(|f| format!("fill@{:?} {:?} scope={}", f.rect, f.color, f.scope))
                .collect();
            eprintln!(
                "OCCLUDED @ {:?} scope={} text_vis={:?} covered_by={:?}",
                l.anchor, tscope, vis, covers
            );
        }
    }

    /// The toolbar's chrome launchers translate into zone page
    /// requests: Commands → Editor ▸ CHROME (2,3), Bell → Media ▸
    /// COMMS (3,0). The ZonePanel tick then activates the tab.
    #[test]
    fn chrome_launchers_navigate() {
        let mut app = App::new(Some(ThemeChoice::Dark), false);
        app.commands_req.set(true);
        app.bell_req.set(true);
        app.drain_service_requests();
        let req = app.model.page_request.get();
        assert_eq!(req[2], Some(3), "commands → editor CHROME");
        assert_eq!(req[3], Some(0), "bell → media COMMS");
        assert!(!app.commands_req.get());
        assert!(!app.bell_req.get());
    }

    /// Scratch: dump every audit lint at a given scale/size.
    #[test]
    fn dump_all_lints() {
        use martensite::access::paint_audit::{audit_paint_list, PaintAuditConfig};
        let mut app = App::new(Some(ThemeChoice::Dark), false);
        app.scale.set(1.0);
        app.build_arena();
        app.apply_dock_layout_at(3200, 2100);
        let arena = app.arena.as_ref().expect("arena");
        let mut list = PaintList::new();
        arena.build_paint_list(app.root.expect("root"), &mut list);
        // Audit at the scale the list was painted at — a mismatch
        // halves every reported font size (13pt reads as "6pt").
        let cfg = PaintAuditConfig {
            scale_factor: app.scale.get(),
            ..Default::default()
        };
        let lints = audit_paint_list(&list, &cfg);
        eprintln!("=== {} lints ===", lints.len());
        for l in &lints {
            eprintln!("{:?} {:?}: {}", l.severity, l.kind, l.detail);
        }
        dump_occluders(&list, &lints);
    }

    /// Scratch: mount each zone page in a ScrollView (as ZonePanel
    /// does), tick bindings, scroll through the content, and audit
    /// every frame — reproduces the live per-frame paint audit.
    #[test]
    fn dump_zone_lints() {
        use martensite::access::paint_audit::{audit_paint_list, PaintAuditConfig};
        use martensite::core::{SemanticAction, WidgetEvent};
        use martensite::widgets::container::Container;
        use martensite::widgets::scrollview::ScrollView;

        // Parent-first + bounds-gated, mirroring
        // `WidgetArena::tick_recursive` — production ticks
        // `ZonePanel` before its pages, which is what lets the
        // panel-level stale-sub clear run before page drains.
        fn tick_all(w: &mut dyn martensite::core::Widget, dt: Duration) {
            let _ = w.tick(dt);
            for i in 0..w.child_count() {
                if w.child_bounds(i).is_none() {
                    continue;
                }
                if let Some(c) = w.child_mut(i) {
                    tick_all(c, dt);
                }
            }
        }

        // Same deterministic-font setup as `lint_sweep::run` — `Text`
        // widgets' lazily-built `FontManager`s resolve the bundled
        // fixture, so the audit can't drift with the host's font set.
        let _font_guard = crate::frames::install_test_fonts();
        let app = App::new(Some(ThemeChoice::Dark), false);
        type ZonePages = Vec<(&'static str, crate::zone::Page)>;
        let mut pages_by_zone: Vec<(&str, usize, f32, f32, ZonePages)> = vec![];
        for zw in [
            700.0f32, 900.0, 1100.0, 1324.0, 1500.0, 1828.0, 2100.0, 2400.0,
        ] {
            pages_by_zone.push(("grid", 0, zw, 480.0, crate::zones::grid::pages(&app.model)));
            pages_by_zone.push((
                "telemetry",
                1,
                zw,
                480.0,
                crate::zones::telemetry::pages(&app.model),
            ));
            pages_by_zone.push((
                "editor",
                2,
                zw,
                350.0,
                crate::zones::editor::pages(&app.model),
            ));
            pages_by_zone.push((
                "media",
                3,
                zw,
                350.0,
                crate::zones::media::pages(&app.model),
            ));
        }
        let cfg = PaintAuditConfig {
            scale_factor: 2.0,
            ..Default::default()
        };
        let mut seen = std::collections::HashSet::new();
        let filter = std::env::var("PAGE_FILTER").unwrap_or_default();
        for (zname, zindex, zw, zh, pages) in pages_by_zone {
            // Same contract as `lint_sweep::run` — no ZonePanel ever
            // publishes here, so seed the zone's width slot or the
            // pages audit at the 960 default and the stacked/disclosure
            // breakpoints go unexercised (pt = px / 2.0 scale).
            app.model.zone_width[zindex].set(zw / 2.0);
            for (label, page) in pages {
                if !filter.is_empty() && !format!("{zname}/{label}@{zw:.0}").contains(&filter) {
                    continue;
                }
                let view = ScrollView::new(
                    Container::new()
                        .padding_uniform(crate::zone::ZONE_PAD)
                        .child(page),
                );
                let mut arena = WidgetArena::new();
                arena.set_theme(martensite::theme::tokens::default_dark());
                arena.set_scale_factor(2.0);
                // Fixture painter, not shared_painter — the audit must
                // not drift with the host's installed font set.
                arena.set_text_painter(crate::frames::FixtureTextShaper::new());
                let mut hot = HotNode::default();
                hot.flags |= NodeFlags::VISIBLE;
                let root = arena.insert_with_widget(hot, Box::new(view));
                let bounds = Rect::new(0.0, 0.0, zw, zh);
                if let Some((hot, cold)) = arena.get_both_mut(root) {
                    hot.bounds = bounds;
                    cold.widget
                        .layout(&mut LayoutContext { hot, scale: 2.0 }, bounds);
                }
                // Run the binding cycle once so Bound::push populates
                // the widgets with model data.
                if let Some(cold) = arena.get_cold_mut(root) {
                    tick_all(&mut *cold.widget, Duration::from_millis(16));
                }
                let content_h = arena
                    .get_cold(root)
                    .and_then(|c| c.widget.child_bounds(0))
                    .map(|b| b.height())
                    .unwrap_or(0.0);
                eprintln!("--- {zname}/{label}@{zw:.0}x{zh:.0}: content_h={content_h:.0} ---");
                let mut y = 0.0f32;
                loop {
                    arena.dispatch_event(
                        root,
                        &WidgetEvent::SemanticAction(SemanticAction::SetScrollOffset(Vec2::new(
                            0.0, y,
                        ))),
                    );
                    let mut list = PaintList::new();
                    arena.build_paint_list(root, &mut list);
                    if std::env::var("DUMP_SCOPES").is_ok() {
                        for cmd in &list.commands {
                            if let martensite::core::PaintCommand::PushScope {
                                name, bounds, ..
                            } = cmd
                            {
                                eprintln!("  scope {name} {bounds:?}");
                            }
                        }
                    }
                    let lints = audit_paint_list(&list, &cfg);
                    if lints.iter().any(|l| {
                        l.kind == martensite::access::paint_audit::PaintLintKind::OccludedText
                    }) {
                        eprintln!("--- occluders {zname}/{label}@{zw:.0} scroll_y={y:.0} ---");
                        dump_occluders(&list, &lints);
                    }
                    for l in lints {
                        if l.severity != martensite::access::paint_audit::LintSeverity::Warning {
                            continue;
                        }
                        let key = format!("{:?} {}", l.kind, l.detail);
                        if seen.insert(key) {
                            eprintln!("{zname}/{label}@{zw:.0}: {:?} {}", l.kind, l.detail);
                        }
                    }
                    if y >= content_h {
                        break;
                    }
                    y += zh * 0.5;
                }
            }
        }
    }

    /// Scratch: run the `martensite-design-lint` catalog over the same
    /// surfaces `dump_zone_lints` paints — each zone page in a
    /// `ScrollView` across widths, plus one full-app dock pass — with
    /// `design-lint.toml` supplying the scale factor, name
    /// reclassification, and path allows. Findings dedupe on
    /// (rule, message); `PAGE_FILTER` narrows the zone loop and
    /// `DUMP_SCOPES` echoes the scope tree. The sweep machinery is
    /// shared with the `design-lint` CLI bin in `crate::lint_sweep`.
    /// Run:
    /// `cargo test -p industrial_dashboard dump_design_lints -- --nocapture`.
    ///
    /// Gating: the warn-and-above set is asserted against the
    /// checked-in `LINT_GATING_BASELINE.txt` — new warnings fail, and
    /// fixed-but-still-listed entries fail as stale, so the baseline
    /// can only shrink through a regenerate. `LINT_BASELINE=rewrite`
    /// rewrites the file. Stale `[[allow]]` entries (suppressing
    /// nothing in any frame) also fail. Skipped under `PAGE_FILTER`
    /// since a partial sweep can't compare against the full set.
    #[test]
    fn dump_design_lints() {
        use martensite_design_lint::LintConfig;

        let cfg = LintConfig::from_toml(include_str!("../design-lint.toml"))
            .expect("design-lint.toml parses");
        let opts = crate::lint_sweep::SweepOptions {
            page_filter: std::env::var("PAGE_FILTER").unwrap_or_default(),
            dump_scopes: std::env::var("DUMP_SCOPES").is_ok(),
            ..Default::default()
        };
        let report = crate::lint_sweep::run(&cfg, &opts);
        eprint!("{}", report.log);
        // Both asserts gate on a full sweep — a filtered run only
        // exercises a slice, so an allow for a filtered-out subtree
        // would read as stale spuriously and the baseline compare
        // would fail on the missing pages' findings.
        if opts.page_filter.is_empty() {
            assert!(
                report.stale_allows.is_empty(),
                "stale path allows in design-lint.toml: {:?} — remove them",
                report.stale_allows
            );
            let baseline = std::path::Path::new("LINT_GATING_BASELINE.txt");
            if std::env::var("LINT_BASELINE").as_deref() == Ok("rewrite") {
                let body = report.gating_details.join("\n");
                // No trailing newline on an empty set — `"\n"` would
                // read back as one empty line and fail as a stale
                // entry the moment the backlog fully clears.
                let body = if body.is_empty() { body } else { body + "\n" };
                std::fs::write(baseline, body).expect("write LINT_GATING_BASELINE.txt");
            }
            let want: Vec<String> = std::fs::read_to_string(baseline)
                .expect("LINT_GATING_BASELINE.txt missing — run with LINT_BASELINE=rewrite")
                .lines()
                .filter(|l| !l.is_empty())
                .map(str::to_string)
                .collect();
            let new: Vec<String> = report
                .gating_details
                .iter()
                .filter(|g| !want.contains(g))
                .cloned()
                .collect();
            let stale: Vec<String> = want
                .iter()
                .filter(|g| !report.gating_details.contains(g))
                .cloned()
                .collect();
            assert!(
                new.is_empty() && stale.is_empty(),
                "gating findings drifted from LINT_GATING_BASELINE.txt\n\
                 NEW (fail — fix or justify + regenerate):\n  {}\n\
                 STALE (fixed — regenerate to shrink):\n  {}",
                new.join("\n  "),
                stale.join("\n  "),
            );
        }
    }

    /// Reproduces the windowed paint audit headless: drive the real
    /// dock layout at a fixed surface size, build the paint list,
    /// and audit — no widget may emit text fully outside its clip
    /// (invisible output = wasted work or a positioning bug).
    #[test]
    fn no_text_paints_outside_active_clip() {
        use martensite::access::paint_audit::{audit_paint_list, PaintAuditConfig, PaintLintKind};
        let mut app = App::new(Some(ThemeChoice::Dark), false);
        app.build_arena();
        eprintln!("arena built");
        app.apply_dock_layout_at(1600, 1000);
        eprintln!("layout done");
        let arena = app.arena.as_ref().expect("arena");
        let mut list = PaintList::new();
        arena.build_paint_list(app.root.expect("root"), &mut list);
        eprintln!("paint list: {} commands", list.commands.len());
        let lints = audit_paint_list(&list, &PaintAuditConfig::default());
        let clipped: Vec<_> = lints
            .iter()
            .filter(|l| matches!(l.kind, PaintLintKind::ClippedText))
            .collect();
        for l in &clipped {
            eprintln!("CLIPPED: {:?}", l.detail);
        }
        assert!(
            clipped.is_empty(),
            "{} clipped-text findings",
            clipped.len()
        );
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
        let mut app = App::new(Some(ThemeChoice::Dark), false);
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

    /// Prints the full widget tree — arena nodes and widget-internal
    /// children — with layout bounds, for structural review:
    /// `cargo test -p industrial_dashboard dump_widget_tree -- --nocapture`.
    #[test]
    fn dump_widget_tree() {
        // Text measure runs through per-widget `FontManager`s — the
        // fixture keeps the tree identical across host font sets.
        let _font_guard = crate::frames::install_test_fonts();
        let mut app = App::new(Some(ThemeChoice::Dark), false);
        app.build_arena();
        app.apply_dock_layout_at(1680, 980);
        // Parent-first + bounds-gated, mirroring
        // `WidgetArena::tick_recursive` (see `tick_all` above).
        fn tick_deep(w: &mut dyn martensite::core::Widget, dt: Duration) {
            let _ = w.tick(dt);
            for i in 0..w.child_count() {
                if w.child_bounds(i).is_none() {
                    continue;
                }
                if let Some(c) = w.child_mut(i) {
                    tick_deep(c, dt);
                }
            }
        }
        let arena = app.arena.as_mut().expect("arena");
        if let Some(cold) = arena.get_cold_mut(app.root.expect("root")) {
            tick_deep(&mut *cold.widget, Duration::from_millis(16));
        }
        eprintln!("{}", arena.debug_tree());
    }

    /// Renders the real dashboard paint list through the *surface*
    /// composite path (`vello_direct=false`, `Bgra8UnormSrgb` target —
    /// the same segment-texture + `WgpuHost` composite the live window
    /// uses) and asserts the frame is non-black. The target is sized
    /// past Vello 0.10's fixed bump-buffer capacity (its `blend_spill`
    /// exhausted around ~2600x1500 on this scene, producing an empty
    /// frame — the black-window bug), so this is regression coverage
    /// for both the compositor and the scaled bump allocation in the
    /// vendored renderer. GPU-dependent: `cargo test -p
    /// industrial_dashboard gpu_readback_real_frame -- --ignored`.
    #[test]
    #[ignore = "requires a GPU adapter"]
    fn gpu_readback_real_frame() {
        let mut app = App::new(Some(ThemeChoice::Dark), false);
        app.build_arena();
        app.apply_dock_layout_at(3024, 1694);
        let w = 3024.0f64;
        let h = 1694.0f64;
        let mut list = PaintList::new();
        list.push_fill_rect(kurbo::Rect::new(0.0, 0.0, w, h), app.pal.bg);
        app.arena
            .as_ref()
            .expect("arena")
            .build_paint_list(app.root.expect("root"), &mut list);
        list.push_scope(None, "App Chrome", kurbo::Rect::new(0.0, 0.0, w, h));
        app.paint_chrome(&mut list, w, h);
        list.pop_scope();

        let ctx = match GpuContext::new() {
            Ok(c) => c,
            Err(e) => {
                eprintln!("no GPU context: {e} — skipping");
                return;
            }
        };
        let mut orchestrator =
            RenderOrchestrator::new(w as u32, h as u32, OrchestratorConfig::new(true, false))
                .expect("orchestrator");
        let recovery = RecoveryMachine::new();
        orchestrator.render(&list, &recovery);
        let pixels = orchestrator
            .render_to_buffer_via_composite(&ctx.device, &ctx.queue, w as u32, h as u32)
            .expect("readback");
        let total = pixels.len() / 4;
        let nonzero = pixels
            .as_chunks::<4>()
            .0
            .iter()
            .filter(|p| p[0] != 0 || p[1] != 0 || p[2] != 0)
            .count();
        eprintln!("composite {w}x{h}: {nonzero}/{total} non-black");
        assert!(nonzero > 0, "GPU composite readback is entirely black");
    }
}
