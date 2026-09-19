//! `RangeSlider` widget: a dual-thumb slider for selecting a numeric
//! range — the QML `RangeSlider` / Material range-slider equivalent.
//!
//! Two thumbs ride a shared rail; the span *between* them is filled
//! with the accent colour. The low thumb can never cross the high
//! thumb — [`RangeSlider::set_low`] clamps at `high` and vice versa.
//!
//! - `Role::Slider` with `numeric_value` (the active thumb),
//!   `min_numeric_value`, `max_numeric_value`, `numeric_value_step`,
//!   `numeric_value_jump`, `orientation`, and a range `aria-valuetext`
//!   (`"low – high"` or the configured formatter).
//! - `Action::SetValue`, `Action::Increment`, `Action::Decrement` act
//!   on the *active* thumb.
//! - Keyboard: `Arrow*` step the active thumb, `Tab` moves the roving
//!   thumb focus between low and high, `PageUp`/`PageDown` jump by the
//!   page step, `Home`/`End` pull the active thumb to the range ends.
//! - Pointer: a press grabs the *nearest* thumb and drags it with
//!   pointer capture, so tracking continues outside the widget.
//!
//! # Examples
//!
//! ```
//! use martensite::widgets::RangeSlider;
//!
//! let mut rs = RangeSlider::new(0.0, 100.0);
//! rs.set_range(20.0, 80.0);
//! assert_eq!((rs.low(), rs.high()), (20.0, 80.0));
//! ```

use accesskit::Node as AccessKitNode;
use glam::Vec2;
use martensite_core::shape::Shape;
use martensite_core::widget::{
    EventContext, EventResponse, LayoutConstraints, LayoutContext, PaintContext, PointerButton,
    SemanticAction, Widget, WidgetEvent,
};
use martensite_core::{NodeFlags, Rect, TokenKey};

use crate::widgets::slider::SliderOrientation;

/// Thumb diameter in logical pixels.
const THUMB: f32 = 16.0;
/// Rail thickness in logical pixels.
const RAIL: f32 = 4.0;
/// Default page jump when none is configured (10% of the range).
const PAGE_FRACTION: f64 = 0.1;
/// Rail colour.
const RAIL_COLOR: [u8; 4] = [205, 210, 218, 255];
/// Filled span colour (accent).
const FILL_COLOR: [u8; 4] = [60, 110, 220, 255];
/// Thumb face colour.
const THUMB_COLOR: [u8; 4] = [255, 255, 255, 255];
/// Thumb edge colour.
const THUMB_EDGE: [u8; 4] = [90, 95, 105, 255];
/// Active-thumb focus ring colour.
const FOCUS_RING: [u8; 4] = [60, 110, 220, 128];

/// Which of a [`RangeSlider`]'s two thumbs an operation targets.
///
/// # Examples
///
/// ```
/// use martensite::widgets::RangeThumb;
///
/// assert_eq!(RangeThumb::default(), RangeThumb::Low);
/// assert_ne!(RangeThumb::Low, RangeThumb::High);
/// ```
#[derive(Copy, Clone, Debug, Default, PartialEq, Eq, Hash)]
pub enum RangeThumb {
    /// The lower (left/bottom) thumb — the range's start.
    #[default]
    Low,
    /// The upper (right/top) thumb — the range's end.
    High,
}

/// A dual-thumb range slider implementing the slider contract for
/// two clamped values.
///
/// `low` and `high` are always clamped to `[min, max]`, snapped to the
/// `step` grid measured from `min`, and kept ordered (`low <= high`).
/// One thumb is *active* at a time — a roving thumb focus moved by
/// `Tab` and by the last-dragged thumb — which keyboard steps and AT
/// `Increment`/`Decrement`/`SetValue` act on.
///
/// # Examples
///
/// ```
/// use martensite::widgets::RangeSlider;
///
/// let mut rs = RangeSlider::new(0.0, 10.0).step(0.5);
/// rs.set_low(3.3);
/// assert_eq!(rs.low(), 3.5); // snapped to the 0.5 grid
/// rs.set_low(9.9);
/// assert_eq!(rs.low(), 10.0); // clamped at high
/// ```
pub struct RangeSlider {
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
    /// Lower thumb value, always `<= high`.
    low: f64,
    /// Upper thumb value, always `>= low`.
    high: f64,
    /// `aria-valuetext` formatter over `(low, high)`.
    value_text: Option<Box<dyn Fn(f64, f64) -> String + Send + Sync>>,
    /// The thumb keyboard steps and AT actions target.
    active: RangeThumb,
    /// The thumb currently being dragged, if any.
    dragging: Option<RangeThumb>,
    /// Whether the widget holds keyboard focus (drives the active
    /// thumb's focus ring).
    focused: bool,
    /// Cached bounds from the last layout pass.
    cached_bounds: Rect,
    /// Display scale cached from `layout` so hit-math uses the same
    /// pixel sizes `paint` emits.
    scale: f32,
}

