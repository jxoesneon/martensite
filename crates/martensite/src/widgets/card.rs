//! `Card` widget: an elevated content surface (M3 Card / Ant Design
//! `Card`).
//!
//! A card is a rounded surface grouping related content: an optional
//! title header, a single content child, and an optional row of
//! [`Button`] actions along the bottom. Three variants mirror the
//! Material 3 card set — [`CardVariant::Elevated`], `Filled`, and
//! `Outlined`.
//!
//! # Examples
//!
//! ```
//! use martensite::widgets::card::{Card, CardVariant};
//! use martensite::widgets::button::Button;
//! use martensite::widgets::text::Text;
//!
//! let card = Card::outlined()
//!     .title("Settings")
//!     .child(Text::new("Body"))
//!     .action(Button::new("Apply"));
//! assert_eq!(card.variant, CardVariant::Outlined);
//! ```

use accesskit::Node as AccessKitNode;
use glam::Vec2;
use martensite_core::shape::Shape;
use martensite_core::widget::{LayoutConstraints, LayoutContext, PaintContext, Widget};
use martensite_core::{Rect, TokenKey};
use martensite_layout::geometry::EdgeInsets;

use crate::widgets::button::Button;

const INK: [u8; 4] = [20, 20, 25, 255];
const EDGE: [u8; 4] = [140, 145, 155, 255];
const SURFACE: [u8; 4] = [248, 249, 251, 255];
/// Faux drop-shadow for the elevated variant — the token dictionary
/// has no general elevation/shadow token (only `CsdShadow*` for window
/// chrome), so the lift cue is a 1.5pt offset translucent edge. When a
/// real `Elevation`/`Shadow` token family lands this should resolve
/// through it.
const SHADOW: [u8; 4] = [0, 0, 0, 48];
/// Title band height in logical points.
const TITLE_H: f32 = 28.0;
/// Vertical gap between the content and the action row, and the
/// horizontal gap between action buttons (logical points).
const ACTION_GAP: f32 = 8.0;
/// Title text size in logical points.
const TITLE_SIZE: f32 = 15.0;

/// Visual variant of a [`Card`], mirroring the Material 3 card set.
///
/// # Examples
///
/// ```
/// use martensite::widgets::card::CardVariant;
///
/// assert_eq!(CardVariant::default(), CardVariant::Elevated);
/// ```
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum CardVariant {
    /// Surface fill lifted by a subtle offset darker edge (the
    /// stand-in for a real shadow — see [`SHADOW`]).
    #[default]
    Elevated,
    /// Flat surface fill, no border and no lift cue.
    Filled,
    /// Surface fill with a 1px outline.
    Outlined,
}

/// An elevated surface grouping a title, a content child, and a row
/// of action [`Button`]s.
///
/// The content child and the action buttons ride the
/// internal-children protocol — the card lays them out, the arena
/// paints and hit-tests them.
///
/// # Examples
///
/// ```
/// use martensite::widgets::{Button, Card};
/// use martensite_core::widget::DummyWidget;
///
/// let card = Card::new()
///     .title("Account")
///     .child(DummyWidget)
///     .action(Button::new("Save"));
/// assert!(card.child.is_some());
/// assert_eq!(card.actions.len(), 1);
/// ```
pub struct Card {
    /// Visual variant.
    pub variant: CardVariant,
    /// Optional title painted in a header band at the top.
    pub title: Option<String>,
    /// Padding between the card edge and the title/content/actions.
    pub padding: EdgeInsets,
    /// The content widget.
    pub child: Option<Box<dyn Widget>>,
    /// Action buttons laid out right-aligned along the bottom.
    pub actions: Vec<Button>,
    /// Cached per-button measured sizes from the last measure pass.
    action_sizes: Vec<Vec2>,
    /// Cached bounds from the last layout pass.
    cached_bounds: Rect,
    /// Cached title band rect (device px).
    title_rect: Rect,
    /// Cached content rect (device px).
    content_rect: Rect,
    /// Internal-child rects in child-protocol order: content first
    /// (when present), then one rect per action button.
    child_rects: Vec<Rect>,
    /// Shared shaped-text painter.
    text_painter: Option<crate::text_paint::SharedTextPainter>,
}

