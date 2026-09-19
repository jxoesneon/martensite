//! `Waterfall` — the running-total bridge chart (McKinsey /
//! finance waterfall idiom).
//!
//! Each entry is either a delta (floating column spanning the
//! previous to the new cumulative total) or a total (column from
//! zero). Deltas paint green when positive and red when negative;
//! totals paint accent. Dashed connectors link consecutive tops.
//! Names and values label each column; hovering parks the index
//! in [`Waterfall::take_hovered`].
//!
//! # Examples
//!
//! ```
//! use martensite::widgets::waterfall::Waterfall;
//!
//! let w = Waterfall::new()
//!     .total("Start", 100.0)
//!     .delta("Sales", 40.0)
//!     .delta("Costs", -25.0)
//!     .total("End", 115.0);
//! assert_eq!(w.entry_count(), 4);
//! ```

use accesskit::Node as AccessKitNode;
use glam::Vec2;
use martensite_core::{
    EventContext, EventResponse, LayoutConstraints, LayoutContext, PaintContext, Rect,
    RenderMinimum, UnderflowPolicy, Widget, WidgetEvent,
};
use martensite_theme::TokenKey;

use crate::text_paint::SharedTextPainter;

const WIDTH_PT: f32 = 360.0;
const HEIGHT_PT: f32 = 200.0;
const AXIS_PT: f32 = 16.0;
const LABEL_PT: f32 = 14.0;

const TRACK: [u8; 4] = [48, 48, 52, 255];
const GRID: [u8; 4] = [70, 70, 76, 255];
const UP: [u8; 4] = [110, 180, 130, 255];
const DOWN: [u8; 4] = [210, 110, 90, 255];
const TOTAL: [u8; 4] = [90, 140, 220, 255];
const FG: [u8; 4] = [230, 230, 235, 255];
const MUTED: [u8; 4] = [140, 140, 148, 255];

/// One waterfall entry.
#[derive(Clone, Debug, PartialEq)]
pub enum WaterfallEntry {
    /// Floating column from the running total to total+delta.
    Delta {
        /// Entry name (bottom label).
        name: String,
        /// Signed change applied to the running total.
        delta: f32,
    },
    /// Full column from zero to `value`.
    Total {
        /// Entry name (bottom label).
        name: String,
        /// Absolute value; resets the running total.
        value: f32,
    },
}

impl WaterfallEntry {
    /// The entry's label.
    ///
    /// ```
    /// use martensite::widgets::waterfall::WaterfallEntry;
    ///
    /// let e = WaterfallEntry::Delta { name: "x".into(), delta: 1.0 };
    /// assert_eq!(e.name(), "x");
    /// ```
    pub fn name(&self) -> &str {
        match self {
            Self::Delta { name, .. } | Self::Total { name, .. } => name,
        }
    }
}

/// A running-total bridge chart — see the module docs.
///
/// ```
/// use martensite::widgets::waterfall::Waterfall;
///
/// assert_eq!(Waterfall::new().entry_count(), 0);
/// ```
pub struct Waterfall {
    /// Accessibility label.
    pub label: String,
    entries: Vec<WaterfallEntry>,
    hovered: Option<usize>,
    pending_hover: Option<usize>,
    bounds: Rect,
    text_painter: Option<SharedTextPainter>,
}

impl Default for Waterfall {
    fn default() -> Self {
        Self::new()
    }
}

impl Waterfall {
    /// Creates an empty chart.
    ///
    /// ```
    /// use martensite::widgets::waterfall::Waterfall;
    ///
    /// assert_eq!(Waterfall::new().entry_count(), 0);
    /// ```
    pub fn new() -> Self {
        Self {
            label: "Waterfall".to_string(),
            entries: Vec::new(),
            hovered: None,
            pending_hover: None,
            bounds: Rect::new(0.0, 0.0, 0.0, 0.0),
            text_painter: None,
        }
    }

    /// Accessibility label.
    ///
    /// ```
    /// use martensite::widgets::waterfall::Waterfall;
    ///
    /// let w = Waterfall::new().label("PnL");
    /// assert_eq!(w.label, "PnL");
    /// ```
    pub fn label(mut self, label: impl Into<String>) -> Self {
        self.label = label.into();
        self
    }

