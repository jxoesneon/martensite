//! `SocialCard` — a feed post card (Twitter/Mastodon idiom): an
//! author row (avatar swatch, display name, handle, timestamp), a
//! body, and an action bar of [`CardAction`]s — like / comment /
//! share — each with a count.
//!
//! Clicking an action parks its index in
//! [`SocialCard::take_action`]; `set_liked`/`set_counts` update
//! state host-side. Companion to [`Card`](crate::widgets::Card)
//! and [`ReactionBar`](crate::widgets::ReactionBar).
//!
//! # Examples
//!
//! ```
//! use martensite::widgets::social_card::SocialCard;
//!
//! let c = SocialCard::new("Ana", "@ana", "2h", "Hello, world!");
//! assert_eq!(c.author(), "Ana");
//! ```

use accesskit::Node as AccessKitNode;
use glam::Vec2;
use martensite_core::{
    EventContext, EventResponse, LayoutConstraints, LayoutContext, PaintContext, PointerButton,
    Rect, RenderMinimum, UnderflowPolicy, Widget, WidgetEvent,
};
use martensite_theme::TokenKey;

use crate::text_paint::SharedTextPainter;

const PAD_PT: f32 = 12.0;
const AVATAR_PT: f32 = 36.0;
const NAME_PT: f32 = 13.0;
const META_PT: f32 = 11.0;
const BODY_PT: f32 = 13.0;
const ACTION_PT: f32 = 28.0;
const AVATAR: [u8; 4] = [120, 140, 180, 255];

const FACE: [u8; 4] = [30, 32, 40, 255];
const TEXT: [u8; 4] = [235, 237, 240, 255];
const MUTED_FG: [u8; 4] = [150, 154, 164, 255];
const LIKED: [u8; 4] = [230, 90, 120, 255];
const ACTION_HOVER: [u8; 4] = [255, 255, 255, 14];

/// A post action shown in the bottom bar.
///
/// ```
/// use martensite::widgets::social_card::CardAction;
///
/// let a = CardAction::new("♥", 12);
/// assert_eq!(a.count, 12);
/// ```
#[derive(Clone, Debug)]
pub struct CardAction {
    /// Glyph or short label.
    pub glyph: String,
    /// Display count (0 hides the number).
    pub count: u32,
}

impl CardAction {
    /// An action with a glyph and count.
    ///
    /// ```
    /// use martensite::widgets::social_card::CardAction;
    ///
    /// assert_eq!(CardAction::new("♥", 3).count, 3);
    /// ```
    pub fn new(glyph: impl Into<String>, count: u32) -> Self {
        Self {
            glyph: glyph.into(),
            count,
        }
    }
}

/// The card — see the module docs.
///
/// ```
/// use martensite::widgets::social_card::SocialCard;
///
/// let c = SocialCard::new("Ana", "@ana", "2h", "hi");
/// assert_eq!(c.action_count(), 3);
/// ```
pub struct SocialCard {
    /// Accessibility label.
    pub label: String,
    /// Action bar contents.
    pub actions: Vec<CardAction>,
    author: String,
    handle: String,
    timestamp: String,
    body: String,
    liked: bool,
    avatar: [u8; 4],
    taken: Option<usize>,
    hovered: Option<usize>,
    action_rects: Vec<Rect>,
    text_painter: Option<SharedTextPainter>,
    bounds: Rect,
    scale: f32,
}

impl std::fmt::Debug for SocialCard {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("SocialCard")
            .field("author", &self.author)
            .finish()
    }
}

impl SocialCard {
    /// A post by `author` (`handle`, `timestamp`) saying `body`.
    ///
    /// ```
    /// use martensite::widgets::social_card::SocialCard;
    ///
    /// let c = SocialCard::new("Ana", "@ana", "2h", "hello");
    /// assert_eq!(c.author(), "Ana");
    /// ```
    pub fn new(
        author: impl Into<String>,
        handle: impl Into<String>,
        timestamp: impl Into<String>,
        body: impl Into<String>,
    ) -> Self {
        Self {
            label: "Post".to_string(),
            actions: vec![
                CardAction::new("♥", 0),
                CardAction::new("💬", 0),
                CardAction::new("↗", 0),
            ],
            author: author.into(),
            handle: handle.into(),
            timestamp: timestamp.into(),
            body: body.into(),
            liked: false,
            avatar: AVATAR,
            taken: None,
            hovered: None,
            action_rects: Vec::new(),
            text_painter: None,
            bounds: Rect::new(0.0, 0.0, 0.0, 0.0),
            scale: 1.0,
        }
    }

