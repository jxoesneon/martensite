//! `SpinBox` widget: a numeric entry field with increment/decrement
//! step buttons — the `QSpinBox` / `NumberBox` equivalent.
//!
//! The widget embeds a [`TextInput`] as an internal child for the
//! editable value (caret, selection, and IME come for free through the
//! `child_count`/`child_bounds` forwarding protocol) and paints a
//! stacked ▲/▼ stepper column on its right edge.
//!
//! - `Role::SpinButton` with `numeric_value`, `min_numeric_value`,
//!   `max_numeric_value`, `numeric_value_step`, `numeric_value_jump`,
//!   and `Action::SetValue`/`Increment`/`Decrement`/`Focus`.
//! - Keyboard: `ArrowUp`/`ArrowDown` step, `PageUp`/`PageDown` jump by
//!   the page step, `Home`/`End` to the range ends, `Enter` commits the
//!   typed text.
//! - Pointer: ▲/▼ press steps once; holding the press auto-repeats
//!   after a short delay (driven by [`Widget::tick`]). A `Scroll` over
//!   the field steps the value.
//! - Typed text commits on `Enter`, on focus loss, and before any
//!   step — an unparseable field restores the formatted value.
//!
//! # Examples
//!
//! ```
//! use martensite::widgets::SpinBox;
//!
//! let mut sb = SpinBox::new().range(0.0, 10.0).step(0.5).suffix(" px");
//! sb.set_value(3.3);
//! assert_eq!(sb.value(), 3.5); // snapped to the 0.5 grid
//! ```

use std::time::Duration;

use accesskit::Node as AccessKitNode;
use glam::Vec2;
use martensite_core::widget::{
    EventContext, EventResponse, LayoutConstraints, LayoutContext, PaintContext, PointerButton,
    SemanticAction, Widget, WidgetEvent,
};
use martensite_core::{NodeFlags, Rect, RenderMinimum, TokenKey, UnderflowPolicy};

use crate::widgets::text_input::TextInput;

/// Stepper column width in logical points.
const STEP_W: f32 = 20.0;
/// Default field size in logical points.
const FIELD_W: f32 = 140.0;
const FIELD_H: f32 = 24.0;
/// Default page jump when none is configured (10% of the range).
const PAGE_FRACTION: f64 = 0.1;
/// Delay before a held step button starts auto-repeating.
const HOLD_DELAY: Duration = Duration::from_millis(450);
/// Interval between auto-repeat steps while a button is held.
const HOLD_RATE: Duration = Duration::from_millis(70);
/// Stepper button face colour.
const STEP_FACE: [u8; 4] = [240, 242, 246, 255];
/// Stepper button face while hovered.
const STEP_HOT: [u8; 4] = [224, 230, 240, 255];
/// Stepper button face while pressed.
const STEP_DOWN: [u8; 4] = [198, 208, 226, 255];
/// Divider between the field and the stepper column.
const DIVIDER: [u8; 4] = [180, 186, 196, 255];
/// Arrow ink colour.
const ARROW: [u8; 4] = [50, 55, 65, 255];
/// Arrow ink while disabled.
const ARROW_DIM: [u8; 4] = [160, 164, 172, 255];

/// Which stepper button a pointer interaction is on.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
enum StepButton {
    /// The upper ▲ button — increments.
    Up,
    /// The lower ▼ button — decrements.
    Down,
}

/// A numeric spin box: an editable field plus ▲/▼ step buttons.
///
/// Values are always clamped to `[min, max]` and snapped to the
/// nearest `step` grid offset from `min`, matching [`Slider`]. The
/// displayed text is `prefix` + the value formatted to `decimals`
/// digits + `suffix`; committed input is parsed with the affixes
/// stripped, so pasting or typing the decorated form round-trips.
///
/// # Examples
///
/// ```
/// use martensite::widgets::SpinBox;
///
/// let mut sb = SpinBox::new().range(0.0, 100.0).suffix(" ms");
/// assert_eq!(sb.value(), 0.0);
/// sb.set_value(250.0);
/// assert_eq!(sb.value(), 100.0); // clamped
/// ```
///
/// [`Slider`]: crate::widgets::Slider
pub struct SpinBox {
    /// Optional accessible label.
    pub label: Option<String>,
    /// Whether the spin box accepts input.
    pub enabled: bool,
    /// Minimum value.
    pub min: f64,
    /// Maximum value.
    pub max: f64,
    /// Arrow-key / button / scroll step.
    pub step: f64,
    /// Page jump (`PageUp`/`PageDown`); defaults to 10% of the range.
    pub page_step: Option<f64>,
    /// Fraction digits shown in the field.
    pub decimals: usize,
    /// Text shown before the number (e.g. `"$"`).
    pub prefix: String,
    /// Text shown after the number (e.g. `" px"`).
    pub suffix: String,
    /// Whether the field accepts typed input (`false` = read-only
    /// display; stepping still works).
    pub editable: bool,
    /// Whether stepping wraps around at the range ends.
    pub wrap: bool,
    /// Current value, always clamped and snapped.
    value: f64,
    /// The embedded text field (internal child).
    input: TextInput,
    /// Field bounds assigned in `layout`.
    input_rect: Rect,
    /// ▲ button bounds assigned in `layout`.
    up_rect: Rect,
    /// ▼ button bounds assigned in `layout`.
    down_rect: Rect,
    /// Step button held by the pointer, if any (drives auto-repeat).
    pressed: Option<StepButton>,
    /// Step button under the pointer, for hover highlighting.
    hover: Option<StepButton>,
    /// Whether the field holds uncommitted typed text — i.e. the
    /// widget has keyboard focus.
    editing: bool,
    /// Time the current step-button press has been held.
    hold_elapsed: Duration,
    /// Accumulator for the auto-repeat interval.
    repeat_accum: Duration,
    /// Cached bounds from the last layout pass.
    cached_bounds: Rect,
    /// Display scale cached from `layout`.
    scale: f32,
}

