//! `StreamGraph` — a flowing stacked-area chart (ThemeRiver /
//! streamgraph idiom).
//!
//! Each layer is an equal-length series stacked symmetrically
//! around a drifting baseline, painted in a categorical palette —
//! the organic alternative to `LineChart`'s axis-bound stacking.
//! Hovering a layer's column parks its index in
//! [`StreamGraph::take_hovered`].
//!
//! # Examples
//!
//! ```
//! use martensite::widgets::stream_graph::StreamGraph;
//!
//! let s = StreamGraph::new().layer("a", [1.0, 2.0, 1.0]).layer("b", [0.5, 1.0, 0.5]);
//! assert_eq!(s.layer_count(), 2);
//! ```

use accesskit::Node as AccessKitNode;
use glam::Vec2;
use martensite_core::{
    EventContext, EventResponse, LayoutConstraints, LayoutContext, PaintContext, Rect,
    RenderMinimum, UnderflowPolicy, Widget, WidgetEvent,
};
use martensite_theme::TokenKey;

const WIDTH_PT: f32 = 260.0;
const HEIGHT_PT: f32 = 120.0;
const PAD_PT: f32 = 6.0;

const FACE: [u8; 4] = [34, 34, 40, 255];
const EDGE: [u8; 4] = [70, 70, 76, 255];
const PALETTE: [[u8; 4]; 6] = [
    [110, 170, 230, 200],
    [230, 150, 90, 200],
    [120, 200, 140, 200],
    [220, 110, 110, 200],
    [190, 140, 230, 200],
    [230, 210, 120, 200],
];

/// A flowing stacked-area chart — see the module docs.
///
/// ```
/// use martensite::widgets::stream_graph::StreamGraph;
///
/// assert_eq!(StreamGraph::new().layer_count(), 0);
/// ```
#[derive(Debug)]
pub struct StreamGraph {
    /// Accessibility label.
    pub label: String,
    layers: Vec<Vec<f32>>,
    hovered: Option<usize>,
    pending: Option<usize>,
    bounds: Rect,
    scale: f32,
}

impl Default for StreamGraph {
    fn default() -> Self {
        Self::new()
    }
}

impl StreamGraph {
    /// Creates an empty chart.
    ///
    /// ```
    /// use martensite::widgets::stream_graph::StreamGraph;
    ///
    /// assert_eq!(StreamGraph::new().layer_count(), 0);
    /// ```
    pub fn new() -> Self {
        Self {
            label: "Stream graph".to_string(),
            layers: Vec::new(),
            hovered: None,
            pending: None,
            bounds: Rect::new(0.0, 0.0, 0.0, 0.0),
            scale: 1.0,
        }
    }

    /// Adds an equal-length layer series.
    ///
    /// ```
    /// use martensite::widgets::stream_graph::StreamGraph;
    ///
    /// let s = StreamGraph::new().layer("a", [1.0, 3.0, 2.0]);
    /// assert_eq!(s.layer_count(), 1);
    /// ```
    pub fn layer(mut self, name: &str, values: impl Into<Vec<f32>>) -> Self {
        let _ = name;
        let mut v: Vec<f32> = values.into();
        v.iter_mut().for_each(|x| *x = x.max(0.0));
        if !v.is_empty() {
            self.layers.push(v);
        }
        self
    }

    /// Accessibility label.
    ///
    /// ```
    /// use martensite::widgets::stream_graph::StreamGraph;
    ///
    /// assert_eq!(StreamGraph::new().label("Flow").label, "Flow");
    /// ```
    pub fn label(mut self, label: impl Into<String>) -> Self {
        self.label = label.into();
        self
    }

    /// Layer count.
    ///
    /// ```
    /// use martensite::widgets::stream_graph::StreamGraph;
    ///
    /// assert_eq!(StreamGraph::new().layer("x", [1.0]).layer_count(), 1);
    /// ```
    pub fn layer_count(&self) -> usize {
        self.layers.len()
    }

    /// Sample count (of the longest layer).
    ///
    /// ```
    /// use martensite::widgets::stream_graph::StreamGraph;
    ///
    /// assert_eq!(StreamGraph::new().layer("x", [1.0, 2.0, 3.0]).sample_count(), 3);
    /// ```
    pub fn sample_count(&self) -> usize {
        self.layers.iter().map(Vec::len).max().unwrap_or(0)
    }

