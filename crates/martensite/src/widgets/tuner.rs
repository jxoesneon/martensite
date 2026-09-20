//! `Tuner` — a chromatic tuner display: detected note name plus a
//! ±50¢ deviation needle (instrument-tuner idiom).
//!
//! The host feeds pitch analysis via [`Tuner::set_pitch`] (or the
//! [`Tuner::note`]/[`Tuner::cents`] builders): a needle sweeps the
//! semicircular gauge — flat left, sharp right — turning green in
//! the in-tune band (`|cents| <= in_tune`, default 5¢). The note
//! name paints big in the middle; [`Tuner::take_steady`] parks a
//! flag once a reading holds inside the band for `steady_secs`.
//!
//! Companion to `PianoKeys`, `Equalizer`, `VuMeter`, `Metronome`,
//! and `Fretboard` in the music family.
//!
//! # Examples
//!
//! ```
//! use martensite::widgets::tuner::Tuner;
//!
//! let t = Tuner::new().note("A").cents(0.0);
//! assert!(t.in_tune());
//! ```

use accesskit::Node as AccessKitNode;
use glam::Vec2;
use martensite_core::{
    LayoutConstraints, LayoutContext, PaintContext, Rect, RenderMinimum, UnderflowPolicy, Widget,
};
use martensite_theme::TokenKey;
use std::time::Duration;

use crate::text_paint::SharedTextPainter;

const W_PT: f32 = 160.0;
const H_PT: f32 = 110.0;
const NOTE_PT: f32 = 26.0;
const TICK_PT: f32 = 6.0;

const TRACK: [u8; 4] = [70, 72, 80, 255];
const GOOD: [u8; 4] = [74, 222, 128, 255];
const BAD: [u8; 4] = [248, 113, 113, 255];
const NEEDLE: [u8; 4] = [240, 240, 245, 255];
const DIM: [u8; 4] = [120, 122, 130, 255];

/// A chromatic tuner — see the module docs.
///
/// ```
/// use martensite::widgets::tuner::Tuner;
///
/// assert_eq!(Tuner::new().note_name(), "—");
/// ```
pub struct Tuner {
    /// Accessibility label.
    pub label: String,
    /// |cents| threshold for the in-tune band.
    pub in_tune: f32,
    /// Seconds a reading must hold in-tune before `take_steady`.
    pub steady_secs: f32,
    note: String,
    cents: f32,
    /// Time the current reading has held inside the band.
    steady: f32,
    steady_fired: bool,
    bounds: Rect,
    scale: f32,
    text_painter: Option<SharedTextPainter>,
}

impl std::fmt::Debug for Tuner {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Tuner")
            .field("note", &self.note)
            .field("cents", &self.cents)
            .finish()
    }
}

impl Default for Tuner {
    fn default() -> Self {
        Self::new()
    }
}

impl Tuner {
    /// No signal (note `—`, needle centered).
    ///
    /// ```
    /// use martensite::widgets::tuner::Tuner;
    ///
    /// assert_eq!(Tuner::new().cents_value(), 0.0);
    /// ```
    pub fn new() -> Self {
        Self {
            label: "Tuner".to_string(),
            in_tune: 5.0,
            steady_secs: 1.0,
            note: "—".to_string(),
            cents: 0.0,
            steady: 0.0,
            steady_fired: false,
            bounds: Rect::new(0.0, 0.0, 0.0, 0.0),
            scale: 1.0,
            text_painter: None,
        }
    }

    /// Accessibility label.
    ///
    /// ```
    /// use martensite::widgets::tuner::Tuner;
    ///
    /// assert_eq!(Tuner::new().label("Guitar").label, "Guitar");
    /// ```
    pub fn label(mut self, label: impl Into<String>) -> Self {
        self.label = label.into();
        self
    }

    /// Detected note name builder.
    ///
    /// ```
    /// use martensite::widgets::tuner::Tuner;
    ///
    /// assert_eq!(Tuner::new().note("E2").note_name(), "E2");
    /// ```
    pub fn note(mut self, note: impl Into<String>) -> Self {
        self.note = note.into();
        self
    }

