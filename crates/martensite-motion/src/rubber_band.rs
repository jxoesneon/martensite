//! iOS-style rubber-band overscroll physics.
//!
//! This module provides [`RubberBandScroller`] (single-axis) and
//! [`RubberBandScroller2D`] (dual-axis with directional axis locking): pure
//! physics types that model the way iOS scroll views stretch beyond their
//! content bounds while dragging and spring back to the nearest boundary on
//! release. The stretch uses a constant linear coefficient
//! ([`RUBBER_BAND_COEFFICIENT`]) and the spring-back is driven by the
//! closed-form [`SpringSolver`] from [`crate::spring`], yielding frame-rate
//! independent, zero-alibration motion.
//!
//! All query and advance methods ([`RubberBandScroller::visible_offset`],
//! [`RubberBandScroller::update`], and their 2D counterparts) run in `O(1)`
//! time and perform zero heap allocations, making them suitable for per-frame
//! use inside a render loop.

use crate::spring::{SpringConfig, SpringSolver};

/// The rubber-band stretch coefficient.
///
/// When overscrolling, the visible offset is computed as
/// `boundary + (delta * RUBBER_BAND_COEFFICIENT)`, so dragging `100px` past a
/// boundary moves the visible content by `55px`.
///
/// # Examples
///
/// ```
/// use martensite_motion::RUBBER_BAND_COEFFICIENT;
///
/// assert_eq!(RUBBER_BAND_COEFFICIENT, 0.55);
/// assert_eq!(100.0_f32 * RUBBER_BAND_COEFFICIENT, 55.0);
/// ```
pub const RUBBER_BAND_COEFFICIENT: f32 = 0.55_f32;

/// Position displacement (in pixels) below which the spring-back is considered
/// numerically settled and snaps exactly to the boundary.
const SETTLE_POSITION_EPS: f32 = 0.5;

/// Velocity magnitude (in pixels/second) below which the spring-back is
/// considered numerically settled.
const SETTLE_VELOCITY_EPS: f32 = 10.0;

/// Axis-lock movement threshold in pixels. The first drag whose primary-axis
/// component exceeds this value establishes the lock direction.
const AXIS_LOCK_THRESHOLD: f32 = 2.0;

/// Returns a critically-damped spring configuration tuned for a ~300ms
/// perceptual settle. `ζ = 1.0` with `ω₀ = √300 ≈ 17.32 rad/s`.
fn rubber_band_spring_config() -> SpringConfig {
    // `2·√(k·m)` with `m = 1` and `k = 300` gives critical damping.
    SpringConfig::new(1.0, 300.0, 2.0 * (300.0_f32).sqrt())
        .expect("rubber-band spring config is statically valid")
}

/// The directional axis lock established during a 2D drag.
///
/// Once a drag establishes a primary axis, cross-axis movement is suppressed
/// until the drag ends (see [`RubberBandScroller2D::release`]).
///
/// # Examples
///
/// ```
/// use martensite_motion::AxisLock;
///
/// let lock = AxisLock::None;
/// assert_eq!(lock, AxisLock::None);
/// ```
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AxisLock {
    /// No axis has been locked yet (the drag has not established a direction).
    None,
    /// Movement is constrained to the horizontal (x) axis.
    Horizontal,
    /// Movement is constrained to the vertical (y) axis.
    Vertical,
}

/// Maps a raw (possibly out-of-bounds) offset through the rubber-band formula.
///
/// In-bounds offsets pass through unchanged; the out-of-bounds portion is
/// scaled by [`RUBBER_BAND_COEFFICIENT`].
#[inline]
fn rubber_band_map(offset: f32, min: f32, max: f32) -> f32 {
    if offset < min {
        min + (offset - min) * RUBBER_BAND_COEFFICIENT
    } else if offset > max {
        max + (offset - max) * RUBBER_BAND_COEFFICIENT
    } else {
        offset
    }
}

