//! `Flex` widget: row/column flexbox layout for multiple children.
//!
//! The `Flex` widget arranges its children along a main axis (horizontal
//! for `Row`, vertical for `Column`) and aligns them on the cross axis.
//! It supports gaps between children and main-axis distribution.
//!
//! # Examples
//!
//! ```
//! use martensite::widgets::flex::Flex;
//! use martensite::widgets::text::Text;
//!
//! let row = Flex::row()
//!     .gap(8.0)
//!     .child(Text::new("First"))
//!     .child(Text::new("Second"));
//! assert_eq!(row.child_count(), 2);
//! ```

use accesskit::Node as AccessKitNode;
use glam::Vec2;
use martensite_core::widget::{LayoutConstraints, LayoutContext, Widget};
use martensite_core::Rect;

/// The direction of flex layout.
///
/// # Examples
///
/// ```
/// use martensite::widgets::FlexDirection;
///
/// assert!(FlexDirection::Row.is_row());
/// assert!(FlexDirection::Column.is_column());
/// ```
#[derive(Copy, Clone, Debug, Default, PartialEq, Eq, Hash)]
pub enum FlexDirection {
    /// Children are arranged horizontally (left to right).
    #[default]
    Row,
    /// Children are arranged vertically (top to bottom).
    Column,
}

impl FlexDirection {
    /// Returns `true` if this is a horizontal (row) direction.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite::widgets::FlexDirection;
    ///
    /// assert!(FlexDirection::Row.is_row());
    /// assert!(!FlexDirection::Column.is_row());
    /// ```
    #[inline]
    pub fn is_row(self) -> bool {
        matches!(self, Self::Row)
    }

    /// Returns `true` if this is a vertical (column) direction.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite::widgets::FlexDirection;
    ///
    /// assert!(FlexDirection::Column.is_column());
    /// assert!(!FlexDirection::Row.is_column());
    /// ```
    #[inline]
    pub fn is_column(self) -> bool {
        matches!(self, Self::Column)
    }

    /// Returns the main-axis component of a `Vec2`.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite::widgets::FlexDirection;
    /// use glam::Vec2;
    ///
    /// let v = Vec2::new(10.0, 20.0);
    /// assert_eq!(FlexDirection::Row.main(v), 10.0);
    /// assert_eq!(FlexDirection::Column.main(v), 20.0);
    /// ```
    #[inline]
    pub fn main(self, v: Vec2) -> f32 {
        if self.is_row() {
            v.x
        } else {
            v.y
        }
    }

    /// Returns the cross-axis component of a `Vec2`.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite::widgets::FlexDirection;
    /// use glam::Vec2;
    ///
    /// let v = Vec2::new(10.0, 20.0);
    /// assert_eq!(FlexDirection::Row.cross(v), 20.0);
    /// assert_eq!(FlexDirection::Column.cross(v), 10.0);
    /// ```
    #[inline]
    pub fn cross(self, v: Vec2) -> f32 {
        if self.is_row() {
            v.y
        } else {
            v.x
        }
    }

    /// Constructs a `Vec2` from main and cross components.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite::widgets::FlexDirection;
    /// use glam::Vec2;
    ///
    /// assert_eq!(FlexDirection::Row.vec(10.0, 20.0), Vec2::new(10.0, 20.0));
    /// assert_eq!(FlexDirection::Column.vec(10.0, 20.0), Vec2::new(20.0, 10.0));
    /// ```
    #[inline]
    pub fn vec(self, main: f32, cross: f32) -> Vec2 {
        if self.is_row() {
            Vec2::new(main, cross)
        } else {
            Vec2::new(cross, main)
        }
    }
}

/// How to distribute children along the main axis.
///
/// # Examples
///
/// ```
/// use martensite::widgets::flex::MainAxisAlignment;
///
/// assert_eq!(MainAxisAlignment::default(), MainAxisAlignment::Start);
/// ```
#[derive(Copy, Clone, Debug, Default, PartialEq, Eq, Hash)]
pub enum MainAxisAlignment {
    /// Children are packed toward the start of the main axis.
    #[default]
    Start,
    /// Children are packed toward the end of the main axis.
    End,
    /// Children are centered along the main axis.
    Center,
    /// Children are evenly distributed with equal space between them.
    SpaceBetween,
    /// Children are evenly distributed with equal space around them.
    SpaceEvenly,
}

