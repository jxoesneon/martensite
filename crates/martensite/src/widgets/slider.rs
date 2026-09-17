//! `Slider` widget: an ARIA APG slider for selecting a numeric value
//! from a range.
//!
//! Implements the [APG slider pattern](https://www.w3.org/WAI/ARIA/apg/patterns/slider/):
//!
//! - `Role::Slider` with `numeric_value`, `min_numeric_value`,
//!   `max_numeric_value`, `numeric_value_step`, `numeric_value_jump`,
//!   `orientation`, and a configurable `aria-valuetext` formatter.
//! - `Action::SetValue`, `Action::Increment`, `Action::Decrement`.
//! - Keyboard: `Arrow*` step, `Home`/`End` to the range ends,
//!   `PageUp`/`PageDown` step by the page jump.
//! - Pointer: rail press jumps the thumb, thumb drags use pointer
//!   capture so tracking continues outside the widget.
//!
//! # Examples
//!
//! ```
//! use martensite::widgets::Slider;
//!
//! let slider = Slider::new(0.0, 100.0).with_value(25.0).step(5.0);
//! assert_eq!(slider.value(), 25.0);
//! ```

use accesskit::Node as AccessKitNode;
use glam::Vec2;
use kurbo::Shape;
use martensite_core::widget::{
    EventContext, EventResponse, LayoutConstraints, LayoutContext, PaintContext, PointerButton,
    SemanticAction, Widget, WidgetEvent,
};
use martensite_core::{NodeFlags, Rect, TokenKey};

/// Thumb diameter in logical pixels.
const THUMB: f32 = 16.0;
/// Rail thickness in logical pixels.
const RAIL: f32 = 4.0;
/// Default page jump when none is configured (10% of the range).
const PAGE_FRACTION: f64 = 0.1;
/// Rail colour.
const RAIL_COLOR: [u8; 4] = [205, 210, 218, 255];
/// Filled rail colour (accent).
const FILL_COLOR: [u8; 4] = [60, 110, 220, 255];
/// Thumb face colour.
const THUMB_COLOR: [u8; 4] = [255, 255, 255, 255];
/// Thumb edge colour.
const THUMB_EDGE: [u8; 4] = [90, 95, 105, 255];

/// Orientation of a [`Slider`].
///
/// # Examples
///
/// ```
/// use martensite::widgets::SliderOrientation;
///
/// assert_eq!(SliderOrientation::default(), SliderOrientation::Horizontal);
/// ```
#[derive(Copy, Clone, Debug, Default, PartialEq, Eq, Hash)]
pub enum SliderOrientation {
    /// Value increases left→right; `ArrowRight`/`ArrowUp` increment.
    #[default]
    Horizontal,
    /// Value increases bottom→top; `ArrowUp`/`ArrowRight` increment.
    Vertical,
}

/// A numeric slider widget implementing the ARIA APG slider contract.
///
/// Values are always clamped to `[min, max]` and snapped to the nearest
/// `step` grid offset from `min`. The `aria-valuetext` formatter (see
/// [`Slider::value_text`]) customises the spoken value; without one the
/// numeric value is reported verbatim.
///
/// # Examples
///
/// ```
/// use martensite::widgets::Slider;
///
/// let mut slider = Slider::new(0.0, 10.0).step(0.5);
/// slider.set_value(3.3);
/// assert_eq!(slider.value(), 3.5); // snapped to the 0.5 grid
/// ```
pub struct Slider {
    /// Optional accessible label.
    pub label: Option<String>,
    /// Whether the slider accepts input.
    pub enabled: bool,
    /// Minimum value.
    pub min: f64,
    /// Maximum value.
    pub max: f64,
    /// Arrow-key step.
    pub step: f64,
    /// Page jump (`PageUp`/`PageDown`); defaults to 10% of the range.
    pub page_step: Option<f64>,
    /// Slider orientation.
    pub orientation: SliderOrientation,
    /// Current value, always clamped and snapped.
    value: f64,
    /// `aria-valuetext` formatter.
    value_text: Option<Box<dyn Fn(f64) -> String + Send + Sync>>,
    /// Whether the thumb is currently being dragged.
    dragging: bool,
    /// Cached bounds from the last layout pass.
    cached_bounds: Rect,
    /// Display scale cached from `layout` so hit-math uses the same
    /// pixel sizes `paint` emits.
    scale: f32,
}