/// Single-axis iOS-style rubber-band overscroll physics.
///
/// A `RubberBandScroller` tracks a logical [`content_offset`](Self::content_offset)
/// within `[min_offset, max_offset]`. While dragging past a boundary the
/// visible position stretches according to [`RUBBER_BAND_COEFFICIENT`]; on
/// [`release`](Self::release) an analytical spring drives the offset back to
/// the nearest boundary.
///
/// The type is pure physics — it has no widget or rendering dependencies — and
/// all per-frame methods are `O(1)` with zero heap allocation.
///
/// # Examples
///
/// ```
/// use martensite_motion::RubberBandScroller;
///
/// let mut scroller = RubberBandScroller::new(1000.0, 500.0);
/// // Dragging past the top boundary stretches the visible offset.
/// scroller.drag(-100.0);
/// assert_eq!(scroller.visible_offset(), -55.0);
/// ```
#[derive(Debug, Clone, Copy)]
pub struct RubberBandScroller {
    /// The logical scroll position (raw, may be out of bounds while dragging).
    content_offset: f32,
    /// Total content size in pixels.
    content_size: f32,
    /// Visible viewport size in pixels.
    viewport_size: f32,
    /// Most recent drag delta, used as a velocity estimate while dragging.
    drag_velocity: f32,
    /// Active spring-back solver, if any.
    spring: Option<SpringSolver>,
    /// Offset at which the current spring started.
    spring_start: f32,
    /// Boundary the current spring is converging towards.
    spring_target: f32,
}

impl RubberBandScroller {
    /// Creates a new scroller with the given content and viewport sizes.
    ///
    /// The initial [`content_offset`](Self::content_offset) is `0.0` (the
    /// minimum boundary) and no spring is active.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite_motion::RubberBandScroller;
    ///
    /// let scroller = RubberBandScroller::new(1000.0, 500.0);
    /// assert_eq!(scroller.content_offset(), 0.0);
    /// assert_eq!(scroller.max_offset(), 500.0);
    /// assert!(scroller.is_settled());
    /// ```
    #[inline]
    pub fn new(content_size: f32, viewport_size: f32) -> Self {
        Self {
            content_offset: 0.0,
            content_size,
            viewport_size,
            drag_velocity: 0.0,
            spring: None,
            spring_start: 0.0,
            spring_target: 0.0,
        }
    }

    /// Sets the total content size in pixels.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite_motion::RubberBandScroller;
    ///
    /// let mut scroller = RubberBandScroller::new(1000.0, 500.0);
    /// scroller.set_content_size(2000.0);
    /// assert_eq!(scroller.max_offset(), 1500.0);
    /// ```
    #[inline]
    pub fn set_content_size(&mut self, size: f32) {
        self.content_size = size;
    }

    /// Sets the visible viewport size in pixels.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite_motion::RubberBandScroller;
    ///
    /// let mut scroller = RubberBandScroller::new(1000.0, 500.0);
    /// scroller.set_viewport_size(800.0);
    /// assert_eq!(scroller.max_offset(), 200.0);
    /// ```
    #[inline]
    pub fn set_viewport_size(&mut self, size: f32) {
        self.viewport_size = size;
    }

    /// Returns the minimum scrollable offset (`0.0`).
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite_motion::RubberBandScroller;
    ///
    /// let scroller = RubberBandScroller::new(1000.0, 500.0);
    /// assert_eq!(scroller.min_offset(), 0.0);
    /// ```
    #[inline]
    pub fn min_offset(&self) -> f32 {
        0.0
    }

    /// Returns the maximum scrollable offset: `(content_size - viewport_size).max(0.0)`.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite_motion::RubberBandScroller;
    ///
    /// let scroller = RubberBandScroller::new(1000.0, 500.0);
    /// assert_eq!(scroller.max_offset(), 500.0);
    ///
    /// // Content smaller than the viewport clamps to zero.
    /// let small = RubberBandScroller::new(200.0, 500.0);
    /// assert_eq!(small.max_offset(), 0.0);
    /// ```
    #[inline]
    pub fn max_offset(&self) -> f32 {
        (self.content_size - self.viewport_size).max(0.0)
    }