impl SpinBox {
    /// Creates a spin box over `0.0..=100.0` starting at `0.0`.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite::widgets::SpinBox;
    ///
    /// let sb = SpinBox::new();
    /// assert_eq!(sb.value(), 0.0);
    /// assert_eq!(sb.min, 0.0);
    /// assert_eq!(sb.max, 100.0);
    /// ```
    pub fn new() -> Self {
        let mut sb = Self {
            label: None,
            enabled: true,
            min: 0.0,
            max: 100.0,
            step: 1.0,
            page_step: None,
            decimals: 0,
            prefix: String::new(),
            suffix: String::new(),
            editable: true,
            wrap: false,
            value: 0.0,
            input: TextInput::new("Value"),
            input_rect: Rect::default(),
            up_rect: Rect::default(),
            down_rect: Rect::default(),
            pressed: None,
            hover: None,
            editing: false,
            hold_elapsed: Duration::ZERO,
            repeat_accum: Duration::ZERO,
            cached_bounds: Rect::default(),
            scale: 1.0,
        };
        sb.sync_text();
        sb
    }

    /// Sets the value range (inverted ends are swapped, the value is
    /// re-clamped and re-snapped).
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite::widgets::SpinBox;
    ///
    /// let sb = SpinBox::new().range(10.0, -10.0);
    /// assert_eq!(sb.min, -10.0);
    /// assert_eq!(sb.max, 10.0);
    /// ```
    #[must_use]
    pub fn range(mut self, min: f64, max: f64) -> Self {
        let (min, max) = if min <= max { (min, max) } else { (max, min) };
        self.min = min;
        self.max = max;
        self.set_value(self.value);
        self
    }

    /// Sets the current value (clamped and snapped).
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite::widgets::SpinBox;
    ///
    /// let sb = SpinBox::new().with_value(40.0);
    /// assert_eq!(sb.value(), 40.0);
    /// ```
    #[inline]
    #[must_use]
    pub fn with_value(mut self, value: f64) -> Self {
        self.set_value(value);
        self
    }

    /// Sets the step used by the buttons, arrows, and scroll.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite::widgets::SpinBox;
    ///
    /// let sb = SpinBox::new().range(0.0, 10.0).step(2.0).with_value(3.0);
    /// assert_eq!(sb.value(), 4.0); // snapped to the 2.0 grid
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
    /// use martensite::widgets::SpinBox;
    ///
    /// let sb = SpinBox::new().with_page_step(25.0);
    /// assert_eq!(sb.page_step(), 25.0);
    /// ```
    #[inline]
    #[must_use]
    pub fn with_page_step(mut self, page_step: f64) -> Self {
        self.page_step = Some(page_step.max(0.0));
        self
    }

    /// Sets the fraction digits shown in the field.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite::widgets::SpinBox;
    ///
    /// let sb = SpinBox::new().step(0.5).decimals(2).with_value(1.5);
    /// assert_eq!(sb.text(), "1.50");
    /// ```
    #[inline]
    #[must_use]
    pub fn decimals(mut self, decimals: usize) -> Self {
        self.decimals = decimals;
        self.sync_text();
        self
    }

    /// Sets the text shown before the number.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite::widgets::SpinBox;
    ///
    /// let sb = SpinBox::new().prefix("$").with_value(4.0);
    /// assert_eq!(sb.text(), "$4");
    /// ```
    #[inline]
    #[must_use]
    pub fn prefix(mut self, prefix: impl Into<String>) -> Self {
        self.prefix = prefix.into();
        self.sync_text();
        self
    }

    /// Sets the text shown after the number.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite::widgets::SpinBox;
    ///
    /// let sb = SpinBox::new().suffix(" ms").with_value(25.0);
    /// assert_eq!(sb.text(), "25 ms");
    /// ```
    #[inline]
    #[must_use]
    pub fn suffix(mut self, suffix: impl Into<String>) -> Self {
        self.suffix = suffix.into();
        self.sync_text();
        self
    }