    /// Drains the last hovered layer index.
    ///
    /// ```
    /// use martensite::widgets::stream_graph::StreamGraph;
    ///
    /// let mut s = StreamGraph::new();
    /// assert!(s.take_hovered().is_none());
    /// ```
    pub fn take_hovered(&mut self) -> Option<usize> {
        self.pending.take()
    }

    /// Peak total stack height across all samples.
    fn peak(&self) -> f32 {
        let n = self.sample_count();
        (0..n)
            .map(|k| {
                self.layers
                    .iter()
                    .map(|l| l.get(k).copied().unwrap_or(0.0))
                    .sum::<f32>()
            })
            .fold(0.0_f32, f32::max)
    }

    /// Baseline y offset for sample `k` (symmetric stacking).
    fn baseline(&self, k: usize, usable_h: f32) -> f32 {
        let peak = self.peak().max(0.0001);
        let total: f32 = self
            .layers
            .iter()
            .map(|l| l.get(k).copied().unwrap_or(0.0))
            .sum();
        // Center the stack vertically.
        (self.bounds.min_y() + self.bounds.max_y()) / 2.0 - (total / peak) * usable_h / 2.0
    }

    /// Y of the cumulative stack boundary above `layer` at sample `k`.
    fn y_of(&self, layer: usize, k: usize, usable_h: f32) -> f32 {
        let peak = self.peak().max(0.0001);
        let cum: f32 = self
            .layers
            .iter()
            .take(layer + 1)
            .map(|l| l.get(k).copied().unwrap_or(0.0))
            .sum();
        self.baseline(k, usable_h) + (cum / peak) * usable_h
    }

    /// Layer index under a point.
    fn layer_at(&self, p: Vec2) -> Option<usize> {
        let n = self.sample_count();
        if n == 0 || !self.bounds.contains(p) {
            return None;
        }
        let pad = PAD_PT * self.scale;
        let usable_h = (self.bounds.height() - 2.0 * pad).max(1.0);
        let k = (((p.x - self.bounds.min_x() - pad) / (self.bounds.width() - 2.0 * pad).max(1.0))
            * (n - 1) as f32)
            .round() as usize;
        let k = k.min(n - 1);
        // Find which band the point sits in.
        let mut lo = self.baseline(k, usable_h);
        for i in 0..self.layers.len() {
            let hi = self.y_of(i, k, usable_h);
            if p.y >= lo && p.y <= hi {
                return Some(i);
            }
            lo = hi;
        }
        None
    }
}

impl Widget for StreamGraph {
    fn measure(&mut self, cx: &mut LayoutContext, constraints: LayoutConstraints) -> Vec2 {
        Vec2::new(
            cx.pt(WIDTH_PT).min(constraints.max_size.x.max(0.0)),
            cx.pt(HEIGHT_PT).min(constraints.max_size.y.max(0.0)),
        )
    }

    fn min_render(&self) -> RenderMinimum {
        RenderMinimum::new(Vec2::new(80.0, 40.0)).with_policy(UnderflowPolicy::Lint)
    }

    fn layout(&mut self, cx: &mut LayoutContext, bounds: Rect) {
        self.bounds = bounds;
        self.scale = cx.scale;
    }

    fn accessibility(&self, node: &mut AccessKitNode) {
        node.set_role(accesskit::Role::Image);
        node.set_label(format!("{} — {} layers", self.label, self.layers.len()));
    }

