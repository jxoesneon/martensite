//! `HueSlider` — a horizontal rainbow rail with a draggable handle
//! picking a hue angle in degrees (the hue strip in every color
//! picker — pairs with [`crate::widgets::color_wheel::ColorWheel`]
//! and the S×V square inside
//! [`crate::widgets::color_picker::ColorPicker`]).
//!
//! Click or drag anywhere on the rail to set the hue; `←`/`→` step
//! by one degree. The final value after each interaction parks in
//! [`HueSlider::take_changed`].
//!
//! # Examples
//!
//! ```
//! use martensite::widgets::hue_slider::HueSlider;
//!
//! let mut s = HueSlider::new().hue(200.0);
//! assert_eq!(s.hue_value(), 200.0);
//! s.set_hue(480.0); // wraps
//! assert_eq!(s.hue_value(), 120.0);
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

const FACE: [u8; 4] = [36, 36, 40, 255];
const HANDLE: [u8; 4] = [250, 250, 252, 255];

/// HSV → RGB, `h` in degrees 0..360, `s`/`v` in 0..1.
fn hsv(h: f32, s: f32, v: f32) -> [u8; 4] {
    let h = h.rem_euclid(360.0) / 60.0;
    let c = v * s;
    let x = c * (1.0 - (h % 2.0 - 1.0).abs());
    let m = v - c;
    let (r, g, b) = match h as u8 {
        0 => (c, x, 0.0),
        1 => (x, c, 0.0),
        2 => (0.0, c, x),
        3 => (0.0, x, c),
        4 => (x, 0.0, c),
        _ => (c, 0.0, x),
    };
    [
        ((r + m) * 255.0).round() as u8,
        ((g + m) * 255.0).round() as u8,
        ((b + m) * 255.0).round() as u8,
        255,
    ]
}

/// A hue-picking rail — see the module docs.
///
/// ```
/// use martensite::widgets::hue_slider::HueSlider;
///
/// assert_eq!(HueSlider::new().hue_value(), 0.0);
/// ```
pub struct HueSlider {
    /// Accessibility label.
    pub label: String,
    /// Hue angle, degrees 0..360.
    hue: f32,
    changed: Option<f32>,
    dragging: bool,
    bounds: Rect,
    scale: f32,
    rail: Rect,
}

impl std::fmt::Debug for HueSlider {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("HueSlider").field("hue", &self.hue).finish()
    }
}

impl Default for HueSlider {
    fn default() -> Self {
        Self::new()
    }
}

