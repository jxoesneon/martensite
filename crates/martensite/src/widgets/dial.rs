//! `Dial` widget: a rotary knob — circular value control (Qt `QDial`,
//! audio-plugin knob idiom).
//!
//! The value maps onto a 270° sweep (from −225° to +45° measured
//! from the positive x-axis, i.e. bottom-left up over the top to
//! bottom-right). Vertical drag or scroll adjusts the value; arrow
//! keys step it. Poll [`Dial::take_changed`].
//!
//! # Examples
//!
//! ```
//! use martensite::widgets::dial::Dial;
//!
//! let d = Dial::new().range(0.0, 100.0).value(25.0);
//! assert_eq!(d.get_value(), 25.0);
//! ```

use accesskit::Node as AccessKitNode;
use glam::Vec2;
use martensite_core::widget::{
    EventContext, EventResponse, LayoutConstraints, LayoutContext, PaintContext, PointerButton,
    Widget, WidgetEvent,
};
use martensite_core::{Rect, TokenKey};

/// Knob diameter, logical points.
const SIZE_PT: f32 = 56.0;
/// Arc stroke width, logical points.
const ARC_PT: f32 = 5.0;
/// Needle length as a fraction of the radius.
const NEEDLE_FRAC: f32 = 0.62;
/// Drag pixels per full range, logical points.
const DRAG_SPAN_PT: f32 = 150.0;

/// Sweep: value `min` sits at 225° (bottom-left), `max` at −45°
/// (bottom-right) — a 270° arc over the top.
const START_DEG: f64 = 225.0;
/// Total sweep.
const SWEEP_DEG: f64 = -270.0;

/// Track arc ink.
const TRACK: [u8; 4] = [222, 225, 231, 255];
/// Face.
const FACE: [u8; 4] = [245, 246, 248, 255];
/// Face border.
const BORDER: [u8; 4] = [190, 194, 202, 255];
/// Needle ink.
const NEEDLE: [u8; 4] = [30, 31, 36, 255];
/// Value arc.
const VALUE: [u8; 4] = [70, 110, 200, 255];

/// A rotary knob.
///
/// # Examples
///
/// ```
/// use martensite::widgets::dial::Dial;
///
/// let d = Dial::new().range(-1.0, 1.0).value(0.5).step(0.05);
/// ```
pub struct Dial {
    /// Range minimum.
    min: f64,
    /// Range maximum.
    max: f64,
    /// Current value.
    value: f64,
    /// Arrow-key/scroll step.
    step: f64,
    /// Enabled flag.
    enabled: bool,
    /// Drag state — the value at press + accumulated delta.
    drag: Option<(f64, f32)>,
    /// Pending change notification.
    changed: Option<f64>,
    /// Cached bounds.
    bounds: Rect,
}

impl Dial {
    /// Creates a 0..1 knob at 0.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite::widgets::dial::Dial;
    ///
    /// let d = Dial::new();
    /// assert_eq!(d.get_value(), 0.0);
    /// ```
    #[must_use]
    pub fn new() -> Self {
        Self {
            min: 0.0,
            max: 1.0,
            value: 0.0,
            step: 0.0,
            enabled: true,
            drag: None,
            changed: None,
            bounds: Rect::default(),
        }
    }

    /// Sets the range.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite::widgets::dial::Dial;
    ///
    /// let d = Dial::new().range(0.0, 127.0);
    /// ```
    #[must_use]
    pub fn range(mut self, min: f64, max: f64) -> Self {
        self.min = min;
        self.max = max.max(min);
        self.value = self.value.clamp(self.min, self.max);
        self
    }

    /// Sets the value (clamped).
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite::widgets::dial::Dial;
    ///
    /// let d = Dial::new().value(0.4);
    /// ```
    #[must_use]
    pub fn value(mut self, value: f64) -> Self {
        self.value = value.clamp(self.min, self.max);
        self
    }

