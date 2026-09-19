//! `Compass` — a cardinal heading indicator (navigation /
//! embedded-instrument idiom).
//!
//! A circular dial with `N E S W` cardinal labels and minor tick
//! marks; the needle points at `heading` degrees (0 = north,
//! clockwise). Display-only — the companion to
//! [`crate::widgets::analog_clock::AnalogClock`].
//!
//! # Examples
//!
//! ```
//! use martensite::widgets::compass::Compass;
//!
//! let c = Compass::new().heading(90.0);
//! assert_eq!(c.heading_value(), 90.0);
//! ```

use accesskit::Node as AccessKitNode;
use glam::Vec2;
use martensite_core::{
    EventContext, EventResponse, LayoutConstraints, LayoutContext, PaintContext, Rect,
    RenderMinimum, UnderflowPolicy, Widget,
};
use martensite_theme::TokenKey;

use crate::text_paint::{paint_label_clipped, SharedTextPainter};

const SIZE_PT: f32 = 64.0;
const FONT_PT: f32 = 9.0;

const FACE: [u8; 4] = [36, 36, 42, 255];
const EDGE: [u8; 4] = [70, 70, 76, 255];
const TICK: [u8; 4] = [140, 140, 148, 255];
const NORTH: [u8; 4] = [220, 90, 80, 255];
const NEEDLE: [u8; 4] = [230, 230, 235, 255];

/// A cardinal heading indicator — see the module docs.
///
/// ```
/// use martensite::widgets::compass::Compass;
///
/// assert_eq!(Compass::new().heading_value(), 0.0);
/// ```
pub struct Compass {
    /// Accessibility label.
    pub label: String,
    heading: f32,
    painter: Option<SharedTextPainter>,
    bounds: Rect,
    scale: f32,
}

impl Default for Compass {
    fn default() -> Self {
        Self::new()
    }
}

impl Compass {
    /// Creates a north-facing compass.
    ///
    /// ```
    /// use martensite::widgets::compass::Compass;
    ///
    /// assert_eq!(Compass::new().heading_value(), 0.0);
    /// ```
    pub fn new() -> Self {
        Self {
            label: "Compass".to_string(),
            heading: 0.0,
            painter: None,
            bounds: Rect::new(0.0, 0.0, 0.0, 0.0),
            scale: 1.0,
        }
    }

    /// Heading in degrees (normalized to `0..360`).
    ///
    /// ```
    /// use martensite::widgets::compass::Compass;
    ///
    /// assert_eq!(Compass::new().heading(370.0).heading_value(), 10.0);
    /// ```
    pub fn heading(mut self, heading: f32) -> Self {
        self.heading = heading.rem_euclid(360.0);
        self
    }

    /// Accessibility label.
    ///
    /// ```
    /// use martensite::widgets::compass::Compass;
    ///
    /// assert_eq!(Compass::new().label("Bow").label, "Bow");
    /// ```
    pub fn label(mut self, label: impl Into<String>) -> Self {
        self.label = label.into();
        self
    }

    /// Shared text painter for glyph-accurate layout.
    ///
    /// ```
    /// use martensite::widgets::compass::Compass;
    /// use martensite::text_paint::shared_painter;
    ///
    /// let c = Compass::new().with_text_painter(shared_painter());
    /// assert_eq!(c.heading_value(), 0.0);
    /// ```
    pub fn with_text_painter(mut self, painter: SharedTextPainter) -> Self {
        self.painter = Some(painter);
        self
    }

    /// Heading in degrees.
    ///
    /// ```
    /// use martensite::widgets::compass::Compass;
    ///
    /// assert_eq!(Compass::new().heading(-90.0).heading_value(), 270.0);
    /// ```
    pub fn heading_value(&self) -> f32 {
        self.heading
    }

    /// Sets the heading (normalized).
    ///
    /// ```
    /// use martensite::widgets::compass::Compass;
    ///
    /// let mut c = Compass::new();
    /// c.set_heading(45.0);
    /// assert_eq!(c.heading_value(), 45.0);
    /// ```
    pub fn set_heading(&mut self, heading: f32) {
        self.heading = heading.rem_euclid(360.0);
    }

    /// Nearest cardinal/intercardinal name.
    ///
    /// ```
    /// use martensite::widgets::compass::Compass;
    ///
    /// assert_eq!(Compass::new().heading(95.0).cardinal(), "E");
    /// assert_eq!(Compass::new().heading(50.0).cardinal(), "NE");
    /// ```
    pub fn cardinal(&self) -> &'static str {
        const NAMES: [&str; 8] = ["N", "NE", "E", "SE", "S", "SW", "W", "NW"];
        NAMES[((self.heading + 22.5) / 45.0) as usize % 8]
    }
}

impl Widget for Compass {
    fn measure(&mut self, cx: &mut LayoutContext, constraints: LayoutConstraints) -> Vec2 {
        let s = cx.pt(SIZE_PT);
        Vec2::new(
            s.min(constraints.max_size.x.max(0.0)),
            s.min(constraints.max_size.y.max(0.0)),
        )
    }

    fn min_render(&self) -> RenderMinimum {
        RenderMinimum::new(Vec2::new(28.0, 28.0)).with_policy(UnderflowPolicy::Lint)
    }

    fn layout(&mut self, cx: &mut LayoutContext, bounds: Rect) {
        self.bounds = bounds;
        self.scale = cx.scale;
    }

