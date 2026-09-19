//! Masonry layout — children flow into the currently-shortest
//! column (Pinterest layout, CSS `masonry`).
//!
//! Each child is measured at the column width and placed at the top
//! of whichever column is shortest, yielding the staggered grid the
//! idiom is known for. Column count is fixed; responsive column
//! derivation belongs to the consumer (rebuild with a new `columns`
//! when the viewport changes).
//!
//! # Examples
//!
//! ```
//! use martensite::core::Widget;
//! use martensite::widgets::{Masonry, Text};
//!
//! let m = Masonry::new()
//!     .columns(3)
//!     .child(Text::new("a"))
//!     .child(Text::new("b"));
//! assert_eq!(m.child_count(), 2);
//! ```

use accesskit::Node as AccessKitNode;
use glam::Vec2;
use martensite_core::{
    EventContext, EventResponse, LayoutConstraints, LayoutContext, PaintContext, Rect,
    RenderMinimum, UnderflowPolicy, Widget, WidgetEvent,
};

/// Inter-column/row gap in points.
const GAP_PT: f32 = 8.0;

/// Masonry container.
///
/// # Examples
///
/// ```
/// use martensite::core::Widget;
/// use martensite::widgets::Masonry;
///
/// assert_eq!(Masonry::new().child_count(), 0);
/// ```
pub struct Masonry {
    /// Accessibility label override.
    pub label: Option<String>,
    /// Whether input reaches children.
    pub enabled: bool,
    /// Column count (≥1).
    pub columns: usize,
    /// Gap between cells in points.
    pub gap: f32,
    children: Vec<Box<dyn Widget>>,
    bounds: Rect,
    cell_bounds: Vec<Rect>,
    /// Content height after the last layout — consumers use it to
    /// size a wrapping `ScrollView`.
    content_height: f32,
}

impl Masonry {
    /// Creates an empty 2-column masonry.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite::widgets::Masonry;
    ///
    /// assert_eq!(Masonry::new().columns, 2);
    /// ```
    pub fn new() -> Self {
        Self {
            label: None,
            enabled: true,
            columns: 2,
            gap: GAP_PT,
            children: Vec::new(),
            bounds: Rect::default(),
            cell_bounds: Vec::new(),
            content_height: 0.0,
        }
    }

    /// Sets the column count (clamped ≥1).
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite::widgets::Masonry;
    ///
    /// assert_eq!(Masonry::new().columns(4).columns, 4);
    /// assert_eq!(Masonry::new().columns(0).columns, 1);
    /// ```
    #[must_use]
    pub fn columns(mut self, n: usize) -> Self {
        self.columns = n.max(1);
        self
    }

    /// Sets the inter-cell gap in points.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite::widgets::Masonry;
    ///
    /// assert_eq!(Masonry::new().gap(12.0).gap, 12.0);
    /// ```
    #[must_use]
    pub fn gap(mut self, pts: f32) -> Self {
        self.gap = pts.max(0.0);
        self
    }

    /// Sets the accessibility label.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite::widgets::Masonry;
    ///
    /// let m = Masonry::new().label("Gallery");
    /// assert_eq!(m.label.as_deref(), Some("Gallery"));
    /// ```
    #[must_use]
    pub fn label(mut self, text: impl Into<String>) -> Self {
        self.label = Some(text.into());
        self
    }

    /// Sets whether input reaches children.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite::widgets::Masonry;
    ///
    /// assert!(!Masonry::new().enabled(false).enabled);
    /// ```
    #[must_use]
    pub fn enabled(mut self, flag: bool) -> Self {
        self.enabled = flag;
        self
    }

    /// Appends a child.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite::core::Widget;
    /// use martensite::widgets::{Masonry, Text};
    ///
    /// assert_eq!(Masonry::new().child(Text::new("a")).child_count(), 1);
    /// ```
    #[must_use]
    pub fn child(mut self, w: impl Widget + 'static) -> Self {
        self.children.push(Box::new(w));
        self
    }

