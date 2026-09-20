//! `Stopwatch` — a tick-driven lap timer (the phone-clock / sprint
//! timing idiom).
//!
//! [`Stopwatch::tick`] accumulates elapsed time while running.
//! `Space`/`Enter` toggles start/stop, `l` records a lap into
//! [`Stopwatch::take_lapped`], `r` resets. Laps store the split time
//! (elapsed at press); [`Stopwatch::lap_at`] returns splits in order.
//!
//! # Examples
//!
//! ```
//! use std::time::Duration;
//! use martensite::widgets::stopwatch::Stopwatch;
//! use martensite_core::Widget;
//!
//! let mut s = Stopwatch::new().running(true);
//! s.tick(Duration::from_millis(1500));
//! assert_eq!(s.elapsed(), Duration::from_millis(1500));
//! assert_eq!(s.face(), "00:01.50");
//! ```

use std::time::Duration;

use accesskit::Node as AccessKitNode;
use glam::Vec2;
use martensite_core::{
    EventContext, EventResponse, LayoutConstraints, LayoutContext, PaintContext, Rect,
    RenderMinimum, UnderflowPolicy, Widget, WidgetEvent,
};
use martensite_theme::TokenKey;

const FACE_PT: f32 = 22.0;
const PAD_PT: f32 = 8.0;

const FG: [u8; 4] = [230, 230, 235, 255];
const DIM: [u8; 4] = [130, 130, 138, 255];
const LAP: [u8; 4] = [96, 165, 250, 255];

/// A tick-driven lap timer — see the module docs.
///
/// ```
/// use martensite::widgets::stopwatch::Stopwatch;
///
/// assert_eq!(Stopwatch::new().lap_count(), 0);
/// ```
pub struct Stopwatch {
    /// Accessibility label.
    pub label: String,
    elapsed: Duration,
    running: bool,
    /// Split times (elapsed at each lap press).
    laps: Vec<Duration>,
    lapped: Option<Duration>,
    bounds: Rect,
    scale: f32,
    text_painter: Option<crate::text_paint::SharedTextPainter>,
}

impl std::fmt::Debug for Stopwatch {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Stopwatch")
            .field("elapsed", &self.elapsed)
            .field("running", &self.running)
            .finish()
    }
}

impl Default for Stopwatch {
    fn default() -> Self {
        Self::new()
    }
}

impl Stopwatch {
    /// Stopped timer at zero.
    ///
    /// ```
    /// use martensite::widgets::stopwatch::Stopwatch;
    ///
    /// assert!(!Stopwatch::new().is_running());
    /// ```
    pub fn new() -> Self {
        Self {
            label: "Stopwatch".to_string(),
            elapsed: Duration::ZERO,
            running: false,
            laps: Vec::new(),
            lapped: None,
            bounds: Rect::new(0.0, 0.0, 0.0, 0.0),
            scale: 1.0,
            text_painter: None,
        }
    }

    /// Accessibility label.
    ///
    /// ```
    /// use martensite::widgets::stopwatch::Stopwatch;
    ///
    /// assert_eq!(Stopwatch::new().label("Sprint").label, "Sprint");
    /// ```
    pub fn label(mut self, label: impl Into<String>) -> Self {
        self.label = label.into();
        self
    }

    /// Custom text painter.
    ///
    /// ```
    /// use martensite::widgets::stopwatch::Stopwatch;
    ///
    /// let _ = Stopwatch::new(); // painter optional
    /// ```
    pub fn with_text_painter(mut self, p: crate::text_paint::SharedTextPainter) -> Self {
        self.text_painter = Some(p);
        self
    }

    /// Starts (or leaves stopped) the timer.
    ///
    /// ```
    /// use martensite::widgets::stopwatch::Stopwatch;
    ///
    /// assert!(Stopwatch::new().running(true).is_running());
    /// ```
    pub fn running(mut self, on: bool) -> Self {
        self.running = on;
        self
    }

    /// Accumulated elapsed time.
    ///
    /// ```
    /// use std::time::Duration;
    /// use martensite::widgets::stopwatch::Stopwatch;
    ///
    /// assert_eq!(Stopwatch::new().elapsed(), Duration::ZERO);
    /// ```
    pub fn elapsed(&self) -> Duration {
        self.elapsed
    }

    /// Whether time is accumulating.
    ///
    /// ```
    /// use martensite::widgets::stopwatch::Stopwatch;
    ///
    /// assert!(!Stopwatch::new().is_running());
    /// ```
    pub fn is_running(&self) -> bool {
        self.running
    }

    /// Starts the timer.
    ///
    /// ```
    /// use martensite::widgets::stopwatch::Stopwatch;
    ///
    /// let mut s = Stopwatch::new();
    /// s.start();
    /// assert!(s.is_running());
    /// ```
    pub fn start(&mut self) {
        self.running = true;
    }

