//! Headless CI mock testing harness.
#![forbid(unsafe_code)]

use std::time::Duration;

#[derive(Default)]
pub struct VirtualClock {
    pub elapsed: Duration,
}

impl VirtualClock {
    pub fn new() -> Self {
        Self {
            elapsed: Duration::ZERO,
        }
    }
    pub fn advance(&mut self, dt: Duration) {
        self.elapsed += dt;
    }
}
