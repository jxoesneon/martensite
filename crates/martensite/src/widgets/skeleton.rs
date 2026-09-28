//! `Skeleton` widget: shimmer placeholder shown while content loads
//! (Ant `Skeleton`, SwiftUI `.redacted`, KDE `LoadingPlaceholder`).
//!
//! A `Skeleton` either paints its own placeholder shapes — a block,
//! a circle, a paragraph of text lines, a row stack, or a cell
//! grid — or wraps a child widget and hides it behind the shimmer
//! while `loading` is set.
//!
//! All painting delegates to `martensite_core::loading`, the same
//! shared painter behind the native `Widget::is_loading`/
//! `Widget::paint_loading` protocol, so standalone skeletons and
//! native loading placeholders can never drift visually.
//!
//! # Examples
//!
//! ```
//! use martensite::widgets::skeleton::Skeleton;
//!
//! let s = Skeleton::lines(3);
//! assert!(s.is_loading());
//! ```

use accesskit::Node as AccessKitNode;
use glam::Vec2;
use martensite_core::loading::{paint_skeleton, preferred_size, SWEEP_SECS};
use martensite_core::widget::{LayoutConstraints, LayoutContext, PaintContext, Widget};
use martensite_core::Rect;

pub use martensite_core::loading::SkeletonShape;

/// A loading placeholder that shimmers until `loading` clears.
///
/// Two usage modes:
///
/// - **Standalone**: `Skeleton::block()` / `Skeleton::circle()` /
///   `Skeleton::lines(n)` / `Skeleton::rows(n)` /
///   `Skeleton::grid(c, r)` paint a placeholder of that shape.
/// - **Wrapping**: `Skeleton::lines(3).child(widget)` hides the child
///   behind the shimmer while [`Skeleton::is_loading`] holds; when
///   `set_loading(false)` is called the child becomes visible and
///   interactive, matching Ant Design's `loading` prop.
///
/// The shimmer is a translucent gradient band sweeping left to right,
/// driven by the framework's per-frame `tick`.
///
/// # Examples
///
/// ```
/// use martensite::widgets::skeleton::Skeleton;
///
/// let mut s = Skeleton::lines(2).animated(false);
/// assert!(s.is_loading());
/// s.set_loading(false);
/// assert!(!s.is_loading());
/// ```
pub struct Skeleton {
    /// Placeholder geometry.
    shape: SkeletonShape,
    /// Whether the shimmer is active and the (optional) child hidden.
    loading: bool,
    /// Whether the shimmer band animates (`animated(false)` paints a
    /// static placeholder — useful in tests and reduced-motion).
    animated: bool,
    /// Optional wrapped child revealed when `loading` clears.
    child: Option<Box<dyn Widget>>,
    /// Bounds assigned to the child — the widget's full bounds.
    child_rect: Rect,
    /// Shimmer phase 0.0..=1.0 (band position across the width).
    phase: f32,
    /// Cached bounds from the last layout pass.
    cached_bounds: Rect,
}

impl Skeleton {
    /// A block placeholder filling its bounds.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite::widgets::skeleton::Skeleton;
    ///
    /// let s = Skeleton::block();
    /// ```
    #[must_use]
    pub fn block() -> Self {
        Self::with_shape(SkeletonShape::Block)
    }

    /// A circular placeholder (avatar stand-in).
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite::widgets::skeleton::Skeleton;
    ///
    /// let s = Skeleton::circle();
    /// ```
    #[must_use]
    pub fn circle() -> Self {
        Self::with_shape(SkeletonShape::Circle)
    }

    /// A paragraph placeholder of `n` text lines.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite::widgets::skeleton::Skeleton;
    ///
    /// let s = Skeleton::lines(4);
    /// ```
    #[must_use]
    pub fn lines(n: usize) -> Self {
        Self::with_shape(SkeletonShape::Lines(n.max(1)))
    }

    /// A row-stack placeholder of `n` rows — the shape of a pending
    /// list, table body, or feed.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite::widgets::skeleton::Skeleton;
    ///
    /// let s = Skeleton::rows(5);
    /// ```
    #[must_use]
    pub fn rows(n: usize) -> Self {
        Self::with_shape(SkeletonShape::Rows { count: n.max(1) })
    }

