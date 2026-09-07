//! Closed-form analytical damped harmonic oscillator spring solver.
//!
//! This module solves the continuous second-order linear ODE
//!
//! `m · d²x/dt² + c · dx/dt + k · x = 0`
//!
//! in closed form for all three damping regimes (underdamped, critically
//! damped, and overdamped). Rather than integrating numerically per frame —
//! which drifts or explodes under variable frame rates — the position and
//! velocity are evaluated directly from the analytical solution at any time
//! `t`, yielding frame-rate independent, energy-conserving motion with
//! exact C¹ velocity continuity across interruptions.

use core::fmt;

/// Tolerance used to classify the damping ratio as exactly critical.
///
/// Floating-point rounding makes `ζ = 1` unreliable to test with strict
/// equality, so values within this band of `1.0` are treated as critical.
const CRITICAL_ZETA_EPS: f32 = 1e-5;

/// Displacement/velocity magnitude below which the spring is considered
/// numerically settled and snaps exactly to the target.
const SETTLE_EPS: f32 = 1e-4;

/// Error returned when [`SpringConfig`] is constructed with invalid parameters.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SpringConfigError {
    /// The mass coefficient must be strictly positive and finite.
    MassMustBePositive,
    /// The stiffness coefficient must be strictly positive and finite.
    StiffnessMustBePositive,
    /// The damping coefficient must be non-negative and finite.
    DampingMustBeNonNegative,
}

impl fmt::Display for SpringConfigError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::MassMustBePositive => {
                f.write_str("spring mass must be strictly positive and finite")
            }
            Self::StiffnessMustBePositive => {
                f.write_str("spring stiffness must be strictly positive and finite")
            }
            Self::DampingMustBeNonNegative => {
                f.write_str("spring damping must be non-negative and finite")
            }
        }
    }
}

impl std::error::Error for SpringConfigError {}

/// The qualitative damping behaviour of a spring system.
///
/// Returned by [`SpringConfig::damping_regime`] and determined by the damping
/// ratio `ζ = c / (2√(km))`.
///
/// # Examples
///
/// ```
/// use martensite_motion::{DampingRegime, SpringConfig};
///
/// let underdamped = SpringConfig::new(1.0, 100.0, 10.0).unwrap();
/// assert_eq!(underdamped.damping_regime(), DampingRegime::Underdamped);
///
/// let overdamped = SpringConfig::new(1.0, 100.0, 100.0).unwrap();
/// assert_eq!(overdamped.damping_regime(), DampingRegime::Overdamped);
/// ```
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DampingRegime {
    /// `ζ < 1` — the system oscillates with exponentially decaying amplitude.
    Underdamped,
    /// `ζ = 1` — the system returns to equilibrium as fast as possible without oscillating.
    Critical,
    /// `ζ > 1` — the system decays to equilibrium monotonically without oscillation.
    Overdamped,
}

/// Physical parameters describing a damped spring system.
///
/// The continuous equation of motion is `m · ẍ + c · ẋ + k · x = 0`, where
/// `m` is [`mass`](Self::mass), `c` is [`damping`](Self::damping), and `k` is
/// [`stiffness`](Self::stiffness).
///
/// # Examples
///
/// ```
/// use martensite_motion::SpringConfig;
///
/// let config = SpringConfig::new(1.0, 200.0, 20.0).unwrap();
/// assert!(config.zeta() < 1.0);
/// ```
#[derive(Debug, Copy, Clone, PartialEq)]
pub struct SpringConfig {
    /// Mass attached to the spring (`m > 0`).
    mass: f32,
    /// Spring stiffness coefficient, Hooke's constant (`k > 0`).
    stiffness: f32,
    /// Damping coefficient controlling velocity decay (`c >= 0`).
    damping: f32,
}

impl SpringConfig {
    /// Returns the mass coefficient (`m > 0`).
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite_motion::SpringConfig;
    ///
    /// let config = SpringConfig::new(2.0, 200.0, 20.0).unwrap();
    /// assert_eq!(config.mass(), 2.0);
    /// ```
    #[inline]
    pub const fn mass(&self) -> f32 {
        self.mass
    }

    /// Returns the spring stiffness coefficient (`k > 0`).
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite_motion::SpringConfig;
    ///
    /// let config = SpringConfig::new(1.0, 300.0, 20.0).unwrap();
    /// assert_eq!(config.stiffness(), 300.0);
    /// ```
    #[inline]
    pub const fn stiffness(&self) -> f32 {
        self.stiffness
    }

    /// Returns the damping coefficient (`c >= 0`).
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite_motion::SpringConfig;
    ///
    /// let config = SpringConfig::new(1.0, 200.0, 15.0).unwrap();
    /// assert_eq!(config.damping(), 15.0);
    /// ```
    #[inline]
    pub const fn damping(&self) -> f32 {
        self.damping
    }
}

impl SpringConfig {
    /// A critically-damped preset: smooth, non-overshooting response.
    ///
    /// `ζ ≈ 1.0` with `ω₀ = √180 ≈ 13.42 rad/s`.
    pub const CRITICAL: Self = Self {
        mass: 1.0,
        stiffness: 180.0,
        damping: 26.8328,
    };

    /// A snappy preset: quick, slightly underdamped response with a small overshoot.
    ///
    /// `ζ ≈ 0.72` with `ω₀ = √300 ≈ 17.32 rad/s`.
    pub const SNAPPY: Self = Self {
        mass: 1.0,
        stiffness: 300.0,
        damping: 25.0,
    };

    /// A gentle preset: slow, soft motion suitable for large or subtle transitions.
    ///
    /// `ζ ≈ 0.77` with `ω₀ = √60 ≈ 7.75 rad/s`.
    pub const GENTLE: Self = Self {
        mass: 1.0,
        stiffness: 60.0,
        damping: 12.0,
    };