/// How to align children on the cross axis.
///
/// # Examples
///
/// ```
/// use martensite::widgets::flex::CrossAxisAlignment;
///
/// assert_eq!(CrossAxisAlignment::default(), CrossAxisAlignment::Stretch);
/// ```
#[derive(Copy, Clone, Debug, Default, PartialEq, Eq, Hash)]
pub enum CrossAxisAlignment {
    /// Children are stretched to fill the cross axis.
    #[default]
    Stretch,
    /// Children are aligned to the start of the cross axis.
    Start,
    /// Children are aligned to the end of the cross axis.
    End,
    /// Children are centered on the cross axis.
    Center,
}

/// A flex container widget that arranges children in a row or column.
///
/// # Examples
///
/// ```
/// use martensite::widgets::Flex;
/// use martensite::widgets::flex::MainAxisAlignment;
/// use martensite_core::widget::DummyWidget;
///
/// let row = Flex::row()
///     .gap(8.0)
///     .main_axis_alignment(MainAxisAlignment::Center)
///     .child(DummyWidget)
///     .child(DummyWidget);
/// assert_eq!(row.child_count(), 2);
/// assert!(row.direction.is_row());
/// ```
pub struct Flex {
    /// The direction of layout (row or column).
    pub direction: FlexDirection,
    /// How to distribute children along the main axis.
    pub main_axis_alignment: MainAxisAlignment,
    /// How to align children on the cross axis.
    pub cross_axis_alignment: CrossAxisAlignment,
    /// Gap between children in logical pixels.
    pub gap: f32,
    /// The child widgets.
    pub children: Vec<Box<dyn Widget>>,
    /// Cached child sizes from the last measure pass.
    child_sizes: Vec<Vec2>,
    /// Cached bounds from the last layout pass.
    cached_bounds: Rect,
}

impl Flex {
    /// Creates a new flex container with the given direction.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite::widgets::{Flex, FlexDirection};
    ///
    /// let flex = Flex::new(FlexDirection::Row);
    /// assert!(flex.direction.is_row());
    /// ```
    pub fn new(direction: FlexDirection) -> Self {
        Self {
            direction,
            main_axis_alignment: MainAxisAlignment::default(),
            cross_axis_alignment: CrossAxisAlignment::default(),
            gap: 0.0,
            children: Vec::new(),
            child_sizes: Vec::new(),
            cached_bounds: Rect::default(),
        }
    }

    /// Creates a new row (horizontal flex).
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite::widgets::Flex;
    ///
    /// let row = Flex::row();
    /// assert!(row.direction.is_row());
    /// ```
    #[inline]
    #[must_use]
    pub fn row() -> Self {
        Self::new(FlexDirection::Row)
    }

    /// Creates a new column (vertical flex).
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite::widgets::Flex;
    ///
    /// let col = Flex::column();
    /// assert!(col.direction.is_column());
    /// ```
    #[inline]
    #[must_use]
    pub fn column() -> Self {
        Self::new(FlexDirection::Column)
    }

    /// Sets the main axis alignment.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite::widgets::Flex;
    /// use martensite::widgets::flex::MainAxisAlignment;
    ///
    /// let flex = Flex::row().main_axis_alignment(MainAxisAlignment::Center);
    /// assert_eq!(flex.main_axis_alignment, MainAxisAlignment::Center);
    /// ```
    #[inline]
    #[must_use]
    pub fn main_axis_alignment(mut self, alignment: MainAxisAlignment) -> Self {
        self.main_axis_alignment = alignment;
        self
    }

    /// Sets the cross axis alignment.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite::widgets::Flex;
    /// use martensite::widgets::flex::CrossAxisAlignment;
    ///
    /// let flex = Flex::row().cross_axis_alignment(CrossAxisAlignment::Center);
    /// assert_eq!(flex.cross_axis_alignment, CrossAxisAlignment::Center);
    /// ```
    #[inline]
    #[must_use]
    pub fn cross_axis_alignment(mut self, alignment: CrossAxisAlignment) -> Self {
        self.cross_axis_alignment = alignment;
        self
    }

    /// Sets the gap between children.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite::widgets::Flex;
    ///
    /// let flex = Flex::row().gap(16.0);
    /// assert_eq!(flex.gap, 16.0);
    /// ```
    #[inline]
    #[must_use]
    pub fn gap(mut self, gap: f32) -> Self {
        self.gap = gap;
        self
    }

    /// Adds a child widget.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite::widgets::{Container, Flex};
    ///
    /// let flex = Flex::row().child(Container::new());
    /// assert_eq!(flex.child_count(), 1);
    /// ```
    #[inline]
    #[must_use]
    pub fn child(mut self, child: impl Widget + 'static) -> Self {
        self.children.push(Box::new(child));
        self
    }

