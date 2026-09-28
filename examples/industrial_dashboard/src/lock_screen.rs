//! The console-lock overlay card — mounted through the arena
//! `OverlayLayer` by `App::sync_lock_overlay` while
//! `PlantModel::console_locked` is held. The signal is set by the
//! CONSOLE LOCK page's LOCK button and the COMMAND SURFACE's
//! `console lock` verb; this widget is the unlock side of the same
//! seam.
//!
//! The card hosts the same four credential paths the CONSOLE LOCK
//! page binds — pattern, PIN keypad, OTP, and password — under a
//! [`Segmented`] method switcher, and applies the zone's exact
//! acceptance rules (any 4+ dot gesture, `CONSOLE_PIN`, `CONSOLE_OTP`,
//! `CONSOLE_PW`) against the same shared model signals, so both
//! surfaces report one operator action stream. Input below the card
//! is blocked by the layer's modal scrim plus the app's event gate;
//! inside the card every event is consumed so nothing bleeds through.
//!
//! Ownership split: this file owns the widget; `app.rs` owns the
//! overlay entry's lifecycle.

use parking_lot::Mutex;

use glam::Vec2;
use martensite::core::overlay::{OverlayAnchor, OverlayLayer, OverlayOptions};
use martensite::core::{
    EventContext, EventResponse, LayoutConstraints, LayoutContext, PaintContext, Rect, Widget,
    WidgetEvent,
};
use martensite::reactive::Signal;
use martensite::render::Point;
use martensite::widgets::keypad::Keypad;
use martensite::widgets::otp_input::OtpInput;
use martensite::widgets::pattern_lock::PatternLock;
use martensite::widgets::segmented::Segmented;
use martensite::widgets::text_input::TextInput;
use martensite::widgets::tour::Tour;

use std::sync::Arc;

use crate::domain::PlantModel;
use crate::model::Palette;
use crate::panels::to_paint;
use crate::text::TextPainter;
use crate::zones::editor::{CONSOLE_OTP, CONSOLE_PIN, CONSOLE_PW};

/// Card chrome geometry (logical pt): uniform padding, title line,
/// method switcher row, status line, and the gaps between them.
const CARD_PAD_PT: f32 = 20.0;
const TITLE_PT: f32 = 22.0;
const SEG_PT: f32 = 30.0;
const HINT_PT: f32 = 18.0;
const GAP_PT: f32 = 10.0;
/// The card's logical width — fields center inside it.
const CARD_W_PT: f32 = 340.0;

/// Method-switcher indices.
const M_PATTERN: usize = 0;
const M_PIN: usize = 1;
const M_OTP: usize = 2;
const M_PW: usize = 3;

/// The locked console's modal surface. `console_locked` going false
/// is the unlock — the card never closes itself; the owning app
/// notices the signal and removes the entry.
pub struct LockScreen {
    /// The shared plant model — the card writes `console_locked` and
    /// the shift log directly, the same seam the CONSOLE LOCK page
    /// uses.
    model: PlantModel,
    /// Method switcher — which credential field is live.
    methods: Segmented,
    /// The four credential fields — only the selected method is laid
    /// out, painted, and offered events; the rest stay parked.
    pattern: PatternLock,
    keypad: Keypad,
    otp: OtpInput,
    pw: TextInput,
    /// PIN attempt accumulator — the keypad emits one char per press;
    /// four digits submit, '*' backspaces, '#' clears (the zone's
    /// exact rule).
    pin_buf: String,
    /// Paint-time shaping — `Widget::paint` is `&self` (F19).
    text: Mutex<TextPainter>,
    bounds: Rect,
    seg_bounds: Rect,
    field_bounds: Rect,
    /// Persistent rejection notice — overlay entries never receive
    /// `tick`, so there is no frame clock to expire a flash against;
    /// the line stays until the next accepted credential or method
    /// switch clears it.
    notice: String,
}

impl LockScreen {
    pub fn new(model: &PlantModel) -> Self {
        Self {
            model: model.clone(),
            methods: Segmented::new()
                .options(["PATTERN", "PIN", "OTP", "PASSWORD"])
                .selected(M_PATTERN),
            pattern: PatternLock::new().label("unlock pattern"),
            keypad: Keypad::new().label("PIN pad"),
            otp: OtpInput::new().length(CONSOLE_OTP.len()),
            pw: TextInput::new("password")
                .secure(true)
                .placeholder("console password…"),
            pin_buf: String::new(),
            text: Mutex::new(TextPainter::new()),
            bounds: Rect::default(),
            seg_bounds: Rect::default(),
            field_bounds: Rect::default(),
            notice: String::new(),
        }
    }