impl Card {
    /// A card with the default ([`CardVariant::Elevated`]) variant.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite::widgets::Card;
    ///
    /// let c = Card::new();
    /// assert!(c.title.is_none());
    /// assert!(c.actions.is_empty());
    /// ```
    pub fn new() -> Self {
        Self {
            variant: CardVariant::default(),
            title: None,
            padding: EdgeInsets::uniform(16.0),
            child: None,
            actions: Vec::new(),
            action_sizes: Vec::new(),
            cached_bounds: Rect::default(),
            title_rect: Rect::default(),
            content_rect: Rect::default(),
            child_rects: Vec::new(),
            text_painter: None,
        }
    }

    /// An elevated card (surface fill + lift cue).
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite::widgets::{Card, CardVariant};
    ///
    /// assert_eq!(Card::elevated().variant, CardVariant::Elevated);
    /// ```
    #[inline]
    #[must_use]
    pub fn elevated() -> Self {
        Self::new().variant(CardVariant::Elevated)
    }

    /// A filled card (flat surface fill).
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite::widgets::{Card, CardVariant};
    ///
    /// assert_eq!(Card::filled().variant, CardVariant::Filled);
    /// ```
    #[inline]
    #[must_use]
    pub fn filled() -> Self {
        Self::new().variant(CardVariant::Filled)
    }

    /// An outlined card (surface fill + 1px outline).
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite::widgets::{Card, CardVariant};
    ///
    /// assert_eq!(Card::outlined().variant, CardVariant::Outlined);
    /// ```
    #[inline]
    #[must_use]
    pub fn outlined() -> Self {
        Self::new().variant(CardVariant::Outlined)
    }

    /// Sets the visual variant.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite::widgets::{Card, CardVariant};
    ///
    /// let c = Card::new().variant(CardVariant::Outlined);
    /// assert_eq!(c.variant, CardVariant::Outlined);
    /// ```
    #[inline]
    #[must_use]
    pub fn variant(mut self, variant: CardVariant) -> Self {
        self.variant = variant;
        self
    }

    /// Sets the title header.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite::widgets::Card;
    ///
    /// let c = Card::new().title("Profile");
    /// assert_eq!(c.title.as_deref(), Some("Profile"));
    /// ```
    #[inline]
    #[must_use]
    pub fn title(mut self, title: impl Into<String>) -> Self {
        self.title = Some(title.into());
        self
    }

    /// Sets the padding inside the card.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite::widgets::Card;
    /// use martensite_layout::geometry::EdgeInsets;
    ///
    /// let c = Card::new().padding(EdgeInsets::uniform(8.0));
    /// assert_eq!(c.padding.left, 8.0);
    /// ```
    #[inline]
    #[must_use]
    pub fn padding(mut self, padding: EdgeInsets) -> Self {
        self.padding = padding;
        self
    }

    /// Sets uniform padding inside the card.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite::widgets::Card;
    ///
    /// let c = Card::new().padding_uniform(12.0);
    /// assert_eq!(c.padding.horizontal(), 24.0);
    /// ```
    #[inline]
    #[must_use]
    pub fn padding_uniform(mut self, value: f32) -> Self {
        self.padding = EdgeInsets::uniform(value);
        self
    }

    /// Sets the content widget.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite::widgets::{Card, Container};
    ///
    /// let c = Card::new().child(Container::new());
    /// assert!(c.child.is_some());
    /// ```
    #[inline]
    #[must_use]
    pub fn child(mut self, child: impl Widget + 'static) -> Self {
        self.child = Some(Box::new(child));
        self
    }

    /// Appends an action [`Button`] to the bottom row.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite::widgets::{Button, Card};
    ///
    /// let c = Card::new().action(Button::new("OK"));
    /// assert_eq!(c.actions.len(), 1);
    /// ```
    #[inline]
    #[must_use]
    pub fn action(mut self, button: Button) -> Self {
        self.actions.push(button);
        self
    }