    fn event(&mut self, cx: &mut EventContext) -> EventResponse {
        match cx.event {
            WidgetEvent::PointerMoved { position } => {
                let hit = self.layer_at(*position);
                if hit != self.hovered {
                    self.hovered = hit;
                    self.pending = hit;
                    return EventResponse::RequestRepaint;
                }
                EventResponse::Ignored
            }
            WidgetEvent::PointerLeave => {
                if self.hovered.take().is_some() {
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
        let pt = |p: Vec2| (f64::from(p.x), f64::from(p.y));
        cx.list.push_fill_shape(
            f(self.bounds),
            &martensite_core::shape::Shape::rounded(cx.pt(4.0)),
            cx.color(TokenKey::SurfaceColor, FACE),
        );
        let n = self.sample_count();
        if n < 2 {
            return;
        }
        let pad = PAD_PT * self.scale;
        let usable_h = (self.bounds.height() - 2.0 * pad).max(1.0);
        let usable_w = (self.bounds.width() - 2.0 * pad).max(1.0);
        let x_of = |k: usize| self.bounds.min_x() + pad + usable_w * (k as f32 / (n - 1) as f32);
        let edge = cx.color(TokenKey::BorderColor, EDGE);

        for i in 0..self.layers.len() {
            // Band: top boundary left→right, bottom boundary right→left.
            let mut path = kurbo::BezPath::new();
            for k in 0..n {
                let p = pt(Vec2::new(x_of(k), self.y_of(i, k, usable_h)));
                if k == 0 {
                    path.move_to(p);
                } else {
                    path.line_to(p);
                }
            }
            for k in (0..n).rev() {
                let y = if i == 0 {
                    self.baseline(k, usable_h)
                } else {
                    self.y_of(i - 1, k, usable_h)
                };
                path.line_to(pt(Vec2::new(x_of(k), y)));
            }
            path.close_path();
            let base = PALETTE[i % PALETTE.len()];
            let boost = self.hovered == Some(i);
            cx.list.push_path(
                path.clone(),
                cx.color(
                    TokenKey::AccentColor,
                    [base[0], base[1], base[2], if boost { 255 } else { base[3] }],
                ),
            );
            cx.list.push_stroke_path(path, cx.pt(0.5), edge);
        }
        cx.list.push_stroke_shape(
            f(self.bounds),
            &martensite_core::shape::Shape::rounded(cx.pt(4.0)),
            cx.pt(0.75),
            edge,
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use martensite_core::HotNode;

    fn laid_out(s: &mut StreamGraph, w: f32, h: f32) {
        let mut hot = HotNode::default();
        let mut cx = LayoutContext {
            hot: &mut hot,
            scale: 1.0,
        };
        s.measure(
            &mut cx,
            LayoutConstraints {
                min_size: Vec2::ZERO,
                max_size: Vec2::new(w, h),
            },
        );
        s.layout(&mut cx, Rect::new(0.0, 0.0, w, h));
    }

    #[test]
    fn negatives_clamp() {
        let s = StreamGraph::new().layer("a", [-1.0, 2.0]);
        assert_eq!(s.sample_count(), 2);
        // Layer stored clamped.
        assert_eq!(s.layers[0][0], 0.0);
    }

    #[test]
    fn empty_layers_rejected() {
        let s = StreamGraph::new().layer("a", Vec::<f32>::new());
        assert_eq!(s.layer_count(), 0);
    }

    #[test]
    fn hover_middle_band() {
        let mut s = StreamGraph::new()
            .layer("a", [10.0, 10.0])
            .layer("b", [10.0, 10.0]);
        laid_out(&mut s, 260.0, 120.0);
        // Band a occupies the upper half of the stack, b the lower.
        s.event(&mut EventContext {
            event: &WidgetEvent::PointerMoved {
                position: Vec2::new(130.0, 30.0), // upper region
            },
            bounds: Rect::new(0.0, 0.0, 260.0, 120.0),
            scale: 1.0,
        });
        assert_eq!(s.take_hovered(), Some(0));
        s.event(&mut EventContext {
            event: &WidgetEvent::PointerMoved {
                position: Vec2::new(130.0, 90.0), // lower region
            },
            bounds: Rect::new(0.0, 0.0, 260.0, 120.0),
            scale: 1.0,
        });
        assert_eq!(s.take_hovered(), Some(1));
    }

    #[test]
    fn outside_ignored() {
        let mut s = StreamGraph::new().layer("a", [1.0, 2.0]);
        laid_out(&mut s, 260.0, 120.0);
        s.event(&mut EventContext {
            event: &WidgetEvent::PointerMoved {
                position: Vec2::new(500.0, 60.0),
            },
            bounds: Rect::new(0.0, 0.0, 260.0, 120.0),
            scale: 1.0,
        });
        assert_eq!(s.take_hovered(), None);
    }

    #[test]
    fn peak_sums() {
        let s = StreamGraph::new()
            .layer("a", [2.0, 1.0])
            .layer("b", [3.0, 1.0]);
        assert_eq!(s.peak(), 5.0);
    }
}
