//! `FontButton` — a font-swatch button face that requests a chooser
//! (GTK `FontButton` / `NSFontPanel` well).
//!
//! The face shows the family name and point size (`"Inter 13"`) in
//! the body style — font enumeration and chooser UI are app/platform
//! concerns, so pressing it parks a request in
//! [`FontButton::take_activated`] for the app to mount a picker, the
//! same seam [`crate::widgets::ColorButton`] uses.
//!
//! # Examples
//!
//! ```
//! use martensite::widgets::font_button::FontButton;
//!
//! let fb = FontButton::new("Inter", 13.0);
//! assert_eq!(fb.family(), "Inter");
//! ```

use accesskit::Node as AccessKitNode;
use glam::Vec2;
use martensite_core::{
    EventContext, EventResponse, LayoutConstraints, LayoutContext, PaintContext, PointerButton,
    Rect, RenderMinimum, UnderflowPolicy, Widget, WidgetEvent,
};
use martensite_theme::TokenKey;

const HEIGHT_PT: f32 = 32.0;
const PAD_PT: f32 = 10.0;
const FONT_PT: f32 = 13.0;
const RADIUS_PT: f32 = 6.0;

const SURFACE: [u8; 4] = [55, 55, 58, 255];
const BORDER: [u8; 4] = [90, 90, 90, 255];
const FG: [u8; 4] = [220, 220, 220, 255];
const MUTED: [u8; 4] = [140, 140, 140, 255];
const ACCENT: [u8; 4] = [80, 140, 220, 255];

/// A font-swatch button — see the module docs.
///
/// ```
/// use martensite::widgets::font_button::FontButton;
///
/// let fb = FontButton::new("Menlo", 12.0);
/// assert_eq!(fb.size(), 12.0);
/// ```
pub struct FontButton {
    /// Font family name shown on the face.
    pub family: String,
    /// Point size shown on the face.
    pub size: f32,
    /// When `false` the button is inert.
    pub enabled: bool,
    activated: bool,
    pressed: bool,
    hot: bool,
    focused: bool,
    bounds: Rect,
    scale: f32,
    text_painter: Option<crate::text_paint::SharedTextPainter>,
}

impl FontButton {
    /// Creates a button showing `family` at `size` pt.
    ///
    /// ```
    /// use martensite::widgets::font_button::FontButton;
    ///
    /// let fb = FontButton::new("Inter", 14.0);
    /// assert_eq!(fb.family(), "Inter");
    /// ```
    pub fn new(family: impl Into<String>, size: f32) -> Self {
        Self {
            family: family.into(),
            size,
            enabled: true,
            activated: false,
            pressed: false,
            hot: false,
            focused: false,
            bounds: Rect::new(0.0, 0.0, 0.0, 0.0),
            scale: 1.0,
            text_painter: None,
        }
    }

    /// Enables or disables the button.
    ///
    /// ```
    /// use martensite::widgets::font_button::FontButton;
    ///
    /// let fb = FontButton::new("I", 12.0).enabled(false);
    /// assert!(!fb.enabled);
    /// ```
    pub fn enabled(mut self, enabled: bool) -> Self {
        self.enabled = enabled;
        self
    }

    /// Explicit painter override — see [`crate::text_paint`].
    ///
    /// ```
    /// use martensite::widgets::font_button::FontButton;
    /// use martensite::text_paint::shared_painter;
    ///
    /// let fb = FontButton::new("I", 12.0).with_text_painter(shared_painter());
    /// assert_eq!(fb.family(), "I");
    /// ```
    pub fn with_text_painter(mut self, painter: crate::text_paint::SharedTextPainter) -> Self {
        self.text_painter = Some(painter);
        self
    }

    /// Font family name.
    ///
    /// ```
    /// use martensite::widgets::font_button::FontButton;
    ///
    /// assert_eq!(FontButton::new("Menlo", 12.0).family(), "Menlo");
    /// ```
    pub fn family(&self) -> &str {
        &self.family
    }

    /// Point size.
    ///
    /// ```
    /// use martensite::widgets::font_button::FontButton;
    ///
    /// assert_eq!(FontButton::new("I", 11.5).size(), 11.5);
    /// ```
    pub fn size(&self) -> f32 {
        self.size
    }

    /// Updates the displayed font (after a chooser commit).
    ///
    /// ```
    /// use martensite::widgets::font_button::FontButton;
    ///
    /// let mut fb = FontButton::new("I", 12.0);
    /// fb.set_font("Georgia", 16.0);
    /// assert_eq!(fb.family(), "Georgia");
    /// ```
    pub fn set_font(&mut self, family: impl Into<String>, size: f32) {
        self.family = family.into();
        self.size = size;
    }

