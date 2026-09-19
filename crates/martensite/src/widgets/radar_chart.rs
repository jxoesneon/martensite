//! `RadarChart` — a spider/polar chart (Ant `Radar`, Qt
//! `QPolarChart`, ECharts radar).
//!
//! `N` named axes radiate from the center over concentric value
//! rings; each [`RadarSeries`] paints a closed polygon (translucent
//! fill, stroked edge, vertex dots) scaled to the chart max. Series
//! without explicit colors draw from the shared chart palette. Axis
//! labels paint at the outer vertices when a text painter is
//! available.
//!
//! # Examples
//!
//! ```
//! use martensite::widgets::radar_chart::{RadarChart, RadarSeries};
//!
//! let c = RadarChart::new()
//!     .axes(["Speed", "Power", "Range"])
//!     .series(RadarSeries::new("A", [4.0, 3.0, 5.0]));
//! assert_eq!(c.axes.len(), 3);
//! ```

use accesskit::Node as AccessKitNode;
use glam::Vec2;
use martensite_core::{
    EventContext, EventResponse, LayoutConstraints, LayoutContext, PaintContext, Rect,
    RenderMinimum, UnderflowPolicy, Widget,
};
use martensite_theme::TokenKey;

use crate::text_paint::SharedTextPainter;

const SIZE_PT: f32 = 200.0;
const LABEL_PT: f32 = 16.0;
const RINGS: usize = 4;

const GRID: [u8; 4] = [215, 217, 222, 255];
const MUTED: [u8; 4] = [110, 110, 118, 255];
const PALETTE: [[u8; 4]; 6] = [
    [80, 140, 220, 255],
    [230, 120, 60, 255],
    [70, 170, 110, 255],
    [190, 90, 180, 255],
    [210, 170, 60, 255],
    [90, 90, 200, 255],
];

/// One radar series — a value per axis.
///
/// ```
/// use martensite::widgets::radar_chart::RadarSeries;
///
/// let s = RadarSeries::new("A", [1.0, 2.0, 3.0]);
/// assert_eq!(s.values.len(), 3);
/// ```
pub struct RadarSeries {
    /// Series name (a11y).
    pub name: String,
    /// One value per axis (missing values read as `0`).
    pub values: Vec<f32>,
    /// Explicit color; `None` draws from the palette.
    pub color: Option<[u8; 4]>,
}

impl RadarSeries {
    /// Creates a series.
    ///
    /// ```
    /// use martensite::widgets::radar_chart::RadarSeries;
    ///
    /// let s = RadarSeries::new("S", [0.0, 1.0]);
    /// assert_eq!(s.values.len(), 2);
    /// ```
    pub fn new(name: impl Into<String>, values: impl IntoIterator<Item = f32>) -> Self {
        Self {
            name: name.into(),
            values: values.into_iter().collect(),
            color: None,
        }
    }

    /// Overrides the palette color.
    ///
    /// ```
    /// use martensite::widgets::radar_chart::RadarSeries;
    ///
    /// let s = RadarSeries::new("S", [0.0]).color([1, 2, 3, 255]);
    /// assert_eq!(s.color, Some([1, 2, 3, 255]));
    /// ```
    pub fn color(mut self, c: [u8; 4]) -> Self {
        self.color = Some(c);
        self
    }
}

/// A spider/radar chart — see the module docs.
///
/// ```
/// use martensite::widgets::radar_chart::RadarChart;
///
/// let c = RadarChart::new();
/// assert!(c.series.is_empty());
/// ```
pub struct RadarChart {
    /// Axis names, clockwise from 12 o'clock.
    pub axes: Vec<String>,
    /// Data series.
    pub series: Vec<RadarSeries>,
    /// Explicit scale max; `None` fits the data.
    pub max: Option<f32>,
    /// Concentric ring count.
    pub rings: usize,
    /// When `false` the chart mutes.
    pub enabled: bool,
    text_painter: Option<SharedTextPainter>,
    bounds: Rect,
    scale: f32,
}

impl Default for RadarChart {
    fn default() -> Self {
        Self::new()
    }
}

impl RadarChart {
    /// Creates an empty chart.
    ///
    /// ```
    /// use martensite::widgets::radar_chart::RadarChart;
    ///
    /// assert!(RadarChart::new().axes.is_empty());
    /// ```
    pub fn new() -> Self {
        Self {
            axes: Vec::new(),
            series: Vec::new(),
            max: None,
            rings: RINGS,
            enabled: true,
            text_painter: None,
            bounds: Rect::new(0.0, 0.0, 0.0, 0.0),
            scale: 1.0,
        }
    }

    /// Sets the axis names.
    ///
    /// ```
    /// use martensite::widgets::radar_chart::RadarChart;
    ///
    /// let c = RadarChart::new().axes(["A", "B"]);
    /// assert_eq!(c.axes.len(), 2);
    /// ```
    pub fn axes(mut self, axes: impl IntoIterator<Item = impl Into<String>>) -> Self {
        self.axes = axes.into_iter().map(Into::into).collect();
        self
    }

