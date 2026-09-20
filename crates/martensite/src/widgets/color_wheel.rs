//! `ColorWheel` — a hue ring with a draggable selector (the
//! classic color-wheel picker; design-tool sibling of
//! [`crate::widgets::color_picker::ColorPicker`] and
//! [`crate::widgets::curve_editor::CurveEditor`]).
//!
//! The ring sweeps the full hue circle; dragging the selector
//! around it picks a hue `0.0..=360.0` and parks
//! [`ColorWheel::take_changed`]. [`ColorWheel::rgb`] converts
//! the selection (with `saturation`/`brightness` builders) to
//! RGBA bytes. Arrow keys rotate the selector in 5° steps.
//!
//! # Examples
//!
//! ```
//! use martensite::widgets::color_wheel::ColorWheel;
//!
//! let w = ColorWheel::new().hue(120.0);
//! assert_eq!(w.hue_value(), 120.0);
//! assert_eq!(w.rgb(), [0, 255, 0, 255]); // pure green
//! ```

use accesskit::Node as AccessKitNode;
use glam::Vec2;
use martensite_core::{
    EventContext, EventResponse, LayoutConstraints, LayoutContext, PaintContext, PointerButton,
    Rect, RenderMinimum, UnderflowPolicy, Widget, WidgetEvent,
};
use martensite_theme::TokenKey;
use std::f32::consts::TAU;

const SIZE_PT: f32 = 140.0;
/// Ring thickness as a fraction of outer radius.
const THICK: f32 = 0.22;
const HANDLE_PT: f32 = 7.0;
/// Hue arc segments.
const SEGS: usize = 48;

const FACE: [u8; 4] = [36, 36, 42, 255];
const EDGE: [u8; 4] = [70, 70, 76, 255];

/// A hue-ring color picker — see the module docs.
///
/// ```
/// use martensite::widgets::color_wheel::ColorWheel;
///
/// assert_eq!(ColorWheel::new().hue_value(), 0.0);
/// ```
#[derive(Debug)]
pub struct ColorWheel {
    /// Accessibility label.
    pub label: String,
    hue: f32,
    saturation: f32,
    brightness: f32,
    dragging: bool,
    changed: bool,
    bounds: Rect,
    scale: f32,
}

impl Default for ColorWheel {
    fn default() -> Self {
        Self::new()
    }
}

impl ColorWheel {
    /// Creates a red (`0°`) wheel at full saturation/brightness.
    ///
    /// ```
    /// use martensite::widgets::color_wheel::ColorWheel;
    ///
    /// assert_eq!(ColorWheel::new().hue_value(), 0.0);
    /// ```
    pub fn new() -> Self {
        Self {
            label: "Color wheel".to_string(),
            hue: 0.0,
            saturation: 1.0,
            brightness: 1.0,
            dragging: false,
            changed: false,
            bounds: Rect::new(0.0, 0.0, 0.0, 0.0),
            scale: 1.0,
        }
    }

    /// Sets the hue in degrees `0..360` (wrapped).
    ///
    /// ```
    /// use martensite::widgets::color_wheel::ColorWheel;
    ///
    /// assert_eq!(ColorWheel::new().hue(370.0).hue_value(), 10.0);
    /// ```
    pub fn hue(mut self, degrees: f32) -> Self {
        self.hue = degrees.rem_euclid(360.0);
        self
    }

    /// Saturation `0..=1` for `rgb` conversion.
    ///
    /// ```
    /// use martensite::widgets::color_wheel::ColorWheel;
    ///
    /// assert_eq!(ColorWheel::new().saturation(0.5).sat(), 0.5);
    /// ```
    pub fn saturation(mut self, s: f32) -> Self {
        self.saturation = s.clamp(0.0, 1.0);
        self
    }