    /// A bouncy preset: pronounced underdamped oscillation with visible overshoot.
    ///
    /// `ζ ≈ 0.35` with `ω₀ = √200 ≈ 14.14 rad/s`.
    pub const BOUNCY: Self = Self {
        mass: 1.0,
        stiffness: 200.0,
        damping: 10.0,
    };

    /// Creates a validated spring configuration.
    ///
    /// Returns an error if `mass` is not strictly positive, `stiffness` is not
    /// strictly positive, `damping` is negative, or any value is non-finite.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite_motion::{SpringConfig, SpringConfigError};
    ///
    /// assert!(SpringConfig::new(1.0, 200.0, 20.0).is_ok());
    /// assert_eq!(
    ///     SpringConfig::new(0.0, 200.0, 20.0).unwrap_err(),
    ///     SpringConfigError::MassMustBePositive
    /// );
    /// ```
    pub fn new(mass: f32, stiffness: f32, damping: f32) -> Result<Self, SpringConfigError> {
        if !mass.is_finite() || mass <= 0.0 {
            return Err(SpringConfigError::MassMustBePositive);
        }
        if !stiffness.is_finite() || stiffness <= 0.0 {
            return Err(SpringConfigError::StiffnessMustBePositive);
        }
        if !damping.is_finite() || damping < 0.0 {
            return Err(SpringConfigError::DampingMustBeNonNegative);
        }
        Ok(Self {
            mass,
            stiffness,
            damping,
        })
    }

    /// Returns the natural angular frequency `ω₀ = √(k/m)`.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite_motion::SpringConfig;
    ///
    /// let config = SpringConfig::new(1.0, 4.0, 4.0).unwrap();
    /// assert!((config.omega0() - 2.0).abs() < 1e-6);
    /// ```
    #[inline]
    pub fn omega0(&self) -> f32 {
        (self.stiffness / self.mass).sqrt()
    }

    /// Returns the damping ratio `ζ = c / (2√(km))`.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite_motion::SpringConfig;
    ///
    /// let config = SpringConfig::new(1.0, 4.0, 4.0).unwrap();
    /// assert!((config.zeta() - 1.0).abs() < 1e-6);
    /// ```
    #[inline]
    pub fn zeta(&self) -> f32 {
        self.damping / (2.0 * (self.mass * self.stiffness).sqrt())
    }

    /// Returns the damping regime classification for this configuration.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite_motion::{DampingRegime, SpringConfig};
    ///
    /// let config = SpringConfig::new(1.0, 4.0, 4.0).unwrap();
    /// assert_eq!(config.damping_regime(), DampingRegime::Critical);
    /// ```
    #[inline]
    pub fn damping_regime(&self) -> DampingRegime {
        let zeta = self.zeta();
        if (zeta - 1.0).abs() < CRITICAL_ZETA_EPS {
            DampingRegime::Critical
        } else if zeta < 1.0 {
            DampingRegime::Underdamped
        } else {
            DampingRegime::Overdamped
        }
    }
}

/// Precomputed closed-form coefficients for a single damping regime.
///
/// These are derived once from the initial conditions and reused for every
/// [`SpringSolver::sample_at`] evaluation, keeping per-sample work branch-free
/// aside from the regime dispatch.
#[derive(Debug, Clone, Copy)]
enum Coeffs {
    /// `ζ < 1`: `x(t) = e^(-ζω₀t)(c₁ cos(ωd t) + c₂ sin(ωd t))`.
    Underdamped {
        /// Damped angular frequency `ωd = ω₀√(1-ζ²)`.
        omega_d: f32,
        /// Cosine coefficient (`= x₀`).
        c1: f32,
        /// Sine coefficient (`= (v₀ + ζω₀x₀)/ωd`).
        c2: f32,
    },
    /// `ζ = 1`: `x(t) = e^(-ω₀t)(c₁ + c₂ t)`.
    Critical {
        /// Constant coefficient (`= x₀`).
        c1: f32,
        /// Linear coefficient (`= v₀ + ω₀x₀`).
        c2: f32,
    },
    /// `ζ > 1`: `x(t) = c₁ e^(r₁ t) + c₂ e^(r₂ t)`.
    Overdamped {
        /// Slow decay rate `r₁ = (-ζ + √(ζ²-1))ω₀`.
        r1: f32,
        /// Fast decay rate `r₂ = (-ζ - √(ζ²-1))ω₀`.
        r2: f32,
        /// Weight of the `r₁` exponential.
        c1: f32,
        /// Weight of the `r₂` exponential.
        c2: f32,
    },
}

impl Coeffs {
    /// Computes the closed-form coefficients for the given parameters.
    ///
    /// `omega0` is the natural frequency, `zeta` the damping ratio, `x0` the
    /// initial displacement from target, and `v0` the initial velocity.
    fn compute(omega0: f32, zeta: f32, x0: f32, v0: f32) -> Self {
        if (zeta - 1.0).abs() < CRITICAL_ZETA_EPS {
            let c1 = x0;
            let c2 = v0 + omega0 * x0;
            Self::Critical { c1, c2 }
        } else if zeta < 1.0 {
            let omega_d = omega0 * (1.0 - zeta * zeta).sqrt();
            let c1 = x0;
            let c2 = (v0 + zeta * omega0 * x0) / omega_d;
            Self::Underdamped { omega_d, c1, c2 }
        } else {
            let root = (zeta * zeta - 1.0).sqrt();
            let r1 = (-zeta + root) * omega0;
            let r2 = (-zeta - root) * omega0;
            let denom = r1 - r2;
            let c1 = (v0 - r2 * x0) / denom;
            let c2 = (r1 * x0 - v0) / denom;
            Self::Overdamped { r1, r2, c1, c2 }
        }
    }
}