    /// Appends a signed delta entry.
    ///
    /// ```
    /// use martensite::widgets::waterfall::Waterfall;
    ///
    /// let w = Waterfall::new().delta("Gain", 25.0);
    /// assert_eq!(w.entry_count(), 1);
    /// ```
    pub fn delta(mut self, name: impl Into<String>, delta: f32) -> Self {
        self.entries.push(WaterfallEntry::Delta {
            name: name.into(),
            delta,
        });
        self
    }

    /// Appends an absolute total entry (resets the running sum).
    ///
    /// ```
    /// use martensite::widgets::waterfall::Waterfall;
    ///
    /// let w = Waterfall::new().total("Start", 100.0);
    /// assert_eq!(w.entry_count(), 1);
    /// ```
    pub fn total(mut self, name: impl Into<String>, value: f32) -> Self {
        self.entries.push(WaterfallEntry::Total {
            name: name.into(),
            value,
        });
        self
    }

    /// Explicit painter override — see [`crate::text_paint`].
    ///
    /// ```
    /// use martensite::widgets::waterfall::Waterfall;
    /// use martensite::text_paint::shared_painter;
    ///
    /// let w = Waterfall::new().with_text_painter(shared_painter());
    /// assert_eq!(w.entry_count(), 0);
    /// ```
    pub fn with_text_painter(mut self, painter: SharedTextPainter) -> Self {
        self.text_painter = Some(painter);
        self
    }

    /// Entry count.
    ///
    /// ```
    /// use martensite::widgets::waterfall::Waterfall;
    ///
    /// assert_eq!(Waterfall::new().entry_count(), 0);
    /// ```
    pub fn entry_count(&self) -> usize {
        self.entries.len()
    }

    /// Entry list.
    ///
    /// ```
    /// use martensite::widgets::waterfall::Waterfall;
    ///
    /// let w = Waterfall::new().delta("x", 1.0);
    /// assert_eq!(w.entries()[0].name(), "x");
    /// ```
    pub fn entries(&self) -> &[WaterfallEntry] {
        &self.entries
    }

    /// Drains the column index hovered since the last drain.
    ///
    /// ```
    /// use martensite::widgets::waterfall::Waterfall;
    ///
    /// let mut w = Waterfall::new();
    /// assert!(w.take_hovered().is_none());
    /// ```
    pub fn take_hovered(&mut self) -> Option<usize> {
        self.pending_hover.take()
    }

    /// Chart region (above the label/axis strips).
    fn chart_rect(&self, scale: f32) -> Rect {
        Rect::new(
            self.bounds.min_x(),
            self.bounds.min_y(),
            self.bounds.width(),
            (self.bounds.height() - (AXIS_PT + LABEL_PT) * scale).max(0.0),
        )
    }

    /// `(lo, hi, tops)` — value range plus each column's `(bottom,
    /// top)` value pair.
    fn spans(&self) -> (f32, f32, Vec<(f32, f32)>) {
        let mut run = 0.0f32;
        let mut lo = 0.0f32;
        let mut hi = 0.0f32;
        let mut tops = Vec::with_capacity(self.entries.len());
        for e in &self.entries {
            match *e {
                WaterfallEntry::Delta { delta, .. } => {
                    let next = run + delta;
                    tops.push((run.min(next), run.max(next)));
                    run = next;
                }
                WaterfallEntry::Total { value, .. } => {
                    tops.push((0.0f32.min(value), 0.0f32.max(value)));
                    run = value;
                }
            }
            let (b, t) = tops.last().copied().unwrap();
            lo = lo.min(b);
            hi = hi.max(t);
        }
        if lo >= hi {
            hi = lo + 1.0;
        }
        (lo, hi, tops)
    }

    /// Column index at a device-space point.
    fn col_at(&self, p: Vec2, scale: f32) -> Option<usize> {
        let chart = self.chart_rect(scale);
        if !chart.contains(p) || self.entries.is_empty() {
            return None;
        }
        let w = chart.width() / self.entries.len() as f32;
        Some(((p.x - chart.min_x()) / w) as usize).map(|i| i.min(self.entries.len() - 1))
    }
}

impl Widget for Waterfall {
    fn measure(&mut self, cx: &mut LayoutContext, constraints: LayoutConstraints) -> Vec2 {
        Vec2::new(
            cx.pt(WIDTH_PT).min(constraints.max_size.x.max(0.0)),
            cx.pt(HEIGHT_PT).min(constraints.max_size.y.max(0.0)),
        )
    }