    /// Applies a drag delta to the scroll position.
    ///
    /// The raw [`content_offset`](Self::content_offset) is advanced by `delta`.
    /// If a spring-back is currently active it is cancelled first, continuing
    /// from the spring's current animated position so drags interrupt the
    /// spring-back smoothly. The visible effect of overscrolling is applied
    /// lazily by [`visible_offset`](Self::visible_offset) via the rubber-band
    /// formula. The most recent `delta` is recorded as
    /// [`drag_velocity`](Self::drag_velocity).
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite_motion::RubberBandScroller;
    ///
    /// let mut scroller = RubberBandScroller::new(1000.0, 500.0);
    /// scroller.drag(200.0);
    /// assert_eq!(scroller.content_offset(), 200.0);
    /// // Dragging past the top boundary stretches the visible offset.
    /// scroller.drag(-300.0);
    /// assert_eq!(scroller.visible_offset(), -55.0);
    /// ```
    #[inline]
    pub fn drag(&mut self, delta: f32) {
        // Interrupt any active spring-back, continuing from its current position.
        if let Some(spring) = self.spring.take() {
            self.content_offset = spring.position();
        }
        self.content_offset += delta;
        self.drag_velocity = delta;
    }

    /// Releases the drag, optionally starting a spring-back to the boundary.
    ///
    /// If the offset is overscrolled (out of bounds), a critically-damped
    /// [`SpringSolver`] is started from the current offset towards the nearest
    /// boundary, using `velocity` (in pixels/second) as the initial condition.
    /// If the offset is within bounds no spring is started.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite_motion::RubberBandScroller;
    ///
    /// let mut scroller = RubberBandScroller::new(1000.0, 500.0);
    /// scroller.drag(-100.0);
    /// assert!(!scroller.is_settled());
    /// scroller.release(0.0);
    /// // A spring-back towards the top boundary is now active.
    /// assert!(!scroller.is_settled());
    /// ```
    #[inline]
    pub fn release(&mut self, velocity: f32) {
        let min = self.min_offset();
        let max = self.max_offset();

        if self.content_offset < min {
            self.spring_start = self.content_offset;
            self.spring_target = min;
            self.spring = Some(SpringSolver::new(
                rubber_band_spring_config(),
                self.content_offset,
                min,
                velocity,
            ));
        } else if self.content_offset > max {
            self.spring_start = self.content_offset;
            self.spring_target = max;
            self.spring = Some(SpringSolver::new(
                rubber_band_spring_config(),
                self.content_offset,
                max,
                velocity,
            ));
        } else {
            // Within bounds: nothing to spring back to.
            self.spring = None;
        }

        self.drag_velocity = 0.0;
    }

    /// Returns the current visible offset.
    ///
    /// If a spring-back is active, the spring is evaluated at its current
    /// elapsed time. Otherwise the rubber-band formula is applied to the raw
    /// [`content_offset`](Self::content_offset): in-bounds offsets pass through
    /// unchanged, while the out-of-bounds portion is scaled by
    /// [`RUBBER_BAND_COEFFICIENT`].
    ///
    /// This method is `O(1)` and performs zero heap allocations.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite_motion::RubberBandScroller;
    ///
    /// let mut scroller = RubberBandScroller::new(1000.0, 500.0);
    /// scroller.drag(250.0);
    /// assert_eq!(scroller.visible_offset(), 250.0);
    /// scroller.drag(-300.0);
    /// assert_eq!(scroller.visible_offset(), -27.5);
    /// ```
    #[inline]
    pub fn visible_offset(&self) -> f32 {
        match self.spring {
            Some(spring) => spring.position(),
            None => rubber_band_map(self.content_offset, self.min_offset(), self.max_offset()),
        }
    }

    /// Returns the raw logical scroll position.
    ///
    /// While a spring-back is active this returns the clamped boundary the
    /// spring is converging towards. While dragging this returns the raw
    /// (possibly out-of-bounds) offset. When settled the stored offset is
    /// already clamped to bounds.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite_motion::RubberBandScroller;
    ///
    /// let mut scroller = RubberBandScroller::new(1000.0, 500.0);
    /// scroller.drag(300.0);
    /// assert_eq!(scroller.content_offset(), 300.0);
    /// ```
    #[inline]
    pub fn content_offset(&self) -> f32 {
        match self.spring {
            Some(_) => self.spring_target,
            None => self.content_offset,
        }
    }

    /// Returns the most recent drag velocity estimate (the last drag delta).
    ///
    /// This is reset to `0.0` when [`release`](Self::release) is called.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite_motion::RubberBandScroller;
    ///
    /// let mut scroller = RubberBandScroller::new(1000.0, 500.0);
    /// scroller.drag(42.0);
    /// assert_eq!(scroller.drag_velocity(), 42.0);
    /// scroller.release(0.0);
    /// assert_eq!(scroller.drag_velocity(), 0.0);
    /// ```
    #[inline]
    pub fn drag_velocity(&self) -> f32 {
        self.drag_velocity
    }

