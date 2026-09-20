//! `Violin` — a violin plot (mirrored density silhouettes per
//! category — the distribution-shape companion to
//! [`crate::widgets::box_plot::BoxPlot`]).
//!
//! Each `ViolinSeries` is a name plus a density profile (`0..=1`
//! half-widths sampled top→bottom) rendered as a symmetric
//! silhouette with a center line and inner quartile tick — the
//! kernel-density-estimate look. Hovering a violin parks its
//! index in [`Violin::take_hovered`].
//!
//! # Examples
//!
//! ```
//! use martensite::widgets::violin::Violin;
//!
//! let v = Violin::new().series("A", [0.1, 0.9, 1.0, 0.9, 0.1]);
//! assert_eq!(v.series_count(), 1);
//! ```

use accesskit::Node as AccessKitNode;
use glam::Vec2;
use martensite_core::{
    EventContext, EventResponse, LayoutConstraints, LayoutContext, PaintContext, Rect,
    RenderMinimum, UnderflowPolicy, Widget, WidgetEvent,
};
use martensite_theme::TokenKey;

const WIDTH_PT: f32 = 240.0;
const HEIGHT_PT: f32 = 140.0;
const PAD_PT: f32 = 8.0;

const FACE: [u8; 4] = [36, 36, 42, 255];
const EDGE: [u8; 4] = [70, 70, 76, 255];
const MID: [u8; 4] = [220, 220, 228, 255];
const PALETTE: [[u8; 4]; 5] = [
    [110, 170, 230, 160],
    [230, 150, 90, 160],
    [120, 200, 140, 160],
    [220, 110, 110, 160],
    [190, 140, 230, 160],
];

/// A mirrored-density chart — see the module docs.
///
/// ```
/// use martensite::widgets::violin::Violin;
///
/// assert_eq!(Violin::new().series_count(), 0);
/// ```
#[derive(Debug)]
pub struct Violin {
    /// Accessibility label.
    pub label: String,
    series: Vec<(String, Vec<f32>)>,
    hovered: Option<usize>,
    pending: Option<usize>,
    bounds: Rect,
    scale: f32,
}

impl Default for Violin {
    fn default() -> Self {
        Self::new()
    }
}

impl Violin {
    /// Creates an empty chart.
    ///
    /// ```
    /// use martensite::widgets::violin::Violin;
    ///
    /// assert_eq!(Violin::new().series_count(), 0);
    /// ```
    pub fn new() -> Self {
        Self {
            label: "Violin plot".to_string(),
            series: Vec::new(),
            hovered: None,
            pending: None,
            bounds: Rect::new(0.0, 0.0, 0.0, 0.0),
            scale: 1.0,
        }
    }

    /// Adds a named density profile (`0..=1` half-widths, ≥3 samples).
    ///
    /// ```
    /// use martensite::widgets::violin::Violin;
    ///
    /// let v = Violin::new().series("x", [0.2, 1.0, 0.2]);
    /// assert_eq!(v.series_count(), 1);
    /// ```
    pub fn series(mut self, name: impl Into<String>, density: impl Into<Vec<f32>>) -> Self {
        let mut d: Vec<f32> = density.into();
        d.iter_mut().for_each(|v| *v = v.clamp(0.0, 1.0));
        if d.len() >= 3 {
            self.series.push((name.into(), d));
        }
        self
    }

    /// Accessibility label.
    ///
    /// ```
    /// use martensite::widgets::violin::Violin;
    ///
    /// assert_eq!(Violin::new().label("Dist").label, "Dist");
    /// ```
    pub fn label(mut self, label: impl Into<String>) -> Self {
        self.label = label.into();
        self
    }

    /// Series count.
    ///
    /// ```
    /// use martensite::widgets::violin::Violin;
    ///
    /// assert_eq!(Violin::new().series("a", [0.0, 1.0, 0.0]).series_count(), 1);
    /// ```
    pub fn series_count(&self) -> usize {
        self.series.len()
    }

    /// A series' density profile.
    ///
    /// ```
    /// use martensite::widgets::violin::Violin;
    ///
    /// let v = Violin::new().series("a", [0.0, 0.5, 0.0]);
    /// assert_eq!(v.density(0), &[0.0, 0.5, 0.0]);
    /// ```
    pub fn density(&self, index: usize) -> &[f32] {
        self.series
            .get(index)
            .map(|(_, d)| d.as_slice())
            .unwrap_or(&[])
    }

    /// Drains the last hovered series index.
    ///
    /// ```
    /// use martensite::widgets::violin::Violin;
    ///
    /// let mut v = Violin::new();
    /// assert!(v.take_hovered().is_none());
    /// ```
    pub fn take_hovered(&mut self) -> Option<usize> {
        self.pending.take()
    }

    /// Series index under a point (by column slot).
    fn series_at(&self, p: Vec2) -> Option<usize> {
        if self.series.is_empty() || !self.bounds.contains(p) {
            return None;
        }
        let w = self.bounds.width() / self.series.len() as f32;
        let i = ((p.x - self.bounds.min_x()) / w.max(1.0)) as usize;
        Some(i.min(self.series.len() - 1))
    }
}

impl Widget for Violin {
    fn measure(&mut self, cx: &mut LayoutContext, constraints: LayoutConstraints) -> Vec2 {
        Vec2::new(
            cx.pt(WIDTH_PT).min(constraints.max_size.x.max(0.0)),
            cx.pt(HEIGHT_PT).min(constraints.max_size.y.max(0.0)),
        )
    }