    /// Sets whether the field accepts typed input.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite::widgets::SpinBox;
    ///
    /// let sb = SpinBox::new().editable(false);
    /// assert!(!sb.editable);
    /// ```
    #[inline]
    #[must_use]
    pub fn editable(mut self, editable: bool) -> Self {
        self.editable = editable;
        self
    }

    /// Sets whether stepping wraps around at the range ends.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite::widgets::SpinBox;
    ///
    /// let sb = SpinBox::new().wrap(true);
    /// assert!(sb.wrap);
    /// ```
    #[inline]
    #[must_use]
    pub fn wrap(mut self, wrap: bool) -> Self {
        self.wrap = wrap;
        self
    }

    /// Sets whether the spin box is enabled.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite::widgets::SpinBox;
    ///
    /// let sb = SpinBox::new().enabled(false);
    /// assert!(!sb.enabled);
    /// ```
    #[inline]
    #[must_use]
    pub fn enabled(mut self, enabled: bool) -> Self {
        self.enabled = enabled;
        self
    }

    /// Sets the accessible label.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite::widgets::SpinBox;
    ///
    /// let sb = SpinBox::new().label("Quantity");
    /// assert_eq!(sb.label.as_deref(), Some("Quantity"));
    /// ```
    #[inline]
    #[must_use]
    pub fn label(mut self, label: impl Into<String>) -> Self {
        self.label = Some(label.into());
        self
    }

    /// Shares a [`crate::text_paint::TextPainter`] with the embedded
    /// field so its value text emits real glyph runs.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite::text_paint::shared_painter;
    /// use martensite::widgets::SpinBox;
    ///
    /// let sb = SpinBox::new().with_text_painter(shared_painter());
    /// ```
    #[must_use]
    pub fn with_text_painter(mut self, painter: crate::text_paint::SharedTextPainter) -> Self {
        self.input = self.input.clone().with_text_painter(painter);
        self
    }

    /// The current value.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite::widgets::SpinBox;
    ///
    /// let sb = SpinBox::new().with_value(3.0);
    /// assert_eq!(sb.value(), 3.0);
    /// ```
    #[inline]
    pub fn value(&self) -> f64 {
        self.value
    }

    /// Sets the value, clamping to `[min, max]` and snapping to the
    /// `step` grid measured from `min`. The displayed text refreshes
    /// unless the field is being edited.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite::widgets::SpinBox;
    ///
    /// let mut sb = SpinBox::new().range(0.0, 10.0).step(0.25);
    /// sb.set_value(99.0);
    /// assert_eq!(sb.value(), 10.0); // clamped
    /// sb.set_value(4.13);
    /// assert_eq!(sb.value(), 4.25); // snapped
    /// ```
    pub fn set_value(&mut self, value: f64) {
        self.value = self.snap(value);
        if !self.editing {
            self.sync_text();
        }
    }

    /// The effective `PageUp`/`PageDown` jump (explicit `page_step` or
    /// 10% of the range, at least one step).
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite::widgets::SpinBox;
    ///
    /// assert_eq!(SpinBox::new().page_step(), 10.0);
    /// assert_eq!(SpinBox::new().with_page_step(5.0).page_step(), 5.0);
    /// ```
    pub fn page_step(&self) -> f64 {
        self.page_step
            .unwrap_or((self.max - self.min) * PAGE_FRACTION)
            .max(self.step)
    }

    /// The text currently shown in the field — the formatted value,
    /// or uncommitted input while the field is being edited.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite::widgets::SpinBox;
    ///
    /// let sb = SpinBox::new().with_value(7.0).suffix(" px");
    /// assert_eq!(sb.text(), "7 px");
    /// ```
    #[inline]
    pub fn text(&self) -> &str {
        &self.input.value
    }

    /// Whether the widget currently holds keyboard focus (the field
    /// may contain uncommitted text).
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite::widgets::SpinBox;
    ///
    /// assert!(!SpinBox::new().is_editing());
    /// ```
    #[inline]
    pub fn is_editing(&self) -> bool {
        self.editing
    }

