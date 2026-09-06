//! Headless CI mock testing harness.
#![forbid(unsafe_code)]

use std::time::Duration;

/// A deterministic, manually-advancing clock for headless CI tests.
#[derive(Default)]
pub struct VirtualClock {
    /// The total elapsed time accumulated by this virtual clock.
    pub elapsed: Duration,
}

impl VirtualClock {
    /// Creates a new [`VirtualClock`] initialized to zero elapsed time.
    pub fn new() -> Self {
        Self {
            elapsed: Duration::ZERO,
        }
    }
    /// Advances the clock's elapsed time by `dt`, simulating the passage of time.
    pub fn advance(&mut self, dt: Duration) {
        self.elapsed += dt;
    }
}

#[cfg(test)]
mod tests {
    use super::VirtualClock;
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
}