    /// Appends several action buttons.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite::widgets::{Button, Card};
    ///
    /// let c = Card::new().actions([Button::new("Cancel"), Button::new("OK")]);
    /// assert_eq!(c.actions.len(), 2);
    /// ```
    #[inline]
    #[must_use]
    pub fn actions(mut self, actions: impl IntoIterator<Item = Button>) -> Self {
        self.actions.extend(actions);
        self
    }

    /// Shares a [`crate::text_paint::TextPainter`] for real glyph runs.
    #[must_use]
    pub fn with_text_painter(mut self, painter: crate::text_paint::SharedTextPainter) -> Self {
        self.text_painter = Some(painter);
        self
    }
}

impl Default for Card {
    fn default() -> Self {
        Self::new()
    }
}

impl Widget for Card {
    fn measure(&mut self, cx: &mut LayoutContext, constraints: LayoutConstraints) -> Vec2 {
        let h_inset = self.padding.horizontal();
        let v_inset = self.padding.vertical();
        let title_h = if self.title.is_some() {
            cx.pt(TITLE_H)
        } else {
            0.0
        };

        // Measure the action buttons so the row can be right-aligned
        // during layout.
        self.action_sizes.clear();
        self.action_sizes.reserve(self.actions.len());
        let mut actions_h = 0.0f32;
        let gap = cx.pt(ACTION_GAP);
        for button in &mut self.actions {
            let size = button.measure(
                cx,
                LayoutConstraints {
                    min_size: Vec2::ZERO,
                    max_size: constraints.max_size,
                },
            );
            self.action_sizes.push(size);
            actions_h = actions_h.max(size.y);
        }
        let actions_band = if self.actions.is_empty() {
            0.0
        } else {
            actions_h + gap
        };

        let child_size = if let Some(child) = &mut self.child {
            child.measure(
                cx,
                LayoutConstraints {
                    min_size: Vec2::ZERO,
                    max_size: Vec2::new(
                        (constraints.max_size.x - h_inset).max(0.0),
                        (constraints.max_size.y - v_inset - title_h - actions_band).max(0.0),
                    ),
                },
            )
        } else {
            Vec2::ZERO
        };

        // Cards fill the offered width when it is bounded (like
        // `Banner`); an unbounded width hugs the content.
        let w = if constraints.max_size.x.is_finite() {
            constraints.max_size.x.max(0.0)
        } else {
            child_size.x + h_inset
        };
        let h = v_inset + title_h + child_size.y + actions_band;
        Vec2::new(w, h.min(constraints.max_size.y.max(0.0)))
    }

    fn layout(&mut self, cx: &mut LayoutContext, bounds: Rect) {
        self.cached_bounds = bounds;
        self.child_rects.clear();

        let inner = Rect::new(
            bounds.origin.x + self.padding.left,
            bounds.origin.y + self.padding.top,
            (bounds.size.x - self.padding.horizontal()).max(0.0),
            (bounds.size.y - self.padding.vertical()).max(0.0),
        );

        // Title band at the top of the interior.
        let mut cursor_y = inner.origin.y;
        self.title_rect = if self.title.is_some() {
            let h = cx.pt(TITLE_H).min(inner.size.y);
            let r = Rect::new(inner.origin.x, cursor_y, inner.size.x, h);
            cursor_y += h;
            r
        } else {
            Rect::default()
        };

        // Action row pinned to the interior bottom, right-aligned as a
        // group in declaration order (first button leftmost).
        let gap = cx.pt(ACTION_GAP);
        let actions_h = self.action_sizes.iter().map(|s| s.y).fold(0.0f32, f32::max);
        let row_y = if self.actions.is_empty() {
            inner.max_y()
        } else {
            (inner.max_y() - actions_h).max(cursor_y)
        };
        if !self.actions.is_empty() {
            let total_w: f32 = self.action_sizes.iter().map(|s| s.x).sum::<f32>()
                + gap * self.actions.len().saturating_sub(1) as f32;
            let mut bx = (inner.max_x() - total_w).max(inner.origin.x);
            for (i, button) in self.actions.iter_mut().enumerate() {
                let size = self.action_sizes.get(i).copied().unwrap_or(Vec2::ZERO);
                let r = Rect::new(bx, row_y, size.x.max(0.0), actions_h);
                self.child_rects.push(r);
                cx.layout_child(button, r);
                bx += size.x + gap;
            }
        }

        // Content occupies the band between title and actions.
        let content_bottom = if self.actions.is_empty() {
            inner.max_y()
        } else {
            (row_y - gap).max(cursor_y)
        };
        self.content_rect = Rect::new(
            inner.origin.x,
            cursor_y,
            inner.size.x,
            (content_bottom - cursor_y).max(0.0),
        );
        if let Some(child) = &mut self.child {
            // Content is child index 0 — keep the rects vector in
            // protocol order (content rect before action rects).
            self.child_rects.insert(0, self.content_rect);
            cx.layout_child(child.as_mut(), self.content_rect);
        }
    }

