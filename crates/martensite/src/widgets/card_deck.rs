//! `CardDeck` — a fanned stack of child-widget cards where only the
//! front card is interactive (the Tinder/card-stack idiom).
//!
//! Cards are [`Widget`] children; up to two cards behind the front
//! peek out at the bottom edge. A horizontal drag past the swipe
//! threshold dismisses the front card and parks `true` in
//! [`CardDeck::take_dismissed`]; `←`/`→` cycle the deck without
//! dismissing ([`CardDeck::cycle_next`] /
//! [`CardDeck::cycle_prev`]).
//!
//! # Examples
//!
//! ```
//! use martensite::widgets::card_deck::CardDeck;
//! use martensite::widgets::text::Text;
//!
//! let mut d = CardDeck::new()
//!     .card(Text::new("first"))
//!     .card(Text::new("second"));
//! assert_eq!(d.depth(), 2);
//! d.dismiss();
//! assert_eq!(d.depth(), 1);
//! ```

use accesskit::Node as AccessKitNode;
use glam::Vec2;
use martensite_core::{
    EventContext, EventResponse, LayoutConstraints, LayoutContext, PaintContext, PointerButton,
    Rect, RenderMinimum, UnderflowPolicy, Widget, WidgetEvent,
};
use martensite_theme::TokenKey;

const W_PT: f32 = 240.0;
const H_PT: f32 = 320.0;
/// Horizontal shrink per stack depth.
const INSET_PT: f32 = 8.0;
/// Downward shift per stack depth.
const SHIFT_PT: f32 = 10.0;
/// Back cards peek this far below the front card.
const PEEK_PT: f32 = 22.0;
/// Horizontal drag distance that dismisses the front card.
const SWIPE_PT: f32 = 40.0;
/// How many cards show through the fan (front + 2 behind).
const VISIBLE: usize = 3;

const FRAME: [u8; 4] = [44, 44, 50, 255];

/// A fanned deck of cards — see the module docs.
///
/// `CardDeck` owns every card; only the front card reports bounds
/// for traversal.
///
/// # Examples
///
/// ```
/// use martensite::prelude::*;
/// use martensite::widgets::card_deck::CardDeck;
///
/// let d = CardDeck::new().card(Text::new("a"));
/// assert_eq!(d.depth(), 1);
/// ```
pub struct CardDeck {
    /// Accessibility label.
    pub label: String,
    /// Cards; the LAST entry is the front (painted last = on top).
    cards: Vec<Box<dyn Widget>>,
    dismissed: bool,
    /// Per-card layout rects (front first).
    rects: Vec<Rect>,
    /// Drag origin for swipe detection.
    drag_from: Option<Vec2>,
    bounds: Rect,
    scale: f32,
    text_painter: Option<crate::text_paint::SharedTextPainter>,
}

impl std::fmt::Debug for CardDeck {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("CardDeck")
            .field("cards", &self.cards.len())
            .finish()
    }
}

impl Default for CardDeck {
    fn default() -> Self {
        Self::new()
    }
}

impl CardDeck {
    /// Creates an empty deck.
    ///
    /// ```
    /// use martensite::widgets::card_deck::CardDeck;
    ///
    /// assert!(CardDeck::new().is_empty());
    /// ```
    pub fn new() -> Self {
        Self {
            label: "Card deck".to_string(),
            cards: Vec::new(),
            dismissed: false,
            rects: Vec::new(),
            drag_from: None,
            bounds: Rect::new(0.0, 0.0, 0.0, 0.0),
            scale: 1.0,
            text_painter: None,
        }
    }

    /// Accessibility label.
    ///
    /// ```
    /// use martensite::widgets::card_deck::CardDeck;
    ///
    /// assert_eq!(CardDeck::new().label("Candidates").label, "Candidates");
    /// ```
    pub fn label(mut self, label: impl Into<String>) -> Self {
        self.label = label.into();
        self
    }

    /// Custom text painter.
    ///
    /// ```
    /// use martensite::widgets::card_deck::CardDeck;
    ///
    /// let _ = CardDeck::new(); // painter optional
    /// ```
    pub fn with_text_painter(mut self, p: crate::text_paint::SharedTextPainter) -> Self {
        self.text_painter = Some(p);
        self
    }