    /// The selected credential method.
    fn method(&self) -> usize {
        self.methods.selected_index()
    }

    /// The live credential field.
    fn field(&self) -> &dyn Widget {
        match self.method() {
            M_PIN => &self.keypad,
            M_OTP => &self.otp,
            M_PW => &self.pw,
            _ => &self.pattern,
        }
    }

    fn field_mut(&mut self) -> &mut dyn Widget {
        match self.method() {
            M_PIN => &mut self.keypad,
            M_OTP => &mut self.otp,
            M_PW => &mut self.pw,
            _ => &mut self.pattern,
        }
    }

    /// One unlock — writes the shared signal and the shift log, the
    /// exact writes the CONSOLE LOCK page performs per method.
    fn unlock(&mut self, how: &str) {
        self.model.console_locked.set_if_changed(false);
        self.model
            .log(usize::MAX, format!("console unlocked — {how}"));
        self.notice.clear();
    }

    fn reject(&mut self, why: &str) {
        self.notice = why.to_string();
        self.model.log(usize::MAX, why.to_string());
    }

    /// Drains each field's completion channel after an event — the
    /// same acceptance rules the zone's `Bound::pull`s apply.
    fn settle(&mut self) {
        match self.method() {
            M_PIN => {
                while let Some(c) = self.keypad.take_pressed() {
                    match c {
                        '0'..='9' => {
                            self.pin_buf.push(c);
                            if self.pin_buf.len() >= CONSOLE_PIN.len() {
                                if self.pin_buf == CONSOLE_PIN {
                                    self.unlock("PIN accepted");
                                } else {
                                    self.reject("PIN rejected");
                                }
                                self.pin_buf.clear();
                            }
                        }
                        '*' => {
                            self.pin_buf.pop();
                        }
                        _ => self.pin_buf.clear(),
                    }
                }
            }
            M_OTP => {
                while let Some(code) = self.otp.take_completed() {
                    if code == CONSOLE_OTP {
                        self.unlock("OTP accepted");
                    } else {
                        self.reject("OTP rejected");
                    }
                    self.otp.set_value("");
                }
            }
            M_PW => {
                if self.pw.take_edited() && self.pw.value == CONSOLE_PW {
                    self.unlock("password accepted");
                    self.pw.set_value("");
                }
            }
            _ => {
                while let Some(p) = self.pattern.take_pattern() {
                    if p.len() >= 4 {
                        self.unlock("pattern accepted");
                    } else {
                        self.reject("pattern rejected — 4+ dots required");
                    }
                    self.pattern.clear();
                }
            }
        }
    }
}

