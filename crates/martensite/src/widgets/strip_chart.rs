//! `StripChart` — a scrolling time-series trace (oscilloscope /
//! strip-chart-recorder idiom).
//!
//! [`StripChart::push`] appends samples to a `capacity`-bounded
//! ring; the newest sample anchors the right edge and older data
//! scrolls left — the classic paper-recorder look. A mid-scale
//! center line and optional `min`/`max` pin the vertical range
//! (auto-fit otherwise). Unlike [`crate::widgets::waveform::Waveform`],
//! which displays a fixed peak list, this is a live append stream.
//!
//! # Examples
//!
//! ```
//! use martensite::widgets::strip_chart::StripChart;
//!
//! let mut s = StripChart::new().capacity(60);
//! s.push(0.5);
//! s.push(0.7);
//! assert_eq!(s.sample_count(), 2);
//! ```

use accesskit::Node as AccessKitNode;
use glam::Vec2;
use martensite_core::{
    EventContext, EventResponse, LayoutConstraints, LayoutContext, PaintContext, Rect,
    RenderMinimum, UnderflowPolicy, Widget,
};
use martensite_theme::TokenKey;

use std::collections::VecDeque;

const WIDTH_PT: f32 = 240.0;
const HEIGHT_PT: f32 = 80.0;
const PAD_PT: f32 = 4.0;

const FACE: [u8; 4] = [32, 32, 38, 255];
const EDGE: [u8; 4] = [70, 70, 76, 255];
const TRACE: [u8; 4] = [110, 200, 140, 255];
const GRID: [u8; 4] = [60, 60, 66, 255];

/// A scrolling strip-chart trace — see the module docs.
///
/// ```
/// use martensite::widgets::strip_chart::StripChart;
///
/// assert_eq!(StripChart::new().sample_count(), 0);
/// ```
#[derive(Debug)]
pub struct StripChart {
    /// Accessibility label.
    pub label: String,
    samples: VecDeque<f32>,
    capacity: usize,
    min: Option<f32>,
    max: Option<f32>,
    bounds: Rect,
    scale: f32,
}

impl Default for StripChart {
    fn default() -> Self {
        Self::new()
    }
}

impl StripChart {
    /// Creates an empty trace with a 120-sample ring.
    ///
    /// ```
    /// use martensite::widgets::strip_chart::StripChart;
    ///
    /// assert_eq!(StripChart::new().capacity_value(), 120);
    /// ```
    pub fn new() -> Self {
        Self {
            label: "Strip chart".to_string(),
            samples: VecDeque::new(),
            capacity: 120,
            min: None,
            max: None,
            bounds: Rect::new(0.0, 0.0, 0.0, 0.0),
            scale: 1.0,
        }
    }

    /// Ring capacity (oldest samples drop past it).
    ///
    /// ```
    /// use martensite::widgets::strip_chart::StripChart;
    ///
    /// assert_eq!(StripChart::new().capacity(30).capacity_value(), 30);
    /// ```
    pub fn capacity(mut self, capacity: usize) -> Self {
        self.capacity = capacity.max(2);
        while self.samples.len() > self.capacity {
            self.samples.pop_front();
        }
        self
    }

    /// Pins the vertical range (auto-fit when unset).
    ///
    /// ```
    /// use martensite::widgets::strip_chart::StripChart;
    ///
    /// assert_eq!(StripChart::new().range(-1.0, 1.0).range_value(), (-1.0, 1.0));
    /// ```
    pub fn range(mut self, min: f32, max: f32) -> Self {
        self.min = Some(min.min(max));
        self.max = Some(max.max(min));
        self
    }

    /// Accessibility label.
    ///
    /// ```
    /// use martensite::widgets::strip_chart::StripChart;
    ///
    /// assert_eq!(StripChart::new().label("Temp").label, "Temp");
    /// ```
    pub fn label(mut self, label: impl Into<String>) -> Self {
        self.label = label.into();
        self
    }