impl RangeSlider {
    /// Creates a range slider over `min..=max` with the thumbs parked
    /// at the range ends.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite::widgets::RangeSlider;
    ///
    /// let rs = RangeSlider::new(0.0, 100.0);
    /// assert_eq!((rs.low(), rs.high()), (0.0, 100.0));
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
            low: min,
            high: max,
            value_text: None,
            active: RangeThumb::Low,
            dragging: None,
            focused: false,
            cached_bounds: Rect::default(),
            scale: 1.0,
        }
    }

    /// Sets the accessible label.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite::widgets::RangeSlider;
    ///
    /// let rs = RangeSlider::new(0.0, 1.0).label("Price range");
    /// assert_eq!(rs.label.as_deref(), Some("Price range"));
    /// ```
    #[inline]
    #[must_use]
    pub fn label(mut self, label: impl Into<String>) -> Self {
        self.label = Some(label.into());
        self
    }

    /// Sets both thumb values (clamped, snapped, ordered).
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite::widgets::RangeSlider;
    ///
    /// let rs = RangeSlider::new(0.0, 100.0).with_range(20.0, 80.0);
    /// assert_eq!((rs.low(), rs.high()), (20.0, 80.0));
    /// ```
    #[inline]
    #[must_use]
    pub fn with_range(mut self, low: f64, high: f64) -> Self {
        self.set_range(low, high);
        self
    }

    /// Sets the arrow-key step.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite::widgets::RangeSlider;
    ///
    /// let rs = RangeSlider::new(0.0, 10.0).step(2.0).with_range(3.0, 9.0);
    /// assert_eq!((rs.low(), rs.high()), (4.0, 10.0));
    /// ```
    #[inline]
    #[must_use]
    pub fn step(mut self, step: f64) -> Self {
        self.step = step.max(f64::EPSILON);
        self.set_range(self.low, self.high);
        self
    }

    /// Sets the `PageUp`/`PageDown` jump.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite::widgets::RangeSlider;
    ///
    /// let rs = RangeSlider::new(0.0, 100.0).with_page_step(25.0);
    /// assert_eq!(rs.page_step(), 25.0);
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
    /// use martensite::widgets::{RangeSlider, SliderOrientation};
    ///
    /// let rs = RangeSlider::new(0.0, 10.0).orientation(SliderOrientation::Vertical);
    /// assert_eq!(rs.orientation, SliderOrientation::Vertical);
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
    /// use martensite::widgets::RangeSlider;
    ///
    /// let rs = RangeSlider::new(0.0, 10.0).enabled(false);
    /// assert!(!rs.enabled);
    /// ```
    #[inline]
    #[must_use]
    pub fn enabled(mut self, enabled: bool) -> Self {
        self.enabled = enabled;
        self
    }

    /// Sets the `aria-valuetext` formatter over `(low, high)`.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite::widgets::RangeSlider;
    ///
    /// let rs = RangeSlider::new(0.0, 100.0)
    ///     .with_range(20.0, 80.0)
    ///     .with_value_text(|lo, hi| format!("{lo:.0} to {hi:.0} dollars"));
    /// assert_eq!(rs.value_text(), "20 to 80 dollars");
    /// ```
    #[must_use]
    pub fn with_value_text(
        mut self,
        f: impl Fn(f64, f64) -> String + Send + Sync + 'static,
    ) -> Self {
        self.value_text = Some(Box::new(f));
        self
    }

    /// The lower thumb's value.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite::widgets::RangeSlider;
    ///
    /// let rs = RangeSlider::new(0.0, 10.0).with_range(3.0, 7.0);
    /// assert_eq!(rs.low(), 3.0);
    /// ```
    #[inline]
    pub fn low(&self) -> f64 {
        self.low
    }

    /// The upper thumb's value.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite::widgets::RangeSlider;
    ///
    /// let rs = RangeSlider::new(0.0, 10.0).with_range(3.0, 7.0);
    /// assert_eq!(rs.high(), 7.0);
    /// ```
    #[inline]
    pub fn high(&self) -> f64 {
        self.high
    }

    /// Sets the lower thumb, clamped to `[min, high]` and snapped.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite::widgets::RangeSlider;
    ///
    /// let mut rs = RangeSlider::new(0.0, 10.0).with_range(2.0, 5.0);
    /// rs.set_low(8.0);
    /// assert_eq!(rs.low(), 5.0); // clamped at high
    /// ```
    pub fn set_low(&mut self, low: f64) {
        self.low = self.snap(low).clamp(self.min, self.high);
    }

    /// Sets the upper thumb, clamped to `[low, max]` and snapped.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite::widgets::RangeSlider;
    ///
    /// let mut rs = RangeSlider::new(0.0, 10.0).with_range(2.0, 5.0);
    /// rs.set_high(1.0);
    /// assert_eq!(rs.high(), 2.0); // clamped at low
    /// ```
    pub fn set_high(&mut self, high: f64) {
        self.high = self.snap(high).clamp(self.low, self.max);
    }

    /// Sets both thumbs at once, keeping `low <= high`.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite::widgets::RangeSlider;
    ///
    /// let mut rs = RangeSlider::new(0.0, 10.0);
    /// rs.set_range(8.0, 2.0); // inverted input collapses in order
    /// assert!(rs.low() <= rs.high());
    /// ```
    pub fn set_range(&mut self, low: f64, high: f64) {
        let (low, high) = if low <= high {
            (low, high)
        } else {
            (high, low)
        };
        self.set_high(high);
        self.set_low(low);
    }

    /// The effective `PageUp`/`PageDown` jump (explicit `page_step` or
    /// 10% of the range, at least one step).
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite::widgets::RangeSlider;
    ///
    /// assert_eq!(RangeSlider::new(0.0, 100.0).page_step(), 10.0);
    /// assert_eq!(RangeSlider::new(0.0, 100.0).with_page_step(5.0).page_step(), 5.0);
    /// ```
    pub fn page_step(&self) -> f64 {
        self.page_step
            .unwrap_or((self.max - self.min) * PAGE_FRACTION)
            .max(self.step)
    }

    /// The thumb keyboard steps and AT actions currently target.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite::widgets::{RangeSlider, RangeThumb};
    ///
    /// assert_eq!(RangeSlider::new(0.0, 1.0).active_thumb(), RangeThumb::Low);
    /// ```
    #[inline]
    pub fn active_thumb(&self) -> RangeThumb {
        self.active
    }

    /// Whether a thumb is currently dragged.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite::widgets::RangeSlider;
    ///
    /// assert!(!RangeSlider::new(0.0, 1.0).is_dragging());
    /// ```
    #[inline]
    pub fn is_dragging(&self) -> bool {
        self.dragging.is_some()
    }

    /// The `aria-valuetext` for the current range — the configured
    /// formatter's output, or `"low – high"`.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite::widgets::RangeSlider;
    ///
    /// let rs = RangeSlider::new(0.0, 10.0).step(0.5).with_range(2.5, 7.5);
    /// assert_eq!(rs.value_text(), "2.5 – 7.5");
    /// ```
    pub fn value_text(&self) -> String {
        match &self.value_text {
            Some(f) => f(self.low, self.high),
            None => format!("{} – {}", self.low, self.high),
        }
    }

    /// Clamps `v` to `[min, max]` and snaps to the `step` grid.
    fn snap(&self, v: f64) -> f64 {
        let clamped = v.clamp(self.min, self.max);
        if self.step > 0.0 {
            let steps = ((clamped - self.min) / self.step).round();
            (self.min + steps * self.step).clamp(self.min, self.max)
        } else {
            clamped
        }
    }

    /// Sets `thumb`'s value, honouring the `low <= high` invariant.
    fn set_thumb(&mut self, thumb: RangeThumb, value: f64) {
        match thumb {
            RangeThumb::Low => self.set_low(value),
            RangeThumb::High => self.set_high(value),
        }
    }

    /// The fraction `0.0..=1.0` of `thumb`'s value through the range.
    fn fraction(&self, thumb: RangeThumb) -> f32 {
        let span = self.max - self.min;
        if span <= 0.0 {
            return 0.0;
        }
        let v = match thumb {
            RangeThumb::Low => self.low,
            RangeThumb::High => self.high,
        };
        ((v - self.min) / span).clamp(0.0, 1.0) as f32
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

    /// The given thumb's centre in window coordinates.
    fn thumb_center(&self, thumb: RangeThumb) -> Vec2 {
        let b = self.cached_bounds;
        let d = self.thumb_px();
        let t = self.fraction(thumb);
        match self.orientation {
            SliderOrientation::Horizontal => Vec2::new(
                b.min_x() + d / 2.0 + t * (b.width() - d).max(0.0),
                b.min_y() + b.height() / 2.0,
            ),
            SliderOrientation::Vertical => Vec2::new(
                b.min_x() + b.width() / 2.0,
                b.max_y() - d / 2.0 - t * (b.height() - d).max(0.0),
            ),
        }
    }

    /// The thumb nearest a window-space point (the one a press grabs).
    /// A tie goes to `Low`, matching "the value under the pointer
    /// starts editing" intuition at the collapsed-thumb case.
    fn nearest_thumb(&self, position: Vec2) -> RangeThumb {
        let dl = self
            .thumb_center(RangeThumb::Low)
            .distance_squared(position);
        let dh = self
            .thumb_center(RangeThumb::High)
            .distance_squared(position);
        if dh < dl {
            RangeThumb::High
        } else {
            RangeThumb::Low
        }
    }

    /// Sets `thumb`'s value from a window-space point.
    fn set_thumb_from_point(&mut self, thumb: RangeThumb, position: Vec2) {
        let f = self.point_fraction(position);
        self.set_thumb(thumb, self.min + f * (self.max - self.min));
    }

    /// Applies one step delta to the active thumb.
    fn nudge(&mut self, delta: f64) {
        let v = match self.active {
            RangeThumb::Low => self.low,
            RangeThumb::High => self.high,
        };
        self.set_thumb(self.active, v + delta);
    }

    fn key_delta(&self, key: &str) -> Option<f64> {
        let step = self.step;
        match key {
            "ArrowRight" | "ArrowUp" => Some(step),
            "ArrowLeft" | "ArrowDown" => Some(-step),
            "PageUp" => Some(self.page_step()),
            "PageDown" => Some(-self.page_step()),
            _ => None,
        }
    }
}

