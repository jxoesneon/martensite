//! `ProgressBar` and `Spinner` widgets: determinate and indeterminate
//! progress indicators.
//!
//! # Examples
//!
//! ```
//! use martensite::widgets::progress::{ProgressBar, Spinner};
//!
//! let bar = ProgressBar::new().value(0.4);
//! assert_eq!(bar.fraction(), Some(0.4));
//! let spinner = Spinner::new();
//! assert_eq!(spinner.size, 20.0);
//! ```

use accesskit::Node as AccessKitNode;
use glam::Vec2;
use martensite_core::shape::Shape;
use martensite_core::widget::{LayoutConstraints, LayoutContext, PaintContext, Widget};
use martensite_core::{Rect, TokenKey};

const ACCENT: [u8; 4] = [40, 110, 220, 255];
const TRACK: [u8; 4] = [90, 94, 104, 110];
/// Determinate bar height in logical points.
const BAR_H: f32 = 6.0;

/// A horizontal progress bar. `value` is a 0..=1 fraction; `None`
/// renders an indeterminate sliding segment whose `phase` advances via
/// [`ProgressBar::tick`] (call once per frame, dt in seconds).
///
/// # Examples
///
/// ```
/// use martensite::widgets::ProgressBar;
///
/// let mut bar = ProgressBar::new().value(0.25);
/// assert_eq!(bar.fraction(), Some(0.25));
/// bar.set_indeterminate();
/// assert_eq!(bar.fraction(), None);
/// ```
#[derive(Clone)]
pub struct ProgressBar {
    /// Determinate fraction, `None` = indeterminate.
    value: Option<f32>,
    /// Optional signal the bar polls in `tick` — reactive binding for
    /// values owned elsewhere (telemetry, load meters).
    signal: Option<std::sync::Arc<dyn Fn() -> f32 + Send + Sync>>,
    /// Indeterminate animation phase, 0..1 (advanced by `tick`).
    pub phase: f32,
    /// Cached bounds from the last layout pass.
    cached_bounds: Rect,
}

impl std::fmt::Debug for ProgressBar {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ProgressBar")
            .field("value", &self.value)
            .field("signal", &self.signal.is_some())
            .field("phase", &self.phase)
            .finish()
    }
}

impl ProgressBar {
    /// A new indeterminate progress bar.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite::widgets::ProgressBar;
    ///
    /// let bar = ProgressBar::new();
    /// assert_eq!(bar.fraction(), None);
    /// ```
    pub fn new() -> Self {
        Self {
            value: None,
            signal: None,
            phase: 0.0,
            cached_bounds: Rect::default(),
        }
    }

    /// Binds the bar to a `Signal` — `tick` polls it and marks the
    /// widget dirty when the fraction changes. Accepts any numeric
    /// signal convertible to `f64` (`f32`, `f64`, …).
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite::prelude::Signal;
    /// use martensite::widgets::ProgressBar;
    ///
    /// let sig = Signal::new(0.5_f64);
    /// let bar = ProgressBar::new().bind(sig);
    /// assert_eq!(bar.fraction(), Some(0.5));
    /// ```
    #[must_use]
    pub fn bind<T>(mut self, signal: martensite_reactive::Signal<T>) -> Self
    where
        T: Clone + Send + Sync + 'static,
        f64: From<T>,
    {
        let sig = signal.clone();
        let initial: f64 = sig.get().into();
        self.value = Some(initial.clamp(0.0, 1.0) as f32);
        self.signal = Some(std::sync::Arc::new(move || {
            let v: f64 = sig.get().into();
            v as f32
        }));
        self
    }

    /// Sets a determinate fraction (clamped to 0..=1).
    #[must_use]
    pub fn value(mut self, value: f32) -> Self {
        self.value = Some(value.clamp(0.0, 1.0));
        self
    }

    /// Switches to the indeterminate sliding-segment look.
    pub fn set_indeterminate(&mut self) {
        self.value = None;
    }

    /// The current fraction, `None` when indeterminate.
    pub fn fraction(&self) -> Option<f32> {
        self.value
    }

