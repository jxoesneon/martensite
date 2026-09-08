//! Vertical layout axis transposition bridge for `writing-mode: vertical-rl` and `vertical-lr`.
//!
//! This module provides:
//! - Standard CSS Writing Modes ([`WritingMode`]).
//! - Flow-relative coordinates and dimensions ([`LogicalPoint`], [`LogicalSize`]).
//! - Axis transposition and coordinate mapping bridge ([`FlowTransposition`]).

use crate::geometry::{Constraints, Point, Size};

/// CSS writing mode specifying block and inline progression axes.
///
/// # Examples
///
/// ```
/// use martensite_layout::vertical_flow::WritingMode;
///
/// let mode = WritingMode::HorizontalTb;
/// assert_eq!(mode, WritingMode::default());
/// ```
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum WritingMode {
    /// Horizontal flow, lines progress from top to bottom (default western typography).
    #[default]
    HorizontalTb,
    /// Vertical flow, lines progress from right to left (traditional East Asian typography).
    VerticalRl,
    /// Vertical flow, lines progress from left to right (Mongolian, vertical western scripts).
    VerticalLr,
}

/// A 2D point expressed in flow-relative logical coordinates (`inline`, `block`).
///
/// - In horizontal modes: `inline` corresponds to X (left-to-right) and `block` to Y (top-to-bottom).
/// - In vertical modes: `inline` corresponds to Y (top-to-bottom) and `block` to X (line column progression).
///
/// # Examples
///
/// ```
/// use martensite_layout::vertical_flow::LogicalPoint;
///
/// let pt = LogicalPoint::new(50.0, 100.0);
/// assert_eq!(pt.inline, 50.0);
/// assert_eq!(pt.block, 100.0);
/// ```
#[derive(Copy, Clone, Debug, Default, PartialEq)]
pub struct LogicalPoint {
    /// Coordinate along the inline reading direction.
    pub inline: f32,
    /// Coordinate along the block column progression direction.
    pub block: f32,
}

impl LogicalPoint {
    /// Creates a new `LogicalPoint` with given inline and block values.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite_layout::vertical_flow::LogicalPoint;
    ///
    /// let pt = LogicalPoint::new(10.0, 20.0);
    /// assert_eq!(pt.inline, 10.0);
    /// assert_eq!(pt.block, 20.0);
    /// ```
    #[inline(always)]
    pub const fn new(inline: f32, block: f32) -> Self {
        Self { inline, block }
    }

    /// Creates a `LogicalPoint` at the logical origin `(0.0, 0.0)`.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite_layout::vertical_flow::LogicalPoint;
    ///
    /// let origin = LogicalPoint::zero();
    /// assert_eq!(origin.inline, 0.0);
    /// assert_eq!(origin.block, 0.0);
    /// ```
    #[inline(always)]
    pub const fn zero() -> Self {
        Self {
            inline: 0.0,
            block: 0.0,
        }
    }
}

/// A 2D size expressed in flow-relative logical dimensions (`inline`, `block`).
///
/// # Examples
///
/// ```
/// use martensite_layout::vertical_flow::LogicalSize;
///
/// let size = LogicalSize::new(200.0, 80.0);
/// assert_eq!(size.inline, 200.0);
/// assert_eq!(size.block, 80.0);
/// ```
#[derive(Copy, Clone, Debug, Default, PartialEq)]
pub struct LogicalSize {
    /// Dimension along the inline axis (advance dimension).
    pub inline: f32,
    /// Dimension along the block axis (line thickness / column depth).
    pub block: f32,
}

impl LogicalSize {
    /// Creates a new `LogicalSize`.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite_layout::vertical_flow::LogicalSize;
    ///
    /// let size = LogicalSize::new(100.0, 40.0);
    /// assert_eq!(size.inline, 100.0);
    /// assert_eq!(size.block, 40.0);
    /// ```
    #[inline(always)]
    pub const fn new(inline: f32, block: f32) -> Self {
        Self { inline, block }
    }

    /// Creates a zero-dimensioned `LogicalSize`.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite_layout::vertical_flow::LogicalSize;
    ///
    /// let size = LogicalSize::zero();
    /// assert_eq!(size.inline, 0.0);
    /// assert_eq!(size.block, 0.0);
    /// ```
    #[inline(always)]
    pub const fn zero() -> Self {
        Self {
            inline: 0.0,
            block: 0.0,
        }
    }
}

