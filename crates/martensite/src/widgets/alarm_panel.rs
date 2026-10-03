//! `AlarmPanel` — an industrial alarm list with an acknowledge
//! lifecycle (the HMI/SCADA alarm-banner idiom).
//!
//! Each [`Alarm`] starts `Active`; clicking a row's `ACK` chip (or
//! calling [`AlarmPanel::acknowledge`] / [`AlarmPanel::acknowledge_all`])
//! moves it to `Acknowledged` and parks the index in
//! [`AlarmPanel::take_acked`]. Active `Error` rows flash their edge
//! on [`Widget::tick`](martensite_core::widget::Widget::tick) until
//! acknowledged. The wheel scrolls when
//! the list overflows.
//!
//! Distinct from [`crate::widgets::log_view::LogView`], which is a
//! read-only stream — alarms carry operator-visible state.
//!
//! # Examples
//!
//! ```
//! use martensite::widgets::alarm_panel::{Alarm, AlarmPanel};
//! use martensite::widgets::banner::Severity;
//!
//! let mut p = AlarmPanel::new();
//! p.push(Alarm::new(Severity::Error, "Tank 4 overpressure").source("PT-104"));
//! p.push(Alarm::new(Severity::Warning, "Filter ΔP high"));
//! assert_eq!(p.unacked_count(), 2);
//! p.acknowledge(0);
//! assert_eq!(p.unacked_count(), 1);
//! ```

use accesskit::Node as AccessKitNode;
use glam::Vec2;
use martensite_core::{
    EventContext, EventResponse, LayoutConstraints, LayoutContext, PaintContext, PointerButton,
    Rect, RenderMinimum, UnderflowPolicy, Widget, WidgetEvent,
};
use martensite_theme::TokenKey;
use parking_lot::Mutex;

use crate::widgets::banner::Severity;

const W_PT: f32 = 320.0;
const H_PT: f32 = 240.0;
const ROW_PT: f32 = 40.0;
const EDGE_PT: f32 = 4.0;
const PAD_PT: f32 = 6.0;

const FACE: [u8; 4] = [24, 24, 28, 255];
const ROW: [u8; 4] = [36, 36, 42, 255];
const ROW_ACKED: [u8; 4] = [30, 30, 34, 255];
const TEXT: [u8; 4] = [214, 214, 220, 255];
const MUTED: [u8; 4] = [150, 150, 158, 255];
const ACKED_TEXT: [u8; 4] = [120, 120, 126, 255];
const ACK_CHIP: [u8; 4] = [64, 64, 72, 255];
const ACK_TEXT: [u8; 4] = [220, 220, 226, 255];

const INFO: [u8; 4] = [90, 160, 250, 255];
const WARN: [u8; 4] = [245, 176, 66, 255];
const ERR: [u8; 4] = [235, 87, 87, 255];

/// Alarm lifecycle state — see [`AlarmPanel`].
///
/// ```
/// use martensite::widgets::alarm_panel::AlarmState;
///
/// assert_eq!(AlarmState::Active, AlarmState::Active);
/// ```
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AlarmState {
    /// Unacknowledged — demands operator attention.
    Active,
    /// Operator has seen it.
    Acknowledged,
}

/// One alarm entry — see [`AlarmPanel`].
///
/// ```
/// use martensite::widgets::alarm_panel::Alarm;
/// use martensite::widgets::banner::Severity;
///
/// let a = Alarm::new(Severity::Warning, "low flow");
/// assert_eq!(a.message, "low flow");
/// ```
#[derive(Debug, Clone)]
pub struct Alarm {
    /// Severity tier — drives the edge color.
    pub severity: Severity,
    /// Alarm description.
    pub message: String,
    /// Optional tag/source label (e.g. a sensor tag).
    pub source: String,
    /// Lifecycle state.
    pub state: AlarmState,
}

impl Alarm {
    /// An active alarm.
    ///
    /// ```
    /// use martensite::widgets::alarm_panel::{Alarm, AlarmState};
    /// use martensite::widgets::banner::Severity;
    ///
    /// let a = Alarm::new(Severity::Error, "fault");
    /// assert_eq!(a.state, AlarmState::Active);
    /// ```
    pub fn new(severity: Severity, message: impl Into<String>) -> Self {
        Self {
            severity,
            message: message.into(),
            source: String::new(),
            state: AlarmState::Active,
        }
    }

