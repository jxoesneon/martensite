//! `PolarArea` — a polar area chart (Nightingale / Coxcomb rose
//! idiom, Ant `PolarArea`).
//!
//! Every wedge spans an equal angle; each wedge's *radius* encodes
//! its value — the rose-chart alternative to `PieChart` (where the
//! angle varies) and `RadarChart` (where points join a polygon).
//! Hovering a wedge parks its index in
//! [`PolarArea::take_hovered`].
//!
//! # Examples
//!
//! ```
//! use martensite::widgets::polar_area::PolarArea;
//!
//! let p = PolarArea::new().slice("A", 4.0).slice("B", 9.0);
//! assert_eq!(p.slice_count(), 2);
//! ```

use accesskit::Node as AccessKitNode;
use glam::Vec2;
use martensite_core::{
    EventContext, EventResponse, LayoutConstraints, LayoutContext, PaintContext, Rect,
    RenderMinimum, UnderflowPolicy, Widget, WidgetEvent,
};
use martensite_theme::TokenKey;

const SIZE_PT: f32 = 160.0;

const PALETTE: [[u8; 4]; 6] = [
    [110, 170, 230, 255],
    [230, 150, 90, 255],
    [120, 200, 140, 255],
    [220, 110, 110, 255],
    [190, 140, 230, 255],
    [230, 210, 120, 255],
];
const EDGE: [u8; 4] = [70, 70, 76, 255];
const HOT_ALPHA: u8 = 200;
const BASE_ALPHA: u8 = 150;

/// A polar area chart — see the module docs.
///
/// ```
/// use martensite::widgets::polar_area::PolarArea;
///
/// assert_eq!(PolarArea::new().slice_count(), 0);
/// ```
#[derive(Debug)]
pub struct PolarArea {
    /// Accessibility label.
    pub label: String,
    slices: Vec<(String, f32)>,
    hovered: Option<usize>,
    pending: Option<usize>,
    bounds: Rect,
    scale: f32,
}

impl Default for PolarArea {
    fn default() -> Self {
        Self::new()
    }
}

impl PolarArea {
    /// Creates an empty chart.
    ///
    /// ```
    /// use martensite::widgets::polar_area::PolarArea;
    ///
    /// assert_eq!(PolarArea::new().slice_count(), 0);
    /// ```
    pub fn new() -> Self {
        Self {
            label: "Polar area".to_string(),
            slices: Vec::new(),
            hovered: None,
            pending: None,
            bounds: Rect::new(0.0, 0.0, 0.0, 0.0),
            scale: 1.0,
        }
    }

    /// Adds a labeled value wedge.
    ///
    /// ```
    /// use martensite::widgets::polar_area::PolarArea;
    ///
    /// let p = PolarArea::new().slice("Q1", 3.0);
    /// assert_eq!(p.slice_count(), 1);
    /// ```
    pub fn slice(mut self, label: impl Into<String>, value: f32) -> Self {
        self.slices.push((label.into(), value.max(0.0)));
        self
    }

    /// Replaces all slices.
    ///
    /// ```
    /// use martensite::widgets::polar_area::PolarArea;
    ///
    /// let p = PolarArea::new().slices(vec![("a".to_string(), 2.0)]);
    /// assert_eq!(p.slice_count(), 1);
    /// ```
    pub fn slices(mut self, slices: Vec<(String, f32)>) -> Self {
        self.slices = slices;
        self
    }

    /// Accessibility label.
    ///
    /// ```
    /// use martensite::widgets::polar_area::PolarArea;
    ///
    /// assert_eq!(PolarArea::new().label("Rain").label, "Rain");
    /// ```
    pub fn label(mut self, label: impl Into<String>) -> Self {
        self.label = label.into();
        self
    }

    /// Wedge count.
    ///
    /// ```
    /// use martensite::widgets::polar_area::PolarArea;
    ///
    /// assert_eq!(PolarArea::new().slice("x", 1.0).slice_count(), 1);
    /// ```
    pub fn slice_count(&self) -> usize {
        self.slices.len()
    }

    /// The largest value.
    ///
    /// ```
    /// use martensite::widgets::polar_area::PolarArea;
    ///
    /// assert_eq!(PolarArea::new().slice("x", 7.0).max_value(), 7.0);
    /// ```
    pub fn max_value(&self) -> f32 {
        self.slices.iter().map(|(_, v)| *v).fold(0.0_f32, f32::max)
    }

    /// Drains the last hovered wedge index.
    ///
    /// ```
    /// use martensite::widgets::polar_area::PolarArea;
    ///
    /// let mut p = PolarArea::new();
    /// assert!(p.take_hovered().is_none());
    /// ```
    pub fn take_hovered(&mut self) -> Option<usize> {
        self.pending.take()
    }

    fn center(&self) -> Vec2 {
        Vec2::new(
            self.bounds.origin.x + self.bounds.size.x / 2.0,
            self.bounds.origin.y + self.bounds.size.y / 2.0,
        )
    }

    fn radius(&self) -> f32 {
        self.bounds.width().min(self.bounds.height()) / 2.0
    }

    /// Wedge radius for a value (sqrt keeps area linear in value).
    fn wedge_r(&self, value: f32) -> f32 {
        (value / self.max_value().max(0.0001)).sqrt() * self.radius()
    }