    /// Avatar swatch color.
    ///
    /// ```
    /// use martensite::widgets::social_card::SocialCard;
    ///
    /// let c = SocialCard::new("A", "@a", "1h", "x").avatar_color([1; 4]);
    /// ```
    pub fn avatar_color(mut self, color: [u8; 4]) -> Self {
        self.avatar = color;
        self
    }

    /// Replaces the action bar.
    ///
    /// ```
    /// use martensite::widgets::social_card::{CardAction, SocialCard};
    ///
    /// let c = SocialCard::new("A", "@a", "1h", "x")
    ///     .with_actions(vec![CardAction::new("♥", 5)]);
    /// assert_eq!(c.action_count(), 1);
    /// ```
    pub fn with_actions(mut self, actions: Vec<CardAction>) -> Self {
        self.actions = actions;
        self
    }

    /// Accessibility label.
    ///
    /// ```
    /// use martensite::widgets::social_card::SocialCard;
    ///
    /// assert_eq!(SocialCard::new("A", "@a", "1h", "x").label("Feed item").label, "Feed item");
    /// ```
    pub fn label(mut self, label: impl Into<String>) -> Self {
        self.label = label.into();
        self
    }

    /// Shared text painter.
    ///
    /// ```no_run
    /// use martensite::widgets::social_card::SocialCard;
    /// # fn example(p: martensite::text_paint::SharedTextPainter) {
    /// let _c = SocialCard::new("A", "@a", "1h", "x").with_text_painter(p);
    /// # }
    /// ```
    pub fn with_text_painter(mut self, painter: SharedTextPainter) -> Self {
        self.text_painter = Some(painter);
        self
    }

    /// Author name.
    ///
    /// ```
    /// use martensite::widgets::social_card::SocialCard;
    ///
    /// assert_eq!(SocialCard::new("Ana", "@a", "1h", "x").author(), "Ana");
    /// ```
    pub fn author(&self) -> &str {
        &self.author
    }

    /// Post body.
    ///
    /// ```
    /// use martensite::widgets::social_card::SocialCard;
    ///
    /// assert_eq!(SocialCard::new("A", "@a", "1h", "hi").body(), "hi");
    /// ```
    pub fn body(&self) -> &str {
        &self.body
    }

    /// Action count.
    ///
    /// ```
    /// use martensite::widgets::social_card::SocialCard;
    ///
    /// assert_eq!(SocialCard::new("A", "@a", "1h", "x").action_count(), 3);
    /// ```
    pub fn action_count(&self) -> usize {
        self.actions.len()
    }

    /// Liked state (tints the first action).
    ///
    /// ```
    /// use martensite::widgets::social_card::SocialCard;
    ///
    /// let mut c = SocialCard::new("A", "@a", "1h", "x");
    /// c.set_liked(true);
    /// assert!(c.is_liked());
    /// ```
    pub fn set_liked(&mut self, liked: bool) {
        self.liked = liked;
    }

    /// Liked state.
    ///
    /// ```
    /// use martensite::widgets::social_card::SocialCard;
    ///
    /// assert!(!SocialCard::new("A", "@a", "1h", "x").is_liked());
    /// ```
    pub fn is_liked(&self) -> bool {
        self.liked
    }

    /// Updates an action's count host-side.
    ///
    /// ```
    /// use martensite::widgets::social_card::SocialCard;
    ///
    /// let mut c = SocialCard::new("A", "@a", "1h", "x");
    /// c.set_count(0, 42);
    /// assert_eq!(c.actions[0].count, 42);
    /// ```
    pub fn set_count(&mut self, i: usize, count: u32) {
        if let Some(a) = self.actions.get_mut(i) {
            a.count = count;
        }
    }

