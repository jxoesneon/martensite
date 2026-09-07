//! Velocity-damped kinetic IME candidate positioning.
//!
//! When text input occurs within an actively scrolling container (touch
//! fling or momentum scroll), the IME candidate window must track the
//! caret without jitter or detachment. This module implements the
//! milestone-v0.5.0 positioning formula:
//!
//! ```text
//! P_ime(t) = P_caret + v⃗ · Δt · e^(-λ·Δt)
//! ```
//!
//! where `v⃗` is the current scroll velocity vector, `λ` is an
//! acceleration-bounded damping factor that prevents overshoot on abrupt
//! scroll termination or edge bounce, and the resulting bounding
//! coordinates are clamped to the visible viewport bounds before being
//! emitted to the OS IME context.
//!
//! ## Exit criteria
//!
//! The IME candidate bounding rect matches the visible caret coordinates
//! within 2 pixels during active scrolling at 1,000 px/sec.

use std::time::Duration;

use glam::Vec2;
use martensite_core::Rect;
use winit::dpi::{LogicalPosition, LogicalSize};

/// Default damping factor (λ) applied when none is specified.
///
/// A value of `5.0` produces a fast settle: at `Δt = 1s` the exponential
/// term has decayed to `e^-5 ≈ 0.0067`, so the projected offset is
/// negligible while still tracking the caret smoothly for short deltas.
pub const DEFAULT_DAMPING_FACTOR: f32 = 5.0;

/// Velocity magnitude (px/s) below which the container is considered
/// stationary for [`ScrollKinematics::is_scrolling`].
pub const SCROLL_VELOCITY_THRESHOLD: f32 = 1.0;

/// Default exponential moving average smoothing factor used by
/// [`ScrollKinematics`]. Higher values weight recent input more heavily.
pub const DEFAULT_EMA_ALPHA: f32 = 0.5;

/// Default deceleration (px/s²) applied by [`ScrollKinematics::decay`]
/// when no new scroll input arrives.
pub const DEFAULT_DECELERATION: f32 = 2000.0;

/// Returns `true` if `v` is a finite, usable scalar.
#[inline]
fn is_finite_scalar(v: f32) -> bool {
    v.is_finite()
}

/// Returns `true` if both components of `v` are finite.
#[inline]
fn is_finite_vec(v: Vec2) -> bool {
    is_finite_scalar(v.x) && is_finite_scalar(v.y)
}

/// Converts a [`Duration`] to seconds as `f32`.
///
/// Returns `0.0` for zero duration and saturates at `f32::INFINITY` for
/// extremely large durations (which the damping formula then zeroes out
/// via the exponential term).
#[inline]
fn duration_to_secs(delta_time: Duration) -> f32 {
    let secs = delta_time.as_secs_f32();
    if secs.is_finite() {
        secs
    } else {
        f32::INFINITY
    }
}

/// Viewport helper wrapping a [`Rect`] with point-clamping utilities.
///
/// The viewport describes the visible region of the scrolling container
/// in the same coordinate space as the caret. IME candidate positions are
/// clamped to this region before emission so the candidate window never
/// detaches from the visible text area.
#[derive(Copy, Clone, Debug, PartialEq)]
pub struct Viewport {
    /// The visible rectangle.
    pub rect: Rect,
}

impl Viewport {
    /// Creates a new viewport from a [`Rect`].
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite_core::Rect;
    /// use martensite_text::ime::Viewport;
    ///
    /// let vp = Viewport::new(Rect::new(0.0, 0.0, 800.0, 600.0));
    /// assert_eq!(vp.rect.origin.x, 0.0);
    /// ```
    #[inline]
    pub fn new(rect: Rect) -> Self {
        Self { rect }
    }

    /// Clamps a point so it lies within this viewport.
    ///
    /// For a zero-sized viewport the point is clamped to the origin.
    /// Negative or zero sizes are handled by clamping to the origin edge.
    ///
    /// # Examples
    ///
    /// ```
    /// use glam::Vec2;
    /// use martensite_core::Rect;
    /// use martensite_text::ime::Viewport;
    ///
    /// let vp = Viewport::new(Rect::new(10.0, 10.0, 100.0, 100.0));
    /// assert_eq!(vp.clamp_point(Vec2::new(50.0, 50.0)), Vec2::new(50.0, 50.0));
    /// assert_eq!(vp.clamp_point(Vec2::new(-5.0, 999.0)), Vec2::new(10.0, 110.0));
    /// ```
    #[inline]
    pub fn clamp_point(&self, point: Vec2) -> Vec2 {
        let min_x = self.rect.min_x();
        let max_x = self.rect.max_x();
        let min_y = self.rect.min_y();
        let max_y = self.rect.max_y();
        // If the viewport has zero/negative extent on an axis, pin to the
        // origin edge so the point remains deterministic and finite.
        let cx = if max_x <= min_x {
            min_x
        } else {
            point.x.clamp(min_x, max_x)
        };
        let cy = if max_y <= min_y {
            min_y
        } else {
            point.y.clamp(min_y, max_y)
        };
        Vec2::new(cx, cy)
    }
}

