//! `Separator` widget: a hairline divider between content regions.
//!
//! # Examples
//!
//! ```
//! use martensite::widgets::separator::Separator;
//!
//! let sep = Separator::horizontal();
//! assert_eq!(sep.direction, martensite::widgets::FlexDirection::Row);
//! ```

use accesskit::Node as AccessKitNode;
use glam::Vec2;
use martensite_core::widget::{LayoutConstraints, LayoutContext, PaintContext, Widget};
use martensite_core::{Rect, TokenKey};

use crate::widgets::flex::FlexDirection;

/// Ink fallback when the theme lacks a border token.
const HAIRLINE: [u8; 4] = [128, 128, 128, 140];

/// A visual separator line. `FlexDirection::Row` draws a horizontal
/// hairline across its bounds (for use inside a column); `Column`
/// draws a vertical hairline (inside a row).
///
/// # Examples
///
/// ```
/// use martensite::widgets::Separator;
///
/// let sep = Separator::vertical();
/// assert_eq!(sep.direction, martensite::widgets::FlexDirection::Column);
/// ```
#[derive(Clone, Debug)]
pub struct Separator {
    /// Line orientation: `Row` = horizontal line, `Column` = vertical.
    pub direction: FlexDirection,
    /// Cached bounds from the last layout pass.
    cached_bounds: Rect,
}

impl Separator {
    /// A horizontal separator line — stretches across its width.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite::widgets::Separator;
    ///
    /// let sep = Separator::horizontal();
    /// assert_eq!(sep.direction, martensite::widgets::FlexDirection::Row);
    /// ```
    pub fn horizontal() -> Self {
        Self {
            direction: FlexDirection::Row,
            cached_bounds: Rect::default(),
        }
    }

    /// A vertical separator line — stretches down its height.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite::widgets::Separator;
    ///
    /// let sep = Separator::vertical();
    /// assert_eq!(sep.direction, martensite::widgets::FlexDirection::Column);
    /// ```
    pub fn vertical() -> Self {
        Self {
            direction: FlexDirection::Column,
            cached_bounds: Rect::default(),
        }
    }
}

impl Widget for Separator {
    fn measure(&mut self, cx: &mut LayoutContext, constraints: LayoutConstraints) -> Vec2 {
        // A thin cross-axis cell that leaves a little breathing room
        // around the 1px line itself.
        match self.direction {
            // Report the offer only when it's bounded — `f32::MAX` is
            // the layout system's unbounded sentinel (it's finite, so
            // `is_finite` can't detect it). A row measuring children
            // for height must not get MAX echoed back as the
            // separator's "intrinsic" length.
            FlexDirection::Row => Vec2::new(
                if constraints.max_size.x < f32::MAX {
                    constraints.max_size.x.max(0.0)
                } else {
                    cx.pt(96.0)
                },
                cx.pt(9.0),
            ),
            FlexDirection::Column => Vec2::new(
                cx.pt(9.0),
                if constraints.max_size.y < f32::MAX {
                    constraints.max_size.y.max(0.0)
                } else {
                    cx.pt(28.0)
                },
            ),
        }
    }

    fn layout(&mut self, _cx: &mut LayoutContext, bounds: Rect) {
        self.cached_bounds = bounds;
    }

    fn accessibility(&self, node: &mut AccessKitNode) {
        node.set_role(accesskit::Role::GenericContainer);
    }

    fn paint(&self, cx: &mut PaintContext) {
        let b = cx.bounds;
        let color = cx.color(TokenKey::BorderColor, HAIRLINE);
        let line = match self.direction {
            FlexDirection::Row => {
                let y = f64::from(b.origin.y + b.size.y / 2.0);
                kurbo::Rect::new(f64::from(b.origin.x), y, f64::from(b.max_x()), y + 1.0)
            }
            FlexDirection::Column => {
                let x = f64::from(b.origin.x + b.size.x / 2.0);
                kurbo::Rect::new(x, f64::from(b.origin.y), x + 1.0, f64::from(b.max_y()))
            }
        };
        cx.list.push_fill_rect(line, color);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use martensite_core::HotNode;

    #[test]
    fn separator_measure() {
        let mut hot = HotNode::default();
        let mut cx = LayoutContext {
            hot: &mut hot,
            scale: 1.0,
        };
        let mut h = Separator::horizontal();
        let size = h.measure(
            &mut cx,
            LayoutConstraints {
                min_size: Vec2::ZERO,
                max_size: Vec2::new(200.0, 100.0),
            },
        );
        assert_eq!(size.x, 200.0);
        assert!(size.y > 0.0 && size.y < 20.0);
        let mut v = Separator::vertical();
        let size = v.measure(
            &mut cx,
            LayoutConstraints {
                min_size: Vec2::ZERO,
                max_size: Vec2::new(200.0, 100.0),
            },
        );
        assert_eq!(size.y, 100.0);
        assert!(size.x > 0.0 && size.x < 20.0);
    }
}
