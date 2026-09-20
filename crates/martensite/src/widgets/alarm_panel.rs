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
        node.set_label(format!("{} — {} active", self.label, self.unacked_count()));
    }

    fn event(&mut self, cx: &mut EventContext) -> EventResponse {
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
        let msg_sz = 11.5 * s;
        let src_sz = 9.5 * s;
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
            let acked = a.state == AlarmState::Acknowledged;
            cx.list.push_fill_shape(
                kr,
                &martensite_core::shape::Shape::rounded(5.0 * s),
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
            crate::text_paint::paint_label_clipped(
                painter,
                cx.list,
                kr,
                kurbo::Point::new(f64::from(tx), f64::from(y + 5.0 * s)),
                &a.message,
                msg_sz,
                tc,
            );
            if !a.source.is_empty() {
                crate::text_paint::paint_label_clipped(
                    painter,
                    cx.list,
                    kr,
                    kurbo::Point::new(f64::from(tx), f64::from(y + 5.0 * s + msg_sz * 1.5)),
                    &a.source,
                    src_sz,
                    cx.color(TokenKey::TextMutedColor, MUTED),
                );
            }
            if acked {
                crate::text_paint::paint_label(
                    painter,
                    cx.list,
                    kurbo::Point::new(
                        f64::from(r.max_x() - 18.0 * s),
                        f64::from(y + (row_h - msg_sz * 1.4) / 2.0),
                    ),
                    "✓",
                    msg_sz,
                    cx.color(TokenKey::SuccessColor, [92, 200, 120, 255]),
                );
            } else {
                let chip = Rect::new(
                    r.max_x() - 44.0 * s,
                    y + (row_h - 20.0 * s) / 2.0,
                    38.0 * s,
                    20.0 * s,
                );
                cx.list.push_fill_shape(
                    krect(chip),
                    &martensite_core::shape::Shape::rounded(4.0 * s),
                    cx.color(TokenKey::BorderColor, ACK_CHIP),
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
}