    fn accessibility(&self, node: &mut AccessKitNode) {
        node.set_role(accesskit::Role::Group);
        if let Some(title) = &self.title {
            node.set_label(title.as_str());
        }
    }

    fn paint(&self, cx: &mut PaintContext) {
        let b = cx.bounds;
        let rect = kurbo::Rect::new(
            f64::from(b.origin.x),
            f64::from(b.origin.y),
            f64::from(b.max_x()),
            f64::from(b.max_y()),
        );
        let shape = Shape::rounded(cx.dim(TokenKey::BorderRadiusLarge, 12.0));
        let surface = cx.color(TokenKey::SurfaceColor, SURFACE);

        match self.variant {
            CardVariant::Elevated => {
                // No elevation/shadow token exists yet — fake the lift
                // with a translucent shape offset 1.5pt below the card.
                let drop = cx.ptf(1.5);
                let shadow_rect =
                    kurbo::Rect::new(rect.x0, rect.y0 + drop, rect.x1, rect.y1 + drop);
                cx.list.push_fill_shape(shadow_rect, &shape, SHADOW);
                cx.list.push_fill_shape(rect, &shape, surface);
            }
            CardVariant::Filled => {
                cx.list.push_fill_shape(rect, &shape, surface);
            }
            CardVariant::Outlined => {
                cx.list.push_fill_shape(rect, &shape, surface);
                cx.list.push_stroke_shape(
                    rect,
                    &shape,
                    cx.pt(1.0),
                    cx.color(TokenKey::BorderColor, EDGE),
                );
            }
        }

        // Title header — clipped to the interior so an over-long title
        // can't spill past the rounded chrome.
        if let Some(title) = &self.title {
            let t = &self.title_rect;
            let pad_x = cx.pt(4.0);
            crate::text_paint::paint_label_clipped(
                crate::text_paint::resolve_painter(&self.text_painter, cx.text_painter),
                cx.list,
                kurbo::Rect::new(
                    f64::from(t.origin.x),
                    f64::from(t.origin.y),
                    f64::from(t.max_x() - pad_x),
                    f64::from(t.max_y()),
                ),
                kurbo::Point::new(
                    f64::from(t.origin.x),
                    f64::from(t.origin.y + (t.size.y - cx.pt(TITLE_SIZE)) / 2.0),
                ),
                title,
                cx.pt(TITLE_SIZE),
                cx.color(TokenKey::TextColor, INK),
            );
        }
    }

    fn child_count(&self) -> usize {
        usize::from(self.child.is_some()) + self.actions.len()
    }

    fn child(&self, index: usize) -> Option<&dyn Widget> {
        let content = usize::from(self.child.is_some());
        if index < content {
            return self.child.as_deref();
        }
        self.actions.get(index - content).map(|b| b as &dyn Widget)
    }

    fn child_mut(&mut self, index: usize) -> Option<&mut dyn Widget> {
        let content = usize::from(self.child.is_some());
        if index < content {
            return self.child.as_deref_mut();
        }
        self.actions
            .get_mut(index - content)
            .map(|b| b as &mut dyn Widget)
    }

    fn child_bounds(&self, index: usize) -> Option<Rect> {
        self.child_rects.get(index).copied()
    }
}

impl std::fmt::Debug for Card {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Card")
            .field("variant", &self.variant)
            .field("title", &self.title)
            .field("has_child", &self.child.is_some())
            .field("actions", &self.actions.len())
            .finish()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use martensite_core::widget::DummyWidget;
    use martensite_core::HotNode;