    /// Whether a step button is currently held.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite::widgets::SpinBox;
    ///
    /// assert!(!SpinBox::new().is_pressed());
    /// ```
    #[inline]
    pub fn is_pressed(&self) -> bool {
        self.pressed.is_some()
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

    /// The decorated field text for `value`.
    fn formatted(&self) -> String {
        let mut s = self.prefix.clone();
        s.push_str(&format!("{:.prec$}", self.value, prec = self.decimals));
        s.push_str(&self.suffix);
        s
    }

    /// Parses the field text with the affixes stripped.
    fn parse_text(&self) -> Option<f64> {
        let mut t = self.input.value.trim();
        if !self.prefix.is_empty() {
            t = t.strip_prefix(self.prefix.as_str()).unwrap_or(t).trim_start();
        }
        if !self.suffix.is_empty() {
            t = t.strip_suffix(self.suffix.as_str()).unwrap_or(t).trim_end();
        }
        t.parse::<f64>().ok()
    }

    /// Rewrites the field text from `value`.
    fn sync_text(&mut self) {
        self.input.set_value(self.formatted());
    }

    /// Commits the typed text: a parseable field becomes the value;
    /// anything else restores the formatted value.
    fn commit_text(&mut self) {
        if let Some(v) = self.parse_text() {
            self.value = self.snap(v);
        }
        self.sync_text();
    }

    /// Steps the value by `delta`, wrapping to the far end at the
    /// bounds when `wrap` is set.
    fn nudge(&mut self, delta: f64) {
        let mut next = self.value + delta;
        if self.wrap {
            if next > self.max {
                next = self.min;
            } else if next < self.min {
                next = self.max;
            }
        }
        self.value = self.snap(next);
        self.sync_text();
    }

    /// Commits any pending text, applies `delta`, and repaints — the
    /// shared response for arrow/page keys and semantic steps.
    fn step_and_refresh(&mut self, delta: f64) -> EventResponse {
        self.commit_text();
        self.nudge(delta);
        EventResponse::RequestRepaint
    }

    /// Mirrors spin-box state onto the embedded field.
    fn sync_input(&mut self) {
        self.input.enabled = self.enabled;
        self.input.read_only = !self.editable;
        if let Some(ref label) = self.label {
            self.input.label.clone_from(label);
        }
    }

    /// Forwards an event to the embedded field the way the default
    /// `Widget::event` child walk would: positional events only inside
    /// the field's bounds, except moves and releases which always
    /// reach it so a captured drag keeps tracking outside.
    fn forward(&mut self, cx: &mut EventContext) -> EventResponse {
        if let Some(pos) = cx.event.position() {
            let drag = matches!(
                cx.event,
                WidgetEvent::PointerMoved { .. } | WidgetEvent::PointerReleased { .. }
            );
            if !drag && !self.input_rect.contains(pos) {
                return EventResponse::Ignored;
            }
        }
        let mut child_cx = EventContext {
            event: cx.event,
            bounds: self.input_rect,
            scale: cx.scale,
        };
        self.input.event(&mut child_cx)
    }
}

impl Default for SpinBox {
    fn default() -> Self {
        Self::new()
    }
}

impl Widget for SpinBox {
    fn measure(&mut self, cx: &mut LayoutContext, constraints: LayoutConstraints) -> Vec2 {
        Vec2::new(
            cx.pt(FIELD_W).min(constraints.max_size.x.max(0.0)),
            cx.pt(FIELD_H).min(constraints.max_size.y.max(0.0)),
        )
    }

    fn min_render(&self) -> RenderMinimum {
        // The same floor `measure` requests — a shorter box cannot fit
        // the stepper column legibly.
        RenderMinimum::new(Vec2::new(FIELD_W, FIELD_H)).with_policy(UnderflowPolicy::Lint)
    }

    fn layout(&mut self, cx: &mut LayoutContext, bounds: Rect) {
        self.cached_bounds = bounds;
        self.scale = cx.scale;
        // Declare keyboard focusability on the arena node so the
        // `FocusManager` accepts focus requests and press-to-focus
        // applies; key/IME input then forwards into the field child.
        if self.enabled {
            cx.hot.flags |= NodeFlags::FOCUSABLE;
        } else {
            cx.hot.flags.remove(NodeFlags::FOCUSABLE);
        }
        self.sync_input();
        let sw = cx.pt(STEP_W).min(bounds.width());
        let field_w = (bounds.width() - sw).max(0.0);
        self.input_rect = Rect::new(bounds.min_x(), bounds.min_y(), field_w, bounds.height());
        let bx = bounds.min_x() + field_w;
        let half = bounds.height() / 2.0;
        self.up_rect = Rect::new(bx, bounds.min_y(), sw, half);
        self.down_rect = Rect::new(bx, bounds.min_y() + half, sw, bounds.height() - half);
        cx.layout_child(&mut self.input, self.input_rect);
    }

    fn accessibility(&self, node: &mut AccessKitNode) {
        node.set_role(accesskit::Role::SpinButton);
        node.set_numeric_value(self.value);
        node.set_min_numeric_value(self.min);
        node.set_max_numeric_value(self.max);
        node.set_numeric_value_step(self.step);
        node.set_numeric_value_jump(self.page_step());
        node.set_value(self.formatted());
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
        if !self.editable {
            node.set_read_only();
        }
    }