    /// Drains a pending chooser request.
    ///
    /// ```
    /// use martensite::widgets::font_button::FontButton;
    ///
    /// let mut fb = FontButton::new("I", 12.0);
    /// assert!(!fb.take_activated());
    /// ```
    pub fn take_activated(&mut self) -> bool {
        std::mem::take(&mut self.activated)
    }

    /// The `"Family NN"` face text.
    fn face_text(&self) -> String {
        format!("{} {}", self.family, self.size)
    }
}

impl Widget for FontButton {
    fn measure(&mut self, cx: &mut LayoutContext, constraints: LayoutConstraints) -> Vec2 {
        let text_w = self.face_text().chars().count() as f32 * FONT_PT * 0.55 * cx.scale;
        Vec2::new(
            (text_w + cx.pt(2.0 * PAD_PT + 18.0)).min(constraints.max_size.x.max(0.0)),
            cx.pt(HEIGHT_PT).min(constraints.max_size.y.max(0.0)),
        )
    }

    fn min_render(&self) -> RenderMinimum {
        RenderMinimum::new(Vec2::new(60.0, 24.0)).with_policy(UnderflowPolicy::Lint)
    }

    fn layout(&mut self, cx: &mut LayoutContext, bounds: Rect) {
        self.bounds = bounds;
        self.scale = cx.scale;
    }

    fn accessibility(&self, node: &mut AccessKitNode) {
        node.set_role(accesskit::Role::Button);
        node.set_label("Font");
        node.set_value(self.face_text());
        if self.enabled {
            node.add_action(accesskit::Action::Click);
        } else {
            node.set_disabled();
        }
    }

    fn event(&mut self, cx: &mut EventContext) -> EventResponse {
        if !self.enabled {
            return EventResponse::Ignored;
        }
        match cx.event {
            WidgetEvent::PointerMoved { position } => {
                let hot = self.bounds.contains(*position);
                if hot != self.hot {
                    self.hot = hot;
                    return EventResponse::RequestRepaint;
                }
                EventResponse::Ignored
            }
            WidgetEvent::PointerLeave => {
                self.hot = false;
                EventResponse::Ignored
            }
            WidgetEvent::PointerPressed {
                button: PointerButton::Primary,
                position,
                ..
            } => {
                if self.bounds.contains(*position) {
                    self.pressed = true;
                    return EventResponse::CapturePointer;
                }
                EventResponse::Ignored
            }
            WidgetEvent::PointerReleased {
                button: PointerButton::Primary,
                position,
            } => {
                if self.pressed {
                    self.pressed = false;
                    if self.bounds.contains(*position) {
                        self.activated = true;
                    }
                    return EventResponse::Handled;
                }
                EventResponse::Ignored
            }
            WidgetEvent::KeyPressed { key, .. } => match key.as_str() {
                "Enter" | "Space" | " " => {
                    self.activated = true;
                    EventResponse::Handled
                }
                _ => EventResponse::Ignored,
            },
            WidgetEvent::FocusGained => {
                self.focused = true;
                EventResponse::RequestRepaint
            }
            WidgetEvent::FocusLost => {
                self.focused = false;
                self.pressed = false;
                EventResponse::RequestRepaint
            }
            WidgetEvent::SemanticAction(martensite_core::widget::SemanticAction::Click) => {
                self.activated = true;
                EventResponse::Handled
            }
            _ => EventResponse::Ignored,
        }
    }