    /// Brightness `0..=1` for `rgb` conversion.
    ///
    /// ```
    /// use martensite::widgets::color_wheel::ColorWheel;
    ///
    /// assert_eq!(ColorWheel::new().brightness(0.5).bright(), 0.5);
    /// ```
    pub fn brightness(mut self, v: f32) -> Self {
        self.brightness = v.clamp(0.0, 1.0);
        self
    }

    /// Accessibility label.
    ///
    /// ```
    /// use martensite::widgets::color_wheel::ColorWheel;
    ///
    /// assert_eq!(ColorWheel::new().label("Theme").label, "Theme");
    /// ```
    pub fn label(mut self, label: impl Into<String>) -> Self {
        self.label = label.into();
        self
    }

    /// Current hue in degrees.
    ///
    /// ```
    /// use martensite::widgets::color_wheel::ColorWheel;
    ///
    /// assert_eq!(ColorWheel::new().hue(240.0).hue_value(), 240.0);
    /// ```
    pub fn hue_value(&self) -> f32 {
        self.hue
    }

    /// Current saturation.
    ///
    /// ```
    /// use martensite::widgets::color_wheel::ColorWheel;
    ///
    /// assert_eq!(ColorWheel::new().sat(), 1.0);
    /// ```
    pub fn sat(&self) -> f32 {
        self.saturation
    }

    /// Current brightness.
    ///
    /// ```
    /// use martensite::widgets::color_wheel::ColorWheel;
    ///
    /// assert_eq!(ColorWheel::new().bright(), 1.0);
    /// ```
    pub fn bright(&self) -> f32 {
        self.brightness
    }

    /// Selected color as RGBA bytes (HSV conversion).
    ///
    /// ```
    /// use martensite::widgets::color_wheel::ColorWheel;
    ///
    /// assert_eq!(ColorWheel::new().hue(60.0).rgb(), [255, 255, 0, 255]);
    /// ```
    pub fn rgb(&self) -> [u8; 4] {
        hsv_to_rgb(self.hue, self.saturation, self.brightness)
    }

    /// Drains whether the hue moved since the last call.
    ///
    /// ```
    /// use martensite::widgets::color_wheel::ColorWheel;
    ///
    /// assert!(!ColorWheel::new().take_changed());
    /// ```
    pub fn take_changed(&mut self) -> bool {
        std::mem::take(&mut self.changed)
    }

    /// Wheel center in widget coordinates.
    fn center(&self) -> Vec2 {
        Vec2::new(
            (self.bounds.min_x() + self.bounds.max_x()) / 2.0,
            (self.bounds.min_y() + self.bounds.max_y()) / 2.0,
        )
    }

    /// Outer radius in pixels.
    fn radius(&self) -> f32 {
        (self.bounds.width().min(self.bounds.height()) / 2.0 - 4.0 * self.scale).max(1.0)
    }

    /// Set the hue from a pointer angle (0° = +x, CCW positive).
    fn set_from(&mut self, p: Vec2) {
        let d = p - self.center();
        if d.length() < 1.0 {
            return;
        }
        // Degrees CCW from +x axis; the wheel maps atan2 so
        // that 0° hue sits at 3 o'clock and sweeps upward.
        let deg = (-d.y).atan2(d.x).to_degrees().rem_euclid(360.0);
        if (deg - self.hue).abs() > 1e-4 {
            self.hue = deg;
            self.changed = true;
        }
    }
}

