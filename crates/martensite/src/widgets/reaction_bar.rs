//! `ReactionBar` — a row of emoji reaction chips (Slack/Teams
//! idiom): emoji + count per chip, an accent ring for reactions
//! the user made, and an optional `+` add chip.
//!
//! Clicking a chip toggles the user's reaction — the count and
//! `mine` flag update locally and the index parks in
//! [`ReactionBar::take_toggled`] for the host to sync. The `+`
//! chip parks [`ReactionBar::take_add`]. Pairs with
//! [`MessageList`](crate::widgets::MessageList).
//!
//! # Examples
//!
//! ```
//! use martensite::widgets::reaction_bar::{Reaction, ReactionBar};
//!
//! let r = ReactionBar::new()
//!     .reaction(Reaction::new("👍", 3))
//!     .reaction(Reaction::new("🎉", 1));
//! assert_eq!(r.reaction_count(), 2);
//! ```

use accesskit::Node as AccessKitNode;
use glam::Vec2;
use martensite_core::{
    EventContext, EventResponse, LayoutConstraints, LayoutContext, PaintContext, PointerButton,
    Rect, RenderMinimum, UnderflowPolicy, Widget, WidgetEvent,
};
use martensite_theme::TokenKey;

use crate::text_paint::SharedTextPainter;

const FONT_PT: f32 = 12.0;
const CHIP_H_PT: f32 = 24.0;
const CHIP_PAD_PT: f32 = 10.0;
const CHIP_GAP_PT: f32 = 6.0;
const RADIUS_PT: f32 = 12.0;

const FACE: [u8; 4] = [36, 38, 44, 255];
const MINE_FACE: [u8; 4] = [44, 62, 92, 255];
const ACCENT: [u8; 4] = [88, 130, 247, 255];
const EDGE: [u8; 4] = [70, 72, 80, 255];
const TEXT: [u8; 4] = [220, 222, 228, 255];
const MUTED: [u8; 4] = [139, 148, 158, 255];

/// One reaction chip — emoji, count, and whether the user reacted.
///
/// ```
/// use martensite::widgets::reaction_bar::Reaction;
///
/// let r = Reaction::new("👍", 4);
/// assert_eq!(r.count, 4);
/// ```
#[derive(Clone, Debug, PartialEq)]
pub struct Reaction {
    /// Emoji glyph.
    pub emoji: String,
    /// Total reactor count (includes the user when `mine`).
    pub count: u32,
    /// Whether the current user reacted.
    pub mine: bool,
}

impl Reaction {
    /// New reaction chip; the user has not reacted.
    ///
    /// ```
    /// use martensite::widgets::reaction_bar::Reaction;
    ///
    /// assert!(!Reaction::new("🎉", 2).mine);
    /// ```
    pub fn new(emoji: impl Into<String>, count: u32) -> Self {
        Self {
            emoji: emoji.into(),
            count,
            mine: false,
        }
    }

    /// Marks that the user reacted.
    ///
    /// ```
    /// use martensite::widgets::reaction_bar::Reaction;
    ///
    /// assert!(Reaction::new("👍", 1).mine(true).mine);
    /// ```
    pub fn mine(mut self, mine: bool) -> Self {
        self.mine = mine;
        self
    }
}

/// A row of reaction chips — see the module docs.
///
/// ```
/// use martensite::widgets::reaction_bar::ReactionBar;
///
/// assert_eq!(ReactionBar::new().reaction_count(), 0);
/// ```
pub struct ReactionBar {
    /// Accessibility label.
    pub label: String,
    /// Show the `+` add chip.
    pub addable: bool,
    reactions: Vec<Reaction>,
    rects: Vec<Rect>,
    add_rect: Rect,
    toggled: Option<usize>,
    add: bool,
    held: Option<usize>,
    held_add: bool,
    text_painter: Option<SharedTextPainter>,
    bounds: Rect,
    scale: f32,
}

impl std::fmt::Debug for ReactionBar {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ReactionBar")
            .field("reactions", &self.reactions)
            .finish()
    }
}

impl Default for ReactionBar {
    fn default() -> Self {
        Self::new()
    }
}

impl ReactionBar {
    /// Empty bar.
    ///
    /// ```
    /// use martensite::widgets::reaction_bar::ReactionBar;
    ///
    /// assert_eq!(ReactionBar::new().reaction_count(), 0);
    /// ```
    pub fn new() -> Self {
        Self {
            label: "Reactions".to_string(),
            addable: true,
            reactions: Vec::new(),
            rects: Vec::new(),
            add_rect: Rect::new(0.0, 0.0, 0.0, 0.0),
            toggled: None,
            add: false,
            held: None,
            held_add: false,
            text_painter: None,
            bounds: Rect::new(0.0, 0.0, 0.0, 0.0),
            scale: 1.0,
        }
    }

