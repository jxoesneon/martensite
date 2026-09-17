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
use martensite::widgets::{Button, CheckBox, Dropdown, Slider, TextInput};

/// Strip height in logical pt.
pub const TOOLBAR_H: f32 = 40.0;

/// Theme dropdown option order — index maps to `ThemeChoice`.
pub const THEME_OPTIONS: [&str; 3] = ["Dark", "Light", "System"];

const PAUSE: usize = 0;
const GLOW: usize = 1;
const TICK: usize = 2;
const THEME: usize = 3;
const FILTER: usize = 4;
const N: usize = 5;

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
    pause: Button,
    glow: CheckBox,
    tick: Slider,
    theme: Dropdown,
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
    /// dropdown writes it, the app writes it back on `T` cycling, and
    /// `tick` reconciles the dropdown with app-side changes.
    theme_sel: Signal<usize>,
    /// Grid filter text — `GridPanel` folds it into its `RowFilter`.
    filter_text: Signal<String>,
    /// Last selection index observed — `tick` uses it to tell a popup
    /// commit (dropdown moved) from an external write (signal moved).
    last_theme_idx: usize,
}

impl Toolbar {
    pub fn new(
        scale: Signal<f32>,
        paused: Signal<bool>,
        glow_on: Signal<bool>,
        tick_ms: Signal<f64>,
        theme_sel: Signal<usize>,
        filter_text: Signal<String>,
    ) -> Self {
        Self {
            scale,
            bounds: Rect::new(0.0, 0.0, 0.0, 0.0),
            focused: false,
            key_target: None,
            pause_armed: false,
            pause: Button::new("Pause").tooltip("pause telemetry (Space in Telemetry works too)"),
            glow: CheckBox::new("glow").checked(true),
            tick: Slider::new(20.0, 500.0)
                .with_value(100.0)
                .step(10.0)
                .label("sample ms"),
            theme: Dropdown::new(THEME_OPTIONS).label("theme"),
            filter: TextInput::new("filter grid").placeholder("filter pid/mem/status…"),
            rects: [Rect::new(0.0, 0.0, 0.0, 0.0); N],
            paused,
            glow_on,
            tick_ms,
            theme_sel,
            filter_text,
            last_theme_idx: 0,
        }
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
        self.filter_text.set_if_changed(self.filter.value.clone());
    }

    fn child_mut_at(&mut self, i: usize) -> Option<&mut dyn Widget> {
        match i {
            PAUSE => Some(&mut self.pause),
            GLOW => Some(&mut self.glow),
            TICK => Some(&mut self.tick),
            THEME => Some(&mut self.theme),
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

        // Fixed slots for the four controls; the filter input takes the
        // remainder (clamped — collapses to nothing under real pressure).
        // Rect is (x, y, width, height) — not min/max corners.
        let slots: [(usize, f32); 4] = [
            (PAUSE, 84.0 * s),
            (GLOW, 76.0 * s),
            (TICK, 180.0 * s),
            (THEME, 120.0 * s),
        ];
        for (i, w) in slots {
            let r = Rect::new(x, y, w.min(right - x).max(0.0), h);
            self.rects[i] = r;
            if let Some(c) = self.child_mut_at(i) {
                cx.layout_child(c, r);
            }
            x = r.max_x() + gap;
        }
        let r = Rect::new(x, y, fw_width(x, right), h);
        self.rects[FILTER] = r;
        if let Some(c) = self.child_mut_at(FILTER) {
            cx.layout_child(c, r);
        }
    }

    fn event(&mut self, cx: &mut EventContext) -> EventResponse {
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
                let mut hit = None;
                for i in (0..N).rev() {
                    if self.rects[i].contains(pos) {
                        hit = Some(i);
                        break;
                    }
                }
                let Some(i) = hit else {
                    return EventResponse::Ignored;
                };
                // Internal focus: pointer press re-targets keys and
                // moves the TextInput's FocusGained/Lost with it.
                if matches!(cx.event, WidgetEvent::PointerPressed { .. }) {
                    self.key_target = Some(i);
                    self.pause_armed = i == PAUSE;
                    let focus_ev = if i == FILTER {
                        WidgetEvent::FocusGained
                    } else {
                        WidgetEvent::FocusLost
                    };
                    let fb = self.rects[FILTER];
                    let mut fcx = EventContext {
                        event: &focus_ev,
                        bounds: fb,
                    };
                    self.filter.event(&mut fcx);
                }
                let bounds = self.rects[i];
                let mut child_cx = EventContext {
                    event: cx.event,
                    bounds,
                };
                let r = self
                    .child_mut_at(i)
                    .map(|c| c.event(&mut child_cx))
                    .unwrap_or(EventResponse::Ignored);
                // Pause is a stateless Button — the click is ours to
                // interpret: armed press + release inside toggles it.
                if i == PAUSE
                    && self.pause_armed
                    && matches!(
                        cx.event,
                        WidgetEvent::PointerReleased {
                            button: PointerButton::Primary,
                            ..
                        }
                    )
                {
                    let now = !self.paused.get();
                    self.paused.set(now);
                    self.pause_armed = false;
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
        let mut dirty = false;
        // Direction-aware reconcile: a popup commit moves `selected()`
        // while an app-side change (the `T` cycle, `--theme`) moves the
        // signal — compare both against the last observed index so each
        // side updates the other without stomping an in-flight gesture.
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
        // Space-driven pause (Telemetry panel) re-syncs the label.
        let want = if self.paused.get() { "Resume" } else { "Pause" };
        if self.pause.label != want {
            self.pause.label = want.to_string();
            dirty = true;
        }
        dirty
    }

    fn sync_overlay(&mut self, overlay: &mut OverlayLayer) {
        // The dropdown owns a popup — let it reconcile against the layer.
        self.theme.sync_overlay(overlay);
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
        self.rects.get(index).copied()
    }

    fn paint(&self, cx: &mut PaintContext) {
        let b = cx.bounds;
        let raised = cx.color(TokenKey::DividerColor, [46, 48, 53, 255]);
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
