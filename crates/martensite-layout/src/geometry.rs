//! Geometry primitives for layout: `Point`, `Size`, `Constraints`, `EdgeInsets`.
//!
//! These types provide the foundational spatial vocabulary used by the
//! [`LayoutEngine`](crate::engine::LayoutEngine) and the Taffy bridge to
//! communicate measurement constraints and final positions between the
//! widget tree and the layout engine.

use glam::Vec2;

/// A 2D point in logical (layout) coordinate space.
#[derive(Copy, Clone, Debug, Default, PartialEq)]
pub struct Point {
    /// Horizontal coordinate.
    pub x: f32,
    /// Vertical coordinate.
    pub y: f32,
}

impl Point {
    /// Creates a new point from `(x, y)`.
    #[inline(always)]
    pub fn new(x: f32, y: f32) -> Self {
        Self { x, y }
    }

    /// Creates a point at the origin `(0, 0)`.
    #[inline(always)]
    pub fn zero() -> Self {
        Self { x: 0.0, y: 0.0 }
    }

    /// Converts to a `glam::Vec2`.
    #[inline(always)]
    pub fn to_vec2(self) -> Vec2 {
        Vec2::new(self.x, self.y)
    }

    /// Creates a point from a `glam::Vec2`.
    #[inline(always)]
    pub fn from_vec2(v: Vec2) -> Self {
        Self { x: v.x, y: v.y }
    }
}

/// A 2D size in logical (layout) coordinate space.
#[derive(Copy, Clone, Debug, Default, PartialEq)]
pub struct Size {
    /// Width in logical pixels.
    pub width: f32,
    /// Height in logical pixels.
    pub height: f32,
}

impl Size {
    /// Creates a new size from `(width, height)`.
    #[inline(always)]
    pub fn new(width: f32, height: f32) -> Self {
        Self { width, height }
    }

    /// Creates a zero-size `(0, 0)`.
    #[inline(always)]
    pub fn zero() -> Self {
        Self {
            width: 0.0,
            height: 0.0,
        }
    }

    /// Returns `true` if either dimension is zero or negative.
    #[inline]
    pub fn is_empty(&self) -> bool {
        self.width <= 0.0 || self.height <= 0.0
    }

    /// Returns the area (width * height).
    #[inline(always)]
    pub fn area(&self) -> f32 {
        self.width * self.height
    }
}

/// Layout constraints describing the minimum and maximum bounds within
/// which a widget must measure itself.
///
/// During the first pass of two-pass layout, the engine sends these
/// constraints to each widget's `measure` method. The widget returns a
/// `Size` that falls within `[min, max]`.
#[derive(Copy, Clone, Debug, PartialEq)]
pub struct Constraints {
    /// Minimum acceptable width.
    pub min_width: f32,
    /// Minimum acceptable height.
    pub min_height: f32,
    /// Maximum acceptable width.
    pub max_width: f32,
    /// Maximum acceptable height.
    pub max_height: f32,
}

impl Constraints {
    /// Creates constraints from individual min/max values.
    #[inline(always)]
    pub fn new(min_width: f32, min_height: f32, max_width: f32, max_height: f32) -> Self {
        Self {
            min_width,
            min_height,
            max_width,
            max_height,
        }
    }

    /// Creates tight constraints where `min == max` for both dimensions.
    #[inline(always)]
    pub fn tight(width: f32, height: f32) -> Self {
        Self {
            min_width: width,
            min_height: height,
            max_width: width,
            max_height: height,
        }
    }

    /// Creates loose constraints with a zero minimum and the given maximum.
    #[inline(always)]
    pub fn loose(max_width: f32, max_height: f32) -> Self {
        Self {
            min_width: 0.0,
            min_height: 0.0,
            max_width,
            max_height,
        }
    }

    /// Creates unbounded constraints (zero min, infinity max).
    #[inline(always)]
    pub fn unbounded() -> Self {
        Self {
            min_width: 0.0,
            min_height: 0.0,
            max_width: f32::INFINITY,
            max_height: f32::INFINITY,
        }
    }

    /// Constrains a given `Size` to fall within `[min, max]` on both axes.
    #[inline]
    pub fn constrain(&self, size: Size) -> Size {
        Size::new(
            size.width.clamp(self.min_width, self.max_width),
            size.height.clamp(self.min_height, self.max_height),
        )
    }
}

impl Default for Constraints {
    fn default() -> Self {
        Self::unbounded()
    }
}

/// Edge insets for padding, margin, or border regions.
///
/// All values are in logical pixels. `left` and `top` are measured from
/// the corresponding edge inward.
#[derive(Copy, Clone, Debug, Default, PartialEq)]
pub struct EdgeInsets {
    /// Inset from the left edge.
    pub left: f32,
    /// Inset from the right edge.
    pub right: f32,
    /// Inset from the top edge.
    pub top: f32,
    /// Inset from the bottom edge.
    pub bottom: f32,
}

impl EdgeInsets {
    /// Creates uniform insets on all sides.
    #[inline(always)]
    pub fn uniform(value: f32) -> Self {
        Self {
            left: value,
            right: value,
            top: value,
            bottom: value,
        }
    }