    /// Removes all children.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite::core::Widget;
    /// use martensite::widgets::{Masonry, Text};
    ///
    /// let mut m = Masonry::new().child(Text::new("a"));
    /// m.clear();
    /// assert_eq!(m.child_count(), 0);
    /// ```
    pub fn clear(&mut self) {
        self.children.clear();
        self.cell_bounds.clear();
        self.content_height = 0.0;
    }

    /// Total laid-out content height in device px — feed a wrapping
    /// `ScrollView`'s content size.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite::widgets::Masonry;
    ///
    /// assert_eq!(Masonry::new().content_height(), 0.0);
    /// ```
    #[inline]
    pub fn content_height(&self) -> f32 {
        self.content_height
    }
}

impl Default for Masonry {
    fn default() -> Self {
        Self::new()
    }
}

impl std::fmt::Debug for Masonry {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Masonry")
            .field("columns", &self.columns)
            .field("children", &self.children.len())
            .finish()
    }
}

impl Widget for Masonry {
    fn measure(&mut self, cx: &mut LayoutContext, constraints: LayoutConstraints) -> Vec2 {
        Vec2::new(
            constraints
                .max_size
                .x
                .max(cx.pt(200.0).min(constraints.max_size.x)),
            constraints
                .max_size
                .y
                .max(cx.pt(120.0).min(constraints.max_size.y)),
        )
    }

    fn min_render(&self) -> RenderMinimum {
        RenderMinimum::new(Vec2::new(160.0, 120.0)).with_policy(UnderflowPolicy::Lint)
    }

    fn layout(&mut self, cx: &mut LayoutContext, bounds: Rect) {
        self.bounds = bounds;
        let cols = self.columns.max(1);
        let gap = cx.pt(self.gap);
        let col_w =
            ((bounds.width() - gap * (cols.saturating_sub(1)) as f32) / cols as f32).max(1.0);
        let mut heights = vec![bounds.min_y(); cols];
        self.cell_bounds = Vec::with_capacity(self.children.len());
        for child in self.children.iter_mut() {
            // Shortest column wins; ties break leftmost.
            let (col, &top) = heights
                .iter()
                .enumerate()
                .min_by(|a, b| a.1.total_cmp(b.1))
                .unwrap_or((0, &bounds.min_y()));
            // Measure the child at the column width for its natural
            // height; clamp to a sane minimum.
            let measured = child.measure(
                cx,
                LayoutConstraints {
                    min_size: Vec2::ZERO,
                    max_size: Vec2::new(col_w, f32::MAX),
                },
            );
            let h = measured.y.max(cx.pt(8.0));
            let x = bounds.min_x() + col as f32 * (col_w + gap);
            let r = Rect::new(x, top, col_w, h);
            cx.layout_child(&mut **child, r);
            self.cell_bounds.push(r);
            heights[col] = top + h + gap;
        }
        self.content_height =
            (heights.iter().copied().fold(0.0_f32, f32::max) - gap - bounds.min_y()).max(0.0);
    }

    fn paint(&self, _cx: &mut PaintContext) {
        // Chrome-free container — children paint themselves.
    }

    fn event(&mut self, cx: &mut EventContext) -> EventResponse {
        if !self.enabled {
            return EventResponse::Ignored;
        }
        if let WidgetEvent::PointerPressed { position, .. }
        | WidgetEvent::PointerReleased { position, .. }
        | WidgetEvent::PointerMoved { position, .. }
        | WidgetEvent::Scroll { position, .. } = cx.event
        {
            let pos = *position;
            for i in (0..self.child_count()).rev() {
                let Some(b) = self.child_bounds(i) else {
                    continue;
                };
                if !b.contains(pos) {
                    continue;
                }
                let mut child_cx = EventContext {
                    event: cx.event,
                    bounds: b,
                    scale: cx.scale,
                };
                if let Some(child) = self.child_mut(i) {
                    return child.event(&mut child_cx);
                }
            }
        }
        EventResponse::Ignored
    }

    fn accessibility(&self, node: &mut AccessKitNode) {
        node.set_role(accesskit::Role::Group);
        if let Some(ref label) = self.label {
            node.set_label(label.as_str());
        }
        if !self.enabled {
            node.set_disabled();
        }
    }