    fn a11y_prepare(&mut self) {
        // A `SetValue` delivered to the embedded field's virtual node
        // writes its text directly — pull the write into the numeric
        // value before the tree is emitted.
        if !self.editing && self.input.value != self.formatted() {
            self.commit_text();
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
                let button = if self.up_rect.contains(*position) {
                    Some((StepButton::Up, self.step))
                } else if self.down_rect.contains(*position) {
                    Some((StepButton::Down, -self.step))
                } else {
                    None
                };
                if let Some((button, delta)) = button {
                    self.commit_text();
                    self.nudge(delta);
                    self.pressed = Some(button);
                    self.hold_elapsed = Duration::ZERO;
                    self.repeat_accum = Duration::ZERO;
                    return EventResponse::CapturePointer;
                }
                self.forward(cx)
            }
            WidgetEvent::PointerMoved { position } => {
                if self.pressed.is_some() {
                    return EventResponse::Ignored;
                }
                let hover = if self.up_rect.contains(*position) {
                    Some(StepButton::Up)
                } else if self.down_rect.contains(*position) {
                    Some(StepButton::Down)
                } else {
                    None
                };
                let changed = hover != self.hover;
                self.hover = hover;
                let forwarded = self.forward(cx);
                if changed {
                    EventResponse::RequestRepaint
                } else {
                    forwarded
                }
            }
            WidgetEvent::PointerReleased {
                button: PointerButton::Primary,
                ..
            } => {
                if self.pressed.is_some() {
                    self.pressed = None;
                    return EventResponse::ReleasePointer;
                }
                self.forward(cx)
            }
            WidgetEvent::PointerLeave => {
                if self.hover.take().is_some() {
                    EventResponse::RequestRepaint
                } else {
                    EventResponse::Ignored
                }
            }
            WidgetEvent::Scroll { delta, .. } => {
                if delta.y.abs() < f32::EPSILON {
                    return EventResponse::Ignored;
                }
                // Wheel up (positive delta) increments — the standard
                // spin-field mapping.
                self.commit_text();
                self.nudge(self.step * f64::from(delta.y.signum()));
                EventResponse::RequestRepaint
            }
            WidgetEvent::KeyPressed { key, .. } => match key.as_str() {
                "ArrowUp" => self.step_and_refresh(self.step),
                "ArrowDown" => self.step_and_refresh(-self.step),
                "PageUp" => self.step_and_refresh(self.page_step()),
                "PageDown" => self.step_and_refresh(-self.page_step()),
                "Home" => {
                    self.commit_text();
                    self.value = self.snap(self.min);
                    self.sync_text();
                    EventResponse::RequestRepaint
                }
                "End" => {
                    self.commit_text();
                    self.value = self.snap(self.max);
                    self.sync_text();
                    EventResponse::RequestRepaint
                }
                "Enter" => {
                    self.commit_text();
                    EventResponse::RequestRepaint
                }
                _ => self.forward(cx),
            },
            WidgetEvent::FocusGained => {
                self.editing = true;
                self.forward(cx)
            }
            WidgetEvent::FocusLost => {
                // Leaving the field commits whatever was typed.
                self.commit_text();
                self.editing = false;
                self.pressed = None;
                self.forward(cx)
            }
            WidgetEvent::SemanticAction(action) => match action {
                SemanticAction::SetValue(text) => {
                    if let Ok(v) = text.parse::<f64>() {
                        self.value = self.snap(v);
                        self.sync_text();
                    }
                    EventResponse::RequestRepaint
                }
                SemanticAction::Increment => self.step_and_refresh(self.step),
                SemanticAction::Decrement => self.step_and_refresh(-self.step),
                SemanticAction::Focus => EventResponse::CaptureFocus,
                _ => EventResponse::Ignored,
            },
            _ => self.forward(cx),
        }
    }

    fn tick(&mut self, dt: Duration) -> bool {
        let Some(button) = self.pressed else {
            return false;
        };
        self.hold_elapsed += dt;
        if self.hold_elapsed < HOLD_DELAY {
            return false;
        }
        self.repeat_accum += dt;
        let mut fired = false;
        while self.repeat_accum >= HOLD_RATE {
            self.repeat_accum -= HOLD_RATE;
            let delta = match button {
                StepButton::Up => self.step,
                StepButton::Down => -self.step,
            };
            self.nudge(delta);
            fired = true;
        }
        fired
    }

