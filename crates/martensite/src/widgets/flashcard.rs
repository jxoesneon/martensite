//! `Flashcard` — a two-faced flip card (Anki / quiz-deck idiom).
//!
//! A centered face shows the front text; click or Space flips to
//! the back (the answer) and parks the new side in
//! [`Flashcard::take_flipped`]. `r` returns to the front. The deck
//! host owns grading — the card just flips. A subtle two-tone face
//! treatment distinguishes front from back.
//!
//! Distinct from [`Card`](crate::widgets::Card) containers: the
//! card owns its two text faces, no child wiring needed.
//!
//! # Examples
//!
//! ```
//! use martensite::widgets::flashcard::Flashcard;
//!
//! let c = Flashcard::new("capital of France", "Paris");
//! assert!(!c.is_flipped());
//! ```

use accesskit::Node as AccessKitNode;
use glam::Vec2;
use martensite_core::{
    EventContext, EventResponse, LayoutConstraints, LayoutContext, PaintContext, PointerButton,
    Rect, RenderMinimum, UnderflowPolicy, Widget, WidgetEvent,
};
use martensite_theme::TokenKey;

use crate::text_paint::SharedTextPainter;

const W_PT: f32 = 260.0;
const H_PT: f32 = 150.0;
const FACE_PT: f32 = 20.0;
const TAG_PT: f32 = 10.0;

const FRONT: [u8; 4] = [52, 54, 62, 255];
const BACK: [u8; 4] = [44, 58, 52, 255];
const EDGE: [u8; 4] = [90, 92, 100, 255];
const FG: [u8; 4] = [235, 237, 243, 255];
const DIM: [u8; 4] = [140, 142, 150, 255];

/// A flip card — see the module docs.
///
/// ```
/// use martensite::widgets::flashcard::Flashcard;
///
/// assert_eq!(Flashcard::new("Q", "A").front(), "Q");
/// ```
pub struct Flashcard {
    /// Accessibility label.
    pub label: String,
    front_text: String,
    back_text: String,
    flipped: bool,
    out: Option<bool>,
    bounds: Rect,
    scale: f32,
    text_painter: Option<SharedTextPainter>,
}

impl std::fmt::Debug for Flashcard {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Flashcard")
            .field("flipped", &self.flipped)
            .finish()
    }
}

impl Flashcard {
    /// A card showing `front` with `back` on the reverse.
    ///
    /// ```
    /// use martensite::widgets::flashcard::Flashcard;
    ///
    /// let c = Flashcard::new("2+2", "4");
    /// assert_eq!(c.back(), "4");
    /// ```
    pub fn new(front: impl Into<String>, back: impl Into<String>) -> Self {
        Self {
            label: "Flashcard".to_string(),
            front_text: front.into(),
            back_text: back.into(),
            flipped: false,
            out: None,
            bounds: Rect::new(0.0, 0.0, 0.0, 0.0),
            scale: 1.0,
            text_painter: None,
        }
    }

    /// Accessibility label.
    ///
    /// ```
    /// use martensite::widgets::flashcard::Flashcard;
    ///
    /// assert_eq!(Flashcard::new("Q", "A").label("Card 1").label, "Card 1");
    /// ```
    pub fn label(mut self, label: impl Into<String>) -> Self {
        self.label = label.into();
        self
    }

    /// Starts on the back face.
    ///
    /// ```
    /// use martensite::widgets::flashcard::Flashcard;
    ///
    /// assert!(Flashcard::new("Q", "A").flipped(true).is_flipped());
    /// ```
    pub fn flipped(mut self, flipped: bool) -> Self {
        self.flipped = flipped;
        self
    }

    /// Optional painter override (tests / headless).
    pub fn with_text_painter(mut self, painter: SharedTextPainter) -> Self {
        self.text_painter = Some(painter);
        self
    }

    /// The front text.
    ///
    /// ```
    /// use martensite::widgets::flashcard::Flashcard;
    ///
    /// assert_eq!(Flashcard::new("Q", "A").front(), "Q");
    /// ```
    pub fn front(&self) -> &str {
        &self.front_text
    }

    /// The back text.
    ///
    /// ```
    /// use martensite::widgets::flashcard::Flashcard;
    ///
    /// assert_eq!(Flashcard::new("Q", "A").back(), "A");
    /// ```
    pub fn back(&self) -> &str {
        &self.back_text
    }

