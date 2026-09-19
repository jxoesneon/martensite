//! `Kbd` — keyboard-key cap chip.
//!
//! The `<kbd>` element / Ant `Keyboard` keycap: a small beveled box
//! around a key name (`⌘`, `Ctrl`, `Shift`, `F5`…), used inside docs,
//! shortcut hints, and menus. Purely presentational — combine with
//! `KeyCapture` for recording and `KeyMap` for dispatch.
//!
//! # Examples
//!
//! ```
//! use martensite::widgets::kbd::Kbd;
//!
//! let k = Kbd::new("⌘");
//! assert_eq!(k.text(), "⌘");
//! ```

use accesskit::Node as AccessKitNode;
use glam::Vec2;
use martensite_core::{
    EventContext, EventResponse, LayoutConstraints, LayoutContext, PaintContext, Rect,
    RenderMinimum, UnderflowPolicy, Widget,
};
use martensite_theme::TokenKey;

/// Cap fill.
const FILL: TokenKey = TokenKey::SurfaceColor;
/// Cap edge.
const EDGE: TokenKey = TokenKey::BorderColor;
/// Legend ink.
const TEXT: TokenKey = TokenKey::TextColor;
/// Horizontal padding inside the cap (logical points).
const PAD_X: f32 = 6.0;
/// Cap height (logical points).
const CAP_H: f32 = 20.0;
/// Bottom edge thickness suggesting key travel (logical points).
const TRAVEL: f32 = 1.5;

/// A keycap chip — see the module docs. Leaf widget, no children.
///
/// # Examples
///
/// ```
/// use martensite::widgets::kbd::Kbd;
/// use martensite::core::Widget;
///
/// let mut k = Kbd::new("Esc");
/// assert_eq!(k.child_count(), 0);
/// ```
pub struct Kbd {
    text: String,
    label: String,
    enabled: bool,
    text_painter: Option<crate::text_paint::SharedTextPainter>,
}

impl Kbd {
    /// A cap showing `text`.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite::widgets::kbd::Kbd;
    ///
    /// let k = Kbd::new("Ctrl");
    /// assert_eq!(k.text(), "Ctrl");
    /// ```
    pub fn new(text: impl Into<String>) -> Self {
        Self {
            text: text.into(),
            label: "key".into(),
            enabled: true,
            text_painter: None,
        }
    }

    /// Set the accessibility label (default `"key"`).
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite::widgets::kbd::Kbd;
    ///
    /// let k = Kbd::new("Q").label("shortcut key");
    /// ```
    pub fn label(mut self, label: impl Into<String>) -> Self {
        self.label = label.into();
        self
    }

    /// Enable or disable (dims the cap; default `true`).
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite::widgets::kbd::Kbd;
    ///
    /// let k = Kbd::new("x").enabled(false);
    /// ```
    pub fn enabled(mut self, enabled: bool) -> Self {
        self.enabled = enabled;
        self
    }

    /// Share a text painter. `SharedTextPainter` is not `Default`, so
    /// this builder is exercised indirectly through `paint`.
    pub fn with_text_painter(mut self, painter: crate::text_paint::SharedTextPainter) -> Self {
        self.text_painter = Some(painter);
        self
    }

    /// The legend text.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite::widgets::kbd::Kbd;
    ///
    /// assert_eq!(Kbd::new("F5").text(), "F5");
    /// ```
    pub fn text(&self) -> &str {
        &self.text
    }
}

impl Widget for Kbd {
    fn measure(&mut self, _cx: &mut LayoutContext, _constraints: LayoutConstraints) -> Vec2 {
        // No painter in LayoutContext — estimate at 0.6em/char (keycap
        // legends are short and mostly wide glyphs).
        let w = PAD_X * 2.0 + self.text.chars().count().max(1) as f32 * 11.0 * 0.6;
        Vec2::new(w.max(CAP_H), CAP_H)
    }

    fn layout(&mut self, _cx: &mut LayoutContext, _bounds: Rect) {}

    fn paint(&self, cx: &mut PaintContext) {
        let b = cx.bounds;
        let fill = cx.color(FILL, [248, 248, 250, 255]);
        let edge = cx.color(EDGE, [200, 202, 208, 255]);
        let ink = cx.color(TEXT, [40, 40, 46, 255]);
        let muted = cx.color(TokenKey::TextMutedColor, [150, 150, 158, 255]);
        let travel = cx.pt(TRAVEL);

        let face = kurbo::Rect::new(
            f64::from(b.min_x()),
            f64::from(b.min_y()),
            f64::from(b.max_x()),
            f64::from(b.max_y() - travel),
        );
        let shape = martensite_core::shape::Shape::rounded(cx.pt(4.0));
        // Travel edge under the face gives the keycap its 3-D hint.
        cx.list.push_fill_rect(
            kurbo::Rect::new(
                face.x0,
                f64::from(b.max_y() - travel),
                face.x1,
                f64::from(b.max_y()),
            ),
            edge,
        );
        cx.list.push_fill_shape(face, &shape, fill);
        cx.list.push_stroke_shape(face, &shape, cx.pt(1.0), edge);

        let painter = crate::text_paint::resolve_painter(&self.text_painter, cx.text_painter);
        let size = 11.0 * cx.scale;
        let w = painter
            .and_then(|p| p.measure_text(&self.text, size))
            .unwrap_or(size * self.text.chars().count() as f32 * 0.6);
        crate::text_paint::paint_label_clipped(
            painter,
            cx.list,
            face,
            kurbo::Point::new(
                face.x0 + (face.width() - f64::from(w)) * 0.5,
                face.y0 + face.height() * 0.72,
            ),
            &self.text,
            size,
            if self.enabled { ink } else { muted },
        );
    }

    fn event(&mut self, _cx: &mut EventContext) -> EventResponse {
        // Presentational — never interactive.
        EventResponse::Ignored
    }

    fn accessibility(&self, node: &mut AccessKitNode) {
        node.set_role(accesskit::Role::GenericContainer);
        node.set_label(format!("{} {}", self.label, self.text));
        if !self.enabled {
            node.set_disabled();
        }
    }

    fn min_render(&self) -> RenderMinimum {
        RenderMinimum::new(Vec2::new(CAP_H, CAP_H)).with_policy(UnderflowPolicy::Lint)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use martensite_core::HotNode;

    #[test]
    fn measure_scales_with_text() {
        let mut hot = HotNode::default();
        let mut cx = LayoutContext {
            hot: &mut hot,
            scale: 1.0,
        };
        let short = Kbd::new("x").measure(
            &mut cx,
            LayoutConstraints {
                min_size: Vec2::ZERO,
                max_size: Vec2::new(400.0, 400.0),
            },
        );
        let long = Kbd::new("Shift+Enter").measure(
            &mut cx,
            LayoutConstraints {
                min_size: Vec2::ZERO,
                max_size: Vec2::new(400.0, 400.0),
            },
        );
        assert!(long.x > short.x);
        assert_eq!(short.y, CAP_H);
    }

    #[test]
    fn presentational_never_handles_events() {
        let mut k = Kbd::new("x");
        let e = martensite_core::WidgetEvent::FocusGained;
        let mut cx = EventContext {
            event: &e,
            bounds: Rect::new(0.0, 0.0, 30.0, 20.0),
            scale: 1.0,
        };
        assert_eq!(k.event(&mut cx), EventResponse::Ignored);
    }
}
