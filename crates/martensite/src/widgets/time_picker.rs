//! `TimePicker` widget: a segmented time-of-day field
//! (`QTimeEdit` / WinUI `TimePicker`).
//!
//! Unlike the popup pickers, editing is **inline on the face**: the
//! field shows `HH:MM` (or `H:MM AM` in 12-hour mode) split into
//! hour / minute / AM-PM segments, and the focused segment is
//! highlighted behind its digits.
//!
//! - **Pointer**: pressing a segment focuses it; pressing anywhere on
//!   the face requests keyboard focus.
//! - **Keyboard**: `ArrowUp`/`ArrowDown` increment/decrement the
//!   focused segment (wrapping — minutes step by
//!   [`TimePicker::minute_step`]), `ArrowLeft`/`ArrowRight` move
//!   segment focus, digits type into the focused segment with the
//!   usual two-digit rollover (`"1"` then `"2"` → `12` and
//!   auto-advance), `a`/`p` set AM/PM in 12-hour mode, `Backspace`
//!   clears the pending digit.
//! - **Edits**: every user change queues the new value for
//!   [`TimePicker::take_edited`]; [`TimePicker::set_time`] is
//!   programmatic and does not feed that seam.
//!
//! Accessibility emits `Role::TimeInput` (the `input[type=time]`
//! role) rather than `Role::SpinButton`: the value is a structured
//! time, not a scalar — `Increment`/`Decrement`/`SetValue` actions and
//! the `HH:MM` value text still give AT full stepper control.
//!
//! # Examples
//!
//! ```
//! use martensite::widgets::{Time, TimePicker};
//!
//! let tp = TimePicker::new()
//!     .time(Time { hour: 9, minute: 30 })
//!     .use_24h(true)
//!     .minute_step(5);
//! assert_eq!(tp.text(), "09:30");
//! ```

use accesskit::Node as AccessKitNode;
use glam::Vec2;
use martensite_core::shape::Shape;
use martensite_core::widget::{
    EventContext, EventResponse, LayoutConstraints, LayoutContext, PaintContext, PointerButton,
    SemanticAction, Widget, WidgetEvent,
};
use martensite_core::{NodeFlags, Rect, RenderMinimum, TokenKey, UnderflowPolicy};

/// Face height (logical points).
const FACE_H: f32 = 26.0;
/// Face widths (logical points) for 24h / 12h modes.
const FACE_W_24: f32 = 96.0;
const FACE_W_12: f32 = 128.0;
/// Face background.
const FACE_BG: [u8; 4] = [250, 250, 252, 255];
/// Face border.
const FACE_BORDER: [u8; 4] = [150, 155, 165, 255];
/// Segment ink.
const INK: [u8; 4] = [30, 30, 36, 255];
/// Disabled ink.
const INK_MUTED: [u8; 4] = [150, 150, 158, 255];
/// Focused-segment highlight wash.
const FOCUS_BG: [u8; 4] = [60, 110, 220, 255];
/// Focused-segment ink.
const FOCUS_INK: [u8; 4] = [255, 255, 255, 255];
/// Inner horizontal padding of the field (logical points).
const INNER_PAD: f32 = 8.0;

/// A time of day: `hour` is `0..=23`, `minute` `0..=59`.
///
/// # Examples
///
/// ```
/// use martensite::widgets::Time;
///
/// assert!(Time { hour: 23, minute: 59 }.is_valid());
/// assert!(!Time { hour: 24, minute: 0 }.is_valid());
/// ```
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Time {
    /// Hour, `0..=23` (24-hour clock; the widget maps to 12-hour
    /// display when `use_24h(false)`).
    pub hour: u32,
    /// Minute, `0..=59`.
    pub minute: u32,
}

impl Time {
    /// Whether the fields form a real time of day.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite::widgets::Time;
    ///
    /// assert!(Time { hour: 0, minute: 0 }.is_valid());
    /// assert!(!Time { hour: 12, minute: 60 }.is_valid());
    /// ```
    pub fn is_valid(&self) -> bool {
        self.hour <= 23 && self.minute <= 59
    }
}

/// Which segment of the face holds focus.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Segment {
    /// The hour digits.
    Hour,
    /// The minute digits.
    Minute,
    /// The AM/PM marker (12-hour mode only).
    AmPm,
}

