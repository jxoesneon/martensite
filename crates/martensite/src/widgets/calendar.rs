//! `Calendar` — always-visible month-grid date selector.
//!
//! `DatePicker` hides its grid behind a text-field popup; `Calendar`
//! renders the month panel inline — the Ant `Calendar` panel /
//! `QCalendarWidget` / GTK `Calendar` shape. The header offers `‹`/`›`
//! month navigation beside the `Month YYYY` caption; a `Su`–`Sa` (or
//! `Mo`–`Su`) weekday row heads a fixed 6×7 day grid so the widget's
//! size never shifts between months.
//!
//! Out-of-month cells render dimmed but stay clickable (selecting one
//! navigates the displayed month). `today` gets an accent ring, the
//! selected date an accent fill, and out-of-`min`/`max`-range cells are
//! inert and dimmed. Pointer clicks select; arrows move a focus cell
//! (crossing a month boundary shifts the displayed month),
//! `PageUp`/`PageDown` step a month, and `Enter`/`Space` select the
//! focus cell. Selections park in [`Calendar::take_selected`].
//!
//! [`CalendarSelection::Range`] switches clicks to the Ant
//! `RangePicker` model: first click anchors, second completes (the
//! pair normalizes to `start <= end`), a third re-anchors. The
//! committed pair parks in [`Calendar::take_range`], interior cells
//! get a hover wash, and hovering while anchored previews the span.
//!
//! # Examples
//!
//! ```
//! use martensite::widgets::calendar::Calendar;
//! use martensite::widgets::date_picker::Date;
//!
//! let mut cal = Calendar::new()
//!     .date(Date { year: 2024, month: 6, day: 15 })
//!     .today(Date { year: 2024, month: 6, day: 1 });
//! cal.set_displayed_month(2024, 6);
//! assert_eq!(cal.selected(), Some(Date { year: 2024, month: 6, day: 15 }));
//! ```

use accesskit::Node as AccessKitNode;
use glam::Vec2;
use martensite_core::{
    EventContext, EventResponse, LayoutConstraints, LayoutContext, PaintContext, Rect,
    RenderMinimum, UnderflowPolicy, Widget, WidgetEvent,
};
use martensite_theme::TokenKey;

use crate::widgets::date_picker::Date;

/// Selected / today / focus ink.
const ACCENT: TokenKey = TokenKey::AccentColor;
/// Body text.
const TEXT: TokenKey = TokenKey::TextColor;
/// Dimmed ink (out-of-month and out-of-range cells).
const MUTED: TokenKey = TokenKey::TextMutedColor;
/// Hover wash.
const HOVER: TokenKey = TokenKey::SecondaryColor;
/// Header strip height (logical points).
const HEADER_H: f32 = 30.0;
/// Weekday-label row height (logical points).
const WEEKDAY_H: f32 = 20.0;
/// Grid rows always shown (stable size across months).
const GRID_ROWS: usize = 6;
/// Chevron hit-zone width inside the header (logical points).
const NAV_W: f32 = 28.0;