/// Transposition coordinator between physical screen coordinates and flow-relative logical coordinates.
///
/// # Examples
///
/// ```
/// use martensite_layout::geometry::{Point, Size};
/// use martensite_layout::vertical_flow::{FlowTransposition, LogicalPoint, WritingMode};
///
/// let trans = FlowTransposition::new(WritingMode::VerticalRl, Size::new(300.0, 500.0));
/// let physical = trans.to_physical_point(LogicalPoint::new(50.0, 40.0));
/// assert_eq!(physical.x, 260.0); // 300 - 40
/// assert_eq!(physical.y, 50.0);
/// ```
#[derive(Copy, Clone, Debug, PartialEq)]
pub struct FlowTransposition {
    /// Writing mode determining orientation and progression axes.
    pub mode: WritingMode,
    /// Physical dimensions of the containing layout box.
    pub container_size: Size,
}

impl FlowTransposition {
    /// Creates a new `FlowTransposition` bridge for the given writing mode and container size.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite_layout::geometry::Size;
    /// use martensite_layout::vertical_flow::{FlowTransposition, WritingMode};
    ///
    /// let trans = FlowTransposition::new(WritingMode::HorizontalTb, Size::new(400.0, 600.0));
    /// assert_eq!(trans.mode, WritingMode::HorizontalTb);
    /// ```
    #[inline(always)]
    pub const fn new(mode: WritingMode, container_size: Size) -> Self {
        Self {
            mode,
            container_size,
        }
    }

    /// Transforms a flow-relative [`LogicalPoint`] into physical screen [`Point`] coordinates.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite_layout::geometry::{Point, Size};
    /// use martensite_layout::vertical_flow::{FlowTransposition, LogicalPoint, WritingMode};
    ///
    /// let trans_rl = FlowTransposition::new(WritingMode::VerticalRl, Size::new(200.0, 400.0));
    /// let p = trans_rl.to_physical_point(LogicalPoint::new(30.0, 50.0));
    /// assert_eq!(p.x, 150.0); // 200 - 50
    /// assert_eq!(p.y, 30.0);
    /// ```
    #[inline]
    pub fn to_physical_point(&self, logical: LogicalPoint) -> Point {
        match self.mode {
            WritingMode::HorizontalTb => Point::new(logical.inline, logical.block),
            WritingMode::VerticalRl => {
                Point::new(self.container_size.width - logical.block, logical.inline)
            }
            WritingMode::VerticalLr => Point::new(logical.block, logical.inline),
        }
    }

    /// Transforms a flow-relative [`LogicalSize`] into physical screen [`Size`].
    ///
    /// In vertical modes, logical inline maps to physical height and logical block to physical width.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite_layout::geometry::Size;
    /// use martensite_layout::vertical_flow::{FlowTransposition, LogicalSize, WritingMode};
    ///
    /// let trans = FlowTransposition::new(WritingMode::VerticalRl, Size::new(300.0, 600.0));
    /// let phys_size = trans.to_physical_size(LogicalSize::new(150.0, 80.0));
    /// assert_eq!(phys_size.width, 80.0);
    /// assert_eq!(phys_size.height, 150.0);
    /// ```
    #[inline]
    pub fn to_physical_size(&self, logical: LogicalSize) -> Size {
        match self.mode {
            WritingMode::HorizontalTb => Size::new(logical.inline, logical.block),
            WritingMode::VerticalRl | WritingMode::VerticalLr => {
                Size::new(logical.block, logical.inline)
            }
        }
    }

    /// Transforms a physical screen [`Point`] into flow-relative [`LogicalPoint`].
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite_layout::geometry::{Point, Size};
    /// use martensite_layout::vertical_flow::{FlowTransposition, LogicalPoint, WritingMode};
    ///
    /// let trans = FlowTransposition::new(WritingMode::VerticalRl, Size::new(200.0, 400.0));
    /// let logical = trans.to_logical_point(Point::new(150.0, 30.0));
    /// assert_eq!(logical.inline, 30.0);
    /// assert_eq!(logical.block, 50.0);
    /// ```
    #[inline]
    pub fn to_logical_point(&self, physical: Point) -> LogicalPoint {
        match self.mode {
            WritingMode::HorizontalTb => LogicalPoint::new(physical.x, physical.y),
            WritingMode::VerticalRl => {
                LogicalPoint::new(physical.y, self.container_size.width - physical.x)
            }
            WritingMode::VerticalLr => LogicalPoint::new(physical.y, physical.x),
        }
    }

    /// Transforms a physical screen [`Size`] into flow-relative [`LogicalSize`].
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite_layout::geometry::Size;
    /// use martensite_layout::vertical_flow::{FlowTransposition, LogicalSize, WritingMode};
    ///
    /// let trans = FlowTransposition::new(WritingMode::VerticalRl, Size::new(300.0, 500.0));
    /// let logical = trans.to_logical_size(Size::new(50.0, 120.0));
    /// assert_eq!(logical.inline, 120.0);
    /// assert_eq!(logical.block, 50.0);
    /// ```
    #[inline]
    pub fn to_logical_size(&self, physical: Size) -> LogicalSize {
        match self.mode {
            WritingMode::HorizontalTb => LogicalSize::new(physical.width, physical.height),
            WritingMode::VerticalRl | WritingMode::VerticalLr => {
                LogicalSize::new(physical.height, physical.width)
            }
        }
    }

