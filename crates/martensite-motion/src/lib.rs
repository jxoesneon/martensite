//! Analytical closed-form spring motion.
#![forbid(unsafe_code)]

use std::time::Instant;

/// Physical parameters describing a damped spring system.
#[derive(Debug, Copy, Clone, PartialEq)]
pub struct SpringConfig {
    /// Mass attached to the spring.
    pub mass: f32,
    /// Spring stiffness coefficient (Hooke's constant).
    pub stiffness: f32,
    /// Damping coefficient controlling velocity decay.
    pub damping: f32,
}

impl SpringConfig {
    /// A critically-damped spring preset with a smooth, non-overshooting response.
    pub const CRITICAL: Self = Self {
        mass: 1.0,
        stiffness: 180.0,
        damping: 26.8328,
    };
    /// A snappy spring preset with a quick, slightly underdamped response.
    pub const SNAPPY: Self = Self {
        mass: 1.0,
        stiffness: 300.0,
        damping: 25.0,
    };
}

/// Analytical solver for a damped spring motion towards a target value.
pub struct SpringSolver {
    config: SpringConfig,
    start_time: Instant,
    x0: f32,
    v0: f32,
    target: f32,
    omega0: f32,
    zeta: f32,
}

impl SpringSolver {
    /// Creates a new solver moving from `initial` towards `target` with the given `initial_vel`.
    pub fn new(config: SpringConfig, initial: f32, target: f32, initial_vel: f32) -> Self {
        let omega0 = (config.stiffness / config.mass).sqrt();
        let zeta = config.damping / (2.0 * (config.mass * config.stiffness).sqrt());
        Self {
            config,
            start_time: Instant::now(),
            x0: initial - target,
            v0: initial_vel,
            target,
            omega0,
            zeta,
        }
    }

    /// Samples the current position and velocity of the spring at the elapsed time.
    ///
    /// Returns a tuple of `(position, velocity)`.
    pub fn sample(&self) -> (f32, f32) {
        let t = self.start_time.elapsed().as_secs_f32();
        let decay = (-self.omega0 * t).exp();
        let c1 = self.x0;
        let c2 = self.v0 + self.omega0 * self.x0;
        let x = decay * (c1 + c2 * t);
        let v = decay * (c2 - self.omega0 * (c1 + c2 * t));
        (self.target + x, v)
    }

    /// Returns the spring configuration used by this solver.
    #[inline]
    pub fn config(&self) -> SpringConfig {
        self.config
    }

    /// Returns the damping ratio (zeta) of the spring system.
    #[inline]
    pub fn damping_ratio(&self) -> f32 {
        self.zeta
    }
}
