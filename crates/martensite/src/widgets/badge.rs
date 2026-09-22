//! `Badge` widget: a small notification pill — a count, a capped
//! `99+` count, or a bare dot — painted standalone or anchored to the
//! top-right corner of a wrapped child.
//!
//! `Badge::new(count)` is a self-sized widget; `Badge::wrap(child)`
//! embeds a child and overlays the badge centered on the child's
//! top-right corner, the Material/most-design-systems convention.
//!
//! # Examples
//!
//! ```
//! use martensite::widgets::badge::Badge;
//! use martensite::widgets::Text;
//!
//! let b = Badge::new(5);
//! assert_eq!(b.text(), "5");
//!
//! // Bare dot on a wrapped child.
//! let w = Badge::wrap(Text::new("inbox")).dot(true);
//! assert!(w.dot);
//! ```

use accesskit::Node as AccessKitNode;
use glam::Vec2;
use martensite_core::shape::Shape;
use martensite_core::widget::{LayoutConstraints, LayoutContext, PaintContext, Widget};
use martensite_core::{Rect, TokenKey};

const ERROR: [u8; 4] = [220, 50, 47, 255];
const INVERSE_INK: [u8; 4] = [255, 255, 255, 255];
const SURFACE: [u8; 4] = [30, 30, 34, 255];
/// Pill height for the count form, logical points.
const PILL_H: f32 = 18.0;
/// Horizontal padding inside the count pill, logical points.
const PILL_PAD: f32 = 6.0;
/// Bare-dot diameter, logical points.
const DOT_D: f32 = 8.0;
/// Count text size, logical points.
const TEXT_PT: f32 = 11.0;

/// A notification badge — a count pill, a `99+`-style capped count, or
/// a bare dot.
///
/// `count == 0` (or [`Badge::dot`]) draws the dot form; otherwise the
/// badge is a [`Shape::PILL`] carrying the count, clamped at
/// [`Badge::max`] (`{max}+`). With [`Badge::wrap`] the badge anchors to
/// the wrapped child's top-right corner; standalone it sizes to its
/// own pill.
///
/// # Examples
///
/// ```
/// use martensite::widgets::Badge;
///
/// let b = Badge::new(120).max(99);
/// assert_eq!(b.text(), "99+");
///
/// let dot = Badge::new(0);
/// assert!(dot.is_dot());
/// ```
pub struct Badge {
    /// The displayed count; `0` draws a bare dot.
    pub count: u32,
    /// Counts above `max` render as `{max}+` (default `99`).
    pub max: u32,
    /// Force the bare-dot presentation regardless of `count`.
    pub dot: bool,
    /// Wrapped child the badge anchors to (`Badge::wrap`).
    child: Option<Box<dyn Widget>>,
    /// Child bounds from the last layout (wrap form only).
    child_rect: Rect,
    /// Badge rect from the last layout — the anchor point for `wrap`,
    /// the widget bounds when standalone.
    badge_rect: Rect,
    /// Shared shaped-text painter (count label).
    text_painter: Option<crate::text_paint::SharedTextPainter>,
}

impl Badge {
    /// Creates a standalone badge showing `count` (a bare dot for `0`).
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite::widgets::Badge;
    ///
    /// let b = Badge::new(3);
    /// assert_eq!(b.count, 3);
    /// assert_eq!(b.text(), "3");
    /// ```
    pub fn new(count: u32) -> Self {
        Self {
            count,
            max: 99,
            dot: false,
            child: None,
            child_rect: Rect::default(),
            badge_rect: Rect::default(),
            text_painter: None,
        }
    }

    /// Creates a badge wrapping `child`, anchored to the child's
    /// top-right corner. Starts as a bare dot; set
    /// [`count`](Badge::count) directly or via [`Badge::with_count`]
    /// for the pill form.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite::widgets::{Badge, Text};
    /// use martensite_core::Widget;
    ///
    /// let b = Badge::wrap(Text::new("inbox")).with_count(4);
    /// assert_eq!(b.child_count(), 1);
    /// assert_eq!(b.text(), "4");
    /// ```
    pub fn wrap(child: impl Widget + 'static) -> Self {
        Self {
            child: Some(Box::new(child)),
            ..Self::new(0)
        }
    }

    /// Sets the count — convenience for the `Badge::wrap` form.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite::widgets::{Badge, Text};
    ///
    /// let b = Badge::wrap(Text::new("inbox")).with_count(7);
    /// assert_eq!(b.count, 7);
    /// ```
    #[inline]
    #[must_use]
    pub fn with_count(mut self, count: u32) -> Self {
        self.count = count;
        self
    }

