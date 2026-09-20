//! `HoverCard` — a delayed hover preview card (Reddit/GitHub
//! profile-popover idiom): the widget occupies the trigger
//! region, and once the pointer rests inside for
//! [`HoverCard::delay`] it opens a title/body card.
//!
//! Call [`HoverCard::tick`] once per frame with the frame delta;
//! `PointerEnter`/`PointerLeave` drive the timer. Opening parks
//! `take_opened`, closing parks `take_closed`, and
//! [`HoverCard::is_open`] reports the state. In a real overlay
//! the host positions this widget over the anchor and paints
//! nothing until `is_open`.
//!
//! # Examples
//!
//! ```
//! use martensite::widgets::hover_card::HoverCard;
//! use std::time::Duration;
//!
//! let mut h = HoverCard::new("Ana", "@ana — maintainer");
//! assert!(!h.is_open());
//! ```

use std::time::Duration;

use accesskit::Node as AccessKitNode;
use glam::Vec2;
use martensite_core::{
    EventContext, EventResponse, LayoutConstraints, LayoutContext, PaintContext, Rect,
    RenderMinimum, UnderflowPolicy, Widget, WidgetEvent,
};
use martensite_theme::TokenKey;

use crate::text_paint::SharedTextPainter;

const PAD_PT: f32 = 12.0;
const TITLE_PT: f32 = 14.0;
const BODY_PT: f32 = 11.5;
const DEFAULT_DELAY_MS: f32 = 500.0;

const FACE: [u8; 4] = [40, 43, 54, 255];
const TEXT: [u8; 4] = [235, 237, 240, 255];
const MUTED_FG: [u8; 4] = [170, 174, 184, 255];

/// The delayed preview card — see the module docs.
///
/// ```
/// use martensite::widgets::hover_card::HoverCard;
///
/// let h = HoverCard::new("Title", "Body");
/// assert!(!h.is_open());
/// ```
pub struct HoverCard {
    /// Accessibility label.
    pub label: String,
    /// Hover delay before opening.
    pub delay: Duration,
    title: String,
    body: String,
    hovering: bool,
    elapsed: f32,
    open: bool,
    opened: bool,
    closed: bool,
    text_painter: Option<SharedTextPainter>,
    bounds: Rect,
    scale: f32,
}

impl std::fmt::Debug for HoverCard {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("HoverCard")
            .field("open", &self.open)
            .finish()
    }
}

impl HoverCard {
    /// A card with a title and body.
    ///
    /// ```
    /// use martensite::widgets::hover_card::HoverCard;
    ///
    /// assert_eq!(HoverCard::new("T", "B").title(), "T");
    /// ```
    pub fn new(title: impl Into<String>, body: impl Into<String>) -> Self {
        Self {
            label: "Hover card".to_string(),
            delay: Duration::from_millis(DEFAULT_DELAY_MS as u64),
            title: title.into(),
            body: body.into(),
            hovering: false,
            elapsed: 0.0,
            open: false,
            opened: false,
            closed: false,
            text_painter: None,
            bounds: Rect::new(0.0, 0.0, 0.0, 0.0),
            scale: 1.0,
        }
    }

    /// Hover delay before opening.
    ///
    /// ```
    /// use martensite::widgets::hover_card::HoverCard;
    /// use std::time::Duration;
    ///
    /// let h = HoverCard::new("T", "B").with_delay(Duration::from_millis(200));
    /// assert_eq!(h.delay, Duration::from_millis(200));
    /// ```
    pub fn with_delay(mut self, delay: Duration) -> Self {
        self.delay = delay;
        self
    }

    /// Accessibility label.
    ///
    /// ```
    /// use martensite::widgets::hover_card::HoverCard;
    ///
    /// assert_eq!(HoverCard::new("T", "B").label("Preview").label, "Preview");
    /// ```
    pub fn label(mut self, label: impl Into<String>) -> Self {
        self.label = label.into();
        self
    }

    /// Shared text painter.
    ///
    /// ```no_run
    /// use martensite::widgets::hover_card::HoverCard;
    /// # fn example(p: martensite::text_paint::SharedTextPainter) {
    /// let _h = HoverCard::new("T", "B").with_text_painter(p);
    /// # }
    /// ```
    pub fn with_text_painter(mut self, painter: SharedTextPainter) -> Self {
        self.text_painter = Some(painter);
        self
    }

