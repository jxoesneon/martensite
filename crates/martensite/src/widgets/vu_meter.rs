//! `VuMeter` — a multi-channel VU / PPM level meter (channel
//! strips over a green→amber→red zone gradient with slowly
//! decaying peak-hold markers — the audio-display companion to
//! [`crate::widgets::spectrum::Spectrum`] and
//! [`crate::widgets::waveform::Waveform`]).
//!
//! Levels are normalized `0.0..=1.0` and clamped. The meter is
//! driven — feed it per-channel samples via [`VuMeter::push`]
//! or replace all channels with [`VuMeter::levels`]. Each
//! channel's peak marker holds for [`VuMeter::peak_hold`] then
//! decays toward the live level on
//! [`Widget::tick`](martensite_core::widget::Widget::tick).
//!
//! # Examples
//!
//! ```
//! use martensite::widgets::vu_meter::VuMeter;
//!
//! let mut v = VuMeter::new().channels(2);
//! v.push([0.6, 0.3]);
//! assert_eq!(v.level_list(), &[0.6, 0.3]);
//! assert_eq!(v.peak_list()[0], 0.6);
//! ```

use accesskit::Node as AccessKitNode;
use glam::Vec2;
use martensite_core::{
    LayoutConstraints, LayoutContext, PaintContext, Rect, RenderMinimum, UnderflowPolicy, Widget,
};
use martensite_theme::TokenKey;
use std::time::Duration;

const HEIGHT_PT: f32 = 90.0;
const CHANNEL_PT: f32 = 10.0;
const GAP_PT: f32 = 2.0;
const PAD_PT: f32 = 4.0;

/// Fraction of full scale where the amber zone begins.
const AMBER_AT: f32 = 0.7;
/// Fraction of full scale where the red zone begins.
const RED_AT: f32 = 0.9;
/// Peak marker decay per second once the hold has elapsed.
const PEAK_DECAY: f32 = 0.8;

const FACE: [u8; 4] = [36, 36, 42, 255];
const EDGE: [u8; 4] = [70, 70, 76, 255];
const GREEN: [u8; 4] = [110, 180, 130, 255];
const AMBER: [u8; 4] = [230, 170, 80, 255];
const RED: [u8; 4] = [210, 110, 90, 255];
const PEAK: [u8; 4] = [230, 230, 235, 255];

/// A channel-strip level meter — see the module docs.
///
/// ```
/// use martensite::widgets::vu_meter::VuMeter;
///
/// assert_eq!(VuMeter::new().channel_count(), 2);
/// ```
#[derive(Debug)]
pub struct VuMeter {
    /// Accessibility label.
    pub label: String,
    /// Instantaneous level per channel, `0.0..=1.0`.
    levels: Vec<f32>,
    /// Peak-hold level per channel.
    peaks: Vec<f32>,
    /// Seconds each peak has held since its last rise.
    hold: Vec<f32>,
    /// Seconds a peak holds before decaying.
    peak_hold_secs: f32,
    /// Whether to show the peak markers.
    show_peak: bool,
    /// Vertical (default) or horizontal strips.
    vertical: bool,
    bounds: Rect,
    scale: f32,
}

impl Default for VuMeter {
    fn default() -> Self {
        Self::new()
    }
}

impl VuMeter {
    /// Creates a two-channel meter.
    ///
    /// ```
    /// use martensite::widgets::vu_meter::VuMeter;
    ///
    /// assert_eq!(VuMeter::new().channel_count(), 2);
    /// ```
    pub fn new() -> Self {
        Self {
            label: "Level meter".to_string(),
            levels: vec![0.0; 2],
            peaks: vec![0.0; 2],
            hold: vec![0.0; 2],
            peak_hold_secs: 1.0,
            show_peak: true,
            vertical: true,
            bounds: Rect::new(0.0, 0.0, 0.0, 0.0),
            scale: 1.0,
        }
    }

    /// Sets the channel count (levels reset to zero).
    ///
    /// ```
    /// use martensite::widgets::vu_meter::VuMeter;
    ///
    /// assert_eq!(VuMeter::new().channels(4).channel_count(), 4);
    /// ```
    pub fn channels(mut self, n: usize) -> Self {
        let n = n.max(1);
        self.levels = vec![0.0; n];
        self.peaks = vec![0.0; n];
        self.hold = vec![0.0; n];
        self
    }

