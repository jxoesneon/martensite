//! Animation driver management with continuous interpolation and C¹ velocity
//! handoff upon interruption.
//!
//! The [`AnimationDriver`] manages a collection of active spring animations keyed
//! by [`AnimationId`]. When an animation is interrupted mid-motion, the new
//! spring is seeded with the exact position and velocity sampled from the
//! outgoing spring, ensuring seamless continuity without position jumps or
//! acceleration spikes (C¹ velocity continuity).
//!
//! # Examples
//!
//! ```
//! use martensite_motion::{AnimationDriver, SpringConfig};
//!
//! let mut driver = AnimationDriver::new();
//! let id = driver.start(SpringConfig::CRITICAL, 0.0, 10.0);
//! assert_eq!(driver.active_count(), 1);
//! driver.advance(0.016);
//! assert!(driver.position(id).is_some());
//! ```

use crate::spring::{SpringConfig, SpringSolver};
use glam::Vec2;
use std::collections::HashMap;

/// Opaque identifier for an active animation.
///
/// Returned by [`AnimationDriver::start`] and
/// [`AnimationDriver::start_with_velocity`]. It is cheap to copy and compare,
/// and may be used to sample, interrupt, or query the corresponding animation.
#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash)]
pub struct AnimationId(u64);

impl AnimationId {
    /// Returns the raw `u64` backing this identifier.
    pub fn raw(self) -> u64 {
        self.0
    }
}

/// Errors that can occur when interacting with an [`AnimationDriver`].
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum AnimationError {
    /// The referenced [`AnimationId`] does not correspond to an active
    /// animation (it was never started, or has already been removed).
    NotFound,
    /// The referenced animation has already settled and cannot be interrupted.
    AlreadySettled,
}

impl std::fmt::Display for AnimationError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            AnimationError::NotFound => write!(f, "animation not found"),
            AnimationError::AlreadySettled => write!(f, "animation already settled"),
        }
    }
}

impl std::error::Error for AnimationError {}

/// A single active spring animation tracked by an [`AnimationDriver`].
///
/// Stores the underlying [`SpringSolver`] which tracks its own elapsed time
/// internally.
#[derive(Debug)]
pub(crate) struct ActiveAnimation {
    /// The analytical spring solver producing position and velocity samples.
    solver: SpringSolver,
}

impl ActiveAnimation {
    /// Creates a new active animation wrapping the given solver.
    fn new(solver: SpringSolver) -> Self {
        Self { solver }
    }

    /// Samples the current `(position, velocity)` of the animation.
    #[inline]
    fn sample(&self) -> (f32, f32) {
        self.solver.sample()
    }

    /// Returns `true` when the animation has converged to its target within the
    /// solver's settle threshold.
    #[inline]
    fn is_settled(&self) -> bool {
        self.solver.settle_threshold()
    }

    /// Advances the underlying solver by `dt` seconds.
    #[inline]
    fn advance(&mut self, dt: f32) {
        self.solver.advance(dt);
    }
}

/// Manages a collection of active one-dimensional spring animations.
///
/// Each animation is identified by a unique [`AnimationId`] and driven by an
/// analytical [`SpringSolver`]. The driver supports continuous interpolation
/// and C¹ velocity handoff when animations are interrupted mid-motion.
///
/// # Examples
///
/// ```
/// use martensite_motion::{AnimationDriver, SpringConfig};
///
/// let mut driver = AnimationDriver::new();
/// let id = driver.start(SpringConfig::CRITICAL, 0.0, 1.0);
/// driver.advance(0.1);
/// assert!(driver.position(id).is_some());
/// ```
#[derive(Debug)]
pub struct AnimationDriver {
    animations: HashMap<AnimationId, ActiveAnimation>,
    next_id: u64,
}

impl Default for AnimationDriver {
    fn default() -> Self {
        Self::new()
    }
}