/// Analytical closed-form solver for a damped spring moving towards a target.
///
/// The solver tracks displacement `x = position - target` and evaluates the
/// exact analytical solution at any time `t` via [`SpringSolver::sample_at`].
/// A deterministic internal clock (`t`) advanced by [`SpringSolver::advance`]
/// makes the motion fully reproducible and testable, independent of wall-clock
/// timing.
///
/// # Examples
///
/// ```
/// use martensite_motion::{SpringConfig, SpringSolver};
///
/// let config = SpringConfig::CRITICAL;
/// let mut solver = SpringSolver::new(config, 0.0, 10.0, 0.0);
///
/// // At t = 0 the spring is exactly at the initial position.
/// let (pos, _vel) = solver.sample();
/// assert_eq!(pos, 0.0);
///
/// // Advance the deterministic clock and sample again.
/// solver.advance(0.1);
/// let (pos, vel) = solver.sample();
/// assert!(pos > 0.0 && pos < 10.0);
/// assert!(vel > 0.0);
/// ```
#[derive(Debug, Clone, Copy)]
pub struct SpringSolver {
    /// Validated spring configuration.
    config: SpringConfig,
    /// Initial displacement from target, `x₀ = initial - target`.
    x0: f32,
    /// Initial velocity `v₀`.
    v0: f32,
    /// Target position the spring converges towards.
    target: f32,
    /// Deterministic elapsed time, advanced by [`SpringSolver::advance`].
    t: f32,
    /// Natural angular frequency `ω₀`.
    omega0: f32,
    /// Damping ratio `ζ`.
    zeta: f32,
    /// Precomputed closed-form coefficients for the active regime.
    coeffs: Coeffs,
}

impl SpringSolver {
    /// Creates a new solver moving from `initial` towards `target` with the
    /// given `initial_vel`.
    ///
    /// The internal clock starts at `t = 0`. Non-finite `initial`, `target`, or
    /// `initial_vel` values are handled safely: non-finite values are replaced
    /// with zeros so the solver always produces finite output. Specifically, a
    /// non-finite `target` is snapped to `0.0`, a non-finite `initial` is
    /// snapped to the (sanitized) target, and a non-finite `initial_vel` is
    /// snapped to `0.0`.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite_motion::{SpringConfig, SpringSolver};
    ///
    /// let solver = SpringSolver::new(SpringConfig::CRITICAL, 0.0, 10.0, 0.0);
    /// assert_eq!(solver.position(), 0.0);
    /// assert_eq!(solver.target(), 10.0);
    /// assert_eq!(solver.elapsed(), 0.0);
    /// ```
    pub fn new(config: SpringConfig, initial: f32, target: f32, initial_vel: f32) -> Self {
        // Sanitize non-finite inputs so the solver never produces NaN/Inf.
        let target = if target.is_finite() { target } else { 0.0 };
        let initial = if initial.is_finite() { initial } else { target };
        let initial_vel = if initial_vel.is_finite() {
            initial_vel
        } else {
            0.0
        };

        let omega0 = config.omega0();
        let zeta = config.zeta();
        let x0 = initial - target;
        let v0 = initial_vel;
        let coeffs = Coeffs::compute(omega0, zeta, x0, v0);
        Self {
            config,
            x0,
            v0,
            target,
            t: 0.0,
            omega0,
            zeta,
            coeffs,
        }
    }

    /// Samples the position and velocity of the spring at an arbitrary time `t`.
    ///
    /// Returns `(position, velocity)` where `position = target + x(t)`. This is
    /// the core evaluation method and handles all three damping regimes using
    /// the precomputed closed-form coefficients.
    ///
    /// When the displacement and velocity both fall below the settle threshold
    /// (`1e-4`), the result snaps exactly to `(target, 0.0)`. If any computed
    /// value is non-finite, the same safe fallback is returned.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite_motion::{SpringConfig, SpringSolver};
    ///
    /// let solver = SpringSolver::new(SpringConfig::CRITICAL, 0.0, 10.0, 0.0);
    /// let (pos, vel) = solver.sample_at(0.0);
    /// assert_eq!(pos, 0.0);
    /// assert_eq!(vel, 0.0);
    /// ```
    pub fn sample_at(&self, t: f32) -> (f32, f32) {
        if !t.is_finite() {
            let target = if self.target.is_finite() {
                self.target
            } else {
                0.0
            };
            return (target, 0.0);
        }

        let (x, v) = match self.coeffs {
            Coeffs::Underdamped { omega_d, c1, c2 } => {
                let decay = (-self.zeta * self.omega0 * t).exp();
                let angle = omega_d * t;
                let cos = angle.cos();
                let sin = angle.sin();
                let x = decay * (c1 * cos + c2 * sin);
                let v = decay
                    * ((c2 * omega_d - c1 * self.zeta * self.omega0) * cos
                        - (c1 * omega_d + c2 * self.zeta * self.omega0) * sin);
                (x, v)
            }
            Coeffs::Critical { c1, c2 } => {
                let decay = (-self.omega0 * t).exp();
                let x = decay * (c1 + c2 * t);
                let v = decay * (c2 - self.omega0 * (c1 + c2 * t));
                (x, v)
            }
            Coeffs::Overdamped { r1, r2, c1, c2 } => {
                let e1 = (r1 * t).exp();
                let e2 = (r2 * t).exp();
                let x = c1 * e1 + c2 * e2;
                let v = c1 * r1 * e1 + c2 * r2 * e2;
                (x, v)
            }
        };

        if !x.is_finite() || !v.is_finite() {
            return (self.target, 0.0);
        }

        if x.abs() < SETTLE_EPS && v.abs() < SETTLE_EPS {
            (self.target, 0.0)
        } else {
            (self.target + x, v)
        }
    }

