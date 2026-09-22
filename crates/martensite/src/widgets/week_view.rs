//! `WeekView` — a seven-day agenda grid with an all-day strip and
//! timed event blocks (the Google-Calendar / Outlook week idiom).
//!
//! Days are `0`–`6` left to right. Events carry a day index, an
//! `f32` start/end hour, and a color; [`WeekView::hour_range`] trims
//! the visible band. Clicking an event parks its index in
//! [`WeekView::take_clicked`]; clicking an empty slot parks
//! `(day, hour)` in [`WeekView::take_slot`] so hosts can open a
//! creation flow.
//!
//! # Examples
//!
//! ```
//! use martensite::widgets::week_view::{WeekView, WeekEvent};
//!
//! let mut w = WeekView::new()
//!     .event(WeekEvent::new("Standup", 0, 9.0, 9.5));
//! assert_eq!(w.event_count(), 1);
//! assert_eq!(w.take_clicked(), None);
//! ```

use accesskit::Node as AccessKitNode;
use glam::Vec2;
use martensite_core::{
    EventContext, EventResponse, LayoutConstraints, LayoutContext, PaintContext, PointerButton,
    Rect, RenderMinimum, UnderflowPolicy, Widget, WidgetEvent,
};
use martensite_theme::TokenKey;
use parking_lot::Mutex;

const HOUR_COL_PT: f32 = 40.0;
const HEADER_PT: f32 = 22.0;
const ALLDAY_PT: f32 = 20.0;
const HOUR_PT: f32 = 36.0;

const FACE: [u8; 4] = [250, 250, 250, 255];
const GRID: [u8; 4] = [225, 227, 232, 255];
const EDGE: [u8; 4] = [180, 183, 190, 255];
const MUTED: [u8; 4] = [120, 124, 132, 255];
const INK: [u8; 4] = [40, 42, 48, 255];
const EVENT: [u8; 4] = [96, 165, 250, 255];

/// One timed or all-day event — see [`WeekView`].
///
/// ```
/// use martensite::widgets::week_view::WeekEvent;
///
/// let e = WeekEvent::new("Lunch", 2, 12.0, 13.0);
/// assert_eq!(e.day, 2);
/// ```
#[derive(Debug, Clone)]
pub struct WeekEvent {
    /// Display title.
    pub title: String,
    /// Day index `0`–`6`.
    pub day: usize,
    /// Start hour (`9.5` = 09:30); ignored for all-day events.
    pub start: f32,
    /// End hour; ignored for all-day events.
    pub end: f32,
    /// Block color.
    pub color: [u8; 4],
    /// Renders in the all-day strip instead of the timed grid.
    pub all_day: bool,
}

impl WeekEvent {
    /// A timed event.
    ///
    /// ```
    /// use martensite::widgets::week_view::WeekEvent;
    ///
    /// assert!(!WeekEvent::new("x", 0, 9.0, 10.0).all_day);
    /// ```
    pub fn new(title: impl Into<String>, day: usize, start: f32, end: f32) -> Self {
        Self {
            title: title.into(),
            day: day.min(6),
            start,
            end: end.max(start),
            color: EVENT,
            all_day: false,
        }
    }

    /// An all-day event (drawn in the top strip).
    ///
    /// ```
    /// use martensite::widgets::week_view::WeekEvent;
    ///
    /// assert!(WeekEvent::all_day("Offsite", 4).all_day);
    /// ```
    pub fn all_day(title: impl Into<String>, day: usize) -> Self {
        Self {
            all_day: true,
            ..Self::new(title, day, 0.0, 0.0)
        }
    }

    /// Block color.
    ///
    /// ```
    /// use martensite::widgets::week_view::WeekEvent;
    ///
    /// assert_eq!(WeekEvent::new("x", 0, 0.0, 1.0).color([1, 2, 3, 255]).color, [1, 2, 3, 255]);
    /// ```
    pub fn color(mut self, color: [u8; 4]) -> Self {
        self.color = color;
        self
    }
}

/// A seven-day timed agenda grid — see the module docs.
///
/// ```
/// use martensite::widgets::week_view::WeekView;
///
/// assert_eq!(WeekView::new().event_count(), 0);
/// ```
pub struct WeekView {
    /// Accessibility label.
    pub label: String,
    events: Vec<WeekEvent>,
    clicked: Option<usize>,
    slot: Option<(usize, f32)>,
    /// Visible hour band `[start, end)`.
    hours: (f32, f32),
    day_names: [String; 7],
    bounds: Rect,
    scale: f32,
    /// Event rects painted last frame.
    hits: Mutex<Vec<(usize, Rect)>>,
    text_painter: Option<crate::text_paint::SharedTextPainter>,
}

