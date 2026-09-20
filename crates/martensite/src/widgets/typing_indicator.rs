//! `TypingIndicator` — the bouncing three-dot "someone is
//! composing" affordance from chat UIs (iMessage / Slack /
//! Messenger idiom).
//!
//! [`TypingIndicator::tick`] advances a phase each frame; the
//! three dots rise and fall in a staggered wave. The indicator
//! is decorative — it carries no state and ignores events.
//! [`TypingIndicator::active`] gates the animation so hosts can
//! park it statically when nobody is typing.
//!
//! # Examples
//!
//! ```
//! use martensite::widgets::typing_indicator::TypingIndicator;
//! use martensite_core::Widget;
//! use std::time::Duration;
//!
//! let mut t = TypingIndicator::new();
//! assert!(t.is_active());
//! assert!(t.tick(Duration::from_millis(50))); // animating → repaint
//! ```

use accesskit::Node as AccessKitNode;
use glam::Vec2;
use martensite_core::{
    LayoutConstraints, LayoutContext, PaintContext, Rect, RenderMinimum, UnderflowPolicy, Widget,
};
use martensite_theme::TokenKey;
use std::f32::consts::TAU;
use std::time::Duration;

const W_PT: f32 = 44.0;
const H_PT: f32 = 20.0;
const DOT_PT: f32 = 3.0;
/// Lift amplitude in points.
const LIFT_PT: f32 = 3.0;
/// Full wave period in seconds.
const PERIOD: f32 = 1.2;

const DOT: [u8; 4] = [150, 150, 160, 255];

/// A bouncing-dots typing affordance — see the module docs.
///
/// ```
/// use martensite::widgets::typing_indicator::TypingIndicator;
///
/// assert!(TypingIndicator::new().is_active());
/// ```
#[derive(Debug)]
pub struct TypingIndicator {
    /// Accessibility label.
    pub label: String,
    active: bool,
    /// Wave phase in seconds.
    phase: f32,
    bounds: Rect,
    scale: f32,
}

impl Default for TypingIndicator {
    fn default() -> Self {
        Self::new()
    }
}

impl TypingIndicator {
    /// Creates an active indicator.
    ///
    /// ```
    /// use martensite::widgets::typing_indicator::TypingIndicator;
    ///
    /// assert!(TypingIndicator::new().is_active());
    /// ```
    pub fn new() -> Self {
        Self {
            label: "Typing".to_string(),
            active: true,
            phase: 0.0,
            bounds: Rect::new(0.0, 0.0, 0.0, 0.0),
            scale: 1.0,
        }
    }

    /// Whether the animation runs; inactive indicators park
    /// the dots at rest and stop repainting.
    ///
    /// ```
    /// use martensite::widgets::typing_indicator::TypingIndicator;
    ///
    /// assert!(!TypingIndicator::new().active(false).is_active());
    /// ```
    pub fn active(mut self, on: bool) -> Self {
        self.active = on;
        self
    }

    /// Accessibility label.
    ///
    /// ```
    /// use martensite::widgets::typing_indicator::TypingIndicator;
    ///
    /// assert_eq!(TypingIndicator::new().label("Ann typing").label, "Ann typing");
    /// ```
    pub fn label(mut self, label: impl Into<String>) -> Self {
        self.label = label.into();
        self
    }

    /// Whether the animation is active.
    ///
    /// ```
    /// use martensite::widgets::typing_indicator::TypingIndicator;
    ///
    /// assert!(TypingIndicator::new().is_active());
    /// ```
    pub fn is_active(&self) -> bool {
        self.active
    }

    /// Wave phase in seconds (diagnostics/tests).
    ///
    /// ```
    /// use martensite::widgets::typing_indicator::TypingIndicator;
    ///
    /// assert_eq!(TypingIndicator::new().phase(), 0.0);
    /// ```
    pub fn phase(&self) -> f32 {
        self.phase
    }

    /// Dot-lift fraction `0..=1` for dot `i` (`0`, `1`, `2`),
    /// staggered by a third of the wave each.
    fn lift(&self, i: usize) -> f32 {
        let t = (self.phase / PERIOD + i as f32 / 3.0).fract();
        // Half-sine bump: up for the first half of the cycle.
        (t * TAU).sin().max(0.0)
    }
}

impl Widget for TypingIndicator {
    fn measure(&mut self, cx: &mut LayoutContext, constraints: LayoutConstraints) -> Vec2 {
        Vec2::new(
            cx.pt(W_PT).min(constraints.max_size.x.max(0.0)),
            cx.pt(H_PT).min(constraints.max_size.y.max(0.0)),
        )
    }

    fn min_render(&self) -> RenderMinimum {
        RenderMinimum::new(Vec2::new(24.0, 12.0)).with_policy(UnderflowPolicy::Lint)
    }

    fn layout(&mut self, cx: &mut LayoutContext, bounds: Rect) {
        self.bounds = bounds;
        self.scale = cx.scale;
    }

    fn accessibility(&self, node: &mut AccessKitNode) {
        node.set_role(accesskit::Role::Image);
        node.set_label(if self.active {
            format!("{}…", self.label)
        } else {
            self.label.clone()
        });
    }

    fn tick(&mut self, dt: Duration) -> bool {
        if !self.active {
            return false;
        }
        self.phase = (self.phase + dt.as_secs_f32()) % PERIOD;
        true
    }

    fn paint(&self, cx: &mut PaintContext) {
        let r = DOT_PT * self.scale;
        let lift = LIFT_PT * self.scale;
        let cy = (self.bounds.min_y() + self.bounds.max_y()) / 2.0 + lift * 0.5;
        let cxm = (self.bounds.min_x() + self.bounds.max_x()) / 2.0;
        let color = cx.color(TokenKey::TextMutedColor, DOT);
        for i in 0..3 {
            let dx = (i as f32 - 1.0) * r * 3.0;
            let dy = if self.active {
                -self.lift(i) * lift
            } else {
                0.0
            };
            let p = Vec2::new(cxm + dx, cy + dy);
            cx.list.push_fill_shape(
                kurbo::Rect::new(
                    f64::from(p.x - r),
                    f64::from(p.y - r),
                    f64::from(p.x + r),
                    f64::from(p.y + r),
                ),
                &martensite_core::shape::Shape::circle(p, r),
                color,
            );
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tick_advances_phase() {
        let mut t = TypingIndicator::new();
        assert!(t.tick(Duration::from_millis(100)));
        assert!(t.phase() > 0.0);
    }

    #[test]
    fn inactive_does_not_tick() {
        let mut t = TypingIndicator::new().active(false);
        assert!(!t.tick(Duration::from_secs(1)));
        assert_eq!(t.phase(), 0.0);
    }

    #[test]
    fn phase_wraps() {
        let mut t = TypingIndicator::new();
        t.tick(Duration::from_millis(1300)); // past one 1.2 s period
        assert!(t.phase() < 0.2);
    }

    #[test]
    fn lifts_stagger() {
        let mut t = TypingIndicator::new();
        // Mid-wave: dot 0 near peak, dot 2 still resting.
        t.tick(Duration::from_millis(300)); // phase = 0.25 period
        let l0 = t.lift(0);
        let l2 = t.lift(2);
        assert!(l0 > l2, "l0 {l0} should lead l2 {l2}");
    }
}