impl HueSlider {
    /// Creates a slider at hue 0° (red).
    ///
    /// ```
    /// use martensite::widgets::hue_slider::HueSlider;
    ///
    /// assert_eq!(HueSlider::new().hue_value(), 0.0);
    /// ```
    pub fn new() -> Self {
        Self {
            label: "Hue".to_string(),
            hue: 0.0,
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
    /// use martensite::widgets::hue_slider::HueSlider;
    ///
    /// assert_eq!(HueSlider::new().label("Base hue").label, "Base hue");
    /// ```
    pub fn label(mut self, label: impl Into<String>) -> Self {
        self.label = label.into();
        self
    }

    /// Initial hue, degrees (wraps into 0..360).
    ///
    /// ```
    /// use martensite::widgets::hue_slider::HueSlider;
    ///
    /// assert_eq!(HueSlider::new().hue(120.0).hue_value(), 120.0);
    /// assert_eq!(HueSlider::new().hue(-30.0).hue_value(), 330.0);
    /// ```
    pub fn hue(mut self, degrees: f32) -> Self {
        self.hue = degrees.rem_euclid(360.0);
        self
    }

    /// Current hue, degrees.
    ///
    /// ```
    /// use martensite::widgets::hue_slider::HueSlider;
    ///
    /// assert_eq!(HueSlider::new().hue(45.0).hue_value(), 45.0);
    /// ```
    pub fn hue_value(&self) -> f32 {
        self.hue
    }

    /// Sets the hue (wraps into 0..360).
    ///
    /// ```
    /// use martensite::widgets::hue_slider::HueSlider;
    ///
    /// let mut s = HueSlider::new();
    /// s.set_hue(270.0);
    /// assert_eq!(s.hue_value(), 270.0);
    /// ```
    pub fn set_hue(&mut self, degrees: f32) {
        self.hue = degrees.rem_euclid(360.0);
    }

    /// Drains the hue parked by the last interaction.
    ///
    /// ```
    /// use martensite::widgets::hue_slider::HueSlider;
    ///
    /// assert_eq!(HueSlider::new().take_changed(), None);
    /// ```
    pub fn take_changed(&mut self) -> Option<f32> {
        self.changed.take()
    }

    /// Hue picked at rail x-position `x`.
    fn hue_at(&self, x: f32) -> f32 {
        let w = self.rail.width().max(1.0);
        ((x - self.rail.min_x()) / w).clamp(0.0, 1.0) * 360.0
    }
}

impl Widget for HueSlider {
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
        node.set_numeric_value(f64::from(self.hue));
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
                    self.hue = self.hue_at(position.x);
                    return EventResponse::RequestRepaint;
                }
                EventResponse::Ignored
            }
            WidgetEvent::PointerMoved { position } => {
                if self.dragging {
                    self.hue = self.hue_at(position.x);
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
                    self.changed = Some(self.hue);
                    return EventResponse::Handled;
                }
                EventResponse::Ignored
            }
            WidgetEvent::KeyPressed { key, .. } => match key.as_str() {
                "ArrowRight" | "ArrowUp" => {
                    self.set_hue(self.hue + 1.0);
                    self.changed = Some(self.hue);
                    EventResponse::RequestRepaint
                }
                "ArrowLeft" | "ArrowDown" => {
                    self.set_hue(self.hue - 1.0);
                    self.changed = Some(self.hue);
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
        // Rainbow rail — full-saturation slices.
        let cols = (self.rail.width() / (self.scale * 2.0).max(1.0))
            .ceil()
            .max(1.0) as usize;
        let col_w = self.rail.width() / cols as f32;
        cx.list.push_clip_shape(
            krect(self.rail),
            &martensite_core::shape::Shape::rounded(self.rail.height() / 2.0),
        );
        for i in 0..cols {
            let t = i as f32 / (cols.saturating_sub(1)).max(1) as f32;
            cx.list.push_fill_rect(
                krect(Rect::new(
                    self.rail.min_x() + i as f32 * col_w,
                    self.rail.min_y(),
                    col_w + 0.5,
                    self.rail.height(),
                )),
                hsv(t * 360.0, 1.0, 1.0),
            );
        }
        cx.list.pop_clip();
        // Handle — white ring at the hue position, tinted center.
        let hx = self.rail.min_x() + (self.hue / 360.0) * self.rail.width();
        let hy = (self.rail.min_y() + self.rail.max_y()) / 2.0;
        let r = self.rail.height() / 2.0 - 1.0 * self.scale;
        cx.list.push_fill_shape(
            krect(Rect::new(hx - r, hy - r, r * 2.0, r * 2.0)),
            &martensite_core::shape::Shape::ELLIPSE,
            hsv(self.hue, 1.0, 1.0),
        );
        cx.list.push_stroke_shape(
            krect(Rect::new(hx - r, hy - r, r * 2.0, r * 2.0)),
            &martensite_core::shape::Shape::ELLIPSE,
            2.0 * self.scale,
            cx.color(TokenKey::TextInverseColor, HANDLE),
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use martensite_core::HotNode;

    fn laid_out(w: &mut HueSlider, wd: f32, h: f32) {
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
    fn hue_wraps() {
        let mut s = HueSlider::new();
        s.set_hue(370.0);
        assert_eq!(s.hue_value(), 10.0);
        s.set_hue(-90.0);
        assert_eq!(s.hue_value(), 270.0);
    }

    #[test]
    fn drag_picks_hue() {
        let mut s = HueSlider::new();
        laid_out(&mut s, 200.0, 26.0);
        // Press at the rail midpoint → hue ≈ 180.
        s.event(&mut EventContext {
            event: &WidgetEvent::PointerPressed {
                button: PointerButton::Primary,
                position: Vec2::new(100.0, 13.0),
                count: 1,
            },
            bounds: s.bounds,
            scale: 1.0,
        });
        assert!((s.hue_value() - 180.0).abs() < 2.0);
        s.event(&mut EventContext {
            event: &WidgetEvent::PointerMoved {
                position: Vec2::new(50.0, 13.0),
            },
            bounds: s.bounds,
            scale: 1.0,
        });
        assert!((s.hue_value() - 90.0).abs() < 2.0);
        s.event(&mut EventContext {
            event: &WidgetEvent::PointerReleased {
                button: PointerButton::Primary,
                position: Vec2::new(50.0, 13.0),
            },
            bounds: s.bounds,
            scale: 1.0,
        });
        assert!((s.take_changed().unwrap() - 90.0).abs() < 2.0);
    }

    #[test]
    fn arrows_step() {
        let mut s = HueSlider::new().hue(10.0);
        laid_out(&mut s, 200.0, 26.0);
        s.event(&mut EventContext {
            event: &WidgetEvent::KeyPressed {
                key: "ArrowRight".to_string(),
                repeat: false,
            },
            bounds: s.bounds,
            scale: 1.0,
        });
        assert_eq!(s.hue_value(), 11.0);
        assert_eq!(s.take_changed(), Some(11.0));
    }

    #[test]
    fn hsv_primary_colors() {
        assert_eq!(hsv(0.0, 1.0, 1.0), [255, 0, 0, 255]);
        assert_eq!(hsv(120.0, 1.0, 1.0), [0, 255, 0, 255]);
        assert_eq!(hsv(240.0, 1.0, 1.0), [0, 0, 255, 255]);
        assert_eq!(hsv(360.0, 1.0, 1.0), [255, 0, 0, 255]);
    }
}