impl AnimationDriver {
    /// Creates a new, empty animation driver.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite_motion::AnimationDriver;
    ///
    /// let driver = AnimationDriver::new();
    /// assert_eq!(driver.active_count(), 0);
    /// ```
    pub fn new() -> Self {
        Self {
            animations: HashMap::new(),
            next_id: 0,
        }
    }

    /// Starts a new spring animation with zero initial velocity and returns its
    /// [`AnimationId`].
    ///
    /// The animation moves from `initial` towards `target` under the given
    /// [`SpringConfig`].
    pub fn start(&mut self, config: SpringConfig, initial: f32, target: f32) -> AnimationId {
        self.start_with_velocity(config, initial, target, 0.0)
    }

    /// Starts a new spring animation with a non-zero initial velocity and
    /// returns its [`AnimationId`].
    ///
    /// This is useful for chaining animations or seeding a spring from an
    /// externally observed velocity.
    pub fn start_with_velocity(
        &mut self,
        config: SpringConfig,
        initial: f32,
        target: f32,
        initial_vel: f32,
    ) -> AnimationId {
        let id = self.allocate_id();
        let solver = SpringSolver::new(config, initial, target, initial_vel);
        let animation = ActiveAnimation::new(solver);
        self.animations.insert(id, animation);
        id
    }

    /// Interrupts an active animation with C¹ velocity continuity.
    ///
    /// The current position and velocity are sampled from the outgoing spring
    /// and used as the initial conditions for a new spring (with the same
    /// [`SpringConfig`]) targeting `new_target`. This guarantees that neither
    /// the position nor the velocity discontinuously jumps at the moment of
    /// interruption.
    ///
    /// # Errors
    ///
    /// Returns [`AnimationError::NotFound`] if `id` is not an active animation,
    /// or [`AnimationError::AlreadySettled`] if the animation has already
    /// settled.
    pub fn interrupt(&mut self, id: AnimationId, new_target: f32) -> Result<(), AnimationError> {
        let animation = self.animations.get(&id).ok_or(AnimationError::NotFound)?;

        if animation.is_settled() {
            return Err(AnimationError::AlreadySettled);
        }

        let config = animation.solver.config();
        let (pos, vel) = animation.sample();

        // The immutable borrow of `animation` ends here; the new solver is
        // constructed from copied values before the mutable insertion below.
        let solver = SpringSolver::new(config, pos, new_target, vel);
        let replacement = ActiveAnimation::new(solver);
        self.animations.insert(id, replacement);
        Ok(())
    }

    /// Advances all active animations by `dt` seconds.
    ///
    /// This advances each underlying solver's deterministic internal clock.
    /// The method performs no heap allocation.
    pub fn advance(&mut self, dt: f32) {
        for animation in self.animations.values_mut() {
            animation.advance(dt);
        }
    }

    /// Samples the current `(position, velocity)` for the animation identified
    /// by `id`.
    ///
    /// Returns `None` if the animation is not active.
    pub fn sample(&self, id: AnimationId) -> Option<(f32, f32)> {
        self.animations.get(&id).map(|a| a.sample())
    }

    /// Returns the current position of the animation identified by `id`, or
    /// `None` if it is not active.
    pub fn position(&self, id: AnimationId) -> Option<f32> {
        self.animations.get(&id).map(|a| a.solver.position())
    }

    /// Returns the current velocity of the animation identified by `id`, or
    /// `None` if it is not active.
    pub fn velocity(&self, id: AnimationId) -> Option<f32> {
        self.animations.get(&id).map(|a| a.solver.velocity())
    }

    /// Returns `true` if the animation has settled below the solver's threshold,
    /// or if it does not exist.
    ///
    /// A non-existent animation is considered settled so that callers can use
    /// this as a poll condition without first checking for existence.
    pub fn is_settled(&self, id: AnimationId) -> bool {
        match self.animations.get(&id) {
            Some(animation) => animation.is_settled(),
            None => true,
        }
    }

