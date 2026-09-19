//! `LineChart` — multi-series XY chart (Ant `Line`, Swift Charts
//! `LineMark`, `QChart` line series).
//!
//! The full-size companion to [`Sparkline`](crate::widgets::sparkline):
//! `Sparkline` is a word-sized single trend; `LineChart` draws
//! multiple named series over a shared domain with a baseline axis and
//! value ticks. Pointer proximity highlights the nearest series —
//! hovering parks it for [`LineChart::take_hovered`] so an app can
//! show a tooltip or legend emphasis.
//!
//! # Examples
//!
//! ```
//! use martensite::widgets::line_chart::{LineChart, LineSeries};
//!
//! let chart = LineChart::new()
//!     .series(LineSeries::new("In", [1.0, 3.0, 2.0, 5.0]))
//!     .series(LineSeries::new("Out", [2.0, 1.0, 4.0, 3.0]));
//! assert_eq!(chart.series.len(), 2);
//! ```

use accesskit::Node as AccessKitNode;
use glam::Vec2;
use martensite_core::{
    EventContext, EventResponse, LayoutConstraints, LayoutContext, PaintContext, Rect,
    RenderMinimum, UnderflowPolicy, Widget, WidgetEvent,
};
use martensite_theme::TokenKey;

use crate::text_paint::SharedTextPainter;

/// Categorical series palette (matches `PieChart`'s family).
const PALETTE: [[u8; 4]; 8] = [
    [24, 144, 255, 255],
    [19, 194, 194, 255],
    [82, 196, 26, 255],
    [250, 173, 20, 255],
    [250, 84, 28, 255],
    [245, 34, 45, 255],
    [114, 46, 209, 255],
    [235, 47, 150, 255],
];
const LINE_PT: f32 = 2.0;
const AXIS_PT: f32 = 1.0;
const TICK_FONT_PT: f32 = 9.0;
const TICK_STRIP_PT: f32 = 14.0;
const TICK_COUNT: usize = 4;
const POINT_R_PT: f32 = 2.5;
/// Series-proximity hit radius (pt).
const HIT_PT: f32 = 10.0;
const FALLBACK_AXIS: [u8; 4] = [208, 211, 217, 255];
const FALLBACK_TICK: [u8; 4] = [140, 143, 152, 255];

/// One named series of y-values over a shared x-domain.
///
/// ```
/// use martensite::widgets::line_chart::LineSeries;
///
/// let s = LineSeries::new("Q1", [1.0, 2.0, 3.0]);
/// assert_eq!(s.name, "Q1");
/// ```
pub struct LineSeries {
    /// Series name (legend/a11y).
    pub name: String,
    /// Y-values, evenly spaced over the domain.
    pub points: Vec<f32>,
    /// Explicit stroke color; `None` uses the palette.
    pub color: Option<[u8; 4]>,
}

impl LineSeries {
    /// Creates a series.
    ///
    /// ```
    /// use martensite::widgets::line_chart::LineSeries;
    ///
    /// let s = LineSeries::new("S", [0.0, 1.0]);
    /// assert_eq!(s.points.len(), 2);
    /// ```
    pub fn new(name: impl Into<String>, points: impl IntoIterator<Item = f32>) -> Self {
        Self {
            name: name.into(),
            points: points.into_iter().collect(),
            color: None,
        }
    }

    /// Overrides the palette color.
    ///
    /// ```
    /// use martensite::widgets::line_chart::LineSeries;
    ///
    /// let s = LineSeries::new("S", [0.0]).color([255, 0, 0, 255]);
    /// assert_eq!(s.color, Some([255, 0, 0, 255]));
    /// ```
    pub fn color(mut self, c: [u8; 4]) -> Self {
        self.color = Some(c);
        self
    }
}

/// Multi-series line chart — see the module docs.
///
/// ```
/// use martensite::widgets::line_chart::LineChart;
///
/// let c = LineChart::new();
/// assert!(c.series.is_empty());
/// ```
pub struct LineChart {
    /// Series in draw order.
    pub series: Vec<LineSeries>,
    /// Whether the baseline axis + value ticks paint.
    pub axis: bool,
    /// When `false` the chart is inert.
    pub enabled: bool,
    hovered: Option<usize>,
    hovered_out: Option<usize>,
    bounds: Rect,
    text_painter: Option<SharedTextPainter>,
    scale: f32,
}