impl Slider {
    /// Creates a slider over `min..=max` starting at `min`.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite::widgets::Slider;
    ///
    /// let s = Slider::new(0.0, 100.0);
    /// assert_eq!(s.value(), 0.0);
    /// ```
    pub fn new(min: f64, max: f64) -> Self {
        let (min, max) = if min <= max { (min, max) } else { (max, min) };
        Self {
            label: None,
            enabled: true,
            min,
            max,
            step: 1.0,
            page_step: None,
            orientation: SliderOrientation::Horizontal,
            value: min,
            value_text: None,
            dragging: false,
            cached_bounds: Rect::default(),
            scale: 1.0,
        }
    }

    /// Sets the accessible label.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite::widgets::Slider;
    ///
    /// let s = Slider::new(0.0, 1.0).label("Volume");
    /// assert_eq!(s.label.as_deref(), Some("Volume"));
    /// ```
    #[inline]
    #[must_use]
    pub fn label(mut self, label: impl Into<String>) -> Self {
        self.label = Some(label.into());
        self
    }

    /// Sets the current value (clamped and snapped).
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite::widgets::Slider;
    ///
    /// let s = Slider::new(0.0, 100.0).with_value(40.0);
    /// assert_eq!(s.value(), 40.0);
    /// ```
    #[inline]
    #[must_use]
    pub fn with_value(mut self, value: f64) -> Self {
        self.set_value(value);
        self
    }

    /// Sets the arrow-key step.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite::widgets::Slider;
    ///
    /// let s = Slider::new(0.0, 10.0).step(2.0).with_value(3.0);
    /// assert_eq!(s.value(), 4.0); // snapped to the 2.0 grid
    /// ```
    #[inline]
    #[must_use]
    pub fn step(mut self, step: f64) -> Self {
        self.step = step.max(f64::EPSILON);
        self.set_value(self.value);
        self
    }

    /// Sets the `PageUp`/`PageDown` jump.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite::widgets::Slider;
    ///
    /// let s = Slider::new(0.0, 100.0).with_page_step(25.0);
    /// assert_eq!(s.page_step(), 25.0);
    /// ```
    #[inline]
    #[must_use]
    pub fn with_page_step(mut self, page_step: f64) -> Self {
        self.page_step = Some(page_step.max(0.0));
        self
    }

    /// Sets the orientation.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite::widgets::{Slider, SliderOrientation};
    ///
    /// let s = Slider::new(0.0, 10.0).orientation(SliderOrientation::Vertical);
    /// assert_eq!(s.orientation, SliderOrientation::Vertical);
    /// ```
    #[inline]
    #[must_use]
    pub fn orientation(mut self, orientation: SliderOrientation) -> Self {
        self.orientation = orientation;
        self
    }

    /// Sets whether the slider is enabled.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite::widgets::Slider;
    ///
    /// let s = Slider::new(0.0, 10.0).enabled(false);
    /// assert!(!s.enabled);
    /// ```
    #[inline]
    #[must_use]
    pub fn enabled(mut self, enabled: bool) -> Self {
        self.enabled = enabled;
        self
    }

    /// Sets the `aria-valuetext` formatter.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite::widgets::Slider;
    ///
    /// let s = Slider::new(0.0, 100.0)
    ///     .with_value(50.0)
    ///     .with_value_text(|v| format!("{v:.0} percent"));
    /// assert_eq!(s.value_text(), "50 percent");
    /// ```
    #[must_use]
    pub fn with_value_text(mut self, f: impl Fn(f64) -> String + Send + Sync + 'static) -> Self {
        self.value_text = Some(Box::new(f));
        self
    }

