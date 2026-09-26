//! `Sparkline` — inline word-sized trend chart.
//!
//! The Tufte sparkline / Swift Charts mini-series: a compact line
//! (optionally area-filled) chart normalized to the data's min/max,
//! with an optional endpoint dot and band overlays for the observed
//! range. Pure display — pair with `Statistic` for the value readout.
//!
//! # Examples
//!
//! ```
//! use martensite::widgets::sparkline::{Sparkline, SparkStyle};
//!
//! let s = Sparkline::new([1.0, 3.0, 2.0, 5.0, 4.0])
//!     .style(SparkStyle::Area);
//! assert_eq!(s.point_count(), 5);
//! ```

use accesskit::Node as AccessKitNode;
use glam::Vec2;
use martensite_core::{
    EventContext, EventResponse, LayoutConstraints, LayoutContext, PaintContext, Rect,
    RenderMinimum, UnderflowPolicy, Widget,
};
use martensite_theme::TokenKey;

/// Line/area ink.
const ACCENT: TokenKey = TokenKey::AccentColor;
/// Baseline.
const BASELINE: TokenKey = TokenKey::DividerColor;

/// How the series renders.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum SparkStyle {
    /// Stroke only.
    #[default]
    Line,
    /// Stroke plus a translucent fill to the baseline.
    Area,
    /// Bars from the baseline to each value.
    Bars,
}

/// An inline trend chart — see the module docs. Leaf widget, no
/// children, no interaction.
///
/// # Examples
///
/// ```
/// use martensite::widgets::sparkline::Sparkline;
/// use martensite::core::Widget;
///
/// let mut s = Sparkline::new([1.0, 2.0, 3.0]);
/// assert_eq!(s.child_count(), 0);
/// ```
pub struct Sparkline {
    data: Vec<f32>,
    style: SparkStyle,
    label: String,
    enabled: bool,
    /// Endpoint dot.
    show_dot: bool,
}

impl Sparkline {
    /// A sparkline over `data` (empty renders nothing).
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite::widgets::sparkline::Sparkline;
    ///
    /// let s = Sparkline::new([3.0, 1.0, 4.0]);
    /// assert_eq!(s.point_count(), 3);
    /// ```
    pub fn new(data: impl IntoIterator<Item = f32>) -> Self {
        Self {
            data: data.into_iter().collect(),
            style: SparkStyle::Line,
            label: "trend".into(),
            enabled: true,
            show_dot: true,
        }
    }

    /// Render style (default `Line`).
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite::widgets::sparkline::{Sparkline, SparkStyle};
    ///
    /// let s = Sparkline::new([1.0]).style(SparkStyle::Bars);
    /// ```
    pub fn style(mut self, style: SparkStyle) -> Self {
        self.style = style;
        self
    }

    /// Show or hide the endpoint dot (default on).
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite::widgets::sparkline::Sparkline;
    ///
    /// let s = Sparkline::new([1.0]).dot(false);
    /// ```
    pub fn dot(mut self, show: bool) -> Self {
        self.show_dot = show;
        self
    }

    /// Set the accessibility label (default `"trend"`).
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite::widgets::sparkline::Sparkline;
    ///
    /// let s = Sparkline::new([1.0]).label("CPU %");
    /// ```
    pub fn label(mut self, label: impl Into<String>) -> Self {
        self.label = label.into();
        self
    }

    /// Enable or disable (dims; default `true`).
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite::widgets::sparkline::Sparkline;
    ///
    /// let s = Sparkline::new([1.0]).enabled(false);
    /// ```
    pub fn enabled(mut self, enabled: bool) -> Self {
        self.enabled = enabled;
        self
    }

    /// Replace the data series.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite::widgets::sparkline::Sparkline;
    ///
    /// let mut s = Sparkline::new([1.0]);
    /// s.set_data([1.0, 2.0, 3.0]);
    /// assert_eq!(s.point_count(), 3);
    /// ```
    pub fn set_data(&mut self, data: impl IntoIterator<Item = f32>) {
        self.data = data.into_iter().collect();
    }

    /// Point count.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite::widgets::sparkline::Sparkline;
    ///
    /// assert_eq!(Sparkline::new([1.0, 2.0]).point_count(), 2);
    /// ```
    pub fn point_count(&self) -> usize {
        self.data.len()
    }

    /// `(min, max)` of the series; `None` when empty.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite::widgets::sparkline::Sparkline;
    ///
    /// assert_eq!(Sparkline::new([2.0, 5.0, 1.0]).range(), Some((1.0, 5.0)));
    /// ```
    pub fn range(&self) -> Option<(f32, f32)> {
        let mut it = self.data.iter();
        let first = *it.next()?;
        let (mut lo, mut hi) = (first, first);
        for v in it {
            lo = lo.min(*v);
            hi = hi.max(*v);
        }
        Some((lo, hi))
    }

    /// Map `data[i]` to a point inside `b` (top = max).
    fn point(&self, b: Rect, i: usize, lo: f32, hi: f32) -> Vec2 {
        let n = self.data.len();
        let x = if n <= 1 {
            b.min_x() + b.width() * 0.5
        } else {
            b.min_x() + b.width() * (i as f32 / (n - 1) as f32)
        };
        let t = if hi > lo {
            (self.data[i] - lo) / (hi - lo)
        } else {
            0.5
        };
        // Inset vertically so the stroke doesn't clip at the extremes.
        let pad = b.height() * 0.1;
        let y = b.min_y() + pad + (1.0 - t) * (b.height() - pad * 2.0).max(0.0);
        Vec2::new(x, y)
    }
}

impl Widget for Sparkline {
    fn measure(&mut self, _cx: &mut LayoutContext, _constraints: LayoutConstraints) -> Vec2 {
        Vec2::new(100.0, 24.0)
    }

