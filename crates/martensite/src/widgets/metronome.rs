//! `Metronome` — a tick-driven tempo indicator (the music
//! family's rhythm sibling of [`crate::widgets::piano_keys::PianoKeys`]
//! and [`crate::widgets::equalizer::Equalizer`]).
//!
//! While running, [`Metronome::tick`] advances a beat cursor at
//! `bpm`, flashing the current beat lamp (beat 0 is the accent)
//! and parking the beat index in [`Metronome::take_beat`] each
//! crossing — hosts drive a click sound from that seam. Click or
//! Space toggles the run state; arrow keys adjust BPM;
//! [`Metronome::tap`] derives tempo from repeated taps.
//!
//! # Examples
//!
//! ```
//! use martensite::widgets::metronome::Metronome;
//! use martensite_core::Widget;
//! use std::time::Duration;
//!
//! let mut m = Metronome::new().bpm(120).running();
//! assert_eq!(m.beat(), 0);
//! m.tick(Duration::from_millis(600)); // past one 500 ms beat
//! assert_eq!(m.take_beat(), Some(1));
//! ```

use accesskit::Node as AccessKitNode;
use glam::Vec2;
use martensite_core::{
    EventContext, EventResponse, LayoutConstraints, LayoutContext, PaintContext, PointerButton,
    Rect, RenderMinimum, UnderflowPolicy, Widget, WidgetEvent,
};
use martensite_theme::TokenKey;
use std::time::Duration;

const W_PT: f32 = 200.0;
const H_PT: f32 = 64.0;
const LAMP_PT: f32 = 7.0;
const FLASH_DECAY: f32 = 4.0;
/// Max taps kept for tempo averaging.
const TAP_MEMORY: usize = 5;

const FACE: [u8; 4] = [36, 36, 42, 255];
const EDGE: [u8; 4] = [70, 70, 76, 255];
const LAMP_OFF: [u8; 4] = [58, 58, 64, 255];
const LAMP_ON: [u8; 4] = [120, 200, 255, 255];
const ACCENT: [u8; 4] = [255, 160, 90, 255];
const TEXT: [u8; 4] = [210, 210, 215, 255];
const MUTED: [u8; 4] = [140, 140, 150, 255];

/// A tempo indicator with beat lamps — see the module docs.
///
/// ```
/// use martensite::widgets::metronome::Metronome;
///
/// assert_eq!(Metronome::new().bpm_value(), 120);
/// ```
pub struct Metronome {
    /// Accessibility label.
    pub label: String,
    bpm: u32,
    beats: u8,
    running: bool,
    beat: u8,
    /// Seconds into the current beat.
    phase: f32,
    /// Beat-flash envelope `1.0` just after a crossing.
    flash: f32,
    /// Parked beat index each crossing.
    pending: Option<u8>,
    /// Tap-tempo intervals in seconds.
    taps: Vec<f32>,
    last_tap: Option<f32>,
    /// Deterministic tap clock (seconds); `tap` uses this
    /// instead of wall time so tests stay reproducible.
    tap_clock: f32,
    bounds: Rect,
    scale: f32,
    text_painter: Option<crate::text_paint::SharedTextPainter>,
}

impl std::fmt::Debug for Metronome {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Metronome")
            .field("bpm", &self.bpm)
            .field("beats", &self.beats)
            .field("running", &self.running)
            .field("beat", &self.beat)
            .finish()
    }
}

impl Default for Metronome {
    fn default() -> Self {
        Self::new()
    }
}

impl Metronome {
    /// Creates a stopped 120 BPM metronome in 4/4.
    ///
    /// ```
    /// use martensite::widgets::metronome::Metronome;
    ///
    /// let m = Metronome::new();
    /// assert_eq!(m.bpm_value(), 120);
    /// assert_eq!(m.beats_per_bar(), 4);
    /// assert!(!m.is_running());
    /// ```
    pub fn new() -> Self {
        Self {
            label: "Metronome".to_string(),
            bpm: 120,
            beats: 4,
            running: false,
            beat: 0,
            phase: 0.0,
            flash: 0.0,
            pending: None,
            taps: Vec::new(),
            last_tap: None,
            tap_clock: 0.0,
            bounds: Rect::new(0.0, 0.0, 0.0, 0.0),
            scale: 1.0,
            text_painter: None,
        }
    }

