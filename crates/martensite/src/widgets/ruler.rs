//! `Ruler` — a measurement scale strip (design-tool / editor
//! ruler idiom).
//!
//! Major and minor ticks span a value range along the horizontal
//! (or vertical) axis, with a marker line at `position` tracking
//! e.g. the cursor. Clicking or dragging parks the picked value
//! in [`Ruler::take_picked`].
//!
//! # Examples
//!
//! ```
//! use martensite::widgets::ruler::Ruler;
//!
//! let r = Ruler::new().range(0.0, 300.0).position(120.0);
//! assert_eq!(r.position_value(), 120.0);
//! ```

use accesskit::Node as AccessKitNode;
use glam::Vec2;
use martensite_core::{
    EventContext, EventResponse, LayoutConstraints, LayoutContext, PaintContext, PointerButton,
    Rect, RenderMinimum, UnderflowPolicy, Widget, WidgetEvent,
};
use martensite_theme::TokenKey;

const THICK_PT: f32 = 18.0;
const MAJOR_LEN: f32 = 0.55;
const MINOR_LEN: f32 = 0.3;

const EDGE: [u8; 4] = [70, 70, 76, 255];
const TICK: [u8; 4] = [150, 150, 158, 255];
const MARKER: [u8; 4] = [220, 90, 80, 255];

/// Ruler orientation.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RulerOrientation {
    /// Ticks rise from the bottom edge of a horizontal strip.
    Horizontal,
    /// Ticks extend from the left edge of a vertical strip.
    Vertical,
}

/// A measurement-scale strip — see the module docs.
///
/// ```
/// use martensite::widgets::ruler::Ruler;
///
/// assert_eq!(Ruler::new().position_value(), 0.0);
/// ```
#[derive(Debug)]
pub struct Ruler {
    /// Accessibility label.
    pub label: String,
    min: f32,
    max: f32,
    position: f32,
    orientation: RulerOrientation,
    major_step: f32,
    minor_step: f32,
    dragging: bool,
    pending: Option<f32>,
    bounds: Rect,
    scale: f32,
}

impl Default for Ruler {
    fn default() -> Self {
        Self::new()
    }
}

impl Ruler {
    /// Creates a `0..=100` horizontal ruler.
    ///
    /// ```
    /// use martensite::widgets::ruler::Ruler;
    ///
    /// assert_eq!(Ruler::new().bounds_range(), (0.0, 100.0));
    /// ```
    pub fn new() -> Self {
        Self {
            label: "Ruler".to_string(),
            min: 0.0,
            max: 100.0,
            position: 0.0,
            orientation: RulerOrientation::Horizontal,
            major_step: 10.0,
            minor_step: 2.0,
            dragging: false,
            pending: None,
            bounds: Rect::new(0.0, 0.0, 0.0, 0.0),
            scale: 1.0,
        }
    }

    /// Value range mapped across the strip.
    ///
    /// ```
    /// use martensite::widgets::ruler::Ruler;
    ///
    /// assert_eq!(Ruler::new().range(-50.0, 50.0).bounds_range(), (-50.0, 50.0));
    /// ```
    pub fn range(mut self, min: f32, max: f32) -> Self {
        self.min = min.min(max);
        self.max = max.max(min);
        self.position = self.position.clamp(self.min, self.max);
        self
    }

    /// Marker position (clamped).
    ///
    /// ```
    /// use martensite::widgets::ruler::Ruler;
    ///
    /// assert_eq!(Ruler::new().position(40.0).position_value(), 40.0);
    /// ```
    pub fn position(mut self, position: f32) -> Self {
        self.position = position.clamp(self.min, self.max);
        self
    }

    /// Vertical orientation.
    ///
    /// ```
    /// use martensite::widgets::ruler::{Ruler, RulerOrientation};
    ///
    /// let r = Ruler::new().vertical();
    /// assert_eq!(r.orientation(), RulerOrientation::Vertical);
    /// ```
    pub fn vertical(mut self) -> Self {
        self.orientation = RulerOrientation::Vertical;
        self
    }

