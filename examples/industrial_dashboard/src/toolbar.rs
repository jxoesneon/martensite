//! Toolbar — the workstation's live-controls strip.
//!
//! A real container `Widget` holding facade children (`Button`,
//! `Checkbox`, `Slider`, `Dropdown`, `TextInput`) that *do things*:
//! telemetry pause, chart-glow toggle, sample-rate control, theme
//! selection, and a live `RowFilter` on the million-row grid. Outcomes
//! travel through shared `Signal`s — the app and panels read them; the
//! toolbar writes them when child state changes. This is the two-level
//! architecture's intended composition: internal children own their own
//! interaction state; the parent observes and translates it into model
//! effects.
//!
//! Focus model: arena focus lands on the toolbar as one unit (internal
//! children aren't arena nodes). Pointer presses pick a `key_target`
//! child that then receives non-positional events — clicking the filter
//! input directs `ImeCommitted`/`Backspace` there; clicking the slider
//! directs arrow keys there. `FocusGained`/`FocusLost` are forwarded so
//! `TextInput` paints its caret only when it is the real edit target.

use accesskit::Node as AccessKitNode;
use glam::Vec2;
use martensite::core::overlay::OverlayLayer;
use martensite::core::{
    EventContext, EventResponse, LayoutConstraints, LayoutContext, PaintContext, PointerButton,
    Rect, SemanticAction, Widget, WidgetEvent,
};
use martensite::prelude::Signal;
use martensite::render::BezPath;
use martensite::theme::TokenKey;
use martensite::widgets::menu::MenuItem;
use martensite::widgets::menu_button::MenuButton;
use martensite::widgets::{Button, CheckBox, Dropdown, Separator, Slider, Switch, TextInput};

/// Strip height in logical pt.
pub const TOOLBAR_H: f32 = 40.0;

/// Theme dropdown option order — index maps to `ThemeChoice`.
pub const THEME_OPTIONS: [&str; 3] = ["Dark", "Light", "System"];

const PAUSE: usize = 0;
const GLOW: usize = 1;
const TICK: usize = 2;
const THEME: usize = 3;
const SEP: usize = 4;
const ALERTS: usize = 5;
const COMMANDS: usize = 6;
const BELL: usize = 7;
const SHELL: usize = 8;
const FILTER: usize = 9;
const N: usize = 10;

/// Shell-menu rows — the index in the `MenuButton` item list that maps
/// to each request signal.
const SHELL_ABOUT: usize = 0;
const SHELL_INSPECTOR: usize = 1;
const SHELL_CONSOLE: usize = 2;
const SHELL_SHARE: usize = 3;
const SHELL_PRINT: usize = 4;

/// Outcome signals shared between the toolbar and the app — clones
/// share the same cells, so both sides observe state without
/// downcasting through `dyn Widget`.
#[derive(Clone)]
pub struct ToolbarSignals {
    /// Telemetry pause — Space in the Telemetry panel writes the same
    /// cell, so the button label re-syncs in `tick`.
    pub paused: Signal<bool>,
    /// Chart area-fill toggle.
    pub glow_on: Signal<bool>,
    /// Telemetry sample period in milliseconds (20–500).
    pub tick_ms: Signal<f64>,
    /// Theme selection as a `THEME_OPTIONS` index — bidirectional.
    pub theme_sel: Signal<usize>,
    /// Grid filter text — `GridPanel` folds it into its `RowFilter`.
    pub filter_text: Signal<String>,
    /// Row-alert strip toggle — `TelemetryPanel` shows its `Banner`
    /// while set; the switch writes, the banner's × clears.
    pub alerts_on: Signal<bool>,
    /// "Commands" pressed — the app navigates to Editor ▸ CHROME
    /// (the `CommandPalette` surface).
    pub commands_req: Signal<bool>,
    /// "Alerts" pressed — the app navigates to Media ▸ COMMS (the
    /// `NotificationCenter` announcements surface).
    pub bell_req: Signal<bool>,
    /// "About" pressed — `ShellOverlays` opens the modal dialog.
    pub about_req: Signal<bool>,
    /// "Inspector" pressed — `ShellOverlays` opens the drawer.
    pub inspector_req: Signal<bool>,
    /// "Console" pressed — the app opens the secondary OS window.
    pub console_req: Signal<bool>,
    /// "Share" pressed — the app dispatches the telemetry report
    /// through the OS share service.
    pub share_req: Signal<bool>,
    /// "Print" pressed — the app submits the same report to the OS
    /// spooler.
    pub print_req: Signal<bool>,
}