    fn paint(&self, cx: &mut PaintContext) {
        // The embedded field paints its own face/border/text via the
        // internal-child walk; the chrome here is the stepper column.
        for (rect, button) in [
            (self.up_rect, StepButton::Up),
            (self.down_rect, StepButton::Down),
        ] {
            let krect = kurbo::Rect::new(
                f64::from(rect.min_x()),
                f64::from(rect.min_y()),
                f64::from(rect.max_x()),
                f64::from(rect.max_y()),
            );
            let face = if !self.enabled {
                STEP_FACE
            } else if self.pressed == Some(button) {
                STEP_DOWN
            } else if self.hover == Some(button) {
                STEP_HOT
            } else {
                STEP_FACE
            };
            cx.list
                .push_fill_rect(krect, cx.color(TokenKey::SurfaceColor, face));

            // Arrow: a filled triangle centred in the half-button.
            let cxm = (krect.x0 + krect.x1) / 2.0;
            let cy = (krect.y0 + krect.y1) / 2.0;
            let s = cx.ptf(4.5);
            let h = cx.ptf(3.5);
            let mut tri = kurbo::BezPath::new();
            match button {
                StepButton::Up => {
                    tri.move_to((cxm - s, cy + h));
                    tri.line_to((cxm, cy - h));
                    tri.line_to((cxm + s, cy + h));
                }
                StepButton::Down => {
                    tri.move_to((cxm - s, cy - h));
                    tri.line_to((cxm, cy + h));
                    tri.line_to((cxm + s, cy - h));
                }
            }
            tri.close_path();
            cx.list.push_path(
                tri,
                if self.enabled {
                    cx.color(TokenKey::TextColor, ARROW)
                } else {
                    cx.color(TokenKey::TextMutedColor, ARROW_DIM)
                },
            );
        }

        // Dividers: between the field and the column, and between the
        // two half-buttons.
        let x = f64::from(self.up_rect.min_x());
        let mut vline = kurbo::BezPath::new();
        vline.move_to((x, f64::from(self.up_rect.min_y())));
        vline.line_to((x, f64::from(self.down_rect.max_y())));
        cx.list
            .push_stroke_path(vline, cx.pt(1.0), cx.color(TokenKey::DividerColor, DIVIDER));
        let y = f64::from(self.down_rect.min_y());
        let mut hline = kurbo::BezPath::new();
        hline.move_to((x, y));
        hline.line_to((f64::from(self.up_rect.max_x()), y));
        cx.list
            .push_stroke_path(hline, cx.pt(1.0), cx.color(TokenKey::DividerColor, DIVIDER));
    }

    fn child_count(&self) -> usize {
        1
    }

    fn child(&self, index: usize) -> Option<&dyn Widget> {
        (index == 0).then_some(&self.input as &dyn Widget)
    }

    fn child_mut(&mut self, index: usize) -> Option<&mut dyn Widget> {
        (index == 0).then_some(&mut self.input as &mut dyn Widget)
    }

    fn child_bounds(&self, index: usize) -> Option<Rect> {
        (index == 0).then_some(self.input_rect)
    }
}

impl std::fmt::Debug for SpinBox {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("SpinBox")
            .field("value", &self.value)
            .field("min", &self.min)
            .field("max", &self.max)
            .field("step", &self.step)
            .field("decimals", &self.decimals)
            .field("enabled", &self.enabled)
            .field("editable", &self.editable)
            .finish()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use martensite_core::HotNode;