    /// Major/minor tick spacing in value units.
    ///
    /// ```
    /// use martensite::widgets::ruler::Ruler;
    ///
    /// let r = Ruler::new().ticks(25.0, 5.0);
    /// assert_eq!(r.major_step_value(), 25.0);
    /// ```
    pub fn ticks(mut self, major: f32, minor: f32) -> Self {
        self.major_step = major.max(0.0001);
        self.minor_step = minor.max(0.0001);
        self
    }

    /// Accessibility label.
    ///
    /// ```
    /// use martensite::widgets::ruler::Ruler;
    ///
    /// assert_eq!(Ruler::new().label("X axis").label, "X axis");
    /// ```
    pub fn label(mut self, label: impl Into<String>) -> Self {
        self.label = label.into();
        self
    }

    /// `(min, max)` range.
    ///
    /// ```
    /// use martensite::widgets::ruler::Ruler;
    ///
    /// assert_eq!(Ruler::new().bounds_range(), (0.0, 100.0));
    /// ```
    pub fn bounds_range(&self) -> (f32, f32) {
        (self.min, self.max)
    }

    /// Marker position.
    ///
    /// ```
    /// use martensite::widgets::ruler::Ruler;
    ///
    /// assert_eq!(Ruler::new().position(30.0).position_value(), 30.0);
    /// ```
    pub fn position_value(&self) -> f32 {
        self.position
    }

    /// Orientation.
    ///
    /// ```
    /// use martensite::widgets::ruler::{Ruler, RulerOrientation};
    ///
    /// assert_eq!(Ruler::new().orientation(), RulerOrientation::Horizontal);
    /// ```
    pub fn orientation(&self) -> RulerOrientation {
        self.orientation
    }

    /// Major tick spacing.
    ///
    /// ```
    /// use martensite::widgets::ruler::Ruler;
    ///
    /// assert_eq!(Ruler::new().major_step_value(), 10.0);
    /// ```
    pub fn major_step_value(&self) -> f32 {
        self.major_step
    }

    /// Sets the marker without parking the seam (programmatic).
    ///
    /// ```
    /// use martensite::widgets::ruler::Ruler;
    ///
    /// let mut r = Ruler::new();
    /// r.set_position(55.0);
    /// assert_eq!(r.position_value(), 55.0);
    /// ```
    pub fn set_position(&mut self, position: f32) {
        self.position = position.clamp(self.min, self.max);
    }

    /// Drains the last picked value.
    ///
    /// ```
    /// use martensite::widgets::ruler::Ruler;
    ///
    /// let mut r = Ruler::new();
    /// assert!(r.take_picked().is_none());
    /// ```
    pub fn take_picked(&mut self) -> Option<f32> {
        self.pending.take()
    }

    /// Screen coordinate of a value along the strip axis.
    fn coord(&self, v: f32) -> f32 {
        let f = (v - self.min) / (self.max - self.min).max(0.0001);
        match self.orientation {
            RulerOrientation::Horizontal => self.bounds.min_x() + f * self.bounds.width(),
            RulerOrientation::Vertical => self.bounds.min_y() + f * self.bounds.height(),
        }
    }

    /// Value at a pointer position.
    fn value_at(&self, p: Vec2) -> f32 {
        let f = match self.orientation {
            RulerOrientation::Horizontal => {
                (p.x - self.bounds.min_x()) / self.bounds.width().max(1.0)
            }
            RulerOrientation::Vertical => {
                (p.y - self.bounds.min_y()) / self.bounds.height().max(1.0)
            }
        };
        (self.min + f.clamp(0.0, 1.0) * (self.max - self.min)).clamp(self.min, self.max)
    }
}