    /// Sets the arrow/scroll step (default: 1% of range).
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite::widgets::dial::Dial;
    ///
    /// let d = Dial::new().step(0.1);
    /// ```
    #[must_use]
    pub fn step(mut self, step: f64) -> Self {
        self.step = step.max(0.0);
        self
    }

    /// Enables or disables the knob.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite::widgets::dial::Dial;
    ///
    /// let d = Dial::new().enabled(false);
    /// ```
    #[must_use]
    pub fn enabled(mut self, enabled: bool) -> Self {
        self.enabled = enabled;
        self
    }

    /// The current value.
    #[inline]
    #[must_use]
    pub fn get_value(&self) -> f64 {
        self.value
    }

    /// Sets the value programmatically (no notification).
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite::widgets::dial::Dial;
    ///
    /// let mut d = Dial::new();
    /// d.set_value(0.7);
    /// assert_eq!(d.get_value(), 0.7);
    /// ```
    pub fn set_value(&mut self, value: f64) {
        self.value = value.clamp(self.min, self.max);
    }

    /// Drains a change notification.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite::widgets::dial::Dial;
    ///
    /// let mut d = Dial::new();
    /// assert_eq!(d.take_changed(), None);
    /// ```
    pub fn take_changed(&mut self) -> Option<f64> {
        self.changed.take()
    }

    /// Effective step (explicit or 1% of range).
    fn eff_step(&self) -> f64 {
        if self.step > 0.0 {
            self.step
        } else {
            (self.max - self.min) / 100.0
        }
    }

    /// Value fraction `0..=1`.
    fn frac(&self) -> f64 {
        if self.max <= self.min {
            0.0
        } else {
            (self.value - self.min) / (self.max - self.min)
        }
    }

    /// Applies a user-set value with notification.
    fn commit(&mut self, value: f64) {
        let new = value.clamp(self.min, self.max);
        if (new - self.value).abs() > f64::EPSILON {
            self.value = new;
            self.changed = Some(new);
        }
    }
}

impl Default for Dial {
    fn default() -> Self {
        Self::new()
    }
}

/// Samples `sweep` degrees of arc starting at `start_deg` into a
/// `BezPath` polyline centered at `center` with `radius`.
fn arc_path(center: kurbo::Point, radius: f64, start_deg: f64, sweep_deg: f64) -> kurbo::BezPath {
    let mut path = kurbo::BezPath::new();
    let segs = 32usize;
    for i in 0..=segs {
        let t = (start_deg + sweep_deg * i as f64 / segs as f64).to_radians();
        let p = kurbo::Point::new(center.x + radius * t.cos(), center.y + radius * t.sin());
        if i == 0 {
            path.move_to(p);
        } else {
            path.line_to(p);
        }
    }
    path
}

impl Widget for Dial {
    fn measure(&mut self, cx: &mut LayoutContext, _constraints: LayoutConstraints) -> Vec2 {
        let s = cx.pt(SIZE_PT);
        Vec2::new(s, s)
    }

    fn layout(&mut self, _cx: &mut LayoutContext, bounds: Rect) {
        self.bounds = bounds;
    }

    fn accessibility(&self, node: &mut AccessKitNode) {
        node.set_role(accesskit::Role::Slider);
        node.set_label("Dial");
        node.set_numeric_value(self.value);
        node.set_min_numeric_value(self.min);
        node.set_max_numeric_value(self.max);
        if !self.enabled {
            node.set_disabled();
        }
        if self.enabled {
            node.add_action(accesskit::Action::SetValue);
            node.add_action(accesskit::Action::Increment);
            node.add_action(accesskit::Action::Decrement);
        }
    }