/// The toolbar widget. Outcome signals are shared cells — the app
/// clones them before constructing the widget, so both sides observe
/// the same state without downcasting through `dyn Widget`.
pub struct Toolbar {
    scale: Signal<f32>,
    bounds: Rect,
    focused: bool,
    /// The child that receives non-positional events (set by the last
    /// pointer press). Internal focus — the arena sees the toolbar as
    /// one focused unit.
    key_target: Option<usize>,
    /// Press armed inside the pause button — the Button facade keeps no
    /// pressed state, so the parent tracks the press/release pair.
    pause_armed: bool,
    /// See `pause_armed` — release inside fires the commands request.
    commands_armed: bool,
    /// See `pause_armed` — release inside fires the bell request.
    bell_armed: bool,
    /// Child currently holding a pointer press. While set, positional
    /// events forward to it regardless of hit position — captured
    /// drags (slider thumb, text-input drag-select) leave every child
    /// rect and would otherwise be dropped by the hit test.
    press_target: Option<usize>,
    pause: Button,
    glow: CheckBox,
    tick: Slider,
    theme: Dropdown,
    sep: Separator,
    alerts: Switch,
    commands: Button,
    bell: Button,
    /// The shell-layer verbs (dialog, drawer, window, OS services)
    /// folded into one menu — they're window chrome, not ops
    /// controls, so they don't compete with Pause/filter for strip
    /// width.
    shell: MenuButton,
    filter: TextInput,
    rects: [Rect; N],
    /// Telemetry pause — Space in the Telemetry panel writes the same
    /// cell, so the button label re-syncs in `tick`.
    paused: Signal<bool>,
    /// Chart area-fill toggle.
    glow_on: Signal<bool>,
    /// Telemetry sample period in milliseconds (20–500).
    tick_ms: Signal<f64>,
    /// Theme selection as a `THEME_OPTIONS` index — bidirectional: the
    /// dropdown writes it, the app may write it back, and `reconcile`
    /// resyncs the dropdown with app-side changes.
    theme_sel: Signal<usize>,
    /// Grid filter text — `GridPanel` folds it into its `RowFilter`.
    filter_text: Signal<String>,
    /// Row-alert strip toggle — `TelemetryPanel` shows its `Banner`
    /// while set; the switch writes, the banner's × clears.
    alerts_on: Signal<bool>,
    /// "Commands" pressed — navigate to the `CommandPalette` surface.
    commands_req: Signal<bool>,
    /// "Alerts" pressed — navigate to the `NotificationCenter` surface.
    bell_req: Signal<bool>,
    /// "About" pressed — `ShellOverlays` opens the modal dialog.
    about_req: Signal<bool>,
    /// "Inspector" pressed — `ShellOverlays` opens the drawer.
    inspector_req: Signal<bool>,
    /// "Console" pressed — the app opens the secondary OS window.
    console_req: Signal<bool>,
    /// "Share" pressed — the app dispatches the telemetry report
    /// through the OS share service.
    share_req: Signal<bool>,
    /// "Print" pressed — the app submits the same report to the OS
    /// spooler.
    print_req: Signal<bool>,

    /// Last selection index observed — `tick` uses it to tell a popup
    /// commit (dropdown moved) from an external write (signal moved).
    last_theme_idx: usize,
}