    /// Samples the position and velocity at the current elapsed time.
    ///
    /// Returns `(position, velocity)`. Equivalent to
    /// [`SpringSolver::sample_at`] with the internal clock value.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite_motion::{SpringConfig, SpringSolver};
    ///
    /// let solver = SpringSolver::new(SpringConfig::CRITICAL, 0.0, 10.0, 0.0);
    /// let (pos, _vel) = solver.sample();
    /// assert_eq!(pos, 0.0);
    /// ```
    #[inline]
    pub fn sample(&self) -> (f32, f32) {
        self.sample_at(self.t)
    }

    /// Advances the deterministic internal clock by `dt` seconds.
    ///
    /// Non-finite or negative `dt` values are ignored so the clock cannot be
    /// corrupted or run backwards, which would produce non-physical motion.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite_motion::{SpringConfig, SpringSolver};
    ///
    /// let mut solver = SpringSolver::new(SpringConfig::CRITICAL, 0.0, 10.0, 0.0);
    /// solver.advance(0.5);
    /// assert_eq!(solver.elapsed(), 0.5);
    /// ```
    #[inline]
    pub fn advance(&mut self, dt: f32) {
        if dt.is_finite() && dt >= 0.0 {
            self.t += dt;
        }
    }

    /// Returns `true` once the spring has numerically settled on the target.
    ///
    /// Settled means `|position - target| < 1e-4` and `|velocity| < 1e-4` at
    /// the current elapsed time.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite_motion::{SpringConfig, SpringSolver};
    ///
    /// let mut solver = SpringSolver::new(SpringConfig::CRITICAL, 0.0, 10.0, 0.0);
    /// assert!(!solver.settle_threshold());
    /// solver.advance(60.0);
    /// assert!(solver.settle_threshold());
    /// ```
    pub fn settle_threshold(&self) -> bool {
        let (pos, vel) = self.sample();
        (pos - self.target).abs() < SETTLE_EPS && vel.abs() < SETTLE_EPS
    }

    /// Interrupts the current motion with a new target, preserving C¹
    /// velocity continuity.
    ///
    /// The current position and velocity are sampled, then the initial
    /// conditions are reset so that `x₀ = current_position - new_target` and
    /// `v₀ = current_velocity`. The internal clock is reset to `t = 0`. This
    /// guarantees no position jump and no acceleration discontinuity when a
    /// spring is retargeted mid-motion.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite_motion::{SpringConfig, SpringSolver};
    ///
    /// let mut solver = SpringSolver::new(SpringConfig::CRITICAL, 0.0, 10.0, 0.0);
    /// solver.advance(0.2);
    /// let (pos, vel) = solver.sample();
    /// solver.interrupt(-5.0);
    /// // Position and velocity are preserved across the interruption.
    /// assert_eq!(solver.position(), pos);
    /// assert!((solver.velocity() - vel).abs() < 1e-5);
    /// assert_eq!(solver.target(), -5.0);
    /// ```
    pub fn interrupt(&mut self, new_target: f32) {
        // Sanitize non-finite target to prevent NaN/Inf propagation.
        let new_target = if new_target.is_finite() {
            new_target
        } else {
            0.0
        };
        let (pos, vel) = self.sample();
        self.target = new_target;
        self.x0 = pos - new_target;
        self.v0 = vel;
        self.t = 0.0;
        self.coeffs = Coeffs::compute(self.omega0, self.zeta, self.x0, self.v0);
    }

    /// Returns the current position at the elapsed time.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite_motion::{SpringConfig, SpringSolver};
    ///
    /// let solver = SpringSolver::new(SpringConfig::CRITICAL, 3.0, 10.0, 0.0);
    /// assert_eq!(solver.position(), 3.0);
    /// ```
    #[inline]
    pub fn position(&self) -> f32 {
        self.sample().0
    }

    /// Returns the current velocity at the elapsed time.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite_motion::{SpringConfig, SpringSolver};
    ///
    /// let solver = SpringSolver::new(SpringConfig::CRITICAL, 0.0, 10.0, 5.0);
    /// assert_eq!(solver.velocity(), 5.0);
    /// ```
    #[inline]
    pub fn velocity(&self) -> f32 {
        self.sample().1
    }

    /// Returns the target position the spring is converging towards.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite_motion::{SpringConfig, SpringSolver};
    ///
    /// let solver = SpringSolver::new(SpringConfig::CRITICAL, 0.0, 42.0, 0.0);
    /// assert_eq!(solver.target(), 42.0);
    /// ```
    #[inline]
    pub fn target(&self) -> f32 {
        self.target
    }

    /// Returns the deterministic elapsed time in seconds.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite_motion::{SpringConfig, SpringSolver};
    ///
    /// let mut solver = SpringSolver::new(SpringConfig::CRITICAL, 0.0, 10.0, 0.0);
    /// solver.advance(1.25);
    /// assert_eq!(solver.elapsed(), 1.25);
    /// ```
    #[inline]
    pub fn elapsed(&self) -> f32 {
        self.t
    }

    /// Returns the spring configuration used by this solver.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite_motion::{SpringConfig, SpringSolver};
    ///
    /// let solver = SpringSolver::new(SpringConfig::SNAPPY, 0.0, 1.0, 0.0);
    /// assert_eq!(solver.config(), SpringConfig::SNAPPY);
    /// ```
    #[inline]
    pub fn config(&self) -> SpringConfig {
        self.config
    }