/// A segmented time-of-day field (`QTimeEdit` / WinUI `TimePicker`).
///
/// The value is always a valid [`Time`]: [`set_time`](Self::set_time)
/// clamps into `0..=23`/`0..=59` and snaps the minute to the
/// [`minute_step`](Self::minute_step) grid.
///
/// # Examples
///
/// ```
/// use martensite::widgets::{Time, TimePicker};
///
/// let mut tp = TimePicker::new().time(Time { hour: 8, minute: 15 });
/// assert_eq!(tp.get_time(), Time { hour: 8, minute: 15 });
/// tp.set_time(Time { hour: 25, minute: 70 }); // clamps
/// assert_eq!(tp.get_time(), Time { hour: 23, minute: 59 });
/// ```
pub struct TimePicker {
    /// Optional accessible label.
    pub label: Option<String>,
    /// Whether the field accepts input.
    pub enabled: bool,
    /// `true` displays `0..=23` hours; `false` displays `1..=12` plus
    /// an AM/PM segment.
    pub use_24h: bool,
    /// Minute granularity for arrow stepping and value snapping
    /// (clamped to `1..=30`).
    pub minute_step: u32,
    /// The committed value.
    value: Time,
    /// Focused segment.
    focused: Segment,
    /// First digit of an in-progress two-digit segment entry.
    pending_digit: Option<u8>,
    /// One-shot edit awaiting [`TimePicker::take_edited`].
    edited_pending: Option<Time>,
    /// Segment rects `[hour, minute, ampm]` from the last layout pass
    /// (`ampm` is empty in 24-hour mode).
    segment_rects: [Rect; 3],
    /// Face bounds from the last layout pass.
    cached_bounds: Rect,
    /// Shared shaped-text painter.
    text_painter: Option<crate::text_paint::SharedTextPainter>,
}

impl TimePicker {
    /// A 24-hour field starting at `00:00` with minute step `1`.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite::widgets::TimePicker;
    ///
    /// let tp = TimePicker::new();
    /// assert_eq!(tp.text(), "00:00");
    /// assert!(tp.use_24h);
    /// ```
    pub fn new() -> Self {
        Self {
            label: None,
            enabled: true,
            use_24h: true,
            minute_step: 1,
            value: Time::default(),
            focused: Segment::Hour,
            pending_digit: None,
            edited_pending: None,
            segment_rects: [Rect::default(); 3],
            cached_bounds: Rect::default(),
            text_painter: None,
        }
    }

    /// Sets the initial value (clamped and snapped).
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite::widgets::{Time, TimePicker};
    ///
    /// let tp = TimePicker::new().time(Time { hour: 14, minute: 30 });
    /// assert_eq!(tp.text(), "14:30");
    /// ```
    #[must_use]
    pub fn time(mut self, time: Time) -> Self {
        self.set_time(time);
        self
    }

    /// Sets 12- vs 24-hour display.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite::widgets::{Time, TimePicker};
    ///
    /// let tp = TimePicker::new()
    ///     .time(Time { hour: 14, minute: 30 })
    ///     .use_24h(false);
    /// assert_eq!(tp.text(), "02:30 PM");
    /// ```
    #[must_use]
    pub fn use_24h(mut self, use_24h: bool) -> Self {
        self.use_24h = use_24h;
        // A stale AM/PM focus is impossible in 24h mode.
        if use_24h && self.focused == Segment::AmPm {
            self.focused = Segment::Minute;
        }
        self
    }

    /// Sets the minute granularity (clamped to `1..=30`; the value is
    /// re-snapped).
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite::widgets::{Time, TimePicker};
    ///
    /// let tp = TimePicker::new()
    ///     .minute_step(15)
    ///     .time(Time { hour: 9, minute: 20 });
    /// assert_eq!(tp.get_time().minute, 15); // snapped to the 15 grid
    /// ```
    #[must_use]
    pub fn minute_step(mut self, step: u32) -> Self {
        self.minute_step = step.clamp(1, 30);
        self.value.minute = self.snap_minute(self.value.minute);
        self
    }

    /// Sets the accessible label.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite::widgets::TimePicker;
    ///
    /// let tp = TimePicker::new().label("Start time");
    /// assert_eq!(tp.label.as_deref(), Some("Start time"));
    /// ```
    #[must_use]
    pub fn label(mut self, label: impl Into<String>) -> Self {
        self.label = Some(label.into());
        self
    }

    /// Sets whether the field is enabled.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite::widgets::TimePicker;
    ///
    /// let tp = TimePicker::new().enabled(false);
    /// assert!(!tp.enabled);
    /// ```
    #[must_use]
    pub fn enabled(mut self, enabled: bool) -> Self {
        self.enabled = enabled;
        self
    }