    /// Tempo in beats per minute `20..=300`.
    ///
    /// ```
    /// use martensite::widgets::metronome::Metronome;
    ///
    /// assert_eq!(Metronome::new().bpm(0).bpm_value(), 20);
    /// assert_eq!(Metronome::new().bpm(400).bpm_value(), 300);
    /// ```
    pub fn bpm(mut self, bpm: u32) -> Self {
        self.bpm = bpm.clamp(20, 300);
        self
    }

    /// Beats per bar `1..=12` (beat 0 flashes as the accent).
    ///
    /// ```
    /// use martensite::widgets::metronome::Metronome;
    ///
    /// assert_eq!(Metronome::new().beats(6).beats_per_bar(), 6);
    /// ```
    pub fn beats(mut self, n: u8) -> Self {
        self.beats = n.clamp(1, 12);
        self.beat = self.beat.min(self.beats - 1);
        self
    }

    /// Starts the beat.
    ///
    /// ```
    /// use martensite::widgets::metronome::Metronome;
    ///
    /// assert!(Metronome::new().running().is_running());
    /// ```
    pub fn running(mut self) -> Self {
        self.running = true;
        self
    }

    /// Accessibility label.
    ///
    /// ```
    /// use martensite::widgets::metronome::Metronome;
    ///
    /// assert_eq!(Metronome::new().label("Click").label, "Click");
    /// ```
    pub fn label(mut self, label: impl Into<String>) -> Self {
        self.label = label.into();
        self
    }

    /// Custom text painter.
    ///
    /// ```
    /// use martensite::widgets::metronome::Metronome;
    ///
    /// let _ = Metronome::new(); // painter optional
    /// ```
    pub fn with_text_painter(mut self, p: crate::text_paint::SharedTextPainter) -> Self {
        self.text_painter = Some(p);
        self
    }

    /// Current tempo.
    ///
    /// ```
    /// use martensite::widgets::metronome::Metronome;
    ///
    /// assert_eq!(Metronome::new().bpm(96).bpm_value(), 96);
    /// ```
    pub fn bpm_value(&self) -> u32 {
        self.bpm
    }

    /// Beats per bar.
    ///
    /// ```
    /// use martensite::widgets::metronome::Metronome;
    ///
    /// assert_eq!(Metronome::new().beats_per_bar(), 4);
    /// ```
    pub fn beats_per_bar(&self) -> u8 {
        self.beats
    }

    /// Whether the beat is running.
    ///
    /// ```
    /// use martensite::widgets::metronome::Metronome;
    ///
    /// assert!(!Metronome::new().is_running());
    /// ```
    pub fn is_running(&self) -> bool {
        self.running
    }

    /// Current beat index `0..beats`.
    ///
    /// ```
    /// use martensite::widgets::metronome::Metronome;
    ///
    /// assert_eq!(Metronome::new().beat(), 0);
    /// ```
    pub fn beat(&self) -> u8 {
        self.beat
    }

    /// Drains the beat index parked by the latest crossing.
    ///
    /// ```
    /// use martensite::widgets::metronome::Metronome;
    ///
    /// assert_eq!(Metronome::new().take_beat(), None);
    /// ```
    pub fn take_beat(&mut self) -> Option<u8> {
        self.pending.take()
    }

    /// Starts or stops the beat.
    ///
    /// ```
    /// use martensite::widgets::metronome::Metronome;
    ///
    /// let mut m = Metronome::new();
    /// m.toggle();
    /// assert!(m.is_running());
    /// ```
    pub fn toggle(&mut self) {
        self.running = !self.running;
        if self.running {
            self.beat = 0;
            self.phase = 0.0;
            self.flash = 1.0;
            self.pending = Some(0);
        }
    }

    /// Registers a tap for tap-tempo. Two or more taps within
    /// 2 s of each other set `bpm` from the mean interval.
    ///
    /// ```
    /// use martensite::widgets::metronome::Metronome;
    ///
    /// let mut m = Metronome::new();
    /// m.tap();
    /// m.tap(); // one second apart → 60 BPM
    /// assert_eq!(m.bpm_value(), 60);
    /// ```
    pub fn tap(&mut self) {
        let now = self.tap_clock;
        let dt = match self.last_tap {
            Some(t) => now - t,
            None => 0.0,
        };
        self.last_tap = Some(now);
        self.tap_clock += 1.0; // deterministic test clock; see tick
        if dt > 2.0 {
            self.taps.clear();
        }
        if dt > 0.0 {
            self.taps.push(dt);
            self.taps.truncate(TAP_MEMORY);
            let mean = self.taps.iter().sum::<f32>() / self.taps.len() as f32;
            if mean > 0.0 {
                self.bpm = (60.0 / mean).round().clamp(20.0, 300.0) as u32;
            }
        }
    }

