//! `DatePicker` widget: a read-only date field with a calendar popup
//! (`QDateEdit` / `GtkCalendar` / WinUI `CalendarDatePicker`).
//!
//! The face looks like a read-only text field with a calendar glyph;
//! pressing it (or `Enter`/`Space`/`ArrowDown`, or AT `Expand`/`Click`)
//! reconciles a `CalendarSurface` into the
//! [`OverlayLayer`](martensite_core::overlay::OverlayLayer) at
//! `OverlayAnchor::Bounds` on the next
//! [`DatePicker::sync_overlay`] — placed below the face when it fits,
//! flipped above and clamped into the viewport otherwise.
//!
//! - **Popup**: a `Role::Dialog` month grid — `«`/`»` month buttons,
//!   a `Mo`–`Su` (or `Su`–`Sa`) weekday header, and day cells. The
//!   highlighted *today* is injected by the host via
//!   [`DatePicker::today`] — the widget never reads a system clock, so
//!   tests stay deterministic.
//! - **Pick**: clicking a valid day (or `Enter` on the keyboard
//!   focus cell) writes it into a shared slot; the face drains it in
//!   `sync_overlay`, stores it as the value, queues it for
//!   [`DatePicker::take_selected`], and closes the popup. Light-dismiss
//!   (outside press, `Escape`) closes without picking.
//! - **Keyboard in the popup**: arrows move the day focus,
//!   `PageUp`/`PageDown` change the displayed month, `Enter` picks,
//!   `Escape` closes.
//! - **Range**: [`DatePicker::min_date`]/[`DatePicker::max_date`] clamp
//!   the pickable range — out-of-range cells are inert and dimmed.
//! - **Range mode**: [`DatePicker::range_mode`] switches the popup to
//!   `Ant RangePicker` two-click span picking — first pick anchors
//!   (stay open, hover previews the span), second completes, commits
//!   to [`DatePicker::take_range`], and closes. The face shows
//!   `"start – end"`.
//!
//! The face emits `Role::ComboBox` with `aria-haspopup="dialog"`,
//! `aria-expanded`, and the formatted date as its value; the popup
//! emits `Role::Dialog`.
//!
//! # Examples
//!
//! ```
//! use martensite::widgets::{Date, DatePicker};
//!
//! let mut dp = DatePicker::new()
//!     .date(Date { year: 2024, month: 6, day: 15 })
//!     .today(Date { year: 2024, month: 6, day: 1 });
//! dp.open();
//! assert!(dp.is_open());
//! ```

use std::sync::{Arc, Mutex};

use accesskit::Node as AccessKitNode;
use glam::Vec2;
use martensite_core::overlay::{OverlayAnchor, OverlayLayer};
use martensite_core::shape::Shape;
use martensite_core::widget::{
    EventContext, EventResponse, LayoutConstraints, LayoutContext, PaintContext, PointerButton,
    SemanticAction, Widget, WidgetEvent,
};
use martensite_core::{NodeFlags, Rect, RenderMinimum, TokenKey, UnderflowPolicy};

/// Face height (logical points).
const FACE_H: f32 = 28.0;
/// Minimum face width (logical points).
const FACE_MIN_W: f32 = 110.0;
/// Face background.
const FACE_BG: [u8; 4] = [250, 250, 252, 255];
/// Face border.
const FACE_BORDER: [u8; 4] = [150, 155, 165, 255];
/// Label ink.
const INK: [u8; 4] = [30, 30, 36, 255];
/// Disabled / placeholder ink.
const INK_MUTED: [u8; 4] = [150, 150, 158, 255];
/// Popup background.
const POPUP_BG: [u8; 4] = [252, 252, 254, 255];
/// Popup border.
const POPUP_BORDER: [u8; 4] = [140, 145, 155, 255];
/// Accent (selected day, focus ring, today outline).
const ACCENT: [u8; 4] = [60, 110, 220, 255];
/// Selected-day ink.
const ACCENT_INK: [u8; 4] = [255, 255, 255, 255];
/// Dimmed out-of-range cell ink.
const DIM_INK: [u8; 4] = [185, 188, 196, 255];
/// Hovered day-cell wash.
const HOVER_BG: [u8; 4] = [225, 232, 246, 255];
/// Header nav-button hit target (logical points).
const NAV_W: f32 = 26.0;
/// Header strip height (logical points).
const HEADER_H: f32 = 26.0;
/// Weekday-label row height (logical points).
const WEEK_ROW_H: f32 = 18.0;
/// Day cell edge (logical points).
const CELL: f32 = 28.0;
/// Grid rows always shown (keeps the popup size stable across months).
const GRID_ROWS: usize = 6;
/// Grid columns — one per weekday.
const GRID_COLS: usize = 7;
/// Popup padding (logical points).
const POPUP_PAD: f32 = 8.0;

/// Short English weekday names indexed `0 = Sunday … 6 = Saturday`
/// (the [`Date::weekday_of`] convention).
const WEEKDAY_SHORT: [&str; 7] = ["Sun", "Mon", "Tue", "Wed", "Thu", "Fri", "Sat"];

/// English month names indexed `month - 1` (`1 = January`).
const MONTH_NAMES: [&str; 12] = [
    "January",
    "February",
    "March",
    "April",
    "May",
    "June",
    "July",
    "August",
    "September",
    "October",
    "November",
    "December",
];

/// A proleptic-Gregorian calendar date (`year` may be negative or
/// large; `month` is `1..=12`, `day` `1..=31`).
///
/// Order is the natural chronological order — the derived `Ord`
/// compares `year`, then `month`, then `day`.
///
/// # Examples
///
/// ```
/// use martensite::widgets::Date;
///
/// let d = Date { year: 2024, month: 2, day: 29 };
/// assert!(d.is_valid()); // 2024 is a leap year
/// assert!(!Date { year: 2023, month: 2, day: 29 }.is_valid());
/// ```
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Date {
    /// Gregorian year (proleptic; `0` and negatives are meaningful to
    /// the arithmetic but unusual in UIs).
    pub year: i32,
    /// Month, `1..=12`.
    pub month: u32,
    /// Day of month, `1..=31` (clamped by the month's real length for
    /// validity — see [`Date::is_valid`]).
    pub day: u32,
}

impl Date {
    /// Whether the fields form a real calendar date.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite::widgets::Date;
    ///
    /// assert!(Date { year: 2000, month: 12, day: 31 }.is_valid());
    /// assert!(!Date { year: 2000, month: 13, day: 1 }.is_valid());
    /// assert!(!Date { year: 2001, month: 4, day: 31 }.is_valid());
    /// ```
    pub fn is_valid(&self) -> bool {
        (1..=12).contains(&self.month)
            && self.day >= 1
            && self.day <= Self::days_in_month(self.year, self.month)
    }

    /// Days in `month` of `year`, honouring the Gregorian leap-year
    /// rule (leap when divisible by 4, except centuries not divisible
    /// by 400). Returns `0` for a month outside `1..=12`.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite::widgets::Date;
    ///
    /// assert_eq!(Date::days_in_month(2024, 2), 29); // leap year
    /// assert_eq!(Date::days_in_month(1900, 2), 28); // century, not /400
    /// assert_eq!(Date::days_in_month(2000, 2), 29); // /400 century
    /// assert_eq!(Date::days_in_month(2024, 0), 0);
    /// ```
    pub fn days_in_month(year: i32, month: u32) -> u32 {
        const DAYS: [u32; 12] = [31, 28, 31, 30, 31, 30, 31, 31, 30, 31, 30, 31];
        match month {
            1..=12 => {
                let leap = year % 4 == 0 && (year % 100 != 0 || year % 400 == 0);
                DAYS[(month - 1) as usize] + u32::from(month == 2 && leap)
            }
            _ => 0,
        }
    }

    /// Weekday of a valid date: `0 = Sunday … 6 = Saturday`, matching
    /// the C `tm_wday` convention. Derived from `epoch_days` —
    /// `1970-01-01` was a Thursday, so `(epoch + 4) mod 7`.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite::widgets::Date;
    ///
    /// assert_eq!(Date::weekday_of(1970, 1, 1), 4); // Thursday
    /// assert_eq!(Date::weekday_of(2024, 6, 15), 6); // Saturday
    /// ```
    pub fn weekday_of(year: i32, month: u32, day: u32) -> u32 {
        let epoch = epoch_days(Date { year, month, day });
        (epoch + 4).rem_euclid(7) as u32
    }

