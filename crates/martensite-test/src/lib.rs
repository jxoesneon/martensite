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