impl Toolbar {
    pub fn new(scale: Signal<f32>, signals: ToolbarSignals) -> Self {
        let ToolbarSignals {
            paused,
            glow_on,
            tick_ms,
            theme_sel,
            filter_text,
            alerts_on,
            commands_req,
            bell_req,
            about_req,
            inspector_req,
            console_req,
            share_req,
            print_req,
        } = signals;
        // Seed every control from its signal — restored/existing
        // values must survive the first `publish()` (an event can
        // arrive before the first `tick` reconcile would mirror
        // them into the widgets).
        let theme_idx = theme_sel.get().min(THEME_OPTIONS.len() - 1);
        Self {
            scale,
            bounds: Rect::new(0.0, 0.0, 0.0, 0.0),
            focused: false,
            key_target: None,
            pause_armed: false,
            commands_armed: false,
            bell_armed: false,
            press_target: None,
            pause: Button::new("Pause").tooltip("pause telemetry (Space in Telemetry works too)"),
            glow: CheckBox::new("glow").checked(glow_on.get()),
            tick: Slider::new(20.0, 500.0)
                .with_value(tick_ms.get())
                .step(10.0)
                .label("sample ms"),
            theme: {
                let mut dd = Dropdown::new(THEME_OPTIONS).label("theme");
                dd.commit(theme_idx);
                dd
            },
            sep: Separator::vertical(),
            alerts: Switch::new("alerts").on(alerts_on.get()),
            commands: Button::new("⌘ Commands").tooltip("command palette — Editor ▸ CHROME"),
            bell: Button::new("Alerts").tooltip("announcements — Media ▸ COMMS"),
            // The five shell verbs as one menu — order maps to the
            // `SHELL_*` row indices drained in `tick`.
            shell: MenuButton::new(
                "Shell",
                vec![
                    MenuItem::action("About…"),
                    MenuItem::action("Inspector"),
                    MenuItem::action("Console"),
                    MenuItem::action("Share"),
                    MenuItem::action("Print"),
                ],
            ),
            filter: TextInput::new("filter grid")
                .placeholder("filter pid/mem/status…")
                .value(filter_text.get()),
            rects: [Rect::new(0.0, 0.0, 0.0, 0.0); N],
            paused,
            glow_on,
            tick_ms,
            theme_sel,
            filter_text,
            alerts_on,
            commands_req,
            bell_req,
            about_req,
            inspector_req,
            console_req,
            share_req,
            print_req,
            last_theme_idx: theme_idx,
        }
    }

    /// Mirror externally-written signals into the widgets —
    /// direction-aware, so an in-flight gesture isn't stomped.
    /// Called from `tick` AND before forwarding any event: a signal
    /// written app-side (banner ×, `--theme`, `T` cycle) must be
    /// mirrored before `publish()` pushes widget state back, or the
    /// stale widget value resurrects over the external write.
    fn reconcile(&mut self) -> bool {
        let mut dirty = false;
        let sig = self.theme_sel.get().min(THEME_OPTIONS.len() - 1);
        let cur = self.theme.selected();
        if cur != self.last_theme_idx {
            self.theme_sel.set_if_changed(cur);
            self.last_theme_idx = cur;
            dirty = true;
        } else if sig != self.last_theme_idx && !self.theme.is_open() {
            self.theme.commit(sig);
            self.last_theme_idx = sig;
            dirty = true;
        }
        if self.alerts.on != self.alerts_on.get() {
            self.alerts.on = self.alerts_on.get();
            dirty = true;
        }
        dirty
    }

    fn s(&self) -> f32 {
        self.scale.get().max(1.0)
    }

    /// Push current child state into the outcome signals. Called after
    /// every forwarded event — `set_if_changed` keeps it cheap.
    fn publish(&mut self) {
        self.glow_on.set_if_changed(self.glow.checked);
        self.tick_ms.set_if_changed(self.tick.value());
        self.theme_sel.set_if_changed(self.theme.selected());
        self.alerts_on.set_if_changed(self.alerts.on);
        self.filter_text.set_if_changed(self.filter.value.clone());
    }

