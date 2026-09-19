//! `Spectrum` — a frequency-band bar display (equalizer /
//! analyzer idiom).
//!
//! Bands are `0..=1` amplitudes pushed by the app (analyzer
//! output); each paints as a column with a gradient intensity —
//! green through the middle, amber near the top, red at the peak.
//! An optional peak-hold marker (`peak_hold`) lingers at each
//! band's recent maximum. Clicking a band parks its index in
//! [`Spectrum::take_pressed`].
//!
//! # Examples
//!
//! ```
//! use martensite::widgets::spectrum::Spectrum;
//!
//! let s = Spectrum::new().bands([0.3, 0.7, 0.5, 0.9]);
//! assert_eq!(s.band_count(), 4);
//! ```

use accesskit::Node as AccessKitNode;
use glam::Vec2;
use martensite_core::{
    EventContext, EventResponse, LayoutConstraints, LayoutContext, PaintContext, PointerButton,
    Rect, RenderMinimum, UnderflowPolicy, Widget, WidgetEvent,
};
use martensite_theme::TokenKey;

const WIDTH_PT: f32 = 200.0;
const HEIGHT_PT: f32 = 80.0;

const TRACK: [u8; 4] = [48, 48, 52, 255];
const LOW: [u8; 4] = [110, 180, 130, 255];
const MID: [u8; 4] = [230, 170, 80, 255];
const HIGH: [u8; 4] = [210, 110, 90, 255];
const PEAK: [u8; 4] = [230, 230, 235, 255];

/// A frequency-band bar display — see the module docs.
///
/// ```
/// use martensite::widgets::spectrum::Spectrum;
///
/// assert_eq!(Spectrum::new().band_count(), 0);
/// ```
pub struct Spectrum {
    /// When `false` clicks are ignored.
    pub enabled: bool,
    /// Accessibility label.
    pub label: String,
    /// Paint a peak-hold tick at each band's recent max.
    pub peak_hold: bool,
    bands: Vec<f32>,
    peaks: Vec<f32>,
    pending: Option<usize>,
    bounds: Rect,
}

impl Default for Spectrum {
    fn default() -> Self {
        Self::new()
    }
}

impl Spectrum {
    /// Creates an empty spectrum.
    ///
    /// ```
    /// use martensite::widgets::spectrum::Spectrum;
    ///
    /// assert_eq!(Spectrum::new().band_count(), 0);
    /// ```
    pub fn new() -> Self {
        Self {
            enabled: true,
            label: "Spectrum".to_string(),
            peak_hold: true,
            bands: Vec::new(),
            peaks: Vec::new(),
            pending: None,
            bounds: Rect::new(0.0, 0.0, 0.0, 0.0),
        }
    }

    /// Band amplitudes (`0..=1`, clamped).
    ///
    /// ```
    /// use martensite::widgets::spectrum::Spectrum;
    ///
    /// let s = Spectrum::new().bands([0.5, 2.0]);
    /// assert_eq!(s.band_list()[1], 1.0);
    /// ```
    pub fn bands(mut self, bands: impl IntoIterator<Item = f32>) -> Self {
        self.bands = bands.into_iter().map(|b| b.clamp(0.0, 1.0)).collect();
        self.peaks = self.bands.clone();
        self
    }

    /// Accessibility label.
    ///
    /// ```
    /// use martensite::widgets::spectrum::Spectrum;
    ///
    /// let s = Spectrum::new().label("Master");
    /// assert_eq!(s.label, "Master");
    /// ```
    pub fn label(mut self, label: impl Into<String>) -> Self {
        self.label = label.into();
        self
    }

    /// Toggles the peak-hold marker.
    ///
    /// ```
    /// use martensite::widgets::spectrum::Spectrum;
    ///
    /// assert!(!Spectrum::new().peak_hold(false).peak_hold);
    /// ```
    pub fn peak_hold(mut self, on: bool) -> Self {
        self.peak_hold = on;
        self
    }

    /// Enables or disables clicks.
    ///
    /// ```
    /// use martensite::widgets::spectrum::Spectrum;
    ///
    /// assert!(!Spectrum::new().enabled(false).enabled);
    /// ```
    pub fn enabled(mut self, enabled: bool) -> Self {
        self.enabled = enabled;
        self
    }