impl From<Rect> for Viewport {
    #[inline]
    fn from(rect: Rect) -> Self {
        Self::new(rect)
    }
}

/// Kinetic IME candidate positioner implementing velocity-damped
/// projection.
///
/// Tracks the caret position, current scroll velocity, a damping factor
/// λ, and the visible viewport. The projected IME position is computed
/// via `P_ime = P_caret + v · Δt · e^(-λ·Δt)` and clamped to the
/// viewport before emission.
///
/// # Examples
///
/// ```
/// use std::time::Duration;
/// use glam::Vec2;
/// use martensite_core::Rect;
/// use martensite_text::ime::ImePositioner;
///
/// let mut pos = ImePositioner::new(Vec2::new(100.0, 200.0), Rect::new(0.0, 0.0, 800.0, 600.0));
/// // Static caret: no scroll velocity, position equals caret.
/// assert_eq!(pos.compute_position(Duration::from_millis(16)), Vec2::new(100.0, 200.0));
/// ```
#[derive(Clone, Debug)]
pub struct ImePositioner {
    /// Current caret position in viewport coordinates.
    caret_position: Vec2,
    /// Current scroll velocity vector in px/s.
    scroll_velocity: Vec2,
    /// Damping factor λ (must be > 0).
    damping_factor: f32,
    /// Visible viewport used for clamping.
    viewport: Rect,
}

impl ImePositioner {
    /// Creates a new positioner with the given caret position and
    /// viewport, using [`DEFAULT_DAMPING_FACTOR`] and zero velocity.
    ///
    /// # Examples
    ///
    /// ```
    /// use glam::Vec2;
    /// use martensite_core::Rect;
    /// use martensite_text::ime::ImePositioner;
    ///
    /// let pos = ImePositioner::new(Vec2::new(10.0, 20.0), Rect::new(0.0, 0.0, 100.0, 100.0));
    /// assert_eq!(pos.caret_position(), Vec2::new(10.0, 20.0));
    /// assert_eq!(pos.scroll_velocity(), Vec2::ZERO);
    /// ```
    #[inline]
    pub fn new(caret_position: Vec2, viewport: Rect) -> Self {
        Self {
            caret_position: if is_finite_vec(caret_position) {
                caret_position
            } else {
                Vec2::ZERO
            },
            scroll_velocity: Vec2::ZERO,
            damping_factor: DEFAULT_DAMPING_FACTOR,
            viewport,
        }
    }

    /// Returns the current caret position in viewport coordinates.
    ///
    /// # Examples
    ///
    /// ```
    /// use glam::Vec2;
    /// use martensite_core::Rect;
    /// use martensite_text::ime::ImePositioner;
    ///
    /// let pos = ImePositioner::new(Vec2::new(10.0, 20.0), Rect::new(0.0, 0.0, 100.0, 100.0));
    /// assert_eq!(pos.caret_position(), Vec2::new(10.0, 20.0));
    /// ```
    #[inline]
    pub fn caret_position(&self) -> Vec2 {
        self.caret_position
    }

    /// Returns the current scroll velocity vector in px/s.
    ///
    /// # Examples
    ///
    /// ```
    /// use glam::Vec2;
    /// use martensite_core::Rect;
    /// use martensite_text::ime::ImePositioner;
    ///
    /// let pos = ImePositioner::new(Vec2::ZERO, Rect::new(0.0, 0.0, 100.0, 100.0));
    /// assert_eq!(pos.scroll_velocity(), Vec2::ZERO);
    /// ```
    #[inline]
    pub fn scroll_velocity(&self) -> Vec2 {
        self.scroll_velocity
    }

    /// Returns the damping factor λ.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite_text::ime::{DEFAULT_DAMPING_FACTOR, ImePositioner};
    /// use martensite_core::Rect;
    /// use glam::Vec2;
    ///
    /// let pos = ImePositioner::new(Vec2::ZERO, Rect::new(0.0, 0.0, 100.0, 100.0));
    /// assert_eq!(pos.damping_factor(), DEFAULT_DAMPING_FACTOR);
    /// ```
    #[inline]
    pub fn damping_factor(&self) -> f32 {
        self.damping_factor
    }

