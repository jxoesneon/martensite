//! `BoxPlot` — the five-number statistical summary chart (Tukey
//! box-and-whisker / Ant `Box` idiom).
//!
//! Each [`BoxSeries`] carries min, q1, median, q3, and max and
//! paints as a box spanning q1→q3 with a median line and whisker
//! caps to min/max along a shared value axis. Series names list
//! under each box; hovering a series parks its index in
//! [`BoxPlot::take_hovered`].
//!
//! # Examples
//!
//! ```
//! use martensite::widgets::box_plot::{BoxPlot, BoxSeries};
//!
//! let plot = BoxPlot::new().series(BoxSeries::new("A", 10.0, 25.0, 40.0, 60.0, 90.0));
//! assert_eq!(plot.series_count(), 1);
//! ```

use accesskit::Node as AccessKitNode;
use glam::Vec2;
use martensite_core::{
    EventContext, EventResponse, LayoutConstraints, LayoutContext, PaintContext, Rect,
    RenderMinimum, UnderflowPolicy, Widget, WidgetEvent,
};
use martensite_theme::TokenKey;

use crate::text_paint::SharedTextPainter;

const WIDTH_PT: f32 = 300.0;
const HEIGHT_PT: f32 = 180.0;
const AXIS_PT: f32 = 18.0;
const LABEL_PT: f32 = 34.0;

const PALETTE: [[u8; 4]; 6] = [
    [90, 140, 220, 255],
    [110, 180, 130, 255],
    [230, 170, 80, 255],
    [210, 110, 90, 255],
    [150, 110, 200, 255],
    [90, 180, 190, 255],
];
const TRACK: [u8; 4] = [48, 48, 52, 255];
const GRID: [u8; 4] = [70, 70, 76, 255];
const FG: [u8; 4] = [230, 230, 235, 255];
const MUTED: [u8; 4] = [140, 140, 148, 255];

/// One five-number series — min, q1, median, q3, max.
#[derive(Clone, Debug, PartialEq)]
pub struct BoxSeries {
    /// Series name (bottom label).
    pub name: String,
    /// Whisker minimum.
    pub min: f32,
    /// First quartile.
    pub q1: f32,
    /// Median.
    pub median: f32,
    /// Third quartile.
    pub q3: f32,
    /// Whisker maximum.
    pub max: f32,
}

impl BoxSeries {
    /// Creates a series. Values sort into order defensively so a
    /// mis-ordered caller still draws a valid box.
    ///
    /// ```
    /// use martensite::widgets::box_plot::BoxSeries;
    ///
    /// let s = BoxSeries::new("A", 90.0, 10.0, 40.0, 60.0, 25.0);
    /// assert_eq!(s.min, 10.0);
    /// assert_eq!(s.max, 90.0);
    /// ```
    pub fn new(name: impl Into<String>, min: f32, q1: f32, median: f32, q3: f32, max: f32) -> Self {
        let mut v = [min, q1, median, q3, max];
        v.sort_by(f32::total_cmp);
        Self {
            name: name.into(),
            min: v[0],
            q1: v[1],
            median: v[2],
            q3: v[3],
            max: v[4],
        }
    }
}

/// A five-number summary chart — see the module docs.
///
/// ```
/// use martensite::widgets::box_plot::BoxPlot;
///
/// assert_eq!(BoxPlot::new().series_count(), 0);
/// ```
pub struct BoxPlot {
    /// Accessibility label.
    pub label: String,
    series: Vec<BoxSeries>,
    hovered: Option<usize>,
    pending_hover: Option<usize>,
    bounds: Rect,
    text_painter: Option<SharedTextPainter>,
}

impl Default for BoxPlot {
    fn default() -> Self {
        Self::new()
    }
}

impl BoxPlot {
    /// Creates an empty plot.
    ///
    /// ```
    /// use martensite::widgets::box_plot::BoxPlot;
    ///
    /// assert_eq!(BoxPlot::new().series_count(), 0);
    /// ```
    pub fn new() -> Self {
        Self {
            label: "Distribution".to_string(),
            series: Vec::new(),
            hovered: None,
            pending_hover: None,
            bounds: Rect::new(0.0, 0.0, 0.0, 0.0),
            text_painter: None,
        }
    }