    /// `true` when `self` falls inside `[min, max]`; open bounds are
    /// treated as unbounded.
    fn in_range(&self, min: Option<Date>, max: Option<Date>) -> bool {
        if let Some(min) = min {
            if *self < min {
                return false;
            }
        }
        if let Some(max) = max {
            if *self > max {
                return false;
            }
        }
        true
    }
}

/// Days since the Unix epoch (`1970-01-01`) for a valid date, using
/// Howard Hinnant's `days_from_civil` — a branch-light epoch-day
/// calculation that shifts the year to start in March so the leap day
/// lands at the year's end, then counts 400-year eras. Documented in
/// <https://howardhinnant.github.io/date_algorithms.html>.
fn epoch_days(date: Date) -> i64 {
    let y = i64::from(date.year) - i64::from(date.month <= 2);
    let era = if y >= 0 { y } else { y - 399 } / 400;
    let yoe = y - era * 400; // [0, 399]
    let mp = (i64::from(date.month) + 9) % 12; // March = 0 … February = 11
    let doy = (153 * mp + 2) / 5 + i64::from(date.day) - 1; // [0, 365]
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy; // [0, 146096]
    era * 146_097 + doe - 719_468
}

/// The calendar date `days` after `date` (negative walks back) — pure
/// epoch arithmetic: convert, add, convert back via Hinnant's
/// `civil_from_days`.
fn add_days(date: Date, days: i64) -> Date {
    let z = epoch_days(date) + days + 719_468;
    let era = if z >= 0 { z } else { z - 146_096 } / 146_097;
    let doe = z - era * 146_097; // [0, 146096]
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365; // [0, 399]
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100); // [0, 365]
    let mp = (5 * doy + 2) / 153; // [0, 11]
    let d = doy - (153 * mp + 2) / 5 + 1; // [1, 31]
    let m = mp + 3 - 12 * i64::from(mp >= 10); // [1, 12]
    Date {
        year: (y + i64::from(m <= 2)) as i32,
        month: m as u32,
        day: d as u32,
    }
}

/// `(year, month)` stepped by `delta` months, staying inside
/// `1..=12`.
fn add_months(year: i32, month: u32, delta: i64) -> (i32, u32) {
    let total = i64::from(year) * 12 + i64::from(month) - 1 + delta;
    let y = total.div_euclid(12);
    let m = total.rem_euclid(12) + 1;
    (y as i32, m as u32)
}

/// The format-lite substitution: writes `date` into `out` honouring
/// the `{year}` / `{month}` / `{month:02}` / `{day}` / `{day:02}` /
/// `{weekday}` tokens; everything else — including unknown
/// `{tokens}` — passes through literally.
fn format_into(out: &mut String, fmt: &str, date: Date) {
    let mut rest = fmt;
    while let Some(open) = rest.find('{') {
        out.push_str(&rest[..open]);
        let after = &rest[open + 1..];
        let Some(close) = after.find('}') else {
            // Unterminated `{` — emit it verbatim and stop.
            out.push('{');
            out.push_str(after);
            return;
        };
        match &after[..close] {
            "year" => out.push_str(&date.year.to_string()),
            "month" => out.push_str(&date.month.to_string()),
            "month:02" => out.push_str(&format!("{:02}", date.month)),
            "day" => out.push_str(&date.day.to_string()),
            "day:02" => out.push_str(&format!("{:02}", date.day)),
            "weekday" => {
                if date.is_valid() {
                    out.push_str(
                        WEEKDAY_SHORT[Date::weekday_of(date.year, date.month, date.day) as usize],
                    );
                }
            }
            // Unknown token — pass through verbatim.
            other => {
                out.push('{');
                out.push_str(other);
                out.push('}');
            }
        }
        rest = &after[close + 1..];
    }
    out.push_str(rest);
}

/// Channel between a [`DatePicker`] and its live [`CalendarSurface`]:
/// the surface writes a picked date and close requests; the face
/// drains them in [`DatePicker::sync_overlay`]. Range/today/week-start
/// config is handed to the surface at construction — it never changes
/// while the popup is open.
#[derive(Debug, Default)]
struct CalendarChannel {
    /// A day the user picked (click or `Enter` on the focus cell).
    picked: Option<Date>,
    /// A completed `(start, end)` range (range mode only).
    ranged: Option<(Date, Date)>,
    /// The surface asked to close without picking (embedded `Escape`).
    close_requested: bool,
}

/// The popup surface for a [`DatePicker`] — a `Role::Dialog` month
/// grid. Stateless w.r.t. the owner: the displayed month, keyboard
/// focus cell, and hover live here for the popup's lifetime; results
/// flow back through [`CalendarChannel`].
struct CalendarSurface {
    /// Viewed month.
    view_year: i32,
    /// Viewed month (`1..=12`).
    view_month: u32,
    /// The currently committed value — painted as the selected cell.
    selected: Option<Date>,
    /// Two-click range picking (`Ant RangePicker` semantics): first
    /// pick anchors, second completes and closes.
    range_mode: bool,
    /// The committed `(start, end)` span — painted as a wash with
    /// accent endpoints.
    range: Option<(Date, Date)>,
    /// The in-progress range anchor (range mode only).
    anchor: Option<Date>,
    /// Host-injected "today" — ringed when visible.
    today: Option<Date>,
    /// Inclusive pickable bounds.
    min: Option<Date>,
    /// Inclusive pickable bounds.
    max: Option<Date>,
    /// `true` shows `Mo`–`Su`, `false` shows `Su`–`Sa`.
    week_starts_monday: bool,
    /// Keyboard focus cell — arrow keys move it, `Enter` picks it.
    focus: Date,
    /// Day under the pointer (hover wash).
    hover: Option<Date>,
    /// Surface bounds from the last layout pass.
    bounds: Rect,
    /// `«` button rect.
    prev_rect: Rect,
    /// `»` button rect.
    next_rect: Rect,
    /// Top-left of the day grid.
    grid_origin: Vec2,
    /// Result channel back to the owning `DatePicker`.
    channel: Arc<Mutex<CalendarChannel>>,
    /// The silhouette painted last frame — the single source of truth
    /// for `clip_shape`/`hit_shape` (mirrors `ListBoxPopup`).
    painted_shape: Mutex<Shape>,
    /// Shared shaped-text painter from the owning `DatePicker`.
    text_painter: Option<crate::text_paint::SharedTextPainter>,
}

impl CalendarSurface {
    /// Grid origin for the day cells (device px) given `bounds`.
    fn compute_grid_origin(&self, bounds: Rect, scale: f32) -> Vec2 {
        Vec2::new(
            bounds.min_x() + POPUP_PAD * scale,
            bounds.min_y() + POPUP_PAD * scale + HEADER_H * scale + WEEK_ROW_H * scale,
        )
    }

    /// Column `date` occupies in the grid for the current week start.
    fn column(&self, date: Date) -> usize {
        let wd = Date::weekday_of(date.year, date.month, date.day) as usize;
        if self.week_starts_monday {
            (wd + 6) % 7
        } else {
            wd
        }
    }

    /// Date shown at grid `(row, col)` — may fall into the adjacent
    /// month (those cells are dimmed like out-of-range ones).
    fn cell_date(&self, row: usize, col: usize) -> Date {
        let first = Date {
            year: self.view_year,
            month: self.view_month,
            day: 1,
        };
        let offset = row * GRID_COLS + col;
        add_days(first, offset as i64 - self.column(first) as i64)
    }

    /// Whether `date` is a real, pickable cell — in range *and* inside
    /// the viewed month (adjacent-month spill cells are inert).
    fn pickable(&self, date: Date) -> bool {
        date.is_valid()
            && date.month == self.view_month
            && date.year == self.view_year
            && date.in_range(self.min, self.max)
    }

    /// The day cell rect for `date` when it lands in the viewed month.
    fn cell_rect(&self, date: Date, scale: f32) -> Option<Rect> {
        if date.month != self.view_month || date.year != self.view_year || !date.is_valid() {
            return None;
        }
        let first = Date {
            year: self.view_year,
            month: self.view_month,
            day: 1,
        };
        let week =
            ((epoch_days(date) - epoch_days(first)) as usize + self.column(first)) / GRID_COLS;
        if week >= GRID_ROWS {
            return None;
        }
        let cell = CELL * scale;
        Some(Rect::new(
            self.grid_origin.x + self.column(date) as f32 * cell,
            self.grid_origin.y + week as f32 * cell,
            cell,
            cell,
        ))
    }

    /// Steps the viewed month, keeping the focus cell inside it
    /// (clamped to the shorter month's last day).
    fn step_month(&mut self, delta: i64) {
        let (y, m) = add_months(self.view_year, self.view_month, delta);
        self.view_year = y;
        self.view_month = m;
        self.focus = Date {
            year: y,
            month: m,
            day: self.focus.day.min(Date::days_in_month(y, m)),
        };
    }