    /// The current value.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite::widgets::Slider;
    ///
    /// let s = Slider::new(-10.0, 10.0).with_value(3.0);
    /// assert_eq!(s.value(), 3.0);
    /// ```
    #[inline]
    pub fn value(&self) -> f64 {
        self.value
    }

    /// Sets the value, clamping to `[min, max]` and snapping to the
    /// `step` grid measured from `min`.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite::widgets::Slider;
    ///
    /// let mut s = Slider::new(0.0, 10.0).step(0.25);
    /// s.set_value(99.0);
    /// assert_eq!(s.value(), 10.0); // clamped
    /// s.set_value(4.13);
    /// assert_eq!(s.value(), 4.25); // snapped
    /// ```
    pub fn set_value(&mut self, value: f64) {
        let clamped = value.clamp(self.min, self.max);
        let snapped = if self.step > 0.0 {
            let steps = ((clamped - self.min) / self.step).round();
            (self.min + steps * self.step).clamp(self.min, self.max)
        } else {
            clamped
        };
        self.value = snapped;
    }

    /// The effective `PageUp`/`PageDown` jump (explicit `page_step` or
    /// 10% of the range, at least one step).
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite::widgets::Slider;
    ///
    /// assert_eq!(Slider::new(0.0, 100.0).page_step(), 10.0);
    /// assert_eq!(Slider::new(0.0, 100.0).with_page_step(5.0).page_step(), 5.0);
    /// ```
    pub fn page_step(&self) -> f64 {
        self.page_step
            .unwrap_or((self.max - self.min) * PAGE_FRACTION)
            .max(self.step)
    }

    /// Whether the thumb is currently dragged.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite::widgets::Slider;
    ///
    /// assert!(!Slider::new(0.0, 1.0).is_dragging());
    /// ```
    #[inline]
    pub fn is_dragging(&self) -> bool {
        self.dragging
    }

    /// The `aria-valuetext` for the current value — the configured
    /// formatter's output, or the bare numeric value.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite::widgets::Slider;
    ///
    /// let s = Slider::new(0.0, 10.0).step(0.5).with_value(2.5);
    /// assert_eq!(s.value_text(), "2.5");
    /// ```
    pub fn value_text(&self) -> String {
        match &self.value_text {
            Some(f) => f(self.value),
            None => format!("{}", self.value),
        }
    }

    /// The fraction `0.0..=1.0` of `value` through the range.
    fn fraction(&self) -> f32 {
        let span = self.max - self.min;
        if span <= 0.0 {
            return 0.0;
        }
        ((self.value - self.min) / span).clamp(0.0, 1.0) as f32
    }

    /// The thumb diameter in the arena's coordinate space.
    fn thumb_px(&self) -> f32 {
        THUMB * self.scale
    }

    /// Maps a window-space point to a fraction along the rail.
    fn point_fraction(&self, position: Vec2) -> f64 {
        let b = self.cached_bounds;
        let thumb = self.thumb_px();
        match self.orientation {
            SliderOrientation::Horizontal => {
                let usable = (b.width() - thumb).max(f32::EPSILON);
                ((position.x - b.min_x() - thumb / 2.0) / usable).clamp(0.0, 1.0)
            }
            SliderOrientation::Vertical => {
                let usable = (b.height() - thumb).max(f32::EPSILON);
                // Vertical sliders increase bottom→top.
                (1.0 - (position.y - b.min_y() - thumb / 2.0) / usable).clamp(0.0, 1.0)
            }
        }
        .into()
    }

    /// Sets the value from a window-space point (rail press / thumb
    /// drag).
    fn set_value_from_point(&mut self, position: Vec2) {
        let f = self.point_fraction(position);
        self.set_value(self.min + f * (self.max - self.min));
    }

