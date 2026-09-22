//! `XYPad` — a two-dimensional drag controller (Kaoss-pad / Ableton
//! XY pad / `NSControl` XY idiom).
//!
//! Dragging inside the square sets a normalized `(x, y)` value —
//! `x` left→right, `y` bottom→top — parked in
//! [`XYPad::take_changed`]. `Home`/`End` snap to the extremes and
//! arrow keys nudge. Axis labels paint at the edges when a painter
//! is present.
//!
//! # Examples
//!
//! ```
//! use martensite::widgets::xy_pad::XYPad;
//!
//! let pad = XYPad::new().value(0.5, 0.25);
//! assert_eq!(pad.value_xy(), (0.5, 0.25));
//! ```

use accesskit::Node as AccessKitNode;
use glam::Vec2;
use martensite_core::{
    EventContext, EventResponse, LayoutConstraints, LayoutContext, PaintContext, PointerButton,
    Rect, RenderMinimum, UnderflowPolicy, Widget, WidgetEvent,
};
use martensite_theme::TokenKey;

use crate::text_paint::SharedTextPainter;

const SIZE_PT: f32 = 160.0;
const THUMB_PT: f32 = 14.0;
const STEP: f32 = 0.05;

const SURFACE: [u8; 4] = [42, 42, 46, 255];
const GRID: [u8; 4] = [70, 70, 76, 255];
const ACCENT: [u8; 4] = [80, 140, 220, 255];
const FG: [u8; 4] = [230, 230, 235, 255];
const MUTED: [u8; 4] = [140, 140, 148, 255];

/// A two-axis drag pad — see the module docs.
///
/// ```
/// use martensite::widgets::xy_pad::XYPad;
///
/// let pad = XYPad::new();
/// assert_eq!(pad.value_xy(), (0.0, 0.0));
/// ```
pub struct XYPad {
    /// When `false` the pad is inert.
    pub enabled: bool,
    /// X axis name (a11y + edge label).
    pub x_label: String,
    /// Y axis name (a11y + edge label).
    pub y_label: String,
    x: f32,
    y: f32,
    changed: bool,
    dragging: bool,
    focused: bool,
    bounds: Rect,
    pad: Rect,
    scale: f32,
    text_painter: Option<SharedTextPainter>,
}

impl Default for XYPad {
    fn default() -> Self {
        Self::new()
    }
}

impl XYPad {
    /// Creates a pad at `(0, 0)` (bottom-left).
    ///
    /// ```
    /// use martensite::widgets::xy_pad::XYPad;
    ///
    /// assert_eq!(XYPad::new().value_xy(), (0.0, 0.0));
    /// ```
    pub fn new() -> Self {
        Self {
            enabled: true,
            x_label: "X".to_string(),
            y_label: "Y".to_string(),
            x: 0.0,
            y: 0.0,
            changed: false,
            dragging: false,
            focused: false,
            bounds: Rect::new(0.0, 0.0, 0.0, 0.0),
            pad: Rect::new(0.0, 0.0, 0.0, 0.0),
            scale: 1.0,
            text_painter: None,
        }
    }

    /// Initial normalized value `(x, y)`, each `0..=1`.
    ///
    /// ```
    /// use martensite::widgets::xy_pad::XYPad;
    ///
    /// let pad = XYPad::new().value(0.8, 0.2);
    /// assert_eq!(pad.value_xy(), (0.8, 0.2));
    /// ```
    pub fn value(mut self, x: f32, y: f32) -> Self {
        self.x = x.clamp(0.0, 1.0);
        self.y = y.clamp(0.0, 1.0);
        self
    }

    /// Axis names (edge labels + a11y).
    ///
    /// ```
    /// use martensite::widgets::xy_pad::XYPad;
    ///
    /// let pad = XYPad::new().labels("Cutoff", "Resonance");
    /// assert_eq!(pad.x_label, "Cutoff");
    /// ```
    pub fn labels(mut self, x: impl Into<String>, y: impl Into<String>) -> Self {
        self.x_label = x.into();
        self.y_label = y.into();
        self
    }

    /// Enables or disables the pad.
    ///
    /// ```
    /// use martensite::widgets::xy_pad::XYPad;
    ///
    /// let pad = XYPad::new().enabled(false);
    /// assert!(!pad.enabled);
    /// ```
    pub fn enabled(mut self, enabled: bool) -> Self {
        self.enabled = enabled;
        self
    }

    /// Explicit painter override — see [`crate::text_paint`].
    ///
    /// ```
    /// use martensite::widgets::xy_pad::XYPad;
    /// use martensite::text_paint::shared_painter;
    ///
    /// let pad = XYPad::new().with_text_painter(shared_painter());
    /// assert_eq!(pad.value_xy(), (0.0, 0.0));
    /// ```
    pub fn with_text_painter(mut self, painter: SharedTextPainter) -> Self {
        self.text_painter = Some(painter);
        self
    }

