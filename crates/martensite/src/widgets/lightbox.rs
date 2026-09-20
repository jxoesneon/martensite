//! `Lightbox` — a fullscreen media overlay: dim backdrop, centered
//! [`Thumbnail`] item, ‹ › navigation, an × close, a `3 / 8`
//! counter, and a caption line (photo-viewer idiom).
//!
//! Unlike [`Dialog`](crate::widgets::Dialog) — a generic modal
//! container — a lightbox is the *media browsing* idiom: backdrop
//! clicks and `Escape` park [`Lightbox::take_closed`]; arrows and
//! edge buttons step the item and park
//! [`Lightbox::take_navigated`]. The host decides visibility by
//! laying the widget out (or not).
//!
//! # Examples
//!
//! ```
//! use martensite::widgets::lightbox::Lightbox;
//! use martensite::widgets::Thumbnail;
//!
//! let l = Lightbox::new()
//!     .item(Thumbnail::new("a", [200, 0, 0, 255]))
//!     .item(Thumbnail::new("b", [0, 200, 0, 255]));
//! assert_eq!(l.item_count(), 2);
//! ```

use accesskit::Node as AccessKitNode;
use glam::Vec2;
use martensite_core::{
    EventContext, EventResponse, LayoutConstraints, LayoutContext, PaintContext, PointerButton,
    Rect, RenderMinimum, UnderflowPolicy, Widget, WidgetEvent,
};
use martensite_theme::TokenKey;

use crate::text_paint::SharedTextPainter;
use crate::widgets::Thumbnail;

const BTN_PT: f32 = 40.0;
const CLOSE_PT: f32 = 32.0;
const PAD_PT: f32 = 16.0;
const FONT_PT: f32 = 13.0;
const CAPTION_PT: f32 = 12.0;

const DIM: [u8; 4] = [0, 0, 0, 190];
const BTN_FACE: [u8; 4] = [50, 52, 60, 200];
const TEXT: [u8; 4] = [235, 237, 240, 255];
const MUTED: [u8; 4] = [170, 175, 185, 255];

/// A media overlay — see the module docs.
///
/// ```
/// use martensite::widgets::lightbox::Lightbox;
///
/// assert_eq!(Lightbox::new().item_count(), 0);
/// ```
pub struct Lightbox {
    /// Accessibility label.
    pub label: String,
    /// Show the caption line.
    pub show_caption: bool,
    /// Show the counter.
    pub show_counter: bool,
    items: Vec<Thumbnail>,
    index: usize,
    closed: bool,
    navigated: Option<usize>,
    item_rect: Rect,
    prev_rect: Rect,
    next_rect: Rect,
    close_rect: Rect,
    text_painter: Option<SharedTextPainter>,
    bounds: Rect,
    scale: f32,
}

impl std::fmt::Debug for Lightbox {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Lightbox")
            .field("items", &self.items.len())
            .field("index", &self.index)
            .finish()
    }
}

impl Default for Lightbox {
    fn default() -> Self {
        Self::new()
    }
}

impl Lightbox {
    /// Empty lightbox.
    ///
    /// ```
    /// use martensite::widgets::lightbox::Lightbox;
    ///
    /// assert_eq!(Lightbox::new().item_count(), 0);
    /// ```
    pub fn new() -> Self {
        Self {
            label: "Lightbox".to_string(),
            show_caption: true,
            show_counter: true,
            items: Vec::new(),
            index: 0,
            closed: false,
            navigated: None,
            item_rect: Rect::new(0.0, 0.0, 0.0, 0.0),
            prev_rect: Rect::new(0.0, 0.0, 0.0, 0.0),
            next_rect: Rect::new(0.0, 0.0, 0.0, 0.0),
            close_rect: Rect::new(0.0, 0.0, 0.0, 0.0),
            text_painter: None,
            bounds: Rect::new(0.0, 0.0, 0.0, 0.0),
            scale: 1.0,
        }
    }

    /// Appends an item.
    ///
    /// ```
    /// use martensite::widgets::lightbox::Lightbox;
    /// use martensite::widgets::Thumbnail;
    ///
    /// assert_eq!(Lightbox::new().item(Thumbnail::new("a", [1; 4])).item_count(), 1);
    /// ```
    pub fn item(mut self, item: Thumbnail) -> Self {
        self.items.push(item);
        self
    }