    /// Tag/source label.
    ///
    /// ```
    /// use martensite::widgets::alarm_panel::Alarm;
    /// use martensite::widgets::banner::Severity;
    ///
    /// let a = Alarm::new(Severity::Info, "x").source("FT-201");
    /// assert_eq!(a.source, "FT-201");
    /// ```
    pub fn source(mut self, source: impl Into<String>) -> Self {
        self.source = source.into();
        self
    }
}

fn accent(sev: Severity) -> [u8; 4] {
    match sev {
        Severity::Info => INFO,
        Severity::Warning => WARN,
        Severity::Error => ERR,
    }
}

/// An HMI alarm list — see the module docs.
///
/// ```
/// use martensite::widgets::alarm_panel::AlarmPanel;
///
/// assert!(AlarmPanel::new().is_empty());
/// ```
pub struct AlarmPanel {
    /// Accessibility label.
    pub label: String,
    /// Alarms, newest first.
    alarms: Vec<Alarm>,
    acked: Option<usize>,
    /// Whether the alarm data is pending (ADR-0040). A pending panel
    /// renders the explicit *data unknown* treatment — muted rows
    /// with `?` marks and no severity hues or ACK chips — never
    /// generic shimmer.
    loading: bool,
    scroll: f32,
    /// Blink phase for unacknowledged Error edges.
    blink: f32,
    bounds: Rect,
    scale: f32,
    /// Per-row `ACK` chip rects painted last frame.
    ack_hits: Mutex<Vec<(usize, Rect)>>,
    text_painter: Option<crate::text_paint::SharedTextPainter>,
}

impl std::fmt::Debug for AlarmPanel {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("AlarmPanel")
            .field("alarms", &self.alarms.len())
            .finish()
    }
}

impl Default for AlarmPanel {
    fn default() -> Self {
        Self::new()
    }
}

impl AlarmPanel {
    /// Creates an empty panel.
    ///
    /// ```
    /// use martensite::widgets::alarm_panel::AlarmPanel;
    ///
    /// assert_eq!(AlarmPanel::new().count(), 0);
    /// ```
    pub fn new() -> Self {
        Self {
            label: "Alarms".to_string(),
            alarms: Vec::new(),
            acked: None,
            loading: false,
            scroll: 0.0,
            blink: 0.0,
            bounds: Rect::new(0.0, 0.0, 0.0, 0.0),
            scale: 1.0,
            ack_hits: Mutex::new(Vec::new()),
            text_painter: None,
        }
    }

    /// Accessibility label.
    ///
    /// ```
    /// use martensite::widgets::alarm_panel::AlarmPanel;
    ///
    /// assert_eq!(AlarmPanel::new().label("Line 2 alarms").label, "Line 2 alarms");
    /// ```
    pub fn label(mut self, label: impl Into<String>) -> Self {
        self.label = label.into();
        self
    }

    /// Custom text painter.
    ///
    /// ```
    /// use martensite::widgets::alarm_panel::AlarmPanel;
    ///
    /// let _ = AlarmPanel::new(); // painter optional
    /// ```
    pub fn with_text_painter(mut self, p: crate::text_paint::SharedTextPainter) -> Self {
        self.text_painter = Some(p);
        self
    }

    /// Prepends an alarm (newest on top).
    ///
    /// ```
    /// use martensite::widgets::alarm_panel::{Alarm, AlarmPanel};
    /// use martensite::widgets::banner::Severity;
    ///
    /// let mut p = AlarmPanel::new();
    /// p.push(Alarm::new(Severity::Warning, "w"));
    /// assert_eq!(p.count(), 1);
    /// ```
    pub fn push(&mut self, alarm: Alarm) {
        self.alarms.insert(0, alarm);
    }

    /// Alarm at `i` (0 = newest).
    ///
    /// ```
    /// use martensite::widgets::alarm_panel::{Alarm, AlarmPanel};
    /// use martensite::widgets::banner::Severity;
    ///
    /// let mut p = AlarmPanel::new();
    /// p.push(Alarm::new(Severity::Info, "i"));
    /// assert_eq!(p.alarm(0).unwrap().message, "i");
    /// ```
    pub fn alarm(&self, i: usize) -> Option<&Alarm> {
        self.alarms.get(i)
    }

