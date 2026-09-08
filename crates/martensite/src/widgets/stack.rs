//! `Stack` widget: layers children on top of each other, with z-ordering.
//!
//! The `Stack` widget positions all children at the same bounds, with
//! later children painted on top of earlier ones. This is useful for
//! overlays, badges, and layered compositions.
//!
//! # Examples
//!
//! ```
//! use martensite::widgets::stack::{Stack, StackAlignment};
//! use martensite::widgets::text::Text;
//!
//! let stack = Stack::new()
//!     .alignment(StackAlignment::Center)
//!     .child(Text::new("Layer 1"));
//! assert_eq!(stack.child_count(), 1);
//! ```

use accesskit::Node as AccessKitNode;
use glam::Vec2;
use martensite_core::widget::{LayoutConstraints, LayoutContext, Widget};
use martensite_core::Rect;

/// How to align children within the stack.
///
/// # Examples
///
/// ```
/// use martensite::widgets::stack::StackAlignment;
///
/// assert_eq!(StackAlignment::default(), StackAlignment::TopStart);
/// ```
#[derive(Copy, Clone, Debug, Default, PartialEq, Eq, Hash)]
pub enum StackAlignment {
    /// Children are aligned to the top-left corner.
    #[default]
    TopStart,
    /// Children are aligned to the top-right corner.
    TopEnd,
    /// Children are aligned to the bottom-left corner.
    BottomStart,
    /// Children are aligned to the bottom-right corner.
    BottomEnd,
    /// Children are centered.
    Center,
    /// Children are stretched to fill the stack.
    Stretch,
}

/// A stack widget that layers children on top of each other.
///
/// # Examples
///
/// ```
/// use martensite::widgets::Stack;
/// use martensite::widgets::stack::StackAlignment;
/// use martensite_core::widget::DummyWidget;
///
/// let s = Stack::new()
///     .alignment(StackAlignment::Center)
///     .child(DummyWidget)
///     .child(DummyWidget);
/// assert_eq!(s.child_count(), 2);
/// ```
pub struct Stack {
    /// How to align children within the stack.
    pub alignment: StackAlignment,
    /// The child widgets, painted in order (later = on top).
    pub children: Vec<Box<dyn Widget>>,
    /// Cached child sizes from the last measure pass.
    child_sizes: Vec<Vec2>,
    /// Cached bounds from the last layout pass.
    cached_bounds: Rect,
}

impl Stack {
    /// Creates a new empty stack.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite::widgets::Stack;
    ///
    /// let s = Stack::new();
    /// assert_eq!(s.child_count(), 0);
    /// ```
    pub fn new() -> Self {
        Self {
            alignment: StackAlignment::default(),
            children: Vec::new(),
            child_sizes: Vec::new(),
            cached_bounds: Rect::default(),
        }
    }

    /// Sets the alignment of children within the stack.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite::widgets::Stack;
    /// use martensite::widgets::stack::StackAlignment;
    ///
    /// let s = Stack::new().alignment(StackAlignment::Center);
    /// assert_eq!(s.alignment, StackAlignment::Center);
    /// ```
    #[inline]
    pub fn alignment(mut self, alignment: StackAlignment) -> Self {
        self.alignment = alignment;
        self
    }

    /// Adds a child widget.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite::widgets::{Container, Stack};
    ///
    /// let s = Stack::new().child(Container::new());
    /// assert_eq!(s.child_count(), 1);
    /// ```
    #[inline]
    pub fn child(mut self, child: impl Widget + 'static) -> Self {
        self.children.push(Box::new(child));
        self
    }

    /// Returns the number of children.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite::widgets::Stack;
    ///
    /// let s = Stack::new();
    /// assert_eq!(s.child_count(), 0);
    /// ```
    #[inline]
    pub fn child_count(&self) -> usize {
        self.children.len()
    }
}

impl Default for Stack {
    fn default() -> Self {
        Self::new()
    }
}

impl Widget for Stack {
    fn measure(&mut self, cx: &mut LayoutContext, constraints: LayoutConstraints) -> Vec2 {
        self.child_sizes.clear();
        let n = self.children.len();
        if n == 0 {
            return Vec2::ZERO;
        }
        self.child_sizes.reserve(n);

        let mut max_width = 0.0f32;
        let mut max_height = 0.0f32;

        for child in &mut self.children {
            let child_constraints = LayoutConstraints {
                min_size: Vec2::ZERO,
                max_size: constraints.max_size,
            };
            let size = child.measure(cx, child_constraints);
            self.child_sizes.push(size);
            max_width = max_width.max(size.x);
            max_height = max_height.max(size.y);
        }

        Vec2::new(max_width, max_height)
    }