/// HSV → RGBA (s, v in `0..=1`; h in degrees).
fn hsv_to_rgb(h: f32, s: f32, v: f32) -> [u8; 4] {
    let h = h.rem_euclid(360.0) / 60.0;
    let c = v * s;
    let x = c * (1.0 - (h % 2.0 - 1.0).abs());
    let m = v - c;
    let (r, g, b) = match h as u32 {
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

impl Widget for ColorWheel {
    fn measure(&mut self, cx: &mut LayoutContext, constraints: LayoutConstraints) -> Vec2 {
        let s = cx.pt(SIZE_PT);
        Vec2::new(
            s.min(constraints.max_size.x.max(0.0)),
            s.min(constraints.max_size.y.max(0.0)),
        )
    }

    fn min_render(&self) -> RenderMinimum {
        RenderMinimum::new(Vec2::new(48.0, 48.0)).with_policy(UnderflowPolicy::Lint)
    }

    fn layout(&mut self, cx: &mut LayoutContext, bounds: Rect) {
        self.bounds = bounds;
        self.scale = cx.scale;
    }

    fn accessibility(&self, node: &mut AccessKitNode) {
        node.set_role(accesskit::Role::ColorWell);
        node.set_label(format!("{} — hue {:.0}°", self.label, self.hue));
    }

    fn event(&mut self, cx: &mut EventContext) -> EventResponse {
        match cx.event {
            WidgetEvent::PointerPressed {
                button: PointerButton::Primary,
                position,
                ..
            } => {
                let d = (*position - self.center()).length();
                let r = self.radius();
                if d <= r && d >= r * (1.0 - THICK) * 0.7 {
                    self.dragging = true;
                    self.set_from(*position);
                    return EventResponse::CapturePointer;
                }
                EventResponse::Ignored
            }
            WidgetEvent::PointerMoved { position } => {
                if self.dragging {
                    self.set_from(*position);
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
                    return EventResponse::RequestRepaint;
                }
                EventResponse::Ignored
            }
            WidgetEvent::KeyPressed { key, .. } => {
                let d = match key.as_str() {
                    "ArrowLeft" | "ArrowDown" => -5.0,
                    "ArrowRight" | "ArrowUp" => 5.0,
                    _ => return EventResponse::Ignored,
                };
                self.hue = (self.hue + d).rem_euclid(360.0);
                self.changed = true;
                EventResponse::RequestRepaint
            }
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
            &martensite_core::shape::Shape::rounded(cx.pt(4.0)),
            cx.color(TokenKey::SurfaceColor, FACE),
        );
        let c = self.center();
        let r = self.radius();
        let edge = cx.color(TokenKey::BorderColor, EDGE);
        // Hue ring as SEGS quad strips — each segment gets the
        // midpoint hue's full-saturation color.
        let ri = r * (1.0 - THICK);
        for i in 0..SEGS {
            let a0 = (i as f32 / SEGS as f32) * TAU;
            let a1 = ((i + 1) as f32 / SEGS as f32) * TAU;
            let mid = (a0 + a1) / 2.0;
            // atan2 sweep is CCW in math coords; our y grows
            // down, so hue increases clockwise visually.
            let color = hsv_to_rgb(mid.to_degrees(), 1.0, 1.0);
            let p = |a: f32, rad: f32| {
                (
                    f64::from(c.x + a.cos() * rad),
                    f64::from(c.y - a.sin() * rad),
                )
            };
            let mut path = kurbo::BezPath::new();
            path.move_to(p(a0, ri));
            path.line_to(p(a0, r));
            let steps = 4;
            for k in 1..=steps {
                let a = a0 + (a1 - a0) * k as f32 / steps as f32;
                path.line_to(p(a, r));
            }
            path.line_to(p(a1, ri));
            for k in (0..steps).rev() {
                let a = a0 + (a1 - a0) * k as f32 / steps as f32;
                path.line_to(p(a, ri));
            }
            path.close_path();
            cx.list.push_path(path, color);
        }
        // Selection handle on the ring mid-radius.
        let hr = (r + ri) / 2.0;
        let a = self.hue.to_radians();
        let hp = Vec2::new(c.x + a.cos() * hr, c.y - a.sin() * hr);
        let hs = HANDLE_PT * self.scale;
        cx.list.push_fill_shape(
            kurbo::Rect::new(
                f64::from(hp.x - hs),
                f64::from(hp.y - hs),
                f64::from(hp.x + hs),
                f64::from(hp.y + hs),
            ),
            &martensite_core::shape::Shape::circle(hp, hs),
            self.rgb(),
        );
        cx.list.push_stroke_shape(
            kurbo::Rect::new(
                f64::from(hp.x - hs),
                f64::from(hp.y - hs),
                f64::from(hp.x + hs),
                f64::from(hp.y + hs),
            ),
            &martensite_core::shape::Shape::circle(hp, hs),
            cx.pt(1.5),
            [240, 240, 245, 255],
        );
        // Center swatch shows the picked color.
        let sw = ri * 0.6;
        cx.list.push_fill_shape(
            kurbo::Rect::new(
                f64::from(c.x - sw),
                f64::from(c.y - sw),
                f64::from(c.x + sw),
                f64::from(c.y + sw),
            ),
            &martensite_core::shape::Shape::circle(c, sw),
            self.rgb(),
        );
        cx.list.push_stroke_shape(
            krect(self.bounds),
            &martensite_core::shape::Shape::rounded(cx.pt(4.0)),
            cx.pt(0.75),
            edge,
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use martensite_core::HotNode;

    fn laid_out(w: &mut ColorWheel, wd: f32, h: f32) {
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

    fn ev(w: &mut ColorWheel, e: &WidgetEvent) {
        w.event(&mut EventContext {
            event: e,
            bounds: w.bounds,
            scale: 1.0,
        });
    }

    #[test]
    fn hsv_conversion() {
        assert_eq!(hsv_to_rgb(0.0, 1.0, 1.0), [255, 0, 0, 255]);
        assert_eq!(hsv_to_rgb(120.0, 1.0, 1.0), [0, 255, 0, 255]);
        assert_eq!(hsv_to_rgb(240.0, 1.0, 1.0), [0, 0, 255, 255]);
        assert_eq!(hsv_to_rgb(0.0, 0.0, 0.5), [128, 128, 128, 255]);
    }

    #[test]
    fn hue_wraps() {
        assert_eq!(ColorWheel::new().hue(370.0).hue_value(), 10.0);
        assert_eq!(ColorWheel::new().hue(-30.0).hue_value(), 330.0);
    }

    #[test]
    fn drag_picks_hue() {
        let mut w = ColorWheel::new();
        laid_out(&mut w, 140.0, 140.0);
        // Top of wheel (12 o'clock) = 90°.
        ev(
            &mut w,
            &WidgetEvent::PointerPressed {
                button: PointerButton::Primary,
                position: Vec2::new(70.0, 12.0),
                count: 1,
            },
        );
        assert!((w.hue_value() - 90.0).abs() < 5.0);
        assert!(w.take_changed());
        ev(
            &mut w,
            &WidgetEvent::PointerReleased {
                button: PointerButton::Primary,
                position: Vec2::new(70.0, 12.0),
            },
        );
    }

    #[test]
    fn arrows_rotate() {
        let mut w = ColorWheel::new().hue(10.0);
        laid_out(&mut w, 140.0, 140.0);
        ev(
            &mut w,
            &WidgetEvent::KeyPressed {
                key: "ArrowRight".to_string(),
                repeat: false,
            },
        );
        assert_eq!(w.hue_value(), 15.0);
        ev(
            &mut w,
            &WidgetEvent::KeyPressed {
                key: "ArrowLeft".to_string(),
                repeat: false,
            },
        );
        assert_eq!(w.hue_value(), 10.0);
    }

    #[test]
    fn center_click_ignored() {
        let mut w = ColorWheel::new();
        laid_out(&mut w, 140.0, 140.0);
        ev(
            &mut w,
            &WidgetEvent::PointerPressed {
                button: PointerButton::Primary,
                position: Vec2::new(70.0, 70.0), // hub, not the ring
                count: 1,
            },
        );
        assert_eq!(w.hue_value(), 0.0);
        assert!(!w.take_changed());
    }
}