    /// Returns `true` when no spring-back is active and the offset is within
    /// bounds.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite_motion::RubberBandScroller;
    ///
    /// let mut scroller = RubberBandScroller::new(1000.0, 500.0);
    /// assert!(scroller.is_settled());
    /// scroller.drag(-100.0);
    /// assert!(!scroller.is_settled());
    /// ```
    #[inline]
    pub fn is_settled(&self) -> bool {
        self.spring.is_none()
            && self.content_offset >= self.min_offset()
            && self.content_offset <= self.max_offset()
    }

    /// Advances the spring-back by `dt` seconds.
    ///
    /// If the spring has settled (both position and velocity below the perceptual
    /// settle threshold) it is cleared and the offset is clamped exactly to the
    /// boundary. Non-finite or negative `dt` is ignored.
    ///
    /// This method is `O(1)` and performs zero heap allocations.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite_motion::RubberBandScroller;
    ///
    /// let mut scroller = RubberBandScroller::new(1000.0, 500.0);
    /// scroller.drag(-20.0);
    /// scroller.release(0.0);
    /// // Run ~400ms of 60fps frames; the critically-damped spring settles.
    /// for _ in 0..24 {
    ///     scroller.update(1.0 / 60.0);
    /// }
    /// assert!(scroller.is_settled());
    /// ```
    #[inline]
    pub fn update(&mut self, dt: f32) {
        let Some(spring) = self.spring.as_mut() else {
            return;
        };
        spring.advance(dt);
        let (pos, vel) = spring.sample();
        if (pos - self.spring_target).abs() < SETTLE_POSITION_EPS && vel.abs() < SETTLE_VELOCITY_EPS
        {
            self.content_offset = self.spring_target;
            self.spring = None;
        }
    }

    /// Returns the active spring-back solver, if any.
    ///
    /// Intended for inspection and testing.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite_motion::RubberBandScroller;
    ///
    /// let mut scroller = RubberBandScroller::new(1000.0, 500.0);
    /// assert!(scroller.spring().is_none());
    /// scroller.drag(-100.0);
    /// scroller.release(0.0);
    /// assert!(scroller.spring().is_some());
    /// ```
    #[inline]
    pub fn spring(&self) -> Option<&SpringSolver> {
        self.spring.as_ref()
    }
}

/// Two-dimensional iOS-style rubber-band overscroll physics with axis locking.
///
/// Wraps two independent [`RubberBandScroller`] instances (horizontal and
/// vertical) and adds directional axis locking: once a drag establishes a
/// primary axis (the first movement exceeding a 2-pixel threshold
/// significance), cross-axis movement is suppressed until the drag ends via
/// [`release`](Self::release).
///
/// # Examples
///
/// ```
/// use martensite_motion::RubberBandScroller2D;
///
/// let mut scroller = RubberBandScroller2D::new((1000.0, 1000.0), (500.0, 500.0));
/// scroller.drag((200.0, 0.0));
/// let (x, y) = scroller.visible_offset();
/// assert_eq!(x, 200.0);
/// assert_eq!(y, 0.0);
/// ```
#[derive(Debug, Clone, Copy)]
pub struct RubberBandScroller2D {
    /// Horizontal axis scroller.
    x: RubberBandScroller,
    /// Vertical axis scroller.
    y: RubberBandScroller,
    /// Current directional axis lock.
    lock: AxisLock,
}

impl RubberBandScroller2D {
    /// Creates a new 2D scroller with the given content and viewport sizes.
    ///
    /// `content_size` and `viewport_size` are `(width, height)` tuples.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite_motion::RubberBandScroller2D;
    ///
    /// let scroller = RubberBandScroller2D::new((1000.0, 800.0), (500.0, 400.0));
    /// let (min_x, min_y) = scroller.min_offset();
    /// let (max_x, max_y) = scroller.max_offset();
    /// assert_eq!(min_x, 0.0);
    /// assert_eq!(max_x, 500.0);
    /// assert_eq!(max_y, 400.0);
    /// ```
    #[inline]
    pub fn new(content_size: (f32, f32), viewport_size: (f32, f32)) -> Self {
        Self {
            x: RubberBandScroller::new(content_size.0, viewport_size.0),
            y: RubberBandScroller::new(content_size.1, viewport_size.1),
            lock: AxisLock::None,
        }
    }