    /// Shares a [`crate::text_paint::TextPainter`] so `paint` emits
    /// real glyph runs instead of `DrawText` placeholder boxes.
    #[must_use]
    pub fn with_text_painter(mut self, painter: crate::text_paint::SharedTextPainter) -> Self {
        self.text_painter = Some(painter);
        self
    }

    /// The current value.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite::widgets::{Time, TimePicker};
    ///
    /// let tp = TimePicker::new().time(Time { hour: 6, minute: 45 });
    /// assert_eq!(tp.get_time(), Time { hour: 6, minute: 45 });
    /// ```
    #[inline]
    pub fn get_time(&self) -> Time {
        self.value
    }

    /// Sets the value programmatically — clamps `hour` to `0..=23`
    /// and `minute` to `0..=59`, then snaps the minute to the
    /// `minute_step` grid. Does not report through
    /// [`take_edited`](Self::take_edited).
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite::widgets::{Time, TimePicker};
    ///
    /// let mut tp = TimePicker::new().minute_step(5);
    /// tp.set_time(Time { hour: 10, minute: 8 });
    /// assert_eq!(tp.get_time(), Time { hour: 10, minute: 10 });
    /// ```
    pub fn set_time(&mut self, time: Time) {
        self.value = Time {
            hour: time.hour.min(23),
            minute: self.snap_minute(time.minute.min(59)),
        };
        self.pending_digit = None;
    }

    /// The field text — `"HH:MM"` or `"HH:MM AM"` in 12-hour mode.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite::widgets::{Time, TimePicker};
    ///
    /// let tp = TimePicker::new().time(Time { hour: 0, minute: 5 }).use_24h(false);
    /// assert_eq!(tp.text(), "12:05 AM");
    /// ```
    pub fn text(&self) -> String {
        if self.use_24h {
            format!("{:02}:{:02}", self.value.hour, self.value.minute)
        } else {
            format!(
                "{:02}:{:02} {}",
                self.display_hour(),
                self.value.minute,
                if self.value.hour < 12 { "AM" } else { "PM" }
            )
        }
    }

    /// Drains the value after each user edit (arrows, digits, AT
    /// actions) — `None` when nothing changed since the last call.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite::widgets::TimePicker;
    ///
    /// let mut tp = TimePicker::new();
    /// assert_eq!(tp.take_edited(), None);
    /// ```
    pub fn take_edited(&mut self) -> Option<Time> {
        self.edited_pending.take()
    }

    /// The currently focused segment index (`0 = hour`, `1 = minute`,
    /// `2 = AM/PM`) — diagnostics and tests.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite::widgets::TimePicker;
    ///
    /// assert_eq!(TimePicker::new().focused_segment(), 0);
    /// ```
    pub fn focused_segment(&self) -> usize {
        match self.focused {
            Segment::Hour => 0,
            Segment::Minute => 1,
            Segment::AmPm => 2,
        }
    }

    /// Number of live segments (`2` in 24-hour mode, `3` in 12-hour).
    fn segment_count(&self) -> usize {
        if self.use_24h {
            2
        } else {
            3
        }
    }

    /// Hour as displayed: `0..=23` in 24-hour mode, `1..=12` in
    /// 12-hour mode (`0`/`12` both render `12`).
    fn display_hour(&self) -> u32 {
        if self.use_24h {
            self.value.hour
        } else {
            match self.value.hour % 12 {
                0 => 12,
                h => h,
            }
        }
    }

    /// Snaps a minute to the `minute_step` grid (nearest multiple,
    /// capped at 59 so `58` at step `15` lands on `45`…`59` — rounds
    /// half away from zero, then clamps).
    fn snap_minute(&self, minute: u32) -> u32 {
        let step = self.minute_step.max(1);
        ((minute + step / 2) / step * step).min(59)
    }

    /// Records a user edit.
    fn mark_edited(&mut self) {
        self.edited_pending = Some(self.value);
    }

    /// Steps the focused segment by `delta` (wrapping) and records the
    /// edit.
    fn nudge(&mut self, delta: i64) {
        match self.focused {
            Segment::Hour => {
                if self.use_24h {
                    self.value.hour = (i64::from(self.value.hour) + delta).rem_euclid(24) as u32;
                } else {
                    // 12-hour dial: wrap inside 1..=12, preserving the
                    // meridiem (Qt `QTimeEdit` behaviour).
                    let h12 = i64::from(self.display_hour());
                    let next = (h12 - 1 + delta).rem_euclid(12) + 1;
                    let am = self.value.hour < 12;
                    self.value.hour = if am {
                        (next % 12) as u32
                    } else {
                        (12 + next % 12) as u32
                    };
                }
            }
            Segment::Minute => {
                let step = i64::from(self.minute_step.max(1));
                self.value.minute =
                    (i64::from(self.value.minute) + delta * step).rem_euclid(60) as u32;
            }
            Segment::AmPm => {
                // Toggling the meridiem flips hour by ±12.
                self.value.hour = (self.value.hour + 12) % 24;
            }
        }
        self.pending_digit = None;
        self.mark_edited();
    }

