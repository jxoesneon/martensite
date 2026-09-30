//! `time` namespace — clocks, calendars, alarms, timers.

use super::super::{IconEntry, IconPair};

/// Qualified icon names (`time.*`).
///
/// Every name constant in this module is prefixed `TIME_`
/// (`TIME_FOO` → `"time.foo"`) so the flattened
/// `builtin::names` re-export cannot collide.
pub mod names {
    /// `"time.clock"` — wall clock.
    pub const CLOCK: &str = "time.clock";
    /// `"time.timer"` — countdown timer.
    pub const TIMER: &str = "time.timer";
    /// `"time.timer-off"` — timer struck through.
    pub const TIMER_OFF: &str = "time.timer-off";
    /// `"time.alarm-clock"` — alarm clock with bells.
    pub const ALARM_CLOCK: &str = "time.alarm-clock";
    /// `"time.alarm-clock-off"` — alarm clock struck through.
    pub const ALARM_CLOCK_OFF: &str = "time.alarm-clock-off";
    /// `"time.alarm-clock-plus"` — alarm clock with add badge.
    pub const ALARM_CLOCK_PLUS: &str = "time.alarm-clock-plus";
    /// `"time.hourglass"` — hourglass.
    pub const HOURGLASS: &str = "time.hourglass";
    /// `"time.stopwatch"` — stopwatch.
    pub const STOPWATCH: &str = "time.stopwatch";
    /// `"time.calendar"` — calendar page.
    pub const CALENDAR: &str = "time.calendar";
    /// `"time.calendar-plus"` — calendar with add mark.
    pub const CALENDAR_PLUS: &str = "time.calendar-plus";
    /// `"time.calendar-x"` — calendar with remove mark.
    pub const CALENDAR_X: &str = "time.calendar-x";
    /// `"time.calendar-check"` — calendar with confirm mark.
    pub const CALENDAR_CHECK: &str = "time.calendar-check";
    /// `"time.calendar-days"` — calendar with day grid.
    pub const CALENDAR_DAYS: &str = "time.calendar-days";
    /// `"time.calendar-clock"` — calendar with time badge.
    pub const CALENDAR_CLOCK: &str = "time.calendar-clock";
}

/// `"time.clock"` — circle plus hands.
pub const TIME_CLOCK: &str = "M21 12a9 9 0 11-18 0 9 9 0 0118 0zM12 7v5l3 2";
/// `"time.timer"` — crown bar, dial, diagonal hand.
pub const TIME_TIMER: &str = "M10 2h4M20 14a8 8 0 11-16 0 8 8 0 0116 0zM12 14l3-3";
/// `"time.timer-off"` — timer plus strike slash.
pub const TIME_TIMER_OFF: &str = "M10 2h4M20 14a8 8 0 11-16 0 8 8 0 0116 0zM12 14l3-3M2 2l20 20";
/// `"time.alarm-clock"` — dial, bells, legs, hands.
pub const TIME_ALARM_CLOCK: &str =
    "M5 3L2 6M22 6l-3-3M20 13a8 8 0 11-16 0 8 8 0 0116 0zM12 9v4l2 2M6.4 18.7L4 21M17.6 18.7L20 21";
/// `"time.alarm-clock-off"` — alarm clock plus strike slash.
pub const TIME_ALARM_CLOCK_OFF: &str =
    "M5 3L2 6M22 6l-3-3M20 13a8 8 0 11-16 0 8 8 0 0116 0zM12 9v4l2 2M6.4 18.7L4 21M17.6 18.7L20 21M2 2l20 20";
/// `"time.alarm-clock-plus"` — alarm clock plus add badge.
pub const TIME_ALARM_CLOCK_PLUS: &str =
    "M5 3L2 6M22 6l-3-3M20 13a8 8 0 10-4 7M12 9v4l2 2M6.4 18.7L4 21M19.5 16.5v5M17 19h5";
/// `"time.hourglass"` — caps and pinched vessels.
pub const TIME_HOURGLASS: &str =
    "M5 2h14M5 22h14M17 2V6.5L12 12L7 6.5V2M7 22V17.5L12 12L17 17.5V22";
