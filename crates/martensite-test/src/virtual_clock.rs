//! Deterministic, manually-advancing clock for headless CI tests.
//!
//! [`VirtualClock`] replaces OS monotonic clocks during test execution so that
//! animations, timers, and physics can be driven deterministically frame by
//! frame. Because time only moves forward when the test explicitly calls
//! [`VirtualClock::advance`], there is zero timing jitter across CI runs.

use std::time::Duration;

/// The duration of a single frame at 60 frames per second (≈ 16.666 ms).
pub const FRAME_60FPS: Duration = Duration::from_nanos(16_666_666);
/// The duration of a single frame at 30 frames per second (≈ 33.333 ms).
///
/// Defined as exactly twice [`FRAME_60FPS`] so the frame-rate relationships
/// hold with sub-nanosecond precision.
pub const FRAME_30FPS: Duration = Duration::from_nanos(33_333_332);
/// The duration of a single frame at 120 frames per second (≈ 8.333 ms).
pub const FRAME_120FPS: Duration = Duration::from_nanos(8_333_333);

/// A deterministic, manually-advancing clock for headless CI tests.
///
/// `VirtualClock` holds a single [`Duration`] accumulator that only changes
/// when the test calls [`advance`](VirtualClock::advance) (or one of the
/// `step_*fps` helpers). This eliminates wall-clock dependence entirely: the
/// same sequence of `advance` calls always produces the same elapsed time,
/// which is the foundation of the deterministic CI test gate.
///
/// # Examples
///
/// ```
/// use martensite_test::VirtualClock;
/// use std::time::Duration;
///
/// let mut clock = VirtualClock::new();
/// assert_eq!(clock.now(), Duration::ZERO);
///
/// // Advance three 60 FPS frames.
/// clock.step_60fps();
/// clock.step_60fps();
/// clock.step_60fps();
///
/// assert!(clock.elapsed_millis() >= 49 && clock.elapsed_millis() <= 50);
/// ```
#[derive(Default, Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct VirtualClock {
    /// The total elapsed time accumulated by this virtual clock.
    pub elapsed: Duration,
}

impl VirtualClock {
    /// Creates a new [`VirtualClock`] initialized to zero elapsed time.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite_test::VirtualClock;
    /// use std::time::Duration;
    ///
    /// let clock = VirtualClock::new();
    /// assert_eq!(clock.elapsed, Duration::ZERO);
    /// ```
    pub fn new() -> Self {
        Self {
            elapsed: Duration::ZERO,
        }
    }

    /// Advances the clock's elapsed time by `dt`, simulating the passage of
    /// time.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite_test::VirtualClock;
    /// use std::time::Duration;
    ///
    /// let mut clock = VirtualClock::new();
    /// clock.advance(Duration::from_millis(16));
    /// assert_eq!(clock.now(), Duration::from_millis(16));
    /// ```
    pub fn advance(&mut self, dt: Duration) {
        self.elapsed += dt;
    }

    /// Returns the total elapsed time accumulated by this clock.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite_test::VirtualClock;
    /// use std::time::Duration;
    ///
    /// let mut clock = VirtualClock::new();
    /// clock.advance(Duration::from_secs(2));
    /// assert_eq!(clock.now(), Duration::from_secs(2));
    /// ```
    pub fn now(&self) -> Duration {
        self.elapsed
    }

    /// Returns the total elapsed time in seconds as a floating-point value.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite_test::VirtualClock;
    /// use std::time::Duration;
    ///
    /// let mut clock = VirtualClock::new();
    /// clock.advance(Duration::from_millis(500));
    /// assert!((clock.elapsed_secs() - 0.5).abs() < 1e-9);
    /// ```
    pub fn elapsed_secs(&self) -> f64 {
        self.elapsed.as_secs_f64()
    }

    /// Returns the total elapsed time in whole milliseconds (truncated
    /// toward zero, matching [`Duration::as_millis`]).
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite_test::VirtualClock;
    /// use std::time::Duration;
    ///
    /// let mut clock = VirtualClock::new();
    /// clock.advance(Duration::from_millis(42));
    /// assert_eq!(clock.elapsed_millis(), 42);
    /// ```
    pub fn elapsed_millis(&self) -> u64 {
        self.elapsed.as_millis() as u64
    }

    /// Resets the clock's elapsed time back to zero.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite_test::VirtualClock;
    /// use std::time::Duration;
    ///
    /// let mut clock = VirtualClock::new();
    /// clock.advance(Duration::from_secs(10));
    /// clock.reset();
    /// assert_eq!(clock.now(), Duration::ZERO);
    /// ```
    pub fn reset(&mut self) {
        self.elapsed = Duration::ZERO;
    }

    /// Advances the clock by a single 60 FPS frame (≈ 16.666 ms).
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite_test::VirtualClock;
    ///
    /// let mut clock = VirtualClock::new();
    /// clock.step_60fps();
    /// assert!(clock.elapsed_millis() == 16 || clock.elapsed_millis() == 17);
    /// ```
    pub fn step_60fps(&mut self) {
        self.elapsed += FRAME_60FPS;
    }