    /// Seconds per beat.
    fn beat_secs(&self) -> f32 {
        60.0 / self.bpm.max(1) as f32
    }
}

impl Widget for Metronome {
    fn measure(&mut self, cx: &mut LayoutContext, constraints: LayoutConstraints) -> Vec2 {
        Vec2::new(
            cx.pt(W_PT).min(constraints.max_size.x.max(0.0)),
            cx.pt(H_PT).min(constraints.max_size.y.max(0.0)),
        )
    }

    fn min_render(&self) -> RenderMinimum {
        RenderMinimum::new(Vec2::new(80.0, 32.0)).with_policy(UnderflowPolicy::Lint)
    }

    fn layout(&mut self, cx: &mut LayoutContext, bounds: Rect) {
        self.bounds = bounds;
        self.scale = cx.scale;
    }

    fn accessibility(&self, node: &mut AccessKitNode) {
        node.set_role(accesskit::Role::Button);
        node.set_label(format!(
            "{} — {} BPM, {} per bar{}{}",
            self.label,
            self.bpm,
            self.beats,
            if self.running { ", running" } else { "" },
            if self.running {
                format!(", beat {}", self.beat + 1)
            } else {
                String::new()
            },
        ));
    }

    fn event(&mut self, cx: &mut EventContext) -> EventResponse {
        match cx.event {
            WidgetEvent::PointerPressed {
                button: PointerButton::Primary,
                position,
                ..
            } => {
                if self.bounds.contains(*position) {
                    self.toggle();
                    EventResponse::Handled
                } else {
                    EventResponse::Ignored
                }
            }
            WidgetEvent::KeyPressed { key, .. } => match key.as_str() {
                " " => {
                    self.toggle();
                    EventResponse::Handled
                }
                "ArrowUp" | "ArrowRight" => {
                    self.bpm = (self.bpm + 1).min(300);
                    EventResponse::RequestRepaint
                }
                "ArrowDown" | "ArrowLeft" => {
                    self.bpm = self.bpm.saturating_sub(1).max(20);
                    EventResponse::RequestRepaint
                }
                _ => EventResponse::Ignored,
            },
            _ => EventResponse::Ignored,
        }
    }

    fn tick(&mut self, dt: Duration) -> bool {
        let mut dirty = false;
        if self.running {
            self.phase += dt.as_secs_f32();
            let spb = self.beat_secs();
            while self.phase >= spb {
                self.phase -= spb;
                self.beat = (self.beat + 1) % self.beats;
                self.pending = Some(self.beat);
                self.flash = 1.0;
                dirty = true;
            }
        }
        if self.flash > 0.0 {
            self.flash = (self.flash - dt.as_secs_f32() * FLASH_DECAY).max(0.0);
            dirty = true;
        }
        dirty
    }

