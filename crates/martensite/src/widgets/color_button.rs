//! `ColorButton` — a color-swatch button face that requests a picker
//! (GTK `ColorButton`, `NSColorWell`, WinUI `ColorPicker` drop-down).
//!
//! The face shows the current color over an alpha checkerboard plus
//! an optional title; pressing it (click, Enter/Space, or a semantic
//! `Click`) parks a request in [`ColorButton::take_activated`] for
//! the app to mount a `ColorPicker`/`ColorPalette` popup — the same
//! parked-request seam `SplitButton` uses.
//!
//! # Examples
//!
//! ```
//! use martensite::widgets::color_button::ColorButton;
//!
//! let cb = ColorButton::new([220, 60, 60, 255]).title("Accent");
//! assert_eq!(cb.color(), [220, 60, 60, 255]);
//! ```

use accesskit::Node as AccessKitNode;
use glam::Vec2;
use martensite_core::{
    EventContext, EventResponse, LayoutConstraints, LayoutContext, PaintContext, PointerButton,
    Rect, RenderMinimum, UnderflowPolicy, Widget, WidgetEvent,
};
use martensite_theme::TokenKey;

const HEIGHT_PT: f32 = 32.0;
const SWATCH_PT: f32 = 20.0;
const PAD_PT: f32 = 8.0;
const FONT_PT: f32 = 13.0;
const RADIUS_PT: f32 = 6.0;

const SURFACE: [u8; 4] = [55, 55, 58, 255];
const BORDER: [u8; 4] = [90, 90, 90, 255];
const FG: [u8; 4] = [220, 220, 220, 255];
const MUTED: [u8; 4] = [140, 140, 140, 255];
const CHECK_A: [u8; 4] = [200, 200, 200, 255];
const CHECK_B: [u8; 4] = [140, 140, 140, 255];

/// A color-swatch button — see the module docs.
///
/// ```
/// use martensite::widgets::color_button::ColorButton;
///
/// let cb = ColorButton::new([0, 120, 220, 255]);
/// assert_eq!(cb.color()[1], 120);
/// ```
pub struct ColorButton {
    /// The displayed color `[r, g, b, a]`.
    pub color: [u8; 4],
    /// When `false` the button is inert.
    pub enabled: bool,
    title: Option<String>,
    activated: bool,
    pressed: bool,
    hot: bool,
    focused: bool,
    bounds: Rect,
    scale: f32,
    text_painter: Option<crate::text_paint::SharedTextPainter>,
}

impl ColorButton {
    /// Creates a button showing `color`.
    ///
    /// ```
    /// use martensite::widgets::color_button::ColorButton;
    ///
    /// let cb = ColorButton::new([255, 0, 0, 255]);
    /// assert_eq!(cb.color(), [255, 0, 0, 255]);
    /// ```
    pub fn new(color: [u8; 4]) -> Self {
        Self {
            color,
            enabled: true,
            title: None,
            activated: false,
            pressed: false,
            hot: false,
            focused: false,
            bounds: Rect::new(0.0, 0.0, 0.0, 0.0),
            scale: 1.0,
            text_painter: None,
        }
    }

    /// Optional label beside the swatch.
    ///
    /// ```
    /// use martensite::widgets::color_button::ColorButton;
    ///
    /// let cb = ColorButton::new([0, 0, 0, 255]).title("Ink");
    /// assert_eq!(cb.label(), Some("Ink"));
    /// ```
    pub fn title(mut self, title: impl Into<String>) -> Self {
        self.title = Some(title.into());
        self
    }

    /// Enables or disables the button.
    ///
    /// ```
    /// use martensite::widgets::color_button::ColorButton;
    ///
    /// let cb = ColorButton::new([0, 0, 0, 255]).enabled(false);
    /// assert!(!cb.enabled);
    /// ```
    pub fn enabled(mut self, enabled: bool) -> Self {
        self.enabled = enabled;
        self
    }

    /// Explicit painter override — see [`crate::text_paint`].
    ///
    /// ```
    /// use martensite::widgets::color_button::ColorButton;
    /// use martensite::text_paint::shared_painter;
    ///
    /// let cb = ColorButton::new([0, 0, 0, 255]).with_text_painter(shared_painter());
    /// assert_eq!(cb.label(), None);
    /// ```
    pub fn with_text_painter(mut self, painter: crate::text_paint::SharedTextPainter) -> Self {
        self.text_painter = Some(painter);
        self
    }