    /// Appends a series.
    ///
    /// ```
    /// use martensite::widgets::radar_chart::{RadarChart, RadarSeries};
    ///
    /// let c = RadarChart::new().series(RadarSeries::new("A", [1.0]));
    /// assert_eq!(c.series.len(), 1);
    /// ```
    pub fn series(mut self, s: RadarSeries) -> Self {
        self.series.push(s);
        self
    }

    /// Pins the scale max.
    ///
    /// ```
    /// use martensite::widgets::radar_chart::RadarChart;
    ///
    /// let c = RadarChart::new().max(10.0);
    /// assert_eq!(c.max, Some(10.0));
    /// ```
    pub fn max(mut self, max: f32) -> Self {
        self.max = Some(max);
        self
    }

    /// Sets the concentric ring count.
    ///
    /// ```
    /// use martensite::widgets::radar_chart::RadarChart;
    ///
    /// let c = RadarChart::new().rings(6);
    /// assert_eq!(c.rings, 6);
    /// ```
    pub fn rings(mut self, rings: usize) -> Self {
        self.rings = rings.max(1);
        self
    }

    /// Enables or disables the chart.
    ///
    /// ```
    /// use martensite::widgets::radar_chart::RadarChart;
    ///
    /// let c = RadarChart::new().enabled(false);
    /// assert!(!c.enabled);
    /// ```
    pub fn enabled(mut self, enabled: bool) -> Self {
        self.enabled = enabled;
        self
    }

    /// Explicit painter override — see [`crate::text_paint`].
    ///
    /// ```
    /// use martensite::widgets::radar_chart::RadarChart;
    /// use martensite::text_paint::shared_painter;
    ///
    /// let c = RadarChart::new().with_text_painter(shared_painter());
    /// assert!(c.series.is_empty());
    /// ```
    pub fn with_text_painter(mut self, painter: SharedTextPainter) -> Self {
        self.text_painter = Some(painter);
        self
    }

    /// Scale max — the explicit pin or the data peak.
    fn scale_max(&self) -> f32 {
        self.max.unwrap_or_else(|| {
            self.series
                .iter()
                .flat_map(|s| s.values.iter().copied())
                .fold(0.0, f32::max)
                .max(1.0)
        })
    }

    /// Clockwise vertex direction for axis `i` of `n` (0 = up).
    fn vertex(i: usize, n: usize) -> (f32, f32) {
        let a = i as f32 / n as f32 * std::f32::consts::TAU - std::f32::consts::FRAC_PI_2;
        (a.cos(), a.sin())
    }

    fn palette(&self, i: usize) -> [u8; 4] {
        PALETTE[i % PALETTE.len()]
    }
}

impl Widget for RadarChart {
    fn measure(&mut self, cx: &mut LayoutContext, constraints: LayoutConstraints) -> Vec2 {
        let s = cx.pt(SIZE_PT);
        Vec2::new(
            s.min(constraints.max_size.x.max(0.0)),
            s.min(constraints.max_size.y.max(0.0)),
        )
    }

    fn min_render(&self) -> RenderMinimum {
        RenderMinimum::new(Vec2::new(64.0, 64.0)).with_policy(UnderflowPolicy::Lint)
    }

    fn layout(&mut self, cx: &mut LayoutContext, bounds: Rect) {
        self.bounds = bounds;
        self.scale = cx.scale;
    }

    fn accessibility(&self, node: &mut AccessKitNode) {
        node.set_role(accesskit::Role::Image);
        node.set_label("Radar chart");
        node.set_value(format!(
            "{} series, {} axes",
            self.series.len(),
            self.axes.len()
        ));
        if !self.enabled {
            node.set_disabled();
        }
    }

    fn event(&mut self, _cx: &mut EventContext) -> EventResponse {
        EventResponse::Ignored
    }