    /// Initial index.
    ///
    /// ```
    /// use martensite::widgets::lightbox::Lightbox;
    /// use martensite::widgets::Thumbnail;
    ///
    /// assert_eq!(Lightbox::new().item(Thumbnail::new("a", [1; 4])).index(0).current(), Some(0));
    /// ```
    pub fn index(mut self, index: usize) -> Self {
        self.index = index;
        self
    }

    /// Accessibility label.
    ///
    /// ```
    /// use martensite::widgets::lightbox::Lightbox;
    ///
    /// assert_eq!(Lightbox::new().label("Photos").label, "Photos");
    /// ```
    pub fn label(mut self, label: impl Into<String>) -> Self {
        self.label = label.into();
        self
    }

    /// Shared text painter for captions/counter.
    ///
    /// ```no_run
    /// use martensite::widgets::lightbox::Lightbox;
    /// # fn example(p: martensite::text_paint::SharedTextPainter) {
    /// let _l = Lightbox::new().with_text_painter(p);
    /// # }
    /// ```
    pub fn with_text_painter(mut self, painter: SharedTextPainter) -> Self {
        self.text_painter = Some(painter);
        self
    }

    /// Item count.
    ///
    /// ```
    /// use martensite::widgets::lightbox::Lightbox;
    ///
    /// assert_eq!(Lightbox::new().item_count(), 0);
    /// ```
    pub fn item_count(&self) -> usize {
        self.items.len()
    }

    /// Current item index.
    ///
    /// ```
    /// use martensite::widgets::lightbox::Lightbox;
    ///
    /// assert_eq!(Lightbox::new().current(), None);
    /// ```
    pub fn current(&self) -> Option<usize> {
        (!self.items.is_empty()).then(|| self.index.min(self.items.len() - 1))
    }

    /// Sets the index (host-driven).
    ///
    /// ```
    /// use martensite::widgets::lightbox::Lightbox;
    /// use martensite::widgets::Thumbnail;
    ///
    /// let mut l = Lightbox::new().item(Thumbnail::new("a", [1; 4])).item(Thumbnail::new("b", [1; 4]));
    /// l.set_index(1);
    /// assert_eq!(l.current(), Some(1));
    /// ```
    pub fn set_index(&mut self, index: usize) {
        if !self.items.is_empty() {
            self.index = index.min(self.items.len() - 1);
        }
    }

    /// Drains a close request (backdrop click, ×, Escape).
    ///
    /// ```
    /// use martensite::widgets::lightbox::Lightbox;
    ///
    /// let mut l = Lightbox::new();
    /// assert!(!l.take_closed());
    /// ```
    pub fn take_closed(&mut self) -> bool {
        std::mem::take(&mut self.closed)
    }

    /// Drains the last navigated index.
    ///
    /// ```
    /// use martensite::widgets::lightbox::Lightbox;
    ///
    /// let mut l = Lightbox::new();
    /// assert_eq!(l.take_navigated(), None);
    /// ```
    pub fn take_navigated(&mut self) -> Option<usize> {
        self.navigated.take()
    }

    /// Steps the index by `d`, wrapping, parking the new index.
    fn step(&mut self, d: isize) {
        if self.items.is_empty() {
            return;
        }
        let n = self.items.len() as isize;
        let next = ((self.index as isize + d) % n + n) % n;
        if next as usize != self.index {
            self.index = next as usize;
            self.navigated = Some(self.index);
        }
    }
}

impl Widget for Lightbox {
    fn measure(&mut self, _cx: &mut LayoutContext, constraints: LayoutConstraints) -> Vec2 {
        constraints.max_size
    }

    fn min_render(&self) -> RenderMinimum {
        RenderMinimum::new(Vec2::new(200.0, 150.0)).with_policy(UnderflowPolicy::Lint)
    }