    /// The displayed color.
    ///
    /// ```
    /// use martensite::widgets::color_button::ColorButton;
    ///
    /// assert_eq!(ColorButton::new([1, 2, 3, 4]).color(), [1, 2, 3, 4]);
    /// ```
    pub fn color(&self) -> [u8; 4] {
        self.color
    }

    /// Optional label.
    ///
    /// ```
    /// use martensite::widgets::color_button::ColorButton;
    ///
    /// assert_eq!(ColorButton::new([0, 0, 0, 255]).label(), None);
    /// ```
    pub fn label(&self) -> Option<&str> {
        self.title.as_deref()
    }

    /// Updates the swatch.
    ///
    /// ```
    /// use martensite::widgets::color_button::ColorButton;
    ///
    /// let mut cb = ColorButton::new([0, 0, 0, 255]);
    /// cb.set_color([9, 9, 9, 255]);
    /// assert_eq!(cb.color(), [9, 9, 9, 255]);
    /// ```
    pub fn set_color(&mut self, color: [u8; 4]) {
        self.color = color;
    }

    /// Drains a pending picker request.
    ///
    /// ```
    /// use martensite::widgets::color_button::ColorButton;
    ///
    /// let mut cb = ColorButton::new([0, 0, 0, 255]);
    /// assert!(!cb.take_activated());
    /// ```
    pub fn take_activated(&mut self) -> bool {
        std::mem::take(&mut self.activated)
    }

    fn hex(&self) -> String {
        let [r, g, b, a] = self.color;
        if a == 255 {
            format!("#{r:02x}{g:02x}{b:02x}")
        } else {
            format!("#{r:02x}{g:02x}{b:02x}{a:02x}")
        }
    }
}

impl Widget for ColorButton {
    fn measure(&mut self, cx: &mut LayoutContext, constraints: LayoutConstraints) -> Vec2 {
        let title_w = self
            .title
            .as_deref()
            .map(|t| t.chars().count() as f32 * FONT_PT * 0.55 * cx.scale)
            .unwrap_or(0.0);
        let w = cx.pt(PAD_PT + SWATCH_PT + PAD_PT + PAD_PT) + title_w;
        Vec2::new(
            w.min(constraints.max_size.x.max(0.0)),
            cx.pt(HEIGHT_PT).min(constraints.max_size.y.max(0.0)),
        )
    }

    fn min_render(&self) -> RenderMinimum {
        RenderMinimum::new(Vec2::new(40.0, 24.0)).with_policy(UnderflowPolicy::Lint)
    }

    fn layout(&mut self, cx: &mut LayoutContext, bounds: Rect) {
        self.bounds = bounds;
        self.scale = cx.scale;
    }

    fn accessibility(&self, node: &mut AccessKitNode) {
        node.set_role(accesskit::Role::Button);
        node.set_label(
            self.title
                .clone()
                .unwrap_or_else(|| "Color picker".to_string()),
        );
        node.set_value(self.hex());
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
                cx.color(TokenKey::AccentColor, [80, 140, 220, 255]),
            );
        }

        // Alpha checkerboard under the swatch.
        let pad = cx.pt(PAD_PT);
        let s = cx.pt(SWATCH_PT);
        let sw = Rect::new(
            self.bounds.min_x() + pad,
            self.bounds.min_y() + (self.bounds.height() - s) / 2.0,
            s,
            s,
        );
        let half = s / 2.0;
        let sw_shape = martensite_core::shape::Shape::rounded(cx.pt(3.0));
        cx.list.push_fill_shape(f(sw), &sw_shape, CHECK_A);
        let q = |x: f32, y: f32| f(Rect::new(sw.min_x() + x, sw.min_y() + y, half, half));
        cx.list.push_fill_shape(q(half, 0.0), &sw_shape, CHECK_B);
        cx.list.push_fill_shape(q(0.0, half), &sw_shape, CHECK_B);
        cx.list.push_fill_shape(f(sw), &sw_shape, self.color);
        cx.list
            .push_stroke_shape(f(sw), &sw_shape, cx.pt(0.5).max(1.0), border);

        // Title.
        if let Some(title) = &self.title {
            let painter = crate::text_paint::resolve_painter(&self.text_painter, cx.text_painter);
            let size = FONT_PT * cx.scale;
            let clip = Rect::new(
                sw.max_x() + pad,
                self.bounds.min_y(),
                (self.bounds.max_x() - pad - sw.max_x() - pad).max(0.0),
                self.bounds.height(),
            );
            crate::text_paint::paint_label_clipped(
                painter,
                cx.list,
                f(clip),
                kurbo::Point::new(
                    f64::from(clip.min_x()),
                    f64::from(self.bounds.min_y() + (self.bounds.height() - size) / 2.0),
                ),
                title,
                size,
                fg,
            );
        }
    }
}

