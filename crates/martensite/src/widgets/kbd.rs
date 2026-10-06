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

    /// Estimated legend width at `size` px — the fallback `paint` and
    /// `measure` share, and `min_render` declares, so a squeezed
    /// allocation reads as underflow instead of silently clipping.
    /// One-char floor: an empty legend still gets a key-shaped cap.
    fn text_width_estimate(&self, size: f32) -> f32 {
        crate::text_paint::estimate_text_width_px(&self.text, size, 0.6).max(size * 0.6)
    }
}

impl Widget for Kbd {
    #[cfg(feature = "devtools-timemachine")]
    fn as_any_mut(&mut self) -> Option<&mut dyn std::any::Any> {
        Some(self)
    }

    fn measure(&mut self, cx: &mut LayoutContext, _constraints: LayoutConstraints) -> Vec2 {
        // Real glyph advance when the ambient measurer is installed —
        // the 0.6em estimate under-reads wide legends like `⌘`, which
        // then clip inside a cap sized by the estimate. Sizes are
        // logical points; the returned size must be device px.
        let text_px = cx
            .measure_text(&self.text, 11.0)
            .unwrap_or_else(|| self.text_width_estimate(11.0) * cx.scale);
        Vec2::new(
            (text_px + cx.pt(PAD_X * 2.0)).max(cx.pt(CAP_H)),
            cx.pt(CAP_H),
        )
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
        let shape = martensite_core::shape::Shape::squircle(cx.pt(4.0));
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
            .unwrap_or_else(|| self.text_width_estimate(size));
        // `paint_label_vcenter` clips to the face — a squeezed cap
        // (below `min_render`, so a lint flag) truncates inside its
        // chrome rather than painting the legend over neighbours.
        crate::text_paint::paint_label_vcenter(
            painter,
            cx.list,
            face,
            face.x0 + (face.width() - f64::from(w)) * 0.5,
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
        // The cap's whole purpose is the legend — a minimum narrower
        // than the text declares "fine" for a render that clips
        // mid-glyph. Declare the same width `measure` asks for.
        RenderMinimum::new(Vec2::new(
            (PAD_X * 2.0 + self.text_width_estimate(11.0)).max(CAP_H),
            CAP_H,
        ))
        .with_policy(UnderflowPolicy::Lint)
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

    #[test]
    fn min_render_tracks_legend() {
        // A cap whose legend needs ~75pt must not declare a 20pt
        // minimum — underflow consumers would treat a squeezed
        // allocation as adequate while the legend clips mid-glyph.
        let wide = Kbd::new("Shift+Enter").min_render().size;
        let narrow = Kbd::new("x").min_render().size;
        assert!(wide.x > narrow.x);
        assert_eq!(narrow.x, CAP_H);
        assert_eq!(wide.y, CAP_H);
    }

    #[test]
    fn squeezed_kbd_flags_text_truncation() {
        use martensite_core::{PaintList, Theme};
        let k = Kbd::new("Shift+Enter");
        // Squeeze to 30pt — the ~75pt legend cannot fit. The arena walk
        // wraps every widget in a scope; a bare `PaintList` attributes
        // stats to no node, so the test reproduces the wrapper.
        let bounds = Rect::new(0.0, 0.0, 30.0, 20.0);
        let mut list = PaintList::new();
        let theme = Theme::new("test");
        list.push_scope(None, "Kbd", kurbo::Rect::new(0.0, 0.0, 30.0, 20.0));
        k.paint(&mut PaintContext {
            list: &mut list,
            bounds,
            theme: &theme,
            scale: 1.0,
            text_painter: None,
        });
        list.pop_scope();
        let scene = martensite_design_lint::LintScene::from_paint_list(&list);
        let findings =
            martensite_design_lint::lint(&scene, &martensite_design_lint::LintConfig::default());
        for f in &findings.findings {
            eprintln!("FINDING {} {:?}", f.rule, f.severity);
        }
        assert!(
            findings
                .findings
                .iter()
                .any(|f| f.rule == "text-truncation"),
            "expected text-truncation on a squeezed Kbd"
        );
    }
}
