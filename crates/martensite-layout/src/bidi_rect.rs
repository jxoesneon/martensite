//! BiDi-level-aware logical-to-physical coordinate mapping for RTL
//! inline flow and selection geometry.
//!
//! While [`crate::geometry::BidiRect`] handles writing-mode transposition
//! (horizontal vs vertical), this module handles the *inline direction*
//! flipping that BiDi embedding levels impose on horizontal text. A
//! Unicode BiDi embedding level is an integer where even levels are LTR
//! and odd levels are RTL. When mapping a logical selection rectangle
//! (expressed in reading order) to physical screen coordinates, an RTL
//! run must mirror its inline axis so that the start of the selection
//! appears on the right rather than the left.
//!
//! # Examples
//!
//! ```
//! use martensite_layout::bidi_rect::{BidiSelectionRect, logical_to_physical};
//!
//! // A 100px-wide selection starting at inline offset 50 in an LTR line
//! // of total width 400.
//! let logical = BidiSelectionRect::new(50.0, 0.0, 100.0, 20.0);
//! let physical = logical_to_physical(logical, 400.0, 0);
//! assert_eq!(physical.origin.x, 50.0);
//! assert_eq!(physical.size.width, 100.0);
//!
//! // The same logical selection in an RTL run (level 1) is mirrored:
//! // physical x = total_width - (inline_start + inline_size) = 400 - 150 = 250.
//! let physical_rtl = logical_to_physical(logical, 400.0, 1);
//! assert_eq!(physical_rtl.origin.x, 250.0);
//! assert_eq!(physical_rtl.size.width, 100.0);
//! ```

use crate::geometry::{Point, Size};

/// Returns `true` if the BiDi embedding `level` indicates an RTL run.
///
/// Per the Unicode Bidirectional Algorithm, even levels are LTR and odd
/// levels are RTL.
///
/// # Examples
///
/// ```
/// use martensite_layout::bidi_rect::is_rtl_level;
///
/// assert!(!is_rtl_level(0));
/// assert!(is_rtl_level(1));
/// assert!(!is_rtl_level(2));
/// assert!(is_rtl_level(3));
/// ```
#[inline]
pub const fn is_rtl_level(level: u8) -> bool {
    level % 2 == 1
}

/// A selection rectangle expressed in *logical* (reading-order)
/// inline/block coordinates.
///
/// `inline_start` is the offset from the leading edge of the line in
/// reading order. For LTR text this is the left edge; for RTL text the
/// leading edge is the right edge, so `inline_start` increases
/// right-to-left in logical space. The [`logical_to_physical`] function
/// converts this to physical screen coordinates using the BiDi level.
///
/// # Examples
///
/// ```
/// use martensite_layout::bidi_rect::BidiSelectionRect;
///
/// let rect = BidiSelectionRect::new(10.0, 5.0, 80.0, 20.0);
/// assert_eq!(rect.inline_start, 10.0);
/// assert_eq!(rect.block_start, 5.0);
/// assert_eq!(rect.inline_size, 80.0);
/// assert_eq!(rect.block_size, 20.0);
/// ```
#[derive(Copy, Clone, Debug, Default, PartialEq)]
pub struct BidiSelectionRect {
    /// Offset from the leading edge of the line in reading order
    /// (inline axis).
    pub inline_start: f32,
    /// Offset from the top of the line (block axis).
    pub block_start: f32,
    /// Extent along the inline axis.
    pub inline_size: f32,
    /// Extent along the block axis.
    pub block_size: f32,
}

impl BidiSelectionRect {
    /// Creates a new logical selection rectangle.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite_layout::bidi_rect::BidiSelectionRect;
    ///
    /// let rect = BidiSelectionRect::new(0.0, 0.0, 100.0, 20.0);
    /// assert_eq!(rect.inline_size, 100.0);
    /// ```
    #[inline]
    pub const fn new(
        inline_start: f32,
        block_start: f32,
        inline_size: f32,
        block_size: f32,
    ) -> Self {
        Self {
            inline_start,
            block_start,
            inline_size,
            block_size,
        }
    }