    /// The currently visible text.
    ///
    /// ```
    /// use martensite::widgets::flashcard::Flashcard;
    ///
    /// assert_eq!(Flashcard::new("Q", "A").flipped(true).shown(), "A");
    /// ```
    pub fn shown(&self) -> &str {
        if self.flipped {
            &self.back_text
        } else {
            &self.front_text
        }
    }

    /// Whether the back face is showing.
    ///
    /// ```
    /// use martensite::widgets::flashcard::Flashcard;
    ///
    /// assert!(!Flashcard::new("Q", "A").is_flipped());
    /// ```
    pub fn is_flipped(&self) -> bool {
        self.flipped
    }

    /// Flips the card and parks the new side.
    ///
    /// ```
    /// use martensite::widgets::flashcard::Flashcard;
    ///
    /// let mut c = Flashcard::new("Q", "A");
    /// c.flip();
    /// assert!(c.is_flipped());
    /// assert_eq!(c.take_flipped(), Some(true));
    /// ```
    pub fn flip(&mut self) {
        self.flipped = !self.flipped;
        self.out = Some(self.flipped);
    }

    /// Shows a specific face (`false` = front).
    ///
    /// ```
    /// use martensite::widgets::flashcard::Flashcard;
    ///
    /// let mut c = Flashcard::new("Q", "A").flipped(true);
    /// c.set_flipped(false);
    /// assert!(!c.is_flipped());
    /// ```
    pub fn set_flipped(&mut self, flipped: bool) {
        if self.flipped != flipped {
            self.flipped = flipped;
            self.out = Some(flipped);
        }
    }

    /// Updates the card text (deck navigation).
    ///
    /// ```
    /// use martensite::widgets::flashcard::Flashcard;
    ///
    /// let mut c = Flashcard::new("Q1", "A1");
    /// c.set_card("Q2", "A2");
    /// assert_eq!(c.front(), "Q2");
    /// assert!(!c.is_flipped()); // new card resets to front
    /// ```
    pub fn set_card(&mut self, front: impl Into<String>, back: impl Into<String>) {
        self.front_text = front.into();
        self.back_text = back.into();
        self.flipped = false;
    }

    /// Drains the last flip (`true` = now showing the back).
    ///
    /// ```
    /// use martensite::widgets::flashcard::Flashcard;
    ///
    /// assert_eq!(Flashcard::new("Q", "A").take_flipped(), None);
    /// ```
    pub fn take_flipped(&mut self) -> Option<bool> {
        self.out.take()
    }
}

impl Widget for Flashcard {
    fn measure(&mut self, cx: &mut LayoutContext, constraints: LayoutConstraints) -> Vec2 {
        Vec2::new(
            cx.pt(W_PT).min(constraints.max_size.x.max(0.0)),
            cx.pt(H_PT).min(constraints.max_size.y.max(0.0)),
        )
    }

    fn min_render(&self) -> RenderMinimum {
        RenderMinimum::new(Vec2::new(100.0, 60.0)).with_policy(UnderflowPolicy::Lint)
    }

    fn layout(&mut self, cx: &mut LayoutContext, bounds: Rect) {
        self.bounds = bounds;
        self.scale = cx.scale;
    }

    fn accessibility(&self, node: &mut AccessKitNode) {
        node.set_role(accesskit::Role::Button);
        node.set_label(format!(
            "{} — {}",
            self.label,
            if self.flipped { "answer" } else { "question" }
        ));
        node.set_value(self.shown().to_string());
        node.add_action(accesskit::Action::Click);
    }

    fn event(&mut self, cx: &mut EventContext) -> EventResponse {
        match cx.event {
            WidgetEvent::PointerPressed {
                button: PointerButton::Primary,
                position,
                ..
            } => {
                if self.bounds.contains(*position) {
                    self.flip();
                    return EventResponse::RequestRepaint;
                }
                EventResponse::Ignored
            }
            WidgetEvent::KeyPressed { key, .. } => match key.as_str() {
                " " | "Enter" => {
                    self.flip();
                    EventResponse::RequestRepaint
                }
                "r" | "R" | "Escape" => {
                    self.set_flipped(false);
                    EventResponse::RequestRepaint
                }
                _ => EventResponse::Ignored,
            },
            _ => EventResponse::Ignored,
        }
    }

