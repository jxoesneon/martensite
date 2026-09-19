//! `ScatterChart` — an XY point-cloud chart (Ant `Scatter`,
//! Qt `QScatterSeries`, ECharts scatter).
//!
//! Series of `(x, y)` points paint as markers inside an axis frame
//! with a grid. Ranges auto-fit the data unless pinned through
//! `x_range`/`y_range`. Pointer proximity parks the nearest point as
//! `(series, index, (x, y))` in [`ScatterChart::take_hovered`] for
//! app tooltips.
//!
//! # Examples
//!
//! ```
//! use martensite::widgets::scatter_chart::{ScatterChart, ScatterSeries};
//!
//! let c = ScatterChart::new()
//!     .series(ScatterSeries::new("A", [(0.0, 1.0), (1.0, 2.0)]));
//! assert_eq!(c.series.len(), 1);
//! ```

use accesskit::Node as AccessKitNode;
use glam::Vec2;
use martensite_core::{
    EventContext, EventResponse, LayoutConstraints, LayoutContext, PaintContext, Rect,
    RenderMinimum, UnderflowPolicy, Widget, WidgetEvent,
};
use martensite_theme::TokenKey;

use crate::text_paint::SharedTextPainter;

const W_PT: f32 = 240.0;
const H_PT: f32 = 160.0;
const PAD_PT: f32 = 8.0;
const MARKER_PT: f32 = 3.0;
const GRID_N: usize = 4;
const HOVER_PT: f32 = 10.0;

const GRID: [u8; 4] = [215, 217, 222, 255];
const FRAME: [u8; 4] = [150, 152, 158, 255];
const SURFACE: [u8; 4] = [250, 250, 252, 255];
const PALETTE: [[u8; 4]; 6] = [
    [80, 140, 220, 255],
    [230, 120, 60, 255],
    [70, 170, 110, 255],
    [190, 90, 180, 255],
    [210, 170, 60, 255],
    [90, 90, 200, 255],
];

/// One scatter series — a set of `(x, y)` points.
///
/// ```
/// use martensite::widgets::scatter_chart::ScatterSeries;
///
/// let s = ScatterSeries::new("A", [(0.0, 0.0)]);
/// assert_eq!(s.points.len(), 1);
/// ```
pub struct ScatterSeries {
    /// Series name (a11y).
    pub name: String,
    /// `(x, y)` data points.
    pub points: Vec<(f32, f32)>,
    /// Explicit marker color; `None` draws from the palette.
    pub color: Option<[u8; 4]>,
    /// Marker radius in pt.
    pub size: f32,
}

impl ScatterSeries {
    /// Creates a series.
    ///
    /// ```
    /// use martensite::widgets::scatter_chart::ScatterSeries;
    ///
    /// let s = ScatterSeries::new("S", [(1.0, 2.0), (3.0, 4.0)]);
    /// assert_eq!(s.points.len(), 2);
    /// ```
    pub fn new(name: impl Into<String>, points: impl IntoIterator<Item = (f32, f32)>) -> Self {
        Self {
            name: name.into(),
            points: points.into_iter().collect(),
            color: None,
            size: MARKER_PT,
        }
    }

    /// Overrides the palette color.
    ///
    /// ```
    /// use martensite::widgets::scatter_chart::ScatterSeries;
    ///
    /// let s = ScatterSeries::new("S", [(0.0, 0.0)]).color([1, 2, 3, 255]);
    /// assert_eq!(s.color, Some([1, 2, 3, 255]));
    /// ```
    pub fn color(mut self, c: [u8; 4]) -> Self {
        self.color = Some(c);
        self
    }

    /// Marker radius in pt.
    ///
    /// ```
    /// use martensite::widgets::scatter_chart::ScatterSeries;
    ///
    /// let s = ScatterSeries::new("S", [(0.0, 0.0)]).size(5.0);
    /// assert_eq!(s.size, 5.0);
    /// ```
    pub fn size(mut self, size: f32) -> Self {
        self.size = size.max(0.5);
        self
    }
}

