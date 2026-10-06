//! Floating action button — a circular overlay button pinned over
//! content (M3 FAB, Ant `FloatButton`, `BackTop`).
//!
//! A leaf meant for [`Stack`] layering: position it via its bounds
//! (the consumer decides the corner/margin), or use
//! [`FloatButton::back_top`] for the scroll-to-top idiom — a
//! circular arrow-up FAB that parks a `take_activated` request the
//! consumer maps onto a `ScrollView`.
//!
//! # Examples
//!
//! ```
//! use martensite::widgets::FloatButton;
//!
//! let mut f = FloatButton::new("+");
//! assert!(!f.take_activated());
//! ```

use accesskit::Node as AccessKitNode;
use glam::Vec2;
use martensite_core::{
    EventContext, EventResponse, LayoutConstraints, LayoutContext, PaintContext, Rect,
    RenderMinimum, UnderflowPolicy, Widget, WidgetEvent,
};
use martensite_theme::TokenKey;

use crate::text_paint::paint_label_vcenter;
use crate::widgets::morph_icon::MorphIcon;

/// Diameter in points.
const SIZE_PT: f32 = 44.0;
/// Stroke-icon box inside the circle, logical points.
const ICON_PT: f32 = 20.0;

/// Floating action button.
///
/// # Examples
///
/// ```
/// use martensite::widgets::FloatButton;
///
/// assert_eq!(FloatButton::new("+").text(), "+");
/// ```
pub struct FloatButton {
    /// Accessibility label override.
    pub label: Option<String>,
    /// Whether input reaches the button.
    pub enabled: bool,
    /// Whether the button is visible (BackTop hides until scrolled).
    pub visible: bool,
    text: String,
    pressed: bool,
    hover: bool,
    activated: bool,
    bounds: Rect,
    /// Optional hosted [`MorphIcon`] painted in place of `text` — a
    /// real internal child, so it ticks with the arena and morphs.
    icon: Option<MorphIcon>,
    /// Bounds assigned to `icon`, in widget space.
    icon_rect: Rect,
    text_painter: Option<crate::text_paint::SharedTextPainter>,
}

impl FloatButton {
    /// Creates a FAB with `text` — a namespaced icon name
    /// (`"status.plus"`, …) paints as a vector icon through the
    /// ambient icon family; anything else is a short text glyph.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite::widgets::FloatButton;
    ///
    /// assert_eq!(FloatButton::new("+").text(), "+");
    /// ```
    pub fn new(text: impl Into<String>) -> Self {
        Self {
            label: None,
            enabled: true,
            visible: true,
            text: text.into(),
            pressed: false,
            hover: false,
            activated: false,
            bounds: Rect::default(),
            icon: None,
            icon_rect: Rect::default(),
            text_painter: None,
        }
    }

    /// The BackTop idiom — an arrow-up FAB, hidden by default; the
    /// consumer flips `visible` when the scroll offset leaves the top.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite::widgets::FloatButton;
    ///
    /// assert!(!FloatButton::back_top().visible);
    /// ```
    pub fn back_top() -> Self {
        let mut b = Self::new("arrow.up").icon_named("arrow.up");
        b.visible = false;
        b.label = Some("Back to top".to_string());
        b
    }

    /// Sets the accessibility label.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite::widgets::FloatButton;
    ///
    /// let f = FloatButton::new("+").label("Add");
    /// assert_eq!(f.label.as_deref(), Some("Add"));
    /// ```
    #[must_use]
    pub fn label(mut self, text: impl Into<String>) -> Self {
        self.label = Some(text.into());
        self
    }

    /// Sets whether input reaches the button.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite::widgets::FloatButton;
    ///
    /// assert!(!FloatButton::new("+").enabled(false).enabled);
    /// ```
    #[must_use]
    pub fn enabled(mut self, flag: bool) -> Self {
        self.enabled = flag;
        self
    }

    /// Sets visibility (BackTop pattern).
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite::widgets::FloatButton;
    ///
    /// assert!(!FloatButton::new("+").visible(false).visible);
    /// ```
    #[must_use]
    pub fn visible(mut self, flag: bool) -> Self {
        self.visible = flag;
        self
    }