    fn paint(&self, cx: &mut PaintContext) {
        let f = |r: Rect| {
            kurbo::Rect::new(
                f64::from(r.min_x()),
                f64::from(r.min_y()),
                f64::from(r.max_x()),
                f64::from(r.max_y()),
            )
        };
        let surface = cx.color(TokenKey::SurfaceColor, SURFACE);
        let border = cx.color(TokenKey::BorderColor, BORDER);
        let fg = if self.enabled {
            cx.color(TokenKey::TextColor, FG)
        } else {
            cx.color(TokenKey::TextMutedColor, MUTED)
        };
        let face = if self.pressed && self.enabled {
            [
                surface[0].saturating_sub(16),
                surface[1].saturating_sub(16),
                surface[2].saturating_sub(16),
                255,
            ]
        } else if self.hot && self.enabled {
            [
                surface[0].saturating_add(10),
                surface[1].saturating_add(10),
                surface[2].saturating_add(10),
                255,
            ]
        } else {
            surface
        };
        let shape = martensite_core::shape::Shape::rounded(cx.pt(RADIUS_PT));
        cx.list.push_fill_shape(f(self.bounds), &shape, face);
        cx.list
            .push_stroke_shape(f(self.bounds), &shape, cx.pt(0.5).max(1.0), border);
        if self.focused {
            let inset = Rect::new(
                self.bounds.min_x() + 1.5,
                self.bounds.min_y() + 1.5,
                (self.bounds.width() - 3.0).max(0.0),
                (self.bounds.height() - 3.0).max(0.0),
            );
            let ring = martensite_core::shape::Shape::rounded(cx.pt(RADIUS_PT - 1.0).max(0.0));
            cx.list.push_stroke_shape(
                f(inset),
                &ring,
                cx.pt(1.5),
                cx.color(TokenKey::AccentColor, ACCENT),
            );
        }

        let pad = cx.pt(PAD_PT);
        let size = FONT_PT * cx.scale;
        let text = self.face_text();
        let clip = Rect::new(
            self.bounds.min_x() + pad,
            self.bounds.min_y(),
            (self.bounds.width() - 2.0 * pad - cx.pt(14.0)).max(0.0),
            self.bounds.height(),
        );
        let painter = crate::text_paint::resolve_painter(&self.text_painter, cx.text_painter);
        crate::text_paint::paint_label_clipped(
            painter,
            cx.list,
            f(clip),
            kurbo::Point::new(
                f64::from(clip.min_x()),
                f64::from(self.bounds.min_y() + (self.bounds.height() - size) / 2.0),
            ),
            &text,
            size,
            fg,
        );
        // Trailing disclosure chevron.
        let cs = cx.pt(5.0);
        let x0 = f64::from(self.bounds.max_x() - pad - cs);
        let y0 = f64::from(self.bounds.min_y() + (self.bounds.height() - cs * 0.6) / 2.0);
        let s = f64::from(cs);
        let mut caret = kurbo::BezPath::new();
        caret.move_to((x0, y0));
        caret.line_to((x0 + s / 2.0, y0 + s * 0.6));
        caret.line_to((x0 + s, y0));
        cx.list
            .push_stroke_path(caret, cx.pt(1.5), cx.color(TokenKey::TextMutedColor, MUTED));
    }
}

impl std::fmt::Debug for FontButton {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("FontButton")
            .field("family", &self.family)
            .field("size", &self.size)
            .finish()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use martensite_core::HotNode;

    fn laid_out(fb: &mut FontButton, w: f32, h: f32) {
        let mut hot = HotNode::default();
        let mut cx = LayoutContext {
            hot: &mut hot,
            scale: 1.0,
        };
        fb.measure(
            &mut cx,
            LayoutConstraints {
                min_size: Vec2::ZERO,
                max_size: Vec2::new(w, h),
            },
        );
        fb.layout(&mut cx, Rect::new(0.0, 0.0, w, h));
    }

    fn click(fb: &mut FontButton) {
        for event in [
            WidgetEvent::PointerPressed {
                button: PointerButton::Primary,
                position: Vec2::new(10.0, 10.0),
                count: 1,
            },
            WidgetEvent::PointerReleased {
                button: PointerButton::Primary,
                position: Vec2::new(10.0, 10.0),
            },
        ] {
            fb.event(&mut EventContext {
                event: &event,
                bounds: Rect::new(0.0, 0.0, 200.0, 32.0),
                scale: 1.0,
            });
        }
    }

    #[test]
    fn click_activates() {
        let mut fb = FontButton::new("Inter", 13.0);
        laid_out(&mut fb, 200.0, 32.0);
        click(&mut fb);
        assert!(fb.take_activated());
        assert!(!fb.take_activated());
    }

    #[test]
    fn release_outside_cancels() {
        let mut fb = FontButton::new("Inter", 13.0);
        laid_out(&mut fb, 200.0, 32.0);
        fb.event(&mut EventContext {
            event: &WidgetEvent::PointerPressed {
                button: PointerButton::Primary,
                position: Vec2::new(10.0, 10.0),
                count: 1,
            },
            bounds: Rect::new(0.0, 0.0, 200.0, 32.0),
            scale: 1.0,
        });
        fb.event(&mut EventContext {
            event: &WidgetEvent::PointerReleased {
                button: PointerButton::Primary,
                position: Vec2::new(500.0, 500.0),
            },
            bounds: Rect::new(0.0, 0.0, 200.0, 32.0),
            scale: 1.0,
        });
        assert!(!fb.take_activated());
    }

    #[test]
    fn enter_activates() {
        let mut fb = FontButton::new("Inter", 13.0);
        laid_out(&mut fb, 200.0, 32.0);
        fb.event(&mut EventContext {
            event: &WidgetEvent::KeyPressed {
                key: "Space".to_string(),
                repeat: false,
            },
            bounds: Rect::new(0.0, 0.0, 200.0, 32.0),
            scale: 1.0,
        });
        assert!(fb.take_activated());
    }

    #[test]
    fn set_font_updates_face() {
        let mut fb = FontButton::new("Inter", 13.0);
        fb.set_font("Georgia", 16.0);
        assert_eq!(fb.face_text(), "Georgia 16");
    }

    #[test]
    fn disabled_inert() {
        let mut fb = FontButton::new("Inter", 13.0).enabled(false);
        laid_out(&mut fb, 200.0, 32.0);
        click(&mut fb);
        assert!(!fb.take_activated());
    }
}