    fn layout(&mut self, cx: &mut LayoutContext, bounds: Rect) {
        self.bounds = bounds;
        self.scale = cx.scale;
        let s = cx.scale;
        // Item: centered, square-ish, inset from edges.
        let inset = (PAD_PT * 2.0 + BTN_PT) * s;
        let w = (bounds.width() - inset * 2.0).max(0.0);
        let h = (bounds.height() - inset * 2.0).max(0.0);
        let side = w.min(h);
        self.item_rect = Rect::new(
            bounds.min_x() + (bounds.width() - side) / 2.0,
            bounds.min_y() + (bounds.height() - side) / 2.0,
            side,
            side,
        );
        let btn = BTN_PT * s;
        let cym = bounds.min_y() + bounds.height() / 2.0;
        self.prev_rect = Rect::new(bounds.min_x() + PAD_PT * s, cym - btn / 2.0, btn, btn);
        self.next_rect = Rect::new(bounds.max_x() - PAD_PT * s - btn, cym - btn / 2.0, btn, btn);
        let c = CLOSE_PT * s;
        self.close_rect = Rect::new(
            bounds.max_x() - PAD_PT * s - c,
            bounds.min_y() + PAD_PT * s,
            c,
            c,
        );
    }

    fn accessibility(&self, node: &mut AccessKitNode) {
        node.set_role(accesskit::Role::Dialog);
        node.set_label(self.label.clone());
        if let Some(i) = self.current() {
            node.set_value(format!("{} of {}", i + 1, self.items.len()));
        }
    }

