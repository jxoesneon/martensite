//! Interactive audio waveform geometry.

use martensite_render::Point;
use std::ops::Range;

/// Maximum number of min/max envelope vertices a single frame may produce.
const MAX_POINT_BUDGET: usize = 1_000;

/// An audio waveform with a normalized viewport and scrub head.
///
/// Rendering downsamples the viewport directly into the requested point budget,
/// bounding per-frame output memory and GPU upload size.
///
/// # Examples
///
/// ```
/// use martensite_blessed::AudioWaveform;
/// let mut waveform = AudioWaveform::from_samples(vec![0.0, 0.5, -0.5, 1.0]);
/// waveform.set_viewport(0.25..0.75);
/// waveform.scrub_to(0.5);
/// assert_eq!(waveform.scrub_head(), 0.5);
/// ```
#[derive(Clone, Debug, Default)]
pub struct AudioWaveform {
    peaks: Vec<f32>,
    viewport: Range<f32>,
    scrub_head: f32,
}

impl AudioWaveform {
    /// Creates a waveform from amplitude peaks, clamped to `[-1, 1]`.
    pub fn from_peaks(peaks: Vec<f32>) -> Self {
        Self {
            peaks: sanitize(peaks),
            viewport: 0.0..1.0,
            scrub_head: 0.0,
        }
    }

    /// Creates a waveform from raw samples, clamped to `[-1, 1]`.
    ///
    /// This is the preferred constructor; it clamps samples but does not
    /// compute peaks.
    pub fn from_samples(samples: Vec<f32>) -> Self {
        Self::from_peaks(samples)
    }

    /// Alias for [`AudioWaveform::from_samples`].
    ///
    /// `from_pcm` only clamps samples into `[-1, 1]`; it does not perform any
    /// peak decimation.
    pub fn from_pcm(samples: Vec<f32>) -> Self {
        Self::from_samples(samples)
    }
    /// Returns waveform samples.
    pub fn peaks(&self) -> &[f32] {
        &self.peaks
    }
    /// Returns the normalized viewport.
    pub fn viewport(&self) -> Range<f32> {
        self.viewport.clone()
    }
    /// Sets the normalized viewport, preserving a nondecreasing range.
    pub fn set_viewport(&mut self, viewport: Range<f32>) {
        let start = normalized(viewport.start);
        let end = normalized(viewport.end);
        self.viewport = start.min(end)..start.max(end);
    }
    /// Sets the normalized scrub-head position.
    pub fn scrub_to(&mut self, position: f32) {
        self.scrub_head = normalized(position);
    }
    /// Returns the normalized scrub-head position.
    pub const fn scrub_head(&self) -> f32 {
        self.scrub_head
    }
    /// Returns the scrub-head x coordinate for a viewport width.
    pub fn scrub_head_x(&self, width: f64) -> Option<f64> {
        if self.scrub_head < self.viewport.start || self.scrub_head > self.viewport.end {
            return None;
        }
        let span = self.viewport.end - self.viewport.start;
        if span == 0.0 {
            return Some(0.0);
        }
        Some(f64::from((self.scrub_head - self.viewport.start) / span) * width.max(0.0))
    }

    /// Builds at most `point_budget` min/max envelope vertices for GPU rendering.
    ///
    /// The requested `point_budget` is silently capped at `MAX_POINT_BUDGET`
    /// (1,000) to keep per-frame memory and GPU upload size bounded.
    pub fn render_points(&self, width: f64, height: f64, point_budget: usize) -> Vec<Point> {
        let budget = point_budget.min(MAX_POINT_BUDGET);
        if budget == 0 || self.peaks.is_empty() {
            return Vec::new();
        }
        let start = (self.viewport.start * self.peaks.len() as f32).floor() as usize;
        let mut end = (self.viewport.end * self.peaks.len() as f32).ceil() as usize;
        end = end.clamp(start.saturating_add(1), self.peaks.len());
        let samples = &self.peaks[start.min(self.peaks.len() - 1)..end];
        let buckets = budget.div_ceil(2).min(samples.len());
        let mut points = Vec::with_capacity(buckets * 2);
        for bucket in 0..buckets {
            let from = bucket * samples.len() / buckets;
            let to = ((bucket + 1) * samples.len() / buckets).max(from + 1);
            let (low, high) = samples[from..to]
                .iter()
                .fold((1.0_f32, -1.0_f32), |(low, high), sample| {
                    (low.min(*sample), high.max(*sample))
                });
            let x = if buckets == 1 {
                0.0
            } else {
                bucket as f64 / (buckets - 1) as f64 * width.max(0.0)
            };
            points.push(Point::new(x, amplitude_y(high, height)));
            if points.len() < budget {
                points.push(Point::new(x, amplitude_y(low, height)));
            }
        }
        points
    }
}

fn normalized(value: f32) -> f32 {
    if value.is_finite() {
        value.clamp(0.0, 1.0)
    } else {
        0.0
    }
}
fn sanitize(mut values: Vec<f32>) -> Vec<f32> {
    for value in &mut values {
        *value = if value.is_finite() {
            value.clamp(-1.0, 1.0)
        } else {
            0.0
        };
    }
    values
}
fn amplitude_y(amplitude: f32, height: f64) -> f64 {
    (1.0 - f64::from(amplitude)) * 0.5 * height.max(0.0)
}

#[cfg(test)]
mod tests {
    use super::AudioWaveform;
    #[test]
    fn scrub_head_tracks_viewport() {
        let mut waveform = AudioWaveform::from_peaks(vec![0.0; 2_000]);
        waveform.set_viewport(0.25..0.75);
        waveform.scrub_to(0.5);
        assert_eq!(waveform.scrub_head_x(800.0), Some(400.0));
        waveform.scrub_to(0.9);
        assert_eq!(waveform.scrub_head_x(800.0), None);
    }
    #[test]
    fn render_shape_is_bounded() {
        let waveform = AudioWaveform::from_samples(vec![0.25; 1_000_000]);
        let points = waveform.render_points(1_000.0, 200.0, 1_000);
        assert_eq!(points.len(), 1_000);
        assert_eq!(points.capacity(), 1_000);
    }
}