impl Widget for RangeSlider {
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
        // Two thumbs cannot both map to `numeric_value` — report the
        // active thumb there and the full range in `aria-valuetext`.
        node.set_numeric_value(match self.active {
            RangeThumb::Low => self.low,
            RangeThumb::High => self.high,
        });
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
                ..
            } => {
                // The nearest thumb wins the press and starts dragging.
                let thumb = self.nearest_thumb(*position);
                self.active = thumb;
                self.dragging = Some(thumb);
                self.set_thumb_from_point(thumb, *position);
                EventResponse::CapturePointer
            }
            WidgetEvent::PointerMoved { position } => {
                if let Some(thumb) = self.dragging {
                    self.set_thumb_from_point(thumb, *position);
                    EventResponse::RequestRepaint
                } else {
                    EventResponse::Ignored
                }
            }
            WidgetEvent::PointerReleased {
                button: PointerButton::Primary,
                ..
            } => {
                if self.dragging.is_some() {
                    self.dragging = None;
                    EventResponse::ReleasePointer
                } else {
                    EventResponse::Ignored
                }
            }
            WidgetEvent::FocusGained => {
                self.focused = true;
                EventResponse::RequestRepaint
            }
            WidgetEvent::FocusLost => {
                self.focused = false;
                self.dragging = None;
                EventResponse::RequestRepaint
            }
            WidgetEvent::KeyPressed { key, .. } => {
                if key == "Tab" {
                    // Roving thumb focus — the group is a single tab
                    // stop; Tab cycles which thumb the arrows drive.
                    self.active = match self.active {
                        RangeThumb::Low => RangeThumb::High,
                        RangeThumb::High => RangeThumb::Low,
                    };
                    return EventResponse::RequestRepaint;
                }
                if let Some(delta) = self.key_delta(key) {
                    self.nudge(delta);
                    EventResponse::RequestRepaint
                } else {
                    match key.as_str() {
                        "Home" => {
                            self.set_thumb(self.active, self.min);
                            EventResponse::RequestRepaint
                        }
                        "End" => {
                            self.set_thumb(self.active, self.max);
                            EventResponse::RequestRepaint
                        }
                        _ => EventResponse::Ignored,
                    }
                }
            }
            WidgetEvent::SemanticAction(action) => match action {
                SemanticAction::SetValue(text) => {
                    if let Ok(v) = text.parse::<f64>() {
                        self.set_thumb(self.active, v);
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
        let lo = self.thumb_center(RangeThumb::Low);
        let hi = self.thumb_center(RangeThumb::High);
        let (rail, fill) = match self.orientation {
            SliderOrientation::Horizontal => {
                let cy = f64::from(b.min_y() + b.height() / 2.0);
                let rail = kurbo::Rect::new(
                    f64::from(b.min_x()),
                    cy - cx.ptf(f64::from(RAIL) / 2.0),
                    f64::from(b.max_x()),
                    cy + cx.ptf(f64::from(RAIL) / 2.0),
                );
                // The accent fills the span *between* the thumbs.
                let fill = kurbo::Rect::new(
                    f64::from(lo.x),
                    rail.y0,
                    f64::from(hi.x).max(f64::from(lo.x)),
                    rail.y1,
                );
                (rail, fill)
            }
            SliderOrientation::Vertical => {
                let cxm = f64::from(b.min_x() + b.width() / 2.0);
                let rail = kurbo::Rect::new(
                    cxm - cx.ptf(f64::from(RAIL) / 2.0),
                    f64::from(b.min_y()),
                    cxm + cx.ptf(f64::from(RAIL) / 2.0),
                    f64::from(b.max_y()),
                );
                // The low thumb sits lower on screen than the high one.
                let fill = kurbo::Rect::new(
                    rail.x0,
                    f64::from(hi.y),
                    rail.x1,
                    f64::from(lo.y).max(f64::from(hi.y)),
                );
                (rail, fill)
            }
        };

        cx.list.push_fill_shape(
            rail,
            &Shape::PILL,
            cx.color(TokenKey::DividerColor, RAIL_COLOR),
        );
        cx.list.push_fill_shape(
            fill,
            &Shape::PILL,
            cx.color(TokenKey::AccentColor, FILL_COLOR),
        );

        for (center, thumb) in [(lo, RangeThumb::Low), (hi, RangeThumb::High)] {
            let thumb_shape = Shape::circle(center, cx.pt(THUMB / 2.0));
            cx.list.push_fill_shape(
                rail,
                &thumb_shape,
                cx.color(TokenKey::SurfaceColor, THUMB_COLOR),
            );
            cx.list.push_stroke_shape(
                rail,
                &thumb_shape,
                cx.pt(1.0),
                cx.color(TokenKey::BorderColor, THUMB_EDGE),
            );
            if self.focused && self.active == thumb {
                // A translucent accent ring marks the thumb the
                // keyboard currently drives.
                let accent = cx.color(TokenKey::AccentColor, FILL_COLOR);
                let ring = Shape::circle(center, cx.pt(THUMB / 2.0 + 3.0));
                cx.list.push_stroke_shape(
                    rail,
                    &ring,
                    cx.pt(2.0),
                    [accent[0], accent[1], accent[2], FOCUS_RING[3]],
                );
            }
        }
    }
}

impl std::fmt::Debug for RangeSlider {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("RangeSlider")
            .field("low", &self.low)
            .field("high", &self.high)
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

    fn laid_out(rs: &mut RangeSlider, w: f32, h: f32) {
        let mut hot = HotNode::default();
        let mut cx = LayoutContext {
            hot: &mut hot,
            scale: 1.0,
        };
        rs.layout(&mut cx, Rect::new(0.0, 0.0, w, h));
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
            count: 1,
        }
    }

    fn event(rs: &mut RangeSlider, ev: &WidgetEvent) -> EventResponse {
        let mut cx = EventContext {
            event: ev,
            bounds: rs.cached_bounds,
            scale: 1.0,
        };
        rs.event(&mut cx)
    }

    #[test]
    fn new_clamps_inverted_range() {
        let rs = RangeSlider::new(10.0, -10.0);
        assert_eq!(rs.min, -10.0);
        assert_eq!(rs.max, 10.0);
        assert_eq!((rs.low(), rs.high()), (-10.0, 10.0));
    }

    #[test]
    fn set_range_orders_snaps_and_clamps() {
        let mut rs = RangeSlider::new(0.0, 10.0).step(0.5);
        rs.set_range(2.2, 7.8);
        assert_eq!((rs.low(), rs.high()), (2.0, 8.0));
        rs.set_range(-5.0, 50.0);
        assert_eq!((rs.low(), rs.high()), (0.0, 10.0));
        // Inverted input swaps into order.
        rs.set_range(8.0, 2.0);
        assert_eq!((rs.low(), rs.high()), (2.0, 8.0));
    }

    #[test]
    fn thumbs_cannot_cross() {
        let mut rs = RangeSlider::new(0.0, 10.0).with_range(3.0, 7.0);
        rs.set_low(9.0);
        assert_eq!(rs.low(), 7.0);
        rs.set_high(1.0);
        assert_eq!(rs.high(), 7.0);
    }

    #[test]
    fn arrow_keys_step_active_thumb() {
        let mut rs = RangeSlider::new(0.0, 100.0).with_range(40.0, 60.0);
        laid_out(&mut rs, 116.0, 24.0);
        event(&mut rs, &key("ArrowRight"));
        assert_eq!(rs.low(), 41.0);
        assert_eq!(rs.high(), 60.0);
        event(&mut rs, &key("Tab"));
        assert_eq!(rs.active_thumb(), RangeThumb::High);
        event(&mut rs, &key("ArrowLeft"));
        assert_eq!(rs.high(), 59.0);
        assert_eq!(rs.low(), 41.0);
    }

    #[test]
    fn page_and_end_keys() {
        let mut rs = RangeSlider::new(0.0, 100.0)
            .with_range(40.0, 60.0)
            .with_page_step(20.0);
        laid_out(&mut rs, 116.0, 24.0);
        event(&mut rs, &key("PageUp"));
        assert_eq!(rs.low(), 60.0); // clamped at high
        event(&mut rs, &key("Home"));
        assert_eq!(rs.low(), 0.0);
        event(&mut rs, &key("Tab"));
        event(&mut rs, &key("End"));
        assert_eq!(rs.high(), 100.0);
    }

    #[test]
    fn press_grabs_nearest_thumb() {
        let mut rs = RangeSlider::new(0.0, 100.0).with_range(30.0, 70.0);
        laid_out(&mut rs, 116.0, 24.0);
        // Thumb x at value v: 8 + v/100 * 100 → low≈38, high≈78.
        // A press at x=60 is nearer the high thumb → drags high.
        assert_eq!(
            event(&mut rs, &press(60.0, 12.0)),
            EventResponse::CapturePointer
        );
        assert_eq!(rs.active_thumb(), RangeThumb::High);
        assert_eq!(rs.high(), 52.0); // fraction (60-8)/100
        assert_eq!(rs.low(), 30.0);
        assert!(rs.is_dragging());
        // A press at x=20 is nearer the low thumb.
        let mut rs = RangeSlider::new(0.0, 100.0).with_range(30.0, 70.0);
        laid_out(&mut rs, 116.0, 24.0);
        event(&mut rs, &press(20.0, 12.0));
        assert_eq!(rs.active_thumb(), RangeThumb::Low);
        assert_eq!(rs.low(), 12.0);
    }

    #[test]
    fn drag_moves_and_release_frees() {
        let mut rs = RangeSlider::new(0.0, 100.0).with_range(30.0, 70.0);
        laid_out(&mut rs, 116.0, 24.0);
        event(&mut rs, &press(38.0, 12.0)); // on the low thumb
        assert_eq!(rs.dragging, Some(RangeThumb::Low));
        let moved = WidgetEvent::PointerMoved {
            position: Vec2::new(58.0, 12.0),
        };
        event(&mut rs, &moved);
        assert_eq!(rs.low(), 50.0);
        let released = WidgetEvent::PointerReleased {
            position: Vec2::new(58.0, 12.0),
            button: PointerButton::Primary,
        };
        assert_eq!(event(&mut rs, &released), EventResponse::ReleasePointer);
        assert!(!rs.is_dragging());
    }

    #[test]
    fn drag_low_stops_at_high() {
        let mut rs = RangeSlider::new(0.0, 100.0).with_range(30.0, 70.0);
        laid_out(&mut rs, 116.0, 24.0);
        event(&mut rs, &press(38.0, 12.0));
        let moved = WidgetEvent::PointerMoved {
            position: Vec2::new(108.0, 12.0),
        };
        event(&mut rs, &moved);
        assert_eq!(rs.low(), 70.0); // pinned at high, not past it
        assert_eq!(rs.high(), 70.0);
    }

    #[test]
    fn vertical_increases_upward() {
        let mut rs = RangeSlider::new(0.0, 100.0).orientation(SliderOrientation::Vertical);
        laid_out(&mut rs, 24.0, 116.0);
        // Press near the top → fraction ~1.0. Thumbs are equidistant
        // only by value; nearest by position picks high (at top).
        event(&mut rs, &press(12.0, 8.0));
        assert_eq!(rs.active_thumb(), RangeThumb::High);
        assert_eq!(rs.high(), 100.0);
    }

    #[test]
    fn semantic_actions_target_active_thumb() {
        let mut rs = RangeSlider::new(0.0, 10.0).with_range(4.0, 6.0);
        laid_out(&mut rs, 116.0, 24.0);
        event(
            &mut rs,
            &WidgetEvent::SemanticAction(SemanticAction::Increment),
        );
        assert_eq!(rs.low(), 5.0);
        event(&mut rs, &key("Tab"));
        event(
            &mut rs,
            &WidgetEvent::SemanticAction(SemanticAction::SetValue("9.0".into())),
        );
        assert_eq!(rs.high(), 9.0);
    }

    #[test]
    fn accessibility_contract() {
        let rs = RangeSlider::new(0.0, 100.0)
            .with_range(20.0, 80.0)
            .step(5.0)
            .with_page_step(20.0)
            .label("Price")
            .with_value_text(|lo, hi| format!("{lo:.0} to {hi:.0} dollars"));
        let mut node = AccessKitNode::new(accesskit::Role::Unknown);
        rs.accessibility(&mut node);
        assert_eq!(node.role(), accesskit::Role::Slider);
        assert_eq!(node.numeric_value(), Some(20.0)); // active = Low
        assert_eq!(node.min_numeric_value(), Some(0.0));
        assert_eq!(node.max_numeric_value(), Some(100.0));
        assert_eq!(node.numeric_value_step(), Some(5.0));
        assert_eq!(node.numeric_value_jump(), Some(20.0));
        assert_eq!(node.value(), Some("20 to 80 dollars"));
        assert_eq!(node.label(), Some("Price"));
        assert_eq!(node.orientation(), Some(accesskit::Orientation::Horizontal));
        assert!(node.supports_action(accesskit::Action::SetValue));
        assert!(node.supports_action(accesskit::Action::Increment));
        assert!(node.supports_action(accesskit::Action::Decrement));
    }

    #[test]
    fn disabled_ignores_input() {
        let mut rs = RangeSlider::new(0.0, 10.0).enabled(false);
        laid_out(&mut rs, 116.0, 24.0);
        assert_eq!(event(&mut rs, &press(50.0, 12.0)), EventResponse::Ignored);
        assert_eq!(event(&mut rs, &key("ArrowRight")), EventResponse::Ignored);
        assert_eq!((rs.low(), rs.high()), (0.0, 10.0));
    }
}