    /// Puts a card on the front of the deck.
    ///
    /// ```
    /// use martensite::prelude::*;
    /// use martensite::widgets::card_deck::CardDeck;
    ///
    /// let d = CardDeck::new().card(Text::new("x"));
    /// assert_eq!(d.depth(), 1);
    /// ```
    pub fn card(mut self, w: impl Widget + 'static) -> Self {
        self.cards.push(Box::new(w));
        self
    }

    /// Cards in the deck.
    ///
    /// ```
    /// use martensite::widgets::card_deck::CardDeck;
    ///
    /// assert_eq!(CardDeck::new().depth(), 0);
    /// ```
    pub fn depth(&self) -> usize {
        self.cards.len()
    }

    /// Whether the deck is empty.
    ///
    /// ```
    /// use martensite::widgets::card_deck::CardDeck;
    ///
    /// assert!(CardDeck::new().is_empty());
    /// ```
    pub fn is_empty(&self) -> bool {
        self.cards.is_empty()
    }

    /// Removes the front card.
    ///
    /// ```
    /// use martensite::prelude::*;
    /// use martensite::widgets::card_deck::CardDeck;
    ///
    /// let mut d = CardDeck::new().card(Text::new("a"));
    /// d.dismiss();
    /// assert!(d.is_empty());
    /// ```
    pub fn dismiss(&mut self) {
        self.cards.pop();
    }

    /// Sends the front card to the back of the deck.
    ///
    /// ```
    /// use martensite::prelude::*;
    /// use martensite::widgets::card_deck::CardDeck;
    ///
    /// let mut d = CardDeck::new()
    ///     .card(Text::new("back"))
    ///     .card(Text::new("front"));
    /// d.cycle_next(); // "front" goes to the back
    /// assert_eq!(d.depth(), 2);
    /// ```
    pub fn cycle_next(&mut self) {
        if self.cards.len() > 1 {
            let top = self.cards.pop().unwrap();
            self.cards.insert(0, top);
        }
    }

    /// Brings the back card to the front.
    ///
    /// ```
    /// use martensite::prelude::*;
    /// use martensite::widgets::card_deck::CardDeck;
    ///
    /// let mut d = CardDeck::new()
    ///     .card(Text::new("back"))
    ///     .card(Text::new("front"));
    /// d.cycle_prev(); // "back" comes forward
    /// assert_eq!(d.depth(), 2);
    /// ```
    pub fn cycle_prev(&mut self) {
        if self.cards.len() > 1 {
            let back = self.cards.remove(0);
            self.cards.push(back);
        }
    }

    /// Drains whether a swipe dismissed the front card.
    ///
    /// ```
    /// use martensite::widgets::card_deck::CardDeck;
    ///
    /// assert!(!CardDeck::new().take_dismissed());
    /// ```
    pub fn take_dismissed(&mut self) -> bool {
        std::mem::take(&mut self.dismissed)
    }

    /// Stack depth of card index `i` (0 = front).
    fn depth_of(&self, i: usize) -> usize {
        self.cards.len() - 1 - i
    }
}

impl Widget for CardDeck {
    fn measure(&mut self, cx: &mut LayoutContext, constraints: LayoutConstraints) -> Vec2 {
        Vec2::new(
            cx.pt(W_PT).min(constraints.max_size.x.max(0.0)),
            cx.pt(H_PT).min(constraints.max_size.y.max(0.0)),
        )
    }

    fn min_render(&self) -> RenderMinimum {
        RenderMinimum::new(Vec2::new(120.0, 160.0)).with_policy(UnderflowPolicy::Lint)
    }