    /// Sets the cap above which the count renders as `{max}+`.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite::widgets::Badge;
    ///
    /// let b = Badge::new(1000).max(99);
    /// assert_eq!(b.text(), "99+");
    /// ```
    #[inline]
    #[must_use]
    pub fn max(mut self, max: u32) -> Self {
        self.max = max;
        self
    }

    /// Forces the bare-dot presentation on or off regardless of
    /// [`Badge::count`].
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite::widgets::Badge;
    ///
    /// let b = Badge::new(12).dot(true);
    /// assert!(b.is_dot());
    /// ```
    #[inline]
    #[must_use]
    pub fn dot(mut self, dot: bool) -> Self {
        self.dot = dot;
        self
    }

    /// Shares a [`crate::text_paint::TextPainter`] for real glyph runs
    /// in the count label.
    #[must_use]
    pub fn with_text_painter(mut self, painter: crate::text_paint::SharedTextPainter) -> Self {
        self.text_painter = Some(painter);
        self
    }

    /// `true` when the badge paints the bare dot (dot forced or
    /// `count == 0`).
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite::widgets::Badge;
    ///
    /// assert!(Badge::new(0).is_dot());
    /// assert!(!Badge::new(1).is_dot());
    /// ```
    pub fn is_dot(&self) -> bool {
        self.dot || self.count == 0
    }

    /// The displayed count text — `{count}`, or `{max}+` when capped.
    /// Meaningful only for the pill form ([`Badge::is_dot`] is `false`).
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite::widgets::Badge;
    ///
    /// assert_eq!(Badge::new(42).text(), "42");
    /// assert_eq!(Badge::new(120).text(), "99+");
    /// ```
    pub fn text(&self) -> String {
        if self.count > self.max {
            format!("{}+", self.max)
        } else {
            self.count.to_string()
        }
    }

    /// Pill/dot size in logical points for the current state.
    fn badge_size(&self) -> Vec2 {
        if self.is_dot() {
            Vec2::splat(DOT_D)
        } else {
            // Estimate the pill width from the label; the painter's
            // exact measure is preferred at paint time but measure must
            // stand alone.
            let chars = self.text().chars().count() as f32;
            let w = (chars * TEXT_PT * 0.6 + PILL_PAD * 2.0).max(PILL_H);
            Vec2::new(w, PILL_H)
        }
    }
}

impl Widget for Badge {
    fn measure(&mut self, cx: &mut LayoutContext, constraints: LayoutConstraints) -> Vec2 {
        if let Some(child) = &mut self.child {
            // Wrap form: the badge overlays the child, which keeps its
            // own intrinsic size.
            return child.measure(cx, constraints);
        }
        let size = self.badge_size() * cx.scale;
        Vec2::new(
            size.x.min(constraints.max_size.x.max(0.0)),
            size.y.min(constraints.max_size.y.max(0.0)),
        )
    }

    fn layout(&mut self, cx: &mut LayoutContext, bounds: Rect) {
        if let Some(child) = &mut self.child {
            self.child_rect = bounds;
            cx.layout_child(child.as_mut(), bounds);
            // Anchor: the badge center sits on the child's top-right
            // corner (Material convention) — it intentionally straddles
            // the corner, painting half over the child, half outside.
            let bs = self.badge_size() * cx.scale;
            self.badge_rect = Rect::new(
                bounds.max_x() - bs.x * 0.5,
                bounds.origin.y - bs.y * 0.5,
                bs.x,
                bs.y,
            );
        } else {
            self.badge_rect = bounds;
        }
    }

    fn paint_extent(&self) -> Option<Rect> {
        self.child.as_ref()?;
        // The bubble straddles the child's top-right corner by design —
        // the scope bounds must include it or the audit reports the
        // count as leaking past the container.
        let (a, b) = (self.child_rect, self.badge_rect);
        Some(Rect::new(
            a.min_x().min(b.min_x()),
            a.min_y().min(b.min_y()),
            a.max_x().max(b.max_x()) - a.min_x().min(b.min_x()),
            a.max_y().max(b.max_y()) - a.min_y().min(b.min_y()),
        ))
    }

    fn accessibility(&self, node: &mut AccessKitNode) {
        node.set_role(accesskit::Role::Status);
        if self.is_dot() {
            node.set_label("unread");
        } else {
            node.set_label(self.text());
        }
    }