    fn child_mut_at(&mut self, i: usize) -> Option<&mut dyn Widget> {
        match i {
            PAUSE => Some(&mut self.pause),
            GLOW => Some(&mut self.glow),
            TICK => Some(&mut self.tick),
            THEME => Some(&mut self.theme),
            SEP => Some(&mut self.sep),
            ALERTS => Some(&mut self.alerts),
            COMMANDS => Some(&mut self.commands),
            BELL => Some(&mut self.bell),
            SHELL => Some(&mut self.shell),
            FILTER => Some(&mut self.filter),
            _ => None,
        }
    }

    fn child_at(&self, i: usize) -> Option<&dyn Widget> {
        match i {
            PAUSE => Some(&self.pause),
            GLOW => Some(&self.glow),
            TICK => Some(&self.tick),
            THEME => Some(&self.theme),
            SEP => Some(&self.sep),
            ALERTS => Some(&self.alerts),
            COMMANDS => Some(&self.commands),
            BELL => Some(&self.bell),
            SHELL => Some(&self.shell),
            FILTER => Some(&self.filter),
            _ => None,
        }
    }

    /// Forward a non-positional event to the current key target only —
    /// the internal-focus contract (default forwarding would broadcast
    /// keys to every child, letting the slider eat Backspace).
    fn forward_key(&mut self, cx: &mut EventContext) -> EventResponse {
        let Some(i) = self.key_target else {
            return EventResponse::Ignored;
        };
        let Some(bounds) = self.rects.get(i).copied() else {
            return EventResponse::Ignored;
        };
        let Some(child) = self.child_mut_at(i) else {
            return EventResponse::Ignored;
        };
        let mut child_cx = EventContext {
            event: cx.event,
            bounds,
            scale: cx.scale,
        };
        child.event(&mut child_cx)
    }
}