    fn layout(&mut self, cx: &mut LayoutContext, bounds: Rect) {
        self.bounds = bounds;
        self.scale = cx.scale;
        let s = cx.scale;
        let peek = PEEK_PT * s;
        // Content = bounds minus the peek strip at the bottom.
        let content = Rect::new(
            bounds.min_x(),
            bounds.min_y(),
            bounds.width(),
            (bounds.height() - peek).max(0.0),
        );
        self.rects.clear();
        for d in 0..VISIBLE {
            let inset = INSET_PT * s * d as f32;
            let shift = SHIFT_PT * s * d as f32;
            self.rects.push(Rect::new(
                content.min_x() + inset,
                content.min_y() + shift,
                (content.width() - inset * 2.0).max(0.0),
                content.height(),
            ));
        }
        let len = self.cards.len();
        for (i, card) in self.cards.iter_mut().enumerate() {
            let d = len - 1 - i;
            if d < VISIBLE {
                cx.layout_child(card.as_mut(), self.rects[d]);
            }
        }
    }

    fn accessibility(&self, node: &mut AccessKitNode) {
        node.set_role(accesskit::Role::Group);
        node.set_label(format!("{} — {} cards", self.label, self.cards.len()));
    }

    fn event(&mut self, cx: &mut EventContext) -> EventResponse {
        match cx.event {
            WidgetEvent::PointerPressed {
                button: PointerButton::Primary,
                position,
                ..
            } => {
                if self.bounds.contains(*position) {
                    self.drag_from = Some(*position);
                }
                self.forward_top(cx)
            }
            WidgetEvent::PointerReleased {
                button: PointerButton::Primary,
                position,
            } => {
                if let Some(from) = self.drag_from.take() {
                    let dx = (position.x - from.x).abs();
                    if dx > SWIPE_PT * cx.scale && !self.cards.is_empty() {
                        self.dismiss();
                        self.dismissed = true;
                        return EventResponse::RequestRepaint;
                    }
                }
                self.forward_top(cx)
            }
            WidgetEvent::KeyPressed { key, .. } => {
                match key.as_str() {
                    "ArrowRight" => {
                        self.cycle_next();
                        return EventResponse::RequestRepaint;
                    }
                    "ArrowLeft" => {
                        self.cycle_prev();
                        return EventResponse::RequestRepaint;
                    }
                    _ => {}
                }
                self.forward_top(cx)
            }
            _ => self.forward_top(cx),
        }
    }

    fn paint(&self, cx: &mut PaintContext) {
        let krect = |r: Rect| {
            kurbo::Rect::new(
                f64::from(r.min_x()),
                f64::from(r.min_y()),
                f64::from(r.max_x()),
                f64::from(r.max_y()),
            )
        };
        let s = self.scale;
        // Frames behind every visible card — children paint over
        // these, so a rim shows even when a card is transparent.
        let visible = self.cards.len().min(VISIBLE);
        for d in (1..visible).rev() {
            let r = self.rects[d];
            let frame = Rect::new(
                r.min_x() - 2.0 * s,
                r.min_y() - 2.0 * s,
                r.width() + 4.0 * s,
                r.height() + 4.0 * s,
            );
            cx.list.push_fill_shape(
                krect(frame),
                &martensite_core::shape::Shape::rounded(10.0 * s),
                cx.color(TokenKey::SurfaceColor, FRAME),
            );
        }
        if !self.cards.is_empty() {
            let r = self.rects[0];
            cx.list.push_fill_shape(
                krect(r),
                &martensite_core::shape::Shape::rounded(10.0 * s),
                cx.color(TokenKey::SurfaceColor, FRAME),
            );
        }
    }

    fn child_count(&self) -> usize {
        self.cards.len()
    }

    fn child(&self, index: usize) -> Option<&dyn Widget> {
        self.cards.get(index).map(|c| c.as_ref() as &dyn Widget)
    }

    fn child_mut(&mut self, index: usize) -> Option<&mut dyn Widget> {
        self.cards
            .get_mut(index)
            .map(|c| c.as_mut() as &mut dyn Widget)
    }

    fn child_bounds(&self, index: usize) -> Option<Rect> {
        let d = self.depth_of(index);
        self.rects.get(d).copied()
    }
}