    /// Appends a reaction chip.
    ///
    /// ```
    /// use martensite::widgets::reaction_bar::{Reaction, ReactionBar};
    ///
    /// assert_eq!(ReactionBar::new().reaction(Reaction::new("👍", 1)).reaction_count(), 1);
    /// ```
    pub fn reaction(mut self, reaction: Reaction) -> Self {
        self.reactions.push(reaction);
        self
    }

    /// Accessibility label.
    ///
    /// ```
    /// use martensite::widgets::reaction_bar::ReactionBar;
    ///
    /// assert_eq!(ReactionBar::new().label("Msg").label, "Msg");
    /// ```
    pub fn label(mut self, label: impl Into<String>) -> Self {
        self.label = label.into();
        self
    }

    /// Add-chip builder.
    ///
    /// ```
    /// use martensite::widgets::reaction_bar::ReactionBar;
    ///
    /// assert!(!ReactionBar::new().addable(false).addable);
    /// ```
    pub fn addable(mut self, addable: bool) -> Self {
        self.addable = addable;
        self
    }

    /// Shared text painter for real glyph metrics.
    ///
    /// ```no_run
    /// use martensite::widgets::reaction_bar::ReactionBar;
    /// # fn example(p: martensite::text_paint::SharedTextPainter) {
    /// let _r = ReactionBar::new().with_text_painter(p);
    /// # }
    /// ```
    pub fn with_text_painter(mut self, painter: SharedTextPainter) -> Self {
        self.text_painter = Some(painter);
        self
    }

    /// Replaces the chip list.
    ///
    /// ```
    /// use martensite::widgets::reaction_bar::{Reaction, ReactionBar};
    ///
    /// let mut r = ReactionBar::new();
    /// r.set_reactions(vec![Reaction::new("👍", 2)]);
    /// assert_eq!(r.reaction_count(), 1);
    /// ```
    pub fn set_reactions(&mut self, reactions: Vec<Reaction>) {
        self.reactions = reactions;
    }

    /// Number of chips.
    ///
    /// ```
    /// use martensite::widgets::reaction_bar::ReactionBar;
    ///
    /// assert_eq!(ReactionBar::new().reaction_count(), 0);
    /// ```
    pub fn reaction_count(&self) -> usize {
        self.reactions.len()
    }

    /// A chip by index.
    ///
    /// ```
    /// use martensite::widgets::reaction_bar::{Reaction, ReactionBar};
    ///
    /// let r = ReactionBar::new().reaction(Reaction::new("👍", 3));
    /// assert_eq!(r.reaction_at(0).unwrap().count, 3);
    /// ```
    pub fn reaction_at(&self, index: usize) -> Option<&Reaction> {
        self.reactions.get(index)
    }

    /// Applies a reaction update from the host (e.g. server echo).
    ///
    /// ```
    /// use martensite::widgets::reaction_bar::{Reaction, ReactionBar};
    ///
    /// let mut r = ReactionBar::new().reaction(Reaction::new("👍", 1));
    /// r.set_reaction(0, Reaction::new("👍", 5).mine(true));
    /// assert_eq!(r.reaction_at(0).unwrap().count, 5);
    /// ```
    pub fn set_reaction(&mut self, index: usize, reaction: Reaction) {
        if let Some(r) = self.reactions.get_mut(index) {
            *r = reaction;
        }
    }

    /// Drains the last toggled chip index.
    ///
    /// ```
    /// use martensite::widgets::reaction_bar::ReactionBar;
    ///
    /// let mut r = ReactionBar::new();
    /// assert_eq!(r.take_toggled(), None);
    /// ```
    pub fn take_toggled(&mut self) -> Option<usize> {
        self.toggled.take()
    }

    /// Drains an add-chip press.
    ///
    /// ```
    /// use martensite::widgets::reaction_bar::ReactionBar;
    ///
    /// let mut r = ReactionBar::new();
    /// assert!(!r.take_add());
    /// ```
    pub fn take_add(&mut self) -> bool {
        std::mem::take(&mut self.add)
    }

    /// Chip hit-test.
    fn chip_at(&self, p: Vec2) -> Option<usize> {
        self.rects.iter().position(|r| r.contains(p))
    }

    /// Flips a chip's `mine` + count locally.
    fn toggle(&mut self, index: usize) {
        if let Some(r) = self.reactions.get_mut(index) {
            r.mine = !r.mine;
            r.count = if r.mine {
                r.count.saturating_add(1)
            } else {
                r.count.saturating_sub(1)
            };
            self.toggled = Some(index);
        }
    }

