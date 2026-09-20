//! `AlphaSlider` — a horizontal opacity rail over a checkerboard
//! backing, with a draggable handle picking alpha in `0.0..=1.0`
//! (the alpha strip in every color picker — pairs with
//! [`crate::widgets::hue_slider::HueSlider`]).
//!
//! Click or drag anywhere on the rail to set alpha; `←`/`→` step
//! by 0.01. The final value after each interaction parks in
//! [`AlphaSlider::take_changed`].
//!
//! # Examples
//!
//! ```
//! use martensite::widgets::alpha_slider::AlphaSlider;
//!
//! let mut s = AlphaSlider::new().alpha(0.5);
//! assert_eq!(s.alpha_value(), 0.5);
//! s.set_alpha(2.0); // clamps
//! assert_eq!(s.alpha_value(), 1.0);
//! ```

use accesskit::Node as AccessKitNode;
use glam::Vec2;
use martensite_core::{
    EventContext, EventResponse, LayoutConstraints, LayoutContext, PaintContext, PointerButton,
    Rect, RenderMinimum, UnderflowPolicy, Widget, WidgetEvent,
};
use martensite_theme::TokenKey;

const W_PT: f32 = 200.0;
const RAIL_PT: f32 = 14.0;
const H_PT: f32 = 26.0;
/// Checkerboard square size, logical points.
const CHECK_PT: f32 = 4.0;

const FACE: [u8; 4] = [36, 36, 40, 255];
const CHECK_A: [u8; 4] = [210, 210, 214, 255];
const CHECK_B: [u8; 4] = [150, 150, 156, 255];
const HANDLE: [u8; 4] = [250, 250, 252, 255];
const BASE: [u8; 4] = [96, 165, 250, 255];

/// An opacity rail — see the module docs.
///
/// ```
/// use martensite::widgets::alpha_slider::AlphaSlider;
///
/// assert_eq!(AlphaSlider::new().alpha_value(), 1.0);
/// ```
pub struct AlphaSlider {
    /// Accessibility label.
    pub label: String,
    /// Opaque end color (alpha channel ignored).
    color: [u8; 4],
    /// Alpha, 0.0..=1.0.
    alpha: f32,
    changed: Option<f32>,
    dragging: bool,
    bounds: Rect,
    scale: f32,
    rail: Rect,
}

impl std::fmt::Debug for AlphaSlider {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("AlphaSlider")
            .field("alpha", &self.alpha)
            .finish()
    }
}

impl Default for AlphaSlider {
    fn default() -> Self {
        Self::new()
    }
}

impl AlphaSlider {
    /// Creates a slider at full opacity.
    ///
    /// ```
    /// use martensite::widgets::alpha_slider::AlphaSlider;
    ///
    /// assert_eq!(AlphaSlider::new().alpha_value(), 1.0);
    /// ```
    pub fn new() -> Self {
        Self {
            label: "Opacity".to_string(),
            color: BASE,
            alpha: 1.0,
            changed: None,
            dragging: false,
            bounds: Rect::new(0.0, 0.0, 0.0, 0.0),
            scale: 1.0,
            rail: Rect::new(0.0, 0.0, 0.0, 0.0),
        }
    }

    /// Accessibility label.
    ///
    /// ```
    /// use martensite::widgets::alpha_slider::AlphaSlider;
    ///
    /// assert_eq!(AlphaSlider::new().label("Fill opacity").label, "Fill opacity");
    /// ```
    pub fn label(mut self, label: impl Into<String>) -> Self {
        self.label = label.into();
        self
    }

    /// The opaque end color shown by the rail.
    ///
    /// ```
    /// use martensite::widgets::alpha_slider::AlphaSlider;
    ///
    /// let s = AlphaSlider::new().color([255, 0, 0, 255]);
    /// assert_eq!(s.color_value(), [255, 0, 0, 255]);
    /// ```
    pub fn color(mut self, color: [u8; 4]) -> Self {
        self.color = [color[0], color[1], color[2], 255];
        self
    }

    /// Initial alpha (clamps to 0..1).
    ///
    /// ```
    /// use martensite::widgets::alpha_slider::AlphaSlider;
    ///
    /// assert_eq!(AlphaSlider::new().alpha(0.25).alpha_value(), 0.25);
    /// ```
    pub fn alpha(mut self, alpha: f32) -> Self {
        self.alpha = alpha.clamp(0.0, 1.0);
        self
    }

    /// The opaque end color.
    ///
    /// ```
    /// use martensite::widgets::alpha_slider::AlphaSlider;
    ///
    /// assert_eq!(AlphaSlider::new().color_value()[3], 255);
    /// ```
    pub fn color_value(&self) -> [u8; 4] {
        self.color
    }