    /// Entry count.
    ///
    /// ```
    /// use martensite::widgets::alarm_panel::AlarmPanel;
    ///
    /// assert_eq!(AlarmPanel::new().count(), 0);
    /// ```
    pub fn count(&self) -> usize {
        self.alarms.len()
    }

    /// Whether the list is empty.
    ///
    /// ```
    /// use martensite::widgets::alarm_panel::AlarmPanel;
    ///
    /// assert!(AlarmPanel::new().is_empty());
    /// ```
    pub fn is_empty(&self) -> bool {
        self.alarms.is_empty()
    }

    /// Number of unacknowledged alarms.
    ///
    /// ```
    /// use martensite::widgets::alarm_panel::{Alarm, AlarmPanel};
    /// use martensite::widgets::banner::Severity;
    ///
    /// let mut p = AlarmPanel::new();
    /// p.push(Alarm::new(Severity::Error, "e"));
    /// assert_eq!(p.unacked_count(), 1);
    /// ```
    pub fn unacked_count(&self) -> usize {
        self.alarms
            .iter()
            .filter(|a| a.state == AlarmState::Active)
            .count()
    }

    /// Acknowledges alarm `i`.
    ///
    /// ```
    /// use martensite::widgets::alarm_panel::{Alarm, AlarmPanel};
    /// use martensite::widgets::banner::Severity;
    ///
    /// let mut p = AlarmPanel::new();
    /// p.push(Alarm::new(Severity::Error, "e"));
    /// p.acknowledge(0);
    /// assert_eq!(p.unacked_count(), 0);
    /// ```
    pub fn acknowledge(&mut self, i: usize) {
        if let Some(a) = self.alarms.get_mut(i) {
            a.state = AlarmState::Acknowledged;
        }
    }

    /// Acknowledges every active alarm.
    ///
    /// ```
    /// use martensite::widgets::alarm_panel::{Alarm, AlarmPanel};
    /// use martensite::widgets::banner::Severity;
    ///
    /// let mut p = AlarmPanel::new();
    /// p.push(Alarm::new(Severity::Warning, "w"));
    /// p.push(Alarm::new(Severity::Error, "e"));
    /// p.acknowledge_all();
    /// assert_eq!(p.unacked_count(), 0);
    /// ```
    pub fn acknowledge_all(&mut self) {
        for a in &mut self.alarms {
            a.state = AlarmState::Acknowledged;
        }
    }

    /// Removes alarm `i` from the list.
    ///
    /// ```
    /// use martensite::widgets::alarm_panel::{Alarm, AlarmPanel};
    /// use martensite::widgets::banner::Severity;
    ///
    /// let mut p = AlarmPanel::new();
    /// p.push(Alarm::new(Severity::Info, "i"));
    /// p.clear(0);
    /// assert!(p.is_empty());
    /// ```
    pub fn clear(&mut self, i: usize) {
        if i < self.alarms.len() {
            self.alarms.remove(i);
        }
    }

    /// Drains the index of the last alarm acked via its `ACK` chip.
    ///
    /// ```
    /// use martensite::widgets::alarm_panel::AlarmPanel;
    ///
    /// assert_eq!(AlarmPanel::new().take_acked(), None);
    /// ```
    pub fn take_acked(&mut self) -> Option<usize> {
        self.acked.take()
    }

    /// Sets whether the alarm data is pending (builder version).
    ///
    /// A pending panel paints the explicit *data unknown* treatment —
    /// each row keeps its slot but loses severity hue and its `ACK`
    /// chip, carrying a `?` mark instead — and reports `status
    /// unknown` to assistive tech; it never shimmers.
    ///
    /// ```
    /// use martensite::widgets::alarm_panel::AlarmPanel;
    ///
    /// let p = AlarmPanel::new().loading(true);
    /// assert!(p.is_loading());
    /// ```
    pub fn loading(mut self, loading: bool) -> Self {
        self.loading = loading;
        self
    }