    /// Moves segment focus left/right, skipping the AM/PM slot in
    /// 24-hour mode.
    fn move_focus(&mut self, delta: i64) {
        let n = self.segment_count() as i64;
        let next = (self.focused_segment() as i64 + delta).rem_euclid(n);
        self.focused = match next {
            0 => Segment::Hour,
            1 => Segment::Minute,
            _ => Segment::AmPm,
        };
        self.pending_digit = None;
    }

    /// Types a digit into the focused segment — the two-digit rollover
    /// idiom: the first digit replaces the segment value, the second
    /// forms `pending*10 + digit` when it fits the segment's range and
    /// then auto-advances; an out-of-range second digit restarts the
    /// entry with itself.
    fn type_digit(&mut self, digit: u8) {
        match self.focused {
            Segment::Hour => {
                let max = if self.use_24h { 23u32 } else { 12 };
                match self.pending_digit {
                    None => {
                        let d = u32::from(digit);
                        self.set_display_hour(if self.use_24h { d.min(23) } else { d });
                        self.pending_digit = Some(digit);
                        // A single digit that can't start a two-digit
                        // number (e.g. "5" of max 23) commits outright.
                        if d * 10 > max {
                            self.pending_digit = None;
                            self.move_focus(1);
                        }
                    }
                    Some(p) => {
                        let combined = u32::from(p) * 10 + u32::from(digit);
                        if combined <= max && (self.use_24h || combined >= 1) {
                            self.set_display_hour(combined);
                            self.pending_digit = None;
                            self.move_focus(1);
                        } else {
                            // Doesn't fit — restart the entry; a digit
                            // that can't begin a two-digit number
                            // commits outright and advances.
                            self.set_display_hour(u32::from(digit));
                            self.pending_digit = Some(digit);
                            if u32::from(digit) * 10 > max {
                                self.pending_digit = None;
                                self.move_focus(1);
                            }
                        }
                    }
                }
            }
            Segment::Minute => match self.pending_digit {
                None => {
                    self.value.minute = self.snap_minute(u32::from(digit));
                    self.pending_digit = Some(digit);
                    if u32::from(digit) * 10 > 59 {
                        self.pending_digit = None;
                        self.move_focus(1);
                    }
                }
                Some(p) => {
                    let combined = u32::from(p) * 10 + u32::from(digit);
                    if combined <= 59 {
                        self.value.minute = self.snap_minute(combined);
                        self.pending_digit = None;
                        self.move_focus(1);
                    } else {
                        self.value.minute = self.snap_minute(u32::from(digit));
                        self.pending_digit = Some(digit);
                        if u32::from(digit) * 10 > 59 {
                            self.pending_digit = None;
                            self.move_focus(1);
                        }
                    }
                }
            },
            Segment::AmPm => {}
        }
        self.mark_edited();
    }

    /// Writes a 12-hour-dial hour (`1..=12`, `0` reads as `12`) back
    /// into the 24-hour `value`, preserving the meridiem.
    fn set_display_hour(&mut self, h12: u32) {
        if self.use_24h {
            self.value.hour = h12.min(23);
        } else {
            let h12 = if h12 == 0 { 12 } else { h12.min(12) };
            let am = self.value.hour < 12;
            self.value.hour = if am { h12 % 12 } else { 12 + h12 % 12 };
        }
    }

    /// Sets the meridiem in 12-hour mode (`true` = AM).
    fn set_meridiem(&mut self, am: bool) {
        if self.use_24h {
            return;
        }
        let h12 = self.display_hour() % 12;
        self.value.hour = if am { h12 } else { 12 + h12 };
        self.mark_edited();
    }

