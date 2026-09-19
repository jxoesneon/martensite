//! `Battery` — a charge-level indicator (the status-bar battery
//! glyph idiom, companion to [`crate::widgets::signal_strength::SignalStrength`]).
//!
//! A rounded battery body with a terminal nub and a fill whose
//! width tracks `level` (`0..=1`). The fill shifts green → amber →
//! red as the level falls, and `charging(true)` overlays a bolt
//! chevron. Display-only.
//!
//! # Examples
//!
//! ```
//! use martensite::widgets::battery::Battery;
//!
//! let b = Battery::new().level(0.75);
//! assert_eq!(b.level_value(), 0.75);
//! ```

use accesskit::Node as AccessKitNode;
use glam::Vec2;
use martensite_core::{
    EventContext, EventResponse, LayoutConstraints, LayoutContext, PaintContext, Rect,
    RenderMinimum, UnderflowPolicy, Widget,
};
use martensite_theme::TokenKey;

const WIDTH_PT: f32 = 28.0;
const HEIGHT_PT: f32 = 14.0;
const NUB_PT: f32 = 3.0;
const INSET_PT: f32 = 2.0;

const EDGE: [u8; 4] = [150, 150, 158, 255];
const GOOD: [u8; 4] = [90, 200, 120, 255];
const MID: [u8; 4] = [220, 180, 80, 255];
const LOW: [u8; 4] = [220, 90, 80, 255];
const BOLT: [u8; 4] = [245, 245, 250, 255];

/// A battery charge-level indicator — see the module docs.
///
/// ```
/// use martensite::widgets::battery::Battery;
///
/// assert_eq!(Battery::new().level_value(), 1.0);
/// ```
#[derive(Debug)]
pub struct Battery {
    /// Charge fraction `0..=1`.
    pub level: f32,
    /// Whether a charge bolt is drawn.
    pub charging: bool,
    /// Accessibility label.
    pub label: String,
    bounds: Rect,
    scale: f32,
}

impl Default for Battery {
    fn default() -> Self {
        Self::new()
    }
}

impl Battery {
    /// Creates a full, non-charging battery.
    ///
    /// ```
    /// use martensite::widgets::battery::Battery;
    ///
    /// assert_eq!(Battery::new().level_value(), 1.0);
    /// ```
    pub fn new() -> Self {
        Self {
            level: 1.0,
            charging: false,
            label: "Battery".to_string(),
            bounds: Rect::new(0.0, 0.0, 0.0, 0.0),
            scale: 1.0,
        }
    }

    /// Charge fraction (clamped to `0..=1`).
    ///
    /// ```
    /// use martensite::widgets::battery::Battery;
    ///
    /// assert_eq!(Battery::new().level(1.4).level_value(), 1.0);
    /// ```
    pub fn level(mut self, level: f32) -> Self {
        self.level = level.clamp(0.0, 1.0);
        self
    }

    /// Draws the charging bolt.
    ///
    /// ```
    /// use martensite::widgets::battery::Battery;
    ///
    /// assert!(Battery::new().charging(true).charging);
    /// ```
    pub fn charging(mut self, charging: bool) -> Self {
        self.charging = charging;
        self
    }

    /// Accessibility label.
    ///
    /// ```
    /// use martensite::widgets::battery::Battery;
    ///
    /// assert_eq!(Battery::new().label("Pack A").label, "Pack A");
    /// ```
    pub fn label(mut self, label: impl Into<String>) -> Self {
        self.label = label.into();
        self
    }

    /// Charge fraction.
    ///
    /// ```
    /// use martensite::widgets::battery::Battery;
    ///
    /// assert_eq!(Battery::new().level(0.2).level_value(), 0.2);
    /// ```
    pub fn level_value(&self) -> f32 {
        self.level
    }

    /// Whether the bolt is drawn.
    ///
    /// ```
    /// use martensite::widgets::battery::Battery;
    ///
    /// assert!(!Battery::new().is_charging());
    /// ```
    pub fn is_charging(&self) -> bool {
        self.charging
    }

    /// Fill color for the current level (red < 0.2, amber < 0.5).
    fn fill_color(&self, cx: &mut PaintContext) -> [u8; 4] {
        if self.level < 0.2 {
            cx.color(TokenKey::ErrorColor, LOW)
        } else if self.level < 0.5 {
            cx.color(TokenKey::WarningColor, MID)
        } else {
            cx.color(TokenKey::SuccessColor, GOOD)
        }
    }
}