    /// The thumb's centre in window coordinates.
    fn thumb_center(&self) -> Vec2 {
        let b = self.cached_bounds;
        let thumb = self.thumb_px();
        let t = self.fraction();
        match self.orientation {
            SliderOrientation::Horizontal => Vec2::new(
                b.min_x() + thumb / 2.0 + t * (b.width() - thumb).max(0.0),
                b.min_y() + b.height() / 2.0,
            ),
            SliderOrientation::Vertical => Vec2::new(
                b.min_x() + b.width() / 2.0,
                b.max_y() - thumb / 2.0 - t * (b.height() - thumb).max(0.0),
            ),
        }
    }

    /// Applies one keyboard step delta.
    fn nudge(&mut self, delta: f64) {
        self.set_value(self.value + delta);
    }

    fn key_delta(&self, key: &str) -> Option<f64> {
        let step = self.step;
        match (key, self.orientation) {
            ("ArrowRight", _) | ("ArrowUp", _) => Some(step),
            ("ArrowLeft", _) | ("ArrowDown", _) => Some(-step),
            ("PageUp", _) => Some(self.page_step()),
            ("PageDown", _) => Some(-self.page_step()),
            _ => None,
        }
    }
}

impl Widget for Slider {
    fn measure(&mut self, cx: &mut LayoutContext, constraints: LayoutConstraints) -> Vec2 {
        let (w, h): (f32, f32) = match self.orientation {
            SliderOrientation::Horizontal => (160.0, 24.0),
            SliderOrientation::Vertical => (24.0, 160.0),
        };
        Vec2::new(
            cx.pt(w).min(constraints.max_size.x.max(0.0)),
            cx.pt(h).min(constraints.max_size.y.max(0.0)),
        )
    }

    fn layout(&mut self, cx: &mut LayoutContext, bounds: Rect) {
        self.cached_bounds = bounds;
        self.scale = cx.scale;
        // Declare keyboard focusability on the arena node so the
        // `FocusManager` accepts focus requests and press-to-focus
        // applies.
        if self.enabled {
            cx.hot.flags |= NodeFlags::FOCUSABLE;
        } else {
            cx.hot.flags.remove(NodeFlags::FOCUSABLE);
        }
    }

    fn accessibility(&self, node: &mut AccessKitNode) {
        node.set_role(accesskit::Role::Slider);
        node.set_numeric_value(self.value);
        node.set_min_numeric_value(self.min);
        node.set_max_numeric_value(self.max);
        node.set_numeric_value_step(self.step);
        node.set_numeric_value_jump(self.page_step());
        node.set_value(self.value_text());
        node.set_orientation(match self.orientation {
            SliderOrientation::Horizontal => accesskit::Orientation::Horizontal,
            SliderOrientation::Vertical => accesskit::Orientation::Vertical,
        });
        node.add_action(accesskit::Action::SetValue);
        node.add_action(accesskit::Action::Increment);
        node.add_action(accesskit::Action::Decrement);
        if self.enabled {
            node.add_action(accesskit::Action::Focus);
        }
        if let Some(ref label) = self.label {
            node.set_label(label.as_str());
        }
        if !self.enabled {
            node.set_disabled();
        }
    }

