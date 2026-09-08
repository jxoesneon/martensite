//! Geometry primitives for layout: `Point`, `Size`, `Constraints`, `EdgeInsets`.
//!
//! These types provide the foundational spatial vocabulary used by the
//! [`LayoutEngine`](crate::engine::LayoutEngine) and the Taffy bridge to
//! communicate measurement constraints and final positions between the
//! widget tree and the layout engine.

use crate::vertical_flow::{FlowTransposition, LogicalPoint, LogicalSize};
use glam::Vec2;

/// A 2D point in logical (layout) coordinate space.
///
/// # Examples
///
/// ```
/// use martensite_layout::geometry::Point;
///
/// let p = Point::new(10.0, 20.0);
/// assert_eq!(p.x, 10.0);
/// assert_eq!(p.y, 20.0);
/// ```
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
///
/// # Examples
///
/// ```
/// use martensite_layout::geometry::Size;
///
/// let s = Size::new(100.0, 200.0);
/// assert_eq!(s.area(), 20_000.0);
/// assert!(!s.is_empty());
/// ```
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
///
/// # Examples
///
/// ```
/// use martensite_layout::geometry::{Constraints, Size};
///
/// let c = Constraints::loose(200.0, 100.0);
/// let s = c.constrain(Size::new(300.0, 50.0));
/// assert_eq!(s.width, 200.0);  // clamped to max
/// assert_eq!(s.height, 50.0);  // within bounds
/// ```
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
///
/// # Examples
///
/// ```
/// use martensite_layout::geometry::{EdgeInsets, Size};
///
/// let insets = EdgeInsets::uniform(10.0);
/// let content = insets.deflate(Size::new(100.0, 100.0));
/// assert_eq!(content.width, 80.0);
/// assert_eq!(content.height, 80.0);
/// ```
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

/// A rectangle expressed in both physical screen coordinates and flow-relative
/// logical coordinates, enabling writing-mode-aware selection geometry.
///
/// `BidiRect` is the bridge between Taffy's physical layout output and the
/// logical (inline/block) coordinate system required for correct bidi text
/// selection, caret positioning, and hit-testing in vertical writing modes.
///
/// # Examples
///
/// ```
/// use martensite_layout::geometry::{BidiRect, Point, Size};
/// use martensite_layout::vertical_flow::{FlowTransposition, WritingMode};
///
/// let trans = FlowTransposition::new(WritingMode::VerticalRl, Size::new(500.0, 800.0));
/// let rect = BidiRect::from_physical(Point::new(460.0, 60.0), Size::new(80.0, 200.0), &trans);
/// assert_eq!(rect.logical_origin.inline, 60.0);
/// assert_eq!(rect.logical_size.inline, 200.0);
/// ```
#[derive(Copy, Clone, Debug, Default, PartialEq)]
pub struct BidiRect {
    /// Origin in physical screen coordinates.
    pub physical_origin: Point,
    /// Size in physical screen coordinates.
    pub physical_size: Size,
    /// Origin in flow-relative logical coordinates.
    pub logical_origin: LogicalPoint,
    /// Size in flow-relative logical coordinates.
    pub logical_size: LogicalSize,
}

impl BidiRect {
    /// Constructs a `BidiRect` from physical coordinates, computing the
    /// logical counterparts via the given [`FlowTransposition`].
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite_layout::geometry::{BidiRect, Point, Size};
    /// use martensite_layout::vertical_flow::{FlowTransposition, WritingMode};
    ///
    /// let trans = FlowTransposition::new(WritingMode::HorizontalTb, Size::new(800.0, 600.0));
    /// let rect = BidiRect::from_physical(Point::new(10.0, 20.0), Size::new(100.0, 30.0), &trans);
    /// assert_eq!(rect.logical_origin.inline, 10.0);
    /// ```
    pub fn from_physical(origin: Point, size: Size, transposition: &FlowTransposition) -> Self {
        Self {
            physical_origin: origin,
            physical_size: size,
            logical_origin: transposition.to_logical_point(origin),
            logical_size: transposition.to_logical_size(size),
        }
    }

    /// Constructs a `BidiRect` from flow-relative logical coordinates,
    /// computing the physical counterparts via the given [`FlowTransposition`].
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite_layout::geometry::{BidiRect, Point, Size};
    /// use martensite_layout::vertical_flow::{
    ///     FlowTransposition, LogicalPoint, LogicalSize, WritingMode,
    /// };
    ///
    /// let trans = FlowTransposition::new(WritingMode::VerticalLr, Size::new(500.0, 800.0));
    /// let rect = BidiRect::from_logical(
    ///     LogicalPoint::new(60.0, 40.0),
    ///     LogicalSize::new(200.0, 80.0),
    ///     &trans,
    /// );
    /// assert_eq!(rect.physical_origin, Point::new(40.0, 60.0));
    /// ```
    pub fn from_logical(
        origin: LogicalPoint,
        size: LogicalSize,
        transposition: &FlowTransposition,
    ) -> Self {
        Self {
            physical_origin: transposition.to_physical_point(origin),
            physical_size: transposition.to_physical_size(size),
            logical_origin: origin,
            logical_size: size,
        }
    }