    /// Current alpha.
    ///
    /// ```
    /// use martensite::widgets::alpha_slider::AlphaSlider;
    ///
    /// assert_eq!(AlphaSlider::new().alpha_value(), 1.0);
    /// ```
    pub fn alpha_value(&self) -> f32 {
        self.alpha
    }

    /// Sets alpha (clamps to 0..1).
    ///
    /// ```
    /// use martensite::widgets::alpha_slider::AlphaSlider;
    ///
    /// let mut s = AlphaSlider::new();
    /// s.set_alpha(-1.0);
    /// assert_eq!(s.alpha_value(), 0.0);
    /// ```
    pub fn set_alpha(&mut self, alpha: f32) {
        self.alpha = alpha.clamp(0.0, 1.0);
    }

    /// Drains the alpha parked by the last interaction.
    ///
    /// ```
    /// use martensite::widgets::alpha_slider::AlphaSlider;
    ///
    /// assert_eq!(AlphaSlider::new().take_changed(), None);
    /// ```
    pub fn take_changed(&mut self) -> Option<f32> {
        self.changed.take()
    }

    /// Alpha picked at rail x-position `x`.
    fn alpha_at(&self, x: f32) -> f32 {
        let w = self.rail.width().max(1.0);
        ((x - self.rail.min_x()) / w).clamp(0.0, 1.0)
    }
}

impl Widget for AlphaSlider {
    fn measure(&mut self, cx: &mut LayoutContext, constraints: LayoutConstraints) -> Vec2 {
        Vec2::new(
            cx.pt(W_PT).min(constraints.max_size.x.max(0.0)),
            cx.pt(H_PT).min(constraints.max_size.y.max(0.0)),
        )
    }

    fn min_render(&self) -> RenderMinimum {
        RenderMinimum::new(Vec2::new(60.0, 16.0)).with_policy(UnderflowPolicy::Lint)
    }

    fn layout(&mut self, cx: &mut LayoutContext, bounds: Rect) {
        self.bounds = bounds;
        self.scale = cx.scale;
        let rail_h = (RAIL_PT * self.scale).min(bounds.height());
        self.rail = Rect::new(
            bounds.min_x(),
            bounds.min_y() + (bounds.height() - rail_h) / 2.0,
            bounds.width(),
            rail_h,
        );
    }

    fn accessibility(&self, node: &mut AccessKitNode) {
        node.set_role(accesskit::Role::Slider);
        node.set_label(self.label.clone());
        node.set_numeric_value(f64::from(self.alpha));
    }

    fn event(&mut self, cx: &mut EventContext) -> EventResponse {
        match cx.event {
            WidgetEvent::PointerPressed {
                button: PointerButton::Primary,
                position,
                ..
            } => {
                if self.bounds.contains(*position) {
                    self.dragging = true;
                    self.alpha = self.alpha_at(position.x);
                    return EventResponse::RequestRepaint;
                }
                EventResponse::Ignored
            }
            WidgetEvent::PointerMoved { position } => {
                if self.dragging {
                    self.alpha = self.alpha_at(position.x);
                    return EventResponse::RequestRepaint;
                }
                EventResponse::Ignored
            }
            WidgetEvent::PointerReleased {
                button: PointerButton::Primary,
                ..
            } => {
                if self.dragging {
                    self.dragging = false;
                    self.changed = Some(self.alpha);
                    return EventResponse::Handled;
                }
                EventResponse::Ignored
            }
            WidgetEvent::KeyPressed { key, .. } => match key.as_str() {
                "ArrowRight" | "ArrowUp" => {
                    self.set_alpha(self.alpha + 0.01);
                    self.changed = Some(self.alpha);
                    EventResponse::RequestRepaint
                }
                "ArrowLeft" | "ArrowDown" => {
                    self.set_alpha(self.alpha - 0.01);
                    self.changed = Some(self.alpha);
                    EventResponse::RequestRepaint
                }
                _ => EventResponse::Ignored,
            },
            _ => EventResponse::Ignored,
        }
    }