    fn paint(&self, cx: &mut PaintContext) {
        let r = self.badge_rect;
        if r.size.x <= 0.0 || r.size.y <= 0.0 {
            return;
        }
        let rect = kurbo::Rect::new(
            f64::from(r.origin.x),
            f64::from(r.origin.y),
            f64::from(r.max_x()),
            f64::from(r.max_y()),
        );
        let bg = cx.color(TokenKey::ErrorColor, ERROR);
        if self.is_dot() {
            cx.list.push_fill_shape(rect, &Shape::ELLIPSE, bg);
            cx.list.push_stroke_shape(
                rect,
                &Shape::ELLIPSE,
                cx.pt(1.0),
                cx.color(TokenKey::SurfaceColor, SURFACE),
            );
            return;
        }
        cx.list.push_fill_shape(rect, &Shape::PILL, bg);
        // Surface-colored hairline separates the badge from whatever
        // the wrapped child painted underneath.
        cx.list.push_stroke_shape(
            rect,
            &Shape::PILL,
            cx.pt(1.0),
            cx.color(TokenKey::SurfaceColor, SURFACE),
        );
        let text = self.text();
        let size_px = cx.pt(TEXT_PT);
        let painter = crate::text_paint::resolve_painter(&self.text_painter, cx.text_painter);
        let w = painter
            .and_then(|p| p.measure_text(&text, size_px))
            .unwrap_or_else(|| text.chars().count() as f32 * size_px * 0.6);
        crate::text_paint::paint_label_clipped(
            painter,
            cx.list,
            rect,
            kurbo::Point::new(
                f64::from(r.origin.x + (r.size.x - w) * 0.5),
                f64::from(r.origin.y + (r.size.y - size_px) * 0.5),
            ),
            &text,
            size_px,
            cx.color(TokenKey::TextInverseColor, INVERSE_INK),
        );
    }

    fn child_count(&self) -> usize {
        usize::from(self.child.is_some())
    }

    fn child(&self, index: usize) -> Option<&dyn Widget> {
        if index == 0 {
            self.child.as_deref()
        } else {
            None
        }
    }

    fn child_mut(&mut self, index: usize) -> Option<&mut dyn Widget> {
        if index == 0 {
            self.child.as_deref_mut()
        } else {
            None
        }
    }

    fn child_bounds(&self, index: usize) -> Option<Rect> {
        if index == 0 && self.child.is_some() {
            Some(self.child_rect)
        } else {
            None
        }
    }
}

impl std::fmt::Debug for Badge {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Badge")
            .field("count", &self.count)
            .field("max", &self.max)
            .field("dot", &self.dot)
            .field("wrapped", &self.child.is_some())
            .finish()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use martensite_core::{HotNode, PaintCommand, PaintList, Theme};

    /// The corner-anchored bubble legitimately overhangs the widget
    /// bounds — `paint_extent` must declare it so the audit's
    /// container-overflow check doesn't flag the count.
    #[test]
    fn badge_overhang_is_declared_paint_extent() {
        let mut arena = martensite_core::WidgetArena::new();
        let mut hot = HotNode::default();
        hot.flags |= martensite_core::NodeFlags::VISIBLE;
        let id = arena.insert_with_widget(
            hot,
            Box::new(Badge::wrap(crate::widgets::text::Text::new("host")).with_count(7)),
        );
        if let Some((hot, cold)) = arena.get_both_mut(id) {
            hot.bounds = Rect::new(0.0, 0.0, 60.0, 20.0);
            cold.widget.layout(
                &mut LayoutContext { hot, scale: 1.0 },
                Rect::new(0.0, 0.0, 60.0, 20.0),
            );
        }
        let mut list = PaintList::new();
        arena.build_paint_list(id, &mut list);
        // The Badge scope's bounds must reach the bubble's right edge.
        let scope = list.commands.iter().find_map(|c| match c {
            PaintCommand::PushScope { name, bounds, .. } if name.contains("Badge") => Some(*bounds),
            _ => None,
        });
        let b = scope.expect("badge scope");
        assert!(b.x1 > 60.0, "badge scope must include the overhang: {b:?}");
    }

    #[test]
    fn badge_text_caps_at_max() {
        assert_eq!(Badge::new(5).text(), "5");
        assert_eq!(Badge::new(99).text(), "99");
        assert_eq!(Badge::new(100).text(), "99+");
        assert_eq!(Badge::new(1000).max(9).text(), "9+");
    }