    fn child_count(&self) -> usize {
        self.children.len()
    }

    fn child(&self, index: usize) -> Option<&dyn Widget> {
        self.children.get(index).map(|c| &**c as &dyn Widget)
    }

    fn child_mut(&mut self, index: usize) -> Option<&mut dyn Widget> {
        self.children
            .get_mut(index)
            .map(|c| &mut **c as &mut dyn Widget)
    }

    fn child_bounds(&self, index: usize) -> Option<Rect> {
        self.cell_bounds.get(index).copied()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::widgets::Text;
    use martensite_core::HotNode;

    /// Fixed-size stub so placement is deterministic.
    struct Box2 {
        size: Vec2,
    }
    impl Widget for Box2 {
        fn measure(&mut self, _cx: &mut LayoutContext, _c: LayoutConstraints) -> Vec2 {
            self.size
        }
        fn layout(&mut self, _cx: &mut LayoutContext, _b: Rect) {}
    }

    fn laid_out(m: &mut Masonry, width: f32, h: f32) {
        let mut hot = HotNode::default();
        let mut cx = LayoutContext {
            hot: &mut hot,
            scale: 1.0,
        };
        m.layout(&mut cx, Rect::new(0.0, 0.0, width, h));
    }

    #[test]
    fn empty_layout() {
        let mut m = Masonry::new();
        laid_out(&mut m, 300.0, 200.0);
        assert_eq!(m.child_count(), 0);
        assert_eq!(m.content_height(), 0.0);
    }

    #[test]
    fn fills_shortest_column() {
        let mut m = Masonry::new()
            .columns(2)
            .gap(0.0)
            .child(Box2 {
                size: Vec2::new(100.0, 50.0),
            })
            .child(Box2 {
                size: Vec2::new(100.0, 30.0),
            })
            .child(Box2 {
                size: Vec2::new(100.0, 20.0),
            });
        laid_out(&mut m, 300.0, 400.0);
        let a = m.child_bounds(0).unwrap();
        let b = m.child_bounds(1).unwrap();
        let c = m.child_bounds(2).unwrap();
        // 300px/2 cols = 150 wide.
        assert_eq!(a.width(), 150.0);
        assert_eq!(a.min_y(), 0.0);
        assert_eq!(b.min_y(), 0.0);
        assert!(b.min_x() > a.min_x());
        // Third child lands in column 1 (30 < 50).
        assert_eq!(c.min_x(), b.min_x());
        assert_eq!(c.min_y(), 30.0);
    }

    #[test]
    fn gap_applies() {
        let mut m = Masonry::new()
            .columns(2)
            .gap(10.0)
            .child(Box2 {
                size: Vec2::new(10.0, 40.0),
            })
            .child(Box2 {
                size: Vec2::new(10.0, 40.0),
            });
        laid_out(&mut m, 310.0, 400.0);
        let a = m.child_bounds(0).unwrap();
        let b = m.child_bounds(1).unwrap();
        // (310 - 10)/2 = 150
        assert_eq!(a.width(), 150.0);
        assert_eq!(b.min_x() - a.max_x(), 10.0);
    }

    #[test]
    fn content_height_is_tallest_column() {
        let mut m = Masonry::new()
            .columns(2)
            .gap(10.0)
            .child(Box2 {
                size: Vec2::new(10.0, 100.0),
            })
            .child(Box2 {
                size: Vec2::new(10.0, 40.0),
            })
            .child(Box2 {
                size: Vec2::new(10.0, 60.0),
            });
        laid_out(&mut m, 300.0, 600.0);
        // Col0: 100 + gap; col1: 40+10+60=110.
        assert_eq!(m.content_height(), 110.0);
    }

    #[test]
    fn columns_clamp() {
        assert_eq!(Masonry::new().columns(0).columns, 1);
    }

    #[test]
    fn clear_empties() {
        let mut m = Masonry::new().child(Text::new("a"));
        m.clear();
        assert_eq!(m.child_count(), 0);
    }
}