/// English month names indexed `month - 1`.
const MONTHS: [&str; 12] = [
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

/// English weekday abbreviations, Sunday-first.
const WEEKDAYS_SUN: [&str; 7] = ["Su", "Mo", "Tu", "We", "Th", "Fr", "Sa"];
/// English weekday abbreviations, Monday-first.
const WEEKDAYS_MON: [&str; 7] = ["Mo", "Tu", "We", "Th", "Fr", "Sa", "Su"];

/// What a click selects.
///
/// # Examples
///
/// ```
/// use martensite::widgets::calendar::CalendarSelection;
///
/// assert_eq!(CalendarSelection::default(), CalendarSelection::Day);
/// ```
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum CalendarSelection {
    /// A single day (the default) — `take_selected`.
    #[default]
    Day,
    /// A start→end range — first click anchors, second completes
    /// (either order; the pair normalizes), parking `(start, end)`
    /// in `take_range`. A third click re-anchors.
    Range,
}

/// An always-visible month-grid date selector — see the module docs.
///
/// `Calendar` is a leaf widget: it paints its own chrome (header, day
/// cells) and reports no children.
///
/// # Examples
///
/// ```
/// use martensite::widgets::calendar::Calendar;
/// use martensite::core::Widget;
///
/// let mut cal = Calendar::new();
/// assert_eq!(cal.child_count(), 0);
/// ```
pub struct Calendar {
    label: String,
    enabled: bool,
    /// Selected date (accent-filled cell).
    selected: Option<Date>,
    /// Click semantics — single day or range.
    mode: CalendarSelection,
    /// Committed range endpoints, normalized `start <= end`.
    range_start: Option<Date>,
    range_end: Option<Date>,
    /// First range click awaiting its partner.
    range_anchor: Option<Date>,
    /// Parked `(start, end)` for `take_range`.
    range_pending: Option<(Date, Date)>,
    /// Date that gets the accent ring.
    today: Option<Date>,
    /// Pickable range; out-of-range cells are inert.
    min: Option<Date>,
    max: Option<Date>,
    /// Displayed month (`year`, `month 1..=12`).
    view: (i32, u32),
    /// `true` when the weekday row starts on Monday.
    week_starts_monday: bool,
    /// Keyboard focus cell (a real date — arrows cross months freely).
    focus_date: Option<Date>,
    /// Parked selection for `take_selected`.
    pending: Option<Date>,
    /// Hover state: a day-cell index (`0..42`) or a nav zone.
    hover: Option<Hit>,
    /// Cells from the last layout, in widget-local coordinates.
    cells: Vec<Cell>,
    text_painter: Option<crate::text_paint::SharedTextPainter>,
}

/// A laid-out day cell.
#[derive(Clone, Copy)]
struct Cell {
    /// The cell rect in widget-local coordinates.
    rect: Rect,
    /// The real date this cell shows (out-of-month included).
    date: Date,
    /// `true` when `date` belongs to the displayed month.
    in_month: bool,
}

/// Pointer hit targets.
#[derive(Clone, Copy, PartialEq)]
enum Hit {
    /// Previous-month chevron.
    Prev,
    /// Next-month chevron.
    Next,
    /// Day cell `0..42`.
    Day(usize),
}

impl Calendar {
    /// A calendar showing `2000-01` until `set_displayed_month`,
    /// `date`, or `today` navigates it.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite::widgets::calendar::Calendar;
    ///
    /// let cal = Calendar::new();
    /// assert_eq!(cal.selected(), None);
    /// ```
    pub fn new() -> Self {
        Self {
            label: "Calendar".into(),
            enabled: true,
            selected: None,
            mode: CalendarSelection::Day,
            range_start: None,
            range_end: None,
            range_anchor: None,
            range_pending: None,
            today: None,
            min: None,
            max: None,
            view: (2000, 1),
            week_starts_monday: false,
            focus_date: None,
            pending: None,
            hover: None,
            cells: Vec::new(),
            text_painter: None,
        }
    }

    /// Preselect `date` and navigate the displayed month to it.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite::widgets::calendar::Calendar;
    /// use martensite::widgets::date_picker::Date;
    ///
    /// let cal = Calendar::new().date(Date { year: 2024, month: 3, day: 9 });
    /// assert_eq!(cal.displayed_month(), (2024, 3));
    /// ```
    pub fn date(mut self, date: Date) -> Self {
        self.selected = Some(date);
        self.view = (date.year, date.month);
        self
    }

    /// Mark `date` with the accent ring (usually "today") and navigate
    /// the displayed month to it.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite::widgets::calendar::Calendar;
    /// use martensite::widgets::date_picker::Date;
    ///
    /// let cal = Calendar::new().today(Date { year: 2024, month: 1, day: 1 });
    /// ```
    pub fn today(mut self, date: Date) -> Self {
        self.today = Some(date);
        self.view = (date.year, date.month);
        self
    }

    /// Clamp the pickable range's lower bound (inclusive).
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite::widgets::calendar::Calendar;
    /// use martensite::widgets::date_picker::Date;
    ///
    /// let cal = Calendar::new().min_date(Date { year: 2024, month: 1, day: 1 });
    /// ```
    pub fn min_date(mut self, date: Date) -> Self {
        self.min = Some(date);
        self
    }

    /// Clamp the pickable range's upper bound (inclusive).
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite::widgets::calendar::Calendar;
    /// use martensite::widgets::date_picker::Date;
    ///
    /// let cal = Calendar::new().max_date(Date { year: 2030, month: 12, day: 31 });
    /// ```
    pub fn max_date(mut self, date: Date) -> Self {
        self.max = Some(date);
        self
    }

    /// Start the weekday row on Monday (`true`) or Sunday (`false`).
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite::widgets::calendar::Calendar;
    ///
    /// let cal = Calendar::new().week_starts_monday(true);
    /// ```
    pub fn week_starts_monday(mut self, monday: bool) -> Self {
        self.week_starts_monday = monday;
        self
    }

    /// Set the accessibility label (default `"Calendar"`).
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite::widgets::calendar::Calendar;
    ///
    /// let cal = Calendar::new().label("Pick a ship date");
    /// ```
    pub fn label(mut self, label: impl Into<String>) -> Self {
        self.label = label.into();
        self
    }

    /// Enable or disable interaction (default `true`).
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite::widgets::calendar::Calendar;
    ///
    /// let cal = Calendar::new().enabled(false);
    /// ```
    pub fn enabled(mut self, enabled: bool) -> Self {
        self.enabled = enabled;
        self
    }

    /// Share a text painter. `SharedTextPainter` is not `Default`, so
    /// this builder is exercised indirectly through `paint`.
    pub fn with_text_painter(mut self, painter: crate::text_paint::SharedTextPainter) -> Self {
        self.text_painter = Some(painter);
        self
    }

    /// Set click semantics — single day or range (the Ant
    /// `RangePicker` picking model).
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite::widgets::calendar::{Calendar, CalendarSelection};
    ///
    /// let cal = Calendar::new().selection(CalendarSelection::Range);
    /// ```
    pub fn selection(mut self, mode: CalendarSelection) -> Self {
        self.mode = mode;
        self
    }

    /// Preselect a range (normalized to `start <= end`) and navigate
    /// to `start`'s month.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite::widgets::calendar::{Calendar, CalendarSelection};
    /// use martensite::widgets::date_picker::Date;
    ///
    /// let cal = Calendar::new()
    ///     .selection(CalendarSelection::Range)
    ///     .range(Date { year: 2024, month: 6, day: 20 }, Date { year: 2024, month: 6, day: 10 });
    /// assert_eq!(
    ///     cal.range_value(),
    ///     Some((Date { year: 2024, month: 6, day: 10 }, Date { year: 2024, month: 6, day: 20 }))
    /// );
    /// ```
    pub fn range(mut self, start: Date, end: Date) -> Self {
        self.set_range(start, end);
        self
    }

    /// Set (or re-set) the committed range programmatically
    /// (normalized; navigates to `start`'s month).
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite::widgets::calendar::Calendar;
    /// use martensite::widgets::date_picker::Date;
    ///
    /// let mut cal = Calendar::new();
    /// cal.set_range(Date { year: 2024, month: 1, day: 5 }, Date { year: 2024, month: 1, day: 2 });
    /// assert_eq!(cal.range_value().unwrap().0.day, 2);
    /// ```
    pub fn set_range(&mut self, start: Date, end: Date) {
        let (lo, hi) = if start <= end {
            (start, end)
        } else {
            (end, start)
        };
        self.range_start = Some(lo);
        self.range_end = Some(hi);
        self.range_anchor = None;
        self.view = (lo.year, lo.month);
    }

    /// The committed `(start, end)` range, if any.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite::widgets::calendar::Calendar;
    ///
    /// assert_eq!(Calendar::new().range_value(), None);
    /// ```
    pub fn range_value(&self) -> Option<(Date, Date)> {
        self.range_start.zip(self.range_end)
    }

    /// Drain the parked user range — one-shot.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite::widgets::calendar::Calendar;
    ///
    /// let mut cal = Calendar::new();
    /// assert_eq!(cal.take_range(), None);
    /// ```
    pub fn take_range(&mut self) -> Option<(Date, Date)> {
        self.range_pending.take()
    }

    /// The selected date, if any.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite::widgets::calendar::Calendar;
    ///
    /// assert_eq!(Calendar::new().selected(), None);
    /// ```
    pub fn selected(&self) -> Option<Date> {
        self.selected
    }

    /// The displayed `(year, month)`.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite::widgets::calendar::Calendar;
    /// use martensite::widgets::date_picker::Date;
    ///
    /// let cal = Calendar::new().date(Date { year: 2024, month: 7, day: 4 });
    /// assert_eq!(cal.displayed_month(), (2024, 7));
    /// ```
    pub fn displayed_month(&self) -> (i32, u32) {
        self.view
    }

    /// Navigate the displayed month (clamped to `1..=12`).
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite::widgets::calendar::Calendar;
    ///
    /// let mut cal = Calendar::new();
    /// cal.set_displayed_month(2025, 12);
    /// assert_eq!(cal.displayed_month(), (2025, 12));
    /// ```
    pub fn set_displayed_month(&mut self, year: i32, month: u32) {
        self.view = (year, month.clamp(1, 12));
    }

    /// Set (or clear) the selected date programmatically; selecting a
    /// date navigates the displayed month to it.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite::widgets::calendar::Calendar;
    /// use martensite::widgets::date_picker::Date;
    ///
    /// let mut cal = Calendar::new();
    /// cal.set_date(Some(Date { year: 2024, month: 2, day: 29 }));
    /// assert!(cal.selected().unwrap().is_valid());
    /// ```
    pub fn set_date(&mut self, date: Option<Date>) {
        self.selected = date;
        if let Some(d) = date {
            self.view = (d.year, d.month);
        }
    }

    /// Drain the parked user selection (one-shot, like every `take_*`
    /// seam).
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite::widgets::calendar::Calendar;
    ///
    /// let mut cal = Calendar::new();
    /// assert_eq!(cal.take_selected(), None);
    /// ```
    pub fn take_selected(&mut self) -> Option<Date> {
        self.pending.take()
    }

    /// The active range span for painting — the committed range, or
    /// the anchor→hover preview while the second click is pending.
    fn active_span(&self) -> Option<(Date, Date)> {
        if self.mode != CalendarSelection::Range {
            return None;
        }
        if let (Some(a), Some(b)) = (self.range_start, self.range_end) {
            return Some((a, b));
        }
        let anchor = self.range_anchor?;
        if let Some(Hit::Day(i)) = self.hover {
            let hov = self.cells.get(i)?.date;
            return Some(if hov <= anchor {
                (hov, anchor)
            } else {
                (anchor, hov)
            });
        }
        Some((anchor, anchor))
    }

    /// Apply a pick — single-day or range-anchor/complete.
    fn pick(&mut self, d: Date) {
        match self.mode {
            CalendarSelection::Day => {
                self.selected = Some(d);
                self.pending = Some(d);
            }
            CalendarSelection::Range => {
                if let Some(anchor) = self.range_anchor.take() {
                    let (lo, hi) = if d <= anchor {
                        (d, anchor)
                    } else {
                        (anchor, d)
                    };
                    self.range_start = Some(lo);
                    self.range_end = Some(hi);
                    self.range_pending = Some((lo, hi));
                } else {
                    self.range_anchor = Some(d);
                    self.range_start = Some(d);
                    self.range_end = None;
                }
            }
        }
    }

    /// Step the displayed month by `delta` (±1 for the chevrons).
    fn step_month(&mut self, delta: i32) {
        let (y, m) = self.view;
        let total = y * 12 + m as i32 - 1 + delta;
        self.view = (total.div_euclid(12), (total.rem_euclid(12) + 1) as u32);
    }

    /// `true` when `d` is inside `[min, max]`.
    fn pickable(&self, d: Date) -> bool {
        if let Some(min) = self.min {
            if d < min {
                return false;
            }
        }
        if let Some(max) = self.max {
            if d > max {
                return false;
            }
        }
        true
    }

    /// The 42 real dates the grid shows for the current view.
    fn grid_dates(&self) -> [Date; GRID_ROWS * 7] {
        let (y, m) = self.view;
        // Weekday column of the 1st, in the current week-start
        // convention. `weekday_of` returns 0 = Sunday … 6 = Saturday.
        let first_wd = Date::weekday_of(y, m, 1) as i32;
        let lead = if self.week_starts_monday {
            (first_wd + 6).rem_euclid(7)
        } else {
            first_wd
        };
        let mut dates = [Date {
            year: y,
            month: m,
            day: 1,
        }; GRID_ROWS * 7];
        for (i, d) in dates.iter_mut().enumerate() {
            *d = offset_day(y, m, i as i32 - lead + 1);
        }
        dates
    }

    /// Hit-test a widget-local point; `header_h`/`nav_w` are the
    /// scale-adjusted header metrics.
    fn hit_at(&self, local: Vec2, width: f32, header_h: f32, nav_w: f32) -> Option<Hit> {
        if local.y < header_h {
            if local.x < nav_w {
                return Some(Hit::Prev);
            }
            if local.x > width - nav_w {
                return Some(Hit::Next);
            }
            return None;
        }
        for (i, c) in self.cells.iter().enumerate() {
            if c.rect.contains(local) {
                return Some(Hit::Day(i));
            }
        }
        None
    }

    /// Keyboard navigation — arrows move `focus_date` (crossing a
    /// month boundary shifts the view), `PageUp`/`PageDown` step
    /// months, `Enter`/`Space` select.
    fn key(&mut self, key: &str) -> EventResponse {
        let (vy, vm) = self.view;
        match key {
            "ArrowLeft" | "ArrowRight" | "ArrowUp" | "ArrowDown" => {
                let cur = self.focus_date.unwrap_or(Date {
                    year: vy,
                    month: vm,
                    day: 1,
                });
                let delta = match key {
                    "ArrowLeft" => -1,
                    "ArrowRight" => 1,
                    "ArrowUp" => -7,
                    _ => 7,
                };
                let next = offset_day(cur.year, cur.month, cur.day as i32 + delta);
                self.focus_date = Some(next);
                self.view = (next.year, next.month);
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
            "Enter" | "Space" | " " => {
                if let Some(d) = self.focus_date {
                    if !self.pickable(d) {
                        return EventResponse::Ignored;
                    }
                    self.pick(d);
                    return EventResponse::RequestRepaint;
                }
                EventResponse::Ignored
            }
            _ => EventResponse::Ignored,
        }
    }
}

impl Default for Calendar {
    fn default() -> Self {
        Self::new()
    }
}

/// The date `day_of_month` in `(year, month)` where `day_of_month` may
/// spill outside `1..=days_in_month` — `0` is the last day of the prior
/// month, `days+1` the first of the next.
fn offset_day(year: i32, month: u32, day_of_month: i32) -> Date {
    let dim = Date::days_in_month(year, month) as i32;
    if (1..=dim).contains(&day_of_month) {
        return Date {
            year,
            month,
            day: day_of_month as u32,
        };
    }
    if day_of_month < 1 {
        let (py, pm) = if month == 1 {
            (year - 1, 12)
        } else {
            (year, month - 1)
        };
        let pdim = Date::days_in_month(py, pm) as i32;
        return Date {
            year: py,
            month: pm,
            day: (pdim + day_of_month) as u32,
        };
    }
    let (ny, nm) = if month == 12 {
        (year + 1, 1)
    } else {
        (year, month + 1)
    };
    Date {
        year: ny,
        month: nm,
        day: (day_of_month - dim) as u32,
    }
}

impl Widget for Calendar {
    fn measure(&mut self, _cx: &mut LayoutContext, _constraints: LayoutConstraints) -> Vec2 {
        Vec2::new(280.0, HEADER_H + WEEKDAY_H + 6.0 * 34.0)
    }

    fn layout(&mut self, cx: &mut LayoutContext, bounds: Rect) {
        let dates = self.grid_dates();
        let (vy, vm) = self.view;
        let header_h = cx.pt(HEADER_H);
        let weekday_h = cx.pt(WEEKDAY_H);
        let cell_w = bounds.width() / 7.0;
        let cell_h = (bounds.height() - header_h - weekday_h).max(0.0) / GRID_ROWS as f32;
        // Cells are stored in widget-local coordinates — `paint`
        // offsets by `bounds`' origin and `hit_at` compares against
        // the local point.
        let top = header_h + weekday_h;
        self.cells = dates
            .iter()
            .enumerate()
            .map(|(i, date)| {
                let col = (i % 7) as f32;
                let row = (i / 7) as f32;
                Cell {
                    rect: Rect::new(col * cell_w, top + row * cell_h, cell_w, cell_h),
                    date: *date,
                    in_month: date.month == vm && date.year == vy,
                }
            })
            .collect();
    }

    fn paint(&self, cx: &mut PaintContext) {
        let b = cx.bounds;
        let header_h = cx.pt(HEADER_H);
        let weekday_h = cx.pt(WEEKDAY_H);
        let nav_w = cx.pt(NAV_W);
        let header = kurbo::Rect::new(
            f64::from(b.min_x()),
            f64::from(b.min_y()),
            f64::from(b.max_x()),
            f64::from(b.min_y() + header_h),
        );
        let painter = crate::text_paint::resolve_painter(&self.text_painter, cx.text_painter);
        let ink = cx.color(TEXT, [30, 30, 35, 255]);
        let muted = cx.color(MUTED, [140, 140, 150, 255]);
        let accent = cx.color(ACCENT, [50, 115, 230, 255]);
        let hover_wash = cx.color(HOVER, [120, 120, 130, 60]);

        // Header: ‹ chevron, "Month YYYY", › chevron.
        let chev_color = if self.enabled { ink } else { muted };
        paint_chevron(cx.list, header, nav_w * 0.5, true, chev_color);
        paint_chevron(cx.list, header, b.width() - nav_w * 0.5, false, chev_color);
        let caption = format!("{} {}", MONTHS[(self.view.1 - 1) as usize], self.view.0);
        let cap_size = 13.0 * cx.scale;
        let cap_w = painter
            .and_then(|p| p.measure_text(&caption, cap_size))
            .unwrap_or(cap_size * caption.len() as f32 * 0.5);
        crate::text_paint::paint_label_clipped(
            painter,
            cx.list,
            header,
            kurbo::Point::new(
                header.x0 + (header.width() - f64::from(cap_w)) * 0.5,
                header.y0 + header.height() * 0.72,
            ),
            &caption,
            cap_size,
            ink,
        );

        // Weekday row.
        let names = if self.week_starts_monday {
            WEEKDAYS_MON
        } else {
            WEEKDAYS_SUN
        };
        let cell_w = b.width() / 7.0;
        let wd_size = 12.0 * cx.scale;
        for (i, name) in names.iter().enumerate() {
            let clip = kurbo::Rect::new(
                f64::from(b.min_x() + i as f32 * cell_w),
                f64::from(b.min_y() + header_h),
                f64::from(b.min_x() + (i + 1) as f32 * cell_w),
                f64::from(b.min_y() + header_h + weekday_h),
            );
            let w = painter
                .and_then(|p| p.measure_text(name, wd_size))
                .unwrap_or(wd_size * name.len() as f32 * 0.5);
            crate::text_paint::paint_label_clipped(
                painter,
                cx.list,
                clip,
                kurbo::Point::new(
                    clip.x0 + (clip.width() - f64::from(w)) * 0.5,
                    clip.y0 + clip.height() * 0.7,
                ),
                name,
                wd_size,
                muted,
            );
        }

        // Day cells.
        let shape = martensite_core::shape::Shape::rounded(cx.pt(6.0));
        let day_size = 12.0 * cx.scale;
        let span = self.active_span();
        for (i, cell) in self.cells.iter().enumerate() {
            let r = kurbo::Rect::new(
                f64::from(b.min_x() + cell.rect.min_x()),
                f64::from(b.min_y() + cell.rect.min_y()),
                f64::from(b.min_x() + cell.rect.max_x()),
                f64::from(b.min_y() + cell.rect.max_y()),
            );
            let picked = self.selected == Some(cell.date);
            let endpoint = span
                .map(|(lo, hi)| cell.date == lo || cell.date == hi)
                .unwrap_or(false);
            let in_span = span
                .map(|(lo, hi)| cell.date > lo && cell.date < hi)
                .unwrap_or(false);
            let is_today = self.today == Some(cell.date);
            let in_range = self.pickable(cell.date);
            let hovered = self.enabled && self.hover == Some(Hit::Day(i)) && in_range;

            if (picked || endpoint) && in_range {
                cx.list.push_fill_shape(r, &shape, accent);
            } else if (in_span && in_range) || hovered {
                cx.list.push_fill_shape(r, &shape, hover_wash);
            }
            if is_today && !picked {
                cx.list.push_stroke_shape(r, &shape, cx.pt(1.5), accent);
            }
            if self.focus_date == Some(cell.date) {
                cx.list.push_stroke_shape(r, &shape, cx.pt(1.0), accent);
            }
            let day_ink = if !self.enabled || !in_range {
                muted
            } else if picked || endpoint {
                // Selected cells are accent-filled — inverse ink reads
                // on the chromatic face where white does not.
                cx.color(TokenKey::TextInverseColor, [255, 255, 255, 255])
            } else if cell.in_month {
                ink
            } else {
                muted
            };
            let label = format!("{}", cell.date.day);
            let w = painter
                .and_then(|p| p.measure_text(&label, day_size))
                .unwrap_or(day_size * label.len() as f32 * 0.5);
            crate::text_paint::paint_label_clipped(
                painter,
                cx.list,
                r,
                kurbo::Point::new(
                    r.x0 + (r.width() - f64::from(w)) * 0.5,
                    r.y0 + r.height() * 0.68,
                ),
                &label,
                day_size,
                day_ink,
            );
        }
    }

    fn event(&mut self, cx: &mut EventContext) -> EventResponse {
        if !self.enabled {
            return EventResponse::Ignored;
        }
        match cx.event {
            WidgetEvent::PointerMoved { position } => {
                let local = *position - cx.bounds.origin;
                let hit = self.hit_at(
                    local,
                    cx.bounds.width(),
                    cx.scale * HEADER_H,
                    cx.scale * NAV_W,
                );
                if hit != self.hover {
                    self.hover = hit;
                    return EventResponse::RequestRepaint;
                }
                EventResponse::Handled
            }
            WidgetEvent::PointerReleased { position, button }
                if *button == martensite_core::PointerButton::Primary =>
            {
                let local = *position - cx.bounds.origin;
                match self.hit_at(
                    local,
                    cx.bounds.width(),
                    cx.scale * HEADER_H,
                    cx.scale * NAV_W,
                ) {
                    Some(Hit::Prev) => {
                        self.step_month(-1);
                        EventResponse::RequestRepaint
                    }
                    Some(Hit::Next) => {
                        self.step_month(1);
                        EventResponse::RequestRepaint
                    }
                    Some(Hit::Day(i)) => {
                        let d = self.cells[i].date;
                        if !self.pickable(d) {
                            return EventResponse::Ignored;
                        }
                        self.pick(d);
                        self.view = (d.year, d.month);
                        EventResponse::RequestRepaint
                    }
                    None => EventResponse::Ignored,
                }
            }
            WidgetEvent::PointerLeave => {
                if self.hover.take().is_some() {
                    EventResponse::RequestRepaint
                } else {
                    EventResponse::Ignored
                }
            }
            WidgetEvent::KeyPressed { key, .. } => self.key(key),
            _ => EventResponse::Ignored,
        }
    }

    fn accessibility(&self, node: &mut AccessKitNode) {
        node.set_role(accesskit::Role::Group);
        node.set_label(self.label.as_str());
        if let Some(d) = self.selected {
            node.set_value(format!("{:04}-{:02}-{:02}", d.year, d.month, d.day));
        }
        if !self.enabled {
            node.set_disabled();
        }
    }

    fn min_render(&self) -> RenderMinimum {
        RenderMinimum::new(Vec2::new(220.0, 210.0)).with_policy(UnderflowPolicy::Lint)
    }
}

/// Paint a chevron centered `cx_off` from the strip's left edge
/// (`left` → `‹`).
fn paint_chevron(
    list: &mut martensite_core::PaintList,
    strip: kurbo::Rect,
    cx_off: f32,
    left: bool,
    color: [u8; 4],
) {
    let cx = strip.x0 + f64::from(cx_off);
    let cy = strip.y0 + strip.height() * 0.5;
    let s = 4.0f64;
    let dir = if left { -1.0 } else { 1.0 };
    let mut path = kurbo::BezPath::new();
    path.move_to(kurbo::Point::new(cx - dir * s, cy - s));
    path.line_to(kurbo::Point::new(cx + dir * s, cy));
    path.line_to(kurbo::Point::new(cx - dir * s, cy + s));
    list.push_stroke_path(path, 1.5, color);
}

#[cfg(test)]
mod tests {
    use super::*;
    use martensite_core::{HotNode, PointerButton};

    fn lay(w: &mut Calendar, width: f32) {
        let mut hot = HotNode::default();
        let mut cx = LayoutContext {
            hot: &mut hot,
            scale: 1.0,
        };
        w.layout(&mut cx, Rect::new(0.0, 0.0, width, 280.0));
    }

    fn ev(w: &mut Calendar, e: WidgetEvent) -> EventResponse {
        let mut cx = EventContext {
            event: &e,
            bounds: Rect::new(0.0, 0.0, 280.0, 280.0),
            scale: 1.0,
        };
        w.event(&mut cx)
    }

    #[test]
    fn grid_is_always_42_cells() {
        let mut cal = Calendar::new();
        lay(&mut cal, 280.0);
        assert_eq!(cal.cells.len(), 42);
    }

    #[test]
    fn june_2024_first_column_is_sunday_may_26() {
        let mut cal = Calendar::new();
        cal.set_displayed_month(2024, 6);
        lay(&mut cal, 280.0);
        // June 1 2024 is a Saturday → Sunday-first grid starts May 26.
        assert_eq!(
            cal.cells[0].date,
            Date {
                year: 2024,
                month: 5,
                day: 26
            }
        );
        assert!(!cal.cells[0].in_month);
    }

    #[test]
    fn monday_start_shifts_the_grid() {
        let mut cal = Calendar::new().week_starts_monday(true);
        cal.set_displayed_month(2024, 6);
        lay(&mut cal, 280.0);
        // June 1 2024 is a Saturday → Monday-first grid starts May 27.
        assert_eq!(
            cal.cells[0].date,
            Date {
                year: 2024,
                month: 5,
                day: 27
            }
        );
    }

    #[test]
    fn click_day_selects_and_parks() {
        let mut cal = Calendar::new();
        cal.set_displayed_month(2024, 6);
        lay(&mut cal, 280.0);
        let i = cal
            .cells
            .iter()
            .position(|c| c.in_month && c.date.day == 15)
            .unwrap();
        let c = cal.cells[i].rect;
        ev(
            &mut cal,
            WidgetEvent::PointerReleased {
                position: Vec2::new(c.min_x() + 2.0, c.min_y() + 2.0),
                button: PointerButton::Primary,
            },
        );
        assert_eq!(
            cal.take_selected(),
            Some(Date {
                year: 2024,
                month: 6,
                day: 15
            })
        );
    }

    #[test]
    fn chevron_steps_month() {
        let mut cal = Calendar::new();
        cal.set_displayed_month(2024, 6);
        lay(&mut cal, 280.0);
        ev(
            &mut cal,
            WidgetEvent::PointerReleased {
                position: Vec2::new(10.0, 15.0), // ‹ zone
                button: PointerButton::Primary,
            },
        );
        assert_eq!(cal.displayed_month(), (2024, 5));
        ev(
            &mut cal,
            WidgetEvent::PointerReleased {
                position: Vec2::new(270.0, 15.0), // › zone
                button: PointerButton::Primary,
            },
        );
        assert_eq!(cal.displayed_month(), (2024, 6));
    }

    #[test]
    fn out_of_month_click_navigates() {
        let mut cal = Calendar::new();
        cal.set_displayed_month(2024, 6);
        lay(&mut cal, 280.0);
        let c = cal.cells[0].rect; // May 26
        ev(
            &mut cal,
            WidgetEvent::PointerReleased {
                position: Vec2::new(c.min_x() + 2.0, c.min_y() + 2.0),
                button: PointerButton::Primary,
            },
        );
        assert_eq!(cal.displayed_month(), (2024, 5));
        assert_eq!(cal.selected().unwrap().month, 5);
    }

    #[test]
    fn arrow_keys_move_focus_across_months() {
        let mut cal = Calendar::new();
        cal.set_displayed_month(2024, 6);
        lay(&mut cal, 280.0);
        cal.focus_date = Some(Date {
            year: 2024,
            month: 6,
            day: 1,
        });
        ev(
            &mut cal,
            WidgetEvent::KeyPressed {
                key: "ArrowLeft".into(),
                repeat: false,
            },
        );
        assert_eq!(cal.focus_date.unwrap().month, 5);
        assert_eq!(cal.displayed_month(), (2024, 5));
    }

    #[test]
    fn enter_selects_focus_cell() {
        let mut cal = Calendar::new();
        cal.set_displayed_month(2024, 6);
        lay(&mut cal, 280.0);
        cal.focus_date = Some(Date {
            year: 2024,
            month: 6,
            day: 10,
        });
        ev(
            &mut cal,
            WidgetEvent::KeyPressed {
                key: "Enter".into(),
                repeat: false,
            },
        );
        assert_eq!(
            cal.take_selected(),
            Some(Date {
                year: 2024,
                month: 6,
                day: 10
            })
        );
    }

    #[test]
    fn page_keys_step_months() {
        let mut cal = Calendar::new();
        cal.set_displayed_month(2024, 1);
        lay(&mut cal, 280.0);
        ev(
            &mut cal,
            WidgetEvent::KeyPressed {
                key: "PageUp".into(),
                repeat: false,
            },
        );
        assert_eq!(cal.displayed_month(), (2023, 12));
        ev(
            &mut cal,
            WidgetEvent::KeyPressed {
                key: "PageDown".into(),
                repeat: false,
            },
        );
        assert_eq!(cal.displayed_month(), (2024, 1));
    }

    #[test]
    fn out_of_range_cells_are_inert() {
        let mut cal = Calendar::new().min_date(Date {
            year: 2024,
            month: 6,
            day: 10,
        });
        cal.set_displayed_month(2024, 6);
        lay(&mut cal, 280.0);
        let i = cal
            .cells
            .iter()
            .position(|c| c.in_month && c.date.day == 5)
            .unwrap();
        let c = cal.cells[i].rect;
        ev(
            &mut cal,
            WidgetEvent::PointerReleased {
                position: Vec2::new(c.min_x() + 2.0, c.min_y() + 2.0),
                button: PointerButton::Primary,
            },
        );
        assert_eq!(cal.take_selected(), None);
        assert_eq!(cal.selected(), None);
    }

    #[test]
    fn disabled_swallows_everything() {
        let mut cal = Calendar::new().enabled(false);
        cal.set_displayed_month(2024, 6);
        lay(&mut cal, 280.0);
        let r = ev(
            &mut cal,
            WidgetEvent::PointerReleased {
                position: Vec2::new(10.0, 15.0),
                button: PointerButton::Primary,
            },
        );
        assert!(matches!(r, EventResponse::Ignored));
        assert_eq!(cal.displayed_month(), (2024, 6));
    }

    #[test]
    fn offset_day_spills_both_directions() {
        assert_eq!(
            offset_day(2024, 3, 0),
            Date {
                year: 2024,
                month: 2,
                day: 29
            }
        );
        assert_eq!(
            offset_day(2024, 1, 32),
            Date {
                year: 2024,
                month: 2,
                day: 1
            }
        );
        assert_eq!(
            offset_day(2024, 12, 32),
            Date {
                year: 2025,
                month: 1,
                day: 1
            }
        );
    }

    fn click_day(cal: &mut Calendar, day: u32) {
        let i = cal
            .cells
            .iter()
            .position(|c| c.in_month && c.date.day == day)
            .unwrap();
        let c = cal.cells[i].rect;
        ev(
            cal,
            WidgetEvent::PointerReleased {
                position: Vec2::new(c.min_x() + 2.0, c.min_y() + 2.0),
                button: PointerButton::Primary,
            },
        );
    }

    #[test]
    fn range_mode_two_clicks_commit_normalized() {
        let mut cal = Calendar::new().selection(CalendarSelection::Range);
        cal.set_displayed_month(2024, 6);
        lay(&mut cal, 280.0);
        // Click 20 then 10 — the pair normalizes to (10, 20).
        click_day(&mut cal, 20);
        assert_eq!(cal.take_range(), None);
        click_day(&mut cal, 10);
        assert_eq!(
            cal.take_range(),
            Some((
                Date {
                    year: 2024,
                    month: 6,
                    day: 10
                },
                Date {
                    year: 2024,
                    month: 6,
                    day: 20
                }
            ))
        );
        assert_eq!(cal.take_range(), None);
    }

    #[test]
    fn range_mode_third_click_reanchors() {
        let mut cal = Calendar::new().selection(CalendarSelection::Range);
        cal.set_displayed_month(2024, 6);
        lay(&mut cal, 280.0);
        click_day(&mut cal, 5);
        click_day(&mut cal, 10);
        assert!(cal.take_range().is_some());
        // Third click starts a fresh anchor.
        click_day(&mut cal, 25);
        assert_eq!(cal.take_range(), None);
        click_day(&mut cal, 28);
        let (lo, hi) = cal.take_range().unwrap();
        assert_eq!((lo.day, hi.day), (25, 28));
    }

    #[test]
    fn range_mode_single_day_span() {
        let mut cal = Calendar::new().selection(CalendarSelection::Range);
        cal.set_displayed_month(2024, 6);
        lay(&mut cal, 280.0);
        click_day(&mut cal, 15);
        click_day(&mut cal, 15);
        let (lo, hi) = cal.take_range().unwrap();
        assert_eq!(lo, hi);
    }

    #[test]
    fn day_mode_unaffected_by_range_state() {
        let mut cal = Calendar::new();
        cal.set_displayed_month(2024, 6);
        lay(&mut cal, 280.0);
        click_day(&mut cal, 15);
        assert!(cal.take_selected().is_some());
        assert_eq!(cal.take_range(), None);
    }
}