    fn paint(&self, cx: &mut PaintContext) {
        let s = cx.scale;
        let b = cx.bounds;
        let krect = kurbo::Rect::new(
            f64::from(b.min_x()),
            f64::from(b.min_y()),
            f64::from(b.max_x()),
            f64::from(b.max_y()),
        );
        let fill = if self.flipped {
            cx.color(TokenKey::SecondaryColor, BACK)
        } else {
            cx.color(TokenKey::SurfaceColor, FRONT)
        };
        let shape = &martensite_core::shape::Shape::rounded(8.0 * s);
        cx.list.push_fill_shape(krect, shape, fill);
        cx.list
            .push_stroke_shape(krect, shape, s, cx.color(TokenKey::BorderColor, EDGE));

        let painter = crate::text_paint::resolve_painter(&self.text_painter, cx.text_painter);
        // Side tag.
        let tag = if self.flipped { "BACK" } else { "FRONT" };
        let tag_size = TAG_PT * s;
        let dim = cx.color(TokenKey::TextMutedColor, DIM);
        crate::text_paint::paint_label_clipped(
            painter,
            cx.list,
            krect,
            kurbo::Point::new(
                f64::from(b.min_x() + 10.0 * s),
                f64::from(b.min_y() + 8.0 * s),
            ),
            tag,
            tag_size,
            dim,
        );
        // Face text — centered.
        let size = FACE_PT * s;
        let face = self.shown();
        let fg = cx.color(TokenKey::TextColor, FG);
        let w = painter
            .and_then(|p| p.measure_text(face, size))
            .unwrap_or(face.chars().count() as f32 * size * 0.55);
        crate::text_paint::paint_label_clipped(
            painter,
            cx.list,
            krect,
            kurbo::Point::new(
                f64::from(b.min_x() + (b.width() - w).max(0.0) / 2.0),
                f64::from(b.min_y() + (b.height() - size * 1.2).max(0.0) / 2.0),
            ),
            face,
            size,
            fg,
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use martensite_core::HotNode;

    fn laid_out(c: &mut Flashcard) {
        let mut hot = HotNode::default();
        let mut cx = LayoutContext {
            hot: &mut hot,
            scale: 1.0,
        };
        c.layout(&mut cx, Rect::new(0.0, 0.0, 260.0, 150.0));
    }

    fn ev(c: &mut Flashcard, e: &WidgetEvent) -> EventResponse {
        c.event(&mut EventContext {
            event: e,
            bounds: c.bounds,
            scale: 1.0,
        })
    }

    #[test]
    fn click_flips() {
        let mut c = Flashcard::new("Q", "A");
        laid_out(&mut c);
        ev(
            &mut c,
            &WidgetEvent::PointerPressed {
                button: PointerButton::Primary,
                position: Vec2::new(130.0, 75.0),
                count: 1,
            },
        );
        assert!(c.is_flipped());
        assert_eq!(c.shown(), "A");
        assert_eq!(c.take_flipped(), Some(true));
    }

    #[test]
    fn space_flips_and_r_resets() {
        let mut c = Flashcard::new("Q", "A");
        laid_out(&mut c);
        ev(
            &mut c,
            &WidgetEvent::KeyPressed {
                key: " ".to_string(),
                repeat: false,
            },
        );
        assert!(c.is_flipped());
        ev(
            &mut c,
            &WidgetEvent::KeyPressed {
                key: "r".to_string(),
                repeat: false,
            },
        );
        assert!(!c.is_flipped());
    }

    #[test]
    fn set_card_resets_face() {
        let mut c = Flashcard::new("Q1", "A1").flipped(true);
        c.set_card("Q2", "A2");
        assert_eq!(c.shown(), "Q2");
        assert!(!c.is_flipped());
    }

    #[test]
    fn outside_click_ignored() {
        let mut c = Flashcard::new("Q", "A");
        laid_out(&mut c);
        assert_eq!(
            ev(
                &mut c,
                &WidgetEvent::PointerPressed {
                    button: PointerButton::Primary,
                    position: Vec2::new(500.0, 500.0),
                    count: 1,
                }
            ),
            EventResponse::Ignored
        );
        assert!(!c.is_flipped());
    }

    #[test]
    fn paint_without_painter() {
        let mut c = Flashcard::new("Question", "Answer").flipped(true);
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
