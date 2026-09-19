//! `Skeleton` widget: shimmer placeholder shown while content loads
//! (Ant `Skeleton`, SwiftUI `.redacted`, KDE `LoadingPlaceholder`).
//!
//! A `Skeleton` either paints its own placeholder shapes — a block,
//! a circle, or a paragraph of text lines — or wraps a child widget
//! and hides it behind the shimmer while `loading` is set.
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
use kurbo::Shape as _;
use martensite_core::widget::{LayoutConstraints, LayoutContext, PaintContext, Widget};
use martensite_core::{GradientStop, GradientStops, Rect, TokenKey};

/// Base placeholder colour.
const BASE: [u8; 4] = [222, 225, 231, 255];
/// Shimmer highlight colour — translucent white band sweeping across.
const SHIMMER: [u8; 4] = [255, 255, 255, 150];
/// Width of the shimmer band as a fraction of the placeholder width.
const BAND_FRAC: f32 = 0.45;
/// Line height of a text-line placeholder, logical points.
const LINE_PT: f32 = 12.0;
/// Gap between text lines, logical points.
const LINE_GAP_PT: f32 = 8.0;
/// Corner radius for block/line placeholders, logical points.
const RADIUS: f64 = 4.0;
/// Seconds per shimmer sweep.
const SWEEP_SECS: f32 = 1.4;

/// Which placeholder geometry a [`Skeleton`] paints while loading.
///
/// # Examples
///
/// ```
/// use martensite::widgets::skeleton::SkeletonShape;
///
/// assert_ne!(SkeletonShape::Block, SkeletonShape::Circle);
/// ```
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SkeletonShape {
    /// A single rounded block filling the bounds.
    Block,
    /// A circle centred in the bounds (avatar placeholders).
    Circle,
    /// A paragraph of `n` text lines; the last line is shorter,
    /// matching real paragraph raggedness.
    Lines(usize),
}

/// A loading placeholder that shimmers until `loading` clears.
///
/// Two usage modes:
///
/// - **Standalone**: `Skeleton::block()` / `Skeleton::circle()` /
///   `Skeleton::lines(n)` paint a placeholder of that shape.
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

    /// Paints one placeholder rect (or circle) at `rect`, including
    /// the shimmer band when animated.
    fn paint_placeholder(&self, cx: &mut PaintContext, rect: Rect, circle: bool) {
        let kr = kurbo::Rect::new(
            f64::from(rect.min_x()),
            f64::from(rect.min_y()),
            f64::from(rect.max_x()),
            f64::from(rect.max_y()),
        );
        let base = cx.color(TokenKey::SurfaceColor, BASE);
        if circle {
            let ellipse = kurbo::Ellipse::from_rect(kr).into_path(0.1);
            cx.list.push_path(ellipse.clone(), base);
            cx.list.push_clip_path(ellipse);
        } else {
            let rounded = kurbo::RoundedRect::from_rect(kr, cx.ptf(RADIUS)).into_path(0.1);
            cx.list.push_path(rounded.clone(), base);
            cx.list.push_clip_path(rounded);
        }

        if self.animated {
            // A translucent light band sweeping left → right; it
            // travels one band-width past each edge so the sweep fully
            // clears the placeholder at both ends.
            let band = rect.size.x * BAND_FRAC;
            let x = rect.origin.x - band + self.phase * (rect.size.x + band);
            let stops = GradientStops::from_slice(&[
                GradientStop::new(0.0, [SHIMMER[0], SHIMMER[1], SHIMMER[2], 0]),
                GradientStop::new(0.5, SHIMMER),
                GradientStop::new(1.0, [SHIMMER[0], SHIMMER[1], SHIMMER[2], 0]),
            ]);
            cx.list.push_linear_gradient(
                kurbo::Rect::new(
                    f64::from(x),
                    f64::from(rect.min_y()),
                    f64::from(x + band),
                    f64::from(rect.max_y()),
                ),
                stops,
                [f64::from(x), f64::from(rect.origin.y)],
                [f64::from(x + band), f64::from(rect.origin.y)],
            );
        }
        cx.list.pop_clip();
    }
}

impl Widget for Skeleton {
    fn measure(&mut self, cx: &mut LayoutContext, constraints: LayoutConstraints) -> Vec2 {
        let preferred = match self.shape {
            SkeletonShape::Block => Vec2::new(120.0, 24.0),
            SkeletonShape::Circle => Vec2::new(40.0, 40.0),
            SkeletonShape::Lines(n) => {
                Vec2::new(200.0, n as f32 * (LINE_PT + LINE_GAP_PT) - LINE_GAP_PT)
            }
        };
        let size = Vec2::new(
            constraints
                .max_size
                .x
                .min(constraints.max_size.x)
                .max(cx.pt(preferred.x)),
            cx.pt(preferred.y).min(constraints.max_size.y.max(0.0)),
        );
        // When not loading with a child, defer to the child's size.
        if !self.loading {
            if let Some(child) = &mut self.child {
                return child.measure(cx, constraints);
            }
        }
        size
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

    fn paint(&self, cx: &mut PaintContext) {
        if !self.loading {
            return; // the arena paints the child directly
        }
        let b = cx.bounds;
        if b.size.x <= 0.0 || b.size.y <= 0.0 {
            return;
        }
        match self.shape {
            SkeletonShape::Block => self.paint_placeholder(cx, b, false),
            SkeletonShape::Circle => {
                let side = b.size.x.min(b.size.y);
                let r = Rect::new(
                    b.origin.x + (b.size.x - side) / 2.0,
                    b.origin.y + (b.size.y - side) / 2.0,
                    side,
                    side,
                );
                self.paint_placeholder(cx, r, true);
            }
            SkeletonShape::Lines(n) => {
                let line_h = cx.pt(LINE_PT);
                let gap = cx.pt(LINE_GAP_PT);
                let mut y = b.origin.y;
                for i in 0..n {
                    if y + line_h > b.max_y() {
                        break;
                    }
                    // Ragged paragraph edge — the last line runs short.
                    let w = if i == n - 1 {
                        b.size.x * 0.62
                    } else {
                        b.size.x
                    };
                    self.paint_placeholder(cx, Rect::new(b.origin.x, y, w, line_h), false);
                    y += line_h + gap;
                }
            }
        }
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
        let mut s = Skeleton::block();
        s.set_loading(false);
        assert!(!s.is_loading());
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