    fn event(&mut self, cx: &mut EventContext) -> EventResponse {
        if !self.enabled {
            return EventResponse::Ignored;
        }
        match cx.event {
            WidgetEvent::PointerPressed {
                position,
                button: PointerButton::Primary,
            } => {
                // Rail clicks jump the thumb straight to the press point.
                self.set_value_from_point(*position);
                self.dragging = true;
                EventResponse::CapturePointer
            }
            WidgetEvent::PointerMoved { position } => {
                if self.dragging {
                    self.set_value_from_point(*position);
                    EventResponse::RequestRepaint
                } else {
                    EventResponse::Ignored
                }
            }
            WidgetEvent::PointerReleased {
                position,
                button: PointerButton::Primary,
            } => {
                if self.dragging {
                    self.set_value_from_point(*position);
                    self.dragging = false;
                    EventResponse::ReleasePointer
                } else {
                    EventResponse::Ignored
                }
            }
            WidgetEvent::KeyPressed { key, .. } => {
                if let Some(delta) = self.key_delta(key) {
                    self.nudge(delta);
                    EventResponse::RequestRepaint
                } else {
                    match key.as_str() {
                        "Home" => {
                            self.set_value(self.min);
                            EventResponse::RequestRepaint
                        }
                        "End" => {
                            self.set_value(self.max);
                            EventResponse::RequestRepaint
                        }
                        _ => EventResponse::Ignored,
                    }
                }
            }
            WidgetEvent::SemanticAction(action) => match action {
                SemanticAction::SetValue(text) => {
                    if let Ok(v) = text.parse::<f64>() {
                        self.set_value(v);
                    }
                    EventResponse::RequestRepaint
                }
                SemanticAction::Increment => {
                    self.nudge(self.step);
                    EventResponse::RequestRepaint
                }
                SemanticAction::Decrement => {
                    self.nudge(-self.step);
                    EventResponse::RequestRepaint
                }
                SemanticAction::Focus => EventResponse::CaptureFocus,
                _ => EventResponse::Ignored,
            },
            _ => EventResponse::Ignored,
        }
    }

    fn paint(&self, cx: &mut PaintContext) {
        let b = cx.bounds;
        let (rail, fill, thumb) = match self.orientation {
            SliderOrientation::Horizontal => {
                let cy = f64::from(b.min_y() + b.height() / 2.0);
                let rail = kurbo::Rect::new(
                    f64::from(b.min_x()),
                    cy - cx.ptf(f64::from(RAIL) / 2.0),
                    f64::from(b.max_x()),
                    cy + cx.ptf(f64::from(RAIL) / 2.0),
                );
                let tx = f64::from(self.thumb_center().x);
                let fill = kurbo::Rect::new(rail.x0, rail.y0, tx, rail.y1);
                (rail, fill, self.thumb_center())
            }
            SliderOrientation::Vertical => {
                let cxm = f64::from(b.min_x() + b.width() / 2.0);
                let rail = kurbo::Rect::new(
                    cxm - cx.ptf(f64::from(RAIL) / 2.0),
                    f64::from(b.min_y()),
                    cxm + cx.ptf(f64::from(RAIL) / 2.0),
                    f64::from(b.max_y()),
                );
                let ty = f64::from(self.thumb_center().y);
                let fill = kurbo::Rect::new(rail.x0, ty, rail.x1, rail.y1);
                (rail, fill, self.thumb_center())
            }
        };

        cx.list.push_path(
            kurbo::RoundedRect::from_rect(rail, cx.ptf(f64::from(RAIL) / 2.0)).to_path(0.1),
            cx.color(TokenKey::DividerColor, RAIL_COLOR),
        );
        cx.list.push_path(
            kurbo::RoundedRect::from_rect(fill, cx.ptf(f64::from(RAIL) / 2.0)).to_path(0.1),
            cx.color(TokenKey::AccentColor, FILL_COLOR),
        );

        let thumb = kurbo::Circle::new(
            kurbo::Point::new(f64::from(thumb.x), f64::from(thumb.y)),
            cx.ptf(f64::from(THUMB) / 2.0),
        );
        cx.list.push_path(
            thumb.to_path(0.1),
            cx.color(TokenKey::SurfaceColor, THUMB_COLOR),
        );
        cx.list.push_stroke_path(
            kurbo::Circle::new(thumb.center, thumb.radius).to_path(0.1),
            cx.pt(1.0),
            cx.color(TokenKey::BorderColor, THUMB_EDGE),
        );
    }
}

impl std::fmt::Debug for Slider {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Slider")
            .field("value", &self.value)
            .field("min", &self.min)
            .field("max", &self.max)
            .field("step", &self.step)
            .field("orientation", &self.orientation)
            .field("enabled", &self.enabled)
            .finish()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use martensite_core::HotNode;