    fn event(&mut self, cx: &mut EventContext) -> EventResponse {
        if !self.enabled {
            return EventResponse::Ignored;
        }
        match cx.event {
            WidgetEvent::PointerPressed {
                button: PointerButton::Primary,
                position,
                ..
            } => {
                if self.bounds.contains(*position) {
                    self.drag = Some((self.value, position.y));
                    return EventResponse::CapturePointer;
                }
                EventResponse::Ignored
            }
            WidgetEvent::PointerMoved { position } => {
                if let Some((start_value, start_y)) = self.drag {
                    let span = cx.scale * DRAG_SPAN_PT;
                    // Up = increase (standard knob drag).
                    let delta =
                        f64::from(start_y - position.y) / f64::from(span) * (self.max - self.min);
                    self.commit(start_value + delta);
                    return EventResponse::RequestRepaint;
                }
                EventResponse::Ignored
            }
            WidgetEvent::PointerReleased { .. } => {
                if self.drag.take().is_some() {
                    return EventResponse::Handled;
                }
                EventResponse::Ignored
            }
            WidgetEvent::Scroll { delta, .. } => {
                let step = self.eff_step();
                self.commit(self.value + f64::from(delta.y).signum() * step);
                EventResponse::Handled
            }
            WidgetEvent::KeyPressed { key, .. } => match key.as_str() {
                "ArrowUp" | "ArrowRight" => {
                    self.commit(self.value + self.eff_step());
                    EventResponse::Handled
                }
                "ArrowDown" | "ArrowLeft" => {
                    self.commit(self.value - self.eff_step());
                    EventResponse::Handled
                }
                "Home" => {
                    self.commit(self.min);
                    EventResponse::Handled
                }
                "End" => {
                    self.commit(self.max);
                    EventResponse::Handled
                }
                _ => EventResponse::Ignored,
            },
            WidgetEvent::SemanticAction(action) => match action {
                martensite_core::widget::SemanticAction::Increment => {
                    self.commit(self.value + self.eff_step());
                    EventResponse::Handled
                }
                martensite_core::widget::SemanticAction::Decrement => {
                    self.commit(self.value - self.eff_step());
                    EventResponse::Handled
                }
                _ => EventResponse::Ignored,
            },
            _ => EventResponse::Ignored,
        }
    }

    fn paint(&self, cx: &mut PaintContext) {
        let r = kurbo::Rect::new(
            f64::from(cx.bounds.min_x()),
            f64::from(cx.bounds.min_y()),
            f64::from(cx.bounds.max_x()),
            f64::from(cx.bounds.max_y()),
        );
        let center = kurbo::Point::new((r.x0 + r.x1) / 2.0, (r.y0 + r.y1) / 2.0);
        let radius = (r.width().min(r.height())) / 2.0;
        let arc_w = f64::from(cx.pt(ARC_PT));
        let arc_r = radius - arc_w / 2.0 - 1.0;
        let track_ink = cx.color(TokenKey::DividerColor, TRACK);
        let value_ink = cx.color(TokenKey::AccentColor, VALUE);

        // Full sweep track arc.
        let track = arc_path(center, arc_r, START_DEG, SWEEP_DEG);
        cx.list.push_stroke_path(track, cx.pt(ARC_PT), track_ink);

        // Value arc from sweep start to the value's angle.
        let sweep = SWEEP_DEG * self.frac();
        if sweep.abs() > 0.01 {
            let value_path = arc_path(center, arc_r, START_DEG, sweep);
            cx.list
                .push_stroke_path(value_path, cx.pt(ARC_PT), value_ink);
        }

        // Knob face.
        let face_r = radius - arc_w - cx.pt(4.0) as f64 * 2.0;
        let face = kurbo::Rect::new(
            center.x - face_r,
            center.y - face_r,
            center.x + face_r,
            center.y + face_r,
        );
        cx.list.push_fill_shape(
            face,
            &martensite_core::shape::Shape::ELLIPSE,
            cx.color(TokenKey::SurfaceColor, FACE),
        );
        cx.list.push_stroke_shape(
            face,
            &martensite_core::shape::Shape::ELLIPSE,
            cx.pt(1.0),
            cx.color(TokenKey::BorderColor, BORDER),
        );

        // Needle pointing at the value angle.
        let angle = (START_DEG + sweep).to_radians();
        let tip = kurbo::Point::new(
            center.x + angle.cos() * face_r * f64::from(NEEDLE_FRAC),
            center.y + angle.sin() * face_r * f64::from(NEEDLE_FRAC),
        );
        let mut needle = kurbo::BezPath::new();
        needle.move_to(center);
        needle.line_to(tip);
        cx.list
            .push_stroke_path(needle, cx.pt(2.0), cx.color(TokenKey::TextColor, NEEDLE));
    }
}