    fn layout(&mut self, _cx: &mut LayoutContext, _bounds: Rect) {}

    fn paint(&self, cx: &mut PaintContext) {
        let n = self.data.len();
        if n == 0 {
            return;
        }
        let b = cx.bounds;
        let Some((lo, hi)) = self.range() else { return };
        let accent = cx.color(ACCENT, [50, 115, 230, 255]);
        let muted = cx.color(TokenKey::TextMutedColor, [160, 160, 170, 255]);
        let baseline = cx.color(BASELINE, [215, 217, 222, 255]);
        let ink = if self.enabled { accent } else { muted };
        let stroke_w = cx.pt(1.5);

        // Baseline hairline — drawn inside the bounds edge so nothing
        // relies on an ancestor clip to stay invisible.
        cx.list.push_fill_rect(
            kurbo::Rect::new(
                f64::from(b.min_x()),
                f64::from(b.max_y() - cx.pt(1.0)),
                f64::from(b.max_x()),
                f64::from(b.max_y()),
            ),
            baseline,
        );

        let pts: Vec<Vec2> = (0..n).map(|i| self.point(b, i, lo, hi)).collect();
        match self.style {
            SparkStyle::Bars => {
                let slot = b.width() / n as f32;
                let bar_w = slot * 0.6;
                for (i, p) in pts.iter().enumerate() {
                    let x = b.min_x() + i as f32 * slot + (slot - bar_w) * 0.5;
                    cx.list.push_fill_rect(
                        kurbo::Rect::new(
                            f64::from(x),
                            f64::from(p.y),
                            f64::from(x + bar_w),
                            f64::from(b.max_y()),
                        ),
                        ink,
                    );
                }
            }
            SparkStyle::Line | SparkStyle::Area => {
                if self.style == SparkStyle::Area && n > 1 {
                    let mut fill = kurbo::BezPath::new();
                    fill.move_to(kurbo::Point::new(f64::from(pts[0].x), f64::from(b.max_y())));
                    for p in &pts {
                        fill.line_to(kurbo::Point::new(f64::from(p.x), f64::from(p.y)));
                    }
                    fill.line_to(kurbo::Point::new(
                        f64::from(pts[n - 1].x),
                        f64::from(b.max_y()),
                    ));
                    fill.close_path();
                    let translucent = [ink[0], ink[1], ink[2], ink[3] / 4];
                    cx.list.push_path(fill, translucent);
                }
                let mut path = kurbo::BezPath::new();
                path.move_to(kurbo::Point::new(f64::from(pts[0].x), f64::from(pts[0].y)));
                for p in &pts[1..] {
                    path.line_to(kurbo::Point::new(f64::from(p.x), f64::from(p.y)));
                }
                cx.list.push_stroke_path(path, stroke_w, ink);
            }
        }

        if self.show_dot && self.style != SparkStyle::Bars {
            let last = pts[n - 1];
            let d = cx.pt(5.0);
            // The last sample sits on the right edge — clamp the dot
            // centre inward by its radius instead of overshooting the
            // bounds and relying on an ancestor clip.
            let dx = last
                .x
                .clamp(b.min_x() + d * 0.5, (b.max_x() - d * 0.5).max(b.min_x()));
            let dy = last
                .y
                .clamp(b.min_y() + d * 0.5, (b.max_y() - d * 0.5).max(b.min_y()));
            cx.list.push_fill_shape(
                kurbo::Rect::new(
                    f64::from(dx - d * 0.5),
                    f64::from(dy - d * 0.5),
                    f64::from(dx + d * 0.5),
                    f64::from(dy + d * 0.5),
                ),
                &martensite_core::shape::Shape::ELLIPSE,
                ink,
            );
        }
    }

    fn event(&mut self, _cx: &mut EventContext) -> EventResponse {
        EventResponse::Ignored
    }

    fn accessibility(&self, node: &mut AccessKitNode) {
        node.set_role(accesskit::Role::Image);
        node.set_label(self.label.as_str());
        if let Some((lo, hi)) = self.range() {
            node.set_value(format!("{lo:.2}–{hi:.2}"));
        }
        if !self.enabled {
            node.set_disabled();
        }
    }

    fn min_render(&self) -> RenderMinimum {
        RenderMinimum::new(Vec2::new(48.0, 12.0)).with_policy(UnderflowPolicy::Lint)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use martensite_core::HotNode;

    #[test]
    fn range_normalizes_extremes() {
        let s = Sparkline::new([5.0, -2.0, 0.0]);
        assert_eq!(s.range(), Some((-2.0, 5.0)));
    }

    #[test]
    fn empty_series_reports_none() {
        let s = Sparkline::new([]);
        assert_eq!(s.range(), None);
        assert_eq!(s.point_count(), 0);
    }

    #[test]
    fn point_maps_max_to_top() {
        let mut s = Sparkline::new([0.0, 10.0]);
        let mut hot = HotNode::default();
        let mut cx = LayoutContext {
            hot: &mut hot,
            scale: 1.0,
        };
        s.layout(&mut cx, Rect::new(0.0, 0.0, 100.0, 50.0));
        let b = Rect::new(0.0, 0.0, 100.0, 50.0);
        let top = s.point(b, 1, 0.0, 10.0);
        let bottom = s.point(b, 0, 0.0, 10.0);
        assert!(top.y < bottom.y, "higher value renders higher");
    }

    #[test]
    fn flat_series_centers() {
        let s = Sparkline::new([7.0, 7.0, 7.0]);
        let b = Rect::new(0.0, 0.0, 100.0, 50.0);
        let p = s.point(b, 1, 7.0, 7.0);
        assert!((p.y - 25.0).abs() < 0.5, "flat series sits mid-height");
    }
}
