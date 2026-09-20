//! `CurveEditor` — a cubic-bezier easing editor (drag the two
//! control handles of a `cubic-bezier(x1, y1, x2, y2)` curve —
//! the DevTools / design-tool easing idiom).
//!
//! Endpoints are pinned at (0, 0) and (1, 1); the two control
//! handles drag within the pad (x clamped to `0..=1`, y allowed
//! to overshoot for springy easings). Dragging parks
//! [`CurveEditor::take_changed`]; arrow keys nudge the last
//! touched handle.
//!
//! # Examples
//!
//! ```
//! use martensite::widgets::curve_editor::CurveEditor;
//!
//! let c = CurveEditor::new().handles((0.25, 0.1), (0.25, 1.0));
//! assert_eq!(c.handle_a(), (0.25, 0.1));
//! ```

use accesskit::Node as AccessKitNode;
use glam::Vec2;
use martensite_core::{
    EventContext, EventResponse, LayoutConstraints, LayoutContext, PaintContext, PointerButton,
    Rect, RenderMinimum, UnderflowPolicy, Widget, WidgetEvent,
};
use martensite_theme::TokenKey;

const SIZE_PT: f32 = 160.0;
const HANDLE_PT: f32 = 5.0;
/// Vertical overshoot headroom beyond 0..1 (fraction of pad).
const OVERSHOOT: f32 = 0.2;

const FACE: [u8; 4] = [36, 36, 42, 255];
const EDGE: [u8; 4] = [70, 70, 76, 255];
const GRID: [u8; 4] = [60, 60, 68, 255];
const CURVE: [u8; 4] = [110, 170, 230, 255];
const ARM: [u8; 4] = [140, 140, 150, 255];
const HANDLE: [u8; 4] = [230, 150, 90, 255];
const ACTIVE: [u8; 4] = [240, 180, 100, 255];

/// A cubic-bezier easing editor — see the module docs.
///
/// ```
/// use martensite::widgets::curve_editor::CurveEditor;
///
/// assert_eq!(CurveEditor::new().handle_a(), (0.25, 0.1));
/// ```
#[derive(Debug)]
pub struct CurveEditor {
    /// Accessibility label.
    pub label: String,
    /// First control point `(x1, y1)` in normalized space.
    p1: Vec2,
    /// Second control point `(x2, y2)` in normalized space.
    p2: Vec2,
    /// Which handle is being dragged (0 or 1).
    dragging: Option<usize>,
    /// Last-touched handle for keyboard nudges.
    active: usize,
    changed: bool,
    bounds: Rect,
    scale: f32,
}

impl Default for CurveEditor {
    fn default() -> Self {
        Self::new()
    }
}

impl CurveEditor {
    /// Creates the `ease` default `(0.25, 0.1)` / `(0.25, 1.0)`.
    ///
    /// ```
    /// use martensite::widgets::curve_editor::CurveEditor;
    ///
    /// assert_eq!(CurveEditor::new().handle_b(), (0.25, 1.0));
    /// ```
    pub fn new() -> Self {
        Self {
            label: "Easing curve".to_string(),
            p1: Vec2::new(0.25, 0.1),
            p2: Vec2::new(0.25, 1.0),
            dragging: None,
            active: 0,
            changed: false,
            bounds: Rect::new(0.0, 0.0, 0.0, 0.0),
            scale: 1.0,
        }
    }

    /// Sets both control handles.
    ///
    /// ```
    /// use martensite::widgets::curve_editor::CurveEditor;
    ///
    /// let c = CurveEditor::new().handles((0.4, 0.0), (0.6, 1.0));
    /// assert_eq!(c.handle_a(), (0.4, 0.0));
    /// ```
    pub fn handles(mut self, a: (f32, f32), b: (f32, f32)) -> Self {
        self.p1 = Self::clamp_point(Vec2::new(a.0, a.1));
        self.p2 = Self::clamp_point(Vec2::new(b.0, b.1));
        self
    }

    /// Accessibility label.
    ///
    /// ```
    /// use martensite::widgets::curve_editor::CurveEditor;
    ///
    /// assert_eq!(CurveEditor::new().label("Ease").label, "Ease");
    /// ```
    pub fn label(mut self, label: impl Into<String>) -> Self {
        self.label = label.into();
        self
    }

    /// First control handle `(x1, y1)`.
    ///
    /// ```
    /// use martensite::widgets::curve_editor::CurveEditor;
    ///
    /// assert_eq!(CurveEditor::new().handles((0.5, 0.2), (0.5, 0.8)).handle_a(), (0.5, 0.2));
    /// ```
    pub fn handle_a(&self) -> (f32, f32) {
        (self.p1.x, self.p1.y)
    }