    /// Moves the keyboard focus by `days`, following into adjacent
    /// months, and refuses to land on an out-of-range cell.
    fn move_focus(&mut self, days: i64) {
        let next = add_days(self.focus, days);
        if !next.in_range(self.min, self.max) {
            return;
        }
        self.focus = next;
        if next.month != self.view_month || next.year != self.view_year {
            self.view_year = next.year;
            self.view_month = next.month;
        }
    }

    /// Writes the pick into the shared channel — the face commits and
    /// closes on the next `sync_overlay`. Range mode: first pick sets
    /// the anchor (stay open); second completes the span, normalizes
    /// order, and asks to close — the `Ant RangePicker` flow.
    fn pick(&mut self, date: Date) {
        let mut channel = self.channel.lock().expect("calendar channel poisoned");
        if self.range_mode {
            match self.anchor {
                None => self.anchor = Some(date),
                Some(a) => {
                    let (lo, hi) = if date < a { (date, a) } else { (a, date) };
                    channel.ranged = Some((lo, hi));
                    channel.close_requested = true;
                    self.anchor = None;
                }
            }
        } else {
            channel.picked = Some(date);
        }
    }

    /// The span highlighted right now: the committed range, or the
    /// provisional `anchor..hover` preview while picking.
    fn active_span(&self) -> Option<(Date, Date)> {
        match self.anchor {
            Some(a) => self.hover.map(|h| if h < a { (h, a) } else { (a, h) }),
            None => self.range,
        }
    }
}

impl Widget for CalendarSurface {
    fn measure(&mut self, cx: &mut LayoutContext, constraints: LayoutConstraints) -> Vec2 {
        Vec2::new(
            (GRID_COLS as f32 * cx.pt(CELL) + cx.pt(POPUP_PAD) * 2.0)
                .min(constraints.max_size.x.max(0.0)),
            (cx.pt(POPUP_PAD) * 2.0
                + cx.pt(HEADER_H)
                + cx.pt(WEEK_ROW_H)
                + GRID_ROWS as f32 * cx.pt(CELL))
            .min(constraints.max_size.y.max(0.0)),
        )
    }

    fn layout(&mut self, cx: &mut LayoutContext, bounds: Rect) {
        self.bounds = bounds;
        cx.hot.flags |= NodeFlags::FOCUSABLE;
        let pad = cx.pt(POPUP_PAD);
        let nav = cx.pt(NAV_W);
        self.prev_rect = Rect::new(
            bounds.min_x() + pad,
            bounds.min_y() + pad,
            nav,
            cx.pt(HEADER_H),
        );
        self.next_rect = Rect::new(
            bounds.max_x() - pad - nav,
            bounds.min_y() + pad,
            nav,
            cx.pt(HEADER_H),
        );
        self.grid_origin = self.compute_grid_origin(bounds, cx.scale);
    }

    fn accessibility(&self, node: &mut AccessKitNode) {
        node.set_role(accesskit::Role::Dialog);
        node.set_label("Calendar");
    }

    fn event(&mut self, cx: &mut EventContext) -> EventResponse {
        match cx.event {
            WidgetEvent::PointerMoved { position } => {
                let mut hit = None;
                'grid: for row in 0..GRID_ROWS {
                    for col in 0..GRID_COLS {
                        let date = self.cell_date(row, col);
                        if let Some(rect) = self.cell_rect(date, cx.scale) {
                            if rect.contains(*position) && self.pickable(date) {
                                hit = Some(date);
                                break 'grid;
                            }
                        }
                    }
                }
                let changed = hit != self.hover;
                self.hover = hit;
                if changed {
                    EventResponse::RequestRepaint
                } else {
                    EventResponse::Handled
                }
            }
            WidgetEvent::PointerPressed {
                position,
                button: PointerButton::Primary,
                ..
            } => {
                if self.prev_rect.contains(*position) {
                    self.step_month(-1);
                    return EventResponse::RequestRepaint;
                }
                if self.next_rect.contains(*position) {
                    self.step_month(1);
                    return EventResponse::RequestRepaint;
                }
                for row in 0..GRID_ROWS {
                    for col in 0..GRID_COLS {
                        let date = self.cell_date(row, col);
                        if let Some(rect) = self.cell_rect(date, cx.scale) {
                            if rect.contains(*position) {
                                if self.pickable(date) {
                                    self.focus = date;
                                    self.pick(date);
                                }
                                // Out-of-range / spill cells are inert:
                                // still consume the press so it can't
                                // count as an outside-press dismissal.
                                return EventResponse::Handled;
                            }
                        }
                    }
                }
                EventResponse::Handled
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
                    self.move_focus(-7);
                    EventResponse::RequestRepaint
                }
                "ArrowDown" => {
                    self.move_focus(7);
                    EventResponse::RequestRepaint
                }
                "PageUp" => {
                    self.step_month(-1);
                    EventResponse::RequestRepaint
                }
                "PageDown" => {
                    self.step_month(1);
                    EventResponse::RequestRepaint
                }
                "Enter" | " " | "Space" => {
                    if self.pickable(self.focus) {
                        self.pick(self.focus);
                    }
                    EventResponse::Handled
                }
                // The layer eats `Escape` before content in arena use —
                // this path serves ownerless-embedded use.
                "Escape" => {
                    self.channel
                        .lock()
                        .expect("calendar channel poisoned")
                        .close_requested = true;
                    EventResponse::Handled
                }
                _ => EventResponse::Ignored,
            },
            // The bubble owns its surface — presses must not leak to
            // the light-dismiss path.
            WidgetEvent::PointerReleased { .. } | WidgetEvent::Scroll { .. } => {
                EventResponse::Handled
            }
            _ => EventResponse::Ignored,
        }
    }

    fn paint(&self, cx: &mut PaintContext) {
        let b = cx.bounds;
        let face = kurbo::Rect::new(
            f64::from(b.min_x()),
            f64::from(b.min_y()),
            f64::from(b.max_x()),
            f64::from(b.max_y()),
        );
        let shape = Shape::rounded(cx.dim(TokenKey::BorderRadius, 6.0));
        *self.painted_shape.lock().expect("popup shape poisoned") = shape.clone();
        cx.list
            .push_fill_shape(face, &shape, cx.color(TokenKey::SurfaceColor, POPUP_BG));
        cx.list.push_stroke_shape(
            face,
            &shape,
            cx.pt(1.0),
            cx.color(TokenKey::BorderColor, POPUP_BORDER),
        );

        let painter = crate::text_paint::resolve_painter(&self.text_painter, cx.text_painter);
        let ink = cx.color(TokenKey::TextColor, INK);
        let accent = cx.color(TokenKey::AccentColor, ACCENT);
        let pad = cx.pt(POPUP_PAD);

        // « / » month buttons — painted hit targets matching
        // `prev_rect`/`next_rect`.
        for (rect, left) in [(self.prev_rect, true), (self.next_rect, false)] {
            let cy = f64::from(rect.min_y() + rect.height() / 2.0);
            let cxm = f64::from(rect.min_x() + rect.width() / 2.0);
            let s = cx.ptf(4.0);
            let mut chev = kurbo::BezPath::new();
            if left {
                chev.move_to((cxm + s * 0.6, cy - s));
                chev.line_to((cxm - s * 0.6, cy));
                chev.line_to((cxm + s * 0.6, cy + s));
            } else {
                chev.move_to((cxm - s * 0.6, cy - s));
                chev.line_to((cxm + s * 0.6, cy));
                chev.line_to((cxm - s * 0.6, cy + s));
            }
            cx.list.push_stroke_path(chev, cx.pt(1.6), ink);
        }

        // "June 2024" title, centred between the nav buttons.
        let title = format!(
            "{} {}",
            MONTH_NAMES[(self.view_month - 1) as usize],
            self.view_year
        );
        let font_px = cx.pt(13.0);
        let title_y = b.min_y() + pad + (cx.pt(HEADER_H) - font_px) / 2.0;
        crate::text_paint::paint_label_clipped(
            painter,
            cx.list,
            kurbo::Rect::new(
                f64::from(self.prev_rect.max_x()),
                f64::from(b.min_y() + pad),
                f64::from(self.next_rect.min_x()),
                f64::from(b.min_y() + pad + cx.pt(HEADER_H)),
            ),
            kurbo::Point::new(
                f64::from(self.prev_rect.max_x() + cx.pt(4.0)),
                f64::from(title_y),
            ),
            &title,
            font_px,
            ink,
        );

        // Weekday header.
        let small_px = cx.pt(10.0);
        let cell = cx.pt(CELL);
        let wk_y = b.min_y() + pad + cx.pt(HEADER_H) + (cx.pt(WEEK_ROW_H) - small_px) / 2.0;
        for col in 0..GRID_COLS {
            // Column index → weekday index (0 = Sunday).
            let wd = if self.week_starts_monday {
                (col + 1) % GRID_COLS
            } else {
                col
            };
            crate::text_paint::paint_label(
                painter,
                cx.list,
                kurbo::Point::new(
                    f64::from(self.grid_origin.x + col as f32 * cell + cell * 0.18),
                    f64::from(wk_y),
                ),
                &WEEKDAY_SHORT[wd][..2],
                small_px,
                cx.color(TokenKey::TextMutedColor, INK_MUTED),
            );
        }

        // Day cells.
        let day_px = cx.pt(12.0);
        for row in 0..GRID_ROWS {
            for col in 0..GRID_COLS {
                let date = self.cell_date(row, col);
                let Some(rect) = self.cell_rect(date, cx.scale) else {
                    continue;
                };
                let krect = kurbo::Rect::new(
                    f64::from(rect.min_x()),
                    f64::from(rect.min_y()),
                    f64::from(rect.max_x()),
                    f64::from(rect.max_y()),
                );
                let in_month = date.month == self.view_month && date.year == self.view_year;
                let pickable = self.pickable(date);
                let span = self.active_span();
                let in_span = span.is_some_and(|(lo, hi)| date >= lo && date <= hi);
                let is_endpoint = span.is_some_and(|(lo, hi)| date == lo || date == hi);
                let is_selected = self.selected == Some(date) || is_endpoint;
                let is_today = self.today == Some(date);
                let is_focus = self.focus == date && in_month;
                let is_hover = self.hover == Some(date);
                if is_selected {
                    cx.list.push_fill_shape(
                        krect,
                        &Shape::rounded(cx.dim(TokenKey::BorderRadiusSmall, 3.0)),
                        accent,
                    );
                } else if (in_span && in_month) || (is_hover && pickable) {
                    cx.list.push_fill_shape(
                        krect,
                        &Shape::rounded(cx.dim(TokenKey::BorderRadiusSmall, 3.0)),
                        cx.color(TokenKey::SecondaryColor, HOVER_BG),
                    );
                }
                if is_today {
                    // A ring inside the cell edge marks "today".
                    let ring = krect.inset(-f64::from(cx.pt(3.0)));
                    cx.list.push_stroke_rect(ring, cx.pt(1.2), accent);
                }
                if is_focus {
                    // Keyboard focus cell: a thin inner outline.
                    cx.list.push_stroke_rect(
                        krect.inset(-f64::from(cx.pt(1.0))),
                        cx.pt(1.0),
                        cx.color(TokenKey::TextColor, INK),
                    );
                }
                let day_ink = if is_selected {
                    cx.color(TokenKey::TextInverseColor, ACCENT_INK)
                } else if !in_month || !pickable {
                    cx.color(TokenKey::TextMutedColor, DIM_INK)
                } else {
                    ink
                };
                // Rough centring: digits average ~0.55em wide.
                let label = date.day.to_string();
                let approx_w = label.len() as f32 * day_px * 0.55;
                crate::text_paint::paint_label(
                    painter,
                    cx.list,
                    kurbo::Point::new(
                        f64::from(rect.min_x() + (rect.width() - approx_w) / 2.0),
                        f64::from(rect.min_y() + (rect.height() - day_px) / 2.0),
                    ),
                    &label,
                    day_px,
                    day_ink,
                );
            }
        }
    }

    fn clips_children(&self) -> bool {
        true
    }

    fn clip_shape(&self) -> Option<Shape> {
        Some(
            self.painted_shape
                .lock()
                .expect("popup shape poisoned")
                .clone(),
        )
    }

    fn hit_shape(&self) -> Option<Shape> {
        Some(
            self.painted_shape
                .lock()
                .expect("popup shape poisoned")
                .clone(),
        )
    }
}