impl Widget for Battery {
    fn measure(&mut self, cx: &mut LayoutContext, constraints: LayoutConstraints) -> Vec2 {
        Vec2::new(
            (cx.pt(WIDTH_PT) + cx.pt(NUB_PT)).min(constraints.max_size.x.max(0.0)),
            cx.pt(HEIGHT_PT).min(constraints.max_size.y.max(0.0)),
        )
    }

    fn min_render(&self) -> RenderMinimum {
        RenderMinimum::new(Vec2::new(14.0, 8.0)).with_policy(UnderflowPolicy::Lint)
    }

    fn layout(&mut self, cx: &mut LayoutContext, bounds: Rect) {
        self.bounds = bounds;
        self.scale = cx.scale;
    }

    fn accessibility(&self, node: &mut AccessKitNode) {
        node.set_role(accesskit::Role::Meter);
        node.set_label(format!(
            "{} — {:.0}%{}",
            self.label,
            self.level * 100.0,
            if self.charging { ", charging" } else { "" }
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
        let nub = NUB_PT * self.scale;
        let body = Rect::new(
            self.bounds.min_x(),
            self.bounds.min_y(),
            self.bounds.width() - nub,
            self.bounds.height(),
        );
        // Body outline.
        cx.list.push_stroke_shape(
            f(body),
            &martensite_core::shape::Shape::rounded(cx.pt(3.0)),
            cx.pt(1.0),
            cx.color(TokenKey::TextColor, EDGE),
        );
        // Terminal nub.
        let nh = body.height() * 0.4;
        cx.list.push_fill_rect(
            f(Rect::new(
                body.max_x(),
                body.min_y() + (body.height() - nh) / 2.0,
                nub,
                nh,
            )),
            cx.color(TokenKey::TextColor, EDGE),
        );
        // Fill.
        let inset = INSET_PT * self.scale;
        let inner = Rect::new(
            body.min_x() + inset,
            body.min_y() + inset,
            (body.width() - 2.0 * inset).max(0.0),
            (body.height() - 2.0 * inset).max(0.0),
        );
        if self.level > 0.0 {
            let color = if self.charging {
                cx.color(TokenKey::SuccessColor, GOOD)
            } else {
                self.fill_color(cx)
            };
            let fill = Rect::new(
                inner.min_x(),
                inner.min_y(),
                inner.width() * self.level,
                inner.height(),
            );
            cx.list.push_fill_rect(f(fill), color);
        }
        // Charging bolt — a zigzag across the fill.
        if self.charging {
            let mid_x = inner.min_x() + inner.width() / 2.0;
            let h = inner.height();
            let mut bolt = kurbo::BezPath::new();
            bolt.move_to((
                f64::from(mid_x + inner.width() * 0.12),
                f64::from(inner.min_y()),
            ));
            bolt.line_to((
                f64::from(mid_x - inner.width() * 0.08),
                f64::from(inner.min_y() + h * 0.55),
            ));
            bolt.line_to((
                f64::from(mid_x + inner.width() * 0.02),
                f64::from(inner.min_y() + h * 0.55),
            ));
            bolt.line_to((
                f64::from(mid_x - inner.width() * 0.12),
                f64::from(inner.max_y()),
            ));
            cx.list
                .push_stroke_path(bolt, cx.pt(1.5), cx.color(TokenKey::TextColor, BOLT));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use martensite_core::HotNode;

    #[test]
    fn clamps_level() {
        assert_eq!(Battery::new().level(1.5).level_value(), 1.0);
        assert_eq!(Battery::new().level(-0.5).level_value(), 0.0);
    }

    #[test]
    fn charging_flag() {
        let b = Battery::new().charging(true);
        assert!(b.is_charging());
        assert!(!Battery::new().is_charging());
    }

    #[test]
    fn lays_out() {
        let mut b = Battery::new();
        let mut hot = HotNode::default();
        let mut cx = LayoutContext {
            hot: &mut hot,
            scale: 1.0,
        };
        let sz = b.measure(
            &mut cx,
            LayoutConstraints {
                min_size: Vec2::ZERO,
                max_size: Vec2::new(100.0, 50.0),
            },
        );
        assert_eq!(sz, Vec2::new(31.0, 14.0));
    }
}