impl std::fmt::Debug for WeekView {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("WeekView")
            .field("events", &self.events.len())
            .field("hours", &self.hours)
            .finish()
    }
}

impl Default for WeekView {
    fn default() -> Self {
        Self::new()
    }
}

impl WeekView {
    /// Empty week, `06:00`–`22:00` visible.
    ///
    /// ```
    /// use martensite::widgets::week_view::WeekView;
    ///
    /// assert_eq!(WeekView::new().event_count(), 0);
    /// ```
    pub fn new() -> Self {
        Self {
            label: "Week".to_string(),
            events: Vec::new(),
            clicked: None,
            slot: None,
            hours: (6.0, 22.0),
            day_names: ["Mon", "Tue", "Wed", "Thu", "Fri", "Sat", "Sun"]
                .iter()
                .map(|s| (*s).to_string())
                .collect::<Vec<_>>()
                .try_into()
                .unwrap_or_else(|_| unreachable!()),
            bounds: Rect::new(0.0, 0.0, 0.0, 0.0),
            scale: 1.0,
            hits: Mutex::new(Vec::new()),
            text_painter: None,
        }
    }

    /// Accessibility label.
    ///
    /// ```
    /// use martensite::widgets::week_view::WeekView;
    ///
    /// assert_eq!(WeekView::new().label("Sprint 12").label, "Sprint 12");
    /// ```
    pub fn label(mut self, label: impl Into<String>) -> Self {
        self.label = label.into();
        self
    }

    /// Custom text painter.
    ///
    /// ```
    /// use martensite::widgets::week_view::WeekView;
    ///
    /// let _ = WeekView::new(); // painter optional
    /// ```
    pub fn with_text_painter(mut self, p: crate::text_paint::SharedTextPainter) -> Self {
        self.text_painter = Some(p);
        self
    }

    /// Appends an event.
    ///
    /// ```
    /// use martensite::widgets::week_view::{WeekView, WeekEvent};
    ///
    /// assert_eq!(WeekView::new().event(WeekEvent::new("a", 0, 9.0, 10.0)).event_count(), 1);
    /// ```
    pub fn event(mut self, e: WeekEvent) -> Self {
        self.events.push(e);
        self
    }

    /// Visible hour band.
    ///
    /// ```
    /// use martensite::widgets::week_view::WeekView;
    ///
    /// assert_eq!(WeekView::new().hour_range(0.0, 24.0).hour_range_value(), (0.0, 24.0));
    /// ```
    pub fn hour_range(mut self, start: f32, end: f32) -> Self {
        self.hours = (start.clamp(0.0, 23.0), end.clamp(start + 1.0, 24.0));
        self
    }

    /// The configured hour band.
    ///
    /// ```
    /// use martensite::widgets::week_view::WeekView;
    ///
    /// assert_eq!(WeekView::new().hour_range_value(), (6.0, 22.0));
    /// ```
    pub fn hour_range_value(&self) -> (f32, f32) {
        self.hours
    }

    /// Day header names, `0`–`6`.
    ///
    /// ```
    /// use martensite::widgets::week_view::WeekView;
    ///
    /// let w = WeekView::new().day_names(["L", "M", "M", "J", "V", "S", "D"]);
    /// assert_eq!(w.day_name(0), "L");
    /// ```
    pub fn day_names(mut self, names: [&str; 7]) -> Self {
        self.day_names = names.map(String::from);
        self
    }

    /// Day `i`'s header name.
    ///
    /// ```
    /// use martensite::widgets::week_view::WeekView;
    ///
    /// assert_eq!(WeekView::new().day_name(0), "Mon");
    /// ```
    pub fn day_name(&self, i: usize) -> &str {
        &self.day_names[i.min(6)]
    }

    /// Event count.
    ///
    /// ```
    /// use martensite::widgets::week_view::WeekView;
    ///
    /// assert_eq!(WeekView::new().event_count(), 0);
    /// ```
    pub fn event_count(&self) -> usize {
        self.events.len()
    }

    /// Event `i`, if in range.
    ///
    /// ```
    /// use martensite::widgets::week_view::{WeekView, WeekEvent};
    ///
    /// let w = WeekView::new().event(WeekEvent::new("a", 0, 9.0, 10.0));
    /// assert_eq!(w.event_at(0).unwrap().title, "a");
    /// ```
    pub fn event_at(&self, i: usize) -> Option<&WeekEvent> {
        self.events.get(i)
    }