    /// Sets the total content size as a `(width, height)` tuple.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite_motion::RubberBandScroller2D;
    ///
    /// let mut scroller = RubberBandScroller2D::new((1000.0, 1000.0), (500.0, 500.0));
    /// scroller.set_content_size((2000.0, 1500.0));
    /// assert_eq!(scroller.max_offset(), (1500.0, 1000.0));
    /// ```
    #[inline]
    pub fn set_content_size(&mut self, size: (f32, f32)) {
        self.x.set_content_size(size.0);
        self.y.set_content_size(size.1);
    }

    /// Sets the visible viewport size as a `(width, height)` tuple.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite_motion::RubberBandScroller2D;
    ///
    /// let mut scroller = RubberBandScroller2D::new((1000.0, 1000.0), (500.0, 500.0));
    /// scroller.set_viewport_size((800.0, 900.0));
    /// assert_eq!(scroller.max_offset(), (200.0, 100.0));
    /// ```
    #[inline]
    pub fn set_viewport_size(&mut self, size: (f32, f32)) {
        self.x.set_viewport_size(size.0);
        self.y.set_viewport_size(size.1);
    }

    /// Returns the minimum scrollable offset as `(min_x, min_y)`, both `0.0`.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite_motion::RubberBandScroller2D;
    ///
    /// let scroller = RubberBandScroller2D::new((1000.0, 1000.0), (500.0, 500.0));
    /// assert_eq!(scroller.min_offset(), (0.0, 0.0));
    /// ```
    #[inline]
    pub fn min_offset(&self) -> (f32, f32) {
        (self.x.min_offset(), self.y.min_offset())
    }

    /// Returns the maximum scrollable offset as `(max_x, max_y)`.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite_motion::RubberBandScroller2D;
    ///
    /// let scroller = RubberBandScroller2D::new((1000.0, 800.0), (500.0, 500.0));
    /// assert_eq!(scroller.max_offset(), (500.0, 300.0));
    /// ```
    #[inline]
    pub fn max_offset(&self) -> (f32, f32) {
        (self.x.max_offset(), self.y.max_offset())
    }

    /// Returns the current directional axis lock.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite_motion::{AxisLock, RubberBandScroller2D};
    ///
    /// let mut scroller = RubberBandScroller2D::new((1000.0, 1000.0), (500.0, 500.0));
    /// assert_eq!(scroller.axis_lock(), AxisLock::None);
    /// scroller.drag((50.0, 5.0));
    /// assert_eq!(scroller.axis_lock(), AxisLock::Horizontal);
    /// ```
    #[inline]
    pub fn axis_lock(&self) -> AxisLock {
        self.lock
    }

    /// Applies a `(dx, dy)` drag delta with axis-lock enforcement.
    ///
    /// On the first significant movement (either component exceeding the
    /// threshold) a primary axis is established and cross-axis movement is
    /// suppressed for the remainder of the drag. The lock is cleared on
    /// [`release`](Self::release).
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite_motion::{AxisLock, RubberBandScroller2D};
    ///
    /// let mut scroller = RubberBandScroller2D::new((1000.0, 1000.0), (500.0, 500.0));
    /// // A horizontal-dominant drag locks to the x axis.
    /// scroller.drag((80.0, 5.0));
    /// assert_eq!(scroller.axis_lock(), AxisLock::Horizontal);
    /// // Substantial vertical movement is suppressed.
    /// scroller.drag((20.0, 90.0));
    /// let (x, y) = scroller.visible_offset();
    /// assert_eq!(x, 100.0);
    /// assert_eq!(y, 0.0);
    /// ```
    #[inline]
    pub fn drag(&mut self, delta: (f32, f32)) {
        let (dx, dy) = delta;

        if self.lock == AxisLock::None {
            let adx = dx.abs();
            let ady = dy.abs();
            if adx > AXIS_LOCK_THRESHOLD || ady > AXIS_LOCK_THRESHOLD {
                self.lock = if adx >= ady {
                    AxisLock::Horizontal
                } else {
                    AxisLock::Vertical
                };
            }
        }

        match self.lock {
            AxisLock::None => {
                self.x.drag(dx);
                self.y.drag(dy);
            }
            AxisLock::Horizontal => {
                self.x.drag(dx);
                // Cross-axis (vertical) movement suppressed.
            }
            AxisLock::Vertical => {
                self.y.drag(dy);
                // Cross-axis (horizontal) movement suppressed.
            }
        }
    }