impl std::fmt::Debug for Dial {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Dial")
            .field("value", &self.value)
            .field("min", &self.min)
            .field("max", &self.max)
            .finish()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use martensite_core::HotNode;

    fn make_cx(hot: &mut HotNode) -> LayoutContext<'_> {
        LayoutContext { hot, scale: 1.0 }
    }

    fn ev<'a>(event: &'a WidgetEvent) -> EventContext<'a> {
        EventContext {
            event,
            bounds: Rect::new(0.0, 0.0, 56.0, 56.0),
            scale: 1.0,
        }
    }

    fn key(name: &str) -> WidgetEvent {
        WidgetEvent::KeyPressed {
            key: name.into(),
            repeat: false,
        }
    }

    #[test]
    fn builder_clamps() {
        assert_eq!(Dial::new().range(0.0, 10.0).value(99.0).get_value(), 10.0);
        assert_eq!(Dial::new().value(-1.0).get_value(), 0.0);
    }

    #[test]
    fn arrows_step() {
        let mut d = Dial::new().range(0.0, 100.0).step(5.0).value(50.0);
        d.event(&mut ev(&key("ArrowUp")));
        assert_eq!(d.get_value(), 55.0);
        d.event(&mut ev(&key("ArrowDown")));
        d.event(&mut ev(&key("ArrowDown")));
        assert_eq!(d.get_value(), 45.0);
        assert_eq!(d.take_changed(), Some(45.0));
    }

    #[test]
    fn home_end_bound() {
        let mut d = Dial::new().range(10.0, 20.0);
        d.event(&mut ev(&key("End")));
        assert_eq!(d.get_value(), 20.0);
        d.event(&mut ev(&key("Home")));
        assert_eq!(d.get_value(), 10.0);
    }

    #[test]
    fn drag_changes_value() {
        let mut d = Dial::new().range(0.0, 100.0).value(50.0);
        let mut hot = HotNode::default();
        d.layout(&mut make_cx(&mut hot), Rect::new(0.0, 0.0, 56.0, 56.0));
        let press = WidgetEvent::PointerPressed {
            position: Vec2::new(28.0, 28.0),
            button: PointerButton::Primary,
            count: 1,
        };
        assert_eq!(d.event(&mut ev(&press)), EventResponse::CapturePointer);
        let mv = WidgetEvent::PointerMoved {
            position: Vec2::new(28.0, 28.0 - 15.0), // up 15px of 150px span → +10
        };
        d.event(&mut ev(&mv));
        assert!((d.get_value() - 60.0).abs() < 0.001);
    }

    #[test]
    fn scroll_steps() {
        let mut d = Dial::new().range(0.0, 10.0).step(1.0).value(5.0);
        let sc = WidgetEvent::Scroll {
            position: Vec2::new(10.0, 10.0),
            delta: Vec2::new(0.0, 1.0),
        };
        d.event(&mut ev(&sc));
        assert_eq!(d.get_value(), 6.0);
    }

    #[test]
    fn disabled_inert() {
        let mut d = Dial::new().enabled(false);
        assert_eq!(d.event(&mut ev(&key("ArrowUp"))), EventResponse::Ignored);
    }
}