    fn laid_out(sb: &mut SpinBox, w: f32, h: f32) {
        let mut hot = HotNode::default();
        let mut cx = LayoutContext {
            hot: &mut hot,
            scale: 1.0,
        };
        sb.layout(&mut cx, Rect::new(0.0, 0.0, w, h));
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

    fn event(sb: &mut SpinBox, ev: &WidgetEvent) -> EventResponse {
        let mut cx = EventContext {
            event: ev,
            bounds: sb.cached_bounds,
            scale: 1.0,
        };
        sb.event(&mut cx)
    }

    #[test]
    fn new_defaults() {
        let sb = SpinBox::new();
        assert_eq!(sb.value(), 0.0);
        assert_eq!(sb.text(), "0");
        assert!(sb.enabled && sb.editable && !sb.wrap);
    }

    #[test]
    fn range_swaps_inverted() {
        let sb = SpinBox::new().range(10.0, -10.0);
        assert_eq!(sb.min, -10.0);
        assert_eq!(sb.max, 10.0);
    }

    #[test]
    fn set_value_clamps_and_snaps() {
        let mut sb = SpinBox::new().range(0.0, 10.0).step(0.5).decimals(1);
        sb.set_value(7.3);
        assert_eq!(sb.value(), 7.5);
        assert_eq!(sb.text(), "7.5");
        sb.set_value(-5.0);
        assert_eq!(sb.value(), 0.0);
        sb.set_value(50.0);
        assert_eq!(sb.value(), 10.0);
    }

    #[test]
    fn arrow_and_page_keys_step() {
        let mut sb = SpinBox::new()
            .range(0.0, 100.0)
            .with_value(50.0)
            .with_page_step(20.0);
        laid_out(&mut sb, 140.0, 24.0);
        event(&mut sb, &key("ArrowUp"));
        assert_eq!(sb.value(), 51.0);
        event(&mut sb, &key("ArrowDown"));
        assert_eq!(sb.value(), 50.0);
        event(&mut sb, &key("PageUp"));
        assert_eq!(sb.value(), 70.0);
        event(&mut sb, &key("PageDown"));
        assert_eq!(sb.value(), 50.0);
    }

    #[test]
    fn home_end_jump_to_ends() {
        let mut sb = SpinBox::new().range(0.0, 10.0).with_value(5.0);
        laid_out(&mut sb, 140.0, 24.0);
        event(&mut sb, &key("End"));
        assert_eq!(sb.value(), 10.0);
        event(&mut sb, &key("Home"));
        assert_eq!(sb.value(), 0.0);
    }

    #[test]
    fn wrap_cycles_at_bounds() {
        let mut sb = SpinBox::new().range(0.0, 10.0).with_value(10.0).wrap(true);
        laid_out(&mut sb, 140.0, 24.0);
        event(&mut sb, &key("ArrowUp"));
        assert_eq!(sb.value(), 0.0);
        event(&mut sb, &key("ArrowDown"));
        assert_eq!(sb.value(), 10.0);
        // Without wrap it stays clamped.
        let mut sb = SpinBox::new().range(0.0, 10.0).with_value(10.0);
        laid_out(&mut sb, 140.0, 24.0);
        event(&mut sb, &key("ArrowUp"));
        assert_eq!(sb.value(), 10.0);
    }

    #[test]
    fn scroll_steps_by_sign() {
        let mut sb = SpinBox::new().range(0.0, 10.0).with_value(5.0);
        laid_out(&mut sb, 140.0, 24.0);
        let up = WidgetEvent::Scroll {
            position: Vec2::new(10.0, 12.0),
            delta: Vec2::new(0.0, 30.0),
        };
        event(&mut sb, &up);
        assert_eq!(sb.value(), 6.0);
        let down = WidgetEvent::Scroll {
            position: Vec2::new(10.0, 12.0),
            delta: Vec2::new(0.0, -30.0),
        };
        event(&mut sb, &down);
        assert_eq!(sb.value(), 5.0);
    }

    #[test]
    fn button_press_steps_and_captures() {
        let mut sb = SpinBox::new().range(0.0, 10.0).with_value(5.0);
        laid_out(&mut sb, 140.0, 24.0);
        // Stepper column is the right 20px; ▲ occupies the top half.
        assert_eq!(
            event(&mut sb, &press(130.0, 6.0)),
            EventResponse::CapturePointer
        );
        assert_eq!(sb.value(), 6.0);
        assert!(sb.is_pressed());
        let release = WidgetEvent::PointerReleased {
            position: Vec2::new(130.0, 6.0),
            button: PointerButton::Primary,
        };
        assert_eq!(event(&mut sb, &release), EventResponse::ReleasePointer);
        // ▼ occupies the bottom half.
        assert_eq!(
            event(&mut sb, &press(130.0, 18.0)),
            EventResponse::CapturePointer
        );
        assert_eq!(sb.value(), 5.0);
    }

    #[test]
    fn hold_repeat_fires_after_delay() {
        let mut sb = SpinBox::new().range(0.0, 100.0).with_value(50.0);
        laid_out(&mut sb, 140.0, 24.0);
        event(&mut sb, &press(130.0, 6.0));
        assert_eq!(sb.value(), 51.0);
        // Before the delay no repeat fires.
        assert!(!sb.tick(Duration::from_millis(100)));
        // Cross the delay — subsequent ticks fire per interval.
        assert!(sb.tick(Duration::from_millis(400)));
        assert_eq!(sb.value(), 56.0); // 51 + 5 repeats in the surplus
        assert!(sb.tick(Duration::from_millis(70)));
        assert_eq!(sb.value(), 57.0);
    }

    #[test]
    fn typed_text_commits_on_enter() {
        let mut sb = SpinBox::new().range(0.0, 100.0);
        laid_out(&mut sb, 140.0, 24.0);
        event(&mut sb, &WidgetEvent::FocusGained);
        assert!(sb.is_editing());
        // Select-all then type replaces the formatted value.
        event(&mut sb, &key("SelectAll"));
        event(&mut sb, &WidgetEvent::ImeCommitted { text: "42".into() });
        assert_eq!(sb.value(), 0.0); // not committed yet
        event(&mut sb, &key("Enter"));
        assert_eq!(sb.value(), 42.0);
        assert_eq!(sb.text(), "42");
    }

    #[test]
    fn typed_text_commits_on_blur() {
        let mut sb = SpinBox::new().range(0.0, 100.0);
        laid_out(&mut sb, 140.0, 24.0);
        event(&mut sb, &WidgetEvent::FocusGained);
        event(&mut sb, &key("SelectAll"));
        event(&mut sb, &WidgetEvent::ImeCommitted { text: "7".into() });
        event(&mut sb, &WidgetEvent::FocusLost);
        assert_eq!(sb.value(), 7.0);
        assert!(!sb.is_editing());
    }

    #[test]
    fn unparseable_text_restores_value() {
        let mut sb = SpinBox::new().range(0.0, 100.0).with_value(9.0);
        laid_out(&mut sb, 140.0, 24.0);
        event(&mut sb, &WidgetEvent::FocusGained);
        event(&mut sb, &key("SelectAll"));
        event(&mut sb, &WidgetEvent::ImeCommitted { text: "abc".into() });
        event(&mut sb, &key("Enter"));
        assert_eq!(sb.value(), 9.0);
        assert_eq!(sb.text(), "9");
    }

    #[test]
    fn affixes_parse_on_commit() {
        let mut sb = SpinBox::new()
            .range(0.0, 1000.0)
            .step(0.5)
            .decimals(1)
            .prefix("$")
            .suffix(" ms");
        laid_out(&mut sb, 140.0, 24.0);
        assert_eq!(sb.text(), "$0.0 ms");
        event(&mut sb, &WidgetEvent::FocusGained);
        event(&mut sb, &key("SelectAll"));
        event(&mut sb, &WidgetEvent::ImeCommitted {
            text: "$12.5 ms".into(),
        });
        event(&mut sb, &key("Enter"));
        assert_eq!(sb.value(), 12.5);
        assert_eq!(sb.text(), "$12.5 ms");
    }

    #[test]
    fn read_only_blocks_typing_but_steps() {
        let mut sb = SpinBox::new().range(0.0, 10.0).editable(false);
        laid_out(&mut sb, 140.0, 24.0);
        event(&mut sb, &WidgetEvent::FocusGained);
        event(&mut sb, &WidgetEvent::ImeCommitted { text: "9".into() });
        event(&mut sb, &key("Enter"));
        assert_eq!(sb.value(), 0.0);
        event(&mut sb, &key("ArrowUp"));
        assert_eq!(sb.value(), 1.0);
    }

    #[test]
    fn step_commits_pending_text() {
        let mut sb = SpinBox::new().range(0.0, 100.0);
        laid_out(&mut sb, 140.0, 24.0);
        event(&mut sb, &WidgetEvent::FocusGained);
        event(&mut sb, &key("SelectAll"));
        event(&mut sb, &WidgetEvent::ImeCommitted { text: "30".into() });
        event(&mut sb, &key("ArrowUp"));
        assert_eq!(sb.value(), 31.0);
        assert_eq!(sb.text(), "31");
    }

    #[test]
    fn semantic_actions_step_and_set() {
        let mut sb = SpinBox::new().range(0.0, 10.0).with_value(5.0);
        laid_out(&mut sb, 140.0, 24.0);
        event(&mut sb, &WidgetEvent::SemanticAction(SemanticAction::Increment));
        assert_eq!(sb.value(), 6.0);
        event(&mut sb, &WidgetEvent::SemanticAction(SemanticAction::Decrement));
        assert_eq!(sb.value(), 5.0);
        event(
            &mut sb,
            &WidgetEvent::SemanticAction(SemanticAction::SetValue("8.0".into())),
        );
        assert_eq!(sb.value(), 8.0);
        assert_eq!(sb.text(), "8");
    }

    #[test]
    fn accessibility_contract() {
        let sb = SpinBox::new()
            .range(0.0, 100.0)
            .with_value(40.0)
            .step(5.0)
            .with_page_step(20.0)
            .suffix(" px")
            .label("Width");
        let mut node = AccessKitNode::new(accesskit::Role::Unknown);
        sb.accessibility(&mut node);
        assert_eq!(node.role(), accesskit::Role::SpinButton);
        assert_eq!(node.numeric_value(), Some(40.0));
        assert_eq!(node.min_numeric_value(), Some(0.0));
        assert_eq!(node.max_numeric_value(), Some(100.0));
        assert_eq!(node.numeric_value_step(), Some(5.0));
        assert_eq!(node.numeric_value_jump(), Some(20.0));
        assert_eq!(node.value(), Some("40 px"));
        assert_eq!(node.label(), Some("Width"));
        assert!(node.supports_action(accesskit::Action::SetValue));
        assert!(node.supports_action(accesskit::Action::Increment));
        assert!(node.supports_action(accesskit::Action::Decrement));
    }

    #[test]
    fn disabled_ignores_input() {
        let mut sb = SpinBox::new().range(0.0, 10.0).enabled(false);
        laid_out(&mut sb, 140.0, 24.0);
        assert_eq!(event(&mut sb, &press(130.0, 6.0)), EventResponse::Ignored);
        assert_eq!(event(&mut sb, &key("ArrowUp")), EventResponse::Ignored);
        assert_eq!(sb.value(), 0.0);
    }

    #[test]
    fn field_is_internal_child() {
        let mut sb = SpinBox::new();
        laid_out(&mut sb, 140.0, 24.0);
        assert_eq!(sb.child_count(), 1);
        assert_eq!(
            sb.child_bounds(0),
            Some(Rect::new(0.0, 0.0, 120.0, 24.0))
        );
        // A press inside the field reaches the TextInput.
        assert_eq!(
            event(&mut sb, &press(10.0, 12.0)),
            EventResponse::CapturePointer
        );
    }
}