    /// Returns the inline end offset (`inline_start + inline_size`).
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite_layout::bidi_rect::BidiSelectionRect;
    ///
    /// let rect = BidiSelectionRect::new(10.0, 0.0, 80.0, 20.0);
    /// assert_eq!(rect.inline_end(), 90.0);
    /// ```
    #[inline]
    pub fn inline_end(&self) -> f32 {
        self.inline_start + self.inline_size
    }

    /// Returns the block end offset (`block_start + block_size`).
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite_layout::bidi_rect::BidiSelectionRect;
    ///
    /// let rect = BidiSelectionRect::new(0.0, 5.0, 100.0, 20.0);
    /// assert_eq!(rect.block_end(), 25.0);
    /// ```
    #[inline]
    pub fn block_end(&self) -> f32 {
        self.block_start + self.block_size
    }
}

/// Converts a logical [`BidiSelectionRect`] to a physical [`Point`]
/// and [`Size`] given the total line width and the BiDi embedding
/// `level` of the run.
///
/// For LTR runs (even `level`) the physical x equals `inline_start`.
/// For RTL runs (odd `level`) the inline axis is mirrored:
/// `physical_x = line_width - (inline_start + inline_size)`.
///
/// The block axis (y) is never flipped by BiDi level; it always maps
/// directly from `block_start`.
///
/// # Examples
///
/// ```
/// use martensite_layout::bidi_rect::{logical_to_physical, BidiSelectionRect};
///
/// let logical = BidiSelectionRect::new(0.0, 10.0, 50.0, 20.0);
/// let phys = logical_to_physical(logical, 200.0, 0);
/// assert_eq!(phys.origin.x, 0.0);
/// assert_eq!(phys.origin.y, 10.0);
/// assert_eq!(phys.size.width, 50.0);
/// assert_eq!(phys.size.height, 20.0);
///
/// let phys_rtl = logical_to_physical(logical, 200.0, 1);
/// assert_eq!(phys_rtl.origin.x, 150.0); // 200 - 50
/// ```
pub fn logical_to_physical(logical: BidiSelectionRect, line_width: f32, level: u8) -> PhysicalRect {
    let x = if is_rtl_level(level) {
        line_width - logical.inline_end()
    } else {
        logical.inline_start
    };
    PhysicalRect {
        origin: Point::new(x, logical.block_start),
        size: Size::new(logical.inline_size, logical.block_size),
    }
}

/// A physical (screen-space) rectangle resulting from a logical-to-
/// physical conversion.
///
/// # Examples
///
/// ```
/// use martensite_layout::bidi_rect::PhysicalRect;
/// use martensite_layout::geometry::{Point, Size};
///
/// let rect = PhysicalRect::new(Point::new(10.0, 20.0), Size::new(100.0, 30.0));
/// assert_eq!(rect.origin.x, 10.0);
/// assert_eq!(rect.size.width, 100.0);
/// ```
#[derive(Copy, Clone, Debug, Default, PartialEq)]
pub struct PhysicalRect {
    /// Top-left origin in physical screen coordinates.
    pub origin: Point,
    /// Width and height in physical screen coordinates.
    pub size: Size,
}

impl PhysicalRect {
    /// Creates a new physical rectangle from an origin and size.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite_layout::bidi_rect::PhysicalRect;
    /// use martensite_layout::geometry::{Point, Size};
    ///
    /// let rect = PhysicalRect::new(Point::new(0.0, 0.0), Size::new(50.0, 50.0));
    /// assert_eq!(rect.size.area(), 2500.0);
    /// ```
    #[inline]
    pub const fn new(origin: Point, size: Size) -> Self {
        Self { origin, size }
    }

    /// Returns the right edge (`origin.x + size.width`).
    #[inline]
    pub fn right(&self) -> f32 {
        self.origin.x + self.size.width
    }

    /// Returns the bottom edge (`origin.y + size.height`).
    #[inline]
    pub fn bottom(&self) -> f32 {
        self.origin.y + self.size.height
    }