impl LineChart {
    /// Creates an empty chart.
    ///
    /// ```
    /// use martensite::widgets::line_chart::LineChart;
    ///
    /// let c = LineChart::new();
    /// assert!(c.series.is_empty());
    /// ```
    pub fn new() -> Self {
        Self {
            series: Vec::new(),
            axis: true,
            enabled: true,
            hovered: None,
            hovered_out: None,
            bounds: Rect::new(0.0, 0.0, 0.0, 0.0),
            text_painter: None,
            scale: 1.0,
        }
    }

    /// Appends a series.
    ///
    /// ```
    /// use martensite::widgets::line_chart::{LineChart, LineSeries};
    ///
    /// let c = LineChart::new().series(LineSeries::new("A", [1.0, 2.0]));
    /// assert_eq!(c.series.len(), 1);
    /// ```
    pub fn series(mut self, s: LineSeries) -> Self {
        self.series.push(s);
        self
    }

    /// Toggles the baseline axis and ticks.
    ///
    /// ```
    /// use martensite::widgets::line_chart::LineChart;
    ///
    /// let c = LineChart::new().axis(false);
    /// assert!(!c.axis);
    /// ```
    pub fn axis(mut self, show: bool) -> Self {
        self.axis = show;
        self
    }

    /// Enables or disables the chart.
    ///
    /// ```
    /// use martensite::widgets::line_chart::LineChart;
    ///
    /// let c = LineChart::new().enabled(false);
    /// assert!(!c.enabled);
    /// ```
    pub fn enabled(mut self, enabled: bool) -> Self {
        self.enabled = enabled;
        self
    }

    /// Installs a shared shaped-text painter (tick labels).
    ///
    /// ```
    /// use martensite::widgets::line_chart::LineChart;
    ///
    /// let c = LineChart::new();
    /// let _ = c.axis;
    /// ```
    pub fn with_text_painter(mut self, painter: SharedTextPainter) -> Self {
        self.text_painter = Some(painter);
        self
    }

    /// Drains the series index the pointer entered (one-shot, for
    /// tooltip/legend emphasis in the app).
    ///
    /// ```
    /// use martensite::widgets::line_chart::LineChart;
    ///
    /// let mut c = LineChart::new();
    /// assert_eq!(c.take_hovered(), None);
    /// ```
    pub fn take_hovered(&mut self) -> Option<usize> {
        self.hovered_out.take()
    }

    /// `(min, max)` across all series (`(0, 1)` when empty).
    fn domain(&self) -> (f32, f32) {
        let mut lo = f32::MAX;
        let mut hi = f32::MIN;
        for s in &self.series {
            for &p in &s.points {
                lo = lo.min(p);
                hi = hi.max(p);
            }
        }
        if lo > hi {
            return (0.0, 1.0);
        }
        if (hi - lo).abs() < f32::EPSILON {
            hi = lo + 1.0;
        }
        (lo, hi)
    }

    /// The plot area — bounds minus the tick strip.
    fn plot(&self) -> Rect {
        let strip = if self.axis {
            TICK_STRIP_PT * self.scale
        } else {
            0.0
        };
        Rect::new(
            self.bounds.origin.x,
            self.bounds.origin.y,
            self.bounds.size.x,
            (self.bounds.size.y - strip).max(0.0),
        )
    }

    /// Maps a data point `(i, y)` of a series with `n` points to plot
    /// space.
    fn map_point(&self, i: usize, n: usize, y: f32, lo: f32, hi: f32) -> Vec2 {
        let plot = self.plot();
        let x = if n <= 1 {
            plot.origin.x + plot.size.x / 2.0
        } else {
            plot.origin.x + i as f32 * plot.size.x / (n - 1) as f32
        };
        let t = (y - lo) / (hi - lo);
        Vec2::new(x, plot.origin.y + plot.size.y * (1.0 - t))
    }