    /// Advances the indeterminate animation. `dt` is seconds elapsed;
    /// one full sweep takes ~1.2s. No-op for determinate bars.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite::widgets::ProgressBar;
    ///
    /// let mut bar = ProgressBar::new();
    /// bar.tick(0.6);
    /// assert!(bar.phase > 0.0);
    /// ```
    pub fn tick(&mut self, dt: f32) {
        if self.value.is_none() {
            self.phase = (self.phase + dt / 1.2) % 1.0;
        }
    }
}

impl Default for ProgressBar {
    fn default() -> Self {
        Self::new()
    }
}

impl Widget for ProgressBar {
    fn measure(&mut self, cx: &mut LayoutContext, constraints: LayoutConstraints) -> Vec2 {
        Vec2::new(
            constraints.max_size.x.max(0.0),
            cx.pt(BAR_H).min(constraints.max_size.y.max(0.0)),
        )
    }

    fn layout(&mut self, _cx: &mut LayoutContext, bounds: Rect) {
        self.cached_bounds = bounds;
    }

    fn accessibility(&self, node: &mut AccessKitNode) {
        node.set_role(accesskit::Role::ProgressIndicator);
        node.set_min_numeric_value(0.0);
        node.set_max_numeric_value(1.0);
        if let Some(v) = self.value {
            node.set_numeric_value(f64::from(v));
        }
    }

    fn tick(&mut self, dt: std::time::Duration) -> bool {
        // A bound signal wins over the stored value — poll every tick.
        if let Some(sig) = &self.signal {
            let v = sig().clamp(0.0, 1.0);
            let changed = self.value != Some(v);
            self.value = Some(v);
            return changed;
        }
        // Indeterminate bars animate every frame; determinate bars are
        // static — report dirty only while animating.
        if self.value.is_none() {
            self.tick(dt.as_secs_f32());
            true
        } else {
            false
        }
    }

    fn paint(&self, cx: &mut PaintContext) {
        let b = cx.bounds;
        let h = cx.pt(BAR_H).min(b.size.y);
        let y = b.origin.y + (b.size.y - h) / 2.0;
        let track = kurbo::Rect::new(
            f64::from(b.origin.x),
            f64::from(y),
            f64::from(b.max_x()),
            f64::from(y + h),
        );
        let pill = Shape::PILL;
        cx.list
            .push_fill_shape(track, &pill, cx.color(TokenKey::SurfaceColor, TRACK));
        let accent = cx.color(TokenKey::AccentColor, ACCENT);
        let w = b.size.x;
        let fill = match self.value {
            Some(v) => Some((0.0, v * w)),
            None => {
                // A 30%-wide segment sweeping left→right with a small
                // ease: translate phase into a position that overshoots
                // both ends so the segment fully exits.
                let seg = w * 0.3;
                let x = -seg + self.phase * (w + seg);
                Some((x, seg))
            }
        };
        if let Some((x, fw)) = fill {
            if fw > 0.0 {
                let rect = kurbo::Rect::new(
                    f64::from(b.origin.x + x),
                    f64::from(y),
                    f64::from(b.origin.x + x + fw),
                    f64::from(y + h),
                );
                // Clip the fill to the track so indeterminate
                // overshoot doesn't spill past the rounded ends.
                cx.list.push_clip_shape(track, &pill);
                cx.list.push_fill_shape(rect, &pill, accent);
                cx.list.pop_clip();
            }
        }
    }
}

/// A spinning arc indicator. `phase` advances via [`Spinner::tick`]
/// (dt in seconds, one revolution ≈ 0.8s).
///
/// # Examples
///
/// ```
/// use martensite::widgets::Spinner;
///
/// let mut sp = Spinner::new();
/// sp.tick(0.4);
/// assert!(sp.phase > 0.0);
/// ```
#[derive(Clone, Debug)]
pub struct Spinner {
    /// Revolution phase, 0..1.
    pub phase: f32,
    /// Diameter in logical points.
    pub size: f32,
    /// Cached bounds from the last layout pass.
    cached_bounds: Rect,
}

impl Spinner {
    /// A 20pt spinner.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite::widgets::Spinner;
    ///
    /// let sp = Spinner::new();
    /// assert_eq!(sp.size, 20.0);
    /// ```
    pub fn new() -> Self {
        Self {
            phase: 0.0,
            size: 20.0,
            cached_bounds: Rect::default(),
        }
    }