    /// Returns the damping ratio `ζ` of the spring system.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite_motion::{SpringConfig, SpringSolver};
    ///
    /// let solver = SpringSolver::new(SpringConfig::CRITICAL, 0.0, 1.0, 0.0);
    /// assert!((solver.damping_ratio() - 1.0).abs() < 1e-3);
    /// ```
    #[inline]
    pub fn damping_ratio(&self) -> f32 {
        self.zeta
    }
}

#[cfg(test)]
mod tests {
    use super::{DampingRegime, SpringConfig, SpringConfigError, SpringSolver};

    /// Helper: floating-point approximate equality within `tol`.
    fn approx_eq(a: f32, b: f32, tol: f32) -> bool {
        (a - b).abs() < tol
    }

    /// Independent f64 re-implementation of the analytical solution, used to
    /// validate [`SpringSolver::sample_at`] against the documented formulae.
    fn analytical(config: SpringConfig, x0: f64, v0: f64, t: f64) -> (f64, f64) {
        let omega0 = (f64::from(config.stiffness) / f64::from(config.mass)).sqrt();
        let zeta = f64::from(config.damping)
            / (2.0 * (f64::from(config.mass) * f64::from(config.stiffness)).sqrt());

        if (zeta - 1.0).abs() < 1e-5 {
            let c1 = x0;
            let c2 = v0 + omega0 * x0;
            let decay = (-omega0 * t).exp();
            let x = decay * (c1 + c2 * t);
            let v = decay * (c2 - omega0 * (c1 + c2 * t));
            (x, v)
        } else if zeta < 1.0 {
            let omega_d = omega0 * (1.0 - zeta * zeta).sqrt();
            let c1 = x0;
            let c2 = (v0 + zeta * omega0 * x0) / omega_d;
            let decay = (-zeta * omega0 * t).exp();
            let cos = (omega_d * t).cos();
            let sin = (omega_d * t).sin();
            let x = decay * (c1 * cos + c2 * sin);
            let v = decay
                * ((c2 * omega_d - c1 * zeta * omega0) * cos
                    - (c1 * omega_d + c2 * zeta * omega0) * sin);
            (x, v)
        } else {
            let root = (zeta * zeta - 1.0).sqrt();
            let r1 = (-zeta + root) * omega0;
            let r2 = (-zeta - root) * omega0;
            let denom = r1 - r2;
            let c1 = (v0 - r2 * x0) / denom;
            let c2 = (r1 * x0 - v0) / denom;
            let e1 = (r1 * t).exp();
            let e2 = (r2 * t).exp();
            let x = c1 * e1 + c2 * e2;
            let v = c1 * r1 * e1 + c2 * r2 * e2;
            (x, v)
        }
    }

    // ------------------------------------------------------------------
    // Config validation
    // ------------------------------------------------------------------

    #[test]
    fn config_rejects_zero_mass() {
        assert_eq!(
            SpringConfig::new(0.0, 200.0, 20.0).unwrap_err(),
            SpringConfigError::MassMustBePositive
        );
    }

    #[test]
    fn config_rejects_negative_mass() {
        assert_eq!(
            SpringConfig::new(-1.0, 200.0, 20.0).unwrap_err(),
            SpringConfigError::MassMustBePositive
        );
    }

    #[test]
    fn config_rejects_zero_stiffness() {
        assert_eq!(
            SpringConfig::new(1.0, 0.0, 20.0).unwrap_err(),
            SpringConfigError::StiffnessMustBePositive
        );
    }

    #[test]
    fn config_rejects_negative_stiffness() {
        assert_eq!(
            SpringConfig::new(1.0, -10.0, 20.0).unwrap_err(),
            SpringConfigError::StiffnessMustBePositive
        );
    }

    #[test]
    fn config_rejects_negative_damping() {
        assert_eq!(
            SpringConfig::new(1.0, 200.0, -1.0).unwrap_err(),
            SpringConfigError::DampingMustBeNonNegative
        );
    }

    #[test]
    fn config_allows_zero_damping() {
        assert!(SpringConfig::new(1.0, 200.0, 0.0).is_ok());
    }

    #[test]
    fn config_rejects_nan_inputs() {
        assert_eq!(
            SpringConfig::new(f32::NAN, 200.0, 20.0).unwrap_err(),
            SpringConfigError::MassMustBePositive
        );
        assert_eq!(
            SpringConfig::new(1.0, f32::INFINITY, 20.0).unwrap_err(),
            SpringConfigError::StiffnessMustBePositive
        );
        assert_eq!(
            SpringConfig::new(1.0, 200.0, f32::NAN).unwrap_err(),
            SpringConfigError::DampingMustBeNonNegative
        );
    }

    #[test]
    fn config_presets_are_distinct_and_valid() {
        assert!(SpringConfig::new(
            SpringConfig::CRITICAL.mass(),
            SpringConfig::CRITICAL.stiffness(),
            SpringConfig::CRITICAL.damping()
        )
        .is_ok());
        assert_ne!(SpringConfig::CRITICAL, SpringConfig::SNAPPY);
        assert_ne!(SpringConfig::GENTLE, SpringConfig::BOUNCY);
    }

    #[test]
    fn config_critical_preset_is_critical_regime() {
        assert_eq!(
            SpringConfig::CRITICAL.damping_regime(),
            DampingRegime::Critical
        );
    }

    #[test]
    fn config_snappy_preset_is_underdamped() {
        assert_eq!(
            SpringConfig::SNAPPY.damping_regime(),
            DampingRegime::Underdamped
        );
    }

    #[test]
    fn config_bouncy_preset_is_underdamped() {
        assert_eq!(
            SpringConfig::BOUNCY.damping_regime(),
            DampingRegime::Underdamped
        );
    }

