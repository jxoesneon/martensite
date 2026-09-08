//! `Container` widget: a simple box with padding, optional color, and
//! a single child.
//!
//! The container is the most basic layout primitive. It wraps a single
//! child widget, applies padding (via [`EdgeInsets`]), and can optionally
//! paint a background color.
//!
//! # Examples
//!
//! ```
//! use martensite::widgets::container::Container;
//! use martensite::widgets::text::Text;
//!
//! let container = Container::new()
//!     .child(Text::new("Inside container"));
//! ```
//!
//! [`EdgeInsets`]: martensite_layout::geometry::EdgeInsets

use accesskit::Node as AccessKitNode;
use glam::Vec2;
use martensite_core::widget::{LayoutConstraints, LayoutContext, Widget};
use martensite_core::Rect;
use martensite_layout::geometry::{EdgeInsets, Size};
use martensite_theme::Oklab;

/// A container widget with padding, optional background, and a single child.
///
/// # Examples
///
/// ```
/// use martensite::widgets::Container;
/// use martensite_layout::geometry::EdgeInsets;
///
/// let c = Container::new()
///     .padding_uniform(16.0)
///     .child(Container::new());
/// assert!(c.child.is_some());
/// ```
pub struct Container {
    /// Padding around the child content.
    pub padding: EdgeInsets,
    /// Optional background color.
    pub background: Option<Oklab>,
    /// The single child widget, if any.
    pub child: Option<Box<dyn Widget>>,
    /// Cached content area size from the last measure pass.
    cached_content_size: Size,
    /// Cached bounds from the last layout pass.
    cached_bounds: Rect,
}

impl Container {
    /// Creates a new empty container with no padding and no background.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite::widgets::Container;
    ///
    /// let c = Container::new();
    /// assert!(c.child.is_none());
    /// ```
    pub fn new() -> Self {
        Self {
            padding: EdgeInsets::default(),
            background: None,
            child: None,
            cached_content_size: Size::zero(),
            cached_bounds: Rect::default(),
        }
    }

    /// Sets the padding.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite::widgets::Container;
    /// use martensite_layout::geometry::EdgeInsets;
    ///
    /// let c = Container::new().padding(EdgeInsets::uniform(8.0));
    /// assert_eq!(c.padding.left, 8.0);
    /// ```
    #[inline]
    #[must_use]
    pub fn padding(mut self, padding: EdgeInsets) -> Self {
        self.padding = padding;
        self
    }

    /// Sets uniform padding on all sides.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite::widgets::Container;
    ///
    /// let c = Container::new().padding_uniform(12.0);
    /// assert_eq!(c.padding.horizontal(), 24.0);
    /// ```
    #[inline]
    #[must_use]
    pub fn padding_uniform(mut self, value: f32) -> Self {
        self.padding = EdgeInsets::uniform(value);
        self
    }

    /// Sets the background color.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite::widgets::Container;
    /// use martensite_theme::Oklab;
    ///
    /// let c = Container::new().background(Oklab { l: 0.8, a: 0.0, b: 0.0, alpha: 1.0 });
    /// assert!(c.background.is_some());
    /// ```
    #[inline]
    #[must_use]
    pub fn background(mut self, color: Oklab) -> Self {
        self.background = Some(color);
        self
    }

    /// Sets the child widget.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite::widgets::Container;
    ///
    /// let c = Container::new().child(Container::new());
    /// assert!(c.child.is_some());
    /// ```
    #[inline]
    #[must_use]
    pub fn child(mut self, child: impl Widget + 'static) -> Self {
        self.child = Some(Box::new(child));
        self
    }

    /// Returns the content area (bounds minus padding) from the last layout.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite::widgets::Container;
    ///
    /// let c = Container::new().padding_uniform(8.0);
    /// let area = c.content_area();
    /// assert_eq!(area.origin.x, 8.0);
    /// ```
    #[inline]
    pub fn content_area(&self) -> Rect {
        Rect::new(
            self.cached_bounds.origin.x + self.padding.left,
            self.cached_bounds.origin.y + self.padding.top,
            self.cached_content_size.width,
            self.cached_content_size.height,
        )
    }
}

impl Default for Container {
    fn default() -> Self {
        Self::new()
    }
}