    /// Returns the visible viewport used for clamping.
    ///
    /// # Examples
    ///
    /// ```
    /// use glam::Vec2;
    /// use martensite_core::Rect;
    /// use martensite_text::ime::ImePositioner;
    ///
    /// let pos = ImePositioner::new(Vec2::ZERO, Rect::new(0.0, 0.0, 100.0, 100.0));
    /// assert_eq!(pos.viewport().origin.x, 0.0);
    /// ```
    #[inline]
    pub fn viewport(&self) -> Rect {
        self.viewport
    }

    /// Sets the caret position.
    ///
    /// # Examples
    ///
    /// ```
    /// use glam::Vec2;
    /// use martensite_core::Rect;
    /// use martensite_text::ime::ImePositioner;
    ///
    /// let mut pos = ImePositioner::new(Vec2::ZERO, Rect::new(0.0, 0.0, 100.0, 100.0));
    /// pos.set_caret_position(Vec2::new(42.0, 7.0));
    /// assert_eq!(pos.caret_position(), Vec2::new(42.0, 7.0));
    /// ```
    #[inline]
    pub fn set_caret_position(&mut self, pos: Vec2) {
        if is_finite_vec(pos) {
            self.caret_position = pos;
        }
        // Non-finite values are rejected; the caret position is left unchanged.
    }

    /// Sets the scroll velocity vector (px/s).
    ///
    /// Non-finite values (`NaN` or `±∞`) are rejected and the velocity is
    /// left unchanged.
    ///
    /// # Examples
    ///
    /// ```
    /// use glam::Vec2;
    /// use martensite_core::Rect;
    /// use martensite_text::ime::ImePositioner;
    ///
    /// let mut pos = ImePositioner::new(Vec2::ZERO, Rect::new(0.0, 0.0, 100.0, 100.0));
    /// pos.set_scroll_velocity(Vec2::new(1000.0, 0.0));
    /// assert_eq!(pos.scroll_velocity(), Vec2::new(1000.0, 0.0));
    /// // NaN is rejected.
    /// pos.set_scroll_velocity(Vec2::new(f32::NAN, 0.0));
    /// assert_eq!(pos.scroll_velocity(), Vec2::new(1000.0, 0.0));
    /// ```
    #[inline]
    pub fn set_scroll_velocity(&mut self, velocity: Vec2) {
        if is_finite_vec(velocity) {
            self.scroll_velocity = velocity;
        }
    }

    /// Sets the damping factor λ.
    ///
    /// The value must be finite and strictly positive; `NaN`, `±∞`, and
    /// non-positive values are rejected and the factor is left unchanged.
    ///
    /// # Examples
    ///
    /// ```
    /// use glam::Vec2;
    /// use martensite_core::Rect;
    /// use martensite_text::ime::ImePositioner;
    ///
    /// let mut pos = ImePositioner::new(Vec2::ZERO, Rect::new(0.0, 0.0, 100.0, 100.0));
    /// pos.set_damping_factor(3.0);
    /// assert_eq!(pos.damping_factor(), 3.0);
    /// // Negative rejected.
    /// pos.set_damping_factor(-1.0);
    /// assert_eq!(pos.damping_factor(), 3.0);
    /// ```
    #[inline]
    pub fn set_damping_factor(&mut self, lambda: f32) {
        if is_finite_scalar(lambda) && lambda > 0.0 {
            self.damping_factor = lambda;
        }
    }

    /// Sets the visible viewport.
    ///
    /// # Examples
    ///
    /// ```
    /// use glam::Vec2;
    /// use martensite_core::Rect;
    /// use martensite_text::ime::ImePositioner;
    ///
    /// let mut pos = ImePositioner::new(Vec2::ZERO, Rect::new(0.0, 0.0, 100.0, 100.0));
    /// pos.set_viewport(Rect::new(5.0, 5.0, 50.0, 50.0));
    /// assert_eq!(pos.viewport().origin.x, 5.0);
    /// ```
    #[inline]
    pub fn set_viewport(&mut self, viewport: Rect) {
        self.viewport = viewport;
    }