    fn paint(&self, cx: &mut PaintContext) {
        let krect = |r: Rect| {
            kurbo::Rect::new(
                f64::from(r.min_x()),
                f64::from(r.min_y()),
                f64::from(r.max_x()),
                f64::from(r.max_y()),
            )
        };
        cx.list.push_fill_shape(
            krect(self.bounds),
            &martensite_core::shape::Shape::RECT,
            cx.color(TokenKey::BackgroundColor, FACE),
        );
        let s = self.scale;
        cx.list.push_clip_shape(
            krect(self.rail),
            &martensite_core::shape::Shape::rounded(self.rail.height() / 2.0),
        );
        // Checkerboard backing.
        let sq = (CHECK_PT * s).max(2.0);
        let rows = (self.rail.height() / sq).ceil() as usize;
        let cols = (self.rail.width() / sq).ceil() as usize;
        for row in 0..rows {
            for col in 0..cols {
                let c = if (row + col) % 2 == 0 {
                    CHECK_A
                } else {
                    CHECK_B
                };
                cx.list.push_fill_rect(
                    krect(Rect::new(
                        self.rail.min_x() + col as f32 * sq,
                        self.rail.min_y() + row as f32 * sq,
                        sq,
                        sq,
                    )),
                    c,
                );
            }
        }
        // Transparent → opaque color ramp on top.
        let steps = (self.rail.width() / (s * 2.0).max(1.0)).ceil().max(1.0) as usize;
        let step_w = self.rail.width() / steps as f32;
        for i in 0..steps {
            let t = i as f32 / (steps.saturating_sub(1)).max(1) as f32;
            cx.list.push_fill_rect(
                krect(Rect::new(
                    self.rail.min_x() + i as f32 * step_w,
                    self.rail.min_y(),
                    step_w + 0.5,
                    self.rail.height(),
                )),
                [
                    self.color[0],
                    self.color[1],
                    self.color[2],
                    (t * 255.0).round() as u8,
                ],
            );
        }
        cx.list.pop_clip();
        // Handle at the alpha position.
        let hx = self.rail.min_x() + self.alpha * self.rail.width();
        let hy = (self.rail.min_y() + self.rail.max_y()) / 2.0;
        let r = self.rail.height() / 2.0 - 1.0 * s;
        let hr = krect(Rect::new(hx - r, hy - r, r * 2.0, r * 2.0));
        cx.list.push_fill_shape(
            hr,
            &martensite_core::shape::Shape::ELLIPSE,
            [
                self.color[0],
                self.color[1],
                self.color[2],
                (self.alpha * 255.0).round() as u8,
            ],
        );
        cx.list.push_stroke_shape(
            hr,
            &martensite_core::shape::Shape::ELLIPSE,
            2.0 * s,
            cx.color(TokenKey::TextInverseColor, HANDLE),
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use martensite_core::HotNode;

    fn laid_out(w: &mut AlphaSlider, wd: f32, h: f32) {
        let mut hot = HotNode::default();
        let mut cx = LayoutContext {
            hot: &mut hot,
            scale: 1.0,
        };
        w.measure(
            &mut cx,
            LayoutConstraints {
                min_size: Vec2::ZERO,
                max_size: Vec2::new(wd, h),
            },
        );
        w.layout(&mut cx, Rect::new(0.0, 0.0, wd, h));
    }

    #[test]
    fn alpha_clamps() {
        let mut s = AlphaSlider::new();
        s.set_alpha(1.5);
        assert_eq!(s.alpha_value(), 1.0);
        s.set_alpha(-0.5);
        assert_eq!(s.alpha_value(), 0.0);
    }

    #[test]
    fn drag_picks_alpha() {
        let mut s = AlphaSlider::new();
        laid_out(&mut s, 200.0, 26.0);
        s.event(&mut EventContext {
            event: &WidgetEvent::PointerPressed {
                button: PointerButton::Primary,
                position: Vec2::new(100.0, 13.0),
                count: 1,
            },
            bounds: s.bounds,
            scale: 1.0,
        });
        assert!((s.alpha_value() - 0.5).abs() < 0.02);
        s.event(&mut EventContext {
            event: &WidgetEvent::PointerMoved {
                position: Vec2::new(50.0, 13.0),
            },
            bounds: s.bounds,
            scale: 1.0,
        });
        assert!((s.alpha_value() - 0.25).abs() < 0.02);
        s.event(&mut EventContext {
            event: &WidgetEvent::PointerReleased {
                button: PointerButton::Primary,
                position: Vec2::new(50.0, 13.0),
            },
            bounds: s.bounds,
            scale: 1.0,
        });
        assert!((s.take_changed().unwrap() - 0.25).abs() < 0.02);
    }

    #[test]
    fn arrows_step() {
        let mut s = AlphaSlider::new().alpha(0.5);
        laid_out(&mut s, 200.0, 26.0);
        s.event(&mut EventContext {
            event: &WidgetEvent::KeyPressed {
                key: "ArrowLeft".to_string(),
                repeat: false,
            },
            bounds: s.bounds,
            scale: 1.0,
        });
        assert!((s.alpha_value() - 0.49).abs() < 1e-6);
        assert!(s.take_changed().is_some());
    }

    #[test]
    fn color_alpha_forced_opaque() {
        let s = AlphaSlider::new().color([10, 20, 30, 0]);
        assert_eq!(s.color_value()[3], 255);
    }
}