impl Widget for Container {
    fn measure(&mut self, cx: &mut LayoutContext, constraints: LayoutConstraints) -> Vec2 {
        // Subtract padding from available space for the child
        let h_inset = self.padding.horizontal();
        let v_inset = self.padding.vertical();

        let child_min = Vec2::new(
            (constraints.min_size.x - h_inset).max(0.0),
            (constraints.min_size.y - v_inset).max(0.0),
        );
        let child_max = Vec2::new(
            (constraints.max_size.x - h_inset).max(0.0),
            (constraints.max_size.y - v_inset).max(0.0),
        );

        let child_size = if let Some(child) = &mut self.child {
            let child_constraints = LayoutConstraints {
                min_size: child_min,
                max_size: child_max,
            };
            child.measure(cx, child_constraints)
        } else {
            Vec2::ZERO
        };

        // Add padding back for the container's total size
        let total = Vec2::new(child_size.x + h_inset, child_size.y + v_inset);

        self.cached_content_size = Size::new(child_size.x, child_size.y);
        total
    }

    fn layout(&mut self, cx: &mut LayoutContext, bounds: Rect) {
        self.cached_bounds = bounds;

        // Compute content area from bounds minus padding, not from
        // cached measure results. This ensures correct layout even
        // when measure was not called or was called with different
        // constraints.
        let content = Rect::new(
            bounds.origin.x + self.padding.left,
            bounds.origin.y + self.padding.top,
            (bounds.size.x - self.padding.horizontal()).max(0.0),
            (bounds.size.y - self.padding.vertical()).max(0.0),
        );
        self.cached_content_size = Size::new(content.size.x, content.size.y);

        if let Some(child) = &mut self.child {
            child.layout(cx, content);
        }
    }

    fn accessibility(&self, node: &mut AccessKitNode) {
        node.set_role(accesskit::Role::GenericContainer);
    }
}

impl std::fmt::Debug for Container {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Container")
            .field("padding", &self.padding)
            .field("background", &self.background)
            .field("has_child", &self.child.is_some())
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
    fn container_new_is_empty() {
        let c = Container::new();
        assert!(c.child.is_none());
        assert!(c.background.is_none());
        assert!(c.padding.is_zero());
    }

    #[test]
    fn container_builder_methods() {
        let c = Container::new().padding_uniform(10.0).background(Oklab {
            l: 1.0,
            a: 0.0,
            b: 0.0,
            alpha: 1.0,
        });
        assert_eq!(c.padding.horizontal(), 20.0);
        assert!(c.background.is_some());
    }

    #[test]
    fn container_measure_no_child() {
        let mut hot = HotNode::new(taffy::NodeId::new(1));
        let mut cx = make_cx(&mut hot);
        let mut c = Container::new().padding_uniform(10.0);
        let size = c.measure(
            &mut cx,
            LayoutConstraints {
                min_size: Vec2::ZERO,
                max_size: Vec2::new(100.0, 100.0),
            },
        );
        // No child → just padding
        assert_eq!(size, Vec2::new(20.0, 20.0));
    }

    #[test]
    fn container_measure_with_child() {
        let mut hot = HotNode::new(taffy::NodeId::new(1));
        let mut cx = make_cx(&mut hot);
        let mut c = Container::new().padding_uniform(10.0).child(DummyWidget);
        let size = c.measure(
            &mut cx,
            LayoutConstraints {
                min_size: Vec2::ZERO,
                max_size: Vec2::new(100.0, 100.0),
            },
        );
        // DummyWidget measures ZERO, so container is just padding
        assert_eq!(size, Vec2::new(20.0, 20.0));
    }

    #[test]
    fn container_layout_sets_bounds() {
        let mut hot = HotNode::new(taffy::NodeId::new(1));
        let mut cx = make_cx(&mut hot);
        let mut c = Container::new().padding_uniform(10.0);
        let bounds = Rect::new(0.0, 0.0, 200.0, 100.0);
        c.layout(&mut cx, bounds);
        assert_eq!(c.cached_bounds, bounds);
        // Content area should be deflated by padding
        let content = c.content_area();
        assert_eq!(content.origin.x, 10.0);
        assert_eq!(content.origin.y, 10.0);
    }

    #[test]
    fn container_debug_format() {
        let c = Container::new().padding_uniform(5.0);
        let debug = format!("{:?}", c);
        assert!(debug.contains("Container"));
    }
}