    /// Adds multiple child widgets.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite::widgets::{Container, Flex};
    /// use martensite_core::widget::Widget;
    ///
    /// let items: Vec<Box<dyn Widget>> = vec![Box::new(Container::new()), Box::new(Container::new())];
    /// let flex = Flex::row().children(items);
    /// assert_eq!(flex.child_count(), 2);
    /// ```
    #[inline]
    #[must_use]
    pub fn children(mut self, children: impl IntoIterator<Item = Box<dyn Widget>>) -> Self {
        self.children.extend(children);
        self
    }

    /// Returns the number of children.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite::widgets::Flex;
    ///
    /// let flex = Flex::row();
    /// assert_eq!(flex.child_count(), 0);
    /// ```
    #[inline]
    pub fn child_count(&self) -> usize {
        self.children.len()
    }

    /// Computes the main-axis offset for each child given the total
    /// main-axis size and the total children main-axis size.
    fn compute_main_offsets(&self, total_main: f32, children_main: f32) -> Vec<f32> {
        let n = self.children.len();
        if n == 0 {
            return vec![];
        }

        // Note: children_main already includes total_gap (see caller),
        // so free_space = total_main - sum(child_sizes) - total_gap.
        // This is the space available for alignment distribution.
        let free_space = (total_main - children_main).max(0.0);

        match self.main_axis_alignment {
            MainAxisAlignment::Start => {
                let mut offsets = Vec::with_capacity(n);
                let mut cursor = 0.0f32;
                for i in 0..n {
                    offsets.push(cursor);
                    cursor += self
                        .direction
                        .main(self.child_sizes.get(i).copied().unwrap_or(Vec2::ZERO));
                    cursor += self.gap;
                }
                offsets
            }
            MainAxisAlignment::End => {
                let mut offsets = Vec::with_capacity(n);
                let mut cursor = free_space;
                for i in 0..n {
                    offsets.push(cursor);
                    cursor += self
                        .direction
                        .main(self.child_sizes.get(i).copied().unwrap_or(Vec2::ZERO));
                    cursor += self.gap;
                }
                offsets
            }
            MainAxisAlignment::Center => {
                let mut offsets = Vec::with_capacity(n);
                let mut cursor = free_space / 2.0;
                for i in 0..n {
                    offsets.push(cursor);
                    cursor += self
                        .direction
                        .main(self.child_sizes.get(i).copied().unwrap_or(Vec2::ZERO));
                    cursor += self.gap;
                }
                offsets
            }
            MainAxisAlignment::SpaceBetween => {
                let mut offsets = Vec::with_capacity(n);
                let space_between = if n > 1 {
                    free_space / (n - 1) as f32
                } else {
                    0.0
                };
                let mut cursor = 0.0f32;
                for i in 0..n {
                    offsets.push(cursor);
                    cursor += self
                        .direction
                        .main(self.child_sizes.get(i).copied().unwrap_or(Vec2::ZERO));
                    cursor += self.gap + space_between;
                }
                offsets
            }
            MainAxisAlignment::SpaceEvenly => {
                let mut offsets = Vec::with_capacity(n);
                // free_space = total_main - sum(child_sizes) - total_gap
                // We distribute free_space evenly across (n+1) slots.
                // Inter-child spacing is gap + space; leading/trailing is space.
                let space = if n > 0 {
                    free_space / (n + 1) as f32
                } else {
                    0.0
                };
                let mut cursor = space;
                for i in 0..n {
                    offsets.push(cursor);
                    cursor += self
                        .direction
                        .main(self.child_sizes.get(i).copied().unwrap_or(Vec2::ZERO));
                    cursor += self.gap + space;
                }
                offsets
            }
        }
    }
}

impl Widget for Flex {
    fn measure(&mut self, cx: &mut LayoutContext, constraints: LayoutConstraints) -> Vec2 {
        let n = self.children.len();
        if n == 0 {
            return Vec2::ZERO;
        }

        self.child_sizes.clear();
        self.child_sizes.reserve(n);

        let mut total_main = 0.0f32;
        let mut max_cross = 0.0f32;
        let total_gap = self.gap * (n.saturating_sub(1)) as f32;

        for child in &mut self.children {
            // Give each child the remaining main-axis space after
            // accounting for previously-measured siblings and gaps.
            // Only check the main-axis constraint, not the cross-axis.
            let max_main = self.direction.main(constraints.max_size);
            let remaining_main = if max_main.is_finite() {
                (max_main - total_main - total_gap).max(0.0)
            } else {
                f32::MAX
            };
            let cross_limit = self.direction.cross(constraints.max_size);
            let child_max = if self.direction.is_row() {
                Vec2::new(remaining_main, cross_limit)
            } else {
                Vec2::new(cross_limit, remaining_main)
            };
            let child_constraints = LayoutConstraints {
                min_size: Vec2::ZERO,
                max_size: child_max,
            };
            let size = child.measure(cx, child_constraints);
            self.child_sizes.push(size);
            total_main += self.direction.main(size);
            max_cross = max_cross.max(self.direction.cross(size));
        }

        total_main += total_gap;

        self.direction.vec(total_main, max_cross)
    }