impl Widget for LockScreen {
    fn debug_name(&self) -> &'static str {
        "Console Lock"
    }

    fn measure(&mut self, cx: &mut LayoutContext, c: LayoutConstraints) -> Vec2 {
        let s = cx.scale;
        // Field area: the method's natural size, padded — the card
        // never squeezes a credential control past its floor.
        let field = self.field_mut().measure(cx, c);
        let chrome = 2.0 * CARD_PAD_PT + TITLE_PT + SEG_PT + 3.0 * GAP_PT + HINT_PT;
        Vec2::new(
            (CARD_W_PT * s)
                .max(field.x + 2.0 * CARD_PAD_PT * s)
                .min(c.max_size.x.max(0.0)),
            (field.y + chrome * s).min(c.max_size.y.max(0.0)),
        )
    }

    fn layout(&mut self, cx: &mut LayoutContext, bounds: Rect) {
        self.bounds = bounds;
        let pad = cx.pt(CARD_PAD_PT);
        // The method switcher rides under the title.
        self.seg_bounds = Rect::new(
            bounds.min_x() + pad,
            bounds.min_y() + pad + cx.pt(TITLE_PT),
            bounds.width() - 2.0 * pad,
            cx.pt(SEG_PT),
        );
        cx.layout_child(&mut self.methods, self.seg_bounds);
        // The active field centers in the band between the switcher
        // and the status line, keeping its measured aspect (the
        // 3×3 grid stays square; the keypad keeps its cell pitch).
        let top = self.seg_bounds.max_y() + cx.pt(GAP_PT);
        let bottom = bounds.max_y() - pad - cx.pt(HINT_PT) - cx.pt(GAP_PT);
        let avail = Rect::new(
            bounds.min_x() + pad,
            top,
            bounds.width() - 2.0 * pad,
            (bottom - top).max(0.0),
        );
        let natural = self.field_mut().measure(
            cx,
            LayoutConstraints {
                min_size: Vec2::ZERO,
                max_size: avail.size,
            },
        );
        let w = natural.x.min(avail.width());
        let h = natural.y.min(avail.height());
        let field_bounds = Rect::new(
            avail.min_x() + (avail.width() - w) * 0.5,
            avail.min_y() + (avail.height() - h) * 0.5,
            w,
            h,
        );
        self.field_bounds = field_bounds;
        cx.layout_child(self.field_mut(), field_bounds);
    }

    fn event(&mut self, cx: &mut EventContext) -> EventResponse {
        // Method switcher first — a segment press reparks the field
        // set before the active one sees the event.
        if matches!(
            cx.event,
            WidgetEvent::PointerPressed { .. }
                | WidgetEvent::PointerMoved { .. }
                | WidgetEvent::PointerReleased { .. }
        ) {
            let mut seg_cx = EventContext {
                event: cx.event,
                bounds: self.seg_bounds,
                scale: cx.scale,
            };
            self.methods.event(&mut seg_cx);
            if self.methods.take_selected().is_some() {
                // Re-measure + relayout the newly active field against
                // the same card, and clear any stale rejection.
                self.notice.clear();
                let mut hot = martensite::core::HotNode::default();
                let mut lx = LayoutContext {
                    hot: &mut hot,
                    scale: cx.scale,
                };
                self.layout(&mut lx, self.bounds);
            }
        }
        // The active credential field — pointer fields take pointer
        // events; the text fields take pointer, key, and IME.
        let deliver = match self.method() {
            M_PW | M_OTP => true,
            _ => matches!(
                cx.event,
                WidgetEvent::PointerPressed { .. }
                    | WidgetEvent::PointerMoved { .. }
                    | WidgetEvent::PointerReleased { .. }
            ),
        };
        if deliver {
            let mut inner = EventContext {
                event: cx.event,
                bounds: self.field_bounds,
                scale: cx.scale,
            };
            self.field_mut().event(&mut inner);
        }
        self.settle();
        // Modal surface: nothing inside the card ever bleeds through —
        // the layer's scrim already floors events outside it.
        EventResponse::Handled
    }

    fn paint(&self, cx: &mut PaintContext) {
        let pal = Palette::from_theme(cx.theme);
        let s = cx.scale;
        let sd = f64::from(s);
        let b = to_paint(self.bounds);
        let card = martensite::core::shape::Shape::rounded(10.0 * s);
        cx.list.push_fill_shape(b, &card, pal.raised);
        cx.list.push_stroke_shape(b, &card, 1.0, pal.border);
        let mut text = self.text.lock();
        let pad = f64::from(CARD_PAD_PT) * sd;
        text.push(
            cx.list,
            Point::new(b.x0 + pad, b.y0 + pad * 0.8),
            "CONSOLE LOCKED",
            14.0 * s,
            pal.text,
            None,
        );
        // Method switcher + active field paint inside their own
        // clips — same card, ordinary child paint.
        self.methods.paint(&mut PaintContext {
            list: cx.list,
            bounds: self.seg_bounds,
            theme: cx.theme,
            scale: cx.scale,
            text_painter: cx.text_painter,
        });
        self.field().paint(&mut PaintContext {
            list: cx.list,
            bounds: self.field_bounds,
            theme: cx.theme,
            scale: cx.scale,
            text_painter: cx.text_painter,
        });
        // The status line is the rejection channel — persistent
        // rather than flashed so it survives the frame the entry
        // never ticks through.
        let (hint, color) = if !self.notice.is_empty() {
            (self.notice.as_str(), pal.error)
        } else {
            ("unlock with pattern, PIN, OTP, or password", pal.text_muted)
        };
        let fit = text.fit(hint, 12.0 * s, (b.width() - 2.0 * pad).max(1.0) as f32);
        text.push(
            cx.list,
            Point::new(b.x0 + pad, b.y1 - pad - 3.0 * sd),
            &fit,
            12.0 * s,
            color,
            None,
        );
    }

    fn accessibility(&self, node: &mut accesskit::Node) {
        node.set_role(accesskit::Role::Dialog);
        node.set_label("Console locked — unlock with pattern, PIN, OTP, or password");
    }

    fn child_count(&self) -> usize {
        2
    }

    fn child(&self, index: usize) -> Option<&dyn Widget> {
        match index {
            0 => Some(&self.methods),
            1 => Some(self.field()),
            _ => None,
        }
    }

    fn child_mut(&mut self, index: usize) -> Option<&mut dyn Widget> {
        match index {
            0 => Some(&mut self.methods),
            1 => Some(self.field_mut()),
            _ => None,
        }
    }

    fn child_bounds(&self, index: usize) -> Option<Rect> {
        match index {
            0 => Some(self.seg_bounds),
            1 => Some(self.field_bounds),
            _ => None,
        }
    }
}