    fn min_render(&self) -> RenderMinimum {
        RenderMinimum::new(Vec2::new(120.0, 64.0)).with_policy(UnderflowPolicy::Lint)
    }

    fn layout(&mut self, _cx: &mut LayoutContext, bounds: Rect) {
        self.bounds = bounds;
    }

    fn accessibility(&self, node: &mut AccessKitNode) {
        node.set_role(accesskit::Role::Image);
        node.set_label(self.label.clone());
        node.set_description(format!("{} entries", self.entries.len()));
    }

    fn event(&mut self, cx: &mut EventContext) -> EventResponse {
        match cx.event {
            WidgetEvent::PointerMoved { position } => {
                let hit = self.col_at(*position, cx.scale);
                if hit != self.hovered {
                    self.hovered = hit;
                    if hit.is_some() {
                        self.pending_hover = hit;
                    }
                    return EventResponse::RequestRepaint;
                }
                EventResponse::Ignored
            }
            _ => EventResponse::Ignored,
        }
    }

    fn paint(&self, cx: &mut PaintContext) {
        let f = |r: Rect| {
            kurbo::Rect::new(
                f64::from(r.min_x()),
                f64::from(r.min_y()),
                f64::from(r.max_x()),
                f64::from(r.max_y()),
            )
        };
        cx.list
            .push_fill_rect(f(self.bounds), cx.color(TokenKey::SurfaceColor, TRACK));
        if self.entries.is_empty() {
            return;
        }
        let painter = crate::text_paint::resolve_painter(&self.text_painter, cx.text_painter);
        let grid = cx.color(TokenKey::DividerColor, GRID);
        let up = cx.color(TokenKey::SuccessColor, UP);
        let down = cx.color(TokenKey::ErrorColor, DOWN);
        let total_c = cx.color(TokenKey::AccentColor, TOTAL);
        let fg = cx.color(TokenKey::TextColor, FG);
        let muted = cx.color(TokenKey::TextMutedColor, MUTED);
        let chart = self.chart_rect(cx.scale);
        let (lo, hi, tops) = self.spans();
        let y_of = |v: f32| chart.max_y() - (v - lo) / (hi - lo).max(0.001) * chart.height();
        let n = self.entries.len();
        let slot_w = chart.width() / n as f32;
        let bar_w = slot_w * 0.62;
        let size = 9.0 * cx.scale;

        // Baseline + quarter grid.
        for i in 0..=4 {
            let v = lo + (hi - lo) * i as f32 / 4.0;
            let y = y_of(v);
            let mut l = kurbo::BezPath::new();
            l.move_to((f64::from(chart.min_x()), f64::from(y)));
            l.line_to((f64::from(chart.max_x()), f64::from(y)));
            cx.list.push_stroke_path(l, cx.pt(0.5), grid);
        }

        for (i, (e, (b, t))) in self.entries.iter().zip(&tops).enumerate() {
            let x = chart.min_x() + i as f32 * slot_w + (slot_w - bar_w) / 2.0;
            let (is_delta, delta) = match e {
                WaterfallEntry::Delta { delta, .. } => (true, *delta),
                WaterfallEntry::Total { value, .. } => (false, *value),
            };
            let mut color = if !is_delta {
                total_c
            } else if delta >= 0.0 {
                up
            } else {
                down
            };
            if self.hovered == Some(i) {
                color = [
                    color[0].saturating_add(30),
                    color[1].saturating_add(30),
                    color[2].saturating_add(30),
                    255,
                ];
            }
            let y0 = y_of(*t);
            let y1 = y_of(*b);
            let bar = Rect::new(x, y0, bar_w, (y1 - y0).max(1.5));
            cx.list.push_fill_shape(
                f(bar),
                &martensite_core::shape::Shape::rounded(cx.pt(1.5)),
                color,
            );

            // Dashed connector to the next column at this top.
            if i + 1 < n {
                let nx = chart.min_x() + (i + 1) as f32 * slot_w + (slot_w - bar_w) / 2.0;
                let mut conn = kurbo::BezPath::new();
                let y = y_of(*t);
                conn.move_to((f64::from(x + bar_w), f64::from(y)));
                conn.line_to((f64::from(nx), f64::from(y)));
                cx.list.push_stroke_path(conn, cx.pt(0.75), muted);
            }

            // Name + value labels.
            let name = e.name();
            let nw = painter
                .and_then(|p| p.measure_text(name, size))
                .unwrap_or(name.chars().count() as f32 * size * 0.55);
            crate::text_paint::paint_label_clipped(
                painter,
                cx.list,
                f(Rect::new(
                    chart.min_x() + i as f32 * slot_w,
                    chart.max_y() + cx.pt(2.0),
                    slot_w,
                    LABEL_PT * cx.scale,
                )),
                kurbo::Point::new(
                    f64::from(x + (bar_w - nw) / 2.0),
                    f64::from(chart.max_y() + cx.pt(4.0)),
                ),
                name,
                size,
                if self.hovered == Some(i) { fg } else { muted },
            );
            let val = if is_delta {
                format!("{delta:+.0}")
            } else {
                format!("{delta:.0}")
            };
            let vw = painter
                .and_then(|p| p.measure_text(&val, size))
                .unwrap_or(val.chars().count() as f32 * size * 0.6);
            crate::text_paint::paint_label_clipped(
                painter,
                cx.list,
                f(Rect::new(
                    chart.min_x() + i as f32 * slot_w,
                    self.bounds.max_y() - AXIS_PT * cx.scale,
                    slot_w,
                    AXIS_PT * cx.scale,
                )),
                kurbo::Point::new(
                    f64::from(x + (bar_w - vw) / 2.0),
                    f64::from(self.bounds.max_y() - size * 1.3),
                ),
                &val,
                size,
                muted,
            );
        }
    }
}

