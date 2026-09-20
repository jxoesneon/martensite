//! `Copyable` — click-to-copy inline text (Ant
//! `Typography.Text copyable`, GitHub commit-hash chip idiom).
//!
//! The text renders with a trailing copy-icon button. Clicking it
//! parks the text in [`Copyable::take_copied`] for the host to push
//! to the clipboard, and flashes a check glyph for ~1.2 s (cleared
//! on [`Widget::tick`](martensite_core::widget::Widget::tick)).
//! `Enter`/`Space` copy when focused.
//!
//! # Examples
//!
//! ```
//! use martensite::widgets::copyable::Copyable;
//!
//! let c = Copyable::new("a1b2c3");
//! assert_eq!(c.text(), "a1b2c3");
//! ```

use accesskit::Node as AccessKitNode;
use glam::Vec2;
use martensite_core::{
    EventContext, EventResponse, LayoutConstraints, LayoutContext, PaintContext, PointerButton,
    Rect, RenderMinimum, UnderflowPolicy, Widget, WidgetEvent,
};
use martensite_theme::TokenKey;

use crate::text_paint::SharedTextPainter;

const PAD_PT: f32 = 6.0;
const ICON_PT: f32 = 14.0;
const GAP_PT: f32 = 4.0;
const FONT_PT: f32 = 12.0;
/// Seconds the ✓ flash stays visible.
const FLASH_S: f32 = 1.2;

const FACE: [u8; 4] = [40, 43, 52, 255];
const EDGE: [u8; 4] = [78, 82, 92, 255];
const TEXT_FG: [u8; 4] = [235, 237, 240, 255];
const MUTED_FG: [u8; 4] = [150, 154, 164, 255];
const OK: [u8; 4] = [70, 180, 100, 255];

/// The copyable text — see the module docs.
///
/// ```
/// use martensite::widgets::copyable::Copyable;
///
/// assert_eq!(Copyable::new("x").text(), "x");
/// ```
pub struct Copyable {
    /// Accessibility label.
    pub label: String,
    /// The text shown and copied.
    pub text: String,
    /// Tooltip-ish caption read by screen readers for the button.
    pub copy_label: String,
    copied: Option<String>,
    copied_flash: f32,
    icon_rect: Rect,
    text_painter: Option<SharedTextPainter>,
    bounds: Rect,
    scale: f32,
}

impl std::fmt::Debug for Copyable {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Copyable")
            .field("text", &self.text)
            .finish()
    }
}

impl Copyable {
    /// A copyable showing `text`.
    ///
    /// ```
    /// use martensite::widgets::copyable::Copyable;
    ///
    /// assert_eq!(Copyable::new("abc").text(), "abc");
    /// ```
    pub fn new(text: impl Into<String>) -> Self {
        Self {
            label: "Copyable text".to_string(),
            text: text.into(),
            copy_label: "Copy".to_string(),
            copied: None,
            copied_flash: 0.0,
            icon_rect: Rect::new(0.0, 0.0, 0.0, 0.0),
            text_painter: None,
            bounds: Rect::new(0.0, 0.0, 0.0, 0.0),
            scale: 1.0,
        }
    }

    /// The displayed text.
    ///
    /// ```
    /// use martensite::widgets::copyable::Copyable;
    ///
    /// assert_eq!(Copyable::new("a").text(), "a");
    /// ```
    pub fn text(&self) -> &str {
        &self.text
    }

    /// Replaces the text.
    ///
    /// ```
    /// use martensite::widgets::copyable::Copyable;
    ///
    /// let mut c = Copyable::new("a");
    /// c.set_text("b");
    /// assert_eq!(c.text(), "b");
    /// ```
    pub fn set_text(&mut self, text: impl Into<String>) {
        self.text = text.into();
    }

    /// Accessibility label.
    ///
    /// ```
    /// use martensite::widgets::copyable::Copyable;
    ///
    /// assert_eq!(Copyable::new("a").label("Commit").label, "Commit");
    /// ```
    pub fn label(mut self, label: impl Into<String>) -> Self {
        self.label = label.into();
        self
    }

    /// Shared text painter.
    ///
    /// ```no_run
    /// use martensite::widgets::copyable::Copyable;
    /// # fn example(p: martensite::text_paint::SharedTextPainter) {
    /// let _c = Copyable::new("a").with_text_painter(p);
    /// # }
    /// ```
    pub fn with_text_painter(mut self, painter: SharedTextPainter) -> Self {
        self.text_painter = Some(painter);
        self
    }

    /// Whether the ✓ flash is visible.
    ///
    /// ```
    /// use martensite::widgets::copyable::Copyable;
    ///
    /// assert!(!Copyable::new("a").is_flashing());
    /// ```
    pub fn is_flashing(&self) -> bool {
        self.copied_flash > 0.0
    }

    /// Drains the copied text.
    ///
    /// ```
    /// use martensite::widgets::copyable::Copyable;
    ///
    /// assert_eq!(Copyable::new("a").take_copied(), None);
    /// ```
    pub fn take_copied(&mut self) -> Option<String> {
        self.copied.take()
    }

    fn copy(&mut self) {
        self.copied = Some(self.text.clone());
        self.copied_flash = FLASH_S;
    }
}

impl Widget for Copyable {
    fn measure(&mut self, cx: &mut LayoutContext, constraints: LayoutConstraints) -> Vec2 {
        let s = cx.scale;
        let w = self.text.len() as f32 * FONT_PT * 0.55 * s
            + (PAD_PT * 2.0 + GAP_PT + ICON_PT + PAD_PT) * s;
        Vec2::new(
            w.min(constraints.max_size.x.max(0.0)),
            ((FONT_PT + PAD_PT * 2.0) * s).min(constraints.max_size.y.max(0.0)),
        )
    }