/// An XY scatter chart — see the module docs.
///
/// ```
/// use martensite::widgets::scatter_chart::ScatterChart;
///
/// let c = ScatterChart::new();
/// assert!(c.series.is_empty());
/// ```
pub struct ScatterChart {
    /// Series in draw order.
    pub series: Vec<ScatterSeries>,
    /// Pinned x range; `None` fits the data.
    pub x_range: Option<(f32, f32)>,
    /// Pinned y range; `None` fits the data.
    pub y_range: Option<(f32, f32)>,
    /// Whether grid lines paint.
    pub grid: bool,
    /// When `false` the chart is inert.
    pub enabled: bool,
    hovered: Option<(usize, usize)>,
    hovered_out: Option<(usize, usize, (f32, f32))>,
    bounds: Rect,
    plot: Rect,
    text_painter: Option<SharedTextPainter>,
    scale: f32,
}

impl Default for ScatterChart {
    fn default() -> Self {
        Self::new()
    }
}

impl ScatterChart {
    /// Creates an empty chart.
    ///
    /// ```
    /// use martensite::widgets::scatter_chart::ScatterChart;
    ///
    /// assert!(ScatterChart::new().grid);
    /// ```
    pub fn new() -> Self {
        Self {
            series: Vec::new(),
            x_range: None,
            y_range: None,
            grid: true,
            enabled: true,
            hovered: None,
            hovered_out: None,
            bounds: Rect::new(0.0, 0.0, 0.0, 0.0),
            plot: Rect::new(0.0, 0.0, 0.0, 0.0),
            text_painter: None,
            scale: 1.0,
        }
    }

    /// Appends a series.
    ///
    /// ```
    /// use martensite::widgets::scatter_chart::{ScatterChart, ScatterSeries};
    ///
    /// let c = ScatterChart::new().series(ScatterSeries::new("A", [(0.0, 0.0)]));
    /// assert_eq!(c.series.len(), 1);
    /// ```
    pub fn series(mut self, s: ScatterSeries) -> Self {
        self.series.push(s);
        self
    }

    /// Pins the x range.
    ///
    /// ```
    /// use martensite::widgets::scatter_chart::ScatterChart;
    ///
    /// let c = ScatterChart::new().x_range(0.0, 10.0);
    /// assert_eq!(c.x_range, Some((0.0, 10.0)));
    /// ```
    pub fn x_range(mut self, lo: f32, hi: f32) -> Self {
        self.x_range = Some((lo, hi));
        self
    }

    /// Pins the y range.
    ///
    /// ```
    /// use martensite::widgets::scatter_chart::ScatterChart;
    ///
    /// let c = ScatterChart::new().y_range(0.0, 10.0);
    /// assert_eq!(c.y_range, Some((0.0, 10.0)));
    /// ```
    pub fn y_range(mut self, lo: f32, hi: f32) -> Self {
        self.y_range = Some((lo, hi));
        self
    }

    /// Toggles the grid.
    ///
    /// ```
    /// use martensite::widgets::scatter_chart::ScatterChart;
    ///
    /// let c = ScatterChart::new().grid(false);
    /// assert!(!c.grid);
    /// ```
    pub fn grid(mut self, show: bool) -> Self {
        self.grid = show;
        self
    }

    /// Enables or disables the chart.
    ///
    /// ```
    /// use martensite::widgets::scatter_chart::ScatterChart;
    ///
    /// let c = ScatterChart::new().enabled(false);
    /// assert!(!c.enabled);
    /// ```
    pub fn enabled(mut self, enabled: bool) -> Self {
        self.enabled = enabled;
        self
    }

    /// Explicit painter override — see [`crate::text_paint`].
    ///
    /// ```
    /// use martensite::widgets::scatter_chart::ScatterChart;
    ///
    /// let c = ScatterChart::new();
    /// let _ = c.grid;
    /// ```
    pub fn with_text_painter(mut self, painter: SharedTextPainter) -> Self {
        self.text_painter = Some(painter);
        self
    }

    /// Drains the nearest hovered point `(series, index, (x, y))`.
    ///
    /// ```
    /// use martensite::widgets::scatter_chart::ScatterChart;
    ///
    /// let mut c = ScatterChart::new();
    /// assert_eq!(c.take_hovered(), None);
    /// ```
    pub fn take_hovered(&mut self) -> Option<(usize, usize, (f32, f32))> {
        self.hovered_out.take()
    }