    /// Appends a sample, dropping the oldest at capacity.
    ///
    /// ```
    /// use martensite::widgets::strip_chart::StripChart;
    ///
    /// let mut s = StripChart::new().capacity(3);
    /// for v in [1.0, 2.0, 3.0, 4.0] {
    ///     s.push(v);
    /// }
    /// assert_eq!(s.sample_count(), 3);
    /// assert_eq!(s.latest(), Some(4.0));
    /// ```
    pub fn push(&mut self, value: f32) {
        if self.samples.len() >= self.capacity {
            self.samples.pop_front();
        }
        self.samples.push_back(value);
    }

    /// Extends the trace with several samples.
    ///
    /// ```
    /// use martensite::widgets::strip_chart::StripChart;
    ///
    /// let mut s = StripChart::new();
    /// s.extend([0.1, 0.2, 0.3]);
    /// assert_eq!(s.sample_count(), 3);
    /// ```
    pub fn extend(&mut self, values: impl IntoIterator<Item = f32>) {
        for v in values {
            self.push(v);
        }
    }

    /// Clears the trace.
    ///
    /// ```
    /// use martensite::widgets::strip_chart::StripChart;
    ///
    /// let mut s = StripChart::new();
    /// s.push(1.0);
    /// s.clear();
    /// assert_eq!(s.sample_count(), 0);
    /// ```
    pub fn clear(&mut self) {
        self.samples.clear();
    }

    /// Sample count.
    ///
    /// ```
    /// use martensite::widgets::strip_chart::StripChart;
    ///
    /// assert_eq!(StripChart::new().sample_count(), 0);
    /// ```
    pub fn sample_count(&self) -> usize {
        self.samples.len()
    }

    /// Ring capacity.
    ///
    /// ```
    /// use martensite::widgets::strip_chart::StripChart;
    ///
    /// assert_eq!(StripChart::new().capacity(50).capacity_value(), 50);
    /// ```
    pub fn capacity_value(&self) -> usize {
        self.capacity
    }

    /// The newest sample.
    ///
    /// ```
    /// use martensite::widgets::strip_chart::StripChart;
    ///
    /// let mut s = StripChart::new();
    /// s.push(0.25);
    /// assert_eq!(s.latest(), Some(0.25));
    /// ```
    pub fn latest(&self) -> Option<f32> {
        self.samples.back().copied()
    }

    /// Effective `(min, max)` — pinned or the data span.
    ///
    /// ```
    /// use martensite::widgets::strip_chart::StripChart;
    ///
    /// assert_eq!(StripChart::new().range(0.0, 10.0).range_value(), (0.0, 10.0));
    /// ```
    pub fn range_value(&self) -> (f32, f32) {
        let lo = self
            .min
            .unwrap_or_else(|| self.samples.iter().copied().fold(0.0, f32::min));
        let hi = self
            .max
            .unwrap_or_else(|| self.samples.iter().copied().fold(1.0, f32::max));
        (lo, hi)
    }

    /// Y coordinate for a sample.
    fn y_of(&self, v: f32) -> f32 {
        let (lo, hi) = self.range_value();
        let f = ((v - lo) / (hi - lo).max(0.0001)).clamp(0.0, 1.0);
        self.bounds.max_y()
            - PAD_PT * self.scale
            - f * (self.bounds.height() - 2.0 * PAD_PT * self.scale)
    }
}

impl Widget for StripChart {
    fn measure(&mut self, cx: &mut LayoutContext, constraints: LayoutConstraints) -> Vec2 {
        Vec2::new(
            cx.pt(WIDTH_PT).min(constraints.max_size.x.max(0.0)),
            cx.pt(HEIGHT_PT).min(constraints.max_size.y.max(0.0)),
        )
    }

    fn min_render(&self) -> RenderMinimum {
        RenderMinimum::new(Vec2::new(60.0, 24.0)).with_policy(UnderflowPolicy::Lint)
    }

    fn layout(&mut self, cx: &mut LayoutContext, bounds: Rect) {
        self.bounds = bounds;
        self.scale = cx.scale;
    }