    /// Chip content width (emoji + count).
    fn chip_width(&self, r: &Reaction) -> f32 {
        let s = self.scale;
        let size = FONT_PT * s;
        let text = format!("{} {}", r.emoji, r.count);
        let w = self
            .text_painter
            .as_ref()
            .map(|p| p.measure(&text, size))
            .unwrap_or_else(|| text.chars().count() as f32 * size * 0.6);
        w + CHIP_PAD_PT * 2.0 * s
    }
}

impl Widget for ReactionBar {
    fn measure(&mut self, cx: &mut LayoutContext, constraints: LayoutConstraints) -> Vec2 {
        let h = CHIP_H_PT * cx.scale;
        Vec2::new(constraints.max_size.x.max(40.0 * cx.scale), h)
    }

    fn min_render(&self) -> RenderMinimum {
        RenderMinimum::new(Vec2::new(30.0, 18.0)).with_policy(UnderflowPolicy::Lint)
    }

    fn layout(&mut self, cx: &mut LayoutContext, bounds: Rect) {
        self.bounds = bounds;
        self.scale = cx.scale;
        self.rects.clear();
        let s = cx.scale;
        let h = (CHIP_H_PT * s).min(bounds.height());
        let y = bounds.min_y() + (bounds.height() - h) / 2.0;
        let mut x = bounds.min_x();
        for r in &self.reactions {
            let w = self.chip_width(r);
            self.rects.push(Rect::new(x, y, w, h));
            x += w + CHIP_GAP_PT * s;
        }
        if self.addable {
            let w = CHIP_H_PT * s; // square + chip
            self.add_rect = Rect::new(x, y, w, h);
        } else {
            self.add_rect = Rect::new(0.0, 0.0, 0.0, 0.0);
        }
    }

    fn accessibility(&self, node: &mut AccessKitNode) {
        node.set_role(accesskit::Role::Toolbar);
        node.set_label(self.label.clone());
    }

    fn event(&mut self, cx: &mut EventContext) -> EventResponse {
        match cx.event {
            WidgetEvent::PointerPressed {
                button: PointerButton::Primary,
                position,
                ..
            } => {
                if !self.bounds.contains(*position) {
                    return EventResponse::Ignored;
                }
                if self.addable && self.add_rect.contains(*position) {
                    self.held_add = true;
                    return EventResponse::CapturePointer;
                }
                if let Some(i) = self.chip_at(*position) {
                    self.held = Some(i);
                    return EventResponse::CapturePointer;
                }
                EventResponse::Ignored
            }
            WidgetEvent::PointerReleased {
                button: PointerButton::Primary,
                position,
            } => {
                if self.held_add {
                    self.held_add = false;
                    if self.add_rect.contains(*position) {
                        self.add = true;
                    }
                    return EventResponse::ReleasePointer;
                }
                if let Some(i) = self.held.take() {
                    if self.chip_at(*position) == Some(i) {
                        self.toggle(i);
                    }
                    return EventResponse::ReleasePointer;
                }
                EventResponse::Ignored
            }
            _ => EventResponse::Ignored,
        }
    }