    /// Data range on each axis — `(0, 1)` fallbacks.
    fn ranges(&self) -> ((f32, f32), (f32, f32)) {
        let fit = |pick: fn((f32, f32)) -> f32, pin: Option<(f32, f32)>| {
            pin.unwrap_or_else(|| {
                let mut lo = f32::MAX;
                let mut hi = f32::MIN;
                for s in &self.series {
                    for &p in &s.points {
                        let v = pick(p);
                        lo = lo.min(v);
                        hi = hi.max(v);
                    }
                }
                if lo > hi || (hi - lo).abs() < f32::EPSILON {
                    (0.0, 1.0)
                } else {
                    (lo, hi)
                }
            })
        };
        (fit(|p| p.0, self.x_range), fit(|p| p.1, self.y_range))
    }

    /// Maps a data point into plot-space.
    fn map(&self, x: f32, y: f32) -> Vec2 {
        let ((x0, x1), (y0, y1)) = self.ranges();
        let fx = (x - x0) / (x1 - x0).max(f32::EPSILON);
        let fy = (y - y0) / (y1 - y0).max(f32::EPSILON);
        Vec2::new(
            self.plot.min_x() + fx * self.plot.width(),
            self.plot.max_y() - fy * self.plot.height(),
        )
    }

    /// Nearest point within `radius` of `pos` (device px).
    fn hit(&self, pos: Vec2, radius: f32) -> Option<(usize, usize)> {
        let mut best: Option<(usize, usize, f32)> = None;
        for (si, s) in self.series.iter().enumerate() {
            for (pi, &(x, y)) in s.points.iter().enumerate() {
                let d = (self.map(x, y) - pos).length();
                if d <= radius && best.is_none_or(|(.., bd)| d < bd) {
                    best = Some((si, pi, d));
                }
            }
        }
        best.map(|(si, pi, _)| (si, pi))
    }

    fn palette(&self, i: usize) -> [u8; 4] {
        PALETTE[i % PALETTE.len()]
    }
}

impl Widget for ScatterChart {
    fn measure(&mut self, cx: &mut LayoutContext, constraints: LayoutConstraints) -> Vec2 {
        Vec2::new(
            cx.pt(W_PT).min(constraints.max_size.x.max(0.0)),
            cx.pt(H_PT).min(constraints.max_size.y.max(0.0)),
        )
    }

    fn min_render(&self) -> RenderMinimum {
        RenderMinimum::new(Vec2::new(80.0, 48.0)).with_policy(UnderflowPolicy::Lint)
    }

    fn layout(&mut self, cx: &mut LayoutContext, bounds: Rect) {
        self.bounds = bounds;
        self.scale = cx.scale;
        let pad = cx.pt(PAD_PT);
        self.plot = Rect::new(
            bounds.min_x() + pad,
            bounds.min_y() + pad,
            (bounds.width() - 2.0 * pad).max(0.0),
            (bounds.height() - 2.0 * pad).max(0.0),
        );
    }

