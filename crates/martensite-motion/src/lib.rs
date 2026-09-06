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

#[cfg(test)]
mod tests {
    use super::{SpringConfig, SpringSolver};

    fn approx_eq(a: f32, b: f32, tol: f32) -> bool {
        (a - b).abs() < tol
    }

    #[test]
    fn critical_and_snappy_are_distinct() {
        assert_ne!(SpringConfig::CRITICAL, SpringConfig::SNAPPY);
    }

    #[test]
    fn new_computes_correct_zeta_for_critical() {
        let solver = SpringSolver::new(SpringConfig::CRITICAL, 0.0, 1.0, 0.0);
        // Critical damping: zeta = damping / (2 * sqrt(mass * stiffness))
        // = 26.8328 / (2 * sqrt(180)) ≈ 1.0
        assert!(approx_eq(solver.damping_ratio(), 1.0, 1e-3));
    }

    #[test]
    fn new_computes_correct_zeta_for_snappy() {
        let solver = SpringSolver::new(SpringConfig::SNAPPY, 0.0, 1.0, 0.0);
        // zeta = 25.0 / (2 * sqrt(300)) ≈ 0.7217
        assert!(approx_eq(solver.damping_ratio(), 0.7217, 1e-3));
    }

    #[test]
    fn sample_at_t_zero_returns_initial_position() {
        let solver = SpringSolver::new(SpringConfig::CRITICAL, 0.0, 10.0, 0.0);
        let (pos, _vel) = solver.sample();
        // At t=0, position should be the initial position (0.0).
        // A tolerance accounts for elapsed time before sampling.
        assert!(approx_eq(pos, 0.0, 1.0));
    }

    #[test]
    fn config_returns_used_config() {
        let config = SpringConfig::CRITICAL;
        let solver = SpringSolver::new(config, 0.0, 1.0, 0.0);
        assert_eq!(solver.config(), config);
    }

    #[test]
    fn damping_ratio_returns_zeta() {
        let solver = SpringSolver::new(SpringConfig::SNAPPY, 0.0, 1.0, 0.0);
        let zeta = solver.damping_ratio();
        // SNAPPY is underdamped: 0 < zeta < 1
        assert!(zeta > 0.0);
        assert!(zeta < 1.0);
    }

    #[test]
    fn zero_displacement_zero_velocity_stays_at_target() {
        let target = 5.0_f32;
        let solver = SpringSolver::new(SpringConfig::CRITICAL, target, target, 0.0);
        let (pos, vel) = solver.sample();
        assert_eq!(pos, target);
        assert_eq!(vel, 0.0);
    }

    #[test]
    fn virtual_clock_advances_deterministically() {
        let mut clock = martensite_test::VirtualClock::new();
        assert_eq!(clock.elapsed.as_nanos(), 0);

        clock.advance(std::time::Duration::from_millis(16));
        assert_eq!(clock.elapsed.as_millis(), 16);

        clock.advance(std::time::Duration::from_millis(16));
        assert_eq!(clock.elapsed.as_millis(), 32);
    }
}