impl Widget for Ruler {
    fn measure(&mut self, cx: &mut LayoutContext, constraints: LayoutConstraints) -> Vec2 {
        let t = cx.pt(THICK_PT);
        match self.orientation {
            RulerOrientation::Horizontal => Vec2::new(
                cx.pt(200.0).min(constraints.max_size.x.max(0.0)),
                t.min(constraints.max_size.y.max(0.0)),
            ),
            RulerOrientation::Vertical => Vec2::new(
                t.min(constraints.max_size.x.max(0.0)),
                cx.pt(200.0).min(constraints.max_size.y.max(0.0)),
            ),
        }
    }

    fn min_render(&self) -> RenderMinimum {
        RenderMinimum::new(Vec2::new(12.0, 12.0)).with_policy(UnderflowPolicy::Lint)
    }

    fn layout(&mut self, cx: &mut LayoutContext, bounds: Rect) {
        self.bounds = bounds;
        self.scale = cx.scale;
    }

    fn accessibility(&self, node: &mut AccessKitNode) {
        node.set_role(accesskit::Role::Slider);
        node.set_label(format!(
            "{} — {:.0} of {:.0}..{:.0}",
            self.label, self.position, self.min, self.max
        ));
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
                    let v = self.value_at(*position);
                    self.position = v;
                    self.pending = Some(v);
                    EventResponse::CapturePointer
                } else {
                    EventResponse::Ignored
                }
            }
            WidgetEvent::PointerMoved { position } => {
                if !self.dragging {
                    return EventResponse::Ignored;
                }
                let v = self.value_at(*position);
                self.position = v;
                self.pending = Some(v);
                EventResponse::RequestRepaint
            }
            WidgetEvent::PointerReleased {
                button: PointerButton::Primary,
                ..
            } => {
                if self.dragging {
                    self.dragging = false;
                    return EventResponse::Handled;
                }
                EventResponse::Ignored
            }
            _ => EventResponse::Ignored,
        }
    }

    fn paint(&self, cx: &mut PaintContext) {
        let pt = |p: Vec2| (f64::from(p.x), f64::from(p.y));
        let horiz = self.orientation == RulerOrientation::Horizontal;
        let thick = if horiz {
            self.bounds.height()
        } else {
            self.bounds.width()
        };
        let edge = cx.color(TokenKey::TextColor, TICK);

        // Tick marks from the leading edge.
        let span_px = if horiz {
            self.bounds.width()
        } else {
            self.bounds.height()
        };
        let minor_px = span_px * (self.minor_step / (self.max - self.min).max(0.0001));
        let major_px = span_px * (self.major_step / (self.max - self.min).max(0.0001));
        let origin = if horiz {
            self.bounds.min_x()
        } else {
            self.bounds.min_y()
        };

        let draw_tick = |cx: &mut PaintContext, off: f32, len: f32| {
            let mut p = kurbo::BezPath::new();
            if horiz {
                p.move_to(pt(Vec2::new(origin + off, self.bounds.max_y())));
                p.line_to(pt(Vec2::new(origin + off, self.bounds.max_y() - len)));
            } else {
                p.move_to(pt(Vec2::new(self.bounds.min_x(), origin + off)));
                p.line_to(pt(Vec2::new(self.bounds.min_x() + len, origin + off)));
            }
            cx.list.push_stroke_path(p, cx.pt(0.75), edge);
        };

        if major_px >= 4.0 {
            let mut v = (self.min / self.major_step).ceil() * self.major_step;
            while v <= self.max {
                draw_tick(cx, self.coord(v) - origin, thick * MAJOR_LEN);
                v += self.major_step;
            }
        }
        if minor_px >= 4.0 {
            let mut v = (self.min / self.minor_step).ceil() * self.minor_step;
            while v <= self.max {
                // Skip positions that already carry a major tick.
                let r = (v / self.major_step).fract().abs();
                if r > 0.01 && (r - 1.0).abs() > 0.01 {
                    draw_tick(cx, self.coord(v) - origin, thick * MINOR_LEN);
                }
                v += self.minor_step;
            }
        }
        // Baseline + marker.
        let mut base = kurbo::BezPath::new();
        if horiz {
            base.move_to(pt(Vec2::new(self.bounds.min_x(), self.bounds.max_y())));
            base.line_to(pt(Vec2::new(self.bounds.max_x(), self.bounds.max_y())));
        } else {
            base.move_to(pt(Vec2::new(self.bounds.min_x(), self.bounds.min_y())));
            base.line_to(pt(Vec2::new(self.bounds.min_x(), self.bounds.max_y())));
        }
        cx.list
            .push_stroke_path(base, cx.pt(0.75), cx.color(TokenKey::BorderColor, EDGE));

        let m = self.coord(self.position);
        let mut mark = kurbo::BezPath::new();
        if horiz {
            mark.move_to(pt(Vec2::new(m, self.bounds.min_y())));
            mark.line_to(pt(Vec2::new(m, self.bounds.max_y())));
        } else {
            mark.move_to(pt(Vec2::new(self.bounds.min_x(), m)));
            mark.line_to(pt(Vec2::new(self.bounds.max_x(), m)));
        }
        cx.list
            .push_stroke_path(mark, cx.pt(1.0), cx.color(TokenKey::ErrorColor, MARKER));
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use martensite_core::HotNode;

    fn laid_out(r: &mut Ruler, w: f32, h: f32) {
        let mut hot = HotNode::default();
        let mut cx = LayoutContext {
            hot: &mut hot,
            scale: 1.0,
        };
        r.measure(
            &mut cx,
            LayoutConstraints {
                min_size: Vec2::ZERO,
                max_size: Vec2::new(w, h),
            },
        );
        r.layout(&mut cx, Rect::new(0.0, 0.0, w, h));
    }

    fn ev(r: &mut Ruler, e: WidgetEvent) {
        r.event(&mut EventContext {
            event: &e,
            bounds: Rect::new(0.0, 0.0, 200.0, 18.0),
            scale: 1.0,
        });
    }

    #[test]
    fn clamps_position() {
        assert_eq!(Ruler::new().position(500.0).position_value(), 100.0);
    }

    #[test]
    fn click_picks_value() {
        let mut r = Ruler::new();
        laid_out(&mut r, 200.0, 18.0);
        ev(
            &mut r,
            WidgetEvent::PointerPressed {
                button: PointerButton::Primary,
                position: Vec2::new(100.0, 9.0), // middle → 50
                count: 1,
            },
        );
        let v = r.take_picked().unwrap();
        assert!((v - 50.0).abs() < 1.0);
    }

    #[test]
    fn drag_updates() {
        let mut r = Ruler::new();
        laid_out(&mut r, 200.0, 18.0);
        ev(
            &mut r,
            WidgetEvent::PointerPressed {
                button: PointerButton::Primary,
                position: Vec2::new(20.0, 9.0),
                count: 1,
            },
        );
        r.take_picked();
        ev(
            &mut r,
            WidgetEvent::PointerMoved {
                position: Vec2::new(180.0, 9.0),
            },
        );
        let v = r.take_picked().unwrap();
        assert!((v - 90.0).abs() < 1.0);
    }

    #[test]
    fn vertical_orientation() {
        let r = Ruler::new().vertical();
        assert_eq!(r.orientation(), RulerOrientation::Vertical);
    }

    #[test]
    fn outside_ignored() {
        let mut r = Ruler::new();
        laid_out(&mut r, 200.0, 18.0);
        ev(
            &mut r,
            WidgetEvent::PointerPressed {
                button: PointerButton::Primary,
                position: Vec2::new(400.0, 9.0),
                count: 1,
            },
        );
        assert!(r.take_picked().is_none());
    }
}