    /// A `cols`×`rows` cell-grid placeholder — the shape of a pending
    /// gallery or icon grid.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite::widgets::skeleton::Skeleton;
    ///
    /// let s = Skeleton::grid(3, 2);
    /// ```
    #[must_use]
    pub fn grid(cols: usize, rows: usize) -> Self {
        Self::with_shape(SkeletonShape::Grid {
            cols: cols.max(1),
            rows: rows.max(1),
        })
    }

    fn with_shape(shape: SkeletonShape) -> Self {
        Self {
            shape,
            loading: true,
            animated: true,
            child: None,
            child_rect: Rect::default(),
            phase: 0.0,
            cached_bounds: Rect::default(),
        }
    }

    /// Wraps a child widget — it stays hidden and non-interactive
    /// until [`Skeleton::set_loading`] clears the flag.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite::widgets::skeleton::Skeleton;
    /// use martensite::widgets::Text;
    ///
    /// let s = Skeleton::block().child(Text::new("content"));
    /// ```
    #[must_use]
    pub fn child(mut self, child: impl Widget + 'static) -> Self {
        self.child = Some(Box::new(child));
        self
    }

    /// Enables or disables the shimmer animation.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite::widgets::skeleton::Skeleton;
    ///
    /// let s = Skeleton::block().animated(false);
    /// ```
    #[must_use]
    pub fn animated(mut self, animated: bool) -> Self {
        self.animated = animated;
        self
    }

    /// Sets the loading flag directly.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite::widgets::skeleton::Skeleton;
    ///
    /// let mut s = Skeleton::block();
    /// s.set_loading(false);
    /// assert!(!s.is_loading());
    /// ```
    pub fn set_loading(&mut self, loading: bool) {
        self.loading = loading;
    }

    /// Whether the placeholder (not the child) is shown.
    #[inline]
    #[must_use]
    pub fn is_loading(&self) -> bool {
        self.loading
    }
}

impl Widget for Skeleton {
    fn measure(&mut self, cx: &mut LayoutContext, constraints: LayoutConstraints) -> Vec2 {
        // A wrapped child owns the size in both states — measuring
        // the child while loading keeps the bounds stable so the
        // reveal never pops layout.
        if let Some(child) = &mut self.child {
            return child.measure(cx, constraints);
        }
        let (pw, ph) = preferred_size(self.shape);
        Vec2::new(
            cx.pt(pw)
                .clamp(constraints.min_size.x, constraints.max_size.x),
            cx.pt(ph)
                .clamp(constraints.min_size.y, constraints.max_size.y),
        )
    }

    fn layout(&mut self, cx: &mut LayoutContext, bounds: Rect) {
        self.cached_bounds = bounds;
        self.child_rect = bounds;
        if let Some(child) = &mut self.child {
            cx.layout_child(child.as_mut(), bounds);
        }
    }

    fn accessibility(&self, node: &mut AccessKitNode) {
        node.set_role(accesskit::Role::GenericContainer);
        if self.loading {
            node.set_label("Loading");
            node.set_busy();
        }
    }

    fn is_loading(&self) -> bool {
        self.loading
    }

    fn paint_loading(&self, cx: &mut PaintContext, phase: Option<f32>) {
        paint_skeleton(cx, cx.bounds, self.shape, phase);
    }

    fn paint(&self, cx: &mut PaintContext) {
        if !self.loading {
            return; // the arena paints the child directly
        }
        let phase = self.animated.then_some(self.phase);
        paint_skeleton(cx, cx.bounds, self.shape, phase);
    }

    fn tick(&mut self, dt: std::time::Duration) -> bool {
        if self.loading && self.animated {
            self.phase = (self.phase + dt.as_secs_f32() / SWEEP_SECS) % 1.0;
            true
        } else {
            false
        }
    }

    // While loading, the child is invisible and must not receive
    // events or emit its own accessibility node — the placeholder is
    // the whole widget. Once `loading` clears the child reappears.
    fn child_count(&self) -> usize {
        usize::from(!self.loading && self.child.is_some())
    }

    fn child(&self, index: usize) -> Option<&dyn Widget> {
        if index == 0 && !self.loading {
            self.child.as_deref()
        } else {
            None
        }
    }