    #[test]
    fn badge_zero_or_forced_dot() {
        assert!(Badge::new(0).is_dot());
        assert!(!Badge::new(1).is_dot());
        assert!(Badge::new(9).dot(true).is_dot());
    }

    #[test]
    fn badge_standalone_measures_to_pill() {
        let mut hot = HotNode::default();
        let mut cx = LayoutContext {
            hot: &mut hot,
            scale: 1.0,
        };
        let mut dot = Badge::new(0);
        let size = dot.measure(
            &mut cx,
            LayoutConstraints {
                min_size: Vec2::ZERO,
                max_size: Vec2::new(1000.0, 1000.0),
            },
        );
        assert_eq!(size, Vec2::splat(DOT_D));

        let mut pill = Badge::new(5);
        let size = pill.measure(
            &mut cx,
            LayoutConstraints {
                min_size: Vec2::ZERO,
                max_size: Vec2::new(1000.0, 1000.0),
            },
        );
        assert_eq!(size.y, PILL_H);
        assert!(size.x >= PILL_H);
    }

    #[test]
    fn badge_dot_paints_ellipse() {
        let mut list = PaintList::new();
        let theme = Theme::new("test");
        let mut b = Badge::new(0);
        {
            let mut hot = HotNode::default();
            let mut lcx = LayoutContext {
                hot: &mut hot,
                scale: 1.0,
            };
            b.layout(&mut lcx, Rect::new(10.0, 10.0, DOT_D, DOT_D));
        }
        {
            let mut cx = PaintContext {
                list: &mut list,
                bounds: Rect::new(10.0, 10.0, DOT_D, DOT_D),
                theme: &theme,
                scale: 1.0,
                text_painter: None,
            };
            b.paint(&mut cx);
        }
        assert_eq!(list.len(), 2);
        assert!(matches!(list.commands[0], PaintCommand::FillPath(..)));
        assert!(matches!(list.commands[1], PaintCommand::StrokePath(..)));
    }

    #[test]
    fn badge_count_paints_pill_and_label() {
        let mut list = PaintList::new();
        let theme = Theme::new("test");
        let mut b = Badge::new(5);
        {
            let mut hot = HotNode::default();
            let mut lcx = LayoutContext {
                hot: &mut hot,
                scale: 1.0,
            };
            b.layout(&mut lcx, Rect::new(0.0, 0.0, 24.0, PILL_H));
        }
        {
            let mut cx = PaintContext {
                list: &mut list,
                bounds: Rect::new(0.0, 0.0, 24.0, PILL_H),
                theme: &theme,
                scale: 1.0,
                text_painter: None,
            };
            b.paint(&mut cx);
        }
        // Pill fill + hairline + clipped label (clip, text, pop).
        assert_eq!(list.len(), 5);
        assert!(matches!(list.commands[0], PaintCommand::FillPath(..)));
        assert!(matches!(list.commands[1], PaintCommand::StrokePath(..)));
        assert!(matches!(list.commands[2], PaintCommand::ClipRect(_)));
        assert!(matches!(list.commands[3], PaintCommand::DrawText(..)));
        assert!(matches!(list.commands[4], PaintCommand::PopClip));
    }

    #[test]
    fn badge_wrap_anchors_top_right() {
        let mut b = Badge::wrap(crate::widgets::text::Text::new("inbox")).with_count(4);
        let mut hot = HotNode::default();
        let mut cx = LayoutContext {
            hot: &mut hot,
            scale: 1.0,
        };
        let bounds = Rect::new(100.0, 100.0, 200.0, 40.0);
        b.layout(&mut cx, bounds);
        assert_eq!(b.child_count(), 1);
        assert_eq!(b.child_bounds(0), Some(bounds));
        // Centered on the child's top-right corner: center ==
        // (child.max_x, child.min_y).
        let r = b.badge_rect;
        assert!((r.origin.x + r.size.x * 0.5 - bounds.max_x()).abs() < 0.01);
        assert!((r.origin.y + r.size.y * 0.5 - bounds.min_y()).abs() < 0.01);
    }

    #[test]
    fn badge_a11y_status() {
        let b = Badge::new(5);
        let mut node = accesskit::Node::new(accesskit::Role::Unknown);
        b.accessibility(&mut node);
        assert_eq!(node.role(), accesskit::Role::Status);
        assert_eq!(node.label(), Some("5"));

        let dot = Badge::new(0);
        let mut node = accesskit::Node::new(accesskit::Role::Unknown);
        dot.accessibility(&mut node);
        assert_eq!(node.label(), Some("unread"));
    }
}