    /// Accessibility label.
    ///
    /// ```
    /// use martensite::widgets::box_plot::BoxPlot;
    ///
    /// let p = BoxPlot::new().label("Latency");
    /// assert_eq!(p.label, "Latency");
    /// ```
    pub fn label(mut self, label: impl Into<String>) -> Self {
        self.label = label.into();
        self
    }

    /// Appends a series.
    ///
    /// ```
    /// use martensite::widgets::box_plot::{BoxPlot, BoxSeries};
    ///
    /// let p = BoxPlot::new().series(BoxSeries::new("A", 0.0, 1.0, 2.0, 3.0, 4.0));
    /// assert_eq!(p.series_count(), 1);
    /// ```
    pub fn series(mut self, s: BoxSeries) -> Self {
        self.series.push(s);
        self
    }

    /// Explicit painter override — see [`crate::text_paint`].
    ///
    /// ```
    /// use martensite::widgets::box_plot::BoxPlot;
    /// use martensite::text_paint::shared_painter;
    ///
    /// let p = BoxPlot::new().with_text_painter(shared_painter());
    /// assert_eq!(p.series_count(), 0);
    /// ```
    pub fn with_text_painter(mut self, painter: SharedTextPainter) -> Self {
        self.text_painter = Some(painter);
        self
    }

    /// Series count.
    ///
    /// ```
    /// use martensite::widgets::box_plot::BoxPlot;
    ///
    /// assert_eq!(BoxPlot::new().series_count(), 0);
    /// ```
    pub fn series_count(&self) -> usize {
        self.series.len()
    }

    /// Series list.
    ///
    /// ```
    /// use martensite::widgets::box_plot::{BoxPlot, BoxSeries};
    ///
    /// let p = BoxPlot::new().series(BoxSeries::new("A", 0.0, 1.0, 2.0, 3.0, 4.0));
    /// assert_eq!(p.series_list()[0].name, "A");
    /// ```
    pub fn series_list(&self) -> &[BoxSeries] {
        &self.series
    }

    /// Drains the series index hovered since the last drain.
    ///
    /// ```
    /// use martensite::widgets::box_plot::BoxPlot;
    ///
    /// let mut p = BoxPlot::new();
    /// assert!(p.take_hovered().is_none());
    /// ```
    pub fn take_hovered(&mut self) -> Option<usize> {
        self.pending_hover.take()
    }

    /// Chart region (above the bottom strips).
    fn chart_rect(&self, scale: f32) -> Rect {
        Rect::new(
            self.bounds.min_x(),
            self.bounds.min_y(),
            self.bounds.width(),
            (self.bounds.height() - (AXIS_PT + LABEL_PT) * scale).max(0.0),
        )
    }

    /// Shared value range across all series.
    fn value_range(&self) -> (f32, f32) {
        let lo = self.series.iter().map(|s| s.min).fold(f32::MAX, f32::min);
        let hi = self.series.iter().map(|s| s.max).fold(f32::MIN, f32::max);
        if lo >= hi {
            (0.0, 1.0)
        } else {
            (lo, hi)
        }
    }

    /// Series index at a device-space point.
    fn series_at(&self, p: Vec2, scale: f32) -> Option<usize> {
        let chart = self.chart_rect(scale);
        if !chart.contains(p) || self.series.is_empty() {
            return None;
        }
        let w = chart.width() / self.series.len() as f32;
        let idx = ((p.x - chart.min_x()) / w) as usize;
        Some(idx.min(self.series.len() - 1))
    }
}

impl Widget for BoxPlot {
    fn measure(&mut self, cx: &mut LayoutContext, constraints: LayoutConstraints) -> Vec2 {
        Vec2::new(
            cx.pt(WIDTH_PT).min(constraints.max_size.x.max(0.0)),
            cx.pt(HEIGHT_PT).min(constraints.max_size.y.max(0.0)),
        )
    }

    fn min_render(&self) -> RenderMinimum {
        RenderMinimum::new(Vec2::new(96.0, 64.0)).with_policy(UnderflowPolicy::Lint)
    }