impl Widget for Toolbar {
    fn debug_name(&self) -> &'static str {
        "Toolbar"
    }

    fn measure(&mut self, _cx: &mut LayoutContext, constraints: LayoutConstraints) -> Vec2 {
        Vec2::new(
            constraints.max_size.x,
            (TOOLBAR_H * self.s()).min(constraints.max_size.y.max(0.0)),
        )
    }

    fn layout(&mut self, cx: &mut LayoutContext, bounds: Rect) {
        self.bounds = bounds;
        let s = self.s();
        let pad = 8.0 * s;
        let gap = 10.0 * s;
        let h = (bounds.size.y - 2.0 * pad).max(1.0);
        let y = bounds.origin.y + pad;
        let mut x = bounds.origin.x + pad;
        let right = bounds.max_x() - pad;

        // Fixed slots for the nine controls; the filter input takes the
        // remainder (clamped — collapses to nothing under real pressure).
        // Rect is (x, y, width, height) — not min/max corners.
        let slots: [(usize, f32); 9] = [
            (PAUSE, 84.0 * s),
            (GLOW, 76.0 * s),
            (TICK, 180.0 * s),
            (THEME, 120.0 * s),
            (SEP, 9.0 * s),
            (ALERTS, 104.0 * s),
            (COMMANDS, 112.0 * s),
            (BELL, 66.0 * s),
            (SHELL, 84.0 * s),
        ];
        // A slot renders only when it fits fully — a partially-shown
        // control emits text past its own bounds (the paint audit's
        // clipped-text findings). Slots that can't fit are suspended:
        // `child_bounds` reports `None`, removing the child from
        // paint, tick, and hit-testing until the toolbar widens.
        for (i, w) in slots {
            if right - x < w {
                self.rects[i] = Rect::new(x, y, 0.0, h);
                if self.key_target == Some(i) {
                    self.key_target = None;
                }
                continue;
            }
            let r = Rect::new(x, y, w, h);
            self.rects[i] = r;
            if let Some(c) = self.child_mut_at(i) {
                cx.layout_child(c, r);
            }
            x = r.max_x() + gap;
        }
        let r = Rect::new(x, y, fw_width(x, right), h);
        self.rects[FILTER] = r;
        if r.width() >= MIN_SLOT_W {
            if let Some(c) = self.child_mut_at(FILTER) {
                cx.layout_child(c, r);
            }
        } else if self.key_target == Some(FILTER) {
            self.key_target = None;
        }
    }

    fn event(&mut self, cx: &mut EventContext) -> EventResponse {
        // Mirror external signal writes before dispatch — otherwise
        // `publish()` below pushes the stale widget value back over
        // them (e.g. the banner × clearing `alerts_on`).
        self.reconcile();
        let resp = match cx.event {
            WidgetEvent::SemanticAction(SemanticAction::Focus | SemanticAction::Click) => {
                EventResponse::CaptureFocus
            }
            WidgetEvent::FocusGained => {
                self.focused = true;
                EventResponse::RequestRepaint
            }
            WidgetEvent::FocusLost => {
                self.focused = false;
                EventResponse::RequestRepaint
            }
            _ if cx.event.position().is_some() => {
                let pos = cx.event.position().expect("checked");
                // A held press keeps the event stream: drag moves and
                // the release forward to `press_target` even when the
                // pointer leaves every child rect. Everything else
                // hit-tests topmost-first.
                let hit = if matches!(cx.event, WidgetEvent::PointerPressed { .. }) {
                    (0..N).rev().find(|&i| self.rects[i].contains(pos))
                } else {
                    self.press_target
                        .or_else(|| (0..N).rev().find(|&i| self.rects[i].contains(pos)))
                };
                let Some(i) = hit else {
                    return EventResponse::Ignored;
                };
                // Internal focus: pointer press re-targets keys and
                // moves the TextInput's FocusGained/Lost with it.
                if matches!(cx.event, WidgetEvent::PointerPressed { .. }) {
                    self.key_target = Some(i);
                    self.press_target = Some(i);
                    self.pause_armed = i == PAUSE;
                    self.commands_armed = i == COMMANDS;
                    self.bell_armed = i == BELL;
                    let focus_ev = if i == FILTER {
                        WidgetEvent::FocusGained
                    } else {
                        WidgetEvent::FocusLost
                    };
                    let fb = self.rects[FILTER];
                    let mut fcx = EventContext {
                        event: &focus_ev,
                        bounds: fb,
                        scale: cx.scale,
                    };
                    self.filter.event(&mut fcx);
                }
                let bounds = self.rects[i];
                let mut child_cx = EventContext {
                    event: cx.event,
                    bounds,
                    scale: 1.0,
                };
                let r = self
                    .child_mut_at(i)
                    .map(|c| c.event(&mut child_cx))
                    .unwrap_or(EventResponse::Ignored);
                // The release ends the hold after the child sees it —
                // the child needs it to answer `ReleasePointer`.
                if matches!(cx.event, WidgetEvent::PointerReleased { .. }) {
                    self.press_target = None;
                }
                // Pause is a stateless Button — the click is ours to
                // interpret: armed press + release *inside* toggles it.
                // `press_target` routing now delivers outside releases
                // too, so the bounds check is explicit; either way the
                // release disarms the press.
                let released = matches!(
                    cx.event,
                    WidgetEvent::PointerReleased {
                        button: PointerButton::Primary,
                        ..
                    }
                );
                if i == PAUSE && self.pause_armed && released {
                    if self.rects[PAUSE].contains(pos) {
                        let now = !self.paused.get();
                        self.paused.set(now);
                    }
                    self.pause_armed = false;
                }
                // Stateless buttons — an armed press + release inside
                // fires the overlay request once.
                if i == COMMANDS && self.commands_armed && released {
                    if self.rects[COMMANDS].contains(pos) {
                        self.commands_req.set(true);
                    }
                    self.commands_armed = false;
                }
                if i == BELL && self.bell_armed && released {
                    if self.rects[BELL].contains(pos) {
                        self.bell_req.set(true);
                    }
                    self.bell_armed = false;
                }
                r
            }
            // Non-positional: internal focus decides.
            _ => self.forward_key(cx),
        };
        self.publish();
        resp
    }

    fn tick(&mut self, _dt: std::time::Duration) -> bool {
        let mut dirty = self.reconcile();
        // Space-driven pause (Telemetry panel) re-syncs the label.
        let want = if self.paused.get() { "Resume" } else { "Pause" };
        if self.pause.label != want {
            self.pause.label = want.to_string();
            dirty = true;
        }
        // Shell menu activations land via the overlay layer (shared
        // menu state), not the toolbar's event path — drain here.
        if let Some(path) = self.shell.take_activated() {
            match path.first() {
                Some(&SHELL_ABOUT) => self.about_req.set(true),
                Some(&SHELL_INSPECTOR) => self.inspector_req.set(true),
                Some(&SHELL_CONSOLE) => self.console_req.set(true),
                Some(&SHELL_SHARE) => self.share_req.set(true),
                Some(&SHELL_PRINT) => self.print_req.set(true),
                _ => {}
            }
            dirty = true;
        }
        dirty
    }

    fn sync_overlay(&mut self, overlay: &mut OverlayLayer) {
        // The dropdown and the shell menu own popups — let both
        // reconcile against the layer.
        self.theme.sync_overlay(overlay);
        self.shell.sync_overlay(overlay);
    }

    fn accessibility(&self, node: &mut AccessKitNode) {
        node.set_role(accesskit::Role::Toolbar);
        node.set_label("workstation controls");
        node.add_action(accesskit::Action::Focus);
    }

    fn child_count(&self) -> usize {
        N
    }

    fn child(&self, index: usize) -> Option<&dyn Widget> {
        self.child_at(index)
    }

    fn child_mut(&mut self, index: usize) -> Option<&mut dyn Widget> {
        self.child_mut_at(index)
    }

    fn child_bounds(&self, index: usize) -> Option<Rect> {
        // Degenerate slots suspend their child — the paint walk, tick
        // walk, and default event forwarding all honour `None` as
        // "not presented".
        self.rects
            .get(index)
            .copied()
            .filter(|r| r.width() >= MIN_SLOT_W)
    }

    fn paint(&self, cx: &mut PaintContext) {
        let b = cx.bounds;
        let raised = cx.color(TokenKey::RaisedColor, [46, 48, 53, 255]);
        let hairline = cx.color(TokenKey::TextMutedColor, [148, 163, 184, 255]);
        cx.list.push_fill_rect(
            martensite::render::Rect::new(
                f64::from(b.min_x()),
                f64::from(b.min_y()),
                f64::from(b.max_x()),
                f64::from(b.max_y()),
            ),
            raised,
        );
        // Bottom hairline separating the strip from the dock area.
        let y = f64::from(b.max_y()) - 1.0;
        let mut path = BezPath::new();
        path.move_to((f64::from(b.min_x()), y));
        path.line_to((f64::from(b.max_x()), y));
        cx.list.push_path(path, hairline);
        // Arena focus ring on the strip itself.
        if self.focused {
            let accent = cx.color(TokenKey::AccentColor, [96, 165, 250, 255]);
            cx.list.push_stroke_rect(
                martensite::render::Rect::new(
                    f64::from(b.min_x()),
                    f64::from(b.min_y()),
                    f64::from(b.max_x()),
                    f64::from(b.max_y()),
                ),
                cx.pt(2.0),
                accent,
            );
        }
    }
}