    /// Lays the strips out horizontally instead of vertically.
    ///
    /// ```
    /// use martensite::widgets::vu_meter::VuMeter;
    ///
    /// assert!(!VuMeter::new().horizontal().is_vertical());
    /// ```
    pub fn horizontal(mut self) -> Self {
        self.vertical = false;
        self
    }

    /// Seconds a peak marker holds before decaying.
    ///
    /// ```
    /// use martensite::widgets::vu_meter::VuMeter;
    ///
    /// assert_eq!(VuMeter::new().peak_hold(2.5).hold_secs(), 2.5);
    /// ```
    pub fn peak_hold(mut self, secs: f32) -> Self {
        self.peak_hold_secs = secs.max(0.0);
        self
    }

    /// Hides the peak-hold markers.
    ///
    /// ```
    /// use martensite::widgets::vu_meter::VuMeter;
    ///
    /// assert!(!VuMeter::new().hide_peak().has_peak());
    /// ```
    pub fn hide_peak(mut self) -> Self {
        self.show_peak = false;
        self
    }

    /// Accessibility label.
    ///
    /// ```
    /// use martensite::widgets::vu_meter::VuMeter;
    ///
    /// assert_eq!(VuMeter::new().label("Mix").label, "Mix");
    /// ```
    pub fn label(mut self, label: impl Into<String>) -> Self {
        self.label = label.into();
        self
    }

    /// Pushes one sample across all channels (extra values ignored).
    ///
    /// ```
    /// use martensite::widgets::vu_meter::VuMeter;
    ///
    /// let mut v = VuMeter::new();
    /// v.push([0.9, 0.2]);
    /// assert_eq!(v.level_list()[0], 0.9);
    /// ```
    pub fn push(&mut self, sample: impl IntoIterator<Item = f32>) {
        for (i, s) in sample.into_iter().take(self.levels.len()).enumerate() {
            let s = s.clamp(0.0, 1.0);
            self.levels[i] = s;
            if s >= self.peaks[i] {
                self.peaks[i] = s;
                self.hold[i] = 0.0;
            }
        }
    }

    /// Replaces all channel levels at once.
    ///
    /// ```
    /// use martensite::widgets::vu_meter::VuMeter;
    ///
    /// assert_eq!(VuMeter::new().levels([0.5, 0.8]).level_list()[1], 0.8);
    /// ```
    pub fn levels(mut self, values: impl IntoIterator<Item = f32>) -> Self {
        for (i, s) in values.into_iter().take(self.levels.len()).enumerate() {
            let s = s.clamp(0.0, 1.0);
            self.levels[i] = s;
            if s > self.peaks[i] {
                self.peaks[i] = s;
            }
        }
        self
    }

    /// Channel count.
    ///
    /// ```
    /// use martensite::widgets::vu_meter::VuMeter;
    ///
    /// assert_eq!(VuMeter::new().channels(3).channel_count(), 3);
    /// ```
    pub fn channel_count(&self) -> usize {
        self.levels.len()
    }

    /// Instantaneous level per channel.
    ///
    /// ```
    /// use martensite::widgets::vu_meter::VuMeter;
    ///
    /// assert_eq!(VuMeter::new().levels([0.4]).level_list()[0], 0.4);
    /// ```
    pub fn level_list(&self) -> &[f32] {
        &self.levels
    }

    /// Peak-hold level per channel.
    ///
    /// ```
    /// use martensite::widgets::vu_meter::VuMeter;
    ///
    /// let mut v = VuMeter::new();
    /// v.push([1.0, 0.0]);
    /// v.push([0.2, 0.0]);
    /// assert_eq!(v.peak_list()[0], 1.0); // peak retained
    /// ```
    pub fn peak_list(&self) -> &[f32] {
        &self.peaks
    }

    /// Peak-hold duration in seconds.
    ///
    /// ```
    /// use martensite::widgets::vu_meter::VuMeter;
    ///
    /// assert_eq!(VuMeter::new().peak_hold(0.5).hold_secs(), 0.5);
    /// ```
    pub fn hold_secs(&self) -> f32 {
        self.peak_hold_secs
    }