    /// Creates symmetric insets (horizontal, vertical).
    #[inline(always)]
    pub fn symmetric(horizontal: f32, vertical: f32) -> Self {
        Self {
            left: horizontal,
            right: horizontal,
            top: vertical,
            bottom: vertical,
        }
    }

    /// Creates insets from individual values.
    #[inline(always)]
    pub fn new(left: f32, top: f32, right: f32, bottom: f32) -> Self {
        Self {
            left,
            top,
            right,
            bottom,
        }
    }

    /// Returns the total horizontal inset (`left + right`).
    #[inline(always)]
    pub fn horizontal(&self) -> f32 {
        self.left + self.right
    }

    /// Returns the total vertical inset (`top + bottom`).
    #[inline(always)]
    pub fn vertical(&self) -> f32 {
        self.top + self.bottom
    }

    /// Returns `true` if all insets are zero.
    #[inline]
    pub fn is_zero(&self) -> bool {
        self.left == 0.0 && self.right == 0.0 && self.top == 0.0 && self.bottom == 0.0
    }

    /// Deflates a `Size` by the insets, returning the remaining content area.
    #[inline(always)]
    pub fn deflate(&self, size: Size) -> Size {
        Size::new(
            (size.width - self.horizontal()).max(0.0),
            (size.height - self.vertical()).max(0.0),
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn point_new_and_zero() {
        let p = Point::new(10.0, 20.0);
        assert_eq!(p.x, 10.0);
        assert_eq!(p.y, 20.0);
        assert_eq!(Point::zero(), Point::new(0.0, 0.0));
    }

    #[test]
    fn point_vec2_roundtrip() {
        let p = Point::new(5.0, 15.0);
        let v = p.to_vec2();
        assert_eq!(v, Vec2::new(5.0, 15.0));
        assert_eq!(Point::from_vec2(v), p);
    }

    #[test]
    fn size_new_and_zero() {
        let s = Size::new(100.0, 200.0);
        assert_eq!(s.width, 100.0);
        assert_eq!(s.height, 200.0);
        assert_eq!(Size::zero(), Size::new(0.0, 0.0));
    }

    #[test]
    fn size_is_empty() {
        assert!(Size::zero().is_empty());
        assert!(Size::new(-1.0, 10.0).is_empty());
        assert!(!Size::new(1.0, 1.0).is_empty());
    }

    #[test]
    fn size_area() {
        assert_eq!(Size::new(10.0, 20.0).area(), 200.0);
        assert_eq!(Size::zero().area(), 0.0);
    }

    #[test]
    fn constraints_tight() {
        let c = Constraints::tight(100.0, 200.0);
        assert_eq!(c.min_width, 100.0);
        assert_eq!(c.max_width, 100.0);
        assert_eq!(c.min_height, 200.0);
        assert_eq!(c.max_height, 200.0);
    }

    #[test]
    fn constraints_loose() {
        let c = Constraints::loose(500.0, 300.0);
        assert_eq!(c.min_width, 0.0);
        assert_eq!(c.max_width, 500.0);
    }

    #[test]
    fn constraints_unbounded() {
        let c = Constraints::unbounded();
        assert_eq!(c.min_width, 0.0);
        assert!(c.max_width.is_infinite());
        assert!(c.max_height.is_infinite());
    }

    #[test]
    fn constraints_constrain() {
        let c = Constraints::new(50.0, 50.0, 200.0, 200.0);
        let clamped = c.constrain(Size::new(10.0, 300.0));
        assert_eq!(clamped.width, 50.0);
        assert_eq!(clamped.height, 200.0);
        let ok = c.constrain(Size::new(100.0, 100.0));
        assert_eq!(ok.width, 100.0);
        assert_eq!(ok.height, 100.0);
    }

    #[test]
    fn edge_insets_uniform() {
        let e = EdgeInsets::uniform(10.0);
        assert_eq!(e.left, 10.0);
        assert_eq!(e.right, 10.0);
        assert_eq!(e.top, 10.0);
        assert_eq!(e.bottom, 10.0);
    }

    #[test]
    fn edge_insets_symmetric() {
        let e = EdgeInsets::symmetric(20.0, 10.0);
        assert_eq!(e.horizontal(), 40.0);
        assert_eq!(e.vertical(), 20.0);
    }

    #[test]
    fn edge_insets_is_zero() {
        assert!(EdgeInsets::default().is_zero());
        assert!(!EdgeInsets::uniform(1.0).is_zero());
    }

    #[test]
    fn edge_insets_deflate() {
        let e = EdgeInsets::uniform(10.0);
        let inner = e.deflate(Size::new(100.0, 100.0));
        assert_eq!(inner.width, 80.0);
        assert_eq!(inner.height, 80.0);
    }

    #[test]
    fn edge_insets_deflate_clamps_to_zero() {
        let e = EdgeInsets::uniform(100.0);
        let inner = e.deflate(Size::new(50.0, 50.0));
        assert_eq!(inner.width, 0.0);
        assert_eq!(inner.height, 0.0);
    }
}