    // ------------------------------------------------------------------
    // Initial conditions
    // ------------------------------------------------------------------

    #[test]
    fn position_at_t_zero_equals_initial() {
        let solver = SpringSolver::new(SpringConfig::CRITICAL, 7.0, 10.0, 3.0);
        let (pos, _vel) = solver.sample_at(0.0);
        assert!(approx_eq(pos, 7.0, 1e-6));
    }

    #[test]
    fn velocity_at_t_zero_equals_v0() {
        let solver = SpringSolver::new(SpringConfig::CRITICAL, 7.0, 10.0, 3.0);
        let (_pos, vel) = solver.sample_at(0.0);
        assert!(approx_eq(vel, 3.0, 1e-6));
    }

    #[test]
    fn initial_conditions_hold_for_underdamped() {
        let config = SpringConfig::new(1.0, 25.0, 6.0).unwrap();
        let solver = SpringSolver::new(config, 2.0, 5.0, -1.5);
        let (pos, vel) = solver.sample_at(0.0);
        assert!(approx_eq(pos, 2.0, 1e-6));
        assert!(approx_eq(vel, -1.5, 1e-6));
    }

    #[test]
    fn initial_conditions_hold_for_overdamped() {
        let config = SpringConfig::new(1.0, 4.0, 10.0).unwrap();
        let solver = SpringSolver::new(config, 2.0, 5.0, -1.5);
        let (pos, vel) = solver.sample_at(0.0);
        assert!(approx_eq(pos, 2.0, 1e-6));
        assert!(approx_eq(vel, -1.5, 1e-6));
    }

    // ------------------------------------------------------------------
    // Analytical accuracy (all three regimes)
    // ------------------------------------------------------------------

    #[test]
    fn underdamped_matches_analytical_solution() {
        // zeta = 6 / (2 * 5) = 0.6, omega0 = 5, omega_d = 4.
        let config = SpringConfig::new(1.0, 25.0, 6.0).unwrap();
        let initial = 1.0_f32;
        let target = 0.0_f32;
        let v0 = 0.5_f32;
        let x0 = f64::from(initial - target);
        let solver = SpringSolver::new(config, initial, target, v0);

        for t in [0.0_f32, 0.1, 0.3, 0.7, 1.3, 2.0] {
            let (pos, vel) = solver.sample_at(t);
            let (ex_x, ex_v) = analytical(config, x0, f64::from(v0), f64::from(t));
            assert!(
                approx_eq(pos, target + ex_x as f32, 1e-6),
                "underdamped pos at t={t}: {pos} vs {ex_x}"
            );
            assert!(
                approx_eq(vel, ex_v as f32, 1e-6),
                "underdamped vel at t={t}: {vel} vs {ex_v}"
            );
        }
    }

    #[test]
    fn critical_matches_analytical_solution() {
        // zeta = 4 / (2 * 2) = 1.0, omega0 = 2.
        let config = SpringConfig::new(1.0, 4.0, 4.0).unwrap();
        let initial = 1.0_f32;
        let target = 0.0_f32;
        let v0 = 0.5_f32;
        let x0 = f64::from(initial - target);
        let solver = SpringSolver::new(config, initial, target, v0);

        for t in [0.0_f32, 0.1, 0.3, 0.5, 1.0, 2.0] {
            let (pos, vel) = solver.sample_at(t);
            let (ex_x, ex_v) = analytical(config, x0, f64::from(v0), f64::from(t));
            assert!(
                approx_eq(pos, target + ex_x as f32, 1e-6),
                "critical pos at t={t}: {pos} vs {ex_x}"
            );
            assert!(
                approx_eq(vel, ex_v as f32, 1e-6),
                "critical vel at t={t}: {vel} vs {ex_v}"
            );
        }
    }

    #[test]
    fn critical_matches_known_hand_value() {
        // x(t) = e^{-2t}(1 + 2t); at t = 0.5 -> 2/e, v = -2/e.
        let config = SpringConfig::new(1.0, 4.0, 4.0).unwrap();
        let solver = SpringSolver::new(config, 1.0, 0.0, 0.0);
        let expected = 2.0_f32 / std::f32::consts::E;
        let (pos, vel) = solver.sample_at(0.5);
        assert!(approx_eq(pos, expected, 1e-6));
        assert!(approx_eq(vel, -expected, 1e-6));
    }

    #[test]
    fn overdamped_matches_analytical_solution() {
        // zeta = 10 / (2 * 2) = 2.5, omega0 = 2.
        let config = SpringConfig::new(1.0, 4.0, 10.0).unwrap();
        let initial = 1.0_f32;
        let target = 0.0_f32;
        let v0 = 0.5_f32;
        let x0 = f64::from(initial - target);
        let solver = SpringSolver::new(config, initial, target, v0);

        for t in [0.0_f32, 0.1, 0.3, 0.7, 1.3, 2.0] {
            let (pos, vel) = solver.sample_at(t);
            let (ex_x, ex_v) = analytical(config, x0, f64::from(v0), f64::from(t));
            assert!(
                approx_eq(pos, target + ex_x as f32, 1e-6),
                "overdamped pos at t={t}: {pos} vs {ex_x}"
            );
            assert!(
                approx_eq(vel, ex_v as f32, 1e-6),
                "overdamped vel at t={t}: {vel} vs {ex_v}"
            );
        }
    }

    // ------------------------------------------------------------------
    // Qualitative behaviour
    // ------------------------------------------------------------------