    fn laid_out(slider: &mut Slider, w: f32, h: f32) {
        let mut hot = HotNode::default();
        let mut cx = LayoutContext {
            hot: &mut hot,
            scale: 1.0,
        };
        slider.layout(&mut cx, Rect::new(0.0, 0.0, w, h));
    }

    fn key(k: &str) -> WidgetEvent {
        WidgetEvent::KeyPressed {
            key: k.to_string(),
            repeat: false,
        }
    }

    fn press(x: f32, y: f32) -> WidgetEvent {
        WidgetEvent::PointerPressed {
            position: Vec2::new(x, y),
            button: PointerButton::Primary,
        }
    }

    #[test]
    fn new_clamps_inverted_range() {
        let s = Slider::new(10.0, -10.0);
        assert_eq!(s.min, -10.0);
        assert_eq!(s.max, 10.0);
    }

    #[test]
    fn set_value_clamps_and_snaps() {
        let mut s = Slider::new(0.0, 10.0).step(0.5);
        s.set_value(7.3);
        assert_eq!(s.value(), 7.5);
        s.set_value(-5.0);
        assert_eq!(s.value(), 0.0);
        s.set_value(50.0);
        assert_eq!(s.value(), 10.0);
    }

    #[test]
    fn arrow_keys_step() {
        let mut s = Slider::new(0.0, 10.0).with_value(5.0);
        let mut hot = HotNode::default();
        let mut cx = LayoutContext {
            hot: &mut hot,
            scale: 1.0,
        };
        s.layout(&mut cx, Rect::new(0.0, 0.0, 100.0, 24.0));
        let mut ecx = EventContext {
            event: &key("ArrowRight"),
            bounds: s.cached_bounds,
        };
        assert_eq!(s.event(&mut ecx), EventResponse::RequestRepaint);
        assert_eq!(s.value(), 6.0);
        let mut ecx = EventContext {
            event: &key("ArrowLeft"),
            bounds: s.cached_bounds,
        };
        s.event(&mut ecx);
        assert_eq!(s.value(), 5.0);
    }

    #[test]
    fn home_end_jump_to_ends() {
        let mut s = Slider::new(0.0, 10.0).with_value(5.0);
        laid_out(&mut s, 100.0, 24.0);
        let mut ecx = EventContext {
            event: &key("End"),
            bounds: s.cached_bounds,
        };
        s.event(&mut ecx);
        assert_eq!(s.value(), 10.0);
        let mut ecx = EventContext {
            event: &key("Home"),
            bounds: s.cached_bounds,
        };
        s.event(&mut ecx);
        assert_eq!(s.value(), 0.0);
    }

    #[test]
    fn page_keys_use_jump() {
        let mut s = Slider::new(0.0, 100.0)
            .with_value(50.0)
            .with_page_step(20.0);
        laid_out(&mut s, 100.0, 24.0);
        let mut ecx = EventContext {
            event: &key("PageUp"),
            bounds: s.cached_bounds,
        };
        s.event(&mut ecx);
        assert_eq!(s.value(), 70.0);
        let mut ecx = EventContext {
            event: &key("PageDown"),
            bounds: s.cached_bounds,
        };
        s.event(&mut ecx);
        assert_eq!(s.value(), 50.0);
    }

    #[test]
    fn rail_press_jumps_and_captures() {
        let mut s = Slider::new(0.0, 100.0);
        laid_out(&mut s, 116.0, 24.0);
        // Press at x=58 → fraction (58-8)/(116-16)=0.5 → value 50.
        let mut ecx = EventContext {
            event: &press(58.0, 12.0),
            bounds: s.cached_bounds,
        };
        assert_eq!(s.event(&mut ecx), EventResponse::CapturePointer);
        assert_eq!(s.value(), 50.0);
        assert!(s.is_dragging());
    }