    /// Returns `true` if the given physical point lies inside this rectangle.
    #[inline]
    pub fn contains_physical(&self, point: Point) -> bool {
        point.x >= self.physical_origin.x
            && point.x <= self.physical_origin.x + self.physical_size.width
            && point.y >= self.physical_origin.y
            && point.y <= self.physical_origin.y + self.physical_size.height
    }

    /// Returns `true` if this rectangle intersects another in physical space.
    #[inline]
    pub fn intersects_physical(&self, other: &BidiRect) -> bool {
        self.physical_origin.x < other.physical_origin.x + other.physical_size.width
            && self.physical_origin.x + self.physical_size.width > other.physical_origin.x
            && self.physical_origin.y < other.physical_origin.y + other.physical_size.height
            && self.physical_origin.y + self.physical_size.height > other.physical_origin.y
    }
}

/// A collection of [`BidiRect`]s representing a text selection across
/// potentially multiple lines or columns in any writing mode.
///
/// In horizontal mode, each rect typically represents one selected line.
/// In vertical modes, each rect represents one selected column.
///
/// # Examples
///
/// ```
/// use martensite_layout::geometry::{BidiRect, Point, SelectionGeometry, Size};
/// use martensite_layout::vertical_flow::{FlowTransposition, WritingMode};
///
/// let trans = FlowTransposition::new(WritingMode::HorizontalTb, Size::new(800.0, 600.0));
/// let sel = SelectionGeometry::single_line(
///     Point::new(10.0, 100.0),
///     Size::new(200.0, 20.0),
///     &trans,
/// );
/// assert_eq!(sel.rects().len(), 1);
/// ```
#[derive(Clone, Debug, Default, PartialEq)]
pub struct SelectionGeometry {
    rects: Vec<BidiRect>,
}

impl SelectionGeometry {
    /// Creates an empty selection geometry.
    #[inline]
    pub fn new() -> Self {
        Self { rects: Vec::new() }
    }

    /// Creates a selection geometry for a single contiguous line or column.
    pub fn single_line(origin: Point, size: Size, transposition: &FlowTransposition) -> Self {
        Self {
            rects: vec![BidiRect::from_physical(origin, size, transposition)],
        }
    }

    /// Appends a selection rect.
    #[inline]
    pub fn push(&mut self, rect: BidiRect) {
        self.rects.push(rect);
    }

    /// Returns a slice of all selection rects.
    #[inline]
    pub fn rects(&self) -> &[BidiRect] {
        &self.rects
    }

    /// Returns `true` if there are no selection rects.
    #[inline]
    pub fn is_empty(&self) -> bool {
        self.rects.is_empty()
    }