    /// Parses `"H:MM"`/`"HH:MM"` (optional `AM`/`PM` suffix) — the
    /// `SetValue` contract.
    fn parse_time(text: &str) -> Option<Time> {
        let t = text.trim();
        let (body, pm) = if let Some(b) = t.strip_suffix("PM") {
            (b, Some(true))
        } else if let Some(b) = t.strip_suffix("AM") {
            (b, Some(false))
        } else {
            (t, None)
        };
        let (h, m) = body.trim().split_once(':')?;
        let mut hour: u32 = h.trim().parse().ok()?;
        let minute: u32 = m.trim().parse().ok()?;
        if let Some(pm) = pm {
            if !(1..=12).contains(&hour) {
                return None;
            }
            hour = if pm { 12 + hour % 12 } else { hour % 12 };
        }
        let time = Time { hour, minute };
        time.is_valid().then_some(time)
    }
}

impl Default for TimePicker {
    fn default() -> Self {
        Self::new()
    }
}

impl Widget for TimePicker {
    fn measure(&mut self, cx: &mut LayoutContext, constraints: LayoutConstraints) -> Vec2 {
        let w = if self.use_24h { FACE_W_24 } else { FACE_W_12 };
        Vec2::new(
            cx.pt(w).min(constraints.max_size.x.max(0.0)),
            cx.pt(FACE_H).min(constraints.max_size.y.max(0.0)),
        )
    }

    fn min_render(&self) -> RenderMinimum {
        RenderMinimum::new(Vec2::new(FACE_W_24, FACE_H)).with_policy(UnderflowPolicy::Lint)
    }

    fn layout(&mut self, cx: &mut LayoutContext, bounds: Rect) {
        self.cached_bounds = bounds;
        if self.enabled {
            cx.hot.flags |= NodeFlags::FOCUSABLE;
        } else {
            cx.hot.flags.remove(NodeFlags::FOCUSABLE);
        }
        // Equal-width segment slots inside the inner padding; the
        // AM/PM slot stays empty in 24-hour mode.
        let pad = cx.pt(INNER_PAD);
        let inner_w = (bounds.width() - pad * 2.0).max(0.0);
        let n = self.segment_count() as f32;
        let slot = inner_w / n;
        self.segment_rects = [
            Rect::new(bounds.min_x() + pad, bounds.min_y(), slot, bounds.height()),
            Rect::new(
                bounds.min_x() + pad + slot,
                bounds.min_y(),
                slot,
                bounds.height(),
            ),
            if self.use_24h {
                Rect::default()
            } else {
                Rect::new(
                    bounds.min_x() + pad + slot * 2.0,
                    bounds.min_y(),
                    slot,
                    bounds.height(),
                )
            },
        ];
    }

