//! `WorldClock` — a multi-timezone clock list (GNOME Clocks /
//! macOS widget idiom): rows of `city · UTC±h · HH:MM`, ticking
//! off a host-set UTC base with `±1d` day-shift markers.
//!
//! [`WorldClock::set_utc`] seeds the base time; `tick` advances it
//! minute-by-minute. [`WorldClock::local`] reports a city's local
//! [`Time`] and [`WorldClock::day_shift`] its ±1 day offset.
//! Companion to `DigitalClock`/`AnalogClock`.
//!
//! # Examples
//!
//! ```
//! use martensite::widgets::world_clock::WorldClock;
//! use martensite::widgets::Time;
//!
//! let w = WorldClock::new()
//!     .zone("Tokyo", 9 * 60)
//!     .zone("New York", -5 * 60);
//! assert_eq!(w.zone_count(), 2);
//! ```

use std::time::Duration;

use accesskit::Node as AccessKitNode;
use glam::Vec2;
use martensite_core::{
    EventContext, EventResponse, LayoutConstraints, LayoutContext, PaintContext, Rect,
    RenderMinimum, UnderflowPolicy, Widget,
};
use martensite_theme::TokenKey;

use crate::text_paint::SharedTextPainter;
use crate::widgets::Time;

const ROW_PT: f32 = 34.0;
const PAD_PT: f32 = 12.0;
const NAME_PT: f32 = 13.0;
const ZONE_PT: f32 = 10.5;
const TIME_PT: f32 = 17.0;

const FACE: [u8; 4] = [30, 32, 40, 255];
const TEXT: [u8; 4] = [235, 237, 240, 255];
const MUTED: [u8; 4] = [150, 155, 170, 255];
const SHIFT: [u8; 4] = [200, 170, 90, 255];
const LINE: [u8; 4] = [70, 74, 88, 255];

/// One timezone row: a label plus a UTC offset in minutes.
///
/// ```
/// use martensite::widgets::world_clock::ZoneEntry;
///
/// let z = ZoneEntry::new("Tokyo", 540);
/// assert_eq!(z.offset_min, 540);
/// ```
#[derive(Clone, Debug)]
pub struct ZoneEntry {
    /// City/zone label.
    pub name: String,
    /// Offset from UTC in minutes (e.g. `9 * 60` for JST).
    pub offset_min: i32,
}

impl ZoneEntry {
    /// Names a zone with a minute offset.
    ///
    /// ```
    /// use martensite::widgets::world_clock::ZoneEntry;
    ///
    /// assert_eq!(ZoneEntry::new("UTC", 0).name, "UTC");
    /// ```
    pub fn new(name: impl Into<String>, offset_min: i32) -> Self {
        Self {
            name: name.into(),
            offset_min,
        }
    }

    /// `UTC±h[:mm]` label.
    fn zone_label(&self) -> String {
        let sign = if self.offset_min < 0 { '-' } else { '+' };
        let abs = self.offset_min.unsigned_abs();
        if abs.is_multiple_of(60) {
            format!("UTC{sign}{}", abs / 60)
        } else {
            format!("UTC{sign}{}:{:02}", abs / 60, abs % 60)
        }
    }
}

/// The clock — see the module docs.
///
/// ```
/// use martensite::widgets::world_clock::WorldClock;
///
/// assert_eq!(WorldClock::new().zone_count(), 0);
/// ```
pub struct WorldClock {
    /// Accessibility label.
    pub label: String,
    /// Show the day-shift marker (`+1d`/`-1d`).
    pub show_day_shift: bool,
    zones: Vec<ZoneEntry>,
    utc: Time,
    elapsed: Duration,
    text_painter: Option<SharedTextPainter>,
    bounds: Rect,
    scale: f32,
}

impl std::fmt::Debug for WorldClock {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("WorldClock")
            .field("zones", &self.zones.len())
            .field("utc", &self.utc)
            .finish()
    }
}

impl Default for WorldClock {
    fn default() -> Self {
        Self::new()
    }
}