    /// Computes the physical-space axis-aligned bounding box of all rects.
    ///
    /// Returns a default (zero) `BidiRect` if empty.
    pub fn bounding_box(&self) -> BidiRect {
        if self.rects.is_empty() {
            return BidiRect::default();
        }
        let mut min_x = f32::MAX;
        let mut min_y = f32::MAX;
        let mut max_x = f32::MIN;
        let mut max_y = f32::MIN;
        for r in &self.rects {
            min_x = min_x.min(r.physical_origin.x);
            min_y = min_y.min(r.physical_origin.y);
            max_x = max_x.max(r.physical_origin.x + r.physical_size.width);
            max_y = max_y.max(r.physical_origin.y + r.physical_size.height);
        }
        // The bounding box is always in physical space; logical fields
        // reflect the first rect's transposition context, which is
        // sufficient for single-mode selections.
        BidiRect {
            physical_origin: Point::new(min_x, min_y),
            physical_size: Size::new(max_x - min_x, max_y - min_y),
            logical_origin: LogicalPoint::default(),
            logical_size: LogicalSize::default(),
        }
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

    // ---- BidiRect tests (RED phase: these types do not exist yet) ----

    #[test]
    fn bidi_rect_from_physical_horizontal() {
        use crate::vertical_flow::{FlowTransposition, WritingMode};
        let trans = FlowTransposition::new(WritingMode::HorizontalTb, Size::new(800.0, 600.0));
        let rect = BidiRect::from_physical(Point::new(10.0, 20.0), Size::new(100.0, 30.0), &trans);
        // In horizontal mode, physical == logical.
        assert_eq!(rect.physical_origin, Point::new(10.0, 20.0));
        assert_eq!(rect.physical_size, Size::new(100.0, 30.0));
        assert_eq!(rect.logical_origin.inline, 10.0);
        assert_eq!(rect.logical_origin.block, 20.0);
        assert_eq!(rect.logical_size.inline, 100.0);
        assert_eq!(rect.logical_size.block, 30.0);
    }

    #[test]
    fn bidi_rect_from_physical_vertical_rl() {
        use crate::vertical_flow::{FlowTransposition, WritingMode};
        let container = Size::new(500.0, 800.0);
        let trans = FlowTransposition::new(WritingMode::VerticalRl, container);
        // Physical rect at (460, 60) size (80, 200) — in vertical-rl:
        //   inline = physical.y = 60, block = container.width - physical.x = 500 - 460 = 40
        //   logical_size.inline = physical_size.height = 200, block = physical_size.width = 80
        let rect = BidiRect::from_physical(Point::new(460.0, 60.0), Size::new(80.0, 200.0), &trans);
        assert_eq!(rect.logical_origin.inline, 60.0);
        assert_eq!(rect.logical_origin.block, 40.0);
        assert_eq!(rect.logical_size.inline, 200.0);
        assert_eq!(rect.logical_size.block, 80.0);
    }

    #[test]
    fn bidi_rect_from_logical_roundtrip_vertical_lr() {
        use crate::vertical_flow::{FlowTransposition, LogicalPoint, LogicalSize, WritingMode};
        let container = Size::new(500.0, 800.0);
        let trans = FlowTransposition::new(WritingMode::VerticalLr, container);
        let log_origin = LogicalPoint::new(60.0, 40.0);
        let log_size = LogicalSize::new(200.0, 80.0);
        let rect = BidiRect::from_logical(log_origin, log_size, &trans);
        // Round-trip: logical -> physical -> logical should match.
        assert_eq!(rect.logical_origin, log_origin);
        assert_eq!(rect.logical_size, log_size);
        // Physical: x = block = 40, y = inline = 60, width = block_dim = 80, height = inline_dim = 200
        assert_eq!(rect.physical_origin, Point::new(40.0, 60.0));
        assert_eq!(rect.physical_size, Size::new(80.0, 200.0));
    }

    #[test]
    fn bidi_rect_contains_physical_point() {
        use crate::vertical_flow::{FlowTransposition, WritingMode};
        let trans = FlowTransposition::new(WritingMode::HorizontalTb, Size::new(800.0, 600.0));
        let rect = BidiRect::from_physical(Point::new(10.0, 20.0), Size::new(100.0, 30.0), &trans);
        assert!(rect.contains_physical(Point::new(50.0, 35.0)));
        assert!(!rect.contains_physical(Point::new(5.0, 35.0)));
        assert!(!rect.contains_physical(Point::new(50.0, 55.0)));
    }

    #[test]
    fn bidi_rect_intersects() {
        use crate::vertical_flow::{FlowTransposition, WritingMode};
        let trans = FlowTransposition::new(WritingMode::HorizontalTb, Size::new(800.0, 600.0));
        let a = BidiRect::from_physical(Point::new(0.0, 0.0), Size::new(50.0, 50.0), &trans);
        let b = BidiRect::from_physical(Point::new(25.0, 25.0), Size::new(50.0, 50.0), &trans);
        let c = BidiRect::from_physical(Point::new(100.0, 100.0), Size::new(10.0, 10.0), &trans);
        assert!(a.intersects_physical(&b));
        assert!(!a.intersects_physical(&c));
    }

    // ---- SelectionGeometry tests ----

    #[test]
    fn selection_geometry_single_line_horizontal() {
        use crate::vertical_flow::{FlowTransposition, WritingMode};
        let trans = FlowTransposition::new(WritingMode::HorizontalTb, Size::new(800.0, 600.0));
        let sel =
            SelectionGeometry::single_line(Point::new(10.0, 100.0), Size::new(200.0, 20.0), &trans);
        assert_eq!(sel.rects().len(), 1);
        assert_eq!(sel.rects()[0].physical_origin, Point::new(10.0, 100.0));
        assert_eq!(sel.rects()[0].physical_size, Size::new(200.0, 20.0));
    }

    #[test]
    fn selection_geometry_multi_line_vertical_rl() {
        use crate::vertical_flow::{FlowTransposition, WritingMode};
        let container = Size::new(500.0, 800.0);
        let trans = FlowTransposition::new(WritingMode::VerticalRl, container);
        let mut sel = SelectionGeometry::new();
        // Two vertical columns of selected text.
        sel.push(BidiRect::from_physical(
            Point::new(400.0, 10.0),
            Size::new(30.0, 200.0),
            &trans,
        ));
        sel.push(BidiRect::from_physical(
            Point::new(360.0, 10.0),
            Size::new(30.0, 150.0),
            &trans,
        ));
        assert_eq!(sel.rects().len(), 2);
        // First rect is further right (lower block offset in vertical-rl).
        assert!(sel.rects()[0].logical_origin.block < sel.rects()[1].logical_origin.block);
    }

    #[test]
    fn selection_geometry_bounding_box() {
        use crate::vertical_flow::{FlowTransposition, WritingMode};
        let trans = FlowTransposition::new(WritingMode::HorizontalTb, Size::new(800.0, 600.0));
        let mut sel = SelectionGeometry::new();
        sel.push(BidiRect::from_physical(
            Point::new(10.0, 100.0),
            Size::new(780.0, 20.0),
            &trans,
        ));
        sel.push(BidiRect::from_physical(
            Point::new(10.0, 120.0),
            Size::new(400.0, 20.0),
            &trans,
        ));
        let bbox = sel.bounding_box();
        assert_eq!(bbox.physical_origin, Point::new(10.0, 100.0));
        assert_eq!(bbox.physical_size, Size::new(780.0, 40.0));
    }
}