    /// Releases the drag, starting spring-back on any overscrolled axis and
    /// clearing the axis lock.
    ///
    /// `velocity` is a `(vx, vy)` tuple of initial velocities in pixels/second.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite_motion::{AxisLock, RubberBandScroller2D};
    ///
    /// let mut scroller = RubberBandScroller2D::new((1000.0, 1000.0), (500.0, 500.0));
    /// scroller.drag((50.0, 5.0));
    /// scroller.release((0.0, 0.0));
    /// assert_eq!(scroller.axis_lock(), AxisLock::None);
    /// ```
    #[inline]
    pub fn release(&mut self, velocity: (f32, f32)) {
        self.x.release(velocity.0);
        self.y.release(velocity.1);
        self.lock = AxisLock::None;
    }

    /// Returns the current visible offset as `(x, y)`.
    ///
    /// This method is `O(1)` and performs zero heap allocations.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite_motion::RubberBandScroller2D;
    ///
    /// let mut scroller = RubberBandScroller2D::new((1000.0, 1000.0), (500.0, 500.0));
    /// // A horizontal-only drag overscrolls the x axis; y stays at zero.
    /// scroller.drag((-100.0, 0.0));
    /// let (x, y) = scroller.visible_offset();
    /// assert_eq!(x, -55.0);
    /// assert_eq!(y, 0.0);
    /// ```
    #[inline]
    pub fn visible_offset(&self) -> (f32, f32) {
        (self.x.visible_offset(), self.y.visible_offset())
    }

    /// Returns the raw logical scroll position as `(x, y)`.
    ///
    /// See [`RubberBandScroller::content_offset`] for the per-axis semantics.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite_motion::RubberBandScroller2D;
    ///
    /// let mut scroller = RubberBandScroller2D::new((1000.0, 1000.0), (500.0, 500.0));
    /// scroller.drag((300.0, 0.0));
    /// assert_eq!(scroller.content_offset(), (300.0, 0.0));
    /// ```
    #[inline]
    pub fn content_offset(&self) -> (f32, f32) {
        (self.x.content_offset(), self.y.content_offset())
    }

    /// Returns `true` when both axes are settled (no active springs and within
    /// bounds).
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite_motion::RubberBandScroller2D;
    ///
    /// let mut scroller = RubberBandScroller2D::new((1000.0, 1000.0), (500.0, 500.0));
    /// assert!(scroller.is_settled());
    /// scroller.drag((-100.0, 0.0));
    /// assert!(!scroller.is_settled());
    /// ```
    #[inline]
    pub fn is_settled(&self) -> bool {
        self.x.is_settled() && self.y.is_settled()
    }

    /// Advances both axes' spring-backs by `dt` seconds.
    ///
    /// This method is `O(1)` and performs zero heap allocations.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite_motion::RubberBandScroller2D;
    ///
    /// let mut scroller = RubberBandScroller2D::new((1000.0, 1000.0), (500.0, 500.0));
    /// scroller.drag((-20.0, -20.0));
    /// scroller.release((0.0, 0.0));
    /// for _ in 0..24 {
    ///     scroller.update(1.0 / 60.0);
    /// }
    /// assert!(scroller.is_settled());
    /// ```
    #[inline]
    pub fn update(&mut self, dt: f32) {
        self.x.update(dt);
        self.y.update(dt);
    }
}

#[cfg(test)]
mod tests {
    use super::{AxisLock, RubberBandScroller, RubberBandScroller2D};

    /// Dragging past the top boundary by 100px moves the visible offset by 55px.
    #[test]
    fn rubber_band_stretch_coefficient() {
        let mut scroller = RubberBandScroller::new(1000.0, 500.0);
        // Start at the top boundary (offset 0) and drag 100px past it.
        scroller.drag(-100.0);
        let visible = scroller.visible_offset();
        assert!(
            (visible - (-55.0)).abs() < 1e-5,
            "visible offset {visible} should be -55.0 (100 * 0.55)"
        );
    }