    /// Returns `true` if the physical point lies inside this rectangle.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite_layout::bidi_rect::PhysicalRect;
    /// use martensite_layout::geometry::{Point, Size};
    ///
    /// let rect = PhysicalRect::new(Point::new(0.0, 0.0), Size::new(100.0, 100.0));
    /// assert!(rect.contains(Point::new(50.0, 50.0)));
    /// assert!(!rect.contains(Point::new(150.0, 50.0)));
    /// ```
    #[inline]
    pub fn contains(&self, point: Point) -> bool {
        point.x >= self.origin.x
            && point.x <= self.right()
            && point.y >= self.origin.y
            && point.y <= self.bottom()
    }
}

/// Converts a physical [`Point`] back to a logical inline offset given
/// the total line width and BiDi embedding `level`.
///
/// This is the inverse of the inline mapping performed by
/// [`logical_to_physical`]. For LTR runs the logical inline offset
/// equals `physical_x`. For RTL runs it equals
/// `line_width - physical_x`.
///
/// The block axis is never flipped.
///
/// # Examples
///
/// ```
/// use martensite_layout::bidi_rect::physical_to_logical_inline;
/// use martensite_layout::geometry::Point;
///
/// // LTR: logical == physical.
/// assert_eq!(physical_to_logical_inline(Point::new(50.0, 10.0), 400.0, 0), 50.0);
/// // RTL: logical = 400 - 50 = 350.
/// assert_eq!(physical_to_logical_inline(Point::new(50.0, 10.0), 400.0, 1), 350.0);
/// ```
pub fn physical_to_logical_inline(physical: Point, line_width: f32, level: u8) -> f32 {
    if is_rtl_level(level) {
        line_width - physical.x
    } else {
        physical.x
    }
}