impl WorldClock {
    /// Empty clock at `00:00` UTC.
    ///
    /// ```
    /// use martensite::widgets::world_clock::WorldClock;
    ///
    /// assert_eq!(WorldClock::new().zone_count(), 0);
    /// ```
    pub fn new() -> Self {
        Self {
            label: "World clock".to_string(),
            show_day_shift: true,
            zones: Vec::new(),
            utc: Time::default(),
            elapsed: Duration::ZERO,
            text_painter: None,
            bounds: Rect::new(0.0, 0.0, 0.0, 0.0),
            scale: 1.0,
        }
    }

    /// Appends a zone row.
    ///
    /// ```
    /// use martensite::widgets::world_clock::WorldClock;
    ///
    /// assert_eq!(WorldClock::new().zone("UTC", 0).zone_count(), 1);
    /// ```
    pub fn zone(mut self, name: impl Into<String>, offset_min: i32) -> Self {
        self.zones.push(ZoneEntry::new(name, offset_min));
        self
    }

    /// Accessibility label.
    ///
    /// ```
    /// use martensite::widgets::world_clock::WorldClock;
    ///
    /// assert_eq!(WorldClock::new().label("Zones").label, "Zones");
    /// ```
    pub fn label(mut self, label: impl Into<String>) -> Self {
        self.label = label.into();
        self
    }

    /// Shared text painter.
    ///
    /// ```no_run
    /// use martensite::widgets::world_clock::WorldClock;
    /// # fn example(p: martensite::text_paint::SharedTextPainter) {
    /// let _w = WorldClock::new().with_text_painter(p);
    /// # }
    /// ```
    pub fn with_text_painter(mut self, painter: SharedTextPainter) -> Self {
        self.text_painter = Some(painter);
        self
    }

    /// Zone count.
    ///
    /// ```
    /// use martensite::widgets::world_clock::WorldClock;
    ///
    /// assert_eq!(WorldClock::new().zone_count(), 0);
    /// ```
    pub fn zone_count(&self) -> usize {
        self.zones.len()
    }

    /// Current UTC base.
    ///
    /// ```
    /// use martensite::widgets::world_clock::WorldClock;
    /// use martensite::widgets::Time;
    ///
    /// let w = WorldClock::new().with_utc(Time { hour: 12, minute: 30 });
    /// assert_eq!(w.utc(), Time { hour: 12, minute: 30 });
    /// ```
    pub fn utc(&self) -> Time {
        self.utc
    }

    /// Seeds the UTC base (builder).
    ///
    /// ```
    /// use martensite::widgets::world_clock::WorldClock;
    /// use martensite::widgets::Time;
    ///
    /// assert_eq!(WorldClock::new().with_utc(Time { hour: 8, minute: 0 }).utc().hour, 8);
    /// ```
    pub fn with_utc(mut self, utc: Time) -> Self {
        self.utc = utc;
        self
    }

    /// Sets the UTC base (host-driven).
    ///
    /// ```
    /// use martensite::widgets::world_clock::WorldClock;
    /// use martensite::widgets::Time;
    ///
    /// let mut w = WorldClock::new();
    /// w.set_utc(Time { hour: 1, minute: 15 });
    /// assert_eq!(w.utc().minute, 15);
    /// ```
    pub fn set_utc(&mut self, utc: Time) {
        self.utc = utc;
    }

    /// Local time for zone `i` (offset applied, wrapped to 24h).
    ///
    /// ```
    /// use martensite::widgets::world_clock::WorldClock;
    /// use martensite::widgets::Time;
    ///
    /// let w = WorldClock::new()
    ///     .with_utc(Time { hour: 12, minute: 0 })
    ///     .zone("Tokyo", 540);
    /// assert_eq!(w.local(0), Some(Time { hour: 21, minute: 0 }));
    /// ```
    pub fn local(&self, i: usize) -> Option<Time> {
        self.zones.get(i).map(|z| {
            let total = self.utc.hour as i32 * 60 + self.utc.minute as i32 + z.offset_min;
            let m = total.rem_euclid(24 * 60);
            Time {
                hour: (m / 60) as u32,
                minute: (m % 60) as u32,
            }
        })
    }