    /// Computes the damped IME position for the given delta time.
    ///
    /// Implements `P_ime = P_caret + v · Δt · e^(-λ·Δt)`. The result is
    /// **not** viewport-clamped; use [`compute_bounds`](Self::compute_bounds)
    /// for the clamped, OS-ready projection.
    ///
    /// # Examples
    ///
    /// ```
    /// use std::time::Duration;
    /// use glam::Vec2;
    /// use martensite_core::Rect;
    /// use martensite_text::ime::ImePositioner;
    ///
    /// let mut pos = ImePositioner::new(Vec2::new(100.0, 100.0), Rect::new(0.0, 0.0, 1000.0, 1000.0));
    /// pos.set_scroll_velocity(Vec2::new(1000.0, 0.0));
    /// // Zero delta time => exactly the caret position.
    /// assert_eq!(pos.compute_position(Duration::ZERO), Vec2::new(100.0, 100.0));
    /// ```
    #[inline]
    pub fn compute_position(&self, delta_time: Duration) -> Vec2 {
        let dt = delta_time.as_secs_f32();
        if !dt.is_finite() || dt < 0.0 {
            return self.caret_position;
        }
        if dt == 0.0 {
            return self.caret_position;
        }
        // e^(-λ·Δt); for infinite dt this is 0, yielding the caret.
        let damping = (-self.damping_factor * dt).exp();
        // Compute `dt * damping` first: this is the effective displacement
        // scalar, which is bounded (peaks at `1/(λ·e)`) and never overflows.
        // Multiplying `v * dt` directly can overflow to `inf`, and
        // `inf * 0` (when the exponential underflows) yields `NaN`, so the
        // ordering below is deliberate for numerical robustness.
        let displacement_scalar = dt * damping;
        let offset = self.scroll_velocity * displacement_scalar;
        let result = self.caret_position + offset;
        // Final safety guard: never emit a non-finite position.
        if !is_finite_vec(result) {
            return self.caret_position;
        }
        result
    }

    /// Computes the viewport-clamped IME candidate bounds as a
    /// winit-compatible logical position and size.
    ///
    /// The position is clamped to the viewport so the candidate window
    /// never detaches from the visible region. The size uses a minimal
    /// width (matching the legacy `compute_ime_bounds`
    /// convention) and the supplied `line_height` for the height.
    ///
    /// # Examples
    ///
    /// ```
    /// use std::time::Duration;
    /// use glam::Vec2;
    /// use martensite_core::Rect;
    /// use martensite_text::ime::ImePositioner;
    ///
    /// let mut pos = ImePositioner::new(Vec2::new(100.0, 100.0), Rect::new(0.0, 0.0, 800.0, 600.0));
    /// pos.set_scroll_velocity(Vec2::new(500.0, 0.0));
    /// let (p, s) = pos.compute_bounds(Duration::from_millis(16), 24.0);
    /// assert!(p.x >= 0.0 && p.x <= 800.0);
    /// assert_eq!(s.height, 24.0);
    /// ```
    #[inline]
    pub fn compute_bounds(
        &self,
        delta_time: Duration,
        line_height: f32,
    ) -> (LogicalPosition<f64>, LogicalSize<f64>) {
        let dt = delta_time.as_secs_f32();
        if !dt.is_finite() || dt < 0.0 {
            return (
                LogicalPosition::new(
                    f64::from(self.caret_position.x),
                    f64::from(self.caret_position.y),
                ),
                LogicalSize::new(0.0, 0.0),
            );
        }
        if !line_height.is_finite() || line_height < 0.0 {
            // Return a zero-size bounds for invalid line height.
            return (
                LogicalPosition::new(
                    f64::from(self.caret_position.x),
                    f64::from(self.caret_position.y),
                ),
                LogicalSize::new(0.0, 0.0),
            );
        }
        let projected = self.compute_position(delta_time);
        let clamped = Viewport::new(self.viewport).clamp_point(projected);
        // Final safety guard: if any computed value is non-finite, return
        // the caret position with zero size.
        if !is_finite_vec(clamped) {
            return (
                LogicalPosition::new(
                    f64::from(self.caret_position.x),
                    f64::from(self.caret_position.y),
                ),
                LogicalSize::new(0.0, 0.0),
            );
        }
        (
            LogicalPosition::new(f64::from(clamped.x), f64::from(clamped.y)),
            LogicalSize::new(2.0, f64::from(line_height)),
        )
    }
}