    /// Sets whether the alarm data is pending (mutable version) —
    /// the `Bound::push` seam for a stale alarm feed.
    ///
    /// ```
    /// use martensite::widgets::alarm_panel::AlarmPanel;
    ///
    /// let mut p = AlarmPanel::new();
    /// p.set_loading(true);
    /// assert!(p.is_loading());
    /// p.set_loading(false);
    /// assert!(!p.is_loading());
    /// ```
    pub fn set_loading(&mut self, loading: bool) {
        self.loading = loading;
    }

    /// Whether the alarm data is pending.
    ///
    /// ```
    /// use martensite::widgets::alarm_panel::AlarmPanel;
    ///
    /// assert!(!AlarmPanel::new().is_loading());
    /// ```
    pub fn is_loading(&self) -> bool {
        self.loading
    }

    /// Content height.
    fn content_h(&self) -> f32 {
        let s = self.scale;
        self.alarms.len() as f32 * (ROW_PT + PAD_PT) * s + PAD_PT * s
    }

    /// Max scroll offset.
    fn max_scroll(&self) -> f32 {
        (self.content_h() - self.bounds.height()).max(0.0)
    }
}

impl Widget for AlarmPanel {
    #[cfg(feature = "devtools-timemachine")]
    fn as_any_mut(&mut self) -> Option<&mut dyn std::any::Any> {
        Some(self)
    }