    /// Second control handle `(x2, y2)`.
    ///
    /// ```
    /// use martensite::widgets::curve_editor::CurveEditor;
    ///
    /// assert_eq!(CurveEditor::new().handles((0.5, 0.2), (0.5, 0.8)).handle_b(), (0.5, 0.8));
    /// ```
    pub fn handle_b(&self) -> (f32, f32) {
        (self.p2.x, self.p2.y)
    }

    /// `cubic-bezier(x1, y1, x2, y2)` as a tuple.
    ///
    /// ```
    /// use martensite::widgets::curve_editor::CurveEditor;
    ///
    /// assert_eq!(
    ///     CurveEditor::new().handles((0.1, 0.2), (0.3, 0.4)).bezier(),
    ///     (0.1, 0.2, 0.3, 0.4)
    /// );
    /// ```
    pub fn bezier(&self) -> (f32, f32, f32, f32) {
        (self.p1.x, self.p1.y, self.p2.x, self.p2.y)
    }

    /// CSS `cubic-bezier(...)` text.
    ///
    /// ```
    /// use martensite::widgets::curve_editor::CurveEditor;
    ///
    /// assert_eq!(
    ///     CurveEditor::new().handles((0.5, 0.0), (0.5, 1.0)).css(),
    ///     "cubic-bezier(0.5, 0, 0.5, 1)"
    /// );
    /// ```
    pub fn css(&self) -> String {
        let fmt = |v: f32| {
            let s = format!("{:.2}", v);
            s.trim_end_matches('0').trim_end_matches('.').to_string()
        };
        format!(
            "cubic-bezier({}, {}, {}, {})",
            fmt(self.p1.x),
            fmt(self.p1.y),
            fmt(self.p2.x),
            fmt(self.p2.y)
        )
    }

    /// Curve value `y` at parameter `x` (Newton-solved cubic).
    ///
    /// ```
    /// use martensite::widgets::curve_editor::CurveEditor;
    ///
    /// let c = CurveEditor::new().handles((0.0, 0.0), (1.0, 1.0));
    /// assert!((c.sample(0.5) - 0.5).abs() < 0.01); // linear ease
    /// ```
    pub fn sample(&self, x: f32) -> f32 {
        // Solve bezier x(t) = x for t, then evaluate y(t).
        let mut t = x.clamp(0.0, 1.0);
        for _ in 0..8 {
            let bx = cubic(t, 0.0, self.p1.x, self.p2.x, 1.0) - x;
            let dx = cubic_d(t, 0.0, self.p1.x, self.p2.x, 1.0);
            if dx.abs() < 1e-6 {
                break;
            }
            t = (t - bx / dx).clamp(0.0, 1.0);
        }
        cubic(t, 0.0, self.p1.y, self.p2.y, 1.0)
    }

    /// Drains whether the handles moved since the last call.
    ///
    /// ```
    /// use martensite::widgets::curve_editor::CurveEditor;
    ///
    /// assert!(!CurveEditor::new().take_changed());
    /// ```
    pub fn take_changed(&mut self) -> bool {
        std::mem::take(&mut self.changed)
    }

    /// Clamp to x `0..=1`, y `-OVERSHOOT..=1+OVERSHOOT`.
    fn clamp_point(p: Vec2) -> Vec2 {
        Vec2::new(p.x.clamp(0.0, 1.0), p.y.clamp(-OVERSHOOT, 1.0 + OVERSHOOT))
    }

    /// Widget-space position of a normalized point (y flipped).
    fn to_px(&self, p: Vec2) -> Vec2 {
        let pad = 8.0 * self.scale;
        let w = self.bounds.width() - 2.0 * pad;
        let h = self.bounds.height() - 2.0 * pad;
        Vec2::new(
            self.bounds.min_x() + pad + p.x * w,
            self.bounds.min_y() + pad + (1.0 - p.y) * h,
        )
    }

    /// Normalized position of a widget-space point.
    fn to_norm(&self, v: Vec2) -> Vec2 {
        let pad = 8.0 * self.scale;
        let w = self.bounds.width() - 2.0 * pad;
        let h = self.bounds.height() - 2.0 * pad;
        Vec2::new(
            (v.x - self.bounds.min_x() - pad) / w.max(1.0),
            1.0 - (v.y - self.bounds.min_y() - pad) / h.max(1.0),
        )
    }

    /// Handle index under a point, if within grab radius.
    fn handle_at(&self, p: Vec2) -> Option<usize> {
        let grab = 10.0 * self.scale;
        let (a, b) = (self.to_px(self.p1), self.to_px(self.p2));
        if p.distance(a) <= grab {
            Some(0)
        } else if p.distance(b) <= grab {
            Some(1)
        } else {
            None
        }
    }