    /// Stops the timer (elapsed is preserved).
    ///
    /// ```
    /// use martensite::widgets::stopwatch::Stopwatch;
    ///
    /// let mut s = Stopwatch::new().running(true);
    /// s.stop();
    /// assert!(!s.is_running());
    /// ```
    pub fn stop(&mut self) {
        self.running = false;
    }

    /// Toggles start/stop.
    ///
    /// ```
    /// use martensite::widgets::stopwatch::Stopwatch;
    ///
    /// let mut s = Stopwatch::new();
    /// s.toggle();
    /// assert!(s.is_running());
    /// s.toggle();
    /// assert!(!s.is_running());
    /// ```
    pub fn toggle(&mut self) {
        self.running = !self.running;
    }

    /// Records the current elapsed as a lap split and parks it in
    /// [`Stopwatch::take_lapped`].
    ///
    /// ```
    /// use martensite::widgets::stopwatch::Stopwatch;
    ///
    /// let mut s = Stopwatch::new();
    /// s.lap();
    /// assert_eq!(s.lap_count(), 1);
    /// ```
    pub fn lap(&mut self) {
        self.laps.push(self.elapsed);
        self.lapped = Some(self.elapsed);
    }

    /// Resets elapsed and laps (running state is preserved).
    ///
    /// ```
    /// use martensite::widgets::stopwatch::Stopwatch;
    ///
    /// let mut s = Stopwatch::new().running(true);
    /// s.lap();
    /// s.reset();
    /// assert_eq!(s.lap_count(), 0);
    /// assert!(s.is_running());
    /// ```
    pub fn reset(&mut self) {
        self.elapsed = Duration::ZERO;
        self.laps.clear();
        self.lapped = None;
    }

    /// Lap count.
    ///
    /// ```
    /// use martensite::widgets::stopwatch::Stopwatch;
    ///
    /// assert_eq!(Stopwatch::new().lap_count(), 0);
    /// ```
    pub fn lap_count(&self) -> usize {
        self.laps.len()
    }

    /// Split recorded at lap `i`.
    ///
    /// ```
    /// use martensite::widgets::stopwatch::Stopwatch;
    ///
    /// let mut s = Stopwatch::new();
    /// s.lap();
    /// assert_eq!(s.lap_at(0), Some(std::time::Duration::ZERO));
    /// ```
    pub fn lap_at(&self, i: usize) -> Option<Duration> {
        self.laps.get(i).copied()
    }

    /// Drains the last lap split.
    ///
    /// ```
    /// use martensite::widgets::stopwatch::Stopwatch;
    ///
    /// assert_eq!(Stopwatch::new().take_lapped(), None);
    /// ```
    pub fn take_lapped(&mut self) -> Option<Duration> {
        self.lapped.take()
    }

    /// `MM:SS.cs` face (`H:MM:SS` past the hour).
    ///
    /// ```
    /// use std::time::Duration;
    /// use martensite::widgets::stopwatch::Stopwatch;
    ///
    /// assert_eq!(Stopwatch::fmt_face(Duration::from_millis(61500)), "01:01.50");
    /// ```
    pub fn fmt_face(d: Duration) -> String {
        let cs = d.subsec_millis() / 10;
        let secs = d.as_secs() % 60;
        let mins = (d.as_secs() / 60) % 60;
        let hours = d.as_secs() / 3600;
        if hours > 0 {
            format!("{hours}:{mins:02}:{secs:02}")
        } else {
            format!("{mins:02}:{secs:02}.{cs:02}")
        }
    }

    /// The face string for the current elapsed time.
    ///
    /// ```
    /// use martensite::widgets::stopwatch::Stopwatch;
    ///
    /// assert_eq!(Stopwatch::new().face(), "00:00.00");
    /// ```
    pub fn face(&self) -> String {
        Self::fmt_face(self.elapsed)
    }
}

impl Widget for Stopwatch {
    fn measure(&mut self, cx: &mut LayoutContext, constraints: LayoutConstraints) -> Vec2 {
        Vec2::new(
            cx.pt(140.0).min(constraints.max_size.x.max(0.0)),
            cx.pt(FACE_PT + PAD_PT * 2.0)
                .min(constraints.max_size.y.max(0.0)),
        )
    }

    fn min_render(&self) -> RenderMinimum {
        RenderMinimum::new(Vec2::new(80.0, FACE_PT + 4.0)).with_policy(UnderflowPolicy::Lint)
    }

    fn layout(&mut self, cx: &mut LayoutContext, bounds: Rect) {
        self.bounds = bounds;
        self.scale = cx.scale;
    }