/// Remaining width for the filter slot — never negative.
fn fw_width(x: f32, right: f32) -> f32 {
    (right - x).max(0.0)
}

/// Narrowest slot that still presents a control — below this the slot
/// is suspended (no paint/tick/events) rather than clipped.
const MIN_SLOT_W: f32 = 4.0;

#[cfg(test)]
mod tests {
    use super::*;

    fn signals() -> ToolbarSignals {
        ToolbarSignals {
            commands_req: Signal::new(false),
            bell_req: Signal::new(false),
            paused: Signal::new(false),
            glow_on: Signal::new(true),
            tick_ms: Signal::new(100.0),
            theme_sel: Signal::new(0),
            filter_text: Signal::new(String::new()),
            alerts_on: Signal::new(true),
            about_req: Signal::new(false),
            inspector_req: Signal::new(false),
            console_req: Signal::new(false),
            share_req: Signal::new(false),
            print_req: Signal::new(false),
        }
    }

    #[test]
    fn seeded_controls_do_not_clobber_restored_signals() {
        // Same defect class as the status-bar locale fix: a toolbar
        // built from restored signals must publish them back
        // unchanged — widget defaults must not overwrite the restore.
        let sigs = signals();
        let alerts = sigs.alerts_on.clone();
        let theme = sigs.theme_sel.clone();
        theme.set(2); // "System" — as restored from the store
        let mut tb = Toolbar::new(Signal::new(1.0), sigs);
        tb.publish();
        assert!(alerts.get(), "publish clobbered restored alerts_on");
        assert_eq!(theme.get(), 2, "publish clobbered restored theme_sel");
    }