    fn accessibility(&self, node: &mut AccessKitNode) {
        // `TimeInput` — the `input[type=time]` role — rather than
        // `SpinButton`: the value is a structured time, not a scalar.
        // Increment/Decrement/SetValue still give AT stepper control.
        node.set_role(accesskit::Role::TimeInput);
        if let Some(ref label) = self.label {
            node.set_label(label.as_str());
        }
        node.set_value(self.text());
        node.add_action(accesskit::Action::SetValue);
        node.add_action(accesskit::Action::Increment);
        node.add_action(accesskit::Action::Decrement);
        if self.enabled {
            node.add_action(accesskit::Action::Focus);
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
                position,
                button: PointerButton::Primary,
                ..
            } => {
                for (i, rect) in self.segment_rects.iter().enumerate() {
                    if rect.contains(*position) {
                        self.focused = match i {
                            0 => Segment::Hour,
                            1 => Segment::Minute,
                            _ => Segment::AmPm,
                        };
                        self.pending_digit = None;
                        break;
                    }
                }
                EventResponse::CaptureFocus
            }
            WidgetEvent::KeyPressed { key, .. } => match key.as_str() {
                "ArrowLeft" => {
                    self.move_focus(-1);
                    EventResponse::RequestRepaint
                }
                "ArrowRight" => {
                    self.move_focus(1);
                    EventResponse::RequestRepaint
                }
                "ArrowUp" => {
                    self.nudge(1);
                    EventResponse::RequestRepaint
                }
                "ArrowDown" => {
                    self.nudge(-1);
                    EventResponse::RequestRepaint
                }
                "Backspace" => {
                    self.pending_digit = None;
                    EventResponse::Handled
                }
                "a" | "A" if !self.use_24h => {
                    self.set_meridiem(true);
                    EventResponse::RequestRepaint
                }
                "p" | "P" if !self.use_24h => {
                    self.set_meridiem(false);
                    EventResponse::RequestRepaint
                }
                k if k.len() == 1 => {
                    if let Some(digit) = k.chars().next().and_then(|c| c.to_digit(10)) {
                        self.type_digit(digit as u8);
                        EventResponse::RequestRepaint
                    } else {
                        EventResponse::Ignored
                    }
                }
                _ => EventResponse::Ignored,
            },
            WidgetEvent::FocusLost => {
                self.pending_digit = None;
                EventResponse::Handled
            }
            WidgetEvent::SemanticAction(action) => match action {
                SemanticAction::Increment => {
                    self.nudge(1);
                    EventResponse::RequestRepaint
                }
                SemanticAction::Decrement => {
                    self.nudge(-1);
                    EventResponse::RequestRepaint
                }
                SemanticAction::SetValue(text) => {
                    if let Some(t) = Self::parse_time(text) {
                        self.set_time(t);
                        self.mark_edited();
                        return EventResponse::RequestRepaint;
                    }
                    EventResponse::Ignored
                }
                SemanticAction::Focus => EventResponse::CaptureFocus,
                _ => EventResponse::Ignored,
            },
            _ => EventResponse::Ignored,
        }
    }

    fn paint(&self, cx: &mut PaintContext) {
        let b = cx.bounds;
        let rect = kurbo::Rect::new(
            f64::from(b.min_x()),
            f64::from(b.min_y()),
            f64::from(b.max_x()),
            f64::from(b.max_y()),
        );
        let face = Shape::rounded(cx.dim(TokenKey::BorderRadiusSmall, 3.0));
        cx.list
            .push_fill_shape(rect, &face, cx.color(TokenKey::SurfaceColor, FACE_BG));
        cx.list.push_stroke_shape(
            rect,
            &face,
            cx.pt(1.0),
            cx.color(TokenKey::BorderColor, FACE_BORDER),
        );

        let painter = crate::text_paint::resolve_painter(&self.text_painter, cx.text_painter);
        let font_px = cx.pt(13.0);
        let ink = if self.enabled {
            cx.color(TokenKey::TextColor, INK)
        } else {
            cx.color(TokenKey::TextMutedColor, INK_MUTED)
        };

        // Focused-segment highlight behind the digits.
        let focus_idx = self.focused_segment();
        if self.enabled && focus_idx < self.segment_count() {
            let r = self.segment_rects[focus_idx];
            cx.list.push_fill_shape(
                kurbo::Rect::new(
                    f64::from(r.min_x() + cx.pt(2.0)),
                    f64::from(r.min_y() + cx.pt(2.0)),
                    f64::from(r.max_x() - cx.pt(2.0)),
                    f64::from(r.max_y() - cx.pt(2.0)),
                ),
                &Shape::rounded(cx.dim(TokenKey::BorderRadiusSmall, 3.0)),
                cx.color(TokenKey::AccentColor, FOCUS_BG),
            );
        }

        // Segment texts, centred in their slots. The ":" separators
        // sit on the slot boundary between hour/minute — painted
        // against the preceding slot's right edge.
        let seg_text: [(usize, String); 3] = [
            (0, format!("{:02}", self.display_hour())),
            (1, format!("{:02}", self.value.minute)),
            (
                2,
                if self.value.hour < 12 {
                    "AM".to_string()
                } else {
                    "PM".to_string()
                },
            ),
        ];
        for (i, text) in &seg_text {
            if *i >= self.segment_count() {
                break;
            }
            let r = self.segment_rects[*i];
            let seg_ink = if self.enabled && *i == focus_idx {
                cx.color(TokenKey::TextInverseColor, FOCUS_INK)
            } else {
                ink
            };
            let w = painter
                .and_then(|p| p.measure_text(text, font_px))
                .unwrap_or(text.len() as f32 * font_px * 0.55);
            let tx = r.min_x() + (r.width() - w).max(0.0) / 2.0;
            let wclip = kurbo::Rect::new(
                f64::from(b.min_x()),
                f64::from(b.min_y()),
                f64::from(b.max_x()),
                f64::from(b.max_y()),
            );
            crate::text_paint::paint_label_clipped(
                painter,
                cx.list,
                wclip,
                kurbo::Point::new(
                    f64::from(tx),
                    f64::from(r.min_y() + (r.height() - font_px) / 2.0),
                ),
                text,
                font_px,
                seg_ink,
            );
            // ':' separator after hour (and after minute in 12h mode
            // would read oddly — only hour→minute gets one).
            if *i == 0 {
                crate::text_paint::paint_label_clipped(
                    painter,
                    cx.list,
                    wclip,
                    kurbo::Point::new(
                        f64::from(r.max_x() - font_px * 0.2),
                        f64::from(r.min_y() + (r.height() - font_px) / 2.0),
                    ),
                    ":",
                    font_px,
                    ink,
                );
            }
        }
    }
}