    fn min_render(&self) -> RenderMinimum {
        RenderMinimum::new(Vec2::new(80.0, 50.0)).with_policy(UnderflowPolicy::Lint)
    }

    fn layout(&mut self, cx: &mut LayoutContext, bounds: Rect) {
        self.bounds = bounds;
        self.scale = cx.scale;
    }

    fn accessibility(&self, node: &mut AccessKitNode) {
        node.set_role(accesskit::Role::Image);
        node.set_label(format!("{} — {} series", self.label, self.series.len()));
    }

    fn event(&mut self, cx: &mut EventContext) -> EventResponse {
        match cx.event {
            WidgetEvent::PointerMoved { position } => {
                let hit = self.series_at(*position);
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
        let pad = PAD_PT * self.scale;
        let slot_w = self.bounds.width() / self.series.len().max(1) as f32;
        let half_w = (slot_w - 2.0 * pad) / 2.0;
        let inner_h = self.bounds.height() - 2.0 * pad;
        let mid_c = cx.color(TokenKey::TextColor, MID);
        let edge = cx.color(TokenKey::BorderColor, EDGE);

        for (i, (_, density)) in self.series.iter().enumerate() {
            let cx_line = self.bounds.min_x() + i as f32 * slot_w + slot_w / 2.0;
            let n = density.len();
            // Mirrored silhouette: up the right side, back down the left.
            let mut path = kurbo::BezPath::new();
            for (k, &d) in density.iter().enumerate() {
                let t = k as f32 / (n - 1).max(1) as f32;
                let p = pt(Vec2::new(
                    cx_line + d * half_w,
                    self.bounds.min_y() + pad + t * inner_h,
                ));
                if k == 0 {
                    path.move_to(p);
                } else {
                    path.line_to(p);
                }
            }
            for k in (0..n).rev() {
                let t = k as f32 / (n - 1).max(1) as f32;
                path.line_to(pt(Vec2::new(
                    cx_line - density[k] * half_w,
                    self.bounds.min_y() + pad + t * inner_h,
                )));
            }
            path.close_path();
            cx.list.push_path(
                path.clone(),
                cx.color(TokenKey::AccentColor, PALETTE[i % PALETTE.len()]),
            );
            cx.list.push_stroke_path(path, cx.pt(0.75), edge);
            // Center line + quartile ticks at 25/50/75%.
            let mut mid = kurbo::BezPath::new();
            mid.move_to(pt(Vec2::new(cx_line, self.bounds.min_y() + pad)));
            mid.line_to(pt(Vec2::new(cx_line, self.bounds.max_y() - pad)));
            cx.list.push_stroke_path(mid, cx.pt(0.75), mid_c);
            for &q in &[0.25_f32, 0.5, 0.75] {
                let y = self.bounds.min_y() + pad + q * inner_h;
                let idx = (q * (n - 1) as f32) as usize;
                let hw = density[idx.min(n - 1)] * half_w * 0.7;
                let mut t = kurbo::BezPath::new();
                t.move_to(pt(Vec2::new(cx_line - hw, y)));
                t.line_to(pt(Vec2::new(cx_line + hw, y)));
                cx.list
                    .push_stroke_path(t, cx.pt(if q == 0.5 { 1.5 } else { 0.75 }), mid_c);
            }
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

    fn laid_out(v: &mut Violin, w: f32, h: f32) {
        let mut hot = HotNode::default();
        let mut cx = LayoutContext {
            hot: &mut hot,
            scale: 1.0,
        };
        v.measure(
            &mut cx,
            LayoutConstraints {
                min_size: Vec2::ZERO,
                max_size: Vec2::new(w, h),
            },
        );
        v.layout(&mut cx, Rect::new(0.0, 0.0, w, h));
    }

    #[test]
    fn clamps_density() {
        let v = Violin::new().series("a", [0.0, 5.0, -2.0]);
        assert_eq!(v.density(0), &[0.0, 1.0, 0.0]);
    }

    #[test]
    fn short_profiles_rejected() {
        let v = Violin::new().series("a", [0.5]);
        assert_eq!(v.series_count(), 0);
    }

    #[test]
    fn hover_parks_slot() {
        let mut v = Violin::new()
            .series("a", [0.1, 1.0, 0.1])
            .series("b", [0.3, 0.8, 0.3]);
        laid_out(&mut v, 240.0, 140.0);
        v.event(&mut EventContext {
            event: &WidgetEvent::PointerMoved {
                position: Vec2::new(200.0, 70.0), // right half → series 1
            },
            bounds: Rect::new(0.0, 0.0, 240.0, 140.0),
            scale: 1.0,
        });
        assert_eq!(v.take_hovered(), Some(1));
    }

    #[test]
    fn leave_clears() {
        let mut v = Violin::new().series("a", [0.1, 1.0, 0.1]);
        laid_out(&mut v, 240.0, 140.0);
        v.event(&mut EventContext {
            event: &WidgetEvent::PointerMoved {
                position: Vec2::new(100.0, 70.0),
            },
            bounds: Rect::new(0.0, 0.0, 240.0, 140.0),
            scale: 1.0,
        });
        assert!(v.take_hovered().is_some());
        v.event(&mut EventContext {
            event: &WidgetEvent::PointerLeave,
            bounds: Rect::new(0.0, 0.0, 240.0, 140.0),
            scale: 1.0,
        });
        assert_eq!(v.hovered, None);
    }
}