    /// Day shift for zone `i`: `-1`, `0`, or `+1`.
    ///
    /// ```
    /// use martensite::widgets::world_clock::WorldClock;
    /// use martensite::widgets::Time;
    ///
    /// let w = WorldClock::new()
    ///     .with_utc(Time { hour: 23, minute: 30 })
    ///     .zone("Tokyo", 540);
    /// assert_eq!(w.day_shift(0), Some(1));
    /// ```
    pub fn day_shift(&self, i: usize) -> Option<i8> {
        self.zones.get(i).map(|z| {
            let total = self.utc.hour as i32 * 60 + self.utc.minute as i32 + z.offset_min;
            if total < 0 {
                -1
            } else if total >= 24 * 60 {
                1
            } else {
                0
            }
        })
    }
}

impl Widget for WorldClock {
    fn measure(&mut self, cx: &mut LayoutContext, constraints: LayoutConstraints) -> Vec2 {
        let h = (self.zones.len().max(1) as f32 * ROW_PT + PAD_PT) * cx.scale;
        Vec2::new(
            (280.0 * cx.scale).min(constraints.max_size.x.max(0.0)),
            h.min(constraints.max_size.y.max(0.0)),
        )
    }

    fn min_render(&self) -> RenderMinimum {
        RenderMinimum::new(Vec2::new(200.0, 60.0)).with_policy(UnderflowPolicy::Lint)
    }

    fn layout(&mut self, cx: &mut LayoutContext, bounds: Rect) {
        self.bounds = bounds;
        self.scale = cx.scale;
    }

    fn accessibility(&self, node: &mut AccessKitNode) {
        node.set_role(accesskit::Role::List);
        node.set_label(self.label.clone());
        node.set_value(format!("{} zones", self.zones.len()));
    }

    fn tick(&mut self, dt: Duration) -> bool {
        self.elapsed += dt;
        let mins = self.elapsed.as_secs() / 60;
        if mins == 0 {
            return false;
        }
        self.elapsed -= Duration::from_secs(mins * 60);
        let total = (self.utc.hour * 60 + self.utc.minute + mins as u32) % (24 * 60);
        self.utc = Time {
            hour: total / 60,
            minute: total % 60,
        };
        true
    }

    fn event(&mut self, _cx: &mut EventContext) -> EventResponse {
        EventResponse::Ignored
    }