    fn accessibility(&self, node: &mut AccessKitNode) {
        node.set_role(accesskit::Role::Time);
        node.set_label(format!("{} — {}", self.label, self.face()));
    }

    fn event(&mut self, cx: &mut EventContext) -> EventResponse {
        match cx.event {
            WidgetEvent::KeyPressed { key, .. } => match key.as_str() {
                " " | "Enter" => {
                    self.toggle();
                    EventResponse::RequestRepaint
                }
                "l" | "L" => {
                    self.lap();
                    EventResponse::RequestRepaint
                }
                "r" | "R" => {
                    self.reset();
                    EventResponse::RequestRepaint
                }
                _ => EventResponse::Ignored,
            },
            _ => EventResponse::Ignored,
        }
    }

    fn tick(&mut self, dt: Duration) -> bool {
        if self.running {
            self.elapsed += dt;
            return true;
        }
        false
    }

    fn paint(&self, cx: &mut PaintContext) {
        let painter = crate::text_paint::resolve_painter(&self.text_painter, cx.text_painter);
        let s = self.scale;
        let size = FACE_PT * s;
        let face = self.face();
        let w = painter
            .and_then(|p| p.measure_text(&face, size))
            .unwrap_or(face.len() as f32 * size * 0.6);
        let x = self.bounds.min_x() + (self.bounds.width() - w).max(0.0) / 2.0;
        let y = self.bounds.min_y() + (self.bounds.height() - size).max(0.0) / 2.0;
        crate::text_paint::paint_label_clipped(
            painter,
            cx.list,
            kurbo::Rect::new(
                f64::from(self.bounds.min_x()),
                f64::from(self.bounds.min_y()),
                f64::from(self.bounds.max_x()),
                f64::from(self.bounds.max_y()),
            ),
            kurbo::Point::new(f64::from(x), f64::from(y)),
            &face,
            size,
            cx.color(TokenKey::TextColor, if self.running { FG } else { DIM }),
        );
        // Lap pips along the bottom — one dot per recorded split.
        if !self.laps.is_empty() {
            let d = 4.0 * s;
            let total = self.laps.len() as f32 * d * 1.6 - d * 0.6;
            let mut x = self.bounds.min_x() + (self.bounds.width() - total).max(0.0) / 2.0;
            for _ in &self.laps {
                cx.list.push_fill_shape(
                    kurbo::Rect::new(
                        f64::from(x),
                        f64::from(self.bounds.max_y() - d * 1.8),
                        f64::from(d),
                        f64::from(d),
                    ),
                    &martensite_core::shape::Shape::ELLIPSE,
                    cx.color(TokenKey::AccentColor, LAP),
                );
                x += d * 1.6;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ev(s: &mut Stopwatch, key: &str) {
        s.event(&mut EventContext {
            event: &WidgetEvent::KeyPressed {
                key: key.to_string(),
                repeat: false,
            },
            bounds: s.bounds,
            scale: 1.0,
        });
    }

    #[test]
    fn tick_accumulates_only_while_running() {
        let mut s = Stopwatch::new();
        assert!(!s.tick(Duration::from_millis(100)));
        assert_eq!(s.elapsed(), Duration::ZERO);
        s.start();
        assert!(s.tick(Duration::from_millis(250)));
        assert_eq!(s.elapsed(), Duration::from_millis(250));
        s.stop();
        assert!(!s.tick(Duration::from_millis(250)));
        assert_eq!(s.elapsed(), Duration::from_millis(250));
    }

    #[test]
    fn face_formats() {
        assert_eq!(Stopwatch::fmt_face(Duration::ZERO), "00:00.00");
        assert_eq!(
            Stopwatch::fmt_face(Duration::from_millis(59990)),
            "00:59.99"
        );
        assert_eq!(Stopwatch::fmt_face(Duration::from_secs(3600)), "1:00:00");
    }

    #[test]
    fn lap_records_splits() {
        let mut s = Stopwatch::new().running(true);
        s.tick(Duration::from_secs(2));
        ev(&mut s, "l");
        s.tick(Duration::from_secs(1));
        ev(&mut s, "l");
        assert_eq!(s.lap_count(), 2);
        assert_eq!(s.lap_at(0), Some(Duration::from_secs(2)));
        assert_eq!(s.lap_at(1), Some(Duration::from_secs(3)));
    }

    #[test]
    fn space_toggles_and_reset_keeps_state() {
        let mut s = Stopwatch::new();
        ev(&mut s, " ");
        assert!(s.is_running());
        ev(&mut s, " ");
        assert!(!s.is_running());
        ev(&mut s, "Enter");
        assert!(s.is_running());
        ev(&mut s, "l");
        ev(&mut s, "r");
        assert_eq!(s.lap_count(), 0);
        assert_eq!(s.elapsed(), Duration::ZERO);
        assert!(s.is_running());
    }
}
