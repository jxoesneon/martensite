//! `SignalStrength` — the ascending-bars connectivity indicator
//! (cellular / Wi-Fi status icon idiom).
//!
//! A `0..=4` level lights that many ascending bars; `0` hollows
//! the whole column. An `offline` flag swaps the lit color to the
//! error token — the "no service" look.
//!
//! # Examples
//!
//! ```
//! use martensite::widgets::signal_strength::SignalStrength;
//!
//! let s = SignalStrength::new().level(3);
//! assert_eq!(s.level_value(), 3);
//! ```

use accesskit::Node as AccessKitNode;
use glam::Vec2;
use martensite_core::{
    LayoutConstraints, LayoutContext, PaintContext, Rect, RenderMinimum, UnderflowPolicy, Widget,
};
use martensite_theme::TokenKey;

const SIZE_PT: f32 = 18.0;
const BARS: usize = 4;

const LIT: [u8; 4] = [110, 180, 130, 255];
const OFF: [u8; 4] = [210, 110, 90, 255];
const DIM: [u8; 4] = [90, 90, 96, 255];

/// An ascending-bars connectivity indicator — see the module docs.
///
/// ```
/// use martensite::widgets::signal_strength::SignalStrength;
///
/// assert_eq!(SignalStrength::new().level_value(), 4);
/// ```
pub struct SignalStrength {
    /// Accessibility label.
    pub label: String,
    /// Error-colored (offline) face.
    pub offline: bool,
    level: u8,
    bounds: Rect,
}

impl Default for SignalStrength {
    fn default() -> Self {
        Self::new()
    }
}

impl SignalStrength {
    /// Creates a full-strength indicator.
    ///
    /// ```
    /// use martensite::widgets::signal_strength::SignalStrength;
    ///
    /// assert_eq!(SignalStrength::new().level_value(), 4);
    /// ```
    pub fn new() -> Self {
        Self {
            label: "Signal".to_string(),
            offline: false,
            level: BARS as u8,
            bounds: Rect::new(0.0, 0.0, 0.0, 0.0),
        }
    }

    /// Lit level `0..=4`.
    ///
    /// ```
    /// use martensite::widgets::signal_strength::SignalStrength;
    ///
    /// let s = SignalStrength::new().level(2);
    /// assert_eq!(s.level_value(), 2);
    /// ```
    pub fn level(mut self, level: u8) -> Self {
        self.level = level.min(BARS as u8);
        self
    }

    /// Accessibility label.
    ///
    /// ```
    /// use martensite::widgets::signal_strength::SignalStrength;
    ///
    /// let s = SignalStrength::new().label("Uplink");
    /// assert_eq!(s.label, "Uplink");
    /// ```
    pub fn label(mut self, label: impl Into<String>) -> Self {
        self.label = label.into();
        self
    }

    /// Offline (error-colored) face.
    ///
    /// ```
    /// use martensite::widgets::signal_strength::SignalStrength;
    ///
    /// let s = SignalStrength::new().offline(true);
    /// assert!(s.offline);
    /// ```
    pub fn offline(mut self, offline: bool) -> Self {
        self.offline = offline;
        self
    }

    /// Current level.
    ///
    /// ```
    /// use martensite::widgets::signal_strength::SignalStrength;
    ///
    /// assert_eq!(SignalStrength::new().level(1).level_value(), 1);
    /// ```
    pub fn level_value(&self) -> u8 {
        self.level
    }

    /// Sets the level programmatically.
    ///
    /// ```
    /// use martensite::widgets::signal_strength::SignalStrength;
    ///
    /// let mut s = SignalStrength::new();
    /// s.set_level(0);
    /// assert_eq!(s.level_value(), 0);
    /// ```
    pub fn set_level(&mut self, level: u8) {
        self.level = level.min(BARS as u8);
    }
}

impl Widget for SignalStrength {
    fn measure(&mut self, cx: &mut LayoutContext, constraints: LayoutConstraints) -> Vec2 {
        let s = cx.pt(SIZE_PT);
        Vec2::new(
            s.min(constraints.max_size.x.max(0.0)),
            s.min(constraints.max_size.y.max(0.0)),
        )
    }

    fn min_render(&self) -> RenderMinimum {
        RenderMinimum::new(Vec2::new(10.0, 10.0)).with_policy(UnderflowPolicy::Lint)
    }

    fn layout(&mut self, _cx: &mut LayoutContext, bounds: Rect) {
        self.bounds = bounds;
    }

    fn accessibility(&self, node: &mut AccessKitNode) {
        node.set_role(accesskit::Role::Image);
        node.set_label(format!("{} — {}/{}", self.label, self.level, BARS));
        if self.offline {
            node.set_description("offline");
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
        let lit = if self.offline {
            cx.color(TokenKey::ErrorColor, OFF)
        } else {
            cx.color(TokenKey::SuccessColor, LIT)
        };
        let dim = cx.color(TokenKey::BorderColor, DIM);
        let slot_w = self.bounds.width() / BARS as f32;
        let gap = cx.pt(1.5);
        let bar_w = (slot_w - gap).max(1.0);
        for i in 0..BARS {
            let frac = (i + 1) as f32 / BARS as f32;
            let h = self.bounds.height() * frac;
            let x = self.bounds.min_x() + i as f32 * slot_w + gap / 2.0;
            cx.list.push_fill_shape(
                f(Rect::new(x, self.bounds.max_y() - h, bar_w, h)),
                &martensite_core::shape::Shape::rounded(cx.pt(0.75)),
                if (i as u8) < self.level { lit } else { dim },
            );
        }
    }
}

impl std::fmt::Debug for SignalStrength {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("SignalStrength")
            .field("level", &self.level)
            .field("offline", &self.offline)
            .finish()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use martensite_core::HotNode;

    #[test]
    fn level_clamps() {
        assert_eq!(SignalStrength::new().level(9).level_value(), 4);
        assert_eq!(SignalStrength::new().level(0).level_value(), 0);
    }

    #[test]
    fn set_level() {
        let mut s = SignalStrength::new();
        s.set_level(1);
        assert_eq!(s.level_value(), 1);
        s.set_level(7);
        assert_eq!(s.level_value(), 4);
    }

    #[test]
    fn measure_square() {
        let mut s = SignalStrength::new();
        let mut hot = HotNode::default();
        let mut cx = LayoutContext {
            hot: &mut hot,
            scale: 1.0,
        };
        let size = s.measure(
            &mut cx,
            LayoutConstraints {
                min_size: Vec2::ZERO,
                max_size: Vec2::new(100.0, 100.0),
            },
        );
        assert_eq!(size.x, size.y);
    }
}