    fn paint(&self, cx: &mut PaintContext) {
        let s = cx.scale;
        let painter = crate::text_paint::resolve_painter(&self.text_painter, cx.text_painter);
        let krect = |r: Rect| {
            kurbo::Rect::new(
                f64::from(r.min_x()),
                f64::from(r.min_y()),
                f64::from(r.max_x()),
                f64::from(r.max_y()),
            )
        };
        let shape = martensite_core::shape::Shape::rounded(RADIUS_PT * s);
        let size = FONT_PT * s;
        for (i, r) in self.reactions.iter().enumerate() {
            let rect = self.rects[i];
            let face = if r.mine {
                cx.color(TokenKey::SecondaryColor, MINE_FACE)
            } else {
                cx.color(TokenKey::SurfaceColor, FACE)
            };
            cx.list.push_fill_shape(krect(rect), &shape, face);
            if r.mine {
                let accent = cx.color(TokenKey::AccentColor, ACCENT);
                cx.list
                    .push_stroke_shape(krect(rect), &shape, 1.2 * s, accent);
            } else {
                let edge = cx.color(TokenKey::DividerColor, EDGE);
                cx.list
                    .push_stroke_shape(krect(rect), &shape, 1.0 * s, edge);
            }
            let label = format!("{} {}", r.emoji, r.count);
            let color = if r.mine {
                cx.color(TokenKey::AccentColor, ACCENT)
            } else {
                cx.color(TokenKey::TextColor, TEXT)
            };
            // Origin is the block top — centre the ink box in the chip.
            let origin = kurbo::Point::new(
                f64::from(rect.min_x() + CHIP_PAD_PT * s),
                f64::from(rect.min_y() + (rect.height() - size * 1.25) / 2.0),
            );
            crate::text_paint::paint_label_clipped(
                painter,
                cx.list,
                krect(rect),
                origin,
                &label,
                size,
                color,
            );
        }
        if self.addable && self.add_rect.width() > 0.0 {
            let edge = cx.color(TokenKey::DividerColor, EDGE);
            cx.list
                .push_stroke_shape(krect(self.add_rect), &shape, 1.0 * s, edge);
            let origin = kurbo::Point::new(
                f64::from(self.add_rect.min_x() + self.add_rect.width() / 2.0 - size * 0.35),
                f64::from(self.add_rect.min_y() + (self.add_rect.height() - size * 1.25) / 2.0),
            );
            crate::text_paint::paint_label_clipped(
                painter,
                cx.list,
                krect(self.add_rect),
                origin,
                "+",
                size,
                cx.color(TokenKey::TextMutedColor, MUTED),
            );
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use martensite_core::HotNode;

    fn laid_out(b: &mut ReactionBar) {
        let mut hot = HotNode::default();
        let mut cx = LayoutContext {
            hot: &mut hot,
            scale: 1.0,
        };
        b.layout(&mut cx, Rect::new(0.0, 0.0, 300.0, 24.0));
    }

    fn ev(b: &mut ReactionBar, e: &WidgetEvent) -> EventResponse {
        b.event(&mut EventContext {
            event: e,
            bounds: b.bounds,
            scale: 1.0,
        })
    }

    fn tap(b: &mut ReactionBar, rect: Rect) {
        let mid = Vec2::new(
            (rect.min_x() + rect.max_x()) / 2.0,
            (rect.min_y() + rect.max_y()) / 2.0,
        );
        ev(
            b,
            &WidgetEvent::PointerPressed {
                button: PointerButton::Primary,
                position: mid,
                count: 1,
            },
        );
        ev(
            b,
            &WidgetEvent::PointerReleased {
                button: PointerButton::Primary,
                position: mid,
            },
        );
    }

    #[test]
    fn click_toggles_mine_and_count() {
        let mut b = ReactionBar::new().reaction(Reaction::new("👍", 3));
        laid_out(&mut b);
        let chip = b.rects[0];
        tap(&mut b, chip);
        let r = b.reaction_at(0).unwrap();
        assert!(r.mine);
        assert_eq!(r.count, 4);
        assert_eq!(b.take_toggled(), Some(0));
        // Untoggle.
        tap(&mut b, chip);
        let r = b.reaction_at(0).unwrap();
        assert!(!r.mine);
        assert_eq!(r.count, 3);
    }

    #[test]
    fn count_saturates_at_zero() {
        let mut b = ReactionBar::new().reaction(Reaction::new("👍", 0).mine(false));
        laid_out(&mut b);
        let chip = b.rects[0];
        tap(&mut b, chip);
        assert_eq!(b.reaction_at(0).unwrap().count, 1);
    }

    #[test]
    fn add_chip_parks() {
        let mut b = ReactionBar::new();
        laid_out(&mut b);
        let add = b.add_rect;
        tap(&mut b, add);
        assert!(b.take_add());
    }

    #[test]
    fn addable_false_hides_chip() {
        let mut b = ReactionBar::new().addable(false);
        laid_out(&mut b);
        assert_eq!(b.add_rect.width(), 0.0);
    }

    #[test]
    fn release_off_ignores() {
        let mut b = ReactionBar::new().reaction(Reaction::new("👍", 1));
        laid_out(&mut b);
        let r = b.rects[0];
        ev(
            &mut b,
            &WidgetEvent::PointerPressed {
                button: PointerButton::Primary,
                position: Vec2::new((r.min_x() + r.max_x()) / 2.0, (r.min_y() + r.max_y()) / 2.0),
                count: 1,
            },
        );
        ev(
            &mut b,
            &WidgetEvent::PointerReleased {
                button: PointerButton::Primary,
                position: Vec2::new(500.0, 500.0),
            },
        );
        assert!(!b.reaction_at(0).unwrap().mine);
        assert_eq!(b.take_toggled(), None);
    }

    #[test]
    fn paint_without_painter() {
        let mut b = ReactionBar::new()
            .reaction(Reaction::new("👍", 3).mine(true))
            .reaction(Reaction::new("🎉", 1));
        laid_out(&mut b);
        let mut list = martensite_core::PaintList::default();
        let theme = martensite_theme::Theme::new("test");
        b.paint(&mut PaintContext {
            list: &mut list,
            bounds: b.bounds,
            theme: &theme,
            scale: 1.0,
            text_painter: None,
        });
        assert!(!list.is_empty());
    }
}