    /// Band count.
    ///
    /// ```
    /// use martensite::widgets::spectrum::Spectrum;
    ///
    /// assert_eq!(Spectrum::new().bands([0.5; 8]).band_count(), 8);
    /// ```
    pub fn band_count(&self) -> usize {
        self.bands.len()
    }

    /// Band list.
    ///
    /// ```
    /// use martensite::widgets::spectrum::Spectrum;
    ///
    /// assert!(Spectrum::new().band_list().is_empty());
    /// ```
    pub fn band_list(&self) -> &[f32] {
        &self.bands
    }

    /// Pushes new analyzer values (peak-hold decays to them).
    ///
    /// ```
    /// use martensite::widgets::spectrum::Spectrum;
    ///
    /// let mut s = Spectrum::new().bands([0.9, 0.9]);
    /// s.set_bands([0.2, 0.9]);
    /// assert_eq!(s.band_list()[0], 0.2);
    /// ```
    pub fn set_bands(&mut self, bands: impl IntoIterator<Item = f32>) {
        self.bands = bands.into_iter().map(|b| b.clamp(0.0, 1.0)).collect();
        self.peaks.resize(self.bands.len(), 0.0);
        for (p, &b) in self.peaks.iter_mut().zip(&self.bands) {
            *p = p.max(b) - 0.02; // decay toward the new level
            *p = p.max(b);
        }
    }

    /// Drains the band index pressed since the last drain.
    ///
    /// ```
    /// use martensite::widgets::spectrum::Spectrum;
    ///
    /// let mut s = Spectrum::new();
    /// assert!(s.take_pressed().is_none());
    /// ```
    pub fn take_pressed(&mut self) -> Option<usize> {
        self.pending.take()
    }

    /// Band index at a device-space x.
    fn band_at(&self, x: f32) -> Option<usize> {
        if self.bands.is_empty() {
            return None;
        }
        let w = self.bounds.width() / self.bands.len() as f32;
        if w <= 0.0 {
            return None;
        }
        Some(((x - self.bounds.min_x()) / w) as usize).map(|i| i.min(self.bands.len() - 1))
    }

    /// Segment color by relative height — green/amber/red thirds.
    fn band_color(frac: f32) -> [u8; 4] {
        if frac > 0.75 {
            HIGH
        } else if frac > 0.5 {
            MID
        } else {
            LOW
        }
    }
}

impl Widget for Spectrum {
    fn measure(&mut self, cx: &mut LayoutContext, constraints: LayoutConstraints) -> Vec2 {
        Vec2::new(
            cx.pt(WIDTH_PT).min(constraints.max_size.x.max(0.0)),
            cx.pt(HEIGHT_PT).min(constraints.max_size.y.max(0.0)),
        )
    }

    fn min_render(&self) -> RenderMinimum {
        RenderMinimum::new(Vec2::new(48.0, 24.0)).with_policy(UnderflowPolicy::Lint)
    }

    fn layout(&mut self, _cx: &mut LayoutContext, bounds: Rect) {
        self.bounds = bounds;
    }

    fn accessibility(&self, node: &mut AccessKitNode) {
        node.set_role(accesskit::Role::Image);
        node.set_label(self.label.clone());
        node.set_description(format!("{} bands", self.bands.len()));
        if !self.enabled {
            node.set_disabled();
        }
    }