    fn paint(&self, cx: &mut PaintContext) {
        let f = |r: Rect| {
            kurbo::Rect::new(
                f64::from(r.min_x()),
                f64::from(r.min_y()),
                f64::from(r.max_x()),
                f64::from(r.max_y()),
            )
        };
        cx.list.push_fill_shape(
            f(self.bounds),
            &martensite_core::shape::Shape::rounded(cx.pt(4.0)),
            cx.color(TokenKey::SurfaceColor, FACE),
        );
        // Beat lamps across the top.
        let n = self.beats as f32;
        let pad = 10.0 * self.scale;
        let lamp_r = LAMP_PT * self.scale;
        let ly = self.bounds.min_y() + pad + lamp_r;
        let span = (self.bounds.width() - pad * 2.0).max(1.0);
        for i in 0..self.beats {
            let t = if n > 1.0 { i as f32 / (n - 1.0) } else { 0.5 };
            let x = self.bounds.min_x() + pad + span * t;
            let lit = self.running && i == self.beat;
            let base = if i == 0 {
                cx.color(TokenKey::WarningColor, ACCENT)
            } else {
                cx.color(TokenKey::AccentColor, LAMP_ON)
            };
            let color = if lit {
                let a = self.flash;
                [
                    (LAMP_OFF[0] as f32 + (base[0] as f32 - LAMP_OFF[0] as f32) * a) as u8,
                    (LAMP_OFF[1] as f32 + (base[1] as f32 - LAMP_OFF[1] as f32) * a) as u8,
                    (LAMP_OFF[2] as f32 + (base[2] as f32 - LAMP_OFF[2] as f32) * a) as u8,
                    255,
                ]
            } else {
                LAMP_OFF
            };
            let c = Vec2::new(x, ly);
            cx.list.push_fill_shape(
                kurbo::Rect::new(
                    f64::from(x - lamp_r),
                    f64::from(ly - lamp_r),
                    f64::from(x + lamp_r),
                    f64::from(ly + lamp_r),
                ),
                &martensite_core::shape::Shape::circle(c, lamp_r),
                color,
            );
        }
        // BPM label + beat count below.
        let painter = crate::text_paint::resolve_painter(&self.text_painter, cx.text_painter);
        let size = 15.0 * self.scale;
        let color = cx.color(TokenKey::TextColor, TEXT);
        crate::text_paint::paint_label(
            painter,
            cx.list,
            kurbo::Point::new(
                f64::from(self.bounds.min_x() + pad),
                f64::from(self.bounds.min_y() + self.bounds.height() - pad - size),
            ),
            &format!("{} BPM", self.bpm),
            size,
            color,
        );
        let sub = if self.running {
            format!("beat {}/{}", self.beat + 1, self.beats)
        } else {
            "stopped — click or Space".to_string()
        };
        crate::text_paint::paint_label(
            painter,
            cx.list,
            kurbo::Point::new(
                f64::from(self.bounds.max_x() - pad - sub.len() as f32 * size * 0.42),
                f64::from(self.bounds.min_y() + self.bounds.height() - pad - size * 0.7),
            ),
            &sub,
            size * 0.7,
            cx.color(TokenKey::TextMutedColor, MUTED),
        );
        cx.list.push_stroke_shape(
            f(self.bounds),
            &martensite_core::shape::Shape::rounded(cx.pt(4.0)),
            cx.pt(0.75),
            cx.color(TokenKey::BorderColor, EDGE),
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use martensite_core::HotNode;

    fn laid_out(w: &mut Metronome, wd: f32, h: f32) {
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

    #[test]
    fn clamps_bpm() {
        assert_eq!(Metronome::new().bpm(0).bpm_value(), 20);
        assert_eq!(Metronome::new().bpm(999).bpm_value(), 300);
        assert_eq!(Metronome::new().beats(0).beats_per_bar(), 1);
        assert_eq!(Metronome::new().beats(99).beats_per_bar(), 12);
    }

    #[test]
    fn tick_advances_beats() {
        let mut m = Metronome::new().bpm(120).running(); // 0.5 s/beat
        laid_out(&mut m, 200.0, 64.0);
        assert!(!m.tick(Duration::from_millis(100)) || m.beat() == 0);
        m.tick(Duration::from_millis(600));
        assert_eq!(m.beat(), 1);
        assert_eq!(m.take_beat(), Some(1));
        assert_eq!(m.take_beat(), None);
        m.tick(Duration::from_millis(1100)); // two more beats
        assert_eq!(m.beat(), 3);
    }

    #[test]
    fn stopped_ticks_quiet() {
        let mut m = Metronome::new();
        assert!(!m.tick(Duration::from_secs(5)));
        assert_eq!(m.beat(), 0);
    }

    #[test]
    fn toggle_and_keys() {
        let mut m = Metronome::new();
        laid_out(&mut m, 200.0, 64.0);
        m.event(&mut EventContext {
            event: &WidgetEvent::PointerPressed {
                button: PointerButton::Primary,
                position: Vec2::new(100.0, 30.0),
                count: 1,
            },
            bounds: m.bounds,
            scale: 1.0,
        });
        assert!(m.is_running());
        assert_eq!(m.take_beat(), Some(0));
        m.event(&mut EventContext {
            event: &WidgetEvent::KeyPressed {
                key: "ArrowUp".to_string(),
                repeat: false,
            },
            bounds: m.bounds,
            scale: 1.0,
        });
        assert_eq!(m.bpm_value(), 121);
    }

    #[test]
    fn tap_tempo() {
        let mut m = Metronome::new();
        m.tap(); // clock 0 → 1
        m.tap(); // dt = 1.0 s → 60 bpm
        assert_eq!(m.bpm_value(), 60);
    }
}