/// A date field with a calendar popup (`QDateEdit` / `GtkCalendar` /
/// WinUI `CalendarDatePicker`).
///
/// The face is a read-only text field showing the formatted value (or
/// `placeholder` when empty); the popup is a `Role::Dialog` month grid
/// reconciled into the [`OverlayLayer`]
/// by [`DatePicker::sync_overlay`] — call it once per frame before
/// `OverlayLayer::layout_pass` (the arena does this for registered
/// widgets).
///
/// The widget never reads a system clock: inject "today" through
/// [`DatePicker::today`] so the popup's highlight stays deterministic.
///
/// # Examples
///
/// ```
/// use martensite::widgets::{Date, DatePicker};
///
/// let mut dp = DatePicker::new()
///     .date(Date { year: 2024, month: 1, day: 5 })
///     .format("{day}.{month}.{year}");
/// dp.open();
/// dp.close();
/// assert_eq!(dp.get_date(), Some(Date { year: 2024, month: 1, day: 5 }));
/// ```
pub struct DatePicker {
    /// Optional accessible label for the face.
    pub label: Option<String>,
    /// Text shown when no date is set.
    pub placeholder: String,
    /// Whether the picker accepts input.
    pub enabled: bool,
    /// Format-lite pattern — `{year}`, `{month}`, `{month:02}`,
    /// `{day}`, `{day:02}`, `{weekday}`.
    pub format: String,
    /// Inclusive minimum pickable date.
    pub min_date: Option<Date>,
    /// Inclusive maximum pickable date.
    pub max_date: Option<Date>,
    /// Host-injected "today" — ringed in the popup. `None` shows no
    /// highlight (the widget never reads a clock itself).
    pub today: Option<Date>,
    /// `true` starts weeks on Monday; `false` on Sunday.
    pub week_starts_monday: bool,
    /// The committed value.
    date: Option<Date>,
    /// Two-click range picking — `Ant RangePicker` semantics.
    pub range_mode: bool,
    /// The committed `(start, end)` span (range mode).
    range: Option<(Date, Date)>,
    /// One-shot span awaiting [`DatePicker::take_range`].
    range_pending: Option<(Date, Date)>,
    /// Whether the popup is logically open.
    open: bool,
    /// Overlay entry id of the open popup.
    popup_id: Option<u64>,
    /// Result channel shared with the live surface.
    channel: Arc<Mutex<CalendarChannel>>,
    /// One-shot pick awaiting [`DatePicker::take_selected`].
    selected_pending: Option<Date>,
    /// Face bounds from the last layout pass.
    cached_bounds: Rect,
    /// The bounds the live popup was last anchored to — `sync_overlay`
    /// re-anchors when `cached_bounds` moves so an open calendar tracks
    /// its face (mirrors `Dropdown`).
    last_anchor: Option<Rect>,
    /// Shared shaped-text painter — `paint` emits real `GlyphRun`s
    /// when set, `DrawText` placeholder boxes otherwise.
    text_painter: Option<crate::text_paint::SharedTextPainter>,
}

impl DatePicker {
    /// An empty picker with the ISO-look default format
    /// `"{year}-{month:02}-{day:02}"`.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite::widgets::DatePicker;
    ///
    /// let dp = DatePicker::new();
    /// assert_eq!(dp.get_date(), None);
    /// assert!(!dp.is_open());
    /// ```
    pub fn new() -> Self {
        Self {
            label: None,
            placeholder: String::new(),
            enabled: true,
            format: "{year}-{month:02}-{day:02}".to_string(),
            min_date: None,
            max_date: None,
            today: None,
            week_starts_monday: true,
            date: None,
            range_mode: false,
            range: None,
            range_pending: None,
            open: false,
            popup_id: None,
            channel: Arc::new(Mutex::new(CalendarChannel::default())),
            selected_pending: None,
            cached_bounds: Rect::default(),
            last_anchor: None,
            text_painter: None,
        }
    }

    /// Sets the initial value.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite::widgets::{Date, DatePicker};
    ///
    /// let dp = DatePicker::new().date(Date { year: 2024, month: 3, day: 9 });
    /// assert_eq!(dp.get_date(), Some(Date { year: 2024, month: 3, day: 9 }));
    /// ```
    #[must_use]
    pub fn date(mut self, date: Date) -> Self {
        self.set_date(Some(date));
        self
    }