    /// Series index nearest to `position` within the hit radius.
    fn hit(&self, position: Vec2) -> Option<usize> {
        let (lo, hi) = self.domain();
        let hit_r = HIT_PT * self.scale;
        let mut best: Option<(usize, f32)> = None;
        for (si, s) in self.series.iter().enumerate() {
            let n = s.points.len();
            for i in 0..n {
                let p0 = self.map_point(i, n, s.points[i], lo, hi);
                // Cheap distance: check point proximity and, for the
                // segment to the next point, midpoint proximity.
                let d = (p0 - position).length();
                if d < hit_r && best.is_none_or(|(_, bd)| d < bd) {
                    best = Some((si, d));
                }
                if i + 1 < n {
                    let p1 = self.map_point(i + 1, n, s.points[i + 1], lo, hi);
                    let mid = (p0 + p1) / 2.0;
                    let dm = (mid - position).length();
                    if dm < hit_r && best.is_none_or(|(_, bd)| dm < bd) {
                        best = Some((si, dm));
                    }
                }
            }
        }
        best.map(|(i, _)| i)
    }
}

impl Default for LineChart {
    fn default() -> Self {
        Self::new()
    }
}

impl Widget for LineChart {
    fn measure(&mut self, cx: &mut LayoutContext, constraints: LayoutConstraints) -> Vec2 {
        Vec2::new(
            cx.pt(160.0).min(constraints.max_size.x.max(0.0)),
            cx.pt(100.0).min(constraints.max_size.y.max(0.0)),
        )
    }

    fn min_render(&self) -> RenderMinimum {
        RenderMinimum::new(Vec2::new(80.0, 60.0)).with_policy(UnderflowPolicy::Lint)
    }

    fn layout(&mut self, cx: &mut LayoutContext, bounds: Rect) {
        self.bounds = bounds;
        self.scale = cx.scale;
    }

    fn accessibility(&self, node: &mut AccessKitNode) {
        node.set_role(accesskit::Role::Image);
        node.set_label("Line chart");
        node.set_description(
            self.series
                .iter()
                .map(|s| s.name.as_str())
                .collect::<Vec<_>>()
                .join(", "),
        );
        if !self.enabled {
            node.set_disabled();
        }
    }

    fn event(&mut self, cx: &mut EventContext) -> EventResponse {
        if !self.enabled {
            return EventResponse::Ignored;
        }
        match cx.event {
            WidgetEvent::PointerMoved { position } => {
                let h = self.hit(*position);
                if h != self.hovered {
                    self.hovered = h;
                    self.hovered_out = h;
                    return EventResponse::RequestRepaint;
                }
                EventResponse::Ignored
            }
            WidgetEvent::PointerLeave => {
                if self.hovered.is_some() {
                    self.hovered = None;
                    return EventResponse::RequestRepaint;
                }
                EventResponse::Ignored
            }
            _ => EventResponse::Ignored,
        }
    }