    /// Removes all settled animations and returns the number removed.
    pub fn remove_settled(&mut self) -> usize {
        let before = self.animations.len();
        self.animations.retain(|_, a| !a.is_settled());
        before - self.animations.len()
    }

    /// Returns the number of currently active animations.
    pub fn active_count(&self) -> usize {
        self.animations.len()
    }

    /// Removes all animations, active or settled.
    pub fn clear(&mut self) {
        self.animations.clear();
    }

    /// Allocates the next unique [`AnimationId`].
    fn allocate_id(&mut self) -> AnimationId {
        let id = AnimationId(self.next_id);
        self.next_id += 1;
        id
    }
}

/// A single active 2D spring animation tracked by an [`AnimationDriver2D`].
#[derive(Debug)]
struct ActiveAnimation2D {
    /// Solver driving the x-axis.
    solver_x: SpringSolver,
    /// Solver driving the y-axis.
    solver_y: SpringSolver,
}

impl ActiveAnimation2D {
    /// Samples the current `(position, velocity)` of the 2D animation.
    fn sample(&self) -> (Vec2, Vec2) {
        let (px, vx) = self.solver_x.sample();
        let (py, vy) = self.solver_y.sample();
        (Vec2::new(px, py), Vec2::new(vx, vy))
    }

    /// Returns `true` when both axes have converged to their targets within the
    /// solver's settle threshold.
    fn is_settled(&self) -> bool {
        self.solver_x.settle_threshold() && self.solver_y.settle_threshold()
    }

    /// Advances both underlying solvers by `dt` seconds.
    fn advance(&mut self, dt: f32) {
        self.solver_x.advance(dt);
        self.solver_y.advance(dt);
    }
}

/// Manages a collection of active two-dimensional spring animations.
///
/// Each axis is driven by an independent [`SpringSolver`], allowing different
/// targets and naturally decoupled timing per axis while still supporting
/// C¹ velocity handoff on interruption.
///
/// # Examples
///
/// ```
/// use martensite_motion::{AnimationDriver2D, SpringConfig};
/// use glam::Vec2;
///
/// let mut driver = AnimationDriver2D::new();
/// let id = driver.start(SpringConfig::CRITICAL, Vec2::ZERO, Vec2::new(10.0, 5.0));
/// driver.advance(0.1);
/// assert!(driver.position(id).is_some());
/// ```
#[derive(Debug)]
pub struct AnimationDriver2D {
    animations: HashMap<AnimationId, ActiveAnimation2D>,
    next_id: u64,
}

impl Default for AnimationDriver2D {
    fn default() -> Self {
        Self::new()
    }
}

impl AnimationDriver2D {
    /// Creates a new, empty 2D animation driver.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite_motion::AnimationDriver2D;
    ///
    /// let driver = AnimationDriver2D::new();
    /// assert_eq!(driver.active_count(), 0);
    /// ```
    pub fn new() -> Self {
        Self {
            animations: HashMap::new(),
            next_id: 0,
        }
    }

    /// Starts a new 2D spring animation with zero initial velocity and returns
    /// its [`AnimationId`].
    ///
    /// The animation moves from `initial` towards `target` using independent
    /// solvers per axis.
    pub fn start(&mut self, config: SpringConfig, initial: Vec2, target: Vec2) -> AnimationId {
        self.start_with_velocity(config, initial, target, Vec2::ZERO)
    }

    /// Starts a new 2D spring animation with a non-zero initial velocity and
    /// returns its [`AnimationId`].
    pub fn start_with_velocity(
        &mut self,
        config: SpringConfig,
        initial: Vec2,
        target: Vec2,
        initial_vel: Vec2,
    ) -> AnimationId {
        let id = self.allocate_id();
        let solver_x = SpringSolver::new(config, initial.x, target.x, initial_vel.x);
        let solver_y = SpringSolver::new(config, initial.y, target.y, initial_vel.y);
        let animation = ActiveAnimation2D { solver_x, solver_y };
        self.animations.insert(id, animation);
        id
    }