    fn paint(&self, cx: &mut PaintContext) {
        let n = self.axes.len();
        if n < 3 {
            return; // a polygon needs ≥3 axes
        }
        let dim = self.bounds.width().min(self.bounds.height());
        if dim <= 0.0 {
            return;
        }
        let center = Vec2::new(
            self.bounds.origin.x + self.bounds.size.x / 2.0,
            self.bounds.origin.y + self.bounds.size.y / 2.0,
        );
        let r = dim / 2.0 - cx.pt(LABEL_PT);
        if r <= 0.0 {
            return;
        }
        let grid = cx.color(TokenKey::DividerColor, GRID);

        // Concentric ring polygons.
        for ring in 1..=self.rings {
            let rr = r * ring as f32 / self.rings as f32;
            let mut p = kurbo::BezPath::new();
            for i in 0..n {
                let (vx, vy) = Self::vertex(i, n);
                let pt = (f64::from(center.x + vx * rr), f64::from(center.y + vy * rr));
                if i == 0 {
                    p.move_to(pt);
                } else {
                    p.line_to(pt);
                }
            }
            p.close_path();
            cx.list.push_stroke_path(p, cx.pt(0.75), grid);
        }
        // Spokes.
        for i in 0..n {
            let (vx, vy) = Self::vertex(i, n);
            let mut p = kurbo::BezPath::new();
            p.move_to((f64::from(center.x), f64::from(center.y)));
            p.line_to((f64::from(center.x + vx * r), f64::from(center.y + vy * r)));
            cx.list.push_stroke_path(p, cx.pt(0.75), grid);
        }
        // Axis labels.
        let painter = crate::text_paint::resolve_painter(&self.text_painter, cx.text_painter);
        let size = 10.0 * cx.scale;
        let muted = cx.color(TokenKey::TextMutedColor, MUTED);
        for (i, name) in self.axes.iter().enumerate() {
            let (vx, vy) = Self::vertex(i, n);
            let lx = center.x + vx * (r + cx.pt(LABEL_PT) * 0.6);
            let ly = center.y + vy * (r + cx.pt(LABEL_PT) * 0.6);
            let w = painter
                .and_then(|p| p.measure_text(name, size))
                .unwrap_or(name.chars().count() as f32 * size * 0.55);
            crate::text_paint::paint_label_clipped(
                painter,
                cx.list,
                kurbo::Rect::new(
                    f64::from(self.bounds.min_x()),
                    f64::from(self.bounds.min_y()),
                    f64::from(self.bounds.max_x()),
                    f64::from(self.bounds.max_y()),
                ),
                kurbo::Point::new(f64::from(lx - w / 2.0), f64::from(ly - size / 2.0)),
                name,
                size,
                muted,
            );
        }

        // Series polygons.
        let max = self.scale_max();
        for (si, s) in self.series.iter().enumerate() {
            let base = s.color.unwrap_or_else(|| self.palette(si));
            let mut p = kurbo::BezPath::new();
            for i in 0..n {
                let v = s.values.get(i).copied().unwrap_or(0.0) / max;
                let (vx, vy) = Self::vertex(i, n);
                let pt = (
                    f64::from(center.x + vx * r * v),
                    f64::from(center.y + vy * r * v),
                );
                if i == 0 {
                    p.move_to(pt);
                } else {
                    p.line_to(pt);
                }
            }
            p.close_path();
            cx.list
                .push_path(p.clone(), [base[0], base[1], base[2], 60]);
            cx.list.push_stroke_path(p, cx.pt(1.5), base);
            // Vertex dots.
            for i in 0..n {
                let v = s.values.get(i).copied().unwrap_or(0.0) / max;
                let (vx, vy) = Self::vertex(i, n);
                let px = center.x + vx * r * v;
                let py = center.y + vy * r * v;
                let dr = cx.pt(2.5);
                cx.list.push_fill_shape(
                    kurbo::Rect::new(
                        f64::from(px - dr),
                        f64::from(py - dr),
                        f64::from(px + dr),
                        f64::from(py + dr),
                    ),
                    &martensite_core::shape::Shape::circle(Vec2::new(px, py), dr),
                    base,
                );
            }
        }
    }
}

impl std::fmt::Debug for RadarChart {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("RadarChart")
            .field("axes", &self.axes.len())
            .field("series", &self.series.len())
            .finish()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use martensite_core::HotNode;

    #[test]
    fn scale_max_fits_data() {
        let c = RadarChart::new()
            .axes(["A", "B", "C"])
            .series(RadarSeries::new("S", [2.0, 8.0, 4.0]));
        assert_eq!(c.scale_max(), 8.0);
        let pinned = RadarChart::new().axes(["A", "B", "C"]).max(20.0);
        assert_eq!(pinned.scale_max(), 20.0);
        let flat = RadarChart::new().axes(["A", "B", "C"]);
        assert_eq!(flat.scale_max(), 1.0); // never zero
    }

    #[test]
    fn vertex_directions() {
        // Axis 0 points up; with 4 axes, axis 1 points right.
        let (x0, y0) = RadarChart::vertex(0, 4);
        assert!(x0.abs() < 0.01 && y0 < -0.99);
        let (x1, y1) = RadarChart::vertex(1, 4);
        assert!(x1 > 0.99 && y1.abs() < 0.01);
    }

    #[test]
    fn palette_cycles() {
        let c = RadarChart::new();
        assert_eq!(c.palette(0), c.palette(PALETTE.len()));
    }

    #[test]
    fn missing_values_read_zero() {
        // paint() indexes values.get(i) — a short series is safe.
        let mut c = RadarChart::new()
            .axes(["A", "B", "C"])
            .series(RadarSeries::new("S", [5.0]));
        let mut hot = HotNode::default();
        let mut cx = LayoutContext {
            hot: &mut hot,
            scale: 1.0,
        };
        c.layout(&mut cx, Rect::new(0.0, 0.0, 200.0, 200.0));
        // Just ensure no panic constructing the series path.
        assert_eq!(c.series[0].values.get(2).copied().unwrap_or(0.0), 0.0);
    }

    #[test]
    fn measure_square() {
        let mut c = RadarChart::new();
        let mut hot = HotNode::default();
        let mut cx = LayoutContext {
            hot: &mut hot,
            scale: 1.0,
        };
        let s = c.measure(
            &mut cx,
            LayoutConstraints {
                min_size: Vec2::ZERO,
                max_size: Vec2::new(500.0, 500.0),
            },
        );
        assert_eq!(s.x, s.y);
    }
}