    /// Whether peak markers are shown.
    ///
    /// ```
    /// use martensite::widgets::vu_meter::VuMeter;
    ///
    /// assert!(VuMeter::new().has_peak());
    /// ```
    pub fn has_peak(&self) -> bool {
        self.show_peak
    }

    /// Whether the strips are vertical.
    ///
    /// ```
    /// use martensite::widgets::vu_meter::VuMeter;
    ///
    /// assert!(VuMeter::new().is_vertical());
    /// ```
    pub fn is_vertical(&self) -> bool {
        self.vertical
    }

    /// Mean level across channels, for the accessibility value.
    ///
    /// ```
    /// use martensite::widgets::vu_meter::VuMeter;
    ///
    /// assert_eq!(VuMeter::new().levels([0.4, 0.8]).mean_level(), 0.6);
    /// ```
    pub fn mean_level(&self) -> f32 {
        if self.levels.is_empty() {
            0.0
        } else {
            self.levels.iter().sum::<f32>() / self.levels.len() as f32
        }
    }

    /// Zone color for a band midpoint at fraction `t` of full scale.
    fn zone_color(t: f32) -> [u8; 4] {
        if t >= RED_AT {
            RED
        } else if t >= AMBER_AT {
            AMBER
        } else {
            GREEN
        }
    }
}

impl Widget for VuMeter {
    fn measure(&mut self, cx: &mut LayoutContext, constraints: LayoutConstraints) -> Vec2 {
        let strip = cx.pt(CHANNEL_PT) * self.levels.len() as f32
            + cx.pt(GAP_PT) * self.levels.len().saturating_sub(1) as f32
            + 2.0 * cx.pt(PAD_PT);
        let size = if self.vertical {
            Vec2::new(strip, cx.pt(HEIGHT_PT))
        } else {
            Vec2::new(cx.pt(HEIGHT_PT), strip)
        };
        Vec2::new(
            size.x.min(constraints.max_size.x.max(0.0)),
            size.y.min(constraints.max_size.y.max(0.0)),
        )
    }

    fn min_render(&self) -> RenderMinimum {
        RenderMinimum::new(Vec2::new(24.0, 24.0)).with_policy(UnderflowPolicy::Lint)
    }

    fn layout(&mut self, cx: &mut LayoutContext, bounds: Rect) {
        self.bounds = bounds;
        self.scale = cx.scale;
    }

    fn accessibility(&self, node: &mut AccessKitNode) {
        node.set_role(accesskit::Role::Meter);
        node.set_label(format!(
            "{} — {}% peak",
            self.label,
            (self.peaks.iter().cloned().fold(0.0f32, f32::max) * 100.0).round()
        ));
    }