    /// Normalized `(x, y)` value.
    ///
    /// ```
    /// use martensite::widgets::xy_pad::XYPad;
    ///
    /// assert_eq!(XYPad::new().value_xy(), (0.0, 0.0));
    /// ```
    pub fn value_xy(&self) -> (f32, f32) {
        (self.x, self.y)
    }

    /// Sets the value programmatically.
    ///
    /// ```
    /// use martensite::widgets::xy_pad::XYPad;
    ///
    /// let mut pad = XYPad::new();
    /// pad.set_value(1.0, 1.0);
    /// assert_eq!(pad.value_xy(), (1.0, 1.0));
    /// ```
    pub fn set_value(&mut self, x: f32, y: f32) {
        self.x = x.clamp(0.0, 1.0);
        self.y = y.clamp(0.0, 1.0);
    }

    /// Drains a pending change notification.
    ///
    /// ```
    /// use martensite::widgets::xy_pad::XYPad;
    ///
    /// let mut pad = XYPad::new();
    /// assert!(!pad.take_changed());
    /// ```
    pub fn take_changed(&mut self) -> bool {
        std::mem::take(&mut self.changed)
    }

    /// Thumb center in device px.
    fn thumb_center(&self) -> Vec2 {
        Vec2::new(
            self.pad.min_x() + self.x * self.pad.width(),
            self.pad.max_y() - self.y * self.pad.height(),
        )
    }

    /// Normalized value at a device point (clamped).
    fn value_at(&self, p: Vec2) -> (f32, f32) {
        (
            ((p.x - self.pad.min_x()) / self.pad.width().max(1.0)).clamp(0.0, 1.0),
            ((self.pad.max_y() - p.y) / self.pad.height().max(1.0)).clamp(0.0, 1.0),
        )
    }
}