    fn accessibility(&self, node: &mut AccessKitNode) {
        node.set_role(accesskit::Role::Image);
        node.set_label("Scatter chart");
        let n: usize = self.series.iter().map(|s| s.points.len()).sum();
        node.set_value(format!("{} points in {} series", n, self.series.len()));
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
                let h = if self.bounds.contains(*position) {
                    self.hit(*position, HOVER_PT * cx.scale)
                } else {
                    None
                };
                if h != self.hovered {
                    self.hovered = h;
                    self.hovered_out = h.map(|(si, pi)| (si, pi, self.series[si].points[pi]));
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
        let f = |r: Rect| {
            kurbo::Rect::new(
                f64::from(r.min_x()),
                f64::from(r.min_y()),
                f64::from(r.max_x()),
                f64::from(r.max_y()),
            )
        };
        cx.list.push_fill_shape(
            f(self.plot),
            &martensite_core::shape::Shape::rounded(0.0),
            cx.color(TokenKey::SurfaceColor, SURFACE),
        );
        let grid = cx.color(TokenKey::DividerColor, GRID);
        if self.grid {
            for i in 1..GRID_N {
                let fx = i as f32 / GRID_N as f32;
                let mut v = kurbo::BezPath::new();
                v.move_to((
                    f64::from(self.plot.min_x() + self.plot.width() * fx),
                    f64::from(self.plot.min_y()),
                ));
                v.line_to((
                    f64::from(self.plot.min_x() + self.plot.width() * fx),
                    f64::from(self.plot.max_y()),
                ));
                cx.list.push_stroke_path(v, cx.pt(0.5), grid);
                let mut h = kurbo::BezPath::new();
                h.move_to((
                    f64::from(self.plot.min_x()),
                    f64::from(self.plot.min_y() + self.plot.height() * fx),
                ));
                h.line_to((
                    f64::from(self.plot.max_x()),
                    f64::from(self.plot.min_y() + self.plot.height() * fx),
                ));
                cx.list.push_stroke_path(h, cx.pt(0.5), grid);
            }
        }
        // Frame.
        cx.list.push_stroke_shape(
            f(self.plot),
            &martensite_core::shape::Shape::rounded(0.0),
            cx.pt(0.75),
            cx.color(TokenKey::BorderColor, FRAME),
        );

        for (si, s) in self.series.iter().enumerate() {
            let base = s.color.unwrap_or_else(|| self.palette(si));
            let r = s.size * cx.scale;
            for (pi, &(x, y)) in s.points.iter().enumerate() {
                let p = self.map(x, y);
                let mut color = base;
                if self.hovered == Some((si, pi)) {
                    color = [
                        base[0].saturating_add(40),
                        base[1].saturating_add(40),
                        base[2].saturating_add(40),
                        255,
                    ];
                }
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

impl std::fmt::Debug for ScatterChart {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ScatterChart")
            .field("series", &self.series.len())
            .finish()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use martensite_core::HotNode;

    fn laid_out(c: &mut ScatterChart, w: f32, h: f32) {
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

    #[test]
    fn ranges_fit_data() {
        let c = ScatterChart::new().series(ScatterSeries::new("A", [(0.0, 0.0), (10.0, 4.0)]));
        assert_eq!(c.ranges(), ((0.0, 10.0), (0.0, 4.0)));
        let pinned = ScatterChart::new().x_range(-5.0, 5.0);
        assert_eq!(pinned.ranges().0, (-5.0, 5.0));
    }

    #[test]
    fn flat_range_falls_back() {
        let c = ScatterChart::new().series(ScatterSeries::new("A", [(2.0, 2.0)]));
        assert_eq!(c.ranges(), ((0.0, 1.0), (0.0, 1.0)));
    }

    #[test]
    fn map_corners() {
        let mut c = ScatterChart::new().x_range(0.0, 10.0).y_range(0.0, 10.0);
        laid_out(&mut c, 216.0, 216.0); // 200px plot at pad 8
        let p00 = c.map(0.0, 0.0);
        let p11 = c.map(10.0, 10.0);
        assert!((p00.x - c.plot.min_x()).abs() < 0.01);
        assert!((p00.y - c.plot.max_y()).abs() < 0.01); // y up
        assert!((p11.x - c.plot.max_x()).abs() < 0.01);
        assert!((p11.y - c.plot.min_y()).abs() < 0.01);
    }

    #[test]
    fn hover_parks_nearest() {
        let mut c = ScatterChart::new()
            .x_range(0.0, 10.0)
            .y_range(0.0, 10.0)
            .series(ScatterSeries::new("A", [(5.0, 5.0)]));
        laid_out(&mut c, 216.0, 216.0);
        let center = c.map(5.0, 5.0);
        c.event(&mut EventContext {
            event: &WidgetEvent::PointerMoved { position: center },
            bounds: Rect::new(0.0, 0.0, 216.0, 216.0),
            scale: 1.0,
        });
        assert_eq!(c.take_hovered(), Some((0, 0, (5.0, 5.0))));
        assert_eq!(c.take_hovered(), None);
    }

    #[test]
    fn hover_misses_far_points() {
        let mut c = ScatterChart::new()
            .x_range(0.0, 10.0)
            .y_range(0.0, 10.0)
            .series(ScatterSeries::new("A", [(0.0, 0.0)]));
        laid_out(&mut c, 216.0, 216.0);
        let far = c.map(9.0, 9.0);
        c.event(&mut EventContext {
            event: &WidgetEvent::PointerMoved { position: far },
            bounds: Rect::new(0.0, 0.0, 216.0, 216.0),
            scale: 1.0,
        });
        assert_eq!(c.take_hovered(), None);
    }

    #[test]
    fn palette_cycles() {
        let c = ScatterChart::new();
        assert_eq!(c.palette(0), c.palette(PALETTE.len()));
    }
}