    fn paint(&self, cx: &mut PaintContext) {
        let (lo, hi) = self.domain();
        let plot = self.plot();
        // Ticks: value labels down the left… simpler convention —
        // min/max labels at the strip edges.
        if self.axis {
            let base_y = plot.max_y();
            cx.list.push_fill_rect(
                kurbo::Rect::new(
                    f64::from(plot.min_x()),
                    f64::from(base_y),
                    f64::from(plot.max_x()),
                    f64::from(base_y + AXIS_PT * self.scale),
                ),
                cx.color(TokenKey::DividerColor, FALLBACK_AXIS),
            );
            let painter = crate::text_paint::resolve_painter(&self.text_painter, cx.text_painter);
            let font = TICK_FONT_PT * self.scale;
            let ty = base_y + (TICK_STRIP_PT * self.scale - font) / 2.0;
            for i in 0..=TICK_COUNT {
                let v = lo + (hi - lo) * i as f32 / TICK_COUNT as f32;
                let label = format!("{v:.1}");
                let tx = plot.origin.x + plot.size.x * i as f32 / TICK_COUNT as f32;
                crate::text_paint::paint_label_clipped(
                    painter,
                    cx.list,
                    kurbo::Rect::new(
                        f64::from(plot.min_x()),
                        f64::from(base_y),
                        f64::from(plot.max_x()),
                        f64::from(self.bounds.max_y()),
                    ),
                    kurbo::Point::new(f64::from(tx - font * 0.5), f64::from(ty)),
                    &label,
                    font,
                    cx.color(TokenKey::TextMutedColor, FALLBACK_TICK),
                );
            }
        }
        for (si, s) in self.series.iter().enumerate() {
            let n = s.points.len();
            if n == 0 {
                continue;
            }
            let mut color = s.color.unwrap_or(PALETTE[si % PALETTE.len()]);
            color = cx.color(TokenKey::AccentColor, color);
            let dim = self.hovered.is_some() && self.hovered != Some(si);
            if dim {
                color[3] /= 3;
            }
            let width = LINE_PT * self.scale * if self.hovered == Some(si) { 1.6 } else { 1.0 };
            if n == 1 {
                let p = self.map_point(0, n, s.points[0], lo, hi);
                cx.list.push_fill_shape(
                    kurbo::Rect::new(
                        f64::from(p.x - 2.0),
                        f64::from(p.y - 2.0),
                        f64::from(p.x + 2.0),
                        f64::from(p.y + 2.0),
                    ),
                    &martensite_core::shape::Shape::circle(p, 2.0 * self.scale),
                    color,
                );
                continue;
            }
            let mut els = Vec::with_capacity(n);
            els.push(kurbo::PathEl::MoveTo(kurbo::Point::new(
                f64::from(self.map_point(0, n, s.points[0], lo, hi).x),
                f64::from(self.map_point(0, n, s.points[0], lo, hi).y),
            )));
            for i in 1..n {
                let p = self.map_point(i, n, s.points[i], lo, hi);
                els.push(kurbo::PathEl::LineTo(kurbo::Point::new(
                    f64::from(p.x),
                    f64::from(p.y),
                )));
            }
            cx.list
                .push_stroke_path(kurbo::BezPath::from_vec(els), width, color);
            // Point markers on the hovered series.
            if self.hovered == Some(si) {
                let r = POINT_R_PT * self.scale;
                for i in 0..n {
                    let p = self.map_point(i, n, s.points[i], lo, hi);
                    cx.list.push_fill_shape(
                        kurbo::Rect::new(
                            f64::from(p.x - r),
                            f64::from(p.y - r),
                            f64::from(p.x + r),
                            f64::from(p.y + r),
                        ),
                        &martensite_core::shape::Shape::circle(p, r),
                        color,
                    );
                }
            }
        }
    }
}

impl std::fmt::Debug for LineChart {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("LineChart")
            .field("series", &self.series.len())
            .finish()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use martensite_core::HotNode;

    fn laid_out(c: &mut LineChart, w: f32, h: f32) {
        let mut hot = HotNode::default();
        let mut cx = LayoutContext {
            hot: &mut hot,
            scale: 1.0,
        };
        c.measure(
            &mut cx,
            LayoutConstraints {
                min_size: Vec2::ZERO,
                max_size: Vec2::new(w, h),
            },
        );
        c.layout(&mut cx, Rect::new(0.0, 0.0, w, h));
    }

    fn chart() -> LineChart {
        LineChart::new()
            .series(LineSeries::new("low", [0.0, 0.0, 0.0]))
            .series(LineSeries::new("high", [10.0, 10.0, 10.0]))
    }

    #[test]
    fn domain_spans_all_series() {
        let c = chart();
        assert_eq!(c.domain(), (0.0, 10.0));
    }

    #[test]
    fn flat_domain_pads() {
        let c = LineChart::new().series(LineSeries::new("f", [5.0, 5.0]));
        assert_eq!(c.domain(), (5.0, 6.0));
    }

    #[test]
    fn hit_picks_nearest_series() {
        let mut c = chart();
        laid_out(&mut c, 200.0, 114.0);
        // "high" (y=10) paints near the top; "low" near the baseline.
        assert_eq!(c.hit(Vec2::new(100.0, 5.0)), Some(1));
        assert_eq!(c.hit(Vec2::new(100.0, 95.0)), Some(0));
        assert_eq!(c.hit(Vec2::new(100.0, 50.0)), None);
    }

    #[test]
    fn hover_parks_series_once() {
        let mut c = chart();
        laid_out(&mut c, 200.0, 114.0);
        c.event(&mut EventContext {
            event: &WidgetEvent::PointerMoved {
                position: Vec2::new(100.0, 5.0),
            },
            bounds: Rect::new(0.0, 0.0, 200.0, 114.0),
            scale: 1.0,
        });
        assert_eq!(c.take_hovered(), Some(1));
        assert_eq!(c.take_hovered(), None);
    }

    #[test]
    fn plot_leaves_tick_strip() {
        let mut c = chart();
        laid_out(&mut c, 200.0, 114.0);
        let plot = c.plot();
        assert!(plot.size.y < c.bounds.size.y);
    }
}