    /// Shows or hides post-build (drives BackTop from scroll offset).
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite::widgets::FloatButton;
    ///
    /// let mut f = FloatButton::back_top();
    /// f.set_visible(true);
    /// assert!(f.visible);
    /// ```
    pub fn set_visible(&mut self, flag: bool) {
        self.visible = flag;
    }

    /// Shares a [`crate::text_paint::TextPainter`] for the glyph.
    #[must_use]
    pub fn with_text_painter(mut self, painter: crate::text_paint::SharedTextPainter) -> Self {
        self.text_painter = Some(painter);
        self
    }

    /// Replaces the text glyph with a hosted [`MorphIcon`] stroke icon
    /// — `d` is SVG path data on the 24-unit icon grid (see
    /// [`crate::icons::builtin`](mod@crate::icons::builtin)). The icon is a real internal child:
    /// it ticks with the arena and morphs via
    /// [`icon_widget_mut`](Self::icon_widget_mut). It paints on the
    /// accent face in the button's ink; a `d` the icon engine rejects
    /// falls back to the text glyph rather than failing the build.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite::widgets::FloatButton;
    ///
    /// let f = FloatButton::new("+")
    ///     .icon_named("status.plus")
    ///     .label("Add");
    /// assert!(f.icon_widget().is_some());
    /// ```
    #[must_use]
    pub fn icon_d(mut self, d: &str) -> Self {
        if let Ok(icon) = MorphIcon::icon(d) {
            self.icon = Some(icon.decorative(true).ink([255, 255, 255, 255]));
        }
        self
    }

    /// [`icon_d`](Self::icon_d) resolving `name` through the ambient
    /// icon family ([`icons::resolve_icon`](crate::icons::resolve_icon))
    /// — `"status.plus"`, `"edit.pen"`, … An unknown name keeps the
    /// text glyph — same fallback contract as a rejected `d`.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite::widgets::FloatButton;
    ///
    /// assert!(FloatButton::new("+").icon_named("status.plus").icon_widget().is_some());
    /// assert!(FloatButton::new("+").icon_named("bogus.name").icon_widget().is_none());
    /// ```
    #[must_use]
    pub fn icon_named(mut self, name: &str) -> Self {
        if let Ok(icon) = MorphIcon::named(name) {
            self.icon = Some(icon.decorative(true).ink([255, 255, 255, 255]));
        }
        self
    }

    /// The hosted stroke icon, if one is installed. Mutate through
    /// [`icon_widget_mut`](Self::icon_widget_mut) to morph it.
    #[must_use]
    pub fn icon_widget(&self) -> Option<&MorphIcon> {
        self.icon.as_ref()
    }

    /// Mutable twin of [`icon_widget`](Self::icon_widget).
    pub fn icon_widget_mut(&mut self) -> Option<&mut MorphIcon> {
        self.icon.as_mut()
    }

    /// The button text.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite::widgets::FloatButton;
    ///
    /// assert_eq!(FloatButton::new("?").text(), "?");
    /// ```
    #[inline]
    pub fn text(&self) -> &str {
        &self.text
    }

    /// Takes the parked activation.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite::widgets::FloatButton;
    ///
    /// assert!(!FloatButton::new("+").take_activated());
    /// ```
    #[inline]
    pub fn take_activated(&mut self) -> bool {
        std::mem::take(&mut self.activated)
    }
}

impl std::fmt::Debug for FloatButton {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("FloatButton")
            .field("text", &self.text)
            .field("visible", &self.visible)
            .finish()
    }
}

impl Widget for FloatButton {
    #[cfg(feature = "devtools-timemachine")]
    fn as_any_mut(&mut self) -> Option<&mut dyn std::any::Any> {
        Some(self)
    }