    /// Removes event `i` and returns it.
    ///
    /// ```
    /// use martensite::widgets::week_view::{WeekView, WeekEvent};
    ///
    /// let mut w = WeekView::new().event(WeekEvent::new("a", 0, 9.0, 10.0));
    /// assert!(w.remove_event(0).is_some());
    /// assert_eq!(w.event_count(), 0);
    /// ```
    pub fn remove_event(&mut self, i: usize) -> Option<WeekEvent> {
        if i < self.events.len() {
            Some(self.events.remove(i))
        } else {
            None
        }
    }

    /// Drains the index of the last clicked event.
    ///
    /// ```
    /// use martensite::widgets::week_view::WeekView;
    ///
    /// assert_eq!(WeekView::new().take_clicked(), None);
    /// ```
    pub fn take_clicked(&mut self) -> Option<usize> {
        self.clicked.take()
    }

    /// Drains the last clicked empty slot as `(day, hour)`.
    ///
    /// ```
    /// use martensite::widgets::week_view::WeekView;
    ///
    /// assert_eq!(WeekView::new().take_slot(), None);
    /// ```
    pub fn take_slot(&mut self) -> Option<(usize, f32)> {
        self.slot.take()
    }

    /// Column rect of day `d` in the timed grid.
    fn day_col(&self, d: usize) -> Rect {
        let s = self.scale;
        let grid_x = self.bounds.min_x() + HOUR_COL_PT * s;
        let grid_w = (self.bounds.width() - HOUR_COL_PT * s).max(0.0);
        let col_w = grid_w / 7.0;
        let top = self.bounds.min_y() + (HEADER_PT + ALLDAY_PT) * s;
        Rect::new(
            grid_x + d as f32 * col_w,
            top,
            col_w,
            (self.bounds.max_y() - top).max(0.0),
        )
    }

    /// Y of hour `h` inside the timed grid.
    fn y_of(&self, h: f32) -> f32 {
        let col = self.day_col(0);
        let f = ((h - self.hours.0) / (self.hours.1 - self.hours.0).max(0.001)).clamp(0.0, 1.0);
        col.min_y() + f * col.height()
    }

    /// `(day, hour)` under point `p` in the timed grid.
    fn slot_at(&self, p: Vec2) -> Option<(usize, f32)> {
        for d in 0..7 {
            let col = self.day_col(d);
            if col.contains(p) {
                let f = (p.y - col.min_y()) / col.height().max(0.001);
                let h = self.hours.0 + f * (self.hours.1 - self.hours.0);
                return Some((d, (h * 4.0).round() / 4.0)); // quarter-hour snap
            }
        }
        None
    }
}

impl Widget for WeekView {
    fn measure(&mut self, cx: &mut LayoutContext, constraints: LayoutConstraints) -> Vec2 {
        let h = HEADER_PT + ALLDAY_PT + (self.hours.1 - self.hours.0).min(10.0) * HOUR_PT;
        Vec2::new(
            cx.pt(560.0).min(constraints.max_size.x.max(0.0)),
            cx.pt(h).min(constraints.max_size.y.max(0.0)),
        )
    }

    fn min_render(&self) -> RenderMinimum {
        RenderMinimum::new(Vec2::new(280.0, 140.0)).with_policy(UnderflowPolicy::Lint)
    }

    fn layout(&mut self, cx: &mut LayoutContext, bounds: Rect) {
        self.bounds = bounds;
        self.scale = cx.scale;
    }

    fn accessibility(&self, node: &mut AccessKitNode) {
        node.set_role(accesskit::Role::Grid);
        node.set_label(format!("{} — {} events", self.label, self.events.len()));
    }