    fn set_handle(&mut self, i: usize, p: Vec2) {
        let c = Self::clamp_point(p);
        if i == 0 {
            self.p1 = c;
        } else {
            self.p2 = c;
        }
        self.active = i;
        self.changed = true;
    }
}

fn cubic(t: f32, a: f32, b: f32, c: f32, d: f32) -> f32 {
    let u = 1.0 - t;
    u * u * u * a + 3.0 * u * u * t * b + 3.0 * u * t * t * c + t * t * t * d
}

fn cubic_d(t: f32, a: f32, b: f32, c: f32, d: f32) -> f32 {
    let u = 1.0 - t;
    3.0 * u * u * (b - a) + 6.0 * u * t * (c - b) + 3.0 * t * t * (d - c)
}

impl Widget for CurveEditor {
    fn measure(&mut self, cx: &mut LayoutContext, constraints: LayoutConstraints) -> Vec2 {
        let s = cx.pt(SIZE_PT);
        Vec2::new(
            s.min(constraints.max_size.x.max(0.0)),
            s.min(constraints.max_size.y.max(0.0)),
        )
    }

    fn min_render(&self) -> RenderMinimum {
        RenderMinimum::new(Vec2::new(80.0, 80.0)).with_policy(UnderflowPolicy::Lint)
    }

    fn layout(&mut self, cx: &mut LayoutContext, bounds: Rect) {
        self.bounds = bounds;
        self.scale = cx.scale;
    }

    fn accessibility(&self, node: &mut AccessKitNode) {
        node.set_role(accesskit::Role::Group);
        node.set_label(format!("{} — {}", self.label, self.css()));
    }