    fn child_mut(&mut self, index: usize) -> Option<&mut dyn Widget> {
        if index == 0 && !self.loading {
            self.child.as_deref_mut()
        } else {
            None
        }
    }

    fn child_bounds(&self, index: usize) -> Option<Rect> {
        if index == 0 && !self.loading && self.child.is_some() {
            Some(self.child_rect)
        } else {
            None
        }
    }
}

impl std::fmt::Debug for Skeleton {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Skeleton")
            .field("shape", &self.shape)
            .field("loading", &self.loading)
            .field("animated", &self.animated)
            .field("has_child", &self.child.is_some())
            .finish()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use martensite_core::{HotNode, Theme};

    fn make_cx(hot: &mut HotNode) -> LayoutContext<'_> {
        LayoutContext { hot, scale: 1.0 }
    }

    #[test]
    fn constructors_and_loading() {
        assert!(Skeleton::block().is_loading());
        assert!(Skeleton::circle().is_loading());
        assert!(Skeleton::lines(3).is_loading());
        assert!(Skeleton::rows(4).is_loading());
        assert!(Skeleton::grid(3, 2).is_loading());
        let mut s = Skeleton::block();
        assert!(Widget::is_loading(&s));
        s.set_loading(false);
        assert!(!s.is_loading());
        assert!(!Widget::is_loading(&s));
    }

    #[test]
    fn lines_clamps_to_one() {
        let s = Skeleton::lines(0);
        assert_eq!(s.shape, SkeletonShape::Lines(1));
    }

    #[test]
    fn child_hidden_while_loading() {
        let mut s = Skeleton::block().child(crate::widgets::Text::new("x"));
        assert_eq!(s.child_count(), 0);
        assert!(Widget::child(&s, 0).is_none());
        s.set_loading(false);
        assert_eq!(s.child_count(), 1);
        assert!(Widget::child(&s, 0).is_some());
    }

    #[test]
    fn wrapped_measure_defers_to_child_while_loading() {
        // Regression: loading measure must not return the
        // placeholder's preferred size while a child is wrapped —
        // that collapses bounds and pops layout on reveal.
        let mut s = Skeleton::block().child(crate::widgets::Text::new("hello"));
        let mut hot = HotNode::default();
        let wrapped = s.measure(
            &mut make_cx(&mut hot),
            LayoutConstraints {
                min_size: Vec2::ZERO,
                max_size: Vec2::new(400.0, 400.0),
            },
        );
        s.set_loading(false);
        let revealed = s.measure(
            &mut make_cx(&mut hot),
            LayoutConstraints {
                min_size: Vec2::ZERO,
                max_size: Vec2::new(400.0, 400.0),
            },
        );
        assert_eq!(wrapped, revealed);
    }

    #[test]
    fn standalone_measure_respects_min_constraint() {
        // Regression for the dead `max_size.x.min(max_size.x)` clamp:
        // a minimum width larger than the preferred size must hold.
        let mut s = Skeleton::block();
        let mut hot = HotNode::default();
        let size = s.measure(
            &mut make_cx(&mut hot),
            LayoutConstraints {
                min_size: Vec2::new(300.0, 0.0),
                max_size: Vec2::new(400.0, 400.0),
            },
        );
        assert_eq!(size.x, 300.0);
    }

    #[test]
    fn shimmer_advances_on_tick() {
        let mut s = Skeleton::block();
        assert!(s.tick(std::time::Duration::from_millis(100)));
        let mut still = Skeleton::block().animated(false);
        assert!(!still.tick(std::time::Duration::from_millis(100)));
    }

    #[test]
    fn paint_emits_placeholder() {
        let mut s = Skeleton::block().animated(false);
        let mut hot = HotNode::default();
        s.layout(&mut make_cx(&mut hot), Rect::new(0.0, 0.0, 100.0, 30.0));
        let mut list = martensite_core::PaintList::new();
        let theme = Theme::new("test");
        let mut cx = PaintContext {
            list: &mut list,
            bounds: Rect::new(0.0, 0.0, 100.0, 30.0),
            theme: &theme,
            scale: 1.0,
            text_painter: None,
        };
        s.paint(&mut cx);
        // Base fill + clip push + pop — at minimum three commands.
        assert!(list.len() >= 3);
    }
}