    fn event(&mut self, cx: &mut EventContext) -> EventResponse {
        match cx.event {
            WidgetEvent::PointerPressed {
                button: PointerButton::Primary,
                position,
                ..
            } => {
                if !self.bounds.contains(*position) {
                    return EventResponse::Ignored;
                }
                // Events take precedence over slots.
                let hits = self.hits.lock();
                if let Some((i, _)) = hits.iter().rev().find(|(_, r)| r.contains(*position)) {
                    let i = *i;
                    drop(hits);
                    self.clicked = Some(i);
                    return EventResponse::RequestRepaint;
                }
                drop(hits);
                if let Some(slot) = self.slot_at(*position) {
                    self.slot = Some(slot);
                    return EventResponse::RequestRepaint;
                }
                EventResponse::Handled
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
        let pt = |p: Vec2| kurbo::Point::new(f64::from(p.x), f64::from(p.y));
        let painter = crate::text_paint::resolve_painter(&self.text_painter, cx.text_painter);
        let s = self.scale;
        let grid = cx.color(TokenKey::DividerColor, GRID);
        let edge = cx.color(TokenKey::BorderColor, EDGE);
        let muted = cx.color(TokenKey::TextMutedColor, MUTED);
        let ink = cx.color(TokenKey::TextColor, INK);

        cx.list.push_fill_rect(
            krect(self.bounds),
            cx.color(TokenKey::BackgroundColor, FACE),
        );
        // Hour labels at the last line and all-day rows can run past
        // the bottom edge — clip to the widget so they cut cleanly.
        cx.list.push_clip(krect(self.bounds));

        // Day headers.
        let hdr_sz = 9.5 * s;
        for d in 0..7 {
            let col = self.day_col(d);
            let name = &self.day_names[d];
            let w = painter
                .and_then(|p| p.measure_text(name, hdr_sz))
                .unwrap_or(name.len() as f32 * hdr_sz * 0.55);
            crate::text_paint::paint_label_clipped(
                painter,
                cx.list,
                krect(Rect::new(
                    col.min_x(),
                    self.bounds.min_y(),
                    col.width(),
                    HEADER_PT * s,
                )),
                pt(Vec2::new(
                    col.min_x() + (col.width() - w) / 2.0,
                    self.bounds.min_y() + (HEADER_PT * s - hdr_sz) / 2.0,
                )),
                name,
                hdr_sz,
                muted,
            );
        }

        // Hour lines + labels.
        let hr_sz = 8.5 * s;
        let start = self.hours.0.ceil() as i32;
        let end = self.hours.1.floor() as i32;
        for h in start..=end {
            let y = self.y_of(h as f32);
            let mut line = kurbo::BezPath::new();
            line.move_to(pt(Vec2::new(self.bounds.min_x() + HOUR_COL_PT * s, y)));
            line.line_to(pt(Vec2::new(self.bounds.max_x(), y)));
            cx.list.push_stroke_path(line, 0.5 * s, grid);
            let label = format!("{h:02}:00");
            crate::text_paint::paint_label(
                painter,
                cx.list,
                pt(Vec2::new(self.bounds.min_x() + 4.0 * s, y - hr_sz * 0.6)),
                &label,
                hr_sz,
                muted,
            );
        }

        // Day separators.
        for d in 0..=7 {
            let col = self.day_col(d.min(6));
            let x = if d == 7 { col.max_x() } else { col.min_x() };
            let mut line = kurbo::BezPath::new();
            line.move_to(pt(Vec2::new(x, self.bounds.min_y())));
            line.line_to(pt(Vec2::new(x, self.bounds.max_y())));
            cx.list.push_stroke_path(line, 0.5 * s, grid);
        }
        cx.list.push_stroke_shape(
            krect(self.bounds),
            &martensite_core::shape::Shape::RECT,
            s,
            edge,
        );

        // Events — rects first so a label can test whether a *later*
        // overlapping block will cover it entirely (dead paint).
        let mut hits = self.hits.lock();
        hits.clear();
        let mut allday_count = [0usize; 7];
        let mut placed: Vec<(usize, Rect)> = Vec::with_capacity(self.events.len());
        for (i, e) in self.events.iter().enumerate() {
            if e.day > 6 {
                continue;
            }
            let col = self.day_col(e.day);
            let r = if e.all_day {
                let row = allday_count[e.day];
                allday_count[e.day] += 1;
                Rect::new(
                    col.min_x() + 2.0 * s,
                    self.bounds.min_y() + HEADER_PT * s + row as f32 * 9.0 * s + 2.0 * s,
                    (col.width() - 4.0 * s).max(0.0),
                    8.0 * s,
                )
            } else {
                let y0 = self.y_of(e.start);
                let y1 = self.y_of(e.end).max(y0 + 6.0 * s);
                Rect::new(
                    col.min_x() + 2.0 * s,
                    y0,
                    (col.width() - 4.0 * s).max(0.0),
                    y1 - y0,
                )
            };
            if r.max_y() < self.bounds.min_y() || r.min_y() > self.bounds.max_y() {
                continue;
            }
            hits.push((i, r));
            placed.push((i, r));
        }
        let krs: Vec<kurbo::Rect> = placed.iter().map(|(_, r)| krect(*r)).collect();
        for (pos, (i, r)) in placed.iter().enumerate() {
            let e = &self.events[*i];
            let kr = krs[pos];
            cx.list.push_fill_shape(
                kr,
                &martensite_core::shape::Shape::rounded(3.0 * s),
                e.color,
            );
            if !e.all_day {
                let o = pt(Vec2::new(r.min_x() + 4.0 * s, r.min_y() + 2.0 * s));
                let size = 9.0 * s;
                // The audit probes `ink ∩ clip` — a long title clipped
                // to a narrow block is judged on the surviving sliver,
                // and events can bleed past the widget edge where the
                // parent's clip is what survives.
                let covered = crate::text_paint::label_ink_bounds(painter, o, &e.title, size)
                    .is_some_and(|tb| {
                        crate::text_paint::fully_occluded(
                            tb.intersect(kr).intersect(krect(self.bounds)),
                            &krs[pos + 1..],
                        )
                    });
                if !covered {
                    crate::text_paint::paint_label_clipped(
                        painter, cx.list, kr, o, &e.title, size, ink,
                    );
                }
            }
        }
        cx.list.pop_clip();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use martensite_core::{HotNode, PaintList};

    fn laid_out(w: &mut WeekView, wd: f32, h: f32) {
        let mut hot = HotNode::default();
        let mut cx = LayoutContext {
            hot: &mut hot,
            scale: 1.0,
        };
        w.measure(
            &mut cx,
            LayoutConstraints {
                min_size: Vec2::ZERO,
                max_size: Vec2::new(wd, h),
            },
        );
        w.layout(&mut cx, Rect::new(0.0, 0.0, wd, h));
    }

    fn painted(w: &WeekView) {
        let theme = martensite_theme::Theme::new("test");
        let mut list = PaintList::new();
        let mut cx = PaintContext {
            list: &mut list,
            bounds: w.bounds,
            scale: 1.0,
            theme: &theme,
            text_painter: None,
        };
        w.paint(&mut cx);
    }

    fn ev(w: &mut WeekView, e: &WidgetEvent) {
        w.event(&mut EventContext {
            event: e,
            bounds: w.bounds,
            scale: 1.0,
        });
    }

    #[test]
    fn click_event_parks_index() {
        let mut w = WeekView::new().event(WeekEvent::new("Standup", 0, 9.0, 10.0));
        laid_out(&mut w, 560.0, 480.0);
        painted(&w);
        let r = w.hits.lock()[0].1;
        ev(
            &mut w,
            &WidgetEvent::PointerPressed {
                button: PointerButton::Primary,
                position: Vec2::new((r.min_x() + r.max_x()) / 2.0, (r.min_y() + r.max_y()) / 2.0),
                count: 1,
            },
        );
        assert_eq!(w.take_clicked(), Some(0));
        assert_eq!(w.take_clicked(), None);
    }

    #[test]
    fn click_empty_slot_parks_day_hour() {
        let mut w = WeekView::new();
        laid_out(&mut w, 560.0, 480.0);
        painted(&w);
        let col = w.day_col(2);
        // 25% down day 2's grid → hour 6 + 0.25*16 = 10.0.
        ev(
            &mut w,
            &WidgetEvent::PointerPressed {
                button: PointerButton::Primary,
                position: Vec2::new(col.min_x() + 10.0, col.min_y() + col.height() * 0.25),
                count: 1,
            },
        );
        assert_eq!(w.take_slot(), Some((2, 10.0)));
    }

    #[test]
    fn all_day_paints_in_strip() {
        let mut w = WeekView::new().event(WeekEvent::all_day("Off", 3));
        laid_out(&mut w, 560.0, 480.0);
        painted(&w);
        let r = w.hits.lock()[0].1;
        let col = w.day_col(3);
        assert!(r.max_y() < col.min_y());
        assert!(r.min_x() >= col.min_x());
    }

    #[test]
    fn remove_event_returns_it() {
        let mut w = WeekView::new()
            .event(WeekEvent::new("a", 0, 9.0, 10.0))
            .event(WeekEvent::new("b", 1, 11.0, 12.0));
        assert_eq!(w.remove_event(0).unwrap().title, "a");
        assert_eq!(w.event_count(), 1);
        assert!(w.remove_event(5).is_none());
    }
}