    fn event(&mut self, cx: &mut EventContext) -> EventResponse {
        match cx.event {
            WidgetEvent::PointerPressed {
                button: PointerButton::Primary,
                position,
                ..
            } => {
                if let Some(i) = self.handle_at(*position) {
                    self.dragging = Some(i);
                    self.active = i;
                    return EventResponse::CapturePointer;
                }
                EventResponse::Ignored
            }
            WidgetEvent::PointerMoved { position } => {
                if let Some(i) = self.dragging {
                    self.set_handle(i, self.to_norm(*position));
                    return EventResponse::RequestRepaint;
                }
                EventResponse::Ignored
            }
            WidgetEvent::PointerReleased {
                button: PointerButton::Primary,
                ..
            } => {
                if self.dragging.take().is_some() {
                    return EventResponse::RequestRepaint;
                }
                EventResponse::Ignored
            }
            WidgetEvent::KeyPressed { key, .. } => {
                let d = 0.05;
                let (dx, dy) = match key.as_str() {
                    "ArrowLeft" => (-d, 0.0),
                    "ArrowRight" => (d, 0.0),
                    "ArrowUp" => (0.0, d),
                    "ArrowDown" => (0.0, -d),
                    _ => return EventResponse::Ignored,
                };
                let cur = if self.active == 0 { self.p1 } else { self.p2 };
                let i = self.active;
                self.set_handle(i, cur + Vec2::new(dx, dy));
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
        let pt = |v: Vec2| (f64::from(v.x), f64::from(v.y));
        cx.list.push_fill_shape(
            krect(self.bounds),
            &martensite_core::shape::Shape::rounded(cx.pt(4.0)),
            cx.color(TokenKey::SurfaceColor, FACE),
        );
        let edge = cx.color(TokenKey::BorderColor, EDGE);
        // Grid quarters + the unit box.
        let pad = 8.0 * self.scale;
        let unit = Rect::new(
            self.bounds.min_x() + pad,
            self.bounds.min_y() + pad,
            self.bounds.width() - 2.0 * pad,
            self.bounds.height() - 2.0 * pad,
        );
        for q in 1..4 {
            let fx = unit.min_x() + unit.width() * q as f32 / 4.0;
            let fy = unit.min_y() + unit.height() * q as f32 / 4.0;
            let mut v = kurbo::BezPath::new();
            v.move_to((f64::from(fx), f64::from(unit.min_y())));
            v.line_to((f64::from(fx), f64::from(unit.max_y())));
            cx.list.push_stroke_path(v, cx.pt(0.5), GRID);
            let mut h = kurbo::BezPath::new();
            h.move_to((f64::from(unit.min_x()), f64::from(fy)));
            h.line_to((f64::from(unit.max_x()), f64::from(fy)));
            cx.list.push_stroke_path(h, cx.pt(0.5), GRID);
        }
        cx.list.push_stroke_shape(
            krect(unit),
            &martensite_core::shape::Shape::RECT,
            cx.pt(0.75),
            edge,
        );
        // Control arms: endpoint → handle.
        let arm = cx.color(TokenKey::TextMutedColor, ARM);
        for (end, h) in [
            (Vec2::new(0.0, 0.0), self.p1),
            (Vec2::new(1.0, 1.0), self.p2),
        ] {
            let mut p = kurbo::BezPath::new();
            p.move_to(pt(self.to_px(end)));
            p.line_to(pt(self.to_px(h)));
            cx.list.push_stroke_path(p, cx.pt(0.75), arm);
        }
        // The bezier itself.
        let mut curve = kurbo::BezPath::new();
        let steps = 40;
        for k in 0..=steps {
            let x = k as f32 / steps as f32;
            let p = pt(self.to_px(Vec2::new(x, self.sample(x))));
            if k == 0 {
                curve.move_to(p);
            } else {
                curve.line_to(p);
            }
        }
        cx.list
            .push_stroke_path(curve, cx.pt(1.5), cx.color(TokenKey::AccentColor, CURVE));
        // Handles.
        let hr = HANDLE_PT * self.scale;
        for (i, hp) in [self.p1, self.p2].iter().enumerate() {
            let c = self.to_px(*hp);
            cx.list.push_fill_shape(
                kurbo::Rect::new(
                    f64::from(c.x - hr),
                    f64::from(c.y - hr),
                    f64::from(c.x + hr),
                    f64::from(c.y + hr),
                ),
                &martensite_core::shape::Shape::circle(c, hr),
                if self.dragging == Some(i) || self.active == i {
                    ACTIVE
                } else {
                    HANDLE
                },
            );
        }
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

    fn laid_out(c: &mut CurveEditor, w: f32, h: f32) {
        let mut hot = HotNode::default();
        let mut cx = LayoutContext {
            hot: &mut hot,
            scale: 1.0,
        };
        c.measure(
            &mut cx,
            LayoutConstraints {
                min_size: Vec2::ZERO,
                max_size: Vec2::new(w, h),
            },
        );
        c.layout(&mut cx, Rect::new(0.0, 0.0, w, h));
    }

    #[test]
    fn defaults_are_css_ease() {
        let c = CurveEditor::new();
        assert_eq!(c.bezier(), (0.25, 0.1, 0.25, 1.0));
        assert_eq!(c.css(), "cubic-bezier(0.25, 0.1, 0.25, 1)");
    }

    #[test]
    fn handles_clamped() {
        let c = CurveEditor::new().handles((-1.0, 2.0), (3.0, -5.0));
        assert_eq!(c.handle_a(), (0.0, 1.2));
        assert_eq!(c.handle_b(), (1.0, -0.2));
    }

    #[test]
    fn drag_moves_handle() {
        let mut c = CurveEditor::new().handles((0.5, 0.5), (0.5, 0.5));
        laid_out(&mut c, 160.0, 160.0);
        let hp = c.to_px(Vec2::new(0.5, 0.5));
        c.event(&mut EventContext {
            event: &WidgetEvent::PointerPressed {
                button: PointerButton::Primary,
                position: hp,
                count: 1,
            },
            bounds: c.bounds,
            scale: 1.0,
        });
        c.event(&mut EventContext {
            event: &WidgetEvent::PointerMoved {
                position: c.to_px(Vec2::new(0.8, 0.2)),
            },
            bounds: c.bounds,
            scale: 1.0,
        });
        assert!(c.take_changed());
        let (x, y) = c.handle_a();
        assert!((x - 0.8).abs() < 0.05 && (y - 0.2).abs() < 0.05);
    }

    #[test]
    fn arrow_keys_nudge() {
        let mut c = CurveEditor::new().handles((0.5, 0.5), (0.5, 0.5));
        laid_out(&mut c, 160.0, 160.0);
        c.event(&mut EventContext {
            event: &WidgetEvent::KeyPressed {
                key: "ArrowRight".to_string(),
                repeat: false,
            },
            bounds: c.bounds,
            scale: 1.0,
        });
        assert!((c.handle_a().0 - 0.55).abs() < 1e-5);
        assert!(c.take_changed());
    }

    #[test]
    fn sample_linear_and_ease_in() {
        let c = CurveEditor::new().handles((0.0, 0.0), (1.0, 1.0));
        assert!((c.sample(0.5) - 0.5).abs() < 0.01);
        let c = CurveEditor::new().handles((0.42, 0.0), (1.0, 1.0)); // ease-in
        assert!(c.sample(0.5) < 0.4);
    }
}
