//! `ActivityRing` — concentric progress rings (Apple Watch
//! Activity / `HKActivitySummary` idiom).
//!
//! Up to a handful of named rings paint as nested arcs, each a
//! fraction `0..=1` of a full circle starting at twelve o'clock.
//! Center text shows the overall average when a painter is
//! present. Ring colors come from the caller or the theme palette.
//!
//! # Examples
//!
//! ```
//! use martensite::widgets::activity_ring::ActivityRing;
//!
//! let rings = ActivityRing::new()
//!     .ring("Move", 0.8, [255, 60, 80, 255])
//!     .ring("Exercise", 0.5, [160, 255, 60, 255]);
//! assert_eq!(rings.ring_count(), 2);
//! ```

use accesskit::Node as AccessKitNode;
use glam::Vec2;
use martensite_core::{
    LayoutConstraints, LayoutContext, PaintContext, Rect, RenderMinimum, UnderflowPolicy, Widget,
};
use martensite_theme::TokenKey;

use crate::text_paint::SharedTextPainter;

const SIZE_PT: f32 = 120.0;
const RING_W_PT: f32 = 9.0;
const GAP_PT: f32 = 3.0;
const MAX_RINGS: usize = 5;

const TRACK: [u8; 4] = [60, 60, 66, 255];
const FG: [u8; 4] = [230, 230, 235, 255];
const FALLBACK_COLORS: [[u8; 4]; MAX_RINGS] = [
    [255, 60, 80, 255],
    [160, 255, 60, 255],
    [60, 200, 255, 255],
    [230, 170, 80, 255],
    [180, 120, 255, 255],
];

/// One concentric ring — name, fraction, RGBA color.
#[derive(Clone, Debug, PartialEq)]
pub struct Ring {
    /// Ring name (a11y summary).
    pub name: String,
    /// Completion fraction `0..=1`.
    pub fraction: f32,
    /// Arc color.
    pub color: [u8; 4],
}

/// Concentric progress rings — see the module docs.
///
/// ```
/// use martensite::widgets::activity_ring::ActivityRing;
///
/// assert_eq!(ActivityRing::new().ring_count(), 0);
/// ```
pub struct ActivityRing {
    /// Accessibility label.
    pub label: String,
    rings: Vec<Ring>,
    bounds: Rect,
    text_painter: Option<SharedTextPainter>,
}

impl Default for ActivityRing {
    fn default() -> Self {
        Self::new()
    }
}

impl ActivityRing {
    /// Creates an empty ring set.
    ///
    /// ```
    /// use martensite::widgets::activity_ring::ActivityRing;
    ///
    /// assert_eq!(ActivityRing::new().ring_count(), 0);
    /// ```
    pub fn new() -> Self {
        Self {
            label: "Activity".to_string(),
            rings: Vec::new(),
            bounds: Rect::new(0.0, 0.0, 0.0, 0.0),
            text_painter: None,
        }
    }

    /// Accessibility label.
    ///
    /// ```
    /// use martensite::widgets::activity_ring::ActivityRing;
    ///
    /// let a = ActivityRing::new().label("Today");
    /// assert_eq!(a.label, "Today");
    /// ```
    pub fn label(mut self, label: impl Into<String>) -> Self {
        self.label = label.into();
        self
    }

    /// Appends a ring (outermost first, max 5).
    ///
    /// ```
    /// use martensite::widgets::activity_ring::ActivityRing;
    ///
    /// let a = ActivityRing::new().ring("Move", 0.6, [255, 60, 80, 255]);
    /// assert_eq!(a.ring_count(), 1);
    /// assert_eq!(a.rings()[0].fraction, 0.6);
    /// ```
    pub fn ring(mut self, name: impl Into<String>, fraction: f32, color: [u8; 4]) -> Self {
        if self.rings.len() < MAX_RINGS {
            self.rings.push(Ring {
                name: name.into(),
                fraction: fraction.clamp(0.0, 1.0),
                color,
            });
        }
        self
    }

    /// Explicit painter override — see [`crate::text_paint`].
    ///
    /// ```
    /// use martensite::widgets::activity_ring::ActivityRing;
    /// use martensite::text_paint::shared_painter;
    ///
    /// let a = ActivityRing::new().with_text_painter(shared_painter());
    /// assert_eq!(a.ring_count(), 0);
    /// ```
    pub fn with_text_painter(mut self, painter: SharedTextPainter) -> Self {
        self.text_painter = Some(painter);
        self
    }

    /// Ring count.
    ///
    /// ```
    /// use martensite::widgets::activity_ring::ActivityRing;
    ///
    /// assert_eq!(ActivityRing::new().ring_count(), 0);
    /// ```
    pub fn ring_count(&self) -> usize {
        self.rings.len()
    }

    /// Ring list.
    ///
    /// ```
    /// use martensite::widgets::activity_ring::ActivityRing;
    ///
    /// assert!(ActivityRing::new().rings().is_empty());
    /// ```
    pub fn rings(&self) -> &[Ring] {
        &self.rings
    }

    /// Average completion across rings.
    ///
    /// ```
    /// use martensite::widgets::activity_ring::ActivityRing;
    ///
    /// let a = ActivityRing::new().ring("A", 0.5, [0, 0, 0, 255]);
    /// assert_eq!(a.average(), 0.5);
    /// ```
    pub fn average(&self) -> f32 {
        if self.rings.is_empty() {
            return 0.0;
        }
        self.rings.iter().map(|r| r.fraction).sum::<f32>() / self.rings.len() as f32
    }
}

impl Widget for ActivityRing {
    fn measure(&mut self, cx: &mut LayoutContext, constraints: LayoutConstraints) -> Vec2 {
        let s = cx.pt(SIZE_PT);
        Vec2::new(
            s.min(constraints.max_size.x.max(0.0)),
            s.min(constraints.max_size.y.max(0.0)),
        )
    }