    /// Drains the last clicked action index.
    ///
    /// ```
    /// use martensite::widgets::social_card::SocialCard;
    ///
    /// let mut c = SocialCard::new("A", "@a", "1h", "x");
    /// assert_eq!(c.take_action(), None);
    /// ```
    pub fn take_action(&mut self) -> Option<usize> {
        self.taken.take()
    }

    fn hit(&self, p: Vec2) -> Option<usize> {
        self.action_rects.iter().position(|r| r.contains(p))
    }
}

impl Widget for SocialCard {
    fn measure(&mut self, cx: &mut LayoutContext, constraints: LayoutConstraints) -> Vec2 {
        let s = cx.scale;
        let lines = self.body.lines().count().max(1) as f32;
        let h = PAD_PT * 2.0 + AVATAR_PT + 8.0 + lines * BODY_PT * 1.3 + ACTION_PT;
        Vec2::new(
            (340.0 * s).min(constraints.max_size.x.max(0.0)),
            (h * s).min(constraints.max_size.y.max(0.0)),
        )
    }

    fn min_render(&self) -> RenderMinimum {
        RenderMinimum::new(Vec2::new(220.0, 120.0)).with_policy(UnderflowPolicy::Lint)
    }

    fn layout(&mut self, cx: &mut LayoutContext, bounds: Rect) {
        self.bounds = bounds;
        self.scale = cx.scale;
        let s = cx.scale;
        // Action row pinned to the bottom.
        let n = self.actions.len();
        self.action_rects.clear();
        let ay = bounds.max_y() - PAD_PT * s - ACTION_PT * s;
        let aw = (bounds.width() - PAD_PT * 2.0 * s) / n.max(1) as f32;
        for i in 0..n {
            self.action_rects.push(Rect::new(
                bounds.min_x() + PAD_PT * s + i as f32 * aw,
                ay,
                aw - 4.0 * s,
                ACTION_PT * s,
            ));
        }
    }

    fn accessibility(&self, node: &mut AccessKitNode) {
        node.set_role(accesskit::Role::Group);
        node.set_label(format!("{} by {}", self.label, self.author));
        node.set_value(self.body.clone());
    }

    fn event(&mut self, cx: &mut EventContext) -> EventResponse {
        match cx.event {
            WidgetEvent::PointerMoved { position } => {
                let h = self.hit(*position);
                if h != self.hovered {
                    self.hovered = h;
                    return EventResponse::RequestRepaint;
                }
                EventResponse::Ignored
            }
            WidgetEvent::PointerLeave => {
                if self.hovered.take().is_some() {
                    EventResponse::RequestRepaint
                } else {
                    EventResponse::Ignored
                }
            }
            WidgetEvent::PointerReleased {
                button: PointerButton::Primary,
                position,
            } => {
                if let Some(i) = self.hit(*position) {
                    if i == 0 {
                        self.liked = !self.liked;
                    }
                    self.taken = Some(i);
                    return EventResponse::RequestRepaint;
                }
                EventResponse::Ignored
            }
            _ => EventResponse::Ignored,
        }
    }