    fn make_cx(hot: &mut HotNode) -> LayoutContext<'_> {
        LayoutContext { hot, scale: 1.0 }
    }

    #[test]
    fn card_new_is_empty() {
        let c = Card::new();
        assert_eq!(c.variant, CardVariant::Elevated);
        assert!(c.title.is_none());
        assert!(c.child.is_none());
        assert!(c.actions.is_empty());
        assert_eq!(c.child_count(), 0);
    }

    #[test]
    fn card_variant_builders() {
        assert_eq!(Card::elevated().variant, CardVariant::Elevated);
        assert_eq!(Card::filled().variant, CardVariant::Filled);
        assert_eq!(Card::outlined().variant, CardVariant::Outlined);
        let c = Card::new().variant(CardVariant::Filled);
        assert_eq!(c.variant, CardVariant::Filled);
    }

    #[test]
    fn card_children_protocol_order() {
        let c = Card::new()
            .child(DummyWidget)
            .action(Button::new("A"))
            .action(Button::new("B"));
        assert_eq!(c.child_count(), 3);
        assert!(Widget::child(&c, 0).is_some());
        assert!(Widget::child(&c, 1).is_some());
        assert!(Widget::child(&c, 2).is_some());
        assert!(Widget::child(&c, 3).is_none());
    }

    #[test]
    fn card_actions_only_shift_indices() {
        // Without a content child the action buttons start at index 0.
        let c = Card::new().action(Button::new("A"));
        assert_eq!(c.child_count(), 1);
        assert!(Widget::child(&c, 0).is_some());
    }

    #[test]
    fn card_measure_with_title_and_actions() {
        let mut hot = HotNode::new(taffy::NodeId::new(1));
        let mut cx = make_cx(&mut hot);
        let mut c = Card::new()
            .title("T")
            .child(DummyWidget)
            .action(Button::new("OK"));
        let size = c.measure(
            &mut cx,
            LayoutConstraints {
                min_size: Vec2::ZERO,
                max_size: Vec2::new(300.0, 400.0),
            },
        );
        assert_eq!(size.x, 300.0);
        // padding*2 + title + content(0) + action row (32 + gap)
        assert!(size.y >= 32.0 + 28.0 + 32.0 + 8.0);
    }

    #[test]
    fn card_layout_positions_regions() {
        let mut hot = HotNode::new(taffy::NodeId::new(1));
        let mut cx = make_cx(&mut hot);
        let mut c = Card::new()
            .padding_uniform(10.0)
            .title("T")
            .child(DummyWidget)
            .action(Button::new("OK"));
        c.measure(
            &mut cx,
            LayoutConstraints {
                min_size: Vec2::ZERO,
                max_size: Vec2::new(300.0, 400.0),
            },
        );
        c.layout(&mut cx, Rect::new(0.0, 0.0, 300.0, 200.0));
        assert_eq!(c.cached_bounds, Rect::new(0.0, 0.0, 300.0, 200.0));
        // Title sits below the top padding.
        assert_eq!(c.title_rect.origin.y, 10.0);
        // Action button is right-aligned at the interior bottom.
        let action_rect = c.child_bounds(1).unwrap();
        assert!(action_rect.max_x() <= 290.0);
        assert!(action_rect.origin.x > 150.0);
        // Content rect spans between title and the action row.
        let content = c.child_bounds(0).unwrap();
        assert!(content.origin.y >= c.title_rect.max_y());
        assert!(content.max_y() <= action_rect.origin.y);
    }

    #[test]
    fn card_a11y_group_with_label() {
        let c = Card::new().title("Details");
        let mut node = accesskit::Node::new(accesskit::Role::Unknown);
        c.accessibility(&mut node);
        assert_eq!(node.role(), accesskit::Role::Group);
        assert_eq!(node.label(), Some("Details"));
    }

    #[test]
    fn card_debug_format() {
        let c = Card::outlined().title("X");
        let debug = format!("{:?}", c);
        assert!(debug.contains("Card"));
        assert!(debug.contains("Outlined"));
    }
}