impl std::fmt::Debug for ColorButton {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ColorButton")
            .field("color", &self.color)
            .field("title", &self.title)
            .finish()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use martensite_core::HotNode;

    fn laid_out(cb: &mut ColorButton, w: f32, h: f32) {
        let mut hot = HotNode::default();
        let mut cx = LayoutContext {
            hot: &mut hot,
            scale: 1.0,
        };
        cb.measure(
            &mut cx,
            LayoutConstraints {
                min_size: Vec2::ZERO,
                max_size: Vec2::new(w, h),
            },
        );
        cb.layout(&mut cx, Rect::new(0.0, 0.0, w, h));
    }

    fn press(cb: &mut ColorButton, p: Vec2) {
        cb.event(&mut EventContext {
            event: &WidgetEvent::PointerPressed {
                button: PointerButton::Primary,
                position: p,
                count: 1,
            },
            bounds: Rect::new(0.0, 0.0, 200.0, 32.0),
            scale: 1.0,
        });
    }

    fn release(cb: &mut ColorButton, p: Vec2) {
        cb.event(&mut EventContext {
            event: &WidgetEvent::PointerReleased {
                button: PointerButton::Primary,
                position: p,
            },
            bounds: Rect::new(0.0, 0.0, 200.0, 32.0),
            scale: 1.0,
        });
    }

    #[test]
    fn press_release_activates() {
        let mut cb = ColorButton::new([1, 2, 3, 255]);
        laid_out(&mut cb, 200.0, 32.0);
        press(&mut cb, Vec2::new(10.0, 10.0));
        release(&mut cb, Vec2::new(10.0, 10.0));
        assert!(cb.take_activated());
        assert!(!cb.take_activated());
    }

    #[test]
    fn release_outside_cancels() {
        let mut cb = ColorButton::new([1, 2, 3, 255]);
        laid_out(&mut cb, 200.0, 32.0);
        press(&mut cb, Vec2::new(10.0, 10.0));
        release(&mut cb, Vec2::new(500.0, 500.0));
        assert!(!cb.take_activated());
    }

    #[test]
    fn enter_activates() {
        let mut cb = ColorButton::new([1, 2, 3, 255]);
        laid_out(&mut cb, 200.0, 32.0);
        cb.event(&mut EventContext {
            event: &WidgetEvent::KeyPressed {
                key: "Enter".to_string(),
                repeat: false,
            },
            bounds: Rect::new(0.0, 0.0, 200.0, 32.0),
            scale: 1.0,
        });
        assert!(cb.take_activated());
    }

    #[test]
    fn semantic_click_activates() {
        use martensite_core::widget::SemanticAction;
        let mut cb = ColorButton::new([1, 2, 3, 255]);
        laid_out(&mut cb, 200.0, 32.0);
        cb.event(&mut EventContext {
            event: &WidgetEvent::SemanticAction(SemanticAction::Click),
            bounds: Rect::new(0.0, 0.0, 200.0, 32.0),
            scale: 1.0,
        });
        assert!(cb.take_activated());
    }

    #[test]
    fn disabled_inert() {
        let mut cb = ColorButton::new([1, 2, 3, 255]).enabled(false);
        laid_out(&mut cb, 200.0, 32.0);
        press(&mut cb, Vec2::new(10.0, 10.0));
        release(&mut cb, Vec2::new(10.0, 10.0));
        assert!(!cb.take_activated());
    }

    #[test]
    fn hex_value() {
        let opaque = ColorButton::new([255, 0, 16, 255]);
        assert_eq!(opaque.hex(), "#ff0010");
        let alpha = ColorButton::new([255, 0, 16, 128]);
        assert_eq!(alpha.hex(), "#ff001080");
    }
}