    /// Cents deviation builder (`-50..=50`, clamped).
    ///
    /// ```
    /// use martensite::widgets::tuner::Tuner;
    ///
    /// assert_eq!(Tuner::new().cents(80.0).cents_value(), 50.0);
    /// ```
    pub fn cents(mut self, cents: f32) -> Self {
        self.cents = cents.clamp(-50.0, 50.0);
        self
    }

    /// In-tune band width builder (±cents).
    ///
    /// ```
    /// use martensite::widgets::tuner::Tuner;
    ///
    /// assert_eq!(Tuner::new().band(10.0).in_tune, 10.0);
    /// ```
    pub fn band(mut self, cents: f32) -> Self {
        self.in_tune = cents.clamp(1.0, 25.0);
        self
    }

    /// Optional painter override (tests / headless).
    pub fn with_text_painter(mut self, painter: SharedTextPainter) -> Self {
        self.text_painter = Some(painter);
        self
    }

    /// Feeds a pitch reading.
    ///
    /// ```
    /// use martensite::widgets::tuner::Tuner;
    ///
    /// let mut t = Tuner::new();
    /// t.set_pitch("G3", -12.0);
    /// assert_eq!(t.note_name(), "G3");
    /// assert_eq!(t.cents_value(), -12.0);
    /// ```
    pub fn set_pitch(&mut self, note: impl Into<String>, cents: f32) {
        self.note = note.into();
        self.cents = cents.clamp(-50.0, 50.0);
        self.steady = 0.0;
        self.steady_fired = false;
    }

    /// Clears the reading back to no-signal.
    ///
    /// ```
    /// use martensite::widgets::tuner::Tuner;
    ///
    /// let mut t = Tuner::new().note("A");
    /// t.clear();
    /// assert_eq!(t.note_name(), "—");
    /// ```
    pub fn clear(&mut self) {
        self.note = "—".to_string();
        self.cents = 0.0;
        self.steady = 0.0;
        self.steady_fired = false;
    }

    /// The displayed note name.
    ///
    /// ```
    /// use martensite::widgets::tuner::Tuner;
    ///
    /// assert_eq!(Tuner::new().note("C#4").note_name(), "C#4");
    /// ```
    pub fn note_name(&self) -> &str {
        &self.note
    }

    /// Cents deviation (`-50..=50`).
    ///
    /// ```
    /// use martensite::widgets::tuner::Tuner;
    ///
    /// assert_eq!(Tuner::new().cents(-25.0).cents_value(), -25.0);
    /// ```
    pub fn cents_value(&self) -> f32 {
        self.cents
    }

    /// Whether the reading sits inside the in-tune band.
    ///
    /// ```
    /// use martensite::widgets::tuner::Tuner;
    ///
    /// assert!(Tuner::new().cents(3.0).in_tune());
    /// assert!(!Tuner::new().cents(30.0).in_tune());
    /// ```
    pub fn in_tune(&self) -> bool {
        self.cents.abs() <= self.in_tune
    }

    /// Drains the steady-in-tune flag (fires once per reading).
    ///
    /// ```
    /// use martensite::widgets::tuner::Tuner;
    ///
    /// assert!(!Tuner::new().take_steady());
    /// ```
    pub fn take_steady(&mut self) -> bool {
        std::mem::take(&mut self.steady_fired)
    }
}

impl Widget for Tuner {
    fn measure(&mut self, cx: &mut LayoutContext, constraints: LayoutConstraints) -> Vec2 {
        Vec2::new(
            cx.pt(W_PT).min(constraints.max_size.x.max(0.0)),
            cx.pt(H_PT).min(constraints.max_size.y.max(0.0)),
        )
    }

    fn min_render(&self) -> RenderMinimum {
        RenderMinimum::new(Vec2::new(80.0, 56.0)).with_policy(UnderflowPolicy::Lint)
    }