    /// `@labeled` declares the accessible name to design-lint's
    /// `icon-only-control` rule — only when `.label()` set an explicit
    /// name; a bare `"+"` glyph is a weak name the rule should catch.
    fn debug_name(&self) -> &'static str {
        if self.label.is_some() {
            "FloatButton@labeled"
        } else {
            "FloatButton"
        }
    }

    fn measure(&mut self, cx: &mut LayoutContext, _c: LayoutConstraints) -> Vec2 {
        Vec2::splat(cx.pt(SIZE_PT))
    }

    fn min_render(&self) -> RenderMinimum {
        RenderMinimum::new(Vec2::new(32.0, 32.0)).with_policy(UnderflowPolicy::Lint)
    }

    fn layout(&mut self, cx: &mut LayoutContext, bounds: Rect) {
        self.bounds = bounds;
        self.icon_rect = Rect::default();
        if let Some(icon) = &mut self.icon {
            let side = cx.pt(ICON_PT);
            self.icon_rect = Rect::new(
                bounds.origin.x + (bounds.size.x - side) / 2.0,
                bounds.origin.y + (bounds.size.y - side) / 2.0,
                side,
                side,
            );
            cx.layout_child(icon, self.icon_rect);
        }
    }

    fn paint(&self, cx: &mut PaintContext) {
        if !self.visible {
            return;
        }
        let b = cx.bounds;
        let accent = cx.color(TokenKey::AccentColor, [0, 122, 204, 255]);
        // The FAB face resolves to `AccentColor` — its pair ink is
        // `TextInverseColor` (dark on the accent, still ≥4.5:1 when
        // the disabled state falls back to the muted face).
        let fg = cx.color(TokenKey::TextInverseColor, [18, 23, 28, 255]);
        let face = if !self.enabled {
            cx.color(TokenKey::TextMutedColor, [120, 120, 120, 255])
        } else if self.pressed {
            [
                accent[0].saturating_sub(30),
                accent[1].saturating_sub(30),
                accent[2].saturating_sub(30),
                255,
            ]
        } else if self.hover {
            [
                accent[0].saturating_add(20),
                accent[1].saturating_add(20),
                accent[2].saturating_add(20),
                255,
            ]
        } else {
            accent
        };
        let r = kurbo::Rect::new(
            f64::from(b.min_x()),
            f64::from(b.min_y()),
            f64::from(b.max_x()),
            f64::from(b.max_y()),
        );
        cx.list
            .push_fill_shape(r, &martensite_core::shape::Shape::ELLIPSE, face);
        // Shadow ring for the raised look.
        cx.list.push_stroke_shape(
            r,
            &martensite_core::shape::Shape::ELLIPSE,
            1.0_f32.max(cx.pt(0.5)),
            [0, 0, 0, 60],
        );
        // A hosted stroke icon paints itself at `icon_rect` as an
        // internal child — the arena emits it after this pass.
        if self.icon.is_none() {
            // A namespaced icon name resolves through the ambient
            // family and paints in the icon box; other strings are
            // text glyphs.
            let side = cx.pt(ICON_PT);
            let icon_rect = Rect::new(
                b.origin.x + (b.size.x - side) / 2.0,
                b.origin.y + (b.size.y - side) / 2.0,
                side,
                side,
            );
            if !crate::widgets::morph_icon::paint_icon_named(
                cx.list, icon_rect, &self.text, cx.scale, fg,
            ) {
                let painter =
                    crate::text_paint::resolve_painter(&self.text_painter, cx.text_painter);
                let size = 16.0 * cx.scale;
                let w = painter
                    .and_then(|p| p.measure_text(&self.text, size))
                    .unwrap_or(size * self.text.chars().count() as f32 * 0.5);
                paint_label_vcenter(
                    painter,
                    cx.list,
                    r,
                    f64::from(b.min_x() + (b.width() - w.min(b.width())) / 2.0),
                    &self.text,
                    size,
                    fg,
                );
            }
        }
    }

    fn child_count(&self) -> usize {
        usize::from(self.icon.is_some())
    }

    fn child(&self, index: usize) -> Option<&dyn Widget> {
        if index == 0 {
            self.icon.as_ref().map(|i| i as &dyn Widget)
        } else {
            None
        }
    }

    fn child_mut(&mut self, index: usize) -> Option<&mut dyn Widget> {
        if index == 0 {
            self.icon.as_mut().map(|i| i as &mut dyn Widget)
        } else {
            None
        }
    }

    fn child_bounds(&self, index: usize) -> Option<Rect> {
        if index == 0 && self.icon.is_some() {
            Some(self.icon_rect)
        } else {
            None
        }
    }

    fn event(&mut self, cx: &mut EventContext) -> EventResponse {
        if !self.enabled || !self.visible {
            return EventResponse::Ignored;
        }
        match cx.event {
            WidgetEvent::PointerMoved { position } => {
                let hot = self.bounds.contains(*position);
                if hot != self.hover {
                    self.hover = hot;
                    return EventResponse::RequestRepaint;
                }
                EventResponse::Ignored
            }
            WidgetEvent::PointerPressed {
                position, button, ..
            } => {
                if *button == martensite_core::PointerButton::Primary
                    && self.bounds.contains(*position)
                {
                    self.pressed = true;
                    EventResponse::CapturePointer
                } else {
                    EventResponse::Ignored
                }
            }
            WidgetEvent::PointerReleased { position, .. } => {
                if self.pressed {
                    self.pressed = false;
                    if self.bounds.contains(*position) {
                        self.activated = true;
                    }
                    EventResponse::ReleasePointer
                } else {
                    EventResponse::Ignored
                }
            }
            WidgetEvent::KeyPressed { key, .. } => {
                if key == "Enter" || key == "Space" {
                    self.activated = true;
                    EventResponse::Handled
                } else {
                    EventResponse::Ignored
                }
            }
            _ => EventResponse::Ignored,
        }
    }

    fn accessibility(&self, node: &mut AccessKitNode) {
        node.set_role(accesskit::Role::Button);
        if let Some(ref label) = self.label {
            node.set_label(label.as_str());
        } else {
            node.set_label(self.text.clone());
        }
        if !self.enabled {
            node.set_disabled();
        }
        if !self.visible {
            node.set_hidden();
        }
        node.add_action(accesskit::Action::Click);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use martensite_core::{HotNode, PointerButton, WidgetEvent};

    fn laid_out(f: &mut FloatButton) {
        let mut hot = HotNode::default();
        let mut cx = LayoutContext {
            hot: &mut hot,
            scale: 1.0,
        };
        f.layout(&mut cx, Rect::new(0.0, 0.0, 44.0, 44.0));
    }

    fn ev(f: &mut FloatButton, event: &WidgetEvent) -> EventResponse {
        let mut cx = EventContext {
            event,
            bounds: Rect::new(0.0, 0.0, 44.0, 44.0),
            scale: 1.0,
        };
        f.event(&mut cx)
    }

    #[test]
    fn builder() {
        let mut f = FloatButton::new("+");
        assert_eq!(f.text(), "+");
        assert!(!f.take_activated());
    }

    #[test]
    fn back_top_hidden_by_default() {
        let f = FloatButton::back_top();
        assert!(!f.visible);
        assert_eq!(f.label.as_deref(), Some("Back to top"));
    }

    #[test]
    fn click_activates() {
        let mut f = FloatButton::new("+");
        laid_out(&mut f);
        let down = WidgetEvent::PointerPressed {
            position: Vec2::new(22.0, 22.0),
            button: PointerButton::Primary,
            count: 1,
        };
        let up = WidgetEvent::PointerReleased {
            position: Vec2::new(22.0, 22.0),
            button: PointerButton::Primary,
        };
        assert_eq!(ev(&mut f, &down), EventResponse::CapturePointer);
        assert_eq!(ev(&mut f, &up), EventResponse::ReleasePointer);
        assert!(f.take_activated());
    }

    #[test]
    fn hidden_is_inert() {
        let mut f = FloatButton::back_top();
        laid_out(&mut f);
        let down = WidgetEvent::PointerPressed {
            position: Vec2::new(22.0, 22.0),
            button: PointerButton::Primary,
            count: 1,
        };
        assert_eq!(ev(&mut f, &down), EventResponse::Ignored);
    }

    #[test]
    fn enter_activates() {
        let mut f = FloatButton::new("+");
        laid_out(&mut f);
        let key = WidgetEvent::KeyPressed {
            key: "Enter".to_string(),
            repeat: false,
        };
        assert_eq!(ev(&mut f, &key), EventResponse::Handled);
        assert!(f.take_activated());
    }

    #[test]
    fn disabled_inert() {
        let mut f = FloatButton::new("+").enabled(false);
        laid_out(&mut f);
        let down = WidgetEvent::PointerPressed {
            position: Vec2::new(22.0, 22.0),
            button: PointerButton::Primary,
            count: 1,
        };
        assert_eq!(ev(&mut f, &down), EventResponse::Ignored);
        assert!(!f.take_activated());
    }
}