    fn accessibility(&self, node: &mut AccessKitNode) {
        node.set_role(accesskit::Role::Image);
        node.set_label(format!(
            "{} — {:.0}° {}",
            self.label,
            self.heading,
            self.cardinal()
        ));
    }

    fn event(&mut self, _cx: &mut EventContext) -> EventResponse {
        EventResponse::Ignored
    }

    fn paint(&self, cx: &mut PaintContext) {
        let center = Vec2::new(
            self.bounds.origin.x + self.bounds.size.x / 2.0,
            self.bounds.origin.y + self.bounds.size.y / 2.0,
        );
        let r = self.bounds.width().min(self.bounds.height()) / 2.0 - cx.pt(2.0);
        let pt = |p: Vec2| (f64::from(p.x), f64::from(p.y));
        let f = |r2: Rect| {
            kurbo::Rect::new(
                f64::from(r2.min_x()),
                f64::from(r2.min_y()),
                f64::from(r2.max_x()),
                f64::from(r2.max_y()),
            )
        };

        // Face + rim.
        cx.list.push_fill_shape(
            f(self.bounds),
            &martensite_core::shape::Shape::circle(center, r),
            cx.color(TokenKey::SurfaceColor, FACE),
        );
        cx.list.push_stroke_shape(
            f(self.bounds),
            &martensite_core::shape::Shape::circle(center, r),
            cx.pt(1.0),
            cx.color(TokenKey::BorderColor, EDGE),
        );

        // Ticks every 30° — long at cardinals.
        let tick_c = cx.color(TokenKey::TextMutedColor, TICK);
        for d in (0..360).step_by(30) {
            let a = (d as f32).to_radians();
            let len = if d % 90 == 0 { r * 0.22 } else { r * 0.12 };
            let mut t = kurbo::BezPath::new();
            t.move_to(pt(Vec2::new(
                center.x + (r - len) * a.sin(),
                center.y - (r - len) * a.cos(),
            )));
            t.line_to(pt(Vec2::new(
                center.x + r * a.sin(),
                center.y - r * a.cos(),
            )));
            cx.list.push_stroke_path(t, cx.pt(0.75), tick_c);
        }

        // Cardinal letters.
        let painter = crate::text_paint::resolve_painter(&self.painter, cx.text_painter);
        let size = FONT_PT * self.scale;
        for (i, letter) in ["N", "E", "S", "W"].iter().enumerate() {
            let a = (i as f32 * 90.0).to_radians();
            let lr = r * 0.62;
            let p = Vec2::new(center.x + lr * a.sin(), center.y - lr * a.cos());
            let w = size * 0.62;
            let color = if i == 0 {
                cx.color(TokenKey::ErrorColor, NORTH)
            } else {
                cx.color(TokenKey::TextColor, NEEDLE)
            };
            paint_label_clipped(
                painter,
                cx.list,
                f(self.bounds),
                kurbo::Point::new(f64::from(p.x - w / 2.0), f64::from(p.y - size / 2.0)),
                letter,
                size,
                color,
            );
        }

        // Needle — two-tone diamond (north half red).
        let a = self.heading.to_radians();
        let tip = Vec2::new(center.x + r * 0.8 * a.sin(), center.y - r * 0.8 * a.cos());
        let tail = Vec2::new(center.x - r * 0.55 * a.sin(), center.y + r * 0.55 * a.cos());
        let side = Vec2::new(a.cos(), a.sin()) * (r * 0.14);
        let mut half1 = kurbo::BezPath::new();
        half1.move_to(pt(tip));
        half1.line_to(pt(center + side));
        half1.line_to(pt(center));
        half1.line_to(pt(center - side));
        half1.close_path();
        cx.list
            .push_path(half1, cx.color(TokenKey::ErrorColor, NORTH));
        let mut half2 = kurbo::BezPath::new();
        half2.move_to(pt(tail));
        half2.line_to(pt(center + side));
        half2.line_to(pt(center));
        half2.line_to(pt(center - side));
        half2.close_path();
        cx.list
            .push_path(half2, cx.color(TokenKey::TextColor, NEEDLE));
    }
}

impl std::fmt::Debug for Compass {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Compass")
            .field("heading", &self.heading)
            .finish()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use martensite_core::HotNode;

    #[test]
    fn normalizes_heading() {
        assert_eq!(Compass::new().heading(360.0).heading_value(), 0.0);
        assert_eq!(Compass::new().heading(-45.0).heading_value(), 315.0);
        assert_eq!(Compass::new().heading(720.0 + 30.0).heading_value(), 30.0);
    }

    #[test]
    fn cardinal_names() {
        assert_eq!(Compass::new().heading(0.0).cardinal(), "N");
        assert_eq!(Compass::new().heading(45.0).cardinal(), "NE");
        assert_eq!(Compass::new().heading(180.0).cardinal(), "S");
        assert_eq!(Compass::new().heading(337.0).cardinal(), "NW");
        assert_eq!(Compass::new().heading(350.0).cardinal(), "N");
    }

    #[test]
    fn lays_out() {
        let mut c = Compass::new();
        let mut hot = HotNode::default();
        let mut cx = LayoutContext {
            hot: &mut hot,
            scale: 1.0,
        };
        let sz = c.measure(
            &mut cx,
            LayoutConstraints {
                min_size: Vec2::ZERO,
                max_size: Vec2::new(100.0, 100.0),
            },
        );
        assert_eq!(sz, Vec2::new(64.0, 64.0));
    }
}