    #[test]
    fn underdamped_crosses_zero_multiple_times() {
        // zeta = 0.6, omega0 = 5, omega_d = 4 -> period 2pi/4 ≈ 1.57s.
        let config = SpringConfig::new(1.0, 25.0, 6.0).unwrap();
        let solver = SpringSolver::new(config, 1.0, 0.0, 0.0);

        let mut prev = solver.sample_at(0.0).0;
        let mut crossings = 0;
        let mut t = 0.05_f32;
        while t < 6.0 {
            let pos = solver.sample_at(t).0;
            if prev * pos < 0.0 {
                crossings += 1;
            }
            prev = pos;
            t += 0.05;
        }
        assert!(
            crossings >= 2,
            "expected multiple zero crossings, got {crossings}"
        );
    }

    #[test]
    fn critical_damping_has_no_overshoot() {
        let config = SpringConfig::new(1.0, 4.0, 4.0).unwrap();
        let solver = SpringSolver::new(config, 1.0, 0.0, 0.0);

        let mut t = 0.0_f32;
        while t < 10.0 {
            let pos = solver.sample_at(t).0;
            assert!(
                pos >= -1e-4,
                "critical spring overshot below target at t={t}: {pos}"
            );
            t += 0.05;
        }
    }

    #[test]
    fn overdamped_approaches_target_monotonically() {
        let config = SpringConfig::new(1.0, 4.0, 10.0).unwrap();
        let solver = SpringSolver::new(config, 1.0, 0.0, 0.0);

        let mut prev = solver.sample_at(0.0).0;
        let mut t = 0.05_f32;
        while t < 10.0 {
            let pos = solver.sample_at(t).0;
            // Monotonic decrease towards target (0.0), never overshooting.
            assert!(
                pos <= prev + 1e-6,
                "overdamped spring increased at t={t}: {pos} > {prev}"
            );
            assert!(pos >= -1e-4, "overdamped spring overshot at t={t}: {pos}");
            prev = pos;
            t += 0.05;
        }
    }

    // ------------------------------------------------------------------
    // Settle threshold
    // ------------------------------------------------------------------

    #[test]
    fn settle_threshold_false_at_start() {
        let solver = SpringSolver::new(SpringConfig::CRITICAL, 0.0, 10.0, 0.0);
        assert!(!solver.settle_threshold());
    }

    #[test]
    fn settle_threshold_true_after_convergence() {
        let mut solver = SpringSolver::new(SpringConfig::CRITICAL, 0.0, 10.0, 0.0);
        solver.advance(60.0);
        assert!(solver.settle_threshold());
        let (pos, vel) = solver.sample();
        assert_eq!(pos, 10.0);
        assert_eq!(vel, 0.0);
    }

    // ------------------------------------------------------------------
    // Interrupt / C¹ continuity
    // ------------------------------------------------------------------

    #[test]
    fn interrupt_preserves_position_and_velocity() {
        let config = SpringConfig::new(1.0, 25.0, 6.0).unwrap();
        let mut solver = SpringSolver::new(config, 0.0, 10.0, 0.0);
        solver.advance(0.3);

        let (pos_before, vel_before) = solver.sample();
        // Must be mid-motion for the test to be meaningful.
        assert!(pos_before > 0.0 && pos_before < 10.0);

        solver.interrupt(-5.0);

        assert_eq!(solver.target(), -5.0);
        assert_eq!(solver.elapsed(), 0.0);
        let (pos_after, vel_after) = solver.sample();
        assert!(approx_eq(pos_after, pos_before, 1e-5));
        assert!(approx_eq(vel_after, vel_before, 1e-5));
    }

    #[test]
    fn interrupt_continues_motion_smoothly() {
        let config = SpringConfig::new(1.0, 25.0, 6.0).unwrap();
        let mut solver = SpringSolver::new(config, 0.0, 10.0, 0.0);
        solver.advance(0.2);
        solver.interrupt(10.0);
        // After retargeting back to the original target, advancing further
        // should continue towards it without a position jump.
        let pos_right_after = solver.position();
        solver.advance(0.1);
        let pos_later = solver.position();
        assert!(pos_later > pos_right_after);
    }

    // ------------------------------------------------------------------
    // Edge cases: NaN / Inf, zero state
    // ------------------------------------------------------------------

    #[test]
    fn nan_initial_returns_target_safely() {
        let solver = SpringSolver::new(SpringConfig::CRITICAL, f32::NAN, 10.0, 0.0);
        let (pos, vel) = solver.sample_at(0.5);
        assert_eq!(pos, 10.0);
        assert_eq!(vel, 0.0);
    }

    #[test]
    fn inf_velocity_is_sanitized_to_zero() {
        // Inf velocity is sanitized to 0.0, so the spring starts at rest.
        let solver = SpringSolver::new(SpringConfig::CRITICAL, 0.0, 10.0, f32::INFINITY);
        assert_eq!(solver.velocity(), 0.0);
        // Output must be finite at all times.
        let (pos, vel) = solver.sample_at(0.5);
        assert!(pos.is_finite(), "position must be finite, got {pos}");
        assert!(vel.is_finite(), "velocity must be finite, got {vel}");
    }

    #[test]
    fn nan_time_returns_target_safely() {
        let solver = SpringSolver::new(SpringConfig::CRITICAL, 0.0, 10.0, 0.0);
        let (pos, vel) = solver.sample_at(f32::NAN);
        assert_eq!(pos, 10.0);
        assert_eq!(vel, 0.0);
    }

    #[test]
    fn advance_with_nan_is_ignored() {
        let mut solver = SpringSolver::new(SpringConfig::CRITICAL, 0.0, 10.0, 0.0);
        solver.advance(0.5);
        solver.advance(f32::NAN);
        assert_eq!(solver.elapsed(), 0.5);
    }

    #[test]
    fn advance_with_negative_dt_is_ignored() {
        let mut solver = SpringSolver::new(SpringConfig::CRITICAL, 0.0, 10.0, 0.0);
        solver.advance(0.5);
        solver.advance(-0.3);
        assert_eq!(solver.elapsed(), 0.5);
    }