    fn accessibility(&self, node: &mut AccessKitNode) {
        node.set_role(accesskit::Role::Image);
        node.set_label(format!(
            "{} — latest {:.2}",
            self.label,
            self.latest().unwrap_or(0.0)
        ));
    }

    fn event(&mut self, _cx: &mut EventContext) -> EventResponse {
        EventResponse::Ignored
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
        let pt = |p: Vec2| (f64::from(p.x), f64::from(p.y));
        cx.list.push_fill_shape(
            f(self.bounds),
            &martensite_core::shape::Shape::rounded(cx.pt(4.0)),
            cx.color(TokenKey::SurfaceColor, FACE),
        );
        // Center grid line.
        let mid_y = (self.bounds.min_y() + self.bounds.max_y()) / 2.0;
        let mut grid = kurbo::BezPath::new();
        grid.move_to(pt(Vec2::new(self.bounds.min_x(), mid_y)));
        grid.line_to(pt(Vec2::new(self.bounds.max_x(), mid_y)));
        cx.list
            .push_stroke_path(grid, cx.pt(0.5), cx.color(TokenKey::BorderColor, GRID));

        // Trace — newest sample anchored to the right edge.
        if self.samples.len() >= 2 {
            let pad = PAD_PT * self.scale;
            let usable_w = (self.bounds.width() - 2.0 * pad).max(1.0);
            let dx = usable_w / (self.capacity.saturating_sub(1)).max(1) as f32;
            let mut path = kurbo::BezPath::new();
            let n = self.samples.len();
            for (i, &v) in self.samples.iter().enumerate() {
                let x = self.bounds.max_x() - pad - (n - 1 - i) as f32 * dx;
                let p = pt(Vec2::new(x.max(self.bounds.min_x() + pad), self.y_of(v)));
                if i == 0 {
                    path.move_to(p);
                } else {
                    path.line_to(p);
                }
            }
            cx.list
                .push_stroke_path(path, cx.pt(1.5), cx.color(TokenKey::SuccessColor, TRACE));
        }
        cx.list.push_stroke_shape(
            f(self.bounds),
            &martensite_core::shape::Shape::rounded(cx.pt(4.0)),
            cx.pt(0.75),
            cx.color(TokenKey::BorderColor, EDGE),
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use martensite_core::HotNode;

    #[test]
    fn ring_drops_oldest() {
        let mut s = StripChart::new().capacity(3);
        s.extend([1.0, 2.0, 3.0, 4.0, 5.0]);
        assert_eq!(s.sample_count(), 3);
        assert_eq!(s.latest(), Some(5.0));
        // Oldest retained is 3.0.
        assert_eq!(s.samples.front().copied(), Some(3.0));
    }

    #[test]
    fn clear_empties() {
        let mut s = StripChart::new();
        s.extend([1.0, 2.0]);
        s.clear();
        assert_eq!(s.sample_count(), 0);
        assert_eq!(s.latest(), None);
    }

    #[test]
    fn autofit_range() {
        let mut s = StripChart::new();
        s.extend([-2.0, 5.0]);
        assert_eq!(s.range_value(), (-2.0, 5.0));
        let pinned = StripChart::new().range(0.0, 1.0);
        assert_eq!(pinned.range_value(), (0.0, 1.0));
    }

    #[test]
    fn capacity_shrinks_existing() {
        let mut s = StripChart::new();
        s.extend([1.0, 2.0, 3.0]);
        let s = s.capacity(2);
        assert_eq!(s.sample_count(), 2);
        assert_eq!(s.samples.front().copied(), Some(2.0));
    }

    #[test]
    fn lays_out() {
        let mut s = StripChart::new();
        let mut hot = HotNode::default();
        let mut cx = LayoutContext {
            hot: &mut hot,
            scale: 1.0,
        };
        let sz = s.measure(
            &mut cx,
            LayoutConstraints {
                min_size: Vec2::ZERO,
                max_size: Vec2::new(500.0, 300.0),
            },
        );
        assert_eq!(sz, Vec2::new(240.0, 80.0));
    }
}