    /// Sets the diameter (logical points).
    #[must_use]
    pub fn size(mut self, size: f32) -> Self {
        self.size = size.max(1.0);
        self
    }

    /// Advances the animation by `dt` seconds.
    pub fn tick(&mut self, dt: f32) {
        self.phase = (self.phase + dt / 0.8) % 1.0;
    }
}

impl Default for Spinner {
    fn default() -> Self {
        Self::new()
    }
}

impl Widget for Spinner {
    fn measure(&mut self, cx: &mut LayoutContext, constraints: LayoutConstraints) -> Vec2 {
        let d = cx.pt(self.size);
        Vec2::new(
            d.min(constraints.max_size.x.max(0.0)),
            d.min(constraints.max_size.y.max(0.0)),
        )
    }

    fn layout(&mut self, _cx: &mut LayoutContext, bounds: Rect) {
        self.cached_bounds = bounds;
    }

    fn accessibility(&self, node: &mut AccessKitNode) {
        node.set_role(accesskit::Role::ProgressIndicator);
        node.set_label("Loading");
    }

    fn tick(&mut self, dt: std::time::Duration) -> bool {
        self.tick(dt.as_secs_f32());
        true
    }

    fn paint(&self, cx: &mut PaintContext) {
        let b = cx.bounds;
        let d = b.size.x.min(b.size.y);
        let inset = cx.pt(2.0);
        let inset = f64::from(inset);
        let rect = kurbo::Rect::new(
            f64::from(b.origin.x + (b.size.x - d) / 2.0) + inset,
            f64::from(b.origin.y + (b.size.y - d) / 2.0) + inset,
            f64::from(b.origin.x + (b.size.x + d) / 2.0) - inset,
            f64::from(b.origin.y + (b.size.y + d) / 2.0) - inset,
        );
        // 270° arc starting at the phase angle, sampled to segments —
        // smooth enough at spinner diameters.
        let start = f64::from(self.phase) * std::f64::consts::TAU;
        let center = rect.center();
        let r = rect.width() / 2.0;
        let sweep = std::f64::consts::TAU * 0.75;
        const SEGS: usize = 24;
        let mut path = kurbo::BezPath::new();
        for i in 0..=SEGS {
            let a = start + sweep * (i as f64 / SEGS as f64);
            let p = (center.x + r * a.cos(), center.y + r * a.sin());
            if i == 0 {
                path.move_to(p);
            } else {
                path.line_to(p);
            }
        }
        cx.list
            .push_stroke_path(path, cx.pt(2.0), cx.color(TokenKey::AccentColor, ACCENT));
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use martensite_core::HotNode;

    #[test]
    fn progress_determinate() {
        let bar = ProgressBar::new().value(2.0);
        assert_eq!(bar.fraction(), Some(1.0));
    }

    #[test]
    fn progress_indeterminate_ticks() {
        let mut bar = ProgressBar::new();
        bar.tick(0.6);
        assert!(bar.phase > 0.0);
        let p = bar.phase;
        bar.tick(0.0);
        assert_eq!(bar.phase, p);
    }

    #[test]
    fn progress_a11y() {
        let bar = ProgressBar::new().value(0.5);
        let mut node = accesskit::Node::new(accesskit::Role::Unknown);
        bar.accessibility(&mut node);
        assert_eq!(node.role(), accesskit::Role::ProgressIndicator);
        assert_eq!(node.numeric_value(), Some(0.5));
    }

    #[test]
    fn spinner_ticks() {
        let mut sp = Spinner::new();
        sp.tick(0.8);
        assert!((sp.phase - 1.0).abs() < 1e-6 || sp.phase < 1.0);
        let mut hot = HotNode::default();
        let mut cx = LayoutContext {
            hot: &mut hot,
            scale: 1.0,
        };
        let size = sp.measure(
            &mut cx,
            LayoutConstraints {
                min_size: Vec2::ZERO,
                max_size: Vec2::new(100.0, 100.0),
            },
        );
        assert_eq!(size, Vec2::new(20.0, 20.0));
    }
}