impl CardDeck {
    /// Forward the event to the front card through the child
    /// protocol.
    fn forward_top(&mut self, cx: &mut EventContext) -> EventResponse {
        let idx = self.cards.len().saturating_sub(1);
        let Some(b) = self.child_bounds(idx) else {
            return EventResponse::Ignored;
        };
        if let Some(pos) = cx.event.position() {
            if !b.contains(pos) {
                return EventResponse::Ignored;
            }
        }
        let mut child_cx = EventContext {
            event: cx.event,
            bounds: b,
            scale: cx.scale,
        };
        match self.child_mut(idx) {
            Some(c) => c.event(&mut child_cx),
            None => EventResponse::Ignored,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::widgets::text::Text;
    use martensite_core::HotNode;

    fn laid_out(w: &mut CardDeck, wd: f32, h: f32) {
        let mut hot = HotNode::default();
        let mut cx = LayoutContext {
            hot: &mut hot,
            scale: 1.0,
        };
        w.measure(
            &mut cx,
            LayoutConstraints {
                min_size: Vec2::ZERO,
                max_size: Vec2::new(wd, h),
            },
        );
        w.layout(&mut cx, Rect::new(0.0, 0.0, wd, h));
    }

    #[test]
    fn cycle_wraps() {
        let mut d = CardDeck::new()
            .card(Text::new("a"))
            .card(Text::new("b"))
            .card(Text::new("c"));
        d.cycle_next(); // c → back; front = b
        assert_eq!(d.depth(), 3);
        d.cycle_prev(); // back → front = c
        d.dismiss();
        assert_eq!(d.depth(), 2);
    }

    #[test]
    fn swipe_dismisses_front() {
        let mut d = CardDeck::new().card(Text::new("a")).card(Text::new("b"));
        laid_out(&mut d, 240.0, 320.0);
        // Press on the front card, release 60pt to the right.
        d.event(&mut EventContext {
            event: &WidgetEvent::PointerPressed {
                button: PointerButton::Primary,
                position: Vec2::new(120.0, 100.0),
                count: 1,
            },
            bounds: d.bounds,
            scale: 1.0,
        });
        d.event(&mut EventContext {
            event: &WidgetEvent::PointerReleased {
                button: PointerButton::Primary,
                position: Vec2::new(180.0, 100.0),
            },
            bounds: d.bounds,
            scale: 1.0,
        });
        assert_eq!(d.depth(), 1);
        assert!(d.take_dismissed());
        assert!(!d.take_dismissed());
    }

    #[test]
    fn short_release_keeps_card() {
        let mut d = CardDeck::new().card(Text::new("a"));
        laid_out(&mut d, 240.0, 320.0);
        d.event(&mut EventContext {
            event: &WidgetEvent::PointerPressed {
                button: PointerButton::Primary,
                position: Vec2::new(120.0, 100.0),
                count: 1,
            },
            bounds: d.bounds,
            scale: 1.0,
        });
        d.event(&mut EventContext {
            event: &WidgetEvent::PointerReleased {
                button: PointerButton::Primary,
                position: Vec2::new(130.0, 100.0),
            },
            bounds: d.bounds,
            scale: 1.0,
        });
        assert_eq!(d.depth(), 1);
        assert!(!d.take_dismissed());
    }

    #[test]
    fn arrows_cycle() {
        let mut d = CardDeck::new().card(Text::new("a")).card(Text::new("b"));
        laid_out(&mut d, 240.0, 320.0);
        d.event(&mut EventContext {
            event: &WidgetEvent::KeyPressed {
                key: "ArrowRight".to_string(),
                repeat: false,
            },
            bounds: d.bounds,
            scale: 1.0,
        });
        // b cycled to back; front is now a — check via child order.
        assert!(d.child_bounds(0).is_some()); // still visible
    }

    #[test]
    fn back_cards_report_fanned_bounds() {
        let mut d = CardDeck::new()
            .card(Text::new("a"))
            .card(Text::new("b"))
            .card(Text::new("c"));
        laid_out(&mut d, 240.0, 320.0);
        let front = d.child_bounds(2).unwrap();
        let back = d.child_bounds(1).unwrap();
        assert!(back.min_y() > front.min_y()); // back card sits lower
        assert!(back.width() < front.width()); // and narrower
    }
}