    fn layout(&mut self, cx: &mut LayoutContext, bounds: Rect) {
        self.cached_bounds = bounds;
        let n = self.children.len();
        if n == 0 {
            return;
        }

        let total_main = self.direction.main(bounds.size);
        let cross_size = self.direction.cross(bounds.size);

        let children_main: f32 = self
            .child_sizes
            .iter()
            .map(|s| self.direction.main(*s))
            .sum::<f32>()
            + self.gap * (n.saturating_sub(1)) as f32;

        let main_offsets = self.compute_main_offsets(total_main, children_main);
        let cross_alignment = self.cross_axis_alignment;
        let direction = self.direction;

        for (i, child) in self.children.iter_mut().enumerate() {
            let child_size = self.child_sizes.get(i).copied().unwrap_or(Vec2::ZERO);
            let child_main = direction.main(child_size);
            let child_cross = if matches!(cross_alignment, CrossAxisAlignment::Stretch) {
                cross_size
            } else {
                direction.cross(child_size)
            };

            let main_offset = main_offsets[i];
            let cross_offset = match cross_alignment {
                CrossAxisAlignment::Stretch | CrossAxisAlignment::Start => 0.0,
                CrossAxisAlignment::End => (cross_size - child_cross).max(0.0),
                CrossAxisAlignment::Center => ((cross_size - child_cross) / 2.0).max(0.0),
            };

            let (x, y) = if direction.is_row() {
                (
                    bounds.origin.x + main_offset,
                    bounds.origin.y + cross_offset,
                )
            } else {
                (
                    bounds.origin.x + cross_offset,
                    bounds.origin.y + main_offset,
                )
            };

            // For Row: width=child_main, height=child_cross
            // For Column: width=child_cross, height=child_main
            let (w, h) = if direction.is_row() {
                (child_main, child_cross)
            } else {
                (child_cross, child_main)
            };
            let child_bounds = Rect::new(x, y, w, h);
            child.layout(cx, child_bounds);
        }
    }

    fn accessibility(&self, node: &mut AccessKitNode) {
        node.set_role(accesskit::Role::GenericContainer);
    }
}

impl std::fmt::Debug for Flex {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Flex")
            .field("direction", &self.direction)
            .field("main_axis_alignment", &self.main_axis_alignment)
            .field("cross_axis_alignment", &self.cross_axis_alignment)
            .field("gap", &self.gap)
            .field("child_count", &self.children.len())
            .finish()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use martensite_core::widget::DummyWidget;
    use martensite_core::HotNode;