    /// After release, the critically-damped spring settles within ~350ms.
    #[test]
    fn rubber_band_spring_back_settles() {
        let mut scroller = RubberBandScroller::new(1000.0, 500.0);
        scroller.drag(-20.0);
        assert!(scroller.visible_offset() < 0.0, "should be overscrolled");
        scroller.release(0.0);
        assert!(scroller.spring().is_some(), "spring-back should be active");

        // ~350ms of 60fps frames.
        for _ in 0..21 {
            scroller.update(1.0 / 60.0);
        }
        assert!(
            scroller.is_settled(),
            "spring should settle within ~350ms (visible = {})",
            scroller.visible_offset()
        );
        assert!(
            scroller.visible_offset().abs() < 0.5,
            "visible offset should be at the boundary"
        );
    }

    /// Releasing with a velocity starts the spring with that initial velocity.
    #[test]
    fn rubber_band_velocity_handoff() {
        let mut scroller = RubberBandScroller::new(1000.0, 500.0);
        scroller.drag(-100.0);
        scroller.release(500.0);

        let spring = scroller
            .spring()
            .expect("spring should be active after release");
        // At t = 0 the spring's velocity equals the handed-off initial velocity.
        assert!(
            (spring.velocity() - 500.0).abs() < 1e-2,
            "spring initial velocity {} should be 500.0",
            spring.velocity()
        );
        assert_eq!(spring.target(), 0.0);
    }

    /// Dragging within bounds does not start a spring on release.
    #[test]
    fn rubber_band_release_within_bounds_no_spring() {
        let mut scroller = RubberBandScroller::new(1000.0, 500.0);
        scroller.drag(200.0);
        scroller.release(0.0);
        assert!(scroller.spring().is_none(), "no spring within bounds");
        assert!(scroller.is_settled());
        assert_eq!(scroller.content_offset(), 200.0);
    }

    /// A horizontal-dominant drag locks to the x axis and suppresses y.
    #[test]
    fn axis_lock_horizontal() {
        let mut scroller = RubberBandScroller2D::new((1000.0, 1000.0), (500.0, 500.0));
        // First drag is horizontal-dominant: establishes a horizontal lock.
        scroller.drag((50.0, 5.0));
        assert_eq!(scroller.axis_lock(), AxisLock::Horizontal);
        // Subsequent large vertical movement is suppressed.
        scroller.drag((30.0, 80.0));
        let (x, y) = scroller.visible_offset();
        assert_eq!(x, 80.0, "x should accumulate horizontal drags");
        assert!(
            y.abs() < 1e-6,
            "y should be suppressed by the horizontal lock (got {y})"
        );
    }

    /// A vertical-dominant drag locks to the y axis and suppresses x.
    #[test]
    fn axis_lock_vertical() {
        let mut scroller = RubberBandScroller2D::new((1000.0, 1000.0), (500.0, 500.0));
        // First drag is vertical-dominant: establishes a vertical lock.
        scroller.drag((5.0, 50.0));
        assert_eq!(scroller.axis_lock(), AxisLock::Vertical);
        // Subsequent large horizontal movement is suppressed.
        scroller.drag((80.0, 30.0));
        let (x, y) = scroller.visible_offset();
        assert_eq!(y, 80.0, "y should accumulate vertical drags");
        assert!(
            x.abs() < 1e-6,
            "x should be suppressed by the vertical lock (got {x})"
        );
    }

    /// The axis lock is cleared on release.
    #[test]
    fn axis_lock_cleared_on_release() {
        let mut scroller = RubberBandScroller2D::new((1000.0, 1000.0), (500.0, 500.0));
        scroller.drag((50.0, 5.0));
        assert_eq!(scroller.axis_lock(), AxisLock::Horizontal);
        scroller.release((0.0, 0.0));
        assert_eq!(scroller.axis_lock(), AxisLock::None);
    }

    /// Small movements below the threshold do not establish a lock.
    #[test]
    fn axis_lock_below_threshold() {
        let mut scroller = RubberBandScroller2D::new((1000.0, 1000.0), (500.0, 500.0));
        scroller.drag((1.0, 1.0));
        assert_eq!(scroller.axis_lock(), AxisLock::None);
        let (x, y) = scroller.visible_offset();
        assert_eq!(x, 1.0);
        assert_eq!(y, 1.0);
    }
}