    fn layout(&mut self, cx: &mut LayoutContext, bounds: Rect) {
        self.cached_bounds = bounds;

        let alignment = self.alignment;

        for (i, child) in self.children.iter_mut().enumerate() {
            let child_size = self.child_sizes.get(i).copied().unwrap_or(Vec2::ZERO);

            let (w, h) = if matches!(alignment, StackAlignment::Stretch) {
                (bounds.size.x, bounds.size.y)
            } else {
                (child_size.x, child_size.y)
            };

            let pos = match alignment {
                StackAlignment::TopStart | StackAlignment::Stretch => bounds.origin,
                StackAlignment::TopEnd => {
                    Vec2::new(bounds.origin.x + bounds.size.x - w, bounds.origin.y)
                }
                StackAlignment::BottomStart => {
                    Vec2::new(bounds.origin.x, bounds.origin.y + bounds.size.y - h)
                }
                StackAlignment::BottomEnd => Vec2::new(
                    bounds.origin.x + bounds.size.x - w,
                    bounds.origin.y + bounds.size.y - h,
                ),
                StackAlignment::Center => Vec2::new(
                    bounds.origin.x + (bounds.size.x - w) / 2.0,
                    bounds.origin.y + (bounds.size.y - h) / 2.0,
                ),
            };
            child.layout(cx, Rect::new(pos.x, pos.y, w, h));
        }
    }

    fn accessibility(&self, node: &mut AccessKitNode) {
        node.set_role(accesskit::Role::GenericContainer);
    }
}

impl std::fmt::Debug for Stack {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Stack")
            .field("alignment", &self.alignment)
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
    fn stack_new_is_empty() {
        let s = Stack::new();
        assert!(s.children.is_empty());
    }

    #[test]
    fn stack_measure_empty() {
        let mut hot = HotNode::new(taffy::NodeId::new(1));
        let mut cx = make_cx(&mut hot);
        let mut s = Stack::new();
        let size = s.measure(
            &mut cx,
            LayoutConstraints {
                min_size: Vec2::ZERO,
                max_size: Vec2::new(100.0, 100.0),
            },
        );
        assert_eq!(size, Vec2::ZERO);
    }

    #[test]
    fn stack_measure_with_children() {
        let mut hot = HotNode::new(taffy::NodeId::new(1));
        let mut cx = make_cx(&mut hot);
        let mut s = Stack::new().child(DummyWidget).child(DummyWidget);
        let size = s.measure(
            &mut cx,
            LayoutConstraints {
                min_size: Vec2::ZERO,
                max_size: Vec2::new(100.0, 100.0),
            },
        );
        // DummyWidgets measure ZERO
        assert_eq!(size, Vec2::ZERO);
    }

    #[test]
    fn stack_layout_positions_children() {
        let mut hot = HotNode::new(taffy::NodeId::new(1));
        let mut cx = make_cx(&mut hot);
        let mut s = Stack::new().child(DummyWidget).child(DummyWidget);
        s.measure(
            &mut cx,
            LayoutConstraints {
                min_size: Vec2::ZERO,
                max_size: Vec2::new(100.0, 100.0),
            },
        );
        s.layout(&mut cx, Rect::new(10.0, 20.0, 100.0, 50.0));
        assert_eq!(s.cached_bounds, Rect::new(10.0, 20.0, 100.0, 50.0));
    }

    #[test]
    fn stack_alignment_center() {
        let mut hot = HotNode::new(taffy::NodeId::new(1));
        let mut cx = make_cx(&mut hot);
        let mut s = Stack::new()
            .alignment(StackAlignment::Center)
            .child(DummyWidget);
        s.measure(
            &mut cx,
            LayoutConstraints {
                min_size: Vec2::ZERO,
                max_size: Vec2::new(100.0, 100.0),
            },
        );
        s.layout(&mut cx, Rect::new(0.0, 0.0, 100.0, 100.0));
    }

    #[test]
    fn stack_alignment_stretch() {
        let mut hot = HotNode::new(taffy::NodeId::new(1));
        let mut cx = make_cx(&mut hot);
        let mut s = Stack::new()
            .alignment(StackAlignment::Stretch)
            .child(DummyWidget);
        s.measure(
            &mut cx,
            LayoutConstraints {
                min_size: Vec2::ZERO,
                max_size: Vec2::new(100.0, 100.0),
            },
        );
        s.layout(&mut cx, Rect::new(0.0, 0.0, 100.0, 100.0));
    }

    #[test]
    fn stack_alignment_corners() {
        let mut hot = HotNode::new(taffy::NodeId::new(1));
        let mut cx = make_cx(&mut hot);
        for align in [
            StackAlignment::TopStart,
            StackAlignment::TopEnd,
            StackAlignment::BottomStart,
            StackAlignment::BottomEnd,
        ] {
            let mut s = Stack::new().alignment(align).child(DummyWidget);
            s.measure(
                &mut cx,
                LayoutConstraints {
                    min_size: Vec2::ZERO,
                    max_size: Vec2::new(100.0, 100.0),
                },
            );
            s.layout(&mut cx, Rect::new(0.0, 0.0, 100.0, 100.0));
        }
    }

    #[test]
    fn stack_debug_format() {
        let s = Stack::new()
            .alignment(StackAlignment::Center)
            .child(DummyWidget);
        let debug = format!("{:?}", s);
        assert!(debug.contains("Stack"));
        assert!(debug.contains("Center"));
    }
}