    #[test]
    fn drag_moves_thumb_and_release_frees() {
        let mut s = Slider::new(0.0, 100.0);
        laid_out(&mut s, 116.0, 24.0);
        let mut ecx = EventContext {
            event: &press(8.0, 12.0),
            bounds: s.cached_bounds,
        };
        s.event(&mut ecx);
        assert_eq!(s.value(), 0.0);

        let moved = WidgetEvent::PointerMoved {
            position: Vec2::new(108.0, 12.0),
        };
        let mut ecx = EventContext {
            event: &moved,
            bounds: s.cached_bounds,
        };
        s.event(&mut ecx);
        assert_eq!(s.value(), 100.0);

        let released = WidgetEvent::PointerReleased {
            position: Vec2::new(108.0, 12.0),
            button: PointerButton::Primary,
        };
        let mut ecx = EventContext {
            event: &released,
            bounds: s.cached_bounds,
        };
        assert_eq!(s.event(&mut ecx), EventResponse::ReleasePointer);
        assert!(!s.is_dragging());
    }

    #[test]
    fn vertical_increases_upward() {
        let mut s = Slider::new(0.0, 100.0).orientation(SliderOrientation::Vertical);
        laid_out(&mut s, 24.0, 116.0);
        // Press at the top → fraction 1.0 → max.
        let mut ecx = EventContext {
            event: &press(12.0, 8.0),
            bounds: s.cached_bounds,
        };
        s.event(&mut ecx);
        assert_eq!(s.value(), 100.0);
        // Press at the bottom → fraction 0.0 → min.
        let mut s2 = Slider::new(0.0, 100.0).orientation(SliderOrientation::Vertical);
        laid_out(&mut s2, 24.0, 116.0);
        let mut ecx = EventContext {
            event: &press(12.0, 108.0),
            bounds: s2.cached_bounds,
        };
        s2.event(&mut ecx);
        assert_eq!(s2.value(), 0.0);
    }

    #[test]
    fn semantic_set_value_and_step() {
        let mut s = Slider::new(0.0, 10.0).with_value(5.0);
        laid_out(&mut s, 100.0, 24.0);
        let ev = WidgetEvent::SemanticAction(SemanticAction::SetValue("7.0".into()));
        let mut ecx = EventContext {
            event: &ev,
            bounds: s.cached_bounds,
        };
        s.event(&mut ecx);
        assert_eq!(s.value(), 7.0);
        let ev = WidgetEvent::SemanticAction(SemanticAction::Increment);
        let mut ecx = EventContext {
            event: &ev,
            bounds: s.cached_bounds,
        };
        s.event(&mut ecx);
        assert_eq!(s.value(), 8.0);
    }

    #[test]
    fn accessibility_contract() {
        let s = Slider::new(0.0, 100.0)
            .with_value(40.0)
            .step(5.0)
            .with_page_step(20.0)
            .label("Volume")
            .with_value_text(|v| format!("{v:.0} percent"));
        let mut node = AccessKitNode::new(accesskit::Role::Unknown);
        s.accessibility(&mut node);
        assert_eq!(node.role(), accesskit::Role::Slider);
        assert_eq!(node.numeric_value(), Some(40.0));
        assert_eq!(node.min_numeric_value(), Some(0.0));
        assert_eq!(node.max_numeric_value(), Some(100.0));
        assert_eq!(node.numeric_value_step(), Some(5.0));
        assert_eq!(node.numeric_value_jump(), Some(20.0));
        assert_eq!(node.value(), Some("40 percent"));
        assert_eq!(node.label(), Some("Volume"));
        assert_eq!(node.orientation(), Some(accesskit::Orientation::Horizontal));
        assert!(node.supports_action(accesskit::Action::SetValue));
        assert!(node.supports_action(accesskit::Action::Increment));
        assert!(node.supports_action(accesskit::Action::Decrement));
    }

    #[test]
    fn disabled_ignores_input() {
        let mut s = Slider::new(0.0, 10.0).enabled(false);
        laid_out(&mut s, 100.0, 24.0);
        let mut ecx = EventContext {
            event: &press(50.0, 12.0),
            bounds: s.cached_bounds,
        };
        assert_eq!(s.event(&mut ecx), EventResponse::Ignored);
        assert_eq!(s.value(), 0.0);
    }
}