    fn paint(&self, cx: &mut PaintContext) {
        let s = cx.scale;
        let painter = crate::text_paint::resolve_painter(&self.text_painter, cx.text_painter);
        cx.list.push_fill_rect(
            kurbo::Rect::new(
                f64::from(self.bounds.min_x()),
                f64::from(self.bounds.min_y()),
                f64::from(self.bounds.max_x()),
                f64::from(self.bounds.max_y()),
            ),
            cx.color(TokenKey::SurfaceColor, FACE),
        );
        let mut y = self.bounds.min_y();
        for (i, z) in self.zones.iter().enumerate() {
            let row_mid = y + ROW_PT * s * 0.62;
            // City name.
            crate::text_paint::paint_label(
                painter,
                cx.list,
                kurbo::Point::new(
                    f64::from(self.bounds.min_x() + PAD_PT * s),
                    f64::from(row_mid),
                ),
                &z.name,
                NAME_PT * s,
                cx.color(TokenKey::TextColor, TEXT),
            );
            // Zone offset under the name.
            crate::text_paint::paint_label(
                painter,
                cx.list,
                kurbo::Point::new(
                    f64::from(self.bounds.min_x() + PAD_PT * s),
                    f64::from(y + ROW_PT * s * 0.95),
                ),
                &z.zone_label(),
                ZONE_PT * s,
                cx.color(TokenKey::TextMutedColor, MUTED),
            );
            // Local time right-aligned, with optional day shift.
            if let Some(local) = self.local(i) {
                let t = format!("{:02}:{:02}", local.hour, local.minute);
                let fs = TIME_PT * s;
                let tw = painter
                    .and_then(|p| p.measure_text(&t, fs))
                    .unwrap_or(t.len() as f32 * fs * 0.6);
                let shift = self.day_shift(i).unwrap_or(0);
                let shift_txt = if self.show_day_shift && shift != 0 {
                    format!("{:+}d ", shift)
                } else {
                    String::new()
                };
                let sw = painter
                    .and_then(|p| p.measure_text(&shift_txt, ZONE_PT * s))
                    .unwrap_or(shift_txt.len() as f32 * ZONE_PT * 0.6 * s);
                let right = self.bounds.max_x() - PAD_PT * s;
                crate::text_paint::paint_label(
                    painter,
                    cx.list,
                    kurbo::Point::new(f64::from(right - tw), f64::from(row_mid)),
                    &t,
                    fs,
                    cx.color(TokenKey::TextColor, TEXT),
                );
                if !shift_txt.is_empty() {
                    crate::text_paint::paint_label(
                        painter,
                        cx.list,
                        kurbo::Point::new(f64::from(right - tw - sw - 4.0 * s), f64::from(row_mid)),
                        &shift_txt,
                        ZONE_PT * s,
                        SHIFT,
                    );
                }
            }
            // Row divider.
            if i + 1 < self.zones.len() {
                let dy = y + ROW_PT * s;
                cx.list.push_fill_rect(
                    kurbo::Rect::new(
                        f64::from(self.bounds.min_x() + PAD_PT * s),
                        f64::from(dy),
                        f64::from(self.bounds.max_x() - PAD_PT * s),
                        f64::from(dy + s.max(1.0)),
                    ),
                    cx.color(TokenKey::DividerColor, LINE),
                );
            }
            y += ROW_PT * s;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use martensite_core::HotNode;

    fn fixture() -> WorldClock {
        WorldClock::new()
            .with_utc(Time {
                hour: 23,
                minute: 30,
            })
            .zone("UTC", 0)
            .zone("Tokyo", 540)
            .zone("New York", -300)
    }

    #[test]
    fn local_applies_offset() {
        let w = fixture();
        assert_eq!(
            w.local(0),
            Some(Time {
                hour: 23,
                minute: 30
            })
        );
        assert_eq!(
            w.local(1),
            Some(Time {
                hour: 8,
                minute: 30
            })
        );
        assert_eq!(
            w.local(2),
            Some(Time {
                hour: 18,
                minute: 30
            })
        );
    }

    #[test]
    fn day_shift_wraps() {
        let w = fixture();
        assert_eq!(w.day_shift(1), Some(1)); // Tokyo is tomorrow
        assert_eq!(w.day_shift(2), Some(0));
        let mut w2 = WorldClock::new().zone("LA", -480);
        w2.set_utc(Time { hour: 1, minute: 0 });
        assert_eq!(w2.day_shift(0), Some(-1));
    }

    #[test]
    fn tick_advances_minutes() {
        let mut w = WorldClock::new().with_utc(Time { hour: 0, minute: 0 });
        assert!(!w.tick(Duration::from_secs(30)));
        assert!(w.tick(Duration::from_secs(60)));
        assert_eq!(w.utc().minute, 1);
    }

    #[test]
    fn paint_without_painter() {
        let mut w = fixture();
        let mut hot = HotNode::default();
        let mut cx = LayoutContext {
            hot: &mut hot,
            scale: 1.0,
        };
        w.layout(&mut cx, Rect::new(0.0, 0.0, 280.0, 200.0));
        let mut list = martensite_core::PaintList::default();
        let theme = martensite_theme::Theme::new("test");
        w.paint(&mut PaintContext {
            list: &mut list,
            bounds: w.bounds,
            theme: &theme,
            scale: 1.0,
            text_painter: None,
        });
        assert!(!list.is_empty());
    }
}