    fn min_render(&self) -> RenderMinimum {
        RenderMinimum::new(Vec2::new(48.0, 18.0)).with_policy(UnderflowPolicy::Lint)
    }

    fn layout(&mut self, cx: &mut LayoutContext, bounds: Rect) {
        self.bounds = bounds;
        self.scale = cx.scale;
        let s = cx.scale;
        let ic = ICON_PT * s;
        self.icon_rect = Rect::new(
            bounds.max_x() - PAD_PT * s - ic,
            bounds.min_y() + (bounds.height() - ic) / 2.0,
            ic,
            ic,
        );
    }

    fn accessibility(&self, node: &mut AccessKitNode) {
        node.set_role(accesskit::Role::Button);
        node.set_label(format!("{}: {}", self.copy_label, self.text));
        node.add_action(accesskit::Action::Click);
        node.add_action(accesskit::Action::Focus);
    }

    fn event(&mut self, cx: &mut EventContext) -> EventResponse {
        match cx.event {
            WidgetEvent::PointerReleased {
                button: PointerButton::Primary,
                position,
            } => {
                if self.icon_rect.contains(*position) || self.bounds.contains(*position) {
                    self.copy();
                    return EventResponse::RequestRepaint;
                }
                EventResponse::Ignored
            }
            WidgetEvent::KeyPressed { key, .. } if matches!(key.as_str(), "Enter" | "Space") => {
                self.copy();
                EventResponse::RequestRepaint
            }
            WidgetEvent::SemanticAction(martensite_core::SemanticAction::Click) => {
                self.copy();
                EventResponse::RequestRepaint
            }
            _ => EventResponse::Ignored,
        }
    }

    fn tick(&mut self, dt: std::time::Duration) -> bool {
        if self.copied_flash > 0.0 {
            self.copied_flash = (self.copied_flash - dt.as_secs_f32()).max(0.0);
            return self.copied_flash > 0.0;
        }
        false
    }

    fn paint(&self, cx: &mut PaintContext) {
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
            &martensite_core::shape::Shape::rounded(4.0 * s),
            cx.color(TokenKey::SurfaceColor, FACE),
        );
        cx.list.push_stroke_rect(
            kurbo::Rect::new(
                f64::from(b.min_x()),
                f64::from(b.min_y()),
                f64::from(b.max_x()),
                f64::from(b.max_y()),
            ),
            1.0,
            EDGE,
        );
        let pad = PAD_PT * s;
        let fs = FONT_PT * s;
        crate::text_paint::paint_label(
            painter,
            cx.list,
            kurbo::Point::new(
                f64::from(b.min_x() + pad),
                f64::from(b.min_y() + b.height() / 2.0 + fs * 0.35),
            ),
            &self.text,
            fs,
            cx.color(TokenKey::TextColor, TEXT_FG),
        );
        // Copy icon: overlapping squares, or ✓ while flashing.
        let r = self.icon_rect;
        if self.copied_flash > 0.0 {
            let mut p = kurbo::BezPath::new();
            p.move_to((
                f64::from(r.min_x() + r.width() * 0.2),
                f64::from(r.min_y() + r.height() * 0.55),
            ));
            p.line_to((
                f64::from(r.min_x() + r.width() * 0.45),
                f64::from(r.min_y() + r.height() * 0.8),
            ));
            p.line_to((
                f64::from(r.min_x() + r.width() * 0.85),
                f64::from(r.min_y() + r.height() * 0.25),
            ));
            cx.list.push_stroke_path(p, 1.6 * s, OK);
        } else {
            let q = r.width() * 0.28;
            let back = kurbo::Rect::new(
                f64::from(r.min_x()),
                f64::from(r.min_y() + q),
                f64::from(r.max_x() - q),
                f64::from(r.max_y()),
            );
            let front = kurbo::Rect::new(
                f64::from(r.min_x() + q),
                f64::from(r.min_y()),
                f64::from(r.max_x()),
                f64::from(r.max_y() - q),
            );
            cx.list.push_stroke_rect(back, 1.2 * s, MUTED_FG);
            cx.list
                .push_fill_rect(front, cx.color(TokenKey::SurfaceColor, FACE));
            cx.list.push_stroke_rect(front, 1.2 * s, MUTED_FG);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use martensite_core::HotNode;

    fn laid_out(c: &mut Copyable) {
        let mut hot = HotNode::default();
        let mut cx = LayoutContext {
            hot: &mut hot,
            scale: 1.0,
        };
        c.layout(&mut cx, Rect::new(0.0, 0.0, 140.0, 24.0));
    }

    #[test]
    fn click_parks_text() {
        let mut c = Copyable::new("a1b2c3");
        laid_out(&mut c);
        c.event(&mut EventContext {
            event: &WidgetEvent::PointerReleased {
                button: PointerButton::Primary,
                position: Vec2::new(10.0, 12.0),
            },
            bounds: c.bounds,
            scale: 1.0,
        });
        assert_eq!(c.take_copied().as_deref(), Some("a1b2c3"));
        assert_eq!(c.take_copied(), None);
        assert!(c.is_flashing());
    }

    #[test]
    fn flash_decays() {
        let mut c = Copyable::new("x");
        c.copied_flash = FLASH_S;
        assert!(c.tick(std::time::Duration::from_millis(600)));
        assert!(!c.tick(std::time::Duration::from_secs(2)));
    }

    #[test]
    fn paint_without_painter() {
        let mut c = Copyable::new("hash");
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