    /// Title text.
    ///
    /// ```
    /// use martensite::widgets::hover_card::HoverCard;
    ///
    /// assert_eq!(HoverCard::new("T", "B").title(), "T");
    /// ```
    pub fn title(&self) -> &str {
        &self.title
    }

    /// Body text.
    ///
    /// ```
    /// use martensite::widgets::hover_card::HoverCard;
    ///
    /// assert_eq!(HoverCard::new("T", "B").body(), "B");
    /// ```
    pub fn body(&self) -> &str {
        &self.body
    }

    /// Whether the card is open.
    ///
    /// ```
    /// use martensite::widgets::hover_card::HoverCard;
    ///
    /// assert!(!HoverCard::new("T", "B").is_open());
    /// ```
    pub fn is_open(&self) -> bool {
        self.open
    }

    /// Accumulates frame time while hovering; opens the card past
    /// `delay`. Returns `true` when the open state flipped.
    ///
    /// ```
    /// use martensite::widgets::hover_card::HoverCard;
    /// use std::time::Duration;
    ///
    /// let mut h = HoverCard::new("T", "B").with_delay(Duration::from_millis(100));
    /// h.set_hovered(true);
    /// assert!(!h.tick(Duration::from_millis(50)));
    /// assert!(h.tick(Duration::from_millis(60)));
    /// assert!(h.is_open());
    /// ```
    pub fn tick(&mut self, dt: Duration) -> bool {
        if !self.hovering || self.open {
            return false;
        }
        self.elapsed += dt.as_secs_f32();
        if self.elapsed * 1000.0 >= self.delay.as_millis() as f32 {
            self.open = true;
            self.opened = true;
            return true;
        }
        false
    }

    /// Host-side hover override (e.g. when the trigger is a
    /// different widget than the card's bounds).
    ///
    /// ```
    /// use martensite::widgets::hover_card::HoverCard;
    ///
    /// let mut h = HoverCard::new("T", "B");
    /// h.set_hovered(true);
    /// h.set_hovered(false);
    /// assert!(!h.is_open());
    /// ```
    pub fn set_hovered(&mut self, hovering: bool) {
        if !hovering {
            self.elapsed = 0.0;
            if self.open {
                self.open = false;
                self.closed = true;
            }
        }
        self.hovering = hovering;
    }

    /// Forces the card shut.
    ///
    /// ```
    /// use martensite::widgets::hover_card::HoverCard;
    ///
    /// let mut h = HoverCard::new("T", "B");
    /// h.dismiss();
    /// assert!(!h.is_open());
    /// ```
    pub fn dismiss(&mut self) {
        if self.open {
            self.closed = true;
        }
        self.open = false;
        self.elapsed = 0.0;
    }

    /// Drains the opened flag.
    ///
    /// ```
    /// use martensite::widgets::hover_card::HoverCard;
    ///
    /// let mut h = HoverCard::new("T", "B");
    /// assert!(!h.take_opened());
    /// ```
    pub fn take_opened(&mut self) -> bool {
        std::mem::take(&mut self.opened)
    }

    /// Drains the closed flag.
    ///
    /// ```
    /// use martensite::widgets::hover_card::HoverCard;
    ///
    /// let mut h = HoverCard::new("T", "B");
    /// assert!(!h.take_closed());
    /// ```
    pub fn take_closed(&mut self) -> bool {
        std::mem::take(&mut self.closed)
    }
}

impl Widget for HoverCard {
    fn measure(&mut self, cx: &mut LayoutContext, constraints: LayoutConstraints) -> Vec2 {
        let s = cx.scale;
        Vec2::new(
            (240.0 * s).min(constraints.max_size.x.max(0.0)),
            ((TITLE_PT + BODY_PT * 1.4 + PAD_PT * 2.0 + 8.0) * s)
                .min(constraints.max_size.y.max(0.0)),
        )
    }

    fn min_render(&self) -> RenderMinimum {
        RenderMinimum::new(Vec2::new(140.0, 60.0)).with_policy(UnderflowPolicy::Lint)
    }

    fn layout(&mut self, cx: &mut LayoutContext, bounds: Rect) {
        self.bounds = bounds;
        self.scale = cx.scale;
    }