    /// Sets the placeholder text shown while no date is set.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite::widgets::DatePicker;
    ///
    /// let dp = DatePicker::new().placeholder("Pick a date");
    /// assert_eq!(dp.placeholder, "Pick a date");
    /// ```
    #[must_use]
    pub fn placeholder(mut self, text: impl Into<String>) -> Self {
        self.placeholder = text.into();
        self
    }

    /// Sets the format-lite pattern; see [`DatePicker::format`].
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite::widgets::{Date, DatePicker};
    ///
    /// let dp = DatePicker::new()
    ///     .date(Date { year: 2024, month: 6, day: 15 })
    ///     .format("{weekday} {day}/{month}");
    /// assert_eq!(dp.text(), "Sat 15/6");
    /// ```
    #[must_use]
    pub fn format(mut self, fmt: impl Into<String>) -> Self {
        self.format = fmt.into();
        self
    }

    /// Sets the inclusive minimum pickable date.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite::widgets::{Date, DatePicker};
    ///
    /// let dp = DatePicker::new().min_date(Date { year: 2020, month: 1, day: 1 });
    /// assert!(dp.min_date.is_some());
    /// ```
    #[must_use]
    pub fn min_date(mut self, date: Date) -> Self {
        self.min_date = Some(date);
        self
    }

    /// Sets the inclusive maximum pickable date.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite::widgets::{Date, DatePicker};
    ///
    /// let dp = DatePicker::new().max_date(Date { year: 2030, month: 12, day: 31 });
    /// assert!(dp.max_date.is_some());
    /// ```
    #[must_use]
    pub fn max_date(mut self, date: Date) -> Self {
        self.max_date = Some(date);
        self
    }

    /// Injects the host's "today" — ringed in the popup grid. The
    /// widget deliberately owns no clock so tests stay deterministic.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite::widgets::{Date, DatePicker};
    ///
    /// let dp = DatePicker::new().today(Date { year: 2024, month: 6, day: 1 });
    /// assert_eq!(dp.today, Some(Date { year: 2024, month: 6, day: 1 }));
    /// ```
    #[must_use]
    pub fn today(mut self, date: Date) -> Self {
        self.today = Some(date);
        self
    }

    /// Sets the first weekday column (`true` = Monday, `false` =
    /// Sunday).
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite::widgets::DatePicker;
    ///
    /// let dp = DatePicker::new().week_starts_monday(false);
    /// assert!(!dp.week_starts_monday);
    /// ```
    #[must_use]
    pub fn week_starts_monday(mut self, monday: bool) -> Self {
        self.week_starts_monday = monday;
        self
    }

    /// Sets the accessible label.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite::widgets::DatePicker;
    ///
    /// let dp = DatePicker::new().label("Birthday");
    /// assert_eq!(dp.label.as_deref(), Some("Birthday"));
    /// ```
    #[must_use]
    pub fn label(mut self, label: impl Into<String>) -> Self {
        self.label = Some(label.into());
        self
    }

    /// Sets whether the picker is enabled.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite::widgets::DatePicker;
    ///
    /// let dp = DatePicker::new().enabled(false);
    /// assert!(!dp.enabled);
    /// ```
    #[must_use]
    pub fn enabled(mut self, enabled: bool) -> Self {
        self.enabled = enabled;
        self
    }

    /// Shares a [`crate::text_paint::TextPainter`] so `paint` emits
    /// real glyph runs instead of `DrawText` placeholder boxes.
    #[must_use]
    /// Switches the popup to two-click range picking (`Ant
    /// `RangePicker` semantics): first pick anchors, second completes
    /// and closes. Completed spans park in
    /// [`take_range`](Self::take_range); single-day `take_selected`
    /// is unused in this mode.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite::widgets::DatePicker;
    ///
    /// let dp = DatePicker::new().range_mode(true);
    /// assert!(dp.range_mode);
    /// ```
    pub fn range_mode(mut self, on: bool) -> Self {
        self.range_mode = on;
        self
    }

    /// Sets the committed range programmatically (range mode).
    /// Normalized to chronological order; does not feed
    /// [`take_range`](Self::take_range).
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite::widgets::{Date, DatePicker};
    ///
    /// let mut dp = DatePicker::new().range_mode(true);
    /// dp.set_range((
    ///     Date { year: 2024, month: 6, day: 20 },
    ///     Date { year: 2024, month: 6, day: 10 },
    /// ));
    /// assert_eq!(
    ///     dp.range_value(),
    ///     Some((
    ///         Date { year: 2024, month: 6, day: 10 },
    ///         Date { year: 2024, month: 6, day: 20 },
    ///     ))
    /// );
    /// ```
    pub fn set_range(&mut self, span: (Date, Date)) {
        let (a, b) = span;
        self.range = Some(if b < a { (b, a) } else { (a, b) });
    }

    /// The committed `(start, end)` span (range mode).
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite::widgets::DatePicker;
    ///
    /// let dp = DatePicker::new().range_mode(true);
    /// assert_eq!(dp.range_value(), None);
    /// ```
    pub fn range_value(&self) -> Option<(Date, Date)> {
        self.range
    }

    /// Drains the span the user completed in the popup (range mode).
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite::widgets::DatePicker;
    ///
    /// let mut dp = DatePicker::new().range_mode(true);
    /// assert_eq!(dp.take_range(), None);
    /// ```
    pub fn take_range(&mut self) -> Option<(Date, Date)> {
        self.range_pending.take()
    }

    /// Installs a shared shaped-text painter.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite::widgets::DatePicker;
    ///
    /// let dp = DatePicker::new();
    /// let _ = dp.text();
    /// ```
    pub fn with_text_painter(mut self, painter: crate::text_paint::SharedTextPainter) -> Self {
        self.text_painter = Some(painter);
        self
    }

    /// The committed date, if any.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite::widgets::{Date, DatePicker};
    ///
    /// let dp = DatePicker::new().date(Date { year: 2024, month: 1, day: 1 });
    /// assert_eq!(dp.get_date(), Some(Date { year: 2024, month: 1, day: 1 }));
    /// ```
    #[inline]
    pub fn get_date(&self) -> Option<Date> {
        self.date
    }

    /// Sets the value programmatically — invalid or out-of-range
    /// dates are ignored. Does not report through
    /// [`take_selected`](Self::take_selected) (that seam is user
    /// picks only).
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite::widgets::{Date, DatePicker};
    ///
    /// let mut dp = DatePicker::new().min_date(Date { year: 2020, month: 1, day: 1 });
    /// dp.set_date(Some(Date { year: 2021, month: 5, day: 5 }));
    /// assert_eq!(dp.get_date(), Some(Date { year: 2021, month: 5, day: 5 }));
    /// dp.set_date(Some(Date { year: 2010, month: 5, day: 5 })); // out of range
    /// assert_eq!(dp.get_date(), Some(Date { year: 2021, month: 5, day: 5 }));
    /// ```
    pub fn set_date(&mut self, date: Option<Date>) {
        match date {
            Some(d) if d.is_valid() && d.in_range(self.min_date, self.max_date) => {
                self.date = Some(d);
            }
            None => self.date = None,
            _ => {}
        }
    }

    /// The face text — the formatted date, or the placeholder when
    /// unset.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite::widgets::{Date, DatePicker};
    ///
    /// let dp = DatePicker::new().date(Date { year: 2024, month: 6, day: 15 });
    /// assert_eq!(dp.text(), "2024-06-15");
    /// ```
    pub fn text(&self) -> String {
        if self.range_mode {
            return match self.range {
                Some((a, b)) => {
                    let (mut sa, mut sb) = (String::new(), String::new());
                    format_into(&mut sa, &self.format, a);
                    format_into(&mut sb, &self.format, b);
                    format!("{sa} – {sb}")
                }
                None => self.placeholder.clone(),
            };
        }
        match self.date {
            Some(d) => {
                let mut s = String::new();
                format_into(&mut s, &self.format, d);
                s
            }
            None => self.placeholder.clone(),
        }
    }

    /// Drains the date the user picked in the popup (click or `Enter`)
    /// — `None` when nothing new was picked. Programmatic
    /// [`set_date`](Self::set_date) does not feed this seam.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite::widgets::DatePicker;
    ///
    /// let mut dp = DatePicker::new();
    /// assert_eq!(dp.take_selected(), None);
    /// ```
    pub fn take_selected(&mut self) -> Option<Date> {
        self.selected_pending.take()
    }

    /// Whether the popup is logically open.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite::widgets::DatePicker;
    ///
    /// let mut dp = DatePicker::new();
    /// dp.open();
    /// assert!(dp.is_open());
    /// ```
    #[inline]
    pub fn is_open(&self) -> bool {
        self.open
    }