    /// Interrupts an active 2D animation with C¹ velocity continuity.
    ///
    /// The current position and velocity are sampled from the outgoing solvers
    /// and used as the initial conditions for a new pair of springs (with the
    /// same [`SpringConfig`]) targeting `new_target`.
    ///
    /// # Errors
    ///
    /// Returns [`AnimationError::NotFound`] if `id` is not an active animation,
    /// or [`AnimationError::AlreadySettled`] if the animation has already
    /// settled.
    pub fn interrupt(&mut self, id: AnimationId, new_target: Vec2) -> Result<(), AnimationError> {
        let animation = self.animations.get(&id).ok_or(AnimationError::NotFound)?;

        if animation.is_settled() {
            return Err(AnimationError::AlreadySettled);
        }

        let config = animation.solver_x.config();
        let (pos, vel) = animation.sample();

        // The immutable borrow of `animation` ends here; the new solvers are
        // constructed from copied values before the mutable insertion below.
        let solver_x = SpringSolver::new(config, pos.x, new_target.x, vel.x);
        let solver_y = SpringSolver::new(config, pos.y, new_target.y, vel.y);
        let replacement = ActiveAnimation2D { solver_x, solver_y };
        self.animations.insert(id, replacement);
        Ok(())
    }

    /// Advances all active 2D animations by `dt` seconds.
    ///
    /// Updates each solver's deterministic internal clock without performing
    /// any heap allocation.
    pub fn advance(&mut self, dt: f32) {
        for animation in self.animations.values_mut() {
            animation.advance(dt);
        }
    }

    /// Samples the current `(position, velocity)` for the 2D animation
    /// identified by `id`.
    ///
    /// Returns `None` if the animation is not active.
    pub fn sample(&self, id: AnimationId) -> Option<(Vec2, Vec2)> {
        self.animations.get(&id).map(|a| a.sample())
    }

    /// Returns the current position of the 2D animation identified by `id`, or
    /// `None` if it is not active.
    pub fn position(&self, id: AnimationId) -> Option<Vec2> {
        self.animations.get(&id).map(|a| a.sample().0)
    }

    /// Returns the current velocity of the 2D animation identified by `id`, or
    /// `None` if it is not active.
    pub fn velocity(&self, id: AnimationId) -> Option<Vec2> {
        self.animations.get(&id).map(|a| a.sample().1)
    }

    /// Returns `true` if the 2D animation has settled below the solver's
    /// threshold, or if it does not exist.
    pub fn is_settled(&self, id: AnimationId) -> bool {
        match self.animations.get(&id) {
            Some(animation) => animation.is_settled(),
            None => true,
        }
    }

    /// Removes all settled 2D animations and returns the number removed.
    pub fn remove_settled(&mut self) -> usize {
        let before = self.animations.len();
        self.animations.retain(|_, a| !a.is_settled());
        before - self.animations.len()
    }

    /// Returns the number of currently active 2D animations.
    pub fn active_count(&self) -> usize {
        self.animations.len()
    }

    /// Removes all 2D animations, active or settled.
    pub fn clear(&mut self) {
        self.animations.clear();
    }