    fn paint(&self, cx: &mut PaintContext) {
        let s = cx.scale;
        let painter = crate::text_paint::resolve_painter(&self.text_painter, cx.text_painter);
        let b = self.bounds;
        cx.list.push_fill_rect(
            kurbo::Rect::new(
                f64::from(b.min_x()),
                f64::from(b.min_y()),
                f64::from(b.max_x()),
                f64::from(b.max_y()),
            ),
            cx.color(TokenKey::SurfaceColor, FACE),
        );
        // Avatar + author row.
        let av = AVATAR_PT * s;
        let ar = kurbo::Rect::new(
            f64::from(b.min_x() + PAD_PT * s),
            f64::from(b.min_y() + PAD_PT * s),
            f64::from(b.min_x() + PAD_PT * s + av),
            f64::from(b.min_y() + PAD_PT * s + av),
        );
        cx.list
            .push_fill_shape(ar, &martensite_core::shape::Shape::ELLIPSE, self.avatar);
        let text_x = b.min_x() + PAD_PT * s + av + 10.0 * s;
        crate::text_paint::paint_label(
            painter,
            cx.list,
            kurbo::Point::new(
                f64::from(text_x),
                f64::from(b.min_y() + PAD_PT * s + av * 0.4),
            ),
            &self.author,
            NAME_PT * s,
            cx.color(TokenKey::TextColor, TEXT),
        );
        crate::text_paint::paint_label(
            painter,
            cx.list,
            kurbo::Point::new(
                f64::from(text_x),
                f64::from(b.min_y() + PAD_PT * s + av * 0.9),
            ),
            &format!("{} · {}", self.handle, self.timestamp),
            META_PT * s,
            MUTED_FG,
        );
        // Body.
        let body_y = b.min_y() + PAD_PT * s + av + 12.0 * s;
        for (i, line) in self.body.lines().enumerate() {
            crate::text_paint::paint_label_clipped(
                painter,
                cx.list,
                kurbo::Rect::new(
                    f64::from(b.min_x() + PAD_PT * s),
                    f64::from(body_y),
                    f64::from(b.max_x() - PAD_PT * s),
                    f64::from(b.max_y() - (PAD_PT + ACTION_PT) * s),
                ),
                kurbo::Point::new(
                    f64::from(b.min_x() + PAD_PT * s),
                    f64::from(body_y + BODY_PT * 1.3 * s * (i as f32 + 0.75)),
                ),
                line,
                BODY_PT * s,
                cx.color(TokenKey::TextColor, TEXT),
            );
        }
        // Action bar.
        let shape = martensite_core::shape::Shape::rounded(5.0 * s);
        for (i, (a, r)) in self
            .actions
            .iter()
            .zip(self.action_rects.iter())
            .enumerate()
        {
            if self.hovered == Some(i) {
                cx.list.push_fill_shape(
                    kurbo::Rect::new(
                        f64::from(r.min_x()),
                        f64::from(r.min_y()),
                        f64::from(r.max_x()),
                        f64::from(r.max_y()),
                    ),
                    &shape,
                    ACTION_HOVER,
                );
            }
            let fg = if i == 0 && self.liked {
                cx.color(TokenKey::ErrorColor, LIKED)
            } else {
                MUTED_FG
            };
            let label = if a.count > 0 {
                format!("{} {}", a.glyph, a.count)
            } else {
                a.glyph.clone()
            };
            crate::text_paint::paint_label(
                painter,
                cx.list,
                kurbo::Point::new(
                    f64::from(r.min_x() + 8.0 * s),
                    f64::from(r.min_y() + r.height() * 0.72),
                ),
                &label,
                META_PT * s,
                fg,
            );
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use martensite_core::HotNode;

    fn fixture() -> SocialCard {
        SocialCard::new("Ana", "@ana", "2h", "Hello, world!")
    }

    fn laid_out(c: &mut SocialCard) {
        let mut hot = HotNode::default();
        let mut cx = LayoutContext {
            hot: &mut hot,
            scale: 1.0,
        };
        c.layout(&mut cx, Rect::new(0.0, 0.0, 340.0, 160.0));
    }

    fn click(c: &mut SocialCard, r: Rect) {
        c.event(&mut EventContext {
            event: &WidgetEvent::PointerReleased {
                button: PointerButton::Primary,
                position: Vec2::new((r.min_x() + r.max_x()) / 2.0, (r.min_y() + r.max_y()) / 2.0),
            },
            bounds: c.bounds,
            scale: 1.0,
        });
    }

    #[test]
    fn like_toggles() {
        let mut c = fixture();
        laid_out(&mut c);
        let r = c.action_rects[0];
        click(&mut c, r);
        assert!(c.is_liked());
        assert_eq!(c.take_action(), Some(0));
        click(&mut c, r);
        assert!(!c.is_liked());
    }

    #[test]
    fn actions_park_index() {
        let mut c = fixture();
        laid_out(&mut c);
        let r = c.action_rects[2];
        click(&mut c, r);
        assert_eq!(c.take_action(), Some(2));
    }

    #[test]
    fn paint_without_painter() {
        let mut c = fixture();
        laid_out(&mut c);
        let mut list = martensite_core::PaintList::default();
        let theme = martensite_theme::Theme::new("test");
        c.paint(&mut PaintContext {
            list: &mut list,
            bounds: c.bounds,
            theme: &theme,
            scale: 1.0,
            text_painter: None,
        });
        assert!(!list.is_empty());
    }
}