// ---------------------------------------------------------------------------
// Shell gate overlays — the app-level modal surfaces.
// ---------------------------------------------------------------------------

/// Terminal-state codes the [`TourOverlay`] wrapper publishes through
/// its `done` cell once the layer owns the `Tour` (the widget's
/// `take_finished`/`take_dismissed` channels live inside the entry,
/// out of the owner's reach).
const TOUR_RUNNING: u8 = 0;
const TOUR_FINISHED: u8 = 1;
const TOUR_DISMISSED: u8 = 2;

/// The first-run [`Tour`] wrapped so its terminal edges surface to the
/// owner as a `Signal<u8>` write — the layer entry holds the widget
/// tree, so the tour's `take_*` channels can only be drained by a
/// widget *inside* it. This shim forwards the whole event stream to
/// the tour and reports the edge the framework can't.
pub struct TourOverlay {
    tour: Tour,
    /// `TOUR_RUNNING` → `TOUR_FINISHED`/`TOUR_DISMISSED`.
    done: Signal<u8>,
}

impl TourOverlay {
    fn new(done: Signal<u8>) -> Self {
        let tour = Tour::new()
            .label("workstation tour")
            .step(
                "Line A workstation",
                "Four docked panels share one plant model — every control writes a signal every view reads.",
                None,
            )
            .step(
                "Grid & telemetry",
                "The process grid and telemetry charts read the same cells; the toolbar pause holds the whole plant.",
                None,
            )
            .step(
                "Zone pages",
                "Press the panel title (or ⌘K) for the domain pages under each panel — registry, orders, comms, chrome.",
                None,
            )
            .step(
                "Console lock",
                "APPEARANCE ▸ CONSOLE LOCK engages the operator lock — pattern, PIN, OTP, or password releases it.",
                None,
            )
            .skippable(true);
        Self { tour, done }
    }
}

impl Widget for TourOverlay {
    fn debug_name(&self) -> &'static str {
        "First-Run Tour"
    }

    fn measure(&mut self, cx: &mut LayoutContext, c: LayoutConstraints) -> Vec2 {
        self.tour.measure(cx, c)
    }

    fn layout(&mut self, cx: &mut LayoutContext, bounds: Rect) {
        cx.layout_child(&mut self.tour, bounds);
    }

    fn event(&mut self, cx: &mut EventContext) -> EventResponse {
        let r = self.tour.event(cx);
        if self.tour.take_finished() {
            self.done.set(TOUR_FINISHED);
        } else if self.tour.take_dismissed() {
            self.done.set(TOUR_DISMISSED);
        }
        r
    }

    fn accessibility(&self, node: &mut accesskit::Node) {
        self.tour.accessibility(node);
    }

    fn paint(&self, cx: &mut PaintContext) {
        self.tour.paint(cx);
    }

    fn child_count(&self) -> usize {
        1
    }

    fn child(&self, index: usize) -> Option<&dyn Widget> {
        (index == 0).then_some(&self.tour as &dyn Widget)
    }

    fn child_mut(&mut self, index: usize) -> Option<&mut dyn Widget> {
        (index == 0).then_some(&mut self.tour as &mut dyn Widget)
    }
}

/// The overlay owner for the shell's gate surfaces — the console
/// lock screen and the first-run tour — reconciled every frame
/// against `console_locked` and `tour_seen`. Same zero-bounds
/// `sync_overlay` pattern as `ShellOverlays`: the widget itself is
/// never painted; everything it shows lives in the layer.
pub struct GateOverlays {
    model: PlantModel,
    /// Where the app publishes the live lock-entry id — the input
    /// gate reads it to route keys and IME into the card.
    pub(crate) lock_entry: Arc<std::sync::Mutex<Option<u64>>>,
    lock_id: Option<u64>,
    tour_id: Option<u64>,
    /// The active tour's terminal-state cell — re-created per
    /// presentation so a stale edge can never fire twice.
    tour_done: Signal<u8>,
    /// `true` once the tour has been presented this session — the
    /// first-run auto-show is one-shot; the RESET TOUR edge
    /// (`tour_seen` true→false) re-arms it.
    tour_shown: bool,
    /// Escape/lock-interrupt close = "remind me later": snoozed for
    /// the session, still `tour_seen == false` so the next launch
    /// shows it again.
    tour_snoozed: bool,
    /// Last observed `tour_seen` — RESET TOUR's edge detection.
    tour_seen_prev: bool,
}