    #[test]
    fn advance_with_inf_is_ignored() {
        let mut solver = SpringSolver::new(SpringConfig::CRITICAL, 0.0, 10.0, 0.0);
        solver.advance(0.5);
        solver.advance(f32::INFINITY);
        assert_eq!(solver.elapsed(), 0.5);
    }

    #[test]
    fn nan_target_is_sanitized_to_zero() {
        let solver = SpringSolver::new(SpringConfig::CRITICAL, 0.0, f32::NAN, 0.0);
        assert_eq!(solver.target(), 0.0);
        let (pos, vel) = solver.sample_at(0.5);
        assert!(pos.is_finite());
        assert!(vel.is_finite());
    }

    #[test]
    fn inf_target_is_sanitized_to_zero() {
        let solver = SpringSolver::new(SpringConfig::CRITICAL, 0.0, f32::INFINITY, 0.0);
        assert_eq!(solver.target(), 0.0);
        let (pos, vel) = solver.sample_at(0.5);
        assert!(pos.is_finite());
        assert!(vel.is_finite());
    }

    #[test]
    fn nan_initial_snaps_to_target() {
        let solver = SpringSolver::new(SpringConfig::CRITICAL, f32::NAN, 10.0, 0.0);
        // Non-finite initial is replaced with the (finite) target.
        assert_eq!(solver.position(), 10.0);
    }

    #[test]
    fn nan_initial_and_target_both_sanitized() {
        let solver = SpringSolver::new(SpringConfig::CRITICAL, f32::NAN, f32::NAN, 0.0);
        // Both NaN: target -> 0.0, initial -> 0.0.
        assert_eq!(solver.target(), 0.0);
        assert_eq!(solver.position(), 0.0);
    }

    #[test]
    fn nan_velocity_is_sanitized_to_zero() {
        let solver = SpringSolver::new(SpringConfig::CRITICAL, 0.0, 10.0, f32::NAN);
        assert_eq!(solver.velocity(), 0.0);
    }

    #[test]
    fn config_fields_are_accessible_via_accessors() {
        let config = SpringConfig::new(2.0, 300.0, 15.0).unwrap();
        assert_eq!(config.mass(), 2.0);
        assert_eq!(config.stiffness(), 300.0);
        assert_eq!(config.damping(), 15.0);
    }

    #[test]
    fn config_presets_have_correct_values() {
        assert!(SpringConfig::CRITICAL.mass() > 0.0);
        assert!(SpringConfig::CRITICAL.stiffness() > 0.0);
        assert!(SpringConfig::CRITICAL.damping() >= 0.0);
        assert!(SpringConfig::SNAPPY.mass() > 0.0);
        assert!(SpringConfig::GENTLE.mass() > 0.0);
        assert!(SpringConfig::BOUNCY.mass() > 0.0);
    }

    #[test]
    fn interrupt_with_nan_target_is_sanitized() {
        let mut solver = SpringSolver::new(SpringConfig::CRITICAL, 0.0, 10.0, 0.0);
        solver.advance(0.2);
        solver.interrupt(f32::NAN);
        // NaN target should be sanitized to 0.0.
        assert_eq!(solver.target(), 0.0);
        let (pos, vel) = solver.sample_at(0.1);
        assert!(
            pos.is_finite(),
            "position must be finite after NaN interrupt"
        );
        assert!(
            vel.is_finite(),
            "velocity must be finite after NaN interrupt"
        );
    }

    #[test]
    fn interrupt_with_inf_target_is_sanitized() {
        let mut solver = SpringSolver::new(SpringConfig::CRITICAL, 0.0, 10.0, 0.0);
        solver.advance(0.2);
        solver.interrupt(f32::INFINITY);
        // Inf target should be sanitized to 0.0.
        assert_eq!(solver.target(), 0.0);
        let (pos, vel) = solver.sample_at(0.1);
        assert!(
            pos.is_finite(),
            "position must be finite after Inf interrupt"
        );
        assert!(
            vel.is_finite(),
            "velocity must be finite after Inf interrupt"
        );
    }

    #[test]
    fn zero_displacement_zero_velocity_stays_at_target() {
        let target = 5.0_f32;
        let solver = SpringSolver::new(SpringConfig::CRITICAL, target, target, 0.0);
        for t in [0.0_f32, 0.1, 1.0, 10.0, 100.0] {
            let (pos, vel) = solver.sample_at(t);
            assert_eq!(pos, target, "position drifted at t={t}");
            assert_eq!(vel, 0.0, "velocity non-zero at t={t}");
        }
    }

    // ------------------------------------------------------------------
    // Accessors
    // ------------------------------------------------------------------

    #[test]
    fn advance_updates_elapsed_clock() {
        let mut solver = SpringSolver::new(SpringConfig::CRITICAL, 0.0, 10.0, 0.0);
        assert_eq!(solver.elapsed(), 0.0);
        solver.advance(0.016);
        assert!(approx_eq(solver.elapsed(), 0.016, 1e-7));
        solver.advance(0.016);
        assert!(approx_eq(solver.elapsed(), 0.032, 1e-7));
    }

    #[test]
    fn config_accessor_returns_config() {
        let solver = SpringSolver::new(SpringConfig::SNAPPY, 0.0, 1.0, 0.0);
        assert_eq!(solver.config(), SpringConfig::SNAPPY);
    }

    #[test]
    fn damping_ratio_accessor_returns_zeta() {
        let solver = SpringSolver::new(SpringConfig::SNAPPY, 0.0, 1.0, 0.0);
        let zeta = solver.damping_ratio();
        assert!(zeta > 0.0 && zeta < 1.0);
    }
}