    fn layout(&mut self, _cx: &mut LayoutContext, bounds: Rect) {
        self.bounds = bounds;
    }

    fn accessibility(&self, node: &mut AccessKitNode) {
        node.set_role(accesskit::Role::Image);
        node.set_label(self.label.clone());
        node.set_description(format!("{} series", self.series.len()));
    }

    fn event(&mut self, cx: &mut EventContext) -> EventResponse {
        match cx.event {
            WidgetEvent::PointerMoved { position } => {
                let hit = self.series_at(*position, cx.scale);
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
        if self.series.is_empty() {
            return;
        }
        let painter = crate::text_paint::resolve_painter(&self.text_painter, cx.text_painter);
        let grid = cx.color(TokenKey::DividerColor, GRID);
        let fg = cx.color(TokenKey::TextColor, FG);
        let muted = cx.color(TokenKey::TextMutedColor, MUTED);
        let chart = self.chart_rect(cx.scale);
        if chart.height() <= 0.0 {
            return;
        }
        let (lo, hi) = self.value_range();
        let pad = (hi - lo) * 0.05;
        let (lo, hi) = (lo - pad, hi + pad);
        let y_of = |v: f32| chart.max_y() - (v - lo) / (hi - lo).max(0.001) * chart.height();

        // Quarter grid.
        for i in 0..=4 {
            let v = lo + (hi - lo) * i as f32 / 4.0;
            let y = y_of(v);
            let mut l = kurbo::BezPath::new();
            l.move_to((f64::from(chart.min_x()), f64::from(y)));
            l.line_to((f64::from(chart.max_x()), f64::from(y)));
            cx.list.push_stroke_path(l, cx.pt(0.5), grid);
        }

        let n = self.series.len();
        let slot_w = chart.width() / n as f32;
        let size = 9.0 * cx.scale;
        for (i, s) in self.series.iter().enumerate() {
            let slot_x = chart.min_x() + i as f32 * slot_w;
            let mid_x = slot_x + slot_w / 2.0;
            let box_w = (slot_w * 0.5).min(cx.pt(48.0));
            let mut color = PALETTE[i % PALETTE.len()];
            if self.hovered == Some(i) {
                color = [
                    color[0].saturating_add(30),
                    color[1].saturating_add(30),
                    color[2].saturating_add(30),
                    255,
                ];
            }
            let dim = [color[0], color[1], color[2], 140];

            // Whiskers — vertical line + caps.
            let mut w = kurbo::BezPath::new();
            w.move_to((f64::from(mid_x), f64::from(y_of(s.min))));
            w.line_to((f64::from(mid_x), f64::from(y_of(s.q1))));
            w.move_to((f64::from(mid_x), f64::from(y_of(s.q3))));
            w.line_to((f64::from(mid_x), f64::from(y_of(s.max))));
            let cap = box_w * 0.4;
            for v in [s.min, s.max] {
                let y = y_of(v);
                w.move_to((f64::from(mid_x - cap), f64::from(y)));
                w.line_to((f64::from(mid_x + cap), f64::from(y)));
            }
            cx.list.push_stroke_path(w, cx.pt(1.0), color);

            // Box q1..q3.
            let box_r = Rect::new(
                mid_x - box_w / 2.0,
                y_of(s.q3),
                box_w,
                (y_of(s.q1) - y_of(s.q3)).max(1.0),
            );
            let shape = martensite_core::shape::Shape::rounded(cx.pt(2.0));
            cx.list.push_fill_shape(f(box_r), &shape, dim);
            cx.list
                .push_stroke_shape(f(box_r), &shape, cx.pt(1.0), color);

            // Median line.
            let mut m = kurbo::BezPath::new();
            m.move_to((f64::from(mid_x - box_w / 2.0), f64::from(y_of(s.median))));
            m.line_to((f64::from(mid_x + box_w / 2.0), f64::from(y_of(s.median))));
            cx.list.push_stroke_path(m, cx.pt(1.5), color);

            // Series name under the chart.
            let name_r = Rect::new(
                slot_x,
                chart.max_y() + cx.pt(2.0),
                slot_w,
                LABEL_PT * cx.scale,
            );
            let nw = painter
                .and_then(|p| p.measure_text(&s.name, size))
                .unwrap_or(s.name.chars().count() as f32 * size * 0.55);
            crate::text_paint::paint_label_clipped(
                painter,
                cx.list,
                f(name_r),
                kurbo::Point::new(
                    f64::from(mid_x - nw / 2.0),
                    f64::from(chart.max_y() + cx.pt(4.0)),
                ),
                &s.name,
                size,
                if self.hovered == Some(i) { fg } else { muted },
            );

            // Median value at the very bottom.
            let med = format!("{:.0}", s.median);
            let mw = painter
                .and_then(|p| p.measure_text(&med, size))
                .unwrap_or(med.chars().count() as f32 * size * 0.6);
            crate::text_paint::paint_label_clipped(
                painter,
                cx.list,
                f(Rect::new(
                    slot_x,
                    self.bounds.max_y() - AXIS_PT * cx.scale,
                    slot_w,
                    AXIS_PT * cx.scale,
                )),
                kurbo::Point::new(
                    f64::from(mid_x - mw / 2.0),
                    f64::from(self.bounds.max_y() - size * 1.3),
                ),
                &med,
                size,
                muted,
            );
        }
    }
}

impl std::fmt::Debug for BoxPlot {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("BoxPlot")
            .field("series", &self.series.len())
            .finish()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use martensite_core::HotNode;

    fn laid_out(p: &mut BoxPlot, w: f32, h: f32) {
        let mut hot = HotNode::default();
        let mut cx = LayoutContext {
            hot: &mut hot,
            scale: 1.0,
        };
        p.measure(
            &mut cx,
            LayoutConstraints {
                min_size: Vec2::ZERO,
                max_size: Vec2::new(w, h),
            },
        );
        p.layout(&mut cx, Rect::new(0.0, 0.0, w, h));
    }

    #[test]
    fn series_sorts_inputs() {
        let s = BoxSeries::new("A", 90.0, 10.0, 40.0, 60.0, 25.0);
        assert_eq!(s.min, 10.0);
        assert_eq!(s.median, 40.0);
        assert_eq!(s.max, 90.0);
    }

    #[test]
    fn series_accumulate() {
        let p = BoxPlot::new()
            .series(BoxSeries::new("A", 0.0, 1.0, 2.0, 3.0, 4.0))
            .series(BoxSeries::new("B", 5.0, 6.0, 7.0, 8.0, 9.0));
        assert_eq!(p.series_count(), 2);
        assert_eq!(p.series_list()[1].name, "B");
    }

    #[test]
    fn hover_parks_series() {
        let mut p = BoxPlot::new()
            .series(BoxSeries::new("A", 0.0, 1.0, 2.0, 3.0, 4.0))
            .series(BoxSeries::new("B", 5.0, 6.0, 7.0, 8.0, 9.0));
        laid_out(&mut p, 300.0, 200.0);
        p.event(&mut EventContext {
            event: &WidgetEvent::PointerMoved {
                position: Vec2::new(225.0, 60.0), // second slot
            },
            bounds: Rect::new(0.0, 0.0, 300.0, 200.0),
            scale: 1.0,
        });
        assert_eq!(p.take_hovered(), Some(1));
    }

    #[test]
    fn hover_below_chart_ignored() {
        let mut p = BoxPlot::new().series(BoxSeries::new("A", 0.0, 1.0, 2.0, 3.0, 4.0));
        laid_out(&mut p, 300.0, 200.0);
        p.event(&mut EventContext {
            event: &WidgetEvent::PointerMoved {
                position: Vec2::new(150.0, 195.0), // axis strip
            },
            bounds: Rect::new(0.0, 0.0, 300.0, 200.0),
            scale: 1.0,
        });
        assert!(p.take_hovered().is_none());
    }

    #[test]
    fn single_value_range_safe() {
        // All-equal values must not produce a zero/negative span.
        let p = BoxPlot::new().series(BoxSeries::new("A", 5.0, 5.0, 5.0, 5.0, 5.0));
        let (lo, hi) = p.value_range();
        assert_eq!((lo, hi), (0.0, 1.0));
    }
}
