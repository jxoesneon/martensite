//! `Joystick` — a spring-return analog stick (drag the knob
//! inside a circular gate; it snaps back to center on release —
//! the gamepad / RC-controller idiom, distinct from
//! [`crate::widgets::xy_pad::XYPad`] which holds its position).
//!
//! [`Joystick::value_xy`] reads the normalized offset
//! (`-1.0..=1.0` per axis, y positive upward). Movement parks
//! [`Joystick::take_changed`]. Arrow keys nudge the knob while
//! focused; releasing the pointer (or pressing `Escape`)
//! recenters it.
//!
//! # Examples
//!
//! ```
//! use martensite::widgets::joystick::Joystick;
//!
//! let j = Joystick::new();
//! assert_eq!(j.value_xy(), (0.0, 0.0));
//! ```

use accesskit::Node as AccessKitNode;
use glam::Vec2;
use martensite_core::{
    EventContext, EventResponse, LayoutConstraints, LayoutContext, PaintContext, PointerButton,
    Rect, RenderMinimum, UnderflowPolicy, Widget, WidgetEvent,
};
use martensite_theme::TokenKey;

const SIZE_PT: f32 = 120.0;
/// Knob travel as a fraction of the gate radius.
const TRAVEL: f32 = 0.6;
const KNOB_PT: f32 = 18.0;

const FACE: [u8; 4] = [36, 36, 42, 255];
const EDGE: [u8; 4] = [70, 70, 76, 255];
const RING: [u8; 4] = [58, 58, 66, 255];
const KNOB: [u8; 4] = [110, 170, 230, 255];
const KNOB_HI: [u8; 4] = [140, 195, 245, 255];

/// A spring-return analog stick — see the module docs.
///
/// ```
/// use martensite::widgets::joystick::Joystick;
///
/// assert_eq!(Joystick::new().value_xy(), (0.0, 0.0));
/// ```
#[derive(Debug)]
pub struct Joystick {
    /// Accessibility label.
    pub label: String,
    /// Normalized offset, `-1..=1` per axis (y up).
    value: Vec2,
    /// Dead-zone radius (normalized) reported as zero.
    dead_zone: f32,
    /// Spring-return enabled (always true for the idiom; a flag
    /// for future hold-mode parity with `XYPad`).
    spring: bool,
    dragging: bool,
    changed: bool,
    bounds: Rect,
    scale: f32,
}

impl Default for Joystick {
    fn default() -> Self {
        Self::new()
    }
}

impl Joystick {
    /// Creates a centered stick.
    ///
    /// ```
    /// use martensite::widgets::joystick::Joystick;
    ///
    /// assert_eq!(Joystick::new().value_xy(), (0.0, 0.0));
    /// ```
    pub fn new() -> Self {
        Self {
            label: "Joystick".to_string(),
            value: Vec2::ZERO,
            dead_zone: 0.05,
            spring: true,
            dragging: false,
            changed: false,
            bounds: Rect::new(0.0, 0.0, 0.0, 0.0),
            scale: 1.0,
        }
    }

    /// Sets the dead-zone radius (offsets inside read as zero).
    ///
    /// ```
    /// use martensite::widgets::joystick::Joystick;
    ///
    /// assert_eq!(Joystick::new().dead_zone(0.2).dead_zone_value(), 0.2);
    /// ```
    pub fn dead_zone(mut self, radius: f32) -> Self {
        self.dead_zone = radius.clamp(0.0, 1.0);
        self
    }

    /// Disables spring return (knob holds position like `XYPad`).
    ///
    /// ```
    /// use martensite::widgets::joystick::Joystick;
    ///
    /// assert!(!Joystick::new().spring(false).has_spring());
    /// ```
    pub fn spring(mut self, on: bool) -> Self {
        self.spring = on;
        self
    }

    /// Accessibility label.
    ///
    /// ```
    /// use martensite::widgets::joystick::Joystick;
    ///
    /// assert_eq!(Joystick::new().label("Aim").label, "Aim");
    /// ```
    pub fn label(mut self, label: impl Into<String>) -> Self {
        self.label = label.into();
        self
    }

    /// Normalized offset `(x, y)` in `-1.0..=1.0` (y up, dead
    /// zone applied).
    ///
    /// ```
    /// use martensite::widgets::joystick::Joystick;
    ///
    /// assert_eq!(Joystick::new().value_xy(), (0.0, 0.0));
    /// ```
    pub fn value_xy(&self) -> (f32, f32) {
        if self.value.length() < self.dead_zone {
            (0.0, 0.0)
        } else {
            (self.value.x, self.value.y)
        }
    }