    /// Allocates the next unique [`AnimationId`].
    fn allocate_id(&mut self) -> AnimationId {
        let id = AnimationId(self.next_id);
        self.next_id += 1;
        id
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const TOL: f32 = 1e-4;

    fn approx_eq(a: f32, b: f32, tol: f32) -> bool {
        (a - b).abs() < tol
    }

    #[test]
    fn start_animation_has_initial_position() {
        let mut driver = AnimationDriver::new();
        let id = driver.start(SpringConfig::CRITICAL, 0.0, 10.0);
        let pos = driver.position(id).expect("animation should exist");
        assert_eq!(
            pos, 0.0,
            "initial position should be exactly the initial value"
        );
    }

    #[test]
    fn advance_changes_position() {
        let mut driver = AnimationDriver::new();
        let id = driver.start(SpringConfig::CRITICAL, 0.0, 10.0);
        let pos_before = driver.position(id).expect("animation should exist");
        assert_eq!(pos_before, 0.0);
        driver.advance(0.1);
        let pos_after = driver.position(id).expect("animation should exist");
        assert!(
            pos_after > pos_before,
            "position should advance towards target: before={pos_before}, after={pos_after}"
        );
    }

    #[test]
    fn interrupt_preserves_position_and_velocity() {
        let mut driver = AnimationDriver::new();
        let id = driver.start(SpringConfig::CRITICAL, 0.0, 100.0);

        // Build up velocity towards the target using the deterministic clock.
        driver.advance(0.1);
        let (pos_before, vel_before) = driver.sample(id).expect("animation should exist");
        assert!(
            vel_before > 0.0,
            "spring should be moving towards target, vel={vel_before}"
        );
        assert!(pos_before > 0.0 && pos_before < 100.0);

        // Interrupt with a new target; C¹ continuity requires position and
        // velocity to be preserved across the handoff.
        driver
            .interrupt(id, -50.0)
            .expect("interrupt should succeed");

        let (pos_after, vel_after) = driver.sample(id).expect("animation should still exist");
        assert!(
            approx_eq(pos_before, pos_after, TOL),
            "position must be continuous across interrupt: before={pos_before}, after={pos_after}"
        );
        assert!(
            approx_eq(vel_before, vel_after, TOL),
            "velocity must be continuous across interrupt: before={vel_before}, after={vel_after}"
        );
    }

    #[test]
    fn interrupt_missing_returns_not_found() {
        let mut driver = AnimationDriver::new();
        let id = driver.start(SpringConfig::CRITICAL, 0.0, 1.0);
        driver.clear();
        let err = driver.interrupt(id, 5.0).unwrap_err();
        assert_eq!(err, AnimationError::NotFound);
    }

    #[test]
    fn interrupt_settled_returns_already_settled() {
        let mut driver = AnimationDriver::new();
        // An animation already at its target with zero velocity is settled.
        let id = driver.start(SpringConfig::CRITICAL, 5.0, 5.0);
        assert!(driver.is_settled(id));
        let err = driver.interrupt(id, 10.0).unwrap_err();
        assert_eq!(err, AnimationError::AlreadySettled);
    }

    #[test]
    fn settled_animation_is_detected() {
        let mut driver = AnimationDriver::new();
        let id = driver.start(SpringConfig::CRITICAL, 0.0, 10.0);
        // Advancing well past the settle time converges the spring.
        driver.advance(60.0);
        assert!(
            driver.is_settled(id),
            "animation should settle after sufficient elapsed time"
        );
    }

    #[test]
    fn animation_at_target_is_immediately_settled() {
        let mut driver = AnimationDriver::new();
        let id = driver.start(SpringConfig::CRITICAL, 7.0, 7.0);
        assert!(
            driver.is_settled(id),
            "animation at target with zero velocity should be settled"
        );
    }

    #[test]
    fn non_settled_animation_is_not_settled() {
        let mut driver = AnimationDriver::new();
        let id = driver.start(SpringConfig::CRITICAL, 0.0, 100.0);
        assert!(
            !driver.is_settled(id),
            "animation far from target should not be settled"
        );
    }

    #[test]
    fn is_settled_for_missing_id_returns_true() {
        let driver = AnimationDriver::new();
        assert!(driver.is_settled(AnimationId(999)));
    }

    #[test]
    fn remove_settled_clears_finished_animations() {
        let mut driver = AnimationDriver::new();
        let _settled = driver.start(SpringConfig::CRITICAL, 3.0, 3.0);
        let _active = driver.start(SpringConfig::CRITICAL, 0.0, 100.0);
        assert_eq!(driver.active_count(), 2);

        let removed = driver.remove_settled();
        assert_eq!(removed, 1);
        assert_eq!(driver.active_count(), 1);
    }

    #[test]
    fn remove_settled_on_empty_driver() {
        let mut driver = AnimationDriver::new();
        assert_eq!(driver.remove_settled(), 0);
    }

    #[test]
    fn multiple_concurrent_animations_are_independent() {
        let mut driver = AnimationDriver::new();
        let a = driver.start(SpringConfig::CRITICAL, 0.0, 10.0);
        let b = driver.start(SpringConfig::CRITICAL, 0.0, 50.0);
        let c = driver.start(SpringConfig::SNAPPY, 20.0, -20.0);

        assert_eq!(driver.active_count(), 3);
        assert!(a != b && b != c && a != c);

        // Advance the deterministic clock so each spring advances independently.
        driver.advance(0.1);

        let pa = driver.position(a).unwrap();
        let pb = driver.position(b).unwrap();
        let pc = driver.position(c).unwrap();

        // Each animation pursues a distinct target, so their positions differ.
        assert!(!approx_eq(pa, pb, TOL));
        assert!(!approx_eq(pb, pc, TOL));
        assert!(!approx_eq(pa, pc, TOL));
    }

    #[test]
    fn clear_removes_all_animations() {
        let mut driver = AnimationDriver::new();
        let _a = driver.start(SpringConfig::CRITICAL, 0.0, 1.0);
        let _b = driver.start(SpringConfig::CRITICAL, 0.0, 2.0);
        assert_eq!(driver.active_count(), 2);
        driver.clear();
        assert_eq!(driver.active_count(), 0);
    }

    #[test]
    fn start_with_velocity_seeds_initial_velocity() {
        let mut driver = AnimationDriver::new();
        let id = driver.start_with_velocity(SpringConfig::CRITICAL, 0.0, 100.0, 25.0);
        let vel = driver.velocity(id).expect("animation should exist");
        // Sampled at t=0, the velocity should exactly match the seed.
        assert_eq!(vel, 25.0, "initial velocity should be seeded exactly");
    }

    #[test]
    fn advance_does_not_grow_allocations() {
        // Advancing animations must not allocate: the HashMap capacity should
        // remain unchanged across repeated `advance` calls.
        let mut driver = AnimationDriver::new();
        let _a = driver.start(SpringConfig::CRITICAL, 0.0, 1.0);
        let _b = driver.start(SpringConfig::CRITICAL, 0.0, 2.0);
        let _c = driver.start(SpringConfig::CRITICAL, 0.0, 3.0);

        let capacity_before = driver.animations.capacity();
        for _ in 0..1000 {
            driver.advance(0.016);
        }
        let capacity_after = driver.animations.capacity();
        assert_eq!(
            capacity_before, capacity_after,
            "advance must not trigger HashMap reallocation"
        );
        assert_eq!(driver.active_count(), 3);
    }

    #[test]
    fn advance_is_deterministic() {
        // The same sequence of `advance` calls must produce the same position
        // regardless of when sampling happens.
        let mut driver_a = AnimationDriver::new();
        let id_a = driver_a.start(SpringConfig::CRITICAL, 0.0, 10.0);
        driver_a.advance(0.1);
        driver_a.advance(0.1);
        let pa = driver_a.position(id_a).unwrap();

        let mut driver_b = AnimationDriver::new();
        let id_b = driver_b.start(SpringConfig::CRITICAL, 0.0, 10.0);
        driver_b.advance(0.2);
        let pb = driver_b.position(id_b).unwrap();

        assert!(
            approx_eq(pa, pb, 1e-6),
            "advancing 0.1+0.1 should equal 0.2: {pa} vs {pb}"
        );
    }

    #[test]
    fn driver_2d_start_and_sample() {
        let mut driver = AnimationDriver2D::new();
        let id = driver.start(SpringConfig::CRITICAL, Vec2::ZERO, Vec2::new(10.0, 5.0));
        let pos = driver.position(id).expect("2d animation should exist");
        assert_eq!(pos, Vec2::ZERO, "initial position should be exactly origin");
        assert_eq!(driver.active_count(), 1);
    }

    #[test]
    fn driver_2d_advance_changes_position() {
        let mut driver = AnimationDriver2D::new();
        let id = driver.start(SpringConfig::CRITICAL, Vec2::ZERO, Vec2::new(10.0, 5.0));
        let pos_before = driver.position(id).unwrap();
        driver.advance(0.1);
        let pos_after = driver.position(id).unwrap();
        assert!(
            pos_after.x > pos_before.x && pos_after.y > pos_before.y,
            "both axes should advance: before={pos_before}, after={pos_after}"
        );
    }

    #[test]
    fn driver_2d_interrupt_preserves_position_and_velocity() {
        let mut driver = AnimationDriver2D::new();
        let id = driver.start(SpringConfig::CRITICAL, Vec2::ZERO, Vec2::new(100.0, 80.0));

        driver.advance(0.1);
        let (pos_before, vel_before) = driver.sample(id).unwrap();
        driver
            .interrupt(id, Vec2::new(-50.0, -40.0))
            .expect("interrupt should succeed");
        let (pos_after, vel_after) = driver.sample(id).unwrap();

        assert!(
            approx_eq(pos_before.x, pos_after.x, TOL) && approx_eq(pos_before.y, pos_after.y, TOL),
            "position must be continuous across 2d interrupt: before={pos_before}, after={pos_after}"
        );
        assert!(
            approx_eq(vel_before.x, vel_after.x, TOL) && approx_eq(vel_before.y, vel_after.y, TOL),
            "velocity must be continuous across 2d interrupt: before={vel_before}, after={vel_after}"
        );
    }

    #[test]
    fn driver_2d_settled_detection() {
        let mut driver = AnimationDriver2D::new();
        let id = driver.start(SpringConfig::CRITICAL, Vec2::ZERO, Vec2::new(10.0, 5.0));
        driver.advance(60.0);
        assert!(
            driver.is_settled(id),
            "2d animation should settle after sufficient elapsed time"
        );
    }

    #[test]
    fn driver_2d_remove_settled() {
        let mut driver = AnimationDriver2D::new();
        let _settled = driver.start(SpringConfig::CRITICAL, Vec2::splat(1.0), Vec2::splat(1.0));
        let _active = driver.start(SpringConfig::CRITICAL, Vec2::ZERO, Vec2::new(100.0, 100.0));
        assert_eq!(driver.active_count(), 2);
        let removed = driver.remove_settled();
        assert_eq!(removed, 1);
        assert_eq!(driver.active_count(), 1);
    }

    #[test]
    fn driver_2d_axes_are_independent() {
        let mut driver = AnimationDriver2D::new();
        let id = driver.start(SpringConfig::CRITICAL, Vec2::ZERO, Vec2::new(100.0, 1.0));
        driver.advance(0.1);
        let pos = driver.position(id).unwrap();
        // The x-axis has a much larger displacement than y, so it should have
        // moved further in absolute terms.
        assert!(pos.x > pos.y, "x-axis should advance faster than y: {pos}");
    }

    #[test]
    fn animation_id_is_copy_and_eq() {
        let a = AnimationId(1);
        let b = a;
        assert_eq!(a, b);
        assert_ne!(a, AnimationId(2));
        assert_eq!(a.raw(), 1);
    }

    #[test]
    fn animation_error_display() {
        assert_eq!(
            format!("{}", AnimationError::NotFound),
            "animation not found"
        );
        assert_eq!(
            format!("{}", AnimationError::AlreadySettled),
            "animation already settled"
        );
    }

    // ------------------------------------------------------------------
    // Edge cases: NaN / Inf targets, negative dt, mixed-axis interruption
    // ------------------------------------------------------------------

    #[test]
    fn nan_target_in_start_produces_finite_output() {
        let mut driver = AnimationDriver::new();
        let id = driver.start(SpringConfig::CRITICAL, 0.0, f32::NAN);
        driver.advance(0.1);
        let pos = driver.position(id).expect("animation should exist");
        assert!(pos.is_finite(), "position must be finite, got {pos}");
    }

    #[test]
    fn inf_target_in_start_produces_finite_output() {
        let mut driver = AnimationDriver::new();
        let id = driver.start(SpringConfig::CRITICAL, 0.0, f32::INFINITY);
        driver.advance(0.1);
        let pos = driver.position(id).expect("animation should exist");
        assert!(pos.is_finite(), "position must be finite, got {pos}");
    }

    #[test]
    fn nan_velocity_in_start_produces_finite_output() {
        let mut driver = AnimationDriver::new();
        let id = driver.start_with_velocity(SpringConfig::CRITICAL, 0.0, 10.0, f32::NAN);
        driver.advance(0.1);
        let vel = driver.velocity(id).expect("animation should exist");
        assert!(vel.is_finite(), "velocity must be finite, got {vel}");
    }

    #[test]
    fn negative_dt_in_advance_is_ignored() {
        let mut driver = AnimationDriver::new();
        let id = driver.start(SpringConfig::CRITICAL, 0.0, 10.0);
        driver.advance(0.1);
        let pos_before = driver.position(id).unwrap();
        driver.advance(-0.5);
        let pos_after = driver.position(id).unwrap();
        assert_eq!(
            pos_before, pos_after,
            "negative dt must not move the animation"
        );
    }

    #[test]
    fn nan_dt_in_advance_is_ignored() {
        let mut driver = AnimationDriver::new();
        let id = driver.start(SpringConfig::CRITICAL, 0.0, 10.0);
        driver.advance(0.1);
        let pos_before = driver.position(id).unwrap();
        driver.advance(f32::NAN);
        let pos_after = driver.position(id).unwrap();
        assert_eq!(pos_before, pos_after, "NaN dt must not move the animation");
    }

    #[test]
    fn driver_2d_nan_target_produces_finite_output() {
        let mut driver = AnimationDriver2D::new();
        let id = driver.start(
            SpringConfig::CRITICAL,
            Vec2::ZERO,
            Vec2::new(f32::NAN, 10.0),
        );
        driver.advance(0.1);
        let pos = driver.position(id).expect("2d animation should exist");
        assert!(pos.x.is_finite(), "x must be finite, got {}", pos.x);
        assert!(pos.y.is_finite(), "y must be finite, got {}", pos.y);
    }

    #[test]
    fn driver_2d_mixed_axis_interruption_preserves_continuity() {
        let mut driver = AnimationDriver2D::new();
        let id = driver.start(SpringConfig::CRITICAL, Vec2::ZERO, Vec2::new(100.0, 1.0));
        driver.advance(0.1);
        let (pos_before, vel_before) = driver.sample(id).unwrap();
        // Interrupt only the x-target, keeping y the same.
        driver
            .interrupt(id, Vec2::new(-50.0, 1.0))
            .expect("interrupt should succeed");
        let (pos_after, vel_after) = driver.sample(id).unwrap();
        assert!(
            approx_eq(pos_before.x, pos_after.x, TOL)
                && approx_eq(pos_before.y, pos_after.y, TOL),
            "position must be continuous across mixed-axis interrupt: before={pos_before}, after={pos_after}"
        );
        assert!(
            approx_eq(vel_before.x, vel_after.x, TOL)
                && approx_eq(vel_before.y, vel_after.y, TOL),
            "velocity must be continuous across mixed-axis interrupt: before={vel_before}, after={vel_after}"
        );
    }

    #[test]
    fn driver_2d_negative_dt_is_ignored() {
        let mut driver = AnimationDriver2D::new();
        let id = driver.start(SpringConfig::CRITICAL, Vec2::ZERO, Vec2::new(10.0, 5.0));
        driver.advance(0.1);
        let pos_before = driver.position(id).unwrap();
        driver.advance(-0.5);
        let pos_after = driver.position(id).unwrap();
        assert_eq!(
            pos_before, pos_after,
            "negative dt must not move 2d animation"
        );
    }
}