impl std::fmt::Debug for TimePicker {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("TimePicker")
            .field("value", &self.value)
            .field("use_24h", &self.use_24h)
            .field("minute_step", &self.minute_step)
            .field("enabled", &self.enabled)
            .finish()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use martensite_core::HotNode;

    fn laid_out(tp: &mut TimePicker) {
        let mut hot = HotNode::default();
        let mut cx = LayoutContext {
            hot: &mut hot,
            scale: 1.0,
        };
        tp.layout(&mut cx, Rect::new(0.0, 0.0, 130.0, 26.0));
    }

    fn key(k: &str) -> WidgetEvent {
        WidgetEvent::KeyPressed {
            key: k.to_string(),
            repeat: false,
        }
    }

    fn event(tp: &mut TimePicker, ev: &WidgetEvent) -> EventResponse {
        let mut cx = EventContext {
            event: ev,
            bounds: tp.cached_bounds,
            scale: 1.0,
        };
        tp.event(&mut cx)
    }

    #[test]
    fn new_defaults() {
        let tp = TimePicker::new();
        assert_eq!(tp.get_time(), Time { hour: 0, minute: 0 });
        assert_eq!(tp.text(), "00:00");
        assert!(tp.use_24h && tp.enabled);
    }

    #[test]
    fn text_formats_12h() {
        let tp = TimePicker::new()
            .time(Time { hour: 0, minute: 5 })
            .use_24h(false);
        assert_eq!(tp.text(), "12:05 AM");
        let tp = TimePicker::new()
            .time(Time {
                hour: 13,
                minute: 30,
            })
            .use_24h(false);
        assert_eq!(tp.text(), "01:30 PM");
    }

    #[test]
    fn set_time_clamps_and_snaps() {
        let mut tp = TimePicker::new().minute_step(5);
        tp.set_time(Time {
            hour: 30,
            minute: 70,
        });
        assert_eq!(
            tp.get_time(),
            Time {
                hour: 23,
                minute: 59
            }
        );
        tp.set_time(Time {
            hour: 10,
            minute: 8,
        });
        assert_eq!(
            tp.get_time(),
            Time {
                hour: 10,
                minute: 10
            }
        );
    }

    #[test]
    fn arrows_step_focused_segment() {
        let mut tp = TimePicker::new().time(Time {
            hour: 10,
            minute: 30,
        });
        laid_out(&mut tp);
        event(&mut tp, &key("ArrowUp"));
        assert_eq!(tp.get_time().hour, 11);
        event(&mut tp, &key("ArrowDown"));
        assert_eq!(tp.get_time().hour, 10);
        // Wrap: 23 + 1 → 0.
        tp.set_time(Time {
            hour: 23,
            minute: 30,
        });
        event(&mut tp, &key("ArrowUp"));
        assert_eq!(tp.get_time().hour, 0);
    }

    #[test]
    fn minute_step_arrows_wrap() {
        let mut tp = TimePicker::new().minute_step(15).time(Time {
            hour: 9,
            minute: 45,
        });
        laid_out(&mut tp);
        event(&mut tp, &key("ArrowRight")); // focus minute
        event(&mut tp, &key("ArrowUp"));
        assert_eq!(tp.get_time().minute, 0); // 45 + 15 wraps
        event(&mut tp, &key("ArrowDown"));
        assert_eq!(tp.get_time().minute, 45);
    }

    #[test]
    fn left_right_move_segment_focus() {
        let mut tp = TimePicker::new().use_24h(false);
        laid_out(&mut tp);
        assert_eq!(tp.focused_segment(), 0);
        event(&mut tp, &key("ArrowRight"));
        assert_eq!(tp.focused_segment(), 1);
        event(&mut tp, &key("ArrowRight"));
        assert_eq!(tp.focused_segment(), 2);
        event(&mut tp, &key("ArrowRight")); // wraps
        assert_eq!(tp.focused_segment(), 0);
        event(&mut tp, &key("ArrowLeft")); // wraps back
        assert_eq!(tp.focused_segment(), 2);
    }

    #[test]
    fn digits_roll_over_and_advance() {
        let mut tp = TimePicker::new();
        laid_out(&mut tp);
        event(&mut tp, &key("1"));
        assert_eq!(tp.get_time().hour, 1);
        event(&mut tp, &key("2"));
        assert_eq!(tp.get_time().hour, 12);
        assert_eq!(tp.focused_segment(), 1); // auto-advanced
        event(&mut tp, &key("4"));
        event(&mut tp, &key("5"));
        assert_eq!(tp.get_time().minute, 45);
    }