impl std::fmt::Debug for Waterfall {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Waterfall")
            .field("entries", &self.entries.len())
            .finish()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use martensite_core::HotNode;

    fn laid_out(w: &mut Waterfall, width: f32, h: f32) {
        let mut hot = HotNode::default();
        let mut cx = LayoutContext {
            hot: &mut hot,
            scale: 1.0,
        };
        w.measure(
            &mut cx,
            LayoutConstraints {
                min_size: Vec2::ZERO,
                max_size: Vec2::new(width, h),
            },
        );
        w.layout(&mut cx, Rect::new(0.0, 0.0, width, h));
    }

    #[test]
    fn spans_track_running_total() {
        let w = Waterfall::new()
            .total("Start", 100.0)
            .delta("Up", 40.0)
            .delta("Down", -25.0);
        let (lo, hi, tops) = w.spans();
        assert_eq!(tops[1], (100.0, 140.0));
        assert_eq!(tops[2], (115.0, 140.0));
        assert_eq!(lo, 0.0);
        assert_eq!(hi, 140.0);
    }

    #[test]
    fn total_resets_running_sum() {
        let w = Waterfall::new()
            .delta("a", 50.0)
            .total("T", 20.0)
            .delta("b", 10.0);
        let (_, _, tops) = w.spans();
        assert_eq!(tops[1], (0.0, 20.0));
        assert_eq!(tops[2], (20.0, 30.0));
    }

    #[test]
    fn hover_parks_column() {
        let mut w = Waterfall::new().delta("a", 10.0).delta("b", -5.0);
        laid_out(&mut w, 300.0, 200.0);
        w.event(&mut EventContext {
            event: &WidgetEvent::PointerMoved {
                position: Vec2::new(225.0, 60.0),
            },
            bounds: Rect::new(0.0, 0.0, 300.0, 200.0),
            scale: 1.0,
        });
        assert_eq!(w.take_hovered(), Some(1));
        assert!(w.take_hovered().is_none());
    }

    #[test]
    fn hover_axis_ignored() {
        let mut w = Waterfall::new().delta("a", 10.0);
        laid_out(&mut w, 300.0, 200.0);
        w.event(&mut EventContext {
            event: &WidgetEvent::PointerMoved {
                position: Vec2::new(150.0, 195.0),
            },
            bounds: Rect::new(0.0, 0.0, 300.0, 200.0),
            scale: 1.0,
        });
        assert!(w.take_hovered().is_none());
    }

    #[test]
    fn empty_spans_safe() {
        let w = Waterfall::new();
        let (lo, hi, tops) = w.spans();
        assert!(tops.is_empty());
        assert_eq!((lo, hi), (0.0, 1.0));
    }
}