    fn event(&mut self, cx: &mut EventContext) -> EventResponse {
        match cx.event {
            WidgetEvent::KeyPressed { key, .. } => match key.as_str() {
                "Escape" => {
                    self.closed = true;
                    EventResponse::Handled
                }
                "ArrowLeft" => {
                    self.step(-1);
                    EventResponse::RequestRepaint
                }
                "ArrowRight" => {
                    self.step(1);
                    EventResponse::RequestRepaint
                }
                _ => EventResponse::Ignored,
            },
            WidgetEvent::PointerReleased {
                button: PointerButton::Primary,
                position,
            } => {
                if self.close_rect.contains(*position) {
                    self.closed = true;
                    return EventResponse::Handled;
                }
                if self.prev_rect.contains(*position) {
                    self.step(-1);
                    return EventResponse::RequestRepaint;
                }
                if self.next_rect.contains(*position) {
                    self.step(1);
                    return EventResponse::RequestRepaint;
                }
                // Backdrop click closes; item clicks do nothing.
                if self.bounds.contains(*position) && !self.item_rect.contains(*position) {
                    self.closed = true;
                    return EventResponse::Handled;
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
        // Backdrop.
        cx.list.push_fill_rect(krect(self.bounds), DIM);
        // Item.
        if let Some(i) = self.current() {
            let item = &self.items[i];
            cx.list.push_fill_rect(krect(self.item_rect), item.color);
            // Caption.
            if self.show_caption {
                let o = kurbo::Point::new(
                    f64::from(self.item_rect.min_x()),
                    f64::from(self.item_rect.max_y() + FONT_PT * s),
                );
                crate::text_paint::paint_label(
                    painter,
                    cx.list,
                    o,
                    &item.label,
                    CAPTION_PT * s,
                    cx.color(TokenKey::TextColor, TEXT),
                );
            }
            // Counter.
            if self.show_counter {
                let counter = format!("{} / {}", i + 1, self.items.len());
                let w = painter
                    .and_then(|p| p.measure_text(&counter, FONT_PT * s))
                    .unwrap_or(counter.len() as f32 * FONT_PT * 0.6 * s);
                let o = kurbo::Point::new(
                    f64::from(self.bounds.min_x() + self.bounds.width() / 2.0 - w / 2.0),
                    f64::from(self.bounds.min_y() + PAD_PT * s + FONT_PT * s),
                );
                crate::text_paint::paint_label(
                    painter,
                    cx.list,
                    o,
                    &counter,
                    FONT_PT * s,
                    cx.color(TokenKey::TextMutedColor, MUTED),
                );
            }
        }
        // Nav + close buttons.
        let shape = martensite_core::shape::Shape::ELLIPSE;
        for (rect, glyph) in [
            (self.prev_rect, "‹"),
            (self.next_rect, "›"),
            (self.close_rect, "×"),
        ] {
            cx.list.push_fill_shape(krect(rect), &shape, BTN_FACE);
            let gsize = FONT_PT * 1.4 * s;
            let gw = painter
                .and_then(|p| p.measure_text(glyph, gsize))
                .unwrap_or(gsize * 0.5);
            let o = kurbo::Point::new(
                f64::from(rect.min_x() + (rect.width() - gw) / 2.0),
                f64::from(rect.min_y() + rect.height() / 2.0),
            );
            crate::text_paint::paint_label(
                painter,
                cx.list,
                o,
                glyph,
                gsize,
                cx.color(TokenKey::TextColor, TEXT),
            );
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use martensite_core::HotNode;

    fn fixture() -> Lightbox {
        Lightbox::new()
            .item(Thumbnail::new("a", [255, 0, 0, 255]))
            .item(Thumbnail::new("b", [0, 255, 0, 255]))
            .item(Thumbnail::new("c", [0, 0, 255, 255]))
    }

    fn laid_out(l: &mut Lightbox) {
        let mut hot = HotNode::default();
        let mut cx = LayoutContext {
            hot: &mut hot,
            scale: 1.0,
        };
        l.layout(&mut cx, Rect::new(0.0, 0.0, 640.0, 480.0));
    }

    fn ev(l: &mut Lightbox, e: &WidgetEvent) -> EventResponse {
        l.event(&mut EventContext {
            event: e,
            bounds: l.bounds,
            scale: 1.0,
        })
    }

    #[test]
    fn arrows_navigate_and_wrap() {
        let mut l = fixture();
        laid_out(&mut l);
        ev(
            &mut l,
            &WidgetEvent::KeyPressed {
                key: "ArrowRight".to_string(),
                repeat: false,
            },
        );
        assert_eq!(l.current(), Some(1));
        assert_eq!(l.take_navigated(), Some(1));
        // Wraps around.
        ev(
            &mut l,
            &WidgetEvent::KeyPressed {
                key: "ArrowLeft".to_string(),
                repeat: false,
            },
        );
        ev(
            &mut l,
            &WidgetEvent::KeyPressed {
                key: "ArrowLeft".to_string(),
                repeat: false,
            },
        );
        assert_eq!(l.current(), Some(2));
    }

    #[test]
    fn escape_closes() {
        let mut l = fixture();
        laid_out(&mut l);
        ev(
            &mut l,
            &WidgetEvent::KeyPressed {
                key: "Escape".to_string(),
                repeat: false,
            },
        );
        assert!(l.take_closed());
    }

    #[test]
    fn backdrop_click_closes_item_click_ignores() {
        let mut l = fixture();
        laid_out(&mut l);
        // Corner — inside bounds, outside item.
        ev(
            &mut l,
            &WidgetEvent::PointerReleased {
                button: PointerButton::Primary,
                position: Vec2::new(4.0, 470.0),
            },
        );
        assert!(l.take_closed());
        // Item click does not close.
        let mid = Vec2::new(
            (l.item_rect.min_x() + l.item_rect.max_x()) / 2.0,
            (l.item_rect.min_y() + l.item_rect.max_y()) / 2.0,
        );
        ev(
            &mut l,
            &WidgetEvent::PointerReleased {
                button: PointerButton::Primary,
                position: mid,
            },
        );
        assert!(!l.take_closed());
    }

    #[test]
    fn nav_buttons_step() {
        let mut l = fixture();
        laid_out(&mut l);
        let r = l.next_rect;
        ev(
            &mut l,
            &WidgetEvent::PointerReleased {
                button: PointerButton::Primary,
                position: Vec2::new((r.min_x() + r.max_x()) / 2.0, (r.min_y() + r.max_y()) / 2.0),
            },
        );
        assert_eq!(l.current(), Some(1));
    }

    #[test]
    fn paint_without_painter() {
        let mut l = fixture();
        laid_out(&mut l);
        let mut list = martensite_core::PaintList::default();
        let theme = martensite_theme::Theme::new("test");
        l.paint(&mut PaintContext {
            list: &mut list,
            bounds: l.bounds,
            theme: &theme,
            scale: 1.0,
            text_painter: None,
        });
        assert!(!list.is_empty());
    }
}