impl GateOverlays {
    pub fn new(model: &PlantModel) -> Self {
        Self {
            model: model.clone(),
            lock_entry: Arc::new(std::sync::Mutex::new(None)),
            lock_id: None,
            tour_id: None,
            tour_done: Signal::new(TOUR_RUNNING),
            tour_shown: false,
            tour_snoozed: false,
            tour_seen_prev: model.tour_seen.get(),
        }
    }
}

impl Widget for GateOverlays {
    fn debug_name(&self) -> &'static str {
        "GateOverlays"
    }

    fn measure(&mut self, _cx: &mut LayoutContext, _c: LayoutConstraints) -> Vec2 {
        Vec2::ZERO
    }

    fn layout(&mut self, _cx: &mut LayoutContext, _b: Rect) {}

    fn sync_overlay(&mut self, overlay: &mut OverlayLayer) {
        let locked = self.model.console_locked.get();
        let seen = self.model.tour_seen.get();

        // RESET TOUR's true→false write re-arms the presentation.
        if self.tour_seen_prev && !seen {
            self.tour_shown = false;
            self.tour_snoozed = false;
        }
        self.tour_seen_prev = seen;

        // --- Tour lifecycle --------------------------------------
        if let Some(id) = self.tour_id {
            if !overlay.is_open(id) {
                // Closed from outside the content — Escape at the
                // layer. Remind-me-later: snooze this session,
                // `tour_seen` stays false so the next launch
                // re-presents.
                self.tour_id = None;
                self.tour_snoozed = true;
            } else {
                match self.tour_done.get() {
                    TOUR_FINISHED => {
                        overlay.close(id);
                        self.tour_id = None;
                        if self.model.tour_seen.set_if_changed(true) {
                            self.model.log(usize::MAX, "first-run tour finished");
                        }
                    }
                    TOUR_DISMISSED => {
                        overlay.close(id);
                        self.tour_id = None;
                        self.tour_snoozed = true;
                        self.model
                            .log(usize::MAX, "tour dismissed — shows on next launch");
                    }
                    _ => {}
                }
            }
        }

        // --- Lock lifecycle --------------------------------------
        if locked {
            // The lock outranks the tour — park a running tour as a
            // snooze rather than stacking two modals.
            if let Some(id) = self.tour_id.take() {
                overlay.close(id);
                self.tour_snoozed = true;
            }
            // Reconcile: the entry may have closed underneath us
            // (the layer's Escape path pops the topmost popup) —
            // while `console_locked` holds, the surface re-mounts.
            if self.lock_id.is_none_or(|id| !overlay.is_open(id)) {
                self.lock_id = Some(overlay.open_with(
                    Box::new(LockScreen::new(&self.model)),
                    OverlayAnchor::Center,
                    OverlayOptions::modal(),
                ));
            }
        } else if let Some(id) = self.lock_id.take() {
            if overlay.is_open(id) {
                overlay.close(id);
            }
        }
        *self.lock_entry.lock().unwrap() = if locked { self.lock_id } else { None };

        // --- Tour presentation ------------------------------------
        // First run (or a RESET edge): `!tour_seen`, never snoozed or
        // shown this session, and no lock up.
        if !locked && !seen && !self.tour_shown && !self.tour_snoozed && self.tour_id.is_none() {
            self.tour_done = Signal::new(TOUR_RUNNING);
            self.tour_id = Some(overlay.open_with(
                Box::new(TourOverlay::new(self.tour_done.clone())),
                OverlayAnchor::Center,
                OverlayOptions::modal(),
            ));
            self.tour_shown = true;
        }
    }

    fn event(&mut self, _cx: &mut EventContext) -> EventResponse {
        EventResponse::Ignored
    }

    fn accessibility(&self, node: &mut accesskit::Node) {
        node.set_role(accesskit::Role::GenericContainer);
        node.set_label("gate overlays");
    }

    fn paint(&self, _cx: &mut PaintContext) {}
}