    fn event(&mut self, cx: &mut EventContext) -> EventResponse {
        if !self.enabled {
            return EventResponse::Ignored;
        }
        match cx.event {
            WidgetEvent::PointerPressed {
                button: PointerButton::Primary,
                position,
                ..
            } => {
                if self.bounds.contains(*position) {
                    if let Some(i) = self.band_at(position.x) {
                        self.pending = Some(i);
                        return EventResponse::Handled;
                    }
                }
                EventResponse::Ignored
            }
            _ => EventResponse::Ignored,
        }
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
        cx.list
            .push_fill_rect(f(self.bounds), cx.color(TokenKey::SurfaceColor, TRACK));
        if self.bands.is_empty() {
            return;
        }
        let n = self.bands.len();
        let slot_w = self.bounds.width() / n as f32;
        let bar_w = (slot_w * 0.72).min(cx.pt(12.0));
        let seg_h = cx.pt(4.0);
        let gap = cx.pt(1.0);
        for (i, &amp) in self.bands.iter().enumerate() {
            let x = self.bounds.min_x() + i as f32 * slot_w + (slot_w - bar_w) / 2.0;
            // Segmented column — each lit segment picks its color by
            // its own height fraction (the LED-meter look).
            let lit = (amp * self.bounds.height() / (seg_h + gap)) as usize;
            for seg in 0..lit {
                let y = self.bounds.max_y() - (seg as f32 + 1.0) * (seg_h + gap);
                let frac = (seg as f32 + 1.0) * (seg_h + gap) / self.bounds.height();
                cx.list.push_fill_shape(
                    f(Rect::new(x, y, bar_w, seg_h)),
                    &martensite_core::shape::Shape::rounded(cx.pt(1.0)),
                    Self::band_color(frac),
                );
            }
            // Peak-hold tick.
            if self.peak_hold {
                if let Some(&p) = self.peaks.get(i) {
                    if p > 0.0 {
                        let py = self.bounds.max_y() - p * self.bounds.height();
                        cx.list.push_fill_rect(
                            f(Rect::new(x, py - cx.pt(0.75), bar_w, cx.pt(1.5))),
                            cx.color(TokenKey::TextColor, PEAK),
                        );
                    }
                }
            }
        }
    }
}

impl std::fmt::Debug for Spectrum {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Spectrum")
            .field("bands", &self.bands.len())
            .finish()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use martensite_core::HotNode;

    fn laid_out(s: &mut Spectrum, w: f32, h: f32) {
        let mut hot = HotNode::default();
        let mut cx = LayoutContext {
            hot: &mut hot,
            scale: 1.0,
        };
        s.measure(
            &mut cx,
            LayoutConstraints {
                min_size: Vec2::ZERO,
                max_size: Vec2::new(w, h),
            },
        );
        s.layout(&mut cx, Rect::new(0.0, 0.0, w, h));
    }

    #[test]
    fn bands_clamp() {
        let s = Spectrum::new().bands([-1.0, 0.5, 2.0]);
        assert_eq!(s.band_list(), &[0.0, 0.5, 1.0]);
    }

    #[test]
    fn peak_hold_tracks_max() {
        let mut s = Spectrum::new().bands([0.8, 0.8]);
        s.set_bands([0.2, 0.8]);
        assert!(s.peaks[0] >= 0.78);
        assert_eq!(s.peaks[1], 0.8);
    }

    #[test]
    fn press_parks_band() {
        let mut s = Spectrum::new().bands([0.5; 4]);
        laid_out(&mut s, 200.0, 80.0);
        s.event(&mut EventContext {
            event: &WidgetEvent::PointerPressed {
                button: PointerButton::Primary,
                position: Vec2::new(175.0, 40.0), // 4th slot of 4
                count: 1,
            },
            bounds: Rect::new(0.0, 0.0, 200.0, 80.0),
            scale: 1.0,
        });
        assert_eq!(s.take_pressed(), Some(3));
        assert!(s.take_pressed().is_none());
    }

    #[test]
    fn press_outside_ignored() {
        let mut s = Spectrum::new().bands([0.5; 4]);
        laid_out(&mut s, 200.0, 80.0);
        s.event(&mut EventContext {
            event: &WidgetEvent::PointerPressed {
                button: PointerButton::Primary,
                position: Vec2::new(300.0, 40.0),
                count: 1,
            },
            bounds: Rect::new(0.0, 0.0, 200.0, 80.0),
            scale: 1.0,
        });
        assert!(s.take_pressed().is_none());
    }

    #[test]
    fn disabled_inert() {
        let mut s = Spectrum::new().bands([0.5; 4]).enabled(false);
        laid_out(&mut s, 200.0, 80.0);
        s.event(&mut EventContext {
            event: &WidgetEvent::PointerPressed {
                button: PointerButton::Primary,
                position: Vec2::new(25.0, 40.0),
                count: 1,
            },
            bounds: Rect::new(0.0, 0.0, 200.0, 80.0),
            scale: 1.0,
        });
        assert!(s.take_pressed().is_none());
    }
}