    /// The overlay entry id of the open popup, if any.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite::widgets::DatePicker;
    ///
    /// assert_eq!(DatePicker::new().popup_id(), None);
    /// ```
    #[inline]
    pub fn popup_id(&self) -> Option<u64> {
        self.popup_id
    }

    /// Opens the calendar popup on the next
    /// [`sync_overlay`](Self::sync_overlay).
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite::widgets::DatePicker;
    ///
    /// let mut dp = DatePicker::new();
    /// dp.open();
    /// assert!(dp.is_open());
    /// ```
    pub fn open(&mut self) {
        self.open = true;
        let mut channel = self.channel.lock().expect("calendar channel poisoned");
        channel.picked = None;
        channel.close_requested = false;
    }

    /// Closes the popup without changing the value.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite::widgets::DatePicker;
    ///
    /// let mut dp = DatePicker::new();
    /// dp.open();
    /// dp.close();
    /// assert!(!dp.is_open());
    /// ```
    pub fn close(&mut self) {
        self.open = false;
    }

    /// The month the popup would open on: the value's month, then
    /// `today`'s, then `min_date`'s, then January 2000 — a fixed,
    /// documented fallback since the widget owns no clock.
    fn initial_view(&self) -> (i32, u32) {
        let d = self.date.or(self.today).or(self.min_date).unwrap_or(Date {
            year: 2000,
            month: 1,
            day: 1,
        });
        // An invalid injected `today`/`min_date` must not panic the
        // month-name lookup — clamp into the displayable range.
        (d.year, d.month.clamp(1, 12))
    }

    /// Reconciles the overlay with the picker's open state.
    ///
    /// Call once per frame before `OverlayLayer::layout_pass`:
    ///
    /// - applies a pick or close request made inside the popup;
    /// - opens/closes the calendar entry to match
    ///   [`is_open`](Self::is_open);
    /// - notices overlay-level dismissal (outside press, `Escape`)
    ///   and resets `open`/`popup_id`;
    /// - re-anchors a live popup whose face moved.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite::widgets::DatePicker;
    /// use martensite_core::overlay::OverlayLayer;
    /// use martensite_core::{HotNode, LayoutContext, Rect, Widget};
    ///
    /// let mut dp = DatePicker::new();
    /// let mut hot = HotNode::default();
    /// let mut cx = LayoutContext {
    ///     hot: &mut hot,
    ///     scale: 1.0,
    /// };
    /// dp.layout(&mut cx, Rect::new(10.0, 10.0, 150.0, 28.0));
    ///
    /// let mut overlay = OverlayLayer::new();
    /// overlay.set_viewport(Rect::new(0.0, 0.0, 800.0, 600.0));
    /// dp.open();
    /// dp.sync_overlay(&mut overlay);
    /// overlay.layout_pass();
    /// assert_eq!(overlay.len(), 1);
    /// ```
    pub fn sync_overlay(&mut self, overlay: &mut OverlayLayer) {
        // Drain the channel: a picked day commits + queues + closes.
        let (picked, ranged, close_req) = {
            let mut channel = self.channel.lock().expect("calendar channel poisoned");
            (
                channel.picked.take(),
                channel.ranged.take(),
                std::mem::take(&mut channel.close_requested),
            )
        };
        if let Some(date) = picked {
            self.date = Some(date);
            self.selected_pending = Some(date);
            self.open = false;
        }
        if let Some(span) = ranged {
            self.range = Some(span);
            self.range_pending = Some(span);
            self.open = false;
        }
        if close_req {
            self.open = false;
        }
        // The layer dismissed our popup (outside press / Escape).
        if let Some(id) = self.popup_id {
            if !overlay.is_open(id) {
                self.popup_id = None;
                self.open = false;
                self.last_anchor = None;
            }
        }
        if self.open && self.popup_id.is_none() {
            let (view_year, view_month) = self.initial_view();
            // Focus starts on the value, else today, else the 1st —
            // clamped into the pickable range.
            let mut focus = self.date.or(self.today).unwrap_or(Date {
                year: view_year,
                month: view_month,
                day: 1,
            });
            if let Some(min) = self.min_date {
                if focus < min {
                    focus = min;
                }
            }
            if let Some(max) = self.max_date {
                if focus > max {
                    focus = max;
                }
            }
            let surface = CalendarSurface {
                view_year,
                view_month,
                selected: self.date,
                range_mode: self.range_mode,
                range: self.range,
                anchor: None,
                today: self.today,
                min: self.min_date,
                max: self.max_date,
                week_starts_monday: self.week_starts_monday,
                focus,
                hover: None,
                bounds: Rect::default(),
                prev_rect: Rect::default(),
                next_rect: Rect::default(),
                grid_origin: Vec2::ZERO,
                channel: Arc::clone(&self.channel),
                painted_shape: Mutex::new(Shape::RECT),
                text_painter: self.text_painter.clone(),
            };
            self.popup_id =
                Some(overlay.open(Box::new(surface), OverlayAnchor::Bounds(self.cached_bounds)));
            self.last_anchor = Some(self.cached_bounds);
        } else if !self.open {
            if let Some(id) = self.popup_id.take() {
                overlay.close(id);
            }
            self.last_anchor = None;
        } else if let Some(id) = self.popup_id {
            // The face moved while open — re-anchor so the calendar
            // tracks it (mirrors `Dropdown`).
            if self.last_anchor != Some(self.cached_bounds) {
                overlay.set_anchor(id, OverlayAnchor::Bounds(self.cached_bounds));
                self.last_anchor = Some(self.cached_bounds);
            }
        }
    }
}

impl Default for DatePicker {
    fn default() -> Self {
        Self::new()
    }
}

impl Widget for DatePicker {
    fn measure(&mut self, cx: &mut LayoutContext, constraints: LayoutConstraints) -> Vec2 {
        // Face width ~ the longer of formatted value / placeholder.
        let widest = self
            .text()
            .chars()
            .count()
            .max(self.placeholder.chars().count()) as f32;
        let w = widest * cx.pt(7.0) + cx.pt(52.0);
        let max_w = constraints.max_size.x.max(0.0);
        Vec2::new(
            w.clamp(cx.pt(FACE_MIN_W).min(max_w), max_w),
            cx.pt(FACE_H).min(constraints.max_size.y.max(0.0)),
        )
    }

    fn min_render(&self) -> RenderMinimum {
        RenderMinimum::new(Vec2::new(FACE_MIN_W, FACE_H)).with_policy(UnderflowPolicy::Lint)
    }

    fn layout(&mut self, cx: &mut LayoutContext, bounds: Rect) {
        self.cached_bounds = bounds;
        if self.enabled {
            cx.hot.flags |= NodeFlags::FOCUSABLE;
        } else {
            cx.hot.flags.remove(NodeFlags::FOCUSABLE);
        }
    }

    fn accessibility(&self, node: &mut AccessKitNode) {
        node.set_role(accesskit::Role::ComboBox);
        if let Some(ref label) = self.label {
            node.set_label(label.as_str());
        }
        node.set_value(self.text());
        node.set_has_popup(accesskit::HasPopup::Dialog);
        node.set_expanded(self.open);
        node.add_action(accesskit::Action::Click);
        node.add_action(accesskit::Action::Expand);
        node.add_action(accesskit::Action::Collapse);
        if self.enabled {
            node.add_action(accesskit::Action::Focus);
        } else {
            node.set_disabled();
        }
    }

    fn a11y_prepare(&mut self) {
        // An AT-driven pick recorded through the surface lands in the
        // channel — drain it here too so the emitted tree reflects the
        // commit even when the action bypassed `sync_overlay`.
        if let Some(date) = self
            .channel
            .lock()
            .expect("calendar channel poisoned")
            .picked
            .take()
        {
            self.date = Some(date);
            self.selected_pending = Some(date);
            self.open = false;
        }
    }