    #[test]
    fn out_of_range_second_digit_restarts() {
        let mut tp = TimePicker::new();
        laid_out(&mut tp);
        event(&mut tp, &key("2"));
        event(&mut tp, &key("9")); // "29" > 23 → restart with 9
        assert_eq!(tp.get_time().hour, 9);
    }

    #[test]
    fn ampm_segment_toggles() {
        let mut tp = TimePicker::new()
            .use_24h(false)
            .time(Time { hour: 9, minute: 0 });
        laid_out(&mut tp);
        event(&mut tp, &key("p")); // 'p' sets PM wherever focus is
        assert_eq!(tp.get_time().hour, 21);
        event(&mut tp, &key("a"));
        assert_eq!(tp.get_time().hour, 9);
        // Arrow on the ampm segment flips the meridiem.
        event(&mut tp, &key("ArrowRight"));
        event(&mut tp, &key("ArrowRight"));
        assert_eq!(tp.focused_segment(), 2);
        event(&mut tp, &key("ArrowUp"));
        assert_eq!(tp.get_time().hour, 21);
    }

    #[test]
    fn press_focuses_segment() {
        let mut tp = TimePicker::new();
        laid_out(&mut tp);
        // 130-wide face, 8pt padding each side → two ~57px slots.
        let press = WidgetEvent::PointerPressed {
            position: Vec2::new(100.0, 13.0),
            button: PointerButton::Primary,
            count: 1,
        };
        assert_eq!(event(&mut tp, &press), EventResponse::CaptureFocus);
        assert_eq!(tp.focused_segment(), 1);
    }

    #[test]
    fn edits_report_via_take_edited() {
        let mut tp = TimePicker::new().time(Time { hour: 8, minute: 0 });
        laid_out(&mut tp);
        event(&mut tp, &key("ArrowUp"));
        assert_eq!(tp.take_edited(), Some(Time { hour: 9, minute: 0 }));
        assert_eq!(tp.take_edited(), None);
        // Programmatic set does not feed the seam.
        tp.set_time(Time { hour: 1, minute: 0 });
        assert_eq!(tp.take_edited(), None);
    }

    #[test]
    fn semantic_actions_step_and_set() {
        let mut tp = TimePicker::new();
        laid_out(&mut tp);
        event(
            &mut tp,
            &WidgetEvent::SemanticAction(SemanticAction::Increment),
        );
        assert_eq!(tp.get_time().hour, 1);
        event(
            &mut tp,
            &WidgetEvent::SemanticAction(SemanticAction::SetValue("14:45".into())),
        );
        assert_eq!(
            tp.get_time(),
            Time {
                hour: 14,
                minute: 45
            }
        );
        event(
            &mut tp,
            &WidgetEvent::SemanticAction(SemanticAction::SetValue("9:30 PM".into())),
        );
        assert_eq!(
            tp.get_time(),
            Time {
                hour: 21,
                minute: 30
            }
        );
    }

    #[test]
    fn parse_time_variants() {
        assert_eq!(
            TimePicker::parse_time("7:05"),
            Some(Time { hour: 7, minute: 5 })
        );
        assert_eq!(
            TimePicker::parse_time("12:00 AM"),
            Some(Time { hour: 0, minute: 0 })
        );
        assert_eq!(
            TimePicker::parse_time("12:00 PM"),
            Some(Time {
                hour: 12,
                minute: 0
            })
        );
        assert_eq!(TimePicker::parse_time("25:00"), None);
        assert_eq!(TimePicker::parse_time("nope"), None);
    }

    #[test]
    fn accessibility_contract() {
        let tp = TimePicker::new()
            .time(Time {
                hour: 14,
                minute: 30,
            })
            .label("Alarm");
        let mut node = AccessKitNode::new(accesskit::Role::Unknown);
        tp.accessibility(&mut node);
        assert_eq!(node.role(), accesskit::Role::TimeInput);
        assert_eq!(node.value(), Some("14:30"));
        assert_eq!(node.label(), Some("Alarm"));
        assert!(node.supports_action(accesskit::Action::Increment));
        assert!(node.supports_action(accesskit::Action::Decrement));
        assert!(node.supports_action(accesskit::Action::SetValue));
    }

    #[test]
    fn disabled_ignores_input() {
        let mut tp = TimePicker::new().enabled(false);
        laid_out(&mut tp);
        assert_eq!(event(&mut tp, &key("ArrowUp")), EventResponse::Ignored);
        assert_eq!(tp.get_time().hour, 0);
    }
}