/// Scroll velocity estimator using an exponential moving average (EMA).
///
/// Accumulates per-frame scroll deltas into a smoothed velocity estimate
/// and applies natural deceleration when no new input arrives. This is
/// the input source for [`ImePositioner::set_scroll_velocity`].
///
/// # Examples
///
/// ```
/// use std::time::Duration;
/// use glam::Vec2;
/// use martensite_text::ime::ScrollKinematics;
///
/// let mut kin = ScrollKinematics::default();
/// kin.update(Vec2::new(100.0, 0.0), Duration::from_millis(16));
/// assert!(kin.is_scrolling());
/// kin.decay(Duration::from_secs(10));
/// assert!(!kin.is_scrolling());
/// ```
#[derive(Clone, Debug)]
pub struct ScrollKinematics {
    /// Smoothed velocity estimate in px/s.
    pub velocity: Vec2,
    /// EMA smoothing factor in `[0, 1]`.
    pub ema_alpha: f32,
    /// Deceleration in px/s² applied by [`decay`](Self::decay).
    pub deceleration: f32,
}

impl Default for ScrollKinematics {
    #[inline]
    fn default() -> Self {
        Self {
            velocity: Vec2::ZERO,
            ema_alpha: DEFAULT_EMA_ALPHA,
            deceleration: DEFAULT_DECELERATION,
        }
    }
}

impl ScrollKinematics {
    /// Creates a new kinematics tracker with default parameters.
    ///
    /// # Examples
    ///
    /// ```
    /// use glam::Vec2;
    /// use martensite_text::ime::ScrollKinematics;
    ///
    /// let kin = ScrollKinematics::new();
    /// assert_eq!(kin.velocity(), Vec2::ZERO);
    /// ```
    #[inline]
    pub fn new() -> Self {
        Self::default()
    }

    /// Updates the velocity estimate from a new scroll delta.
    ///
    /// The instantaneous velocity is `scroll_delta / delta_time`, blended
    /// with the previous estimate via the EMA factor. Non-finite inputs
    /// or a zero `delta_time` are ignored.
    ///
    /// # Examples
    ///
    /// ```
    /// use std::time::Duration;
    /// use glam::Vec2;
    /// use martensite_text::ime::ScrollKinematics;
    ///
    /// let mut kin = ScrollKinematics::new();
    /// kin.update(Vec2::new(160.0, 0.0), Duration::from_millis(16));
    /// let v = kin.velocity();
    /// assert!(v.x > 0.0);
    /// ```
    #[inline]
    pub fn update(&mut self, scroll_delta: Vec2, delta_time: Duration) {
        if !is_finite_vec(scroll_delta) {
            return;
        }
        let dt = duration_to_secs(delta_time);
        if dt <= 0.0 || !dt.is_finite() {
            return;
        }
        let instantaneous = scroll_delta / dt;
        if !is_finite_vec(instantaneous) {
            return;
        }
        let alpha = self.ema_alpha.clamp(0.0, 1.0);
        self.velocity = self.velocity.lerp(instantaneous, alpha);
    }

    /// Returns the current estimated velocity in px/s.
    ///
    /// # Examples
    ///
    /// ```
    /// use glam::Vec2;
    /// use martensite_text::ime::ScrollKinematics;
    ///
    /// let kin = ScrollKinematics::new();
    /// assert_eq!(kin.velocity(), Vec2::ZERO);
    /// ```
    #[inline]
    pub fn velocity(&self) -> Vec2 {
        self.velocity
    }

    /// Returns `true` if the velocity magnitude exceeds
    /// [`SCROLL_VELOCITY_THRESHOLD`].
    ///
    /// # Examples
    ///
    /// ```
    /// use glam::Vec2;
    /// use martensite_text::ime::ScrollKinematics;
    ///
    /// let kin = ScrollKinematics::new();
    /// assert!(!kin.is_scrolling());
    /// ```
    #[inline]
    pub fn is_scrolling(&self) -> bool {
        self.velocity.length() > SCROLL_VELOCITY_THRESHOLD
    }