    fn tick(&mut self, dt: Duration) -> bool {
        let dt = dt.as_secs_f32();
        let mut changed = false;
        for i in 0..self.peaks.len() {
            if self.peaks[i] <= self.levels[i] {
                self.hold[i] = 0.0;
                continue;
            }
            self.hold[i] += dt;
            if self.hold[i] > self.peak_hold_secs {
                let new = (self.peaks[i] - PEAK_DECAY * dt).max(self.levels[i]);
                if new != self.peaks[i] {
                    self.peaks[i] = new;
                    changed = true;
                }
            }
        }
        changed
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
        let edge = cx.color(TokenKey::BorderColor, EDGE);
        cx.list.push_fill_shape(
            f(self.bounds),
            &martensite_core::shape::Shape::rounded(cx.pt(4.0)),
            cx.color(TokenKey::SurfaceColor, FACE),
        );
        let n = self.levels.len();
        if n == 0 {
            return;
        }
        let pad = PAD_PT * self.scale;
        let gap = GAP_PT * self.scale;
        let inner = Rect::new(
            self.bounds.min_x() + pad,
            self.bounds.min_y() + pad,
            self.bounds.width() - 2.0 * pad,
            self.bounds.height() - 2.0 * pad,
        );
        let (bar_long, slot) = if self.vertical {
            (
                inner.height(),
                (inner.width() - gap * (n - 1) as f32) / n as f32,
            )
        } else {
            (
                inner.width(),
                (inner.height() - gap * (n - 1) as f32) / n as f32,
            )
        };
        let zones = [(0.0f32, AMBER_AT), (AMBER_AT, RED_AT), (RED_AT, 1.0)];
        for (i, &level) in self.levels.iter().enumerate() {
            // Fill rises from the far end as stacked zone segments.
            for &(lo, hi) in &zones {
                let covered = level.clamp(lo, hi);
                if covered <= lo {
                    continue;
                }
                let seg_start = lo * bar_long;
                let seg_len = (covered - lo) * bar_long;
                let r = if self.vertical {
                    Rect::new(
                        inner.min_x() + i as f32 * (slot + gap),
                        inner.max_y() - seg_start - seg_len,
                        slot,
                        seg_len,
                    )
                } else {
                    Rect::new(
                        inner.min_x() + seg_start,
                        inner.min_y() + i as f32 * (slot + gap),
                        seg_len,
                        slot,
                    )
                };
                cx.list.push_fill_shape(
                    f(r),
                    &martensite_core::shape::Shape::RECT,
                    Self::zone_color((lo + hi) / 2.0),
                );
            }
            // Peak-hold marker.
            if self.show_peak && self.peaks[i] > 0.0 {
                let p = self.peaks[i] * bar_long;
                let t = 2.0 * self.scale;
                let r = if self.vertical {
                    Rect::new(
                        inner.min_x() + i as f32 * (slot + gap),
                        inner.max_y() - p - t / 2.0,
                        slot,
                        t,
                    )
                } else {
                    Rect::new(
                        inner.min_x() + p - t / 2.0,
                        inner.min_y() + i as f32 * (slot + gap),
                        t,
                        slot,
                    )
                };
                cx.list
                    .push_fill_shape(f(r), &martensite_core::shape::Shape::RECT, PEAK);
            }
        }
        cx.list.push_stroke_shape(
            f(self.bounds),
            &martensite_core::shape::Shape::rounded(cx.pt(4.0)),
            cx.pt(0.75),
            edge,
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use martensite_core::HotNode;

    fn laid_out(v: &mut VuMeter, w: f32, h: f32) {
        let mut hot = HotNode::default();
        let mut cx = LayoutContext {
            hot: &mut hot,
            scale: 1.0,
        };
        v.measure(
            &mut cx,
            LayoutConstraints {
                min_size: Vec2::ZERO,
                max_size: Vec2::new(w, h),
            },
        );
        v.layout(&mut cx, Rect::new(0.0, 0.0, w, h));
    }

    #[test]
    fn channel_setup() {
        let v = VuMeter::new().channels(4);
        assert_eq!(v.channel_count(), 4);
        assert_eq!(v.level_list().len(), 4);
        assert!(VuMeter::new().is_vertical());
        assert!(!VuMeter::new().horizontal().is_vertical());
    }

    #[test]
    fn push_clamps_and_sets_peak() {
        let mut v = VuMeter::new();
        v.push([1.4, -0.5]);
        assert_eq!(v.level_list(), &[1.0, 0.0]);
        assert_eq!(v.peak_list(), &[1.0, 0.0]);
    }

    #[test]
    fn peak_holds_then_decays() {
        let mut v = VuMeter::new().peak_hold(0.5);
        v.push([1.0, 0.0]);
        v.push([0.2, 0.0]);
        // Hold period not elapsed — no decay.
        v.tick(Duration::from_millis(400));
        assert_eq!(v.peak_list()[0], 1.0);
        // Past the hold — peak decays toward the level.
        v.tick(Duration::from_millis(200));
        assert!(v.peak_list()[0] < 1.0);
        assert!(v.peak_list()[0] >= 0.2);
        // Decays all the way down to the live level.
        for _ in 0..20 {
            v.tick(Duration::from_millis(200));
        }
        assert_eq!(v.peak_list()[0], 0.2);
    }

    #[test]
    fn mean_level_for_a11y() {
        let v = VuMeter::new().levels([0.2, 1.0]);
        assert!((v.mean_level() - 0.6).abs() < 1e-6);
    }

    #[test]
    fn smoke() {
        let mut v = VuMeter::new().channels(3).peak_hold(0.5);
        v.push([0.9, 0.5, 0.1]);
        laid_out(&mut v, 60.0, 120.0);
        assert_eq!(v.channel_count(), 3);
        assert_eq!(v.hold_secs(), 0.5);
        assert!(v.has_peak());
    }
}