impl Widget for XYPad {
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
        // Square pad region inset for the edge labels.
        let dim = bounds.width().min(bounds.height());
        let m = cx.pt(10.0);
        self.pad = Rect::new(
            bounds.min_x() + (bounds.width() - dim) / 2.0 + m,
            bounds.min_y() + (bounds.height() - dim) / 2.0,
            (dim - 2.0 * m).max(0.0),
            (dim - 2.0 * m).max(0.0),
        );
    }

    fn accessibility(&self, node: &mut AccessKitNode) {
        node.set_role(accesskit::Role::Group);
        node.set_label(format!("{} / {} pad", self.x_label, self.y_label));
        node.set_value(format!("{:.2}, {:.2}", self.x, self.y));
        if self.enabled {
            node.add_action(accesskit::Action::SetValue);
        } else {
            node.set_disabled();
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
                if self.pad.contains(*position) {
                    self.dragging = true;
                    let (x, y) = self.value_at(*position);
                    self.set_value(x, y);
                    self.changed = true;
                    return EventResponse::CapturePointer;
                }
                EventResponse::Ignored
            }
            WidgetEvent::PointerMoved { position } => {
                if self.dragging {
                    let (x, y) = self.value_at(*position);
                    if (x, y) != (self.x, self.y) {
                        self.set_value(x, y);
                        self.changed = true;
                        return EventResponse::RequestRepaint;
                    }
                    return EventResponse::Handled;
                }
                EventResponse::Ignored
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
            WidgetEvent::KeyPressed { key, .. } => {
                let (mut x, mut y) = (self.x, self.y);
                match key.as_str() {
                    "ArrowLeft" => x -= STEP,
                    "ArrowRight" => x += STEP,
                    "ArrowDown" => y -= STEP,
                    "ArrowUp" => y += STEP,
                    "Home" => {
                        x = 0.0;
                        y = 0.0;
                    }
                    "End" => {
                        x = 1.0;
                        y = 1.0;
                    }
                    _ => return EventResponse::Ignored,
                }
                self.set_value(x, y);
                self.changed = true;
                EventResponse::RequestRepaint
            }
            WidgetEvent::FocusGained => {
                self.focused = true;
                EventResponse::RequestRepaint
            }
            WidgetEvent::FocusLost => {
                self.focused = false;
                EventResponse::RequestRepaint
            }
            WidgetEvent::SemanticAction(martensite_core::widget::SemanticAction::SetValue(
                text,
            )) => {
                // "x,y" comma pair — the two-axis SetValue convention.
                let mut parts = text.split(',');
                let x = parts.next().and_then(|s| s.trim().parse::<f32>().ok());
                let y = parts.next().and_then(|s| s.trim().parse::<f32>().ok());
                if let (Some(x), Some(y)) = (x, y) {
                    self.set_value(x, y);
                    self.changed = true;
                    return EventResponse::Handled;
                }
                EventResponse::Ignored
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
        let grid = cx.color(TokenKey::DividerColor, GRID);
        let accent = if self.enabled {
            cx.color(TokenKey::AccentColor, ACCENT)
        } else {
            cx.color(TokenKey::TextMutedColor, MUTED)
        };
        let shape = martensite_core::shape::Shape::rounded(cx.pt(6.0));
        cx.list.push_fill_shape(f(self.pad), &shape, surface);
        // Quarter grid.
        for i in 1..4 {
            let fx = i as f32 / 4.0;
            let mut v = kurbo::BezPath::new();
            v.move_to((
                f64::from(self.pad.min_x() + self.pad.width() * fx),
                f64::from(self.pad.min_y()),
            ));
            v.line_to((
                f64::from(self.pad.min_x() + self.pad.width() * fx),
                f64::from(self.pad.max_y()),
            ));
            cx.list.push_stroke_path(v, cx.pt(0.5), grid);
            let mut h = kurbo::BezPath::new();
            h.move_to((
                f64::from(self.pad.min_x()),
                f64::from(self.pad.min_y() + self.pad.height() * fx),
            ));
            h.line_to((
                f64::from(self.pad.max_x()),
                f64::from(self.pad.min_y() + self.pad.height() * fx),
            ));
            cx.list.push_stroke_path(h, cx.pt(0.5), grid);
        }
        cx.list
            .push_stroke_shape(f(self.pad), &shape, cx.pt(0.75), grid);
        if self.focused {
            let inset = Rect::new(
                self.pad.min_x() - 2.0,
                self.pad.min_y() - 2.0,
                self.pad.width() + 4.0,
                self.pad.height() + 4.0,
            );
            let ring = martensite_core::shape::Shape::rounded(cx.pt(8.0));
            cx.list
                .push_stroke_shape(f(inset), &ring, cx.pt(1.5), accent);
        }

        // Crosshair through the thumb.
        let c = self.thumb_center();
        let mut cross = kurbo::BezPath::new();
        cross.move_to((f64::from(self.pad.min_x()), f64::from(c.y)));
        cross.line_to((f64::from(self.pad.max_x()), f64::from(c.y)));
        cross.move_to((f64::from(c.x), f64::from(self.pad.min_y())));
        cross.line_to((f64::from(c.x), f64::from(self.pad.max_y())));
        cx.list.push_stroke_path(cross, cx.pt(0.5), accent);

        // Thumb.
        let r = cx.pt(THUMB_PT) / 2.0;
        cx.list.push_fill_shape(
            kurbo::Rect::new(
                f64::from(c.x - r),
                f64::from(c.y - r),
                f64::from(c.x + r),
                f64::from(c.y + r),
            ),
            &martensite_core::shape::Shape::circle(c, r),
            accent,
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
            cx.color(TokenKey::TextColor, FG),
        );

        // Edge labels: x under the pad, y rotated at the left — text
        // rotation isn't in the paint API, so y sits top-left.
        let painter = crate::text_paint::resolve_painter(&self.text_painter, cx.text_painter);
        let size = 12.0 * cx.scale;
        let muted = cx.color(TokenKey::TextMutedColor, MUTED);
        let xw = painter
            .and_then(|p| p.measure_text(&self.x_label, size))
            .unwrap_or(self.x_label.chars().count() as f32 * size * 0.55);
        crate::text_paint::paint_label_clipped(
            painter,
            cx.list,
            f(Rect::new(
                self.bounds.min_x(),
                self.pad.max_y() + 1.0,
                self.bounds.width(),
                self.bounds.height() - (self.pad.max_y() - self.bounds.min_y()),
            )),
            kurbo::Point::new(
                f64::from(self.pad.min_x() + (self.pad.width() - xw) / 2.0),
                f64::from(self.pad.max_y() + 1.0),
            ),
            &self.x_label,
            size,
            muted,
        );
        crate::text_paint::paint_label_clipped(
            painter,
            cx.list,
            f(Rect::new(
                self.bounds.min_x(),
                self.bounds.min_y(),
                self.pad.min_x() - self.bounds.min_x() + 4.0,
                self.pad.min_y() - self.bounds.min_y(),
            )),
            kurbo::Point::new(
                f64::from(self.bounds.min_x() + 1.0),
                f64::from(self.bounds.min_y() + 1.0),
            ),
            &self.y_label,
            size,
            muted,
        );
    }
}

impl std::fmt::Debug for XYPad {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("XYPad")
            .field("x", &self.x)
            .field("y", &self.y)
            .finish()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use martensite_core::HotNode;

    fn laid_out(pad: &mut XYPad, w: f32, h: f32) {
        let mut hot = HotNode::default();
        let mut cx = LayoutContext {
            hot: &mut hot,
            scale: 1.0,
        };
        pad.measure(
            &mut cx,
            LayoutConstraints {
                min_size: Vec2::ZERO,
                max_size: Vec2::new(w, h),
            },
        );
        pad.layout(&mut cx, Rect::new(0.0, 0.0, w, h));
    }

    #[test]
    fn drag_sets_value() {
        let mut pad = XYPad::new();
        laid_out(&mut pad, 200.0, 200.0);
        let c = pad.pad;
        let mid = Vec2::new(c.min_x() + c.width() / 2.0, c.min_y() + c.height() / 2.0);
        pad.event(&mut EventContext {
            event: &WidgetEvent::PointerPressed {
                button: PointerButton::Primary,
                position: mid,
                count: 1,
            },
            bounds: Rect::new(0.0, 0.0, 200.0, 200.0),
            scale: 1.0,
        });
        let (x, y) = pad.value_xy();
        assert!((x - 0.5).abs() < 0.01 && (y - 0.5).abs() < 0.01);
        assert!(pad.take_changed());
    }

    #[test]
    fn drag_top_right_is_one_one() {
        let mut pad = XYPad::new();
        laid_out(&mut pad, 200.0, 200.0);
        let c = pad.pad;
        pad.event(&mut EventContext {
            event: &WidgetEvent::PointerPressed {
                button: PointerButton::Primary,
                position: Vec2::new(c.max_x() - 0.5, c.min_y() + 0.5),
                count: 1,
            },
            bounds: Rect::new(0.0, 0.0, 200.0, 200.0),
            scale: 1.0,
        });
        let (x, y) = pad.value_xy();
        assert!(x > 0.98 && y > 0.98);
    }

    #[test]
    fn arrows_nudge() {
        let mut pad = XYPad::new().value(0.5, 0.5);
        laid_out(&mut pad, 200.0, 200.0);
        for k in ["ArrowRight", "ArrowUp"] {
            pad.event(&mut EventContext {
                event: &WidgetEvent::KeyPressed {
                    key: k.to_string(),
                    repeat: false,
                },
                bounds: Rect::new(0.0, 0.0, 200.0, 200.0),
                scale: 1.0,
            });
        }
        let (x, y) = pad.value_xy();
        assert!((x - 0.55).abs() < 0.001 && (y - 0.55).abs() < 0.001);
    }

    #[test]
    fn home_end_snap() {
        let mut pad = XYPad::new().value(0.5, 0.5);
        laid_out(&mut pad, 200.0, 200.0);
        for k in ["End", "Home"] {
            pad.event(&mut EventContext {
                event: &WidgetEvent::KeyPressed {
                    key: k.to_string(),
                    repeat: false,
                },
                bounds: Rect::new(0.0, 0.0, 200.0, 200.0),
                scale: 1.0,
            });
        }
        assert_eq!(pad.value_xy(), (0.0, 0.0));
    }

    #[test]
    fn semantic_set_value_pair() {
        use martensite_core::widget::SemanticAction;
        let mut pad = XYPad::new();
        laid_out(&mut pad, 200.0, 200.0);
        pad.event(&mut EventContext {
            event: &WidgetEvent::SemanticAction(SemanticAction::SetValue("0.25, 0.75".to_string())),
            bounds: Rect::new(0.0, 0.0, 200.0, 200.0),
            scale: 1.0,
        });
        assert_eq!(pad.value_xy(), (0.25, 0.75));
        assert!(pad.take_changed());
    }

    #[test]
    fn press_outside_ignored() {
        let mut pad = XYPad::new();
        laid_out(&mut pad, 200.0, 200.0);
        pad.event(&mut EventContext {
            event: &WidgetEvent::PointerPressed {
                button: PointerButton::Primary,
                position: Vec2::new(1.0, 1.0),
                count: 1,
            },
            bounds: Rect::new(0.0, 0.0, 200.0, 200.0),
            scale: 1.0,
        });
        assert!(!pad.take_changed());
    }

    #[test]
    fn disabled_inert() {
        let mut pad = XYPad::new().enabled(false);
        laid_out(&mut pad, 200.0, 200.0);
        let c = pad.pad;
        pad.event(&mut EventContext {
            event: &WidgetEvent::PointerPressed {
                button: PointerButton::Primary,
                position: Vec2::new(c.min_x() + 4.0, c.min_y() + 4.0),
                count: 1,
            },
            bounds: Rect::new(0.0, 0.0, 200.0, 200.0),
            scale: 1.0,
        });
        assert!(!pad.take_changed());
    }
}