    fn layout(&mut self, cx: &mut LayoutContext, bounds: Rect) {
        self.bounds = bounds;
        self.scale = cx.scale;
    }

    fn accessibility(&self, node: &mut AccessKitNode) {
        node.set_role(accesskit::Role::Meter);
        node.set_label(format!(
            "{} — {} {:+.0} cents{}",
            self.label,
            self.note,
            self.cents,
            if self.in_tune() { " in tune" } else { "" }
        ));
        node.set_numeric_value(f64::from(self.cents));
    }

    fn tick(&mut self, dt: Duration) -> bool {
        if self.in_tune() && self.note != "—" && !self.steady_fired {
            self.steady += dt.as_secs_f32();
            if self.steady >= self.steady_secs {
                self.steady_fired = true;
            }
            true
        } else {
            false
        }
    }

    fn paint(&self, cx: &mut PaintContext) {
        let s = cx.scale;
        let b = cx.bounds;
        let pt = |p: Vec2| kurbo::Point::new(f64::from(p.x), f64::from(p.y));
        let painter = crate::text_paint::resolve_painter(&self.text_painter, cx.text_painter);
        let good = cx.color(TokenKey::SuccessColor, GOOD);
        let bad = cx.color(TokenKey::ErrorColor, BAD);
        let track = cx.color(TokenKey::DividerColor, TRACK);
        let dim = cx.color(TokenKey::TextMutedColor, DIM);

        // Semicircular gauge: pivot at bottom-center, arc across the top.
        let px = (b.min_x() + b.max_x()) / 2.0;
        let py = b.max_y() - 8.0 * s;
        let r = ((b.width() / 2.0) - 8.0 * s)
            .min(b.height() - 24.0 * s)
            .max(1.0);
        let pivot = Vec2::new(px, py);

        // Gauge arc + cent ticks: -50..=50 mapped to 180°..0°.
        let mut arc = kurbo::BezPath::new();
        for i in 0..=40 {
            let t = i as f32 / 40.0;
            let a = std::f32::consts::PI - t * std::f32::consts::PI;
            let p = Vec2::new(pivot.x + r * a.cos(), pivot.y - r * a.sin());
            if i == 0 {
                arc.move_to(pt(p));
            } else {
                arc.line_to(pt(p));
            }
        }
        cx.list.push_stroke_path(arc, 2.0 * s, track);

        let tick = TICK_PT * s;
        for c in [-50_i32, -25, 0, 25, 50] {
            let a = std::f32::consts::PI - ((c + 50) as f32 / 100.0) * std::f32::consts::PI;
            let dir = Vec2::new(a.cos(), -a.sin());
            let from = pivot + dir * (r - tick);
            let to = pivot + dir * (r + tick * 0.5);
            let mut seg = kurbo::BezPath::new();
            seg.move_to(pt(from));
            seg.line_to(pt(to));
            cx.list
                .push_stroke_path(seg, if c == 0 { 2.0 * s } else { s }, dim);
        }

        // In-tune band arc.
        let band_half = self.in_tune / 100.0 * std::f32::consts::PI;
        let mid = std::f32::consts::FRAC_PI_2;
        let mut band = kurbo::BezPath::new();
        let steps = 8;
        for i in 0..=steps {
            let a = mid + band_half - 2.0 * band_half * (i as f32 / steps as f32);
            let p = Vec2::new(pivot.x + r * a.cos(), pivot.y - r * a.sin());
            if i == 0 {
                band.move_to(pt(p));
            } else {
                band.line_to(pt(p));
            }
        }
        cx.list.push_stroke_path(band, 3.0 * s, good);

        // Needle.
        let needle_a = std::f32::consts::PI - ((self.cents + 50.0) / 100.0) * std::f32::consts::PI;
        let tip = Vec2::new(
            pivot.x + r * 0.9 * needle_a.cos(),
            pivot.y - r * 0.9 * needle_a.sin(),
        );
        let needle_color = if self.in_tune() { good } else { bad };
        let mut needle = kurbo::BezPath::new();
        needle.move_to(pt(pivot));
        needle.line_to(pt(tip));
        cx.list
            .push_stroke_path(needle, 2.5 * s, cx.color(TokenKey::TextColor, NEEDLE));
        let hub = 4.0 * s;
        cx.list.push_fill_shape(
            kurbo::Rect::new(
                f64::from(pivot.x - hub / 2.0),
                f64::from(pivot.y - hub / 2.0),
                f64::from(pivot.x + hub / 2.0),
                f64::from(pivot.y + hub / 2.0),
            ),
            &martensite_core::shape::Shape::ELLIPSE,
            needle_color,
        );

        // Note name.
        let note_size = NOTE_PT * s;
        let nw = painter
            .and_then(|p| p.measure_text(&self.note, note_size))
            .unwrap_or(self.note.chars().count() as f32 * note_size * 0.55);
        crate::text_paint::paint_label_clipped(
            painter,
            cx.list,
            kurbo::Rect::new(
                f64::from(b.min_x()),
                f64::from(b.min_y()),
                f64::from(b.max_x()),
                f64::from(b.max_y()),
            ),
            kurbo::Point::new(
                f64::from(px - nw / 2.0),
                f64::from(py - r * 0.55 - note_size * 0.6),
            ),
            &self.note,
            note_size,
            needle_color,
        );
        // Cents readout.
        let cents_size = 11.0 * s;
        let cents_face = format!("{:+.0}¢", self.cents);
        let cw = painter
            .and_then(|p| p.measure_text(&cents_face, cents_size))
            .unwrap_or(cents_face.chars().count() as f32 * cents_size * 0.55);
        crate::text_paint::paint_label_clipped(
            painter,
            cx.list,
            kurbo::Rect::new(
                f64::from(b.min_x()),
                f64::from(b.min_y()),
                f64::from(b.max_x()),
                f64::from(b.max_y()),
            ),
            kurbo::Point::new(
                f64::from(px - cw / 2.0),
                f64::from(py - r * 0.3 - cents_size * 0.6),
            ),
            &cents_face,
            cents_size,
            dim,
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use martensite_core::HotNode;

    fn laid_out(t: &mut Tuner) {
        let mut hot = HotNode::default();
        let mut cx = LayoutContext {
            hot: &mut hot,
            scale: 1.0,
        };
        t.layout(&mut cx, Rect::new(0.0, 0.0, 160.0, 110.0));
    }

    #[test]
    fn cents_clamps() {
        assert_eq!(Tuner::new().cents(-99.0).cents_value(), -50.0);
        assert_eq!(Tuner::new().cents(99.0).cents_value(), 50.0);
    }

    #[test]
    fn band_detects_in_tune() {
        let t = Tuner::new().band(10.0).cents(8.0);
        assert!(t.in_tune());
        let t2 = Tuner::new().band(10.0).cents(-15.0);
        assert!(!t2.in_tune());
    }

    #[test]
    fn steady_fires_after_hold() {
        let mut t = Tuner::new().note("A4").cents(0.0);
        t.steady_secs = 0.5;
        laid_out(&mut t);
        assert!(t.tick(Duration::from_millis(100)));
        assert!(!t.take_steady());
        t.tick(Duration::from_millis(400));
        t.tick(Duration::from_millis(200));
        assert!(t.take_steady());
        assert!(!t.take_steady());
    }

    #[test]
    fn sharp_reading_never_fires() {
        let mut t = Tuner::new().note("E2").cents(30.0);
        laid_out(&mut t);
        t.tick(Duration::from_secs(5));
        assert!(!t.take_steady());
    }

    #[test]
    fn paint_without_painter() {
        let mut t = Tuner::new().note("A4").cents(12.0);
        laid_out(&mut t);
        let mut list = martensite_core::PaintList::default();
        let theme = martensite_theme::Theme::new("test");
        t.paint(&mut PaintContext {
            list: &mut list,
            bounds: t.bounds,
            theme: &theme,
            scale: 1.0,
            text_painter: None,
        });
        assert!(!list.is_empty());
    }
}