    /// Deflection magnitude `0.0..=1.0`.
    ///
    /// ```
    /// use martensite::widgets::joystick::Joystick;
    ///
    /// assert_eq!(Joystick::new().magnitude(), 0.0);
    /// ```
    pub fn magnitude(&self) -> f32 {
        self.value.length().min(1.0)
    }

    /// Whether spring-return is enabled.
    ///
    /// ```
    /// use martensite::widgets::joystick::Joystick;
    ///
    /// assert!(Joystick::new().has_spring());
    /// ```
    pub fn has_spring(&self) -> bool {
        self.spring
    }

    /// Dead-zone radius.
    ///
    /// ```
    /// use martensite::widgets::joystick::Joystick;
    ///
    /// assert_eq!(Joystick::new().dead_zone_value(), 0.05);
    /// ```
    pub fn dead_zone_value(&self) -> f32 {
        self.dead_zone
    }

    /// Drains whether the value moved since the last call.
    ///
    /// ```
    /// use martensite::widgets::joystick::Joystick;
    ///
    /// assert!(!Joystick::new().take_changed());
    /// ```
    pub fn take_changed(&mut self) -> bool {
        std::mem::take(&mut self.changed)
    }

    /// Gate center in widget coordinates.
    fn center(&self) -> Vec2 {
        Vec2::new(
            (self.bounds.min_x() + self.bounds.max_x()) / 2.0,
            (self.bounds.min_y() + self.bounds.max_y()) / 2.0,
        )
    }

    /// Gate radius in pixels.
    fn gate_r(&self) -> f32 {
        (self.bounds.width().min(self.bounds.height()) / 2.0 - 4.0 * self.scale).max(1.0)
    }

    /// Set value from a pointer position (clamped to the unit
    /// circle, y flipped to positive-up).
    fn update_from(&mut self, p: Vec2) {
        let d = p - self.center();
        let mut v = Vec2::new(d.x, -d.y) / (self.gate_r() * TRAVEL);
        if v.length() > 1.0 {
            v = v.normalize();
        }
        if v != self.value {
            self.value = v;
            self.changed = true;
        }
    }

    /// Knob position in widget coordinates.
    fn knob_pos(&self) -> Vec2 {
        let c = self.center();
        c + Vec2::new(self.value.x, -self.value.y) * (self.gate_r() * TRAVEL)
    }
}