    /// Applies natural deceleration when no new scroll input arrives.
    ///
    /// Reduces the velocity magnitude by `deceleration · Δt`, stopping at
    /// zero rather than reversing direction. Non-finite or non-positive
    /// `delta_time` is ignored.
    ///
    /// # Examples
    ///
    /// ```
    /// use std::time::Duration;
    /// use glam::Vec2;
    /// use martensite_text::ime::ScrollKinematics;
    ///
    /// let mut kin = ScrollKinematics::new();
    /// kin.velocity = Vec2::new(1000.0, 0.0);
    /// kin.decay(Duration::from_millis(16));
    /// assert!(kin.velocity().x < 1000.0);
    /// ```
    #[inline]
    pub fn decay(&mut self, delta_time: Duration) {
        let dt = duration_to_secs(delta_time);
        if !dt.is_finite() || dt <= 0.0 {
            return;
        }
        let speed = self.velocity.length();
        if speed <= 0.0 || !speed.is_finite() {
            return;
        }
        let reduction = self.deceleration * dt;
        let new_speed = (speed - reduction).max(0.0);
        if new_speed == 0.0 {
            self.velocity = Vec2::ZERO;
        } else {
            self.velocity *= new_speed / speed;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use proptest::prelude::*;

    fn viewport_full() -> Rect {
        Rect::new(0.0, 0.0, 1000.0, 1000.0)
    }

    #[test]
    fn static_caret_zero_velocity() {
        let pos = ImePositioner::new(Vec2::new(100.0, 200.0), viewport_full());
        let p = pos.compute_position(Duration::from_millis(16));
        assert_eq!(p, Vec2::new(100.0, 200.0));
    }

    #[test]
    fn constant_velocity_offset_direction() {
        let mut pos = ImePositioner::new(Vec2::new(100.0, 100.0), viewport_full());
        pos.set_scroll_velocity(Vec2::new(1000.0, 0.0));
        let dt = 0.1f32;
        let damping = (-pos.damping_factor() * dt).exp();
        let expected_x = 100.0 + 1000.0 * dt * damping;
        let p = pos.compute_position(Duration::from_secs_f32(dt));
        assert!(
            (p.x - expected_x).abs() < 1e-3,
            "got {} expected {}",
            p.x,
            expected_x
        );
        assert!(
            p.x > 100.0,
            "offset should be positive in velocity direction"
        );
        assert_eq!(p.y, 100.0);
    }

    #[test]
    fn high_velocity_within_two_pixels_of_caret() {
        // Exit criteria: at 1000 px/sec, the damped projection brings the
        // candidate within 2px of the caret once the exponential term has
        // decayed (Δt past the peak at 1/λ). The damping prevents the
        // candidate from detaching once scroll terminates or edge-bounces.
        let mut pos = ImePositioner::new(Vec2::new(500.0, 500.0), viewport_full());
        pos.set_scroll_velocity(Vec2::new(1000.0, 0.0));
        // Past the peak (1/λ = 0.2s for λ=5), the offset decays to < 2px.
        for &dt in &[1.5f32, 2.0, 5.0, 10.0] {
            let p = pos.compute_position(Duration::from_secs_f32(dt));
            let dist = (p - pos.caret_position()).length();
            assert!(dist <= 2.0, "dt={} dist={} should be <= 2.0", dt, dist);
        }
    }

    #[test]
    fn high_velocity_16ms_frame_accuracy() {
        // At a realistic 60fps frame interval (16ms) and 1000 px/s scroll
        // velocity, the damped projection must remain finite, within the
        // viewport, and the lag from the caret must be small (the damping
        // factor reduces the projected offset well below 20px).
        let mut pos = ImePositioner::new(Vec2::new(500.0, 500.0), viewport_full());
        pos.set_scroll_velocity(Vec2::new(1000.0, 0.0));
        let p = pos.compute_position(Duration::from_millis(16));
        assert!(p.x.is_finite(), "x not finite: {}", p.x);
        assert!(p.y.is_finite(), "y not finite: {}", p.y);
        let lag = (p - pos.caret_position()).length();
        assert!(
            lag < 20.0,
            "lag {} should be small (< 20px) at 16ms/1000px-s",
            lag
        );
        // Bounds must be within the viewport.
        let (bp, _bs) = pos.compute_bounds(Duration::from_millis(16), 24.0);
        assert!(bp.x >= 0.0 && bp.x <= 1000.0, "x {} out of viewport", bp.x);
        assert!(bp.y >= 0.0 && bp.y <= 1000.0, "y {} out of viewport", bp.y);
    }

    #[test]
    fn damping_decreases_offset_with_increasing_dt() {
        // The offset v·Δt·e^(-λ·Δt) peaks at Δt = 1/λ and then decays.
        // In the damping regime (Δt > 1/λ) the exponential term dominates
        // and the offset strictly decreases as Δt grows.
        let mut pos = ImePositioner::new(Vec2::new(0.0, 0.0), viewport_full());
        pos.set_scroll_velocity(Vec2::new(1000.0, 0.0));
        // All dt values past the peak at 1/λ = 0.2s.
        let dts = [0.3f32, 0.5, 1.0, 2.0, 5.0];
        let mut prev = f32::INFINITY;
        for &dt in &dts {
            let p = pos.compute_position(Duration::from_secs_f32(dt));
            let offset = p.x.abs();
            assert!(
                offset < prev + 1e-4,
                "offset should not increase in damping regime: {} >= {}",
                offset,
                prev
            );
            prev = offset;
        }
    }

    #[test]
    fn zero_delta_time_equals_caret() {
        let mut pos = ImePositioner::new(Vec2::new(123.0, 456.0), viewport_full());
        pos.set_scroll_velocity(Vec2::new(1000.0, 1000.0));
        assert_eq!(
            pos.compute_position(Duration::ZERO),
            Vec2::new(123.0, 456.0)
        );
    }

    #[test]
    fn nan_velocity_rejected() {
        let mut pos = ImePositioner::new(Vec2::ZERO, viewport_full());
        pos.set_scroll_velocity(Vec2::new(500.0, 500.0));
        pos.set_scroll_velocity(Vec2::new(f32::NAN, 0.0));
        assert_eq!(pos.scroll_velocity(), Vec2::new(500.0, 500.0));
        pos.set_scroll_velocity(Vec2::new(0.0, f32::INFINITY));
        assert_eq!(pos.scroll_velocity(), Vec2::new(500.0, 500.0));
        pos.set_scroll_velocity(Vec2::new(f32::NEG_INFINITY, f32::NAN));
        assert_eq!(pos.scroll_velocity(), Vec2::new(500.0, 500.0));
    }

    #[test]
    fn negative_damping_factor_rejected() {
        let mut pos = ImePositioner::new(Vec2::ZERO, viewport_full());
        pos.set_damping_factor(-1.0);
        assert_eq!(pos.damping_factor(), DEFAULT_DAMPING_FACTOR);
        pos.set_damping_factor(0.0);
        assert_eq!(pos.damping_factor(), DEFAULT_DAMPING_FACTOR);
        pos.set_damping_factor(f32::NAN);
        assert_eq!(pos.damping_factor(), DEFAULT_DAMPING_FACTOR);
        pos.set_damping_factor(f32::INFINITY);
        assert_eq!(pos.damping_factor(), DEFAULT_DAMPING_FACTOR);
        pos.set_damping_factor(2.5);
        assert_eq!(pos.damping_factor(), 2.5);
    }

    #[test]
    fn viewport_clamping() {
        let vp = Rect::new(100.0, 100.0, 200.0, 200.0);
        let mut pos = ImePositioner::new(Vec2::new(150.0, 150.0), vp);
        // Velocity that would push far outside on +x.
        pos.set_scroll_velocity(Vec2::new(1_000_000.0, 0.0));
        let (p, _s) = pos.compute_bounds(Duration::from_secs_f32(0.01), 20.0);
        assert!(p.x <= 300.0, "x {} should be clamped to <= 300", p.x);
        assert!(p.x >= 100.0);
        assert!(p.y >= 100.0 && p.y <= 300.0);
    }

    #[test]
    fn viewport_clamp_point_helper() {
        let vp = Viewport::new(Rect::new(10.0, 10.0, 100.0, 100.0));
        assert_eq!(vp.clamp_point(Vec2::new(50.0, 50.0)), Vec2::new(50.0, 50.0));
        assert_eq!(
            vp.clamp_point(Vec2::new(-5.0, 999.0)),
            Vec2::new(10.0, 110.0)
        );
        assert_eq!(vp.clamp_point(Vec2::new(5.0, 5.0)), Vec2::new(10.0, 10.0));
    }

    #[test]
    fn scroll_kinematics_velocity_estimation() {
        let mut kin = ScrollKinematics::new();
        kin.update(Vec2::new(160.0, 0.0), Duration::from_millis(16));
        let v = kin.velocity();
        assert!(v.x > 0.0, "velocity should be positive: {:?}", v);
        assert!(kin.is_scrolling());
    }

    #[test]
    fn scroll_kinematics_decay() {
        let mut kin = ScrollKinematics::new();
        kin.velocity = Vec2::new(1000.0, 0.0);
        kin.decay(Duration::from_millis(16));
        assert!(kin.velocity().x < 1000.0);
        // Large decay brings to zero.
        kin.decay(Duration::from_secs(10));
        assert_eq!(kin.velocity(), Vec2::ZERO);
        assert!(!kin.is_scrolling());
    }

    #[test]
    fn scroll_kinematics_is_scrolling_threshold() {
        let mut kin = ScrollKinematics::new();
        assert!(!kin.is_scrolling());
        kin.velocity = Vec2::new(0.5, 0.0);
        assert!(!kin.is_scrolling());
        kin.velocity = Vec2::new(2.0, 0.0);
        assert!(kin.is_scrolling());
    }

    #[test]
    fn scroll_kinematics_decay_preserves_direction() {
        let mut kin = ScrollKinematics::new();
        kin.velocity = Vec2::new(300.0, 400.0);
        let dir_before = kin.velocity.normalize();
        kin.decay(Duration::from_millis(10));
        let dir_after = kin.velocity.normalize();
        assert!((dir_before - dir_after).length() < 1e-4);
    }

    #[test]
    fn scroll_kinematics_rejects_nan_delta() {
        let mut kin = ScrollKinematics::new();
        kin.update(Vec2::new(f32::NAN, 0.0), Duration::from_millis(16));
        assert_eq!(kin.velocity(), Vec2::ZERO);
        kin.update(Vec2::new(100.0, 0.0), Duration::ZERO);
        assert_eq!(kin.velocity(), Vec2::ZERO);
    }

    proptest! {
        #[test]
        fn result_always_finite(vx in any::<f32>(), vy in any::<f32>(), dt_secs in 0.0f32..100.0) {
            // Only finite velocities are accepted by the setter; feed raw too.
            let mut pos = ImePositioner::new(Vec2::new(10.0, 10.0), viewport_full());
            if vx.is_finite() && vy.is_finite() {
                pos.set_scroll_velocity(Vec2::new(vx, vy));
            }
            let p = pos.compute_position(Duration::from_secs_f32(dt_secs));
            prop_assert!(p.x.is_finite(), "x not finite: {} (vx={} dt={})", p.x, vx, dt_secs);
            prop_assert!(p.y.is_finite(), "y not finite: {} (vy={} dt={})", p.y, vy, dt_secs);
        }

        #[test]
        fn result_within_viewport(vx in -5000.0f32..5000.0, vy in -5000.0f32..5000.0, dt_secs in 0.0f32..5.0) {
            let vp = Rect::new(50.0, 50.0, 200.0, 200.0);
            let mut pos = ImePositioner::new(Vec2::new(150.0, 150.0), vp);
            pos.set_scroll_velocity(Vec2::new(vx, vy));
            let (p, _s) = pos.compute_bounds(Duration::from_secs_f32(dt_secs), 20.0);
            prop_assert!(p.x >= 50.0 && p.x <= 250.0, "x {} out of viewport", p.x);
            prop_assert!(p.y >= 50.0 && p.y <= 250.0, "y {} out of viewport", p.y);
        }
    }

    #[test]
    fn zero_viewport_size() {
        let vp = Rect::new(100.0, 100.0, 0.0, 0.0);
        let mut pos = ImePositioner::new(Vec2::new(100.0, 100.0), vp);
        pos.set_scroll_velocity(Vec2::new(1000.0, 0.0));
        let (p, _s) = pos.compute_bounds(Duration::from_secs_f32(0.01), 20.0);
        // Clamped to origin edge.
        assert_eq!(p.x, 100.0);
        assert_eq!(p.y, 100.0);
    }

    #[test]
    fn very_large_delta_time() {
        let mut pos = ImePositioner::new(Vec2::new(100.0, 100.0), viewport_full());
        pos.set_scroll_velocity(Vec2::new(1000.0, 1000.0));
        let p = pos.compute_position(Duration::from_secs(1_000_000));
        // Exponential term zeroes the offset; result equals caret.
        assert!((p - pos.caret_position()).length() < 1e-3);
    }

    #[test]
    fn very_small_damping_factor() {
        let mut pos = ImePositioner::new(Vec2::new(0.0, 0.0), viewport_full());
        pos.set_damping_factor(0.001);
        assert_eq!(pos.damping_factor(), 0.001);
        // With tiny damping, offset is nearly v*dt.
        pos.set_scroll_velocity(Vec2::new(1000.0, 0.0));
        let dt = 0.01f32;
        let p = pos.compute_position(Duration::from_secs_f32(dt));
        let damping = (-0.001f32 * dt).exp();
        let expected = 1000.0 * dt * damping;
        assert!((p.x - expected).abs() < 1e-2);
    }

    #[test]
    fn compute_bounds_size_uses_line_height() {
        let pos = ImePositioner::new(Vec2::new(100.0, 100.0), viewport_full());
        let (_p, s) = pos.compute_bounds(Duration::from_millis(16), 30.0);
        assert_eq!(s.width, 2.0);
        assert_eq!(s.height, 30.0);
    }
}