    /// Advances the clock by a single 30 FPS frame (≈ 33.333 ms).
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite_test::VirtualClock;
    ///
    /// let mut clock = VirtualClock::new();
    /// clock.step_30fps();
    /// assert!(clock.elapsed_millis() == 33 || clock.elapsed_millis() == 34);
    /// ```
    pub fn step_30fps(&mut self) {
        self.elapsed += FRAME_30FPS;
    }

    /// Advances the clock by a single 120 FPS frame (≈ 8.333 ms).
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite_test::VirtualClock;
    ///
    /// let mut clock = VirtualClock::new();
    /// clock.step_120fps();
    /// assert!(clock.elapsed_millis() == 8 || clock.elapsed_millis() == 9);
    /// ```
    pub fn step_120fps(&mut self) {
        self.elapsed += FRAME_120FPS;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    #[test]
    fn new_creates_zero_elapsed() {
        let clock = VirtualClock::new();
        assert_eq!(clock.elapsed, Duration::ZERO);
    }

    #[test]
    fn default_creates_zero_elapsed() {
        let clock = VirtualClock::default();
        assert_eq!(clock.elapsed, Duration::ZERO);
    }

    #[test]
    fn advance_adds_to_elapsed() {
        let mut clock = VirtualClock::new();
        clock.advance(Duration::from_secs(5));
        assert_eq!(clock.elapsed, Duration::from_secs(5));
    }

    #[test]
    fn multiple_advances_accumulate() {
        let mut clock = VirtualClock::new();
        clock.advance(Duration::from_secs(3));
        clock.advance(Duration::from_millis(500));
        clock.advance(Duration::from_micros(100));
        assert_eq!(
            clock.elapsed,
            Duration::from_secs(3) + Duration::from_millis(500) + Duration::from_micros(100)
        );
    }

    #[test]
    fn advance_with_zero_is_noop() {
        let mut clock = VirtualClock::new();
        clock.advance(Duration::ZERO);
        assert_eq!(clock.elapsed, Duration::ZERO);
    }

    #[test]
    fn now_returns_elapsed() {
        let mut clock = VirtualClock::new();
        clock.advance(Duration::from_millis(250));
        assert_eq!(clock.now(), Duration::from_millis(250));
    }

    #[test]
    fn elapsed_secs_is_float_seconds() {
        let mut clock = VirtualClock::new();
        clock.advance(Duration::from_millis(1500));
        assert!((clock.elapsed_secs() - 1.5).abs() < 1e-9);
    }

    #[test]
    fn elapsed_secs_zero_at_start() {
        let clock = VirtualClock::new();
        assert_eq!(clock.elapsed_secs(), 0.0);
    }

    #[test]
    fn elapsed_millis_truncates() {
        let mut clock = VirtualClock::new();
        clock.advance(Duration::from_micros(16_666));
        assert_eq!(clock.elapsed_millis(), 16);
    }

    #[test]
    fn reset_zeroes_elapsed() {
        let mut clock = VirtualClock::new();
        clock.advance(Duration::from_secs(99));
        clock.reset();
        assert_eq!(clock.elapsed, Duration::ZERO);
        assert_eq!(clock.now(), Duration::ZERO);
        assert_eq!(clock.elapsed_secs(), 0.0);
        assert_eq!(clock.elapsed_millis(), 0);
    }

    #[test]
    fn step_60fps_accumulates() {
        let mut clock = VirtualClock::new();
        for _ in 0..60 {
            clock.step_60fps();
        }
        // 60 frames at ~16.666ms = ~1 second.
        assert!((clock.elapsed_secs() - 1.0).abs() < 1e-6);
    }

    #[test]
    fn step_30fps_accumulates() {
        let mut clock = VirtualClock::new();
        for _ in 0..30 {
            clock.step_30fps();
        }
        // 30 frames at ~33.333ms = ~1 second.
        assert!((clock.elapsed_secs() - 1.0).abs() < 1e-6);
    }

    #[test]
    fn step_120fps_accumulates() {
        let mut clock = VirtualClock::new();
        for _ in 0..120 {
            clock.step_120fps();
        }
        // 120 frames at ~8.333ms = ~1 second.
        assert!((clock.elapsed_secs() - 1.0).abs() < 1e-6);
    }

    #[test]
    fn step_120_is_half_of_60() {
        let mut a = VirtualClock::new();
        let mut b = VirtualClock::new();
        a.step_60fps();
        b.step_120fps();
        b.step_120fps();
        assert_eq!(a.elapsed, b.elapsed);
    }

    #[test]
    fn step_30_is_double_of_60() {
        let mut a = VirtualClock::new();
        let mut b = VirtualClock::new();
        a.step_30fps();
        b.step_60fps();
        b.step_60fps();
        assert_eq!(a.elapsed, b.elapsed);
    }

    #[test]
    fn clock_is_copy_and_eq() {
        let mut clock = VirtualClock::new();
        clock.advance(Duration::from_millis(10));
        let copy = clock;
        assert_eq!(clock, copy);
    }

    #[test]
    fn frame_constants_are_consistent() {
        assert_eq!(FRAME_60FPS * 2, FRAME_30FPS);
        assert_eq!(FRAME_120FPS * 2, FRAME_60FPS);
    }
}