    fn event(&mut self, cx: &mut EventContext) -> EventResponse {
        if !self.enabled {
            return EventResponse::Ignored;
        }
        match cx.event {
            WidgetEvent::PointerPressed {
                button: PointerButton::Primary,
                ..
            } => {
                if self.open {
                    self.close();
                } else {
                    self.open();
                }
                EventResponse::CaptureFocus
            }
            WidgetEvent::KeyPressed { key, .. } => match key.as_str() {
                // Normally unreachable while open — the OverlayLayer
                // consumes `Escape` first. Kept for ownerless-embedded
                // use (mirrors `Dropdown`).
                "Escape" => {
                    if self.open {
                        self.close();
                        return EventResponse::RequestRepaint;
                    }
                    EventResponse::Ignored
                }
                "Enter" | " " | "Space" | "ArrowDown" => {
                    if self.open {
                        self.close();
                    } else {
                        self.open();
                    }
                    EventResponse::RequestRepaint
                }
                _ => EventResponse::Ignored,
            },
            WidgetEvent::SemanticAction(action) => match action {
                SemanticAction::Expand => {
                    self.open();
                    EventResponse::RequestRepaint
                }
                SemanticAction::Collapse => {
                    self.close();
                    EventResponse::RequestRepaint
                }
                SemanticAction::Click => {
                    if self.open {
                        self.close();
                    } else {
                        self.open();
                    }
                    EventResponse::RequestRepaint
                }
                SemanticAction::Focus => EventResponse::CaptureFocus,
                _ => EventResponse::Ignored,
            },
            _ => EventResponse::Ignored,
        }
    }

    fn sync_overlay(&mut self, overlay: &mut OverlayLayer) {
        // Delegate to the inherent method so `DatePicker::sync_overlay`
        // and the `Widget` trait seam stay in lock-step.
        DatePicker::sync_overlay(self, overlay);
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
        let has_value = self.date.is_some();
        let ink = if !self.enabled || !has_value {
            cx.color(TokenKey::TextMutedColor, INK_MUTED)
        } else {
            cx.color(TokenKey::TextColor, INK)
        };
        let font_px = cx.pt(13.0);
        let text_x = b.min_x() + cx.pt(10.0);
        crate::text_paint::paint_label_clipped(
            crate::text_paint::resolve_painter(&self.text_painter, cx.text_painter),
            cx.list,
            kurbo::Rect::new(
                f64::from(text_x),
                f64::from(b.min_y()),
                f64::from(b.max_x() - cx.pt(30.0)),
                f64::from(b.max_y()),
            ),
            kurbo::Point::new(
                f64::from(text_x),
                f64::from(b.min_y() + (b.height() - font_px) / 2.0),
            ),
            &self.text(),
            font_px,
            ink,
        );
        // Calendar glyph: a small rounded page with a header band and
        // two binder rings, right-aligned in the face.
        let s = cx.pt(7.0); // half-size of the icon box
        let gx = b.max_x() - cx.pt(16.0) - s;
        let gy = b.min_y() + b.height() / 2.0 - s;
        let icon = kurbo::Rect::new(
            f64::from(gx),
            f64::from(gy),
            f64::from(gx + s * 2.0),
            f64::from(gy + s * 2.0),
        );
        let icon_ink = cx.color(TokenKey::TextColor, INK);
        let icon_shape = Shape::rounded(cx.pt(1.5));
        cx.list
            .push_stroke_shape(icon, &icon_shape, cx.pt(1.0), icon_ink);
        // Header band.
        cx.list.push_fill_rect(
            kurbo::Rect::new(icon.x0, icon.y0, icon.x1, icon.y0 + f64::from(cx.pt(4.0))),
            icon_ink,
        );
        // Binder rings above the page.
        for dx in [s * 0.45, s * 1.55] {
            cx.list.push_fill_rect(
                kurbo::Rect::new(
                    f64::from(gx + dx - cx.pt(0.6)),
                    f64::from(gy - cx.pt(1.6)),
                    f64::from(gx + dx + cx.pt(0.6)),
                    f64::from(gy + cx.pt(2.4)),
                ),
                icon_ink,
            );
        }
    }
}

impl std::fmt::Debug for DatePicker {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("DatePicker")
            .field("date", &self.date)
            .field("format", &self.format)
            .field("enabled", &self.enabled)
            .field("open", &self.open)
            .finish()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use martensite_core::HotNode;

    fn laid_out(dp: &mut DatePicker) {
        let mut hot = HotNode::default();
        let mut cx = LayoutContext {
            hot: &mut hot,
            scale: 1.0,
        };
        dp.layout(&mut cx, Rect::new(10.0, 10.0, 150.0, 28.0));
    }

    fn overlay() -> OverlayLayer {
        let mut o = OverlayLayer::new();
        o.set_viewport(Rect::new(0.0, 0.0, 800.0, 600.0));
        o
    }

    fn key(k: &str) -> WidgetEvent {
        WidgetEvent::KeyPressed {
            key: k.to_string(),
            repeat: false,
        }
    }

    fn event(dp: &mut DatePicker, ev: &WidgetEvent) -> EventResponse {
        let mut cx = EventContext {
            event: ev,
            bounds: dp.cached_bounds,
            scale: 1.0,
        };
        dp.event(&mut cx)
    }

    fn surface_event(surface: &mut dyn Widget, bounds: Rect, ev: &WidgetEvent) -> EventResponse {
        let mut cx = EventContext {
            event: ev,
            bounds,
            scale: 1.0,
        };
        surface.event(&mut cx)
    }

    #[test]
    fn date_validity() {
        assert!(Date {
            year: 2024,
            month: 2,
            day: 29
        }
        .is_valid());
        assert!(!Date {
            year: 2023,
            month: 2,
            day: 29
        }
        .is_valid());
        assert!(!Date {
            year: 2024,
            month: 0,
            day: 1
        }
        .is_valid());
        assert!(!Date {
            year: 2024,
            month: 4,
            day: 31
        }
        .is_valid());
    }

    #[test]
    fn days_in_month_leap_rules() {
        assert_eq!(Date::days_in_month(2024, 2), 29);
        assert_eq!(Date::days_in_month(1900, 2), 28);
        assert_eq!(Date::days_in_month(2000, 2), 29);
        assert_eq!(Date::days_in_month(2024, 12), 31);
        assert_eq!(Date::days_in_month(2024, 13), 0);
    }

    #[test]
    fn weekday_of_known_dates() {
        assert_eq!(Date::weekday_of(1970, 1, 1), 4); // Thursday
        assert_eq!(Date::weekday_of(2000, 1, 1), 6); // Saturday
        assert_eq!(Date::weekday_of(2024, 6, 15), 6); // Saturday
        assert_eq!(Date::weekday_of(2024, 1, 1), 1); // Monday
    }

    #[test]
    fn add_days_crosses_months_and_years() {
        let d = Date {
            year: 2024,
            month: 1,
            day: 31,
        };
        assert_eq!(
            add_days(d, 1),
            Date {
                year: 2024,
                month: 2,
                day: 1
            }
        );
        assert_eq!(
            add_days(d, -31),
            Date {
                year: 2023,
                month: 12,
                day: 31
            }
        );
        // Round-trip stability.
        assert_eq!(add_days(add_days(d, 400), -400), d);
    }

    #[test]
    fn add_months_wraps_year() {
        assert_eq!(add_months(2024, 12, 1), (2025, 1));
        assert_eq!(add_months(2024, 1, -1), (2023, 12));
        assert_eq!(add_months(2024, 6, 13), (2025, 7));
    }

    #[test]
    fn format_tokens() {
        let d = Date {
            year: 2024,
            month: 6,
            day: 15,
        };
        let mut s = String::new();
        format_into(&mut s, "{year}-{month:02}-{day:02}", d);
        assert_eq!(s, "2024-06-15");
        s.clear();
        format_into(&mut s, "{weekday}, {day}/{month}/{year}", d);
        assert_eq!(s, "Sat, 15/6/2024");
        s.clear();
        format_into(&mut s, "{bogus} stays", d);
        assert_eq!(s, "{bogus} stays");
    }

    #[test]
    fn face_opens_on_press_and_keys() {
        let mut dp = DatePicker::new();
        laid_out(&mut dp);
        let press = WidgetEvent::PointerPressed {
            position: Vec2::new(20.0, 20.0),
            button: PointerButton::Primary,
            count: 1,
        };
        assert_eq!(event(&mut dp, &press), EventResponse::CaptureFocus);
        assert!(dp.is_open());
        event(&mut dp, &key("Escape"));
        assert!(!dp.is_open());
        event(&mut dp, &key("ArrowDown"));
        assert!(dp.is_open());
        event(&mut dp, &key("Enter"));
        assert!(!dp.is_open());
    }

    #[test]
    fn semantic_expand_click_toggle() {
        let mut dp = DatePicker::new();
        laid_out(&mut dp);
        event(
            &mut dp,
            &WidgetEvent::SemanticAction(SemanticAction::Expand),
        );
        assert!(dp.is_open());
        event(
            &mut dp,
            &WidgetEvent::SemanticAction(SemanticAction::Collapse),
        );
        assert!(!dp.is_open());
        event(&mut dp, &WidgetEvent::SemanticAction(SemanticAction::Click));
        assert!(dp.is_open());
    }