    #[test]
    fn external_signal_write_survives_event_dispatch() {
        // The banner × clears `alerts_on` app-side while the switch
        // still shows ON. `event()` must reconcile first — a stale
        // switch resurrecting the flag was the review finding.
        let sigs = signals();
        let alerts = sigs.alerts_on.clone();
        let mut tb = Toolbar::new(Signal::new(1.0), sigs);
        alerts.set(false);
        let ev = WidgetEvent::FocusGained;
        let mut cx = EventContext {
            event: &ev,
            bounds: Rect::new(0.0, 0.0, 800.0, 40.0),
            scale: 1.0,
        };
        tb.event(&mut cx);
        assert!(!alerts.get(), "stale switch resurrected alerts_on");
        assert!(!tb.alerts.on, "switch not reconciled to external write");
    }

    #[test]
    fn narrow_toolbar_suspends_slots_that_cannot_fit() {
        // The 524 clipped-text findings: right-edge slots painted
        // labels past their clamped rects. Now a slot renders only
        // when it fits fully — `child_bounds → None` suspends it.
        let mut tb = Toolbar::new(Signal::new(1.0), signals());
        let mut hot = martensite::core::HotNode::default();
        let mut cx = martensite::core::LayoutContext {
            hot: &mut hot,
            scale: 1.0,
        };
        // ~350pt holds only the first few slots (PAUSE 84 + GLOW 76 +
        // TICK 180 already exceed it with gaps).
        tb.layout(&mut cx, Rect::new(0.0, 0.0, 350.0, 40.0));

        let presented: Vec<usize> = (0..N).filter(|&i| tb.child_bounds(i).is_some()).collect();
        assert!(
            presented.len() < N,
            "every slot still presented at 350pt — suspension is broken"
        );
        // Presented slots must fit fully inside the toolbar.
        for i in presented {
            let r = tb.child_bounds(i).expect("checked");
            assert!(
                r.width() >= MIN_SLOT_W && r.max_x() <= 350.0 + 0.01,
                "slot {i} partially shown: {r:?}"
            );
        }
        // The first slot always fits.
        assert!(tb.child_bounds(PAUSE).is_some());

        // Widening restores the suspended slots.
        tb.layout(&mut cx, Rect::new(0.0, 0.0, 1600.0, 40.0));
        let restored: Vec<usize> = (0..N).filter(|&i| tb.child_bounds(i).is_some()).collect();
        assert_eq!(restored.len(), N, "slots did not restore at full width");
    }
}