/// Flips a logical inline offset to a physical x coordinate for the
/// given BiDi `level` and `line_width`.
///
/// For LTR runs (even level) this returns `inline_offset`. For RTL runs
/// (odd level) it returns `line_width - inline_offset`.
///
/// # Examples
///
/// ```
/// use martensite_layout::bidi_rect::flip_inline_offset;
///
/// assert_eq!(flip_inline_offset(100.0, 400.0, 0), 100.0);
/// assert_eq!(flip_inline_offset(100.0, 400.0, 1), 300.0);
/// ```
#[inline]
pub fn flip_inline_offset(inline_offset: f32, line_width: f32, level: u8) -> f32 {
    if is_rtl_level(level) {
        line_width - inline_offset
    } else {
        inline_offset
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn is_rtl_level_basic() {
        assert!(!is_rtl_level(0));
        assert!(is_rtl_level(1));
        assert!(!is_rtl_level(2));
        assert!(is_rtl_level(3));
        assert!(!is_rtl_level(4));
        assert!(is_rtl_level(5));
    }

    #[test]
    fn logical_to_physical_ltr() {
        let logical = BidiSelectionRect::new(50.0, 10.0, 100.0, 20.0);
        let phys = logical_to_physical(logical, 400.0, 0);
        assert_eq!(phys.origin, Point::new(50.0, 10.0));
        assert_eq!(phys.size, Size::new(100.0, 20.0));
    }

    #[test]
    fn logical_to_physical_rtl_mirrors_inline() {
        let logical = BidiSelectionRect::new(50.0, 10.0, 100.0, 20.0);
        let phys = logical_to_physical(logical, 400.0, 1);
        // physical x = 400 - (50 + 100) = 250
        assert_eq!(phys.origin, Point::new(250.0, 10.0));
        assert_eq!(phys.size, Size::new(100.0, 20.0));
    }

    #[test]
    fn logical_to_physical_rtl_full_width() {
        // A selection spanning the entire line in RTL should start at x=0.
        let logical = BidiSelectionRect::new(0.0, 0.0, 400.0, 20.0);
        let phys = logical_to_physical(logical, 400.0, 1);
        assert_eq!(phys.origin, Point::new(0.0, 0.0));
        assert_eq!(phys.size, Size::new(400.0, 20.0));
    }

    #[test]
    fn logical_to_physical_rtl_zero_offset() {
        // A zero-offset selection in RTL starts at the right edge.
        let logical = BidiSelectionRect::new(0.0, 0.0, 100.0, 20.0);
        let phys = logical_to_physical(logical, 400.0, 1);
        assert_eq!(phys.origin, Point::new(300.0, 0.0));
        assert_eq!(phys.size, Size::new(100.0, 20.0));
    }

    #[test]
    fn logical_to_physical_even_levels_are_ltr() {
        let logical = BidiSelectionRect::new(10.0, 5.0, 50.0, 15.0);
        // Levels 0, 2, 4 are all LTR.
        for level in [0u8, 2, 4] {
            let phys = logical_to_physical(logical, 200.0, level);
            assert_eq!(phys.origin, Point::new(10.0, 5.0));
        }
    }

    #[test]
    fn logical_to_physical_odd_levels_are_rtl() {
        let logical = BidiSelectionRect::new(10.0, 5.0, 50.0, 15.0);
        // Levels 1, 3, 5 are all RTL.
        for level in [1u8, 3, 5] {
            let phys = logical_to_physical(logical, 200.0, level);
            // 200 - (10 + 50) = 140
            assert_eq!(phys.origin, Point::new(140.0, 5.0));
        }
    }

    #[test]
    fn physical_to_logical_inline_ltr() {
        let p = Point::new(75.0, 10.0);
        assert_eq!(physical_to_logical_inline(p, 400.0, 0), 75.0);
    }

    #[test]
    fn physical_to_logical_inline_rtl() {
        let p = Point::new(75.0, 10.0);
        assert_eq!(physical_to_logical_inline(p, 400.0, 1), 325.0);
    }

    #[test]
    fn flip_inline_offset_ltr() {
        assert_eq!(flip_inline_offset(100.0, 400.0, 0), 100.0);
    }

    #[test]
    fn flip_inline_offset_rtl() {
        assert_eq!(flip_inline_offset(100.0, 400.0, 1), 300.0);
        assert_eq!(flip_inline_offset(0.0, 400.0, 1), 400.0);
        assert_eq!(flip_inline_offset(400.0, 400.0, 1), 0.0);
    }

    #[test]
    fn bidi_selection_rect_endpoints() {
        let rect = BidiSelectionRect::new(10.0, 5.0, 80.0, 20.0);
        assert_eq!(rect.inline_end(), 90.0);
        assert_eq!(rect.block_end(), 25.0);
    }

    #[test]
    fn physical_rect_contains() {
        let rect = PhysicalRect::new(Point::new(10.0, 20.0), Size::new(100.0, 50.0));
        assert!(rect.contains(Point::new(50.0, 40.0)));
        assert!(rect.contains(Point::new(10.0, 20.0))); // top-left corner
        assert!(rect.contains(Point::new(110.0, 70.0))); // bottom-right corner
        assert!(!rect.contains(Point::new(9.0, 40.0)));
        assert!(!rect.contains(Point::new(111.0, 40.0)));
    }

    #[test]
    fn physical_rect_edges() {
        let rect = PhysicalRect::new(Point::new(10.0, 20.0), Size::new(100.0, 50.0));
        assert_eq!(rect.right(), 110.0);
        assert_eq!(rect.bottom(), 70.0);
    }

    #[test]
    fn roundtrip_ltr() {
        let logical = BidiSelectionRect::new(75.0, 10.0, 50.0, 20.0);
        let phys = logical_to_physical(logical, 400.0, 0);
        let back = physical_to_logical_inline(phys.origin, 400.0, 0);
        assert_eq!(back, 75.0);
    }

    #[test]
    fn roundtrip_rtl() {
        let logical = BidiSelectionRect::new(75.0, 10.0, 50.0, 20.0);
        let phys = logical_to_physical(logical, 400.0, 1);
        // logical_to_physical uses inline_end (75 + 50 = 125) for RTL,
        // so physical x = 400 - 125 = 275. physical_to_logical_inline
        // mirrors back: 400 - 275 = 125, which is the logical inline_end,
        // not the inline_start. The roundtrip preserves inline_end.
        let back = physical_to_logical_inline(phys.origin, 400.0, 1);
        assert_eq!(back, logical.inline_end());
    }
}