    /// Wedge index under a point.
    fn slice_at(&self, p: Vec2) -> Option<usize> {
        let n = self.slices.len();
        if n == 0 {
            return None;
        }
        let c = self.center();
        let d = p - c;
        let dist = d.length();
        if dist > self.radius() {
            return None;
        }
        let wedge = std::f32::consts::TAU / n as f32;
        // Angle from -y (north), clockwise.
        let mut a = d.y.atan2(d.x) + std::f32::consts::FRAC_PI_2;
        if a < 0.0 {
            a += std::f32::consts::TAU;
        }
        let i = (a / wedge) as usize % n;
        (dist <= self.wedge_r(self.slices[i].1)).then_some(i)
    }

    /// Wedge path from `a0` to `a1` at radius `r` around `c`.
    fn wedge_path(c: Vec2, r: f32, a0: f32, a1: f32) -> kurbo::BezPath {
        let mut p = kurbo::BezPath::new();
        p.move_to((f64::from(c.x), f64::from(c.y)));
        let steps = ((a1 - a0).abs() / 0.15).ceil().max(2.0) as usize;
        for i in 0..=steps {
            let a = a0 + (a1 - a0) * (i as f32 / steps as f32);
            p.line_to((f64::from(c.x + r * a.cos()), f64::from(c.y + r * a.sin())));
        }
        p.close_path();
        p
    }
}

impl Widget for PolarArea {
    fn measure(&mut self, cx: &mut LayoutContext, constraints: LayoutConstraints) -> Vec2 {
        let s = cx.pt(SIZE_PT);
        Vec2::new(
            s.min(constraints.max_size.x.max(0.0)),
            s.min(constraints.max_size.y.max(0.0)),
        )
    }

    fn min_render(&self) -> RenderMinimum {
        RenderMinimum::new(Vec2::new(50.0, 50.0)).with_policy(UnderflowPolicy::Lint)
    }

    fn layout(&mut self, cx: &mut LayoutContext, bounds: Rect) {
        self.bounds = bounds;
        self.scale = cx.scale;
    }

    fn accessibility(&self, node: &mut AccessKitNode) {
        node.set_role(accesskit::Role::Image);
        node.set_label(format!("{} — {} slices", self.label, self.slices.len()));
    }

    fn event(&mut self, cx: &mut EventContext) -> EventResponse {
        match cx.event {
            WidgetEvent::PointerMoved { position } => {
                let hit = self.slice_at(*position);
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
        let n = self.slices.len();
        if n == 0 {
            return;
        }
        let c = self.center();
        let wedge = std::f32::consts::TAU / n as f32;
        let start = -std::f32::consts::FRAC_PI_2; // first wedge at north
        let edge = cx.color(TokenKey::BorderColor, EDGE);
        for (i, (_, v)) in self.slices.iter().enumerate() {
            let base = PALETTE[i % PALETTE.len()];
            let alpha = if self.hovered == Some(i) {
                HOT_ALPHA
            } else {
                BASE_ALPHA
            };
            let color = cx.color(TokenKey::AccentColor, [base[0], base[1], base[2], alpha]);
            let a0 = start + i as f32 * wedge;
            let path = Self::wedge_path(c, self.wedge_r(*v), a0, a0 + wedge);
            cx.list.push_path(path.clone(), color);
            cx.list.push_stroke_path(path, cx.pt(0.75), edge);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use martensite_core::HotNode;

    fn laid_out(p: &mut PolarArea, w: f32, h: f32) {
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
    fn clamps_negative() {
        let p = PolarArea::new().slice("x", -5.0);
        assert_eq!(p.max_value(), 0.0);
    }

    #[test]
    fn hover_center_hits_smallest() {
        // With a tiny first wedge, the center still maps to wedge 0.
        let mut p = PolarArea::new().slice("tiny", 0.01).slice("big", 100.0);
        laid_out(&mut p, 160.0, 160.0);
        p.event(&mut EventContext {
            event: &WidgetEvent::PointerMoved {
                position: Vec2::new(80.0, 80.0), // dead center
            },
            bounds: Rect::new(0.0, 0.0, 160.0, 160.0),
            scale: 1.0,
        });
        assert_eq!(p.take_hovered(), Some(0));
    }

    #[test]
    fn hover_north_hits_first() {
        let mut p = PolarArea::new().slice("a", 10.0).slice("b", 10.0);
        laid_out(&mut p, 160.0, 160.0);
        p.event(&mut EventContext {
            event: &WidgetEvent::PointerMoved {
                position: Vec2::new(80.0, 20.0), // north of center
            },
            bounds: Rect::new(0.0, 0.0, 160.0, 160.0),
            scale: 1.0,
        });
        assert_eq!(p.take_hovered(), Some(0));
    }

    #[test]
    fn outside_ring_ignored() {
        let mut p = PolarArea::new().slice("a", 1.0);
        laid_out(&mut p, 160.0, 160.0);
        p.event(&mut EventContext {
            event: &WidgetEvent::PointerMoved {
                position: Vec2::new(200.0, 80.0),
            },
            bounds: Rect::new(0.0, 0.0, 160.0, 160.0),
            scale: 1.0,
        });
        assert_eq!(p.take_hovered(), None);
    }

    #[test]
    fn small_wedge_rejects_far_point() {
        // Slice 0 is tiny: a far-north point is outside its radius.
        let mut p = PolarArea::new().slice("tiny", 1.0).slice("huge", 100.0);
        laid_out(&mut p, 160.0, 160.0);
        p.event(&mut EventContext {
            event: &WidgetEvent::PointerMoved {
                position: Vec2::new(80.0, 5.0), // near top edge — beyond tiny's radius
            },
            bounds: Rect::new(0.0, 0.0, 160.0, 160.0),
            scale: 1.0,
        });
        assert_eq!(p.take_hovered(), None);
    }
}