/// `"time.stopwatch"` — crown stem, side button, dial, hand.
pub const TIME_STOPWATCH: &str =
    "M10 2h4M12 2v4M19 5l2-2M20 14a8 8 0 11-16 0 8 8 0 0116 0zM12 10v4";
/// `"time.calendar"` — body, header, binder rings.
pub const TIME_CALENDAR: &str =
    "M5 4h14a2 2 0 012 2v13a2 2 0 01-2 2H5a2 2 0 01-2-2V6a2 2 0 012-2zM8 2v4M16 2v4M3 10h18";
/// `"time.calendar-plus"` — calendar plus centered add mark.
pub const TIME_CALENDAR_PLUS: &str =
    "M5 4h14a2 2 0 012 2v13a2 2 0 01-2 2H5a2 2 0 01-2-2V6a2 2 0 012-2zM8 2v4M16 2v4M3 10h18M12 14v5M9.5 16.5h5";
/// `"time.calendar-x"` — calendar plus centered remove mark.
pub const TIME_CALENDAR_X: &str =
    "M5 4h14a2 2 0 012 2v13a2 2 0 01-2 2H5a2 2 0 01-2-2V6a2 2 0 012-2zM8 2v4M16 2v4M3 10h18M9.5 14.5l5 5M14.5 14.5l-5 5";
/// `"time.calendar-check"` — calendar plus centered check mark.
pub const TIME_CALENDAR_CHECK: &str =
    "M5 4h14a2 2 0 012 2v13a2 2 0 01-2 2H5a2 2 0 01-2-2V6a2 2 0 012-2zM8 2v4M16 2v4M3 10h18M9 16l2 2 4-4.5";
/// `"time.calendar-days"` — calendar plus day dots.
pub const TIME_CALENDAR_DAYS: &str =
    "M5 4h14a2 2 0 012 2v13a2 2 0 01-2 2H5a2 2 0 01-2-2V6a2 2 0 012-2zM8 2v4M16 2v4M3 10h18M8 14h.01M12 14h.01M16 14h.01M8 18h.01M12 18h.01M16 18h.01";
/// `"time.calendar-clock"` — calendar plus clock badge.
pub const TIME_CALENDAR_CLOCK: &str =
    "M21 10.5V6a2 2 0 00-2-2H5a2 2 0 00-2 2v13a2 2 0 002 2h5.5M8 2v4M16 2v4M3 10h18M20 17.5a3.5 3.5 0 11-7 0 3.5 3.5 0 017 0zM16.5 15.75V17.5L17.5 18.5";

/// `time` entries — registered in the pack in this order.
pub const ENTRIES: &[IconEntry] = &[
    IconEntry::new(names::CLOCK, TIME_CLOCK),
    IconEntry::new(names::TIMER, TIME_TIMER),
    IconEntry::new(names::TIMER_OFF, TIME_TIMER_OFF),
    IconEntry::new(names::ALARM_CLOCK, TIME_ALARM_CLOCK),
    IconEntry::new(names::ALARM_CLOCK_OFF, TIME_ALARM_CLOCK_OFF),
    IconEntry::new(names::ALARM_CLOCK_PLUS, TIME_ALARM_CLOCK_PLUS),
    IconEntry::new(names::HOURGLASS, TIME_HOURGLASS),
    IconEntry::new(names::STOPWATCH, TIME_STOPWATCH),
    IconEntry::new(names::CALENDAR, TIME_CALENDAR),
    IconEntry::new(names::CALENDAR_PLUS, TIME_CALENDAR_PLUS),
    IconEntry::new(names::CALENDAR_X, TIME_CALENDAR_X),
    IconEntry::new(names::CALENDAR_CHECK, TIME_CALENDAR_CHECK),
    IconEntry::new(names::CALENDAR_DAYS, TIME_CALENDAR_DAYS),
    IconEntry::new(names::CALENDAR_CLOCK, TIME_CALENDAR_CLOCK),
];

/// `time` morph pairs.
pub const PAIRS: &[IconPair] = &[
    IconPair::new(names::TIMER, names::TIMER_OFF),
    IconPair::new(names::ALARM_CLOCK, names::ALARM_CLOCK_OFF),
];