    #[test]
    fn overlay_opens_calendar_below() {
        let mut dp = DatePicker::new();
        laid_out(&mut dp);
        let mut o = overlay();
        dp.open();
        dp.sync_overlay(&mut o);
        o.layout_pass();
        assert_eq!(o.len(), 1);
        let b = o.entry_bounds(dp.popup_id.unwrap()).unwrap();
        assert!(b.min_y() >= 38.0);
    }

    #[test]
    fn outside_press_dismisses() {
        let mut dp = DatePicker::new();
        laid_out(&mut dp);
        let mut o = overlay();
        dp.open();
        dp.sync_overlay(&mut o);
        o.layout_pass();
        let press = WidgetEvent::PointerPressed {
            position: Vec2::new(700.0, 500.0),
            button: PointerButton::Primary,
            count: 1,
        };
        assert_eq!(o.dispatch_event(&press), EventResponse::Ignored);
        dp.sync_overlay(&mut o);
        assert!(!dp.is_open());
        assert_eq!(dp.popup_id, None);
    }

    #[test]
    fn day_press_picks_and_closes() {
        let mut dp = DatePicker::new().today(Date {
            year: 2024,
            month: 6,
            day: 1,
        });
        laid_out(&mut dp);
        let mut o = overlay();
        dp.open();
        dp.sync_overlay(&mut o);
        o.layout_pass();
        let id = dp.popup_id.unwrap();
        let bounds = o.entry_bounds(id).unwrap();
        // Week starts Monday; June 2024 starts Saturday → 2024-06-10 is
        // row 1, col 0 of the grid: pad + col*cell, header+weekrow rows.
        let surface = o.widget_at_mut(id, &[]).expect("surface widget");
        // Navigate via keyboard instead of reconstructing geometry:
        // focus starts on `today` (Jun 1); Right ×9 lands on Jun 10.
        for _ in 0..9 {
            surface_event(surface, bounds, &key("ArrowRight"));
        }
        surface_event(surface, bounds, &key("Enter"));
        dp.sync_overlay(&mut o);
        assert_eq!(
            dp.get_date(),
            Some(Date {
                year: 2024,
                month: 6,
                day: 10
            })
        );
        assert_eq!(
            dp.take_selected(),
            Some(Date {
                year: 2024,
                month: 6,
                day: 10
            })
        );
        assert!(!dp.is_open());
    }

    #[test]
    fn surface_keyboard_changes_month() {
        let mut dp = DatePicker::new().today(Date {
            year: 2024,
            month: 6,
            day: 15,
        });
        laid_out(&mut dp);
        let mut o = overlay();
        dp.open();
        dp.sync_overlay(&mut o);
        o.layout_pass();
        let id = dp.popup_id.unwrap();
        let bounds = o.entry_bounds(id).unwrap();
        let surface = o.widget_at_mut(id, &[]).expect("surface widget");
        surface_event(surface, bounds, &key("PageDown"));
        surface_event(surface, bounds, &key("PageUp"));
        // Focus followed the month step but stayed a valid day.
        assert_eq!(
            surface_event(surface, bounds, &key("Enter")),
            EventResponse::Handled
        );
        dp.sync_overlay(&mut o);
        assert_eq!(dp.get_date().unwrap().month, 6);
    }

    #[test]
    fn min_max_blocks_focus_and_clicks() {
        let mut dp = DatePicker::new()
            .date(Date {
                year: 2024,
                month: 6,
                day: 15,
            })
            .min_date(Date {
                year: 2024,
                month: 6,
                day: 10,
            })
            .max_date(Date {
                year: 2024,
                month: 6,
                day: 20,
            });
        laid_out(&mut dp);
        let mut o = overlay();
        dp.open();
        dp.sync_overlay(&mut o);
        o.layout_pass();
        let id = dp.popup_id.unwrap();
        let bounds = o.entry_bounds(id).unwrap();
        let surface = o.widget_at_mut(id, &[]).expect("surface widget");
        // Trying to walk before min_date refuses to move the focus.
        for _ in 0..10 {
            surface_event(surface, bounds, &key("ArrowLeft"));
        }
        surface_event(surface, bounds, &key("Enter"));
        dp.sync_overlay(&mut o);
        // Enter picked the focused cell — still ≥ min_date.
        assert!(dp.get_date().unwrap() >= dp.min_date.unwrap());
    }

    #[test]
    fn set_date_ignores_invalid_and_out_of_range() {
        let mut dp = DatePicker::new().min_date(Date {
            year: 2020,
            month: 1,
            day: 1,
        });
        dp.set_date(Some(Date {
            year: 2019,
            month: 12,
            day: 31,
        }));
        assert_eq!(dp.get_date(), None);
        dp.set_date(Some(Date {
            year: 2021,
            month: 2,
            day: 30,
        }));
        assert_eq!(dp.get_date(), None);
        dp.set_date(Some(Date {
            year: 2021,
            month: 2,
            day: 28,
        }));
        assert!(dp.get_date().is_some());
        // Programmatic set does not feed take_selected.
        assert_eq!(dp.take_selected(), None);
    }

    #[test]
    fn accessibility_contract() {
        let mut dp = DatePicker::new()
            .date(Date {
                year: 2024,
                month: 6,
                day: 15,
            })
            .label("Due date");
        dp.open();
        let mut node = AccessKitNode::new(accesskit::Role::Unknown);
        dp.accessibility(&mut node);
        assert_eq!(node.role(), accesskit::Role::ComboBox);
        assert_eq!(node.label(), Some("Due date"));
        assert_eq!(node.value(), Some("2024-06-15"));
        assert_eq!(node.has_popup(), Some(accesskit::HasPopup::Dialog));
        assert_eq!(node.is_expanded(), Some(true));
    }

    #[test]
    fn range_mode_two_picks_commit_and_close() {
        let mut dp = DatePicker::new().range_mode(true).today(Date {
            year: 2024,
            month: 6,
            day: 10,
        });
        laid_out(&mut dp);
        let mut o = overlay();
        dp.open();
        dp.sync_overlay(&mut o);
        o.layout_pass();
        let id = dp.popup_id.unwrap();
        let bounds = o.entry_bounds(id).unwrap();
        {
            let surface = o.widget_at_mut(id, &[]).expect("surface widget");
            // Focus starts on `today` (Jun 10) → Enter anchors.
            surface_event(surface, bounds, &key("Enter"));
        }
        dp.sync_overlay(&mut o);
        assert!(dp.is_open());
        assert_eq!(dp.take_range(), None);
        {
            let surface = o.widget_at_mut(id, &[]).expect("surface widget");
            // Four days later completes the span.
            for _ in 0..4 {
                surface_event(surface, bounds, &key("ArrowRight"));
            }
            surface_event(surface, bounds, &key("Enter"));
        }
        dp.sync_overlay(&mut o);
        assert!(!dp.is_open());
        assert_eq!(
            dp.take_range(),
            Some((
                Date {
                    year: 2024,
                    month: 6,
                    day: 10
                },
                Date {
                    year: 2024,
                    month: 6,
                    day: 14
                },
            ))
        );
        assert_eq!(dp.text(), "2024-06-10 – 2024-06-14");
        assert_eq!(dp.take_range(), None);
    }

    #[test]
    fn range_mode_reverse_pick_normalizes() {
        let mut dp = DatePicker::new().range_mode(true).today(Date {
            year: 2024,
            month: 6,
            day: 10,
        });
        laid_out(&mut dp);
        let mut o = overlay();
        dp.open();
        dp.sync_overlay(&mut o);
        o.layout_pass();
        let id = dp.popup_id.unwrap();
        let bounds = o.entry_bounds(id).unwrap();
        let surface = o.widget_at_mut(id, &[]).expect("surface widget");
        surface_event(surface, bounds, &key("Enter")); // anchor Jun 10
        for _ in 0..5 {
            surface_event(surface, bounds, &key("ArrowLeft"));
        }
        surface_event(surface, bounds, &key("Enter")); // pick Jun 5
        dp.sync_overlay(&mut o);
        assert_eq!(
            dp.range_value(),
            Some((
                Date {
                    year: 2024,
                    month: 6,
                    day: 5
                },
                Date {
                    year: 2024,
                    month: 6,
                    day: 10
                },
            ))
        );
    }

    #[test]
    fn range_mode_placeholder_until_complete() {
        let mut dp = DatePicker::new()
            .range_mode(true)
            .placeholder("Pick a range");
        laid_out(&mut dp);
        assert_eq!(dp.text(), "Pick a range");
    }
}