    fn min_render(&self) -> RenderMinimum {
        RenderMinimum::new(Vec2::new(32.0, 32.0)).with_policy(UnderflowPolicy::Lint)
    }

    fn layout(&mut self, _cx: &mut LayoutContext, bounds: Rect) {
        self.bounds = bounds;
    }

    fn accessibility(&self, node: &mut AccessKitNode) {
        node.set_role(accesskit::Role::Image);
        node.set_label(self.label.clone());
        let summary = self
            .rings
            .iter()
            .map(|r| format!("{} {:.0}%", r.name, r.fraction * 100.0))
            .collect::<Vec<_>>()
            .join(", ");
        node.set_description(summary);
    }

    fn paint(&self, cx: &mut PaintContext) {
        let dim = self.bounds.width().min(self.bounds.height());
        let center = Vec2::new(
            self.bounds.min_x() + self.bounds.width() / 2.0,
            self.bounds.min_y() + self.bounds.height() / 2.0,
        );
        let ring_w = cx.pt(RING_W_PT);
        let gap = cx.pt(GAP_PT);
        let outer_r = dim / 2.0 - ring_w / 2.0;
        let track = cx.color(TokenKey::BorderColor, TRACK);
        let painter = crate::text_paint::resolve_painter(&self.text_painter, cx.text_painter);

        for (i, ring) in self.rings.iter().enumerate() {
            let r = outer_r - i as f32 * (ring_w + gap);
            if r <= ring_w {
                break;
            }
            let color = if ring.color == [0, 0, 0, 0] {
                FALLBACK_COLORS[i % FALLBACK_COLORS.len()]
            } else {
                ring.color
            };
            // Track arc — full circle at low alpha.
            let mut track_path = kurbo::BezPath::new();
            track_path.move_to((f64::from(center.x), f64::from(center.y - r)));
            track_path.extend(circle_el(center, r, -90.0, 360.0));
            cx.list
                .push_stroke_path(track_path, ring_w, [track[0], track[1], track[2], 110]);
            // Value arc from twelve o-clock.
            let sweep = ring.fraction * 360.0;
            if sweep > 0.0 {
                let mut path = kurbo::BezPath::new();
                path.move_to((f64::from(center.x), f64::from(center.y - r)));
                path.extend(circle_el(center, r, -90.0, sweep));
                cx.list.push_stroke_path(path, ring_w, color);
            }
        }

        // Center average.
        let pct = format!("{:.0}%", self.average() * 100.0);
        let size = 13.0 * cx.scale;
        let w = painter
            .and_then(|p| p.measure_text(&pct, size))
            .unwrap_or(pct.chars().count() as f32 * size * 0.6);
        crate::text_paint::paint_label_clipped(
            painter,
            cx.list,
            kurbo::Rect::new(
                f64::from(center.x - dim / 4.0),
                f64::from(center.y - size),
                f64::from(center.x + dim / 4.0),
                f64::from(center.y + size),
            ),
            kurbo::Point::new(
                f64::from(center.x - w / 2.0),
                f64::from(center.y - size / 2.0),
            ),
            &pct,
            size,
            cx.color(TokenKey::TextColor, FG),
        );
    }
}

/// Approximates a circle arc with line segments — `start` and
/// `sweep` in degrees, zero at twelve o'clock.
fn circle_el(center: Vec2, r: f32, start_deg: f32, sweep_deg: f32) -> Vec<kurbo::PathEl> {
    let segs = ((sweep_deg.abs() / 5.0).ceil() as usize).max(4);
    let mut els = Vec::with_capacity(segs);
    for i in 1..=segs {
        let a = (start_deg + sweep_deg * i as f32 / segs as f32).to_radians();
        els.push(kurbo::PathEl::LineTo(kurbo::Point::new(
            f64::from(center.x + r * a.cos()),
            f64::from(center.y + r * a.sin()),
        )));
    }
    els
}

impl std::fmt::Debug for ActivityRing {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ActivityRing")
            .field("rings", &self.rings.len())
            .finish()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rings_accumulate() {
        let a = ActivityRing::new()
            .ring("Move", 0.8, [255, 0, 0, 255])
            .ring("Exercise", 0.5, [0, 255, 0, 255])
            .ring("Stand", 0.9, [0, 0, 255, 255]);
        assert_eq!(a.ring_count(), 3);
        assert_eq!(a.rings()[1].name, "Exercise");
    }

    #[test]
    fn fraction_clamps() {
        let a = ActivityRing::new().ring("A", 1.5, [0, 0, 0, 255]);
        assert_eq!(a.rings()[0].fraction, 1.0);
    }

    #[test]
    fn caps_at_five_rings() {
        let mut a = ActivityRing::new();
        for i in 0..8 {
            a = a.ring(format!("r{i}"), 0.5, [0, 0, 0, 255]);
        }
        assert_eq!(a.ring_count(), 5);
    }

    #[test]
    fn average() {
        let a = ActivityRing::new()
            .ring("A", 0.6, [0, 0, 0, 255])
            .ring("B", 0.4, [0, 0, 0, 255]);
        assert_eq!(a.average(), 0.5);
        assert_eq!(ActivityRing::new().average(), 0.0);
    }

    #[test]
    fn measure_square() {
        let mut a = ActivityRing::new();
        let mut hot = martensite_core::HotNode::default();
        let mut cx = LayoutContext {
            hot: &mut hot,
            scale: 2.0,
        };
        let s = a.measure(
            &mut cx,
            LayoutConstraints {
                min_size: Vec2::ZERO,
                max_size: Vec2::new(500.0, 500.0),
            },
        );
        assert_eq!(s.x, s.y);
    }
}