    fn accessibility(&self, node: &mut AccessKitNode) {
        node.set_role(accesskit::Role::Tooltip);
        node.set_label(self.title.clone());
        node.set_value(self.body.clone());
    }

    fn event(&mut self, cx: &mut EventContext) -> EventResponse {
        match cx.event {
            WidgetEvent::PointerEnter => {
                self.set_hovered(true);
                EventResponse::Ignored
            }
            WidgetEvent::PointerLeave => {
                let was_open = self.open;
                self.set_hovered(false);
                if was_open {
                    EventResponse::RequestRepaint
                } else {
                    EventResponse::Ignored
                }
            }
            _ => EventResponse::Ignored,
        }
    }

    fn paint(&self, cx: &mut PaintContext) {
        if !self.open {
            return;
        }
        let s = cx.scale;
        let painter = crate::text_paint::resolve_painter(&self.text_painter, cx.text_painter);
        let b = self.bounds;
        cx.list.push_fill_shape(
            kurbo::Rect::new(
                f64::from(b.min_x()),
                f64::from(b.min_y()),
                f64::from(b.max_x()),
                f64::from(b.max_y()),
            ),
            &martensite_core::shape::Shape::rounded(8.0 * s),
            cx.color(TokenKey::SurfaceColor, FACE),
        );
        let clip = kurbo::Rect::new(
            f64::from(b.min_x()),
            f64::from(b.min_y()),
            f64::from(b.max_x()),
            f64::from(b.max_y()),
        );
        crate::text_paint::paint_label_clipped(
            painter,
            cx.list,
            clip,
            kurbo::Point::new(
                f64::from(b.min_x() + PAD_PT * s),
                f64::from(b.min_y() + PAD_PT * s + TITLE_PT * s * 0.8),
            ),
            &self.title,
            TITLE_PT * s,
            cx.color(TokenKey::TextColor, TEXT),
        );
        crate::text_paint::paint_label_clipped(
            painter,
            cx.list,
            clip,
            kurbo::Point::new(
                f64::from(b.min_x() + PAD_PT * s),
                f64::from(b.min_y() + PAD_PT * s + TITLE_PT * s * 1.4 + BODY_PT * s),
            ),
            &self.body,
            BODY_PT * s,
            cx.color(TokenKey::TextMutedColor, MUTED_FG),
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use martensite_core::HotNode;

    fn fixture() -> HoverCard {
        HoverCard::new("Ana", "@ana — maintainer").with_delay(Duration::from_millis(100))
    }

    #[test]
    fn opens_after_delay() {
        let mut h = fixture();
        h.set_hovered(true);
        assert!(!h.tick(Duration::from_millis(60)));
        assert!(!h.is_open());
        assert!(h.tick(Duration::from_millis(50)));
        assert!(h.is_open());
        assert!(h.take_opened());
    }

    #[test]
    fn leave_before_delay_never_opens() {
        let mut h = fixture();
        h.set_hovered(true);
        h.tick(Duration::from_millis(60));
        h.set_hovered(false);
        assert!(!h.tick(Duration::from_millis(200)));
        assert!(!h.is_open());
    }

    #[test]
    fn leave_closes_and_flags() {
        let mut h = fixture();
        h.set_hovered(true);
        h.tick(Duration::from_millis(200));
        assert!(h.is_open());
        h.set_hovered(false);
        assert!(!h.is_open());
        assert!(h.take_closed());
    }

    #[test]
    fn paint_hidden_until_open() {
        let mut h = fixture();
        let mut hot = HotNode::default();
        let mut lcx = LayoutContext {
            hot: &mut hot,
            scale: 1.0,
        };
        h.layout(&mut lcx, Rect::new(0.0, 0.0, 240.0, 70.0));
        let theme = martensite_theme::Theme::new("test");
        let mut list = martensite_core::PaintList::default();
        h.paint(&mut PaintContext {
            list: &mut list,
            bounds: h.bounds,
            theme: &theme,
            scale: 1.0,
            text_painter: None,
        });
        assert!(list.is_empty());
        h.set_hovered(true);
        h.tick(Duration::from_millis(200));
        h.paint(&mut PaintContext {
            list: &mut list,
            bounds: h.bounds,
            theme: &theme,
            scale: 1.0,
            text_painter: None,
        });
        assert!(!list.is_empty());
    }
}