    /// Transposes layout [`Constraints`] across the inline and block axes.
    ///
    /// In vertical modes, width and height constraints are swapped so that measurement
    /// closures compute inline dimension against height constraints and block dimension
    /// against width constraints.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite_layout::geometry::{Constraints, Size};
    /// use martensite_layout::vertical_flow::{FlowTransposition, WritingMode};
    ///
    /// let trans = FlowTransposition::new(WritingMode::VerticalRl, Size::new(200.0, 400.0));
    /// let c = Constraints::new(10.0, 20.0, 100.0, 200.0);
    /// let transposed = trans.transpose_constraints(c);
    /// assert_eq!(transposed.min_width, 20.0);
    /// assert_eq!(transposed.min_height, 10.0);
    /// assert_eq!(transposed.max_width, 200.0);
    /// assert_eq!(transposed.max_height, 100.0);
    /// ```
    #[inline]
    pub fn transpose_constraints(&self, constraints: Constraints) -> Constraints {
        match self.mode {
            WritingMode::HorizontalTb => constraints,
            WritingMode::VerticalRl | WritingMode::VerticalLr => Constraints::new(
                constraints.min_height,
                constraints.min_width,
                constraints.max_height,
                constraints.max_width,
            ),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_horizontal_transposition_roundtrip() {
        let trans = FlowTransposition::new(WritingMode::HorizontalTb, Size::new(300.0, 400.0));
        let log_pt = LogicalPoint::new(25.0, 75.0);
        let phys_pt = trans.to_physical_point(log_pt);
        assert_eq!(phys_pt, Point::new(25.0, 75.0));
        assert_eq!(trans.to_logical_point(phys_pt), log_pt);

        let log_sz = LogicalSize::new(100.0, 50.0);
        let phys_sz = trans.to_physical_size(log_sz);
        assert_eq!(phys_sz, Size::new(100.0, 50.0));
        assert_eq!(trans.to_logical_size(phys_sz), log_sz);
    }

    #[test]
    fn test_vertical_rl_transposition_roundtrip() {
        let container = Size::new(500.0, 800.0);
        let trans = FlowTransposition::new(WritingMode::VerticalRl, container);

        let log_pt = LogicalPoint::new(60.0, 40.0);
        let phys_pt = trans.to_physical_point(log_pt);
        // x = 500 - 40 = 460, y = 60
        assert_eq!(phys_pt, Point::new(460.0, 60.0));
        assert_eq!(trans.to_logical_point(phys_pt), log_pt);

        let log_sz = LogicalSize::new(200.0, 80.0);
        let phys_sz = trans.to_physical_size(log_sz);
        // width = 80 (block), height = 200 (inline)
        assert_eq!(phys_sz, Size::new(80.0, 200.0));
        assert_eq!(trans.to_logical_size(phys_sz), log_sz);
    }

    #[test]
    fn test_vertical_lr_transposition_roundtrip() {
        let container = Size::new(500.0, 800.0);
        let trans = FlowTransposition::new(WritingMode::VerticalLr, container);

        let log_pt = LogicalPoint::new(60.0, 40.0);
        let phys_pt = trans.to_physical_point(log_pt);
        // x = 40 (block), y = 60 (inline)
        assert_eq!(phys_pt, Point::new(40.0, 60.0));
        assert_eq!(trans.to_logical_point(phys_pt), log_pt);

        let log_sz = LogicalSize::new(200.0, 80.0);
        let phys_sz = trans.to_physical_size(log_sz);
        assert_eq!(phys_sz, Size::new(80.0, 200.0));
        assert_eq!(trans.to_logical_size(phys_sz), log_sz);
    }

    #[test]
    fn test_transpose_constraints() {
        let c = Constraints::new(10.0, 20.0, 100.0, 200.0);

        let tb = FlowTransposition::new(WritingMode::HorizontalTb, Size::new(500.0, 500.0));
        assert_eq!(tb.transpose_constraints(c), c);

        let rl = FlowTransposition::new(WritingMode::VerticalRl, Size::new(500.0, 500.0));
        let transposed = rl.transpose_constraints(c);
        assert_eq!(transposed.min_width, 20.0);
        assert_eq!(transposed.min_height, 10.0);
        assert_eq!(transposed.max_width, 200.0);
        assert_eq!(transposed.max_height, 100.0);
    }
}