    /// Alarm annunciator — reserved-hue paint is the alarm channel
    /// here, declared via the `@alarm` marker for the lint lineage.
    fn debug_name(&self) -> &'static str {
        "AlarmPanel@alarm"
    }

    fn measure(&mut self, cx: &mut LayoutContext, constraints: LayoutConstraints) -> Vec2 {
        Vec2::new(
            cx.pt(W_PT).min(constraints.max_size.x.max(0.0)),
            cx.pt(H_PT).min(constraints.max_size.y.max(0.0)),
        )
    }

    fn min_render(&self) -> RenderMinimum {
        RenderMinimum::new(Vec2::new(180.0, 60.0)).with_policy(UnderflowPolicy::Lint)
    }

    fn layout(&mut self, cx: &mut LayoutContext, bounds: Rect) {
        self.bounds = bounds;
        self.scale = cx.scale;
        self.scroll = self.scroll.clamp(0.0, self.max_scroll());
    }

    fn accessibility(&self, node: &mut AccessKitNode) {
        node.set_role(accesskit::Role::List);
        if self.loading {
            // A pending panel reports "unknown" — never a stale
            // unacked count — matching the `?` marks on screen.
            node.set_label(format!("{} — status unknown", self.label));
            return;
        }
        node.set_label(format!("{} — {} active", self.label, self.unacked_count()));
    }

    fn event(&mut self, cx: &mut EventContext) -> EventResponse {
        // Pending rows are placeholders — swallow input so a stale
        // `ack_hits` rect can't acknowledge an alarm whose state is
        // unknown.
        if self.loading {
            return EventResponse::Handled;
        }
        match cx.event {
            WidgetEvent::Scroll { position, delta } => {
                if self.bounds.contains(*position) {
                    self.scroll = (self.scroll - delta.y).clamp(0.0, self.max_scroll());
                    return EventResponse::RequestRepaint;
                }
                EventResponse::Ignored
            }
            WidgetEvent::PointerPressed {
                button: PointerButton::Primary,
                position,
                ..
            } => {
                if !self.bounds.contains(*position) {
                    return EventResponse::Ignored;
                }
                let hits = self.ack_hits.lock();
                if let Some((i, _)) = hits.iter().find(|(_, r)| r.contains(*position)) {
                    let i = *i;
                    drop(hits);
                    self.acknowledge(i);
                    self.acked = Some(i);
                    return EventResponse::RequestRepaint;
                }
                EventResponse::Handled
            }
            _ => EventResponse::Ignored,
        }
    }

    fn tick(&mut self, dt: std::time::Duration) -> bool {
        // Flash unacknowledged Error edges at ~1.4 Hz.
        let flashing = self
            .alarms
            .iter()
            .any(|a| a.severity == Severity::Error && a.state == AlarmState::Active);
        if !flashing {
            return false;
        }
        self.blink = (self.blink + dt.as_secs_f32() * 1.4) % 1.0;
        true
    }

    fn is_loading(&self) -> bool {
        self.loading
    }

    fn paint_loading(&self, cx: &mut PaintContext, _phase: Option<f32>) {
        // "Data unknown", not shimmer: the list keeps its row slots,
        // but every row is muted — no severity edge (the severity is
        // unknown), no ACK chip (there is nothing safe to
        // acknowledge) — and carries the `?` mark. `_phase` is
        // ignored deliberately; safety widgets never sweep.
        let krect = |r: Rect| {
            kurbo::Rect::new(
                f64::from(r.min_x()),
                f64::from(r.min_y()),
                f64::from(r.max_x()),
                f64::from(r.max_y()),
            )
        };
        cx.list.push_fill_shape(
            krect(self.bounds),
            &martensite_core::shape::Shape::RECT,
            cx.color(TokenKey::SurfaceColor, FACE),
        );
        let painter = crate::text_paint::resolve_painter(&self.text_painter, cx.text_painter);
        let s = self.scale;
        let pad = PAD_PT * s;
        let row_h = ROW_PT * s;
        let msg_sz = 12.0 * s;
        let muted = cx.color(TokenKey::TextMutedColor, MUTED);
        // A stale list keeps its shape; an empty pending list still
        // shows three unknown rows so the panel reads "fetching",
        // not "empty".
        let rows = self.alarms.len().max(3);
        self.ack_hits.lock().clear();
        cx.list.push_clip(krect(self.bounds));
        for i in 0..rows {
            let y = self.bounds.min_y() + pad + i as f32 * (row_h + pad) - self.scroll;
            if y + row_h < self.bounds.min_y() || y > self.bounds.max_y() {
                continue;
            }
            let r = Rect::new(
                self.bounds.min_x() + pad,
                y,
                self.bounds.width() - pad * 2.0,
                row_h,
            );
            let kr = krect(r);
            cx.list.push_fill_shape(
                kr,
                &martensite_core::shape::Shape::squircle(5.0 * s),
                cx.color(TokenKey::BackgroundColor, ROW_ACKED),
            );
            // Muted edge — a pending row must never borrow a live
            // severity hue.
            let edge_r = Rect::new(r.min_x(), y, EDGE_PT * s, row_h);
            cx.list.push_fill_shape(
                krect(edge_r),
                &martensite_core::shape::Shape::RECT,
                cx.color(TokenKey::DividerColor, [110, 112, 120, 255]),
            );
            // The stale message still shows, dimmed — ISA-101
            // questionable data is the last value *marked* unknown,
            // not erased. Empty slots get an em-dash placeholder.
            let tx = r.min_x() + EDGE_PT * s + 8.0 * s;
            let msg = self
                .alarms
                .get(i)
                .map(|a| a.message.as_str())
                .unwrap_or("—");
            crate::text_paint::paint_label_clipped(
                painter,
                cx.list,
                kr,
                kurbo::Point::new(f64::from(tx), f64::from(y + 5.0 * s)),
                msg,
                msg_sz,
                muted,
            );
            // The `?` mark where the ACK chip would sit — the
            // redundant non-color unknown channel.
            let gr = cx.pt(7.0);
            let center = Vec2::new(r.max_x() - 22.0 * s, y + row_h / 2.0);
            crate::widgets::status_dot::paint_unknown_glyph(cx.list, center, gr, muted);
        }
        cx.list.pop_clip();
    }

    fn paint(&self, cx: &mut PaintContext) {
        let krect = |r: Rect| {
            kurbo::Rect::new(
                f64::from(r.min_x()),
                f64::from(r.min_y()),
                f64::from(r.max_x()),
                f64::from(r.max_y()),
            )
        };
        cx.list.push_fill_shape(
            krect(self.bounds),
            &martensite_core::shape::Shape::RECT,
            cx.color(TokenKey::SurfaceColor, FACE),
        );
        let painter = crate::text_paint::resolve_painter(&self.text_painter, cx.text_painter);
        let s = self.scale;
        let pad = PAD_PT * s;
        let row_h = ROW_PT * s;
        let msg_sz = 12.0 * s;
        let src_sz = 12.0 * s;
        let edge_on = self.blink < 0.55;
        let mut hits = self.ack_hits.lock();
        hits.clear();
        cx.list.push_clip(krect(self.bounds));
        for (i, a) in self.alarms.iter().enumerate() {
            let y = self.bounds.min_y() + pad + i as f32 * (row_h + pad) - self.scroll;
            if y + row_h < self.bounds.min_y() || y > self.bounds.max_y() {
                continue;
            }
            let r = Rect::new(
                self.bounds.min_x() + pad,
                y,
                self.bounds.width() - pad * 2.0,
                row_h,
            );
            let kr = krect(r);
            // Text clip stops before the trailing ack affordance —
            // the message must truncate, not run under the chip.
            let text_clip = kurbo::Rect::new(kr.x0, kr.y0, kr.x1 - f64::from(50.0 * s), kr.y1);
            let acked = a.state == AlarmState::Acknowledged;
            cx.list.push_fill_shape(
                kr,
                &martensite_core::shape::Shape::squircle(5.0 * s),
                cx.color(
                    TokenKey::BackgroundColor,
                    if acked { ROW_ACKED } else { ROW },
                ),
            );
            // Severity edge — unacked Errors flash.
            let mut edge = accent(a.severity);
            if a.severity == Severity::Error && !acked && !edge_on {
                edge = [edge[0], edge[1], edge[2], 90];
            }
            let edge_r = Rect::new(r.min_x(), y, EDGE_PT * s, row_h);
            cx.list
                .push_fill_shape(krect(edge_r), &martensite_core::shape::Shape::RECT, edge);
            let tx = r.min_x() + EDGE_PT * s + 8.0 * s;
            let tc = if acked {
                cx.color(TokenKey::TextMutedColor, ACKED_TEXT)
            } else {
                cx.color(TokenKey::TextColor, TEXT)
            };
            // Elide, then clip: a run painted past its clip edge is a
            // paint-audit finding, `…` reads as an intentional
            // truncation.
            let msg = crate::text_paint::elide_label(
                painter,
                s,
                &a.message,
                msg_sz / s,
                text_clip.width() as f32 - (tx - r.min_x()),
            );
            crate::text_paint::paint_label_clipped(
                painter,
                cx.list,
                text_clip,
                kurbo::Point::new(f64::from(tx), f64::from(y + 5.0 * s)),
                &msg,
                msg_sz,
                tc,
            );
            if !a.source.is_empty() {
                let src = crate::text_paint::elide_label(
                    painter,
                    s,
                    &a.source,
                    src_sz / s,
                    text_clip.width() as f32 - (tx - r.min_x()),
                );
                crate::text_paint::paint_label_clipped(
                    painter,
                    cx.list,
                    text_clip,
                    kurbo::Point::new(f64::from(tx), f64::from(y + 5.0 * s + msg_sz * 1.5)),
                    &src,
                    src_sz,
                    cx.color(TokenKey::TextMutedColor, MUTED),
                );
            }
            if acked {
                let ink = cx.color(TokenKey::SuccessColor, [92, 200, 120, 255]);
                let side = msg_sz;
                let icon_ok = crate::icons::builtin()
                    .lookup("status.check")
                    .is_some_and(|d| {
                        crate::widgets::morph_icon::paint_icon_d(
                            cx.list,
                            Rect::new(r.max_x() - 18.0 * s, y + (row_h - side) / 2.0, side, side),
                            d,
                            s,
                            ink,
                        )
                    });
                if !icon_ok {
                    crate::text_paint::paint_label(
                        painter,
                        cx.list,
                        kurbo::Point::new(
                            f64::from(r.max_x() - 18.0 * s),
                            crate::text_paint::centered_label_top(
                                painter,
                                y + (row_h) / 2.0,
                                "✓",
                                msg_sz,
                            ),
                        ),
                        "✓",
                        msg_sz,
                        ink,
                    );
                }
            } else {
                let chip = Rect::new(
                    r.max_x() - 44.0 * s,
                    y + (row_h - 20.0 * s) / 2.0,
                    38.0 * s,
                    20.0 * s,
                );
                cx.list.push_fill_shape(
                    krect(chip),
                    &martensite_core::shape::Shape::squircle(4.0 * s),
                    cx.color(TokenKey::RaisedColor, ACK_CHIP),
                );
                crate::text_paint::paint_label(
                    painter,
                    cx.list,
                    kurbo::Point::new(
                        f64::from(chip.min_x() + 7.0 * s),
                        f64::from(chip.min_y() + 4.0 * s),
                    ),
                    "ACK",
                    src_sz,
                    cx.color(TokenKey::TextColor, ACK_TEXT),
                );
                hits.push((i, chip));
            }
        }
        cx.list.pop_clip();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use martensite_core::HotNode;
    use martensite_core::PaintList;

    fn laid_out(w: &mut AlarmPanel, wd: f32, h: f32) {
        let mut hot = HotNode::default();
        let mut cx = LayoutContext {
            hot: &mut hot,
            scale: 1.0,
        };
        w.measure(
            &mut cx,
            LayoutConstraints {
                min_size: Vec2::ZERO,
                max_size: Vec2::new(wd, h),
            },
        );
        w.layout(&mut cx, Rect::new(0.0, 0.0, wd, h));
    }

    fn painted(w: &AlarmPanel) {
        let theme = martensite_theme::Theme::new("test");
        let mut list = PaintList::new();
        let mut cx = PaintContext {
            list: &mut list,
            bounds: w.bounds,
            scale: 1.0,
            theme: &theme,
            text_painter: None,
        };
        w.paint(&mut cx);
    }

    #[test]
    fn lifecycle() {
        let mut p = AlarmPanel::new();
        p.push(Alarm::new(Severity::Info, "a"));
        p.push(Alarm::new(Severity::Error, "b"));
        assert_eq!(p.unacked_count(), 2);
        p.acknowledge(0);
        assert_eq!(p.unacked_count(), 1);
        p.acknowledge_all();
        assert_eq!(p.unacked_count(), 0);
    }

    #[test]
    fn ack_chip_click() {
        let mut p = AlarmPanel::new();
        p.push(Alarm::new(Severity::Warning, "filter ΔP").source("DP-12"));
        laid_out(&mut p, 320.0, 240.0);
        painted(&p);
        let chip = p.ack_hits.lock()[0].1;
        p.event(&mut EventContext {
            event: &WidgetEvent::PointerPressed {
                button: PointerButton::Primary,
                position: Vec2::new(
                    (chip.min_x() + chip.max_x()) / 2.0,
                    (chip.min_y() + chip.max_y()) / 2.0,
                ),
                count: 1,
            },
            bounds: p.bounds,
            scale: 1.0,
        });
        assert_eq!(p.alarm(0).unwrap().state, AlarmState::Acknowledged);
        assert_eq!(p.take_acked(), Some(0));
    }

    #[test]
    fn acked_rows_have_no_chip() {
        let mut p = AlarmPanel::new();
        p.push(Alarm::new(Severity::Error, "e"));
        p.acknowledge(0);
        laid_out(&mut p, 320.0, 240.0);
        painted(&p);
        assert!(p.ack_hits.lock().is_empty());
    }

    #[test]
    fn scroll_clamps() {
        let mut p = AlarmPanel::new();
        for i in 0..20 {
            p.push(Alarm::new(Severity::Info, format!("a{i}")));
        }
        laid_out(&mut p, 320.0, 240.0);
        p.event(&mut EventContext {
            event: &WidgetEvent::Scroll {
                position: Vec2::new(160.0, 120.0),
                delta: Vec2::new(0.0, -9999.0),
            },
            bounds: p.bounds,
            scale: 1.0,
        });
        assert_eq!(p.scroll, p.max_scroll());
    }

    fn painted_loading(w: &AlarmPanel, phase: Option<f32>) -> PaintList {
        let theme = martensite_theme::Theme::new("test");
        let mut list = PaintList::new();
        let mut cx = PaintContext {
            list: &mut list,
            bounds: w.bounds,
            scale: 1.0,
            theme: &theme,
            text_painter: None,
        };
        w.paint_loading(&mut cx, phase);
        list
    }

    #[test]
    fn loading_flag_round_trip() {
        let mut p = AlarmPanel::new();
        p.push(Alarm::new(Severity::Error, "e"));
        assert!(!p.is_loading());
        assert!(!<AlarmPanel as Widget>::is_loading(&p));
        p.set_loading(true);
        assert!(p.is_loading());
        assert!(<AlarmPanel as Widget>::is_loading(&p));
        p.set_loading(false);
        assert!(!p.is_loading());
    }

    #[test]
    fn loading_a11y_reports_unknown_not_stale_count() {
        let mut p = AlarmPanel::new().label("Line 2").loading(true);
        p.push(Alarm::new(Severity::Error, "e"));
        let mut node = AccessKitNode::new(accesskit::Role::Unknown);
        p.accessibility(&mut node);
        // "status unknown" — never a stale "1 active" reading.
        assert_eq!(node.label(), Some("Line 2 — status unknown"));
    }

    #[test]
    fn loading_paint_is_unknown_not_shimmer() {
        use martensite_core::PaintCommand;
        let mut p = AlarmPanel::new().loading(true);
        p.push(Alarm::new(Severity::Error, "Tank 4 overpressure").source("PT-104"));
        p.push(Alarm::new(Severity::Warning, "Filter ΔP high"));
        laid_out(&mut p, 320.0, 240.0);
        // Even with an animated phase a safety panel must never emit
        // a shimmer band.
        let list = painted_loading(&p, Some(0.5));
        assert!(!list.commands.iter().any(|c| matches!(
            c,
            PaintCommand::FillLinearGradient(..) | PaintCommand::FillLinearGradientPath(..)
        )));
        // No row may borrow a live severity hue or the success green —
        // pending data must never read as a real alarm state.
        for hue in [INFO, WARN, ERR, [92, 200, 120, 255]] {
            assert!(!list.commands.iter().any(|c| matches!(
                c,
                PaintCommand::FillRect(_, c) | PaintCommand::FillPath(_, c) if *c == hue
            )));
        }
        // Stale messages still show, dimmed — questionable data keeps
        // its shape (ISA-101).
        let texts: Vec<&str> = list
            .commands
            .iter()
            .filter_map(|c| match c {
                PaintCommand::DrawText(_, t, _, _) => Some(t.as_str()),
                _ => None,
            })
            .collect();
        assert!(texts.contains(&"Tank 4 overpressure"));
        assert!(texts.contains(&"Filter ΔP high"));
        // Each row carries the `?` stroke where the ACK chip sat —
        // the two stale rows plus the third "fetching" slot.
        let marks = list
            .commands
            .iter()
            .filter(|c| matches!(c, PaintCommand::StrokePath(..)))
            .count();
        assert_eq!(marks, 3);
        // No ack affordances while the data is unknown.
        assert!(p.ack_hits.lock().is_empty());
    }

    #[test]
    fn loading_empty_panel_shows_unknown_rows() {
        use martensite_core::PaintCommand;
        let mut p = AlarmPanel::new().loading(true);
        laid_out(&mut p, 320.0, 240.0);
        let list = painted_loading(&p, None);
        // Three placeholder rows — "fetching", not "empty".
        let marks = list
            .commands
            .iter()
            .filter(|c| matches!(c, PaintCommand::StrokePath(..)))
            .count();
        assert_eq!(marks, 3);
    }

    #[test]
    fn loading_swallows_ack_presses() {
        let mut p = AlarmPanel::new().loading(true);
        p.push(Alarm::new(Severity::Warning, "w"));
        laid_out(&mut p, 320.0, 240.0);
        // Seed stale chip rects as a non-loading frame would, then
        // confirm a press cannot ack while the data is unknown.
        painted(&p);
        assert!(!p.ack_hits.lock().is_empty());
        let chip = p.ack_hits.lock()[0].1;
        let press = WidgetEvent::PointerPressed {
            button: PointerButton::Primary,
            position: Vec2::new(
                (chip.min_x() + chip.max_x()) / 2.0,
                (chip.min_y() + chip.max_y()) / 2.0,
            ),
            count: 1,
        };
        assert_eq!(
            p.event(&mut EventContext {
                event: &press,
                bounds: p.bounds,
                scale: 1.0,
            }),
            EventResponse::Handled
        );
        assert_eq!(p.alarm(0).unwrap().state, AlarmState::Active);
        assert_eq!(p.take_acked(), None);
    }
}