impl Widget for Joystick {
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
        node.set_role(accesskit::Role::Group);
        node.set_label(format!(
            "{} — x {:.2}, y {:.2}",
            self.label, self.value.x, self.value.y
        ));
    }

    fn event(&mut self, cx: &mut EventContext) -> EventResponse {
        match cx.event {
            WidgetEvent::PointerPressed {
                button: PointerButton::Primary,
                position,
                ..
            } => {
                let grab = self.knob_pos().distance(*position) <= KNOB_PT * self.scale;
                if grab || self.center().distance(*position) <= self.gate_r() {
                    self.dragging = true;
                    self.update_from(*position);
                    return EventResponse::CapturePointer;
                }
                EventResponse::Ignored
            }
            WidgetEvent::PointerMoved { position } => {
                if self.dragging {
                    self.update_from(*position);
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
                    if self.spring && self.value != Vec2::ZERO {
                        self.value = Vec2::ZERO;
                        self.changed = true;
                    }
                    return EventResponse::RequestRepaint;
                }
                EventResponse::Ignored
            }
            WidgetEvent::KeyPressed { key, .. } => {
                let step = 0.1;
                let d = match key.as_str() {
                    "ArrowLeft" => Vec2::new(-step, 0.0),
                    "ArrowRight" => Vec2::new(step, 0.0),
                    "ArrowUp" => Vec2::new(0.0, step),
                    "ArrowDown" => Vec2::new(0.0, -step),
                    "Escape" => {
                        if self.value != Vec2::ZERO {
                            self.value = Vec2::ZERO;
                            self.changed = true;
                            return EventResponse::RequestRepaint;
                        }
                        return EventResponse::Ignored;
                    }
                    _ => return EventResponse::Ignored,
                };
                let mut v = self.value + d;
                if v.length() > 1.0 {
                    v = v.normalize();
                }
                self.value = v;
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
        let r = self.gate_r();
        let edge = cx.color(TokenKey::BorderColor, EDGE);
        // Gate ring.
        cx.list.push_fill_shape(
            kurbo::Rect::new(
                f64::from(c.x - r),
                f64::from(c.y - r),
                f64::from(c.x + r),
                f64::from(c.y + r),
            ),
            &martensite_core::shape::Shape::circle(c, r),
            RING,
        );
        cx.list.push_stroke_shape(
            kurbo::Rect::new(
                f64::from(c.x - r),
                f64::from(c.y - r),
                f64::from(c.x + r),
                f64::from(c.y + r),
            ),
            &martensite_core::shape::Shape::circle(c, r),
            cx.pt(1.0),
            edge,
        );
        // Dead-zone hint.
        let dz = r * TRAVEL * self.dead_zone;
        if dz > 2.0 {
            cx.list.push_stroke_shape(
                kurbo::Rect::new(
                    f64::from(c.x - dz),
                    f64::from(c.y - dz),
                    f64::from(c.x + dz),
                    f64::from(c.y + dz),
                ),
                &martensite_core::shape::Shape::circle(c, dz),
                cx.pt(0.5),
                edge,
            );
        }
        // Knob.
        let kp = self.knob_pos();
        let kr = KNOB_PT * self.scale;
        cx.list.push_fill_shape(
            kurbo::Rect::new(
                f64::from(kp.x - kr),
                f64::from(kp.y - kr),
                f64::from(kp.x + kr),
                f64::from(kp.y + kr),
            ),
            &martensite_core::shape::Shape::circle(kp, kr),
            cx.color(
                TokenKey::AccentColor,
                if self.dragging { KNOB_HI } else { KNOB },
            ),
        );
        cx.list.push_stroke_shape(
            kurbo::Rect::new(
                f64::from(kp.x - kr),
                f64::from(kp.y - kr),
                f64::from(kp.x + kr),
                f64::from(kp.y + kr),
            ),
            &martensite_core::shape::Shape::circle(kp, kr),
            cx.pt(1.0),
            edge,
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

    fn laid_out(j: &mut Joystick, w: f32, h: f32) {
        let mut hot = HotNode::default();
        let mut cx = LayoutContext {
            hot: &mut hot,
            scale: 1.0,
        };
        j.measure(
            &mut cx,
            LayoutConstraints {
                min_size: Vec2::ZERO,
                max_size: Vec2::new(w, h),
            },
        );
        j.layout(&mut cx, Rect::new(0.0, 0.0, w, h));
    }

    fn ev(j: &mut Joystick, e: &WidgetEvent) {
        j.event(&mut EventContext {
            event: e,
            bounds: j.bounds,
            scale: 1.0,
        });
    }

    #[test]
    fn defaults() {
        let j = Joystick::new();
        assert_eq!(j.value_xy(), (0.0, 0.0));
        assert!(j.has_spring());
        assert_eq!(j.magnitude(), 0.0);
    }

    #[test]
    fn drag_deflects_then_springs_back() {
        let mut j = Joystick::new();
        laid_out(&mut j, 120.0, 120.0);
        let edge = Vec2::new(110.0, 60.0); // far right of gate
        ev(
            &mut j,
            &WidgetEvent::PointerPressed {
                button: PointerButton::Primary,
                position: edge,
                count: 1,
            },
        );
        let (x, y) = j.value_xy();
        assert!(x > 0.9 && y.abs() < 0.1);
        assert!(j.take_changed());
        ev(
            &mut j,
            &WidgetEvent::PointerReleased {
                button: PointerButton::Primary,
                position: edge,
            },
        );
        assert_eq!(j.value_xy(), (0.0, 0.0));
        assert!(j.take_changed()); // recenter counts as a change
    }

    #[test]
    fn clamps_to_unit_circle() {
        let mut j = Joystick::new();
        laid_out(&mut j, 120.0, 120.0);
        ev(
            &mut j,
            &WidgetEvent::PointerPressed {
                button: PointerButton::Primary,
                position: Vec2::new(119.0, 1.0), // corner, outside
                count: 1,
            },
        );
        assert!(j.magnitude() <= 1.0);
    }

    #[test]
    fn dead_zone_reads_zero() {
        let mut j = Joystick::new().dead_zone(0.9);
        laid_out(&mut j, 120.0, 120.0);
        ev(
            &mut j,
            &WidgetEvent::PointerPressed {
                button: PointerButton::Primary,
                position: Vec2::new(70.0, 60.0), // small offset
                count: 1,
            },
        );
        assert_eq!(j.value_xy(), (0.0, 0.0));
    }

    #[test]
    fn arrows_nudge_escape_centers() {
        let mut j = Joystick::new();
        laid_out(&mut j, 120.0, 120.0);
        ev(
            &mut j,
            &WidgetEvent::KeyPressed {
                key: "ArrowRight".to_string(),
                repeat: false,
            },
        );
        assert!(j.value_xy().0 > 0.0);
        ev(
            &mut j,
            &WidgetEvent::KeyPressed {
                key: "Escape".to_string(),
                repeat: false,
            },
        );
        assert_eq!(j.value_xy(), (0.0, 0.0));
    }
}