    fn make_cx(hot: &mut HotNode) -> LayoutContext<'_> {
        LayoutContext { hot }
    }

    #[test]
    fn flex_row_new() {
        let f = Flex::row();
        assert_eq!(f.direction, FlexDirection::Row);
        assert!(f.children.is_empty());
    }

    #[test]
    fn flex_column_new() {
        let f = Flex::column();
        assert_eq!(f.direction, FlexDirection::Column);
    }

    #[test]
    fn flex_direction_helpers() {
        assert!(FlexDirection::Row.is_row());
        assert!(!FlexDirection::Row.is_column());
        assert!(FlexDirection::Column.is_column());
        let v = Vec2::new(10.0, 20.0);
        assert_eq!(FlexDirection::Row.main(v), 10.0);
        assert_eq!(FlexDirection::Row.cross(v), 20.0);
        assert_eq!(FlexDirection::Column.main(v), 20.0);
        assert_eq!(FlexDirection::Column.cross(v), 10.0);
    }

    #[test]
    fn flex_measure_empty() {
        let mut hot = HotNode::new(taffy::NodeId::new(1));
        let mut cx = make_cx(&mut hot);
        let mut f = Flex::row();
        let size = f.measure(
            &mut cx,
            LayoutConstraints {
                min_size: Vec2::ZERO,
                max_size: Vec2::new(100.0, 100.0),
            },
        );
        assert_eq!(size, Vec2::ZERO);
    }

    #[test]
    fn flex_measure_with_dummy_children() {
        let mut hot = HotNode::new(taffy::NodeId::new(1));
        let mut cx = make_cx(&mut hot);
        let mut f = Flex::row()
            .child(DummyWidget)
            .child(DummyWidget)
            .child(DummyWidget);
        let size = f.measure(
            &mut cx,
            LayoutConstraints {
                min_size: Vec2::ZERO,
                max_size: Vec2::new(100.0, 100.0),
            },
        );
        // DummyWidgets measure ZERO, so flex is ZERO
        assert_eq!(size, Vec2::ZERO);
    }

    #[test]
    fn flex_layout_positions_children() {
        let mut hot = HotNode::new(taffy::NodeId::new(1));
        let mut cx = make_cx(&mut hot);
        let mut f = Flex::row().child(DummyWidget).child(DummyWidget);
        // Measure first to populate child_sizes
        f.measure(
            &mut cx,
            LayoutConstraints {
                min_size: Vec2::ZERO,
                max_size: Vec2::new(100.0, 100.0),
            },
        );
        // Layout
        f.layout(&mut cx, Rect::new(0.0, 0.0, 200.0, 100.0));
        assert_eq!(f.cached_bounds, Rect::new(0.0, 0.0, 200.0, 100.0));
    }

    #[test]
    fn flex_main_axis_alignment_center() {
        let mut hot = HotNode::new(taffy::NodeId::new(1));
        let mut cx = make_cx(&mut hot);
        let mut f = Flex::row()
            .main_axis_alignment(MainAxisAlignment::Center)
            .child(DummyWidget);
        f.measure(
            &mut cx,
            LayoutConstraints {
                min_size: Vec2::ZERO,
                max_size: Vec2::new(100.0, 100.0),
            },
        );
        f.layout(&mut cx, Rect::new(0.0, 0.0, 100.0, 50.0));
        // Should not panic
    }

    #[test]
    fn flex_space_between() {
        let mut hot = HotNode::new(taffy::NodeId::new(1));
        let mut cx = make_cx(&mut hot);
        let mut f = Flex::row()
            .main_axis_alignment(MainAxisAlignment::SpaceBetween)
            .child(DummyWidget)
            .child(DummyWidget);
        f.measure(
            &mut cx,
            LayoutConstraints {
                min_size: Vec2::ZERO,
                max_size: Vec2::new(100.0, 100.0),
            },
        );
        f.layout(&mut cx, Rect::new(0.0, 0.0, 100.0, 50.0));
    }

    #[test]
    fn flex_space_evenly() {
        let mut hot = HotNode::new(taffy::NodeId::new(1));
        let mut cx = make_cx(&mut hot);
        let mut f = Flex::row()
            .main_axis_alignment(MainAxisAlignment::SpaceEvenly)
            .child(DummyWidget);
        f.measure(
            &mut cx,
            LayoutConstraints {
                min_size: Vec2::ZERO,
                max_size: Vec2::new(100.0, 100.0),
            },
        );
        f.layout(&mut cx, Rect::new(0.0, 0.0, 100.0, 50.0));
    }

    #[test]
    fn flex_cross_axis_alignment() {
        let mut hot = HotNode::new(taffy::NodeId::new(1));
        let mut cx = make_cx(&mut hot);
        let mut f = Flex::row()
            .cross_axis_alignment(CrossAxisAlignment::Center)
            .child(DummyWidget);
        f.measure(
            &mut cx,
            LayoutConstraints {
                min_size: Vec2::ZERO,
                max_size: Vec2::new(100.0, 100.0),
            },
        );
        f.layout(&mut cx, Rect::new(0.0, 0.0, 100.0, 50.0));
    }

    #[test]
    fn flex_gap() {
        let mut hot = HotNode::new(taffy::NodeId::new(1));
        let mut cx = make_cx(&mut hot);
        let mut f = Flex::row().gap(10.0).child(DummyWidget).child(DummyWidget);
        let size = f.measure(
            &mut cx,
            LayoutConstraints {
                min_size: Vec2::ZERO,
                max_size: Vec2::new(100.0, 100.0),
            },
        );
        // DummyWidgets are zero, so size is just gap
        assert_eq!(size.x, 10.0);
    }

    #[test]
    fn flex_debug_format() {
        let f = Flex::row().gap(5.0).child(DummyWidget);
        let debug = format!("{:?}", f);
        assert!(debug.contains("Flex"));
        assert!(debug.contains("Row"));
    }
}
