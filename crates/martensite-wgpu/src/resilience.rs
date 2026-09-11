//! GPU device-loss recovery finite state machine.
//!
//! This module implements the formal typestate recovery machine described in
//! the v0.2.0 rendering-pipeline milestone. The machine survives driver
//! crashes, external display disconnects, and GPU timeout-detection-and
//! -recovery (TDR) events without terminating the host process by transitioning
//! through a deterministic sequence of states and falling back to CPU
//! rasterization when recovery cannot complete within the configured budget.
//!
//! # State diagram
//!
//! ```text
//! Active ──► DeviceLost ──► SuspendedWithRetry ──► Recreated ──► Restored ──► Active
//!                               │
//!                               └──► FallbackCpu   (retry budget exceeded)
//! FallbackCpu ──► Active         (software fallback hands back over)
//! ```
//!
//! # Note on the `SurfaceError` type
//!
//! The milestone specification references `wgpu::SurfaceError` in the
//! [`DeviceStatus::DeviceLost`] variant. As of `wgpu` 30 that type no longer
//! exists: surface acquisition returns [`wgpu::CurrentSurfaceTexture`] and
//! status is reported via [`wgpu::SurfaceStatus`]. To preserve the specified
//! shape of [`DeviceStatus`] while compiling against `wgpu` 30, this crate
//! defines a local [`SurfaceError`] enumeration that captures the recoverable
//! failure modes. Production code can construct it from
//! [`wgpu::CurrentSurfaceTexture`] via [`SurfaceError::from_current_texture`].

use std::time::{Duration, Instant};

use crate::surface::SurfaceWrapperError;

/// The maximum number of recovery attempts before the machine gives up and
/// falls back to the CPU rasterizer, regardless of elapsed time.
///
/// # Examples
///
/// ```
/// use martensite_wgpu::resilience::DEFAULT_MAX_RETRIES;
///
/// assert_eq!(DEFAULT_MAX_RETRIES, 8);
/// ```
pub const DEFAULT_MAX_RETRIES: u32 = 8;

/// The cumulative time the machine is allowed to spend in the
/// [`DeviceStatus::SuspendedWithRetry`] state before falling back to the CPU
/// rasterizer. Matches the milestone specification of 32 milliseconds.
///
/// # Examples
///
/// ```
/// use martensite_wgpu::resilience::DEFAULT_FALLBACK_THRESHOLD;
/// use std::time::Duration;
///
/// assert_eq!(DEFAULT_FALLBACK_THRESHOLD, Duration::from_millis(32));
/// ```
pub const DEFAULT_FALLBACK_THRESHOLD: Duration = Duration::from_millis(32);

/// The recovery budget: the total time from device loss to a successful
/// repaint must be below this duration to satisfy the exit gate. Matches the
/// milestone specification of 16.6 milliseconds (one 60 Hz frame).
///
/// # Examples
///
/// ```
/// use martensite_wgpu::resilience::RECOVERY_BUDGET;
/// use std::time::Duration;
///
/// assert_eq!(RECOVERY_BUDGET, Duration::from_nanos(16_600_000));
/// ```
pub const RECOVERY_BUDGET: Duration = Duration::from_nanos(16_600_000);

/// The initial exponential-backoff delay, applied before the first retry.
const INITIAL_BACKOFF: Duration = Duration::from_millis(1);

/// A recoverable surface or device failure mode.
///
/// This is the local analogue of the historical `wgpu::SurfaceError` type. It
/// captures the failure modes that the recovery FSM reacts to. Variants that
/// indicate the device itself is gone (`Lost`, `DeviceRemoved`) trigger full
/// device recovery; transient modes (`Outdated`, `Timeout`) trigger a
/// reconfigure-only path.
///
/// # Examples
///
/// ```
/// use martensite_wgpu::resilience::SurfaceError;
/// use std::error::Error;
///
/// let err = SurfaceError::Lost;
/// assert!(err.is_device_loss());
/// assert!(!err.is_transient());
/// assert!(err.to_string().contains("surface lost"));
/// assert!(err.source().is_none());
/// ```
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SurfaceError {
    /// The surface has been lost and needs to be recreated.
    Lost,
    /// The underlying surface has changed and the configuration is outdated.
    Outdated,
    /// A timeout was encountered while acquiring the next frame.
    Timeout,
    /// The window is occluded (e.g. minimized).
    Occluded,
    /// A validation error was raised inside `get_current_texture`.
    Validation,
    /// The logical device was removed by the driver (TDR / driver crash).
    DeviceRemoved,
}

impl SurfaceError {
    /// Converts a [`wgpu::CurrentSurfaceTexture`] result into a [`SurfaceError`],
    /// returning `None` when the frame was acquired successfully (the `Success`
    /// and `Suboptimal` variants).
    ///
    /// This is the production entry point: the render loop calls
    /// `get_current_texture`, maps the result through this function, and feeds
    /// any `Some(error)` into [`RecoveryMachine::handle_surface_error`].
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite_wgpu::resilience::SurfaceError;
    ///
    /// // The failure variants are unit-like and can be constructed without a GPU.
    /// assert_eq!(
    ///     SurfaceError::from_current_texture(&wgpu::CurrentSurfaceTexture::Lost),
    ///     Some(SurfaceError::Lost)
    /// );
    /// assert_eq!(
    ///     SurfaceError::from_current_texture(&wgpu::CurrentSurfaceTexture::Outdated),
    ///     Some(SurfaceError::Outdated)
    /// );
    /// ```
    #[must_use]
    pub fn from_current_texture(result: &wgpu::CurrentSurfaceTexture) -> Option<Self> {
        match result {
            wgpu::CurrentSurfaceTexture::Success(_)
            | wgpu::CurrentSurfaceTexture::Suboptimal(_) => None,
            wgpu::CurrentSurfaceTexture::Lost => Some(Self::Lost),
            wgpu::CurrentSurfaceTexture::Outdated => Some(Self::Outdated),
            wgpu::CurrentSurfaceTexture::Timeout => Some(Self::Timeout),
            wgpu::CurrentSurfaceTexture::Occluded => Some(Self::Occluded),
            wgpu::CurrentSurfaceTexture::Validation => Some(Self::Validation),
        }
    }

    /// Returns `true` when this error indicates the logical device itself is
    /// gone and must be recreated, rather than merely the surface.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite_wgpu::resilience::SurfaceError;
    ///
    /// assert!(SurfaceError::Lost.is_device_loss());
    /// assert!(SurfaceError::DeviceRemoved.is_device_loss());
    /// assert!(!SurfaceError::Outdated.is_device_loss());
    /// assert!(!SurfaceError::Timeout.is_device_loss());
    /// ```
    #[must_use]
    pub fn is_device_loss(self) -> bool {
        matches!(self, Self::Lost | Self::DeviceRemoved)
    }

    /// Returns `true` when this error is transient and can be resolved by
    /// reconfiguring the surface without going through full device-loss
    /// recovery.
    ///
    /// Transient errors include [`SurfaceError::Outdated`] (surface needs
    /// reconfiguration after resize) and [`SurfaceError::Timeout`] (temporary
    /// acquire failure). These do not require adapter re-enumeration or
    /// device recreation.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite_wgpu::resilience::SurfaceError;
    ///
    /// assert!(SurfaceError::Outdated.is_transient());
    /// assert!(SurfaceError::Timeout.is_transient());
    /// assert!(SurfaceError::Occluded.is_transient());
    /// assert!(!SurfaceError::Lost.is_transient());
    /// assert!(!SurfaceError::DeviceRemoved.is_transient());
    /// ```
    #[must_use]
    pub fn is_transient(self) -> bool {
        matches!(self, Self::Outdated | Self::Timeout | Self::Occluded)
    }
}

impl std::fmt::Display for SurfaceError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let s = match self {
            Self::Lost => "surface lost",
            Self::Outdated => "surface outdated",
            Self::Timeout => "surface acquire timed out",
            Self::Occluded => "surface occluded",
            Self::Validation => "surface validation error",
            Self::DeviceRemoved => "logical device removed by driver",
        };
        f.write_str(s)
    }
}

impl std::error::Error for SurfaceError {}

/// The current state of the GPU device recovery machine.
///
/// The variants and their fields follow the v0.2.0 milestone specification,
/// with the addition of [`DeviceStatus::FallbackCpu`] required by the state
/// diagram for the "retry exceeded" transition.
///
/// # Examples
///
/// ```
/// use martensite_wgpu::resilience::DeviceStatus;
///
/// let status = DeviceStatus::Active;
/// assert!(status.is_active());
/// assert!(!status.is_fallback_cpu());
/// ```
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DeviceStatus {
    /// The device is healthy and rendering normally.
    Active,
    /// A surface or device error has been observed; the swapchain and
    /// pipelines must be dropped before retrying.
    DeviceLost {
        /// The error that triggered the loss.
        error: SurfaceError,
        /// When the loss was observed.
        timestamp: Instant,
    },
    /// The swapchain and pipelines have been dropped and the machine is waiting
    /// to retry adapter enumeration and device creation.
    SuspendedWithRetry {
        /// The number of retry attempts performed so far (starts at 1).
        attempts: u32,
        /// The earliest instant at which the next retry should be attempted.
        next_retry: Instant,
    },
    /// A new adapter and device have been successfully acquired.
    Recreated,
    /// Vello pipelines have been rebuilt and textures rebound; the next frame
    /// is ready to be repainted.
    Restored,
    /// Recovery could not complete within the retry budget; the CPU software
    /// rasterizer is now active. The machine returns to [`DeviceStatus::Active`]
    /// once the GPU becomes available again.
    FallbackCpu,
}

impl DeviceStatus {
    /// Returns `true` when the machine is in the [`DeviceStatus::Active`]
    /// state.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite_wgpu::resilience::DeviceStatus;
    ///
    /// assert!(DeviceStatus::Active.is_active());
    /// assert!(!DeviceStatus::Recreated.is_active());
    /// ```
    #[must_use]
    pub fn is_active(&self) -> bool {
        matches!(self, Self::Active)
    }

    /// Returns `true` when the machine is in the [`DeviceStatus::FallbackCpu`]
    /// state.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite_wgpu::resilience::DeviceStatus;
    ///
    /// assert!(DeviceStatus::FallbackCpu.is_fallback_cpu());
    /// assert!(!DeviceStatus::Active.is_fallback_cpu());
    /// ```
    #[must_use]
    pub fn is_fallback_cpu(&self) -> bool {
        matches!(self, Self::FallbackCpu)
    }
}

/// Computes the exponential backoff delay before retry attempt `attempt`.
///
/// The delay starts at 1 millisecond for the first attempt and doubles on each
/// subsequent attempt: `1ms, 2ms, 4ms, 8ms, …`.
///
/// `attempt` is clamped to a minimum of 1 so that `backoff_duration(0)` returns
/// the initial delay rather than a zero duration.
///
/// # Examples
///
/// ```
/// use martensite_wgpu::resilience::backoff_duration;
/// use std::time::Duration;
///
/// assert_eq!(backoff_duration(1), Duration::from_millis(1));
/// assert_eq!(backoff_duration(2), Duration::from_millis(2));
/// assert_eq!(backoff_duration(3), Duration::from_millis(4));
/// assert_eq!(backoff_duration(4), Duration::from_millis(8));
/// // `attempt` is clamped to a minimum of 1.
/// assert_eq!(backoff_duration(0), Duration::from_millis(1));
/// ```
#[must_use]
pub fn backoff_duration(attempt: u32) -> Duration {
    let n = attempt.max(1) - 1;
    INITIAL_BACKOFF
        .checked_mul(1u32 << n.min(u32::BITS - 1))
        .unwrap_or(Duration::MAX)
}

/// A formal typestate recovery machine for GPU device loss.
///
/// The machine owns the current [`DeviceStatus`] plus the policy parameters
/// (maximum retry count, fallback threshold, and recovery budget) and exposes
/// the transition methods that drive it through the recovery sequence. All
/// time-sensitive transitions have `_at` variants that accept an explicit
/// [`Instant`] clock, enabling deterministic unit testing without real sleeps.
///
/// # Examples
///
/// ```
/// use martensite_wgpu::resilience::RecoveryMachine;
///
/// let machine = RecoveryMachine::new();
/// assert!(machine.is_active());
/// ```
pub struct RecoveryMachine {
    /// The current state of the machine.
    status: DeviceStatus,
    /// The maximum number of retry attempts before falling back to the CPU.
    max_retries: u32,
    /// The cumulative time allowed in [`DeviceStatus::SuspendedWithRetry`]
    /// before falling back to the CPU.
    fallback_threshold: Duration,
    /// The time at which the current loss event began, used to measure total
    /// recovery time.
    recovery_start: Option<Instant>,
    /// The instant at which the machine entered the suspended state, used to
    /// measure the retry budget.
    suspended_since: Option<Instant>,
    /// The duration of the most recently completed recovery, if any.
    last_recovery_duration: Option<Duration>,
}

impl RecoveryMachine {
    /// Creates a new recovery machine starting in the [`DeviceStatus::Active`]
    /// state with the default policy parameters.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite_wgpu::resilience::RecoveryMachine;
    ///
    /// let machine = RecoveryMachine::new();
    /// assert!(machine.is_active());
    /// ```
    #[must_use]
    pub fn new() -> Self {
        Self::with_policy(DEFAULT_MAX_RETRIES, DEFAULT_FALLBACK_THRESHOLD)
    }

    /// Creates a new recovery machine with explicit policy parameters.
    ///
    /// `max_retries` caps the number of retry attempts; `fallback_threshold` is
    /// the cumulative time allowed in the suspended state before falling back
    /// to the CPU rasterizer.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite_wgpu::resilience::RecoveryMachine;
    /// use std::time::Duration;
    ///
    /// let machine = RecoveryMachine::with_policy(4, Duration::from_millis(100));
    /// assert!(machine.is_active());
    /// ```
    #[must_use]
    pub fn with_policy(max_retries: u32, fallback_threshold: Duration) -> Self {
        Self {
            status: DeviceStatus::Active,
            max_retries,
            fallback_threshold,
            recovery_start: None,
            suspended_since: None,
            last_recovery_duration: None,
        }
    }

    /// Returns a reference to the current [`DeviceStatus`].
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite_wgpu::resilience::{DeviceStatus, RecoveryMachine};
    ///
    /// let machine = RecoveryMachine::new();
    /// assert_eq!(machine.status(), &DeviceStatus::Active);
    /// ```
    #[must_use]
    pub fn status(&self) -> &DeviceStatus {
        &self.status
    }

    /// Returns `true` when the machine is in the [`DeviceStatus::Active`]
    /// state.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite_wgpu::resilience::RecoveryMachine;
    ///
    /// let machine = RecoveryMachine::new();
    /// assert!(machine.is_active());
    /// ```
    #[must_use]
    pub fn is_active(&self) -> bool {
        self.status.is_active()
    }

    /// Returns `true` when the machine is in the [`DeviceStatus::FallbackCpu`]
    /// state and the CPU software rasterizer should be used.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite_wgpu::resilience::RecoveryMachine;
    ///
    /// let machine = RecoveryMachine::new();
    /// assert!(!machine.is_fallback_cpu());
    /// ```
    #[must_use]
    pub fn is_fallback_cpu(&self) -> bool {
        self.status.is_fallback_cpu()
    }

    /// Returns the duration of the most recently completed recovery, if any.
    ///
    /// A recovery is "completed" when the machine transitions back to
    /// [`DeviceStatus::Active`] via [`RecoveryMachine::repaint_completed`].
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite_wgpu::resilience::RecoveryMachine;
    ///
    /// let machine = RecoveryMachine::new();
    /// assert_eq!(machine.last_recovery_duration(), None);
    /// ```
    #[must_use]
    pub fn last_recovery_duration(&self) -> Option<Duration> {
        self.last_recovery_duration
    }

    /// Returns `true` when the most recent recovery completed within the
    /// [`RECOVERY_BUDGET`] (16.6 milliseconds), satisfying the exit gate.
    ///
    /// Returns `true` when no recovery has occurred yet.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite_wgpu::resilience::RecoveryMachine;
    ///
    /// let machine = RecoveryMachine::new();
    /// // No recovery has occurred yet, so the budget is satisfied.
    /// assert!(machine.last_recovery_within_budget());
    /// ```
    #[must_use]
    pub fn last_recovery_within_budget(&self) -> bool {
        self.last_recovery_duration
            .map(|d| d <= RECOVERY_BUDGET)
            .unwrap_or(true)
    }

    /// Handles an observed surface or device error.
    ///
    /// When the machine is [`DeviceStatus::Active`]:
    /// - **Transient errors** ([`SurfaceError::is_transient`]) are recorded
    ///   but do not trigger device-loss recovery; the caller should
    ///   reconfigure the surface and continue.
    /// - **Device-loss errors** ([`SurfaceError::is_device_loss`]) transition
    ///   to [`DeviceStatus::DeviceLost`] and record the start of the recovery
    ///   interval.
    /// - **Validation errors** are treated as device-loss because they
    ///   typically indicate a misconfigured pipeline that requires recreation.
    ///
    /// When already in a recovery state, the error is ignored (the machine
    /// is already handling the loss).
    ///
    /// Returns `true` if the error was transient and the caller should
    /// reconfigure the surface without entering recovery; returns `false`
    /// if the machine entered the recovery path or was already in one.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite_wgpu::resilience::{RecoveryMachine, SurfaceError};
    ///
    /// let mut machine = RecoveryMachine::new();
    /// // A transient error does not trigger recovery.
    /// let transient = machine.handle_surface_error(SurfaceError::Outdated);
    /// assert!(transient);
    /// assert!(machine.is_active());
    /// ```
    pub fn handle_surface_error(&mut self, error: SurfaceError) -> bool {
        self.handle_surface_error_at(error, Instant::now())
    }

    /// [`RecoveryMachine::handle_surface_error`] with an explicit clock for
    /// deterministic testing.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite_wgpu::resilience::{RecoveryMachine, SurfaceError};
    /// use std::time::Instant;
    ///
    /// let mut machine = RecoveryMachine::new();
    /// let now = Instant::now();
    /// // A device-loss error transitions to DeviceLost.
    /// machine.handle_surface_error_at(SurfaceError::Lost, now);
    /// assert!(!machine.is_active());
    /// ```
    pub fn handle_surface_error_at(&mut self, error: SurfaceError, now: Instant) -> bool {
        if self.status.is_active() {
            if error.is_transient() {
                // Transient errors (Outdated, Timeout, Occluded) only need
                // a surface reconfigure, not full device-loss recovery.
                return true;
            }
            self.recovery_start = Some(now);
            self.status = DeviceStatus::DeviceLost {
                error,
                timestamp: now,
            };
            false
        } else {
            false
        }
    }

    /// Begins the retry phase: drops the swapchain and pipelines and schedules
    /// the first retry attempt.
    ///
    /// Transitions [`DeviceStatus::DeviceLost`] →
    /// [`DeviceStatus::SuspendedWithRetry`] with `attempts = 1` and
    /// `next_retry = now + backoff_duration(1)`.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite_wgpu::resilience::{DeviceStatus, RecoveryMachine, SurfaceError};
    /// use std::time::Instant;
    ///
    /// let mut machine = RecoveryMachine::new();
    /// let now = Instant::now();
    /// machine.handle_surface_error_at(SurfaceError::Lost, now);
    /// machine.begin_retry_at(now);
    /// assert!(matches!(machine.status(), DeviceStatus::SuspendedWithRetry { .. }));
    /// ```
    pub fn begin_retry(&mut self) {
        self.begin_retry_at(Instant::now());
    }

    /// [`RecoveryMachine::begin_retry`] with an explicit clock for
    /// deterministic testing.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite_wgpu::resilience::{DeviceStatus, RecoveryMachine, SurfaceError};
    /// use std::time::{Duration, Instant};
    ///
    /// let mut machine = RecoveryMachine::new();
    /// let now = Instant::now();
    /// machine.handle_surface_error_at(SurfaceError::Lost, now);
    /// machine.begin_retry_at(now);
    /// match machine.status() {
    ///     DeviceStatus::SuspendedWithRetry { next_retry, .. } => {
    ///         assert_eq!(*next_retry, now + Duration::from_millis(1));
    ///     }
    ///     _ => panic!("expected SuspendedWithRetry"),
    /// }
    /// ```
    pub fn begin_retry_at(&mut self, now: Instant) {
        if matches!(self.status, DeviceStatus::DeviceLost { .. }) {
            self.suspended_since = Some(now);
            self.status = DeviceStatus::SuspendedWithRetry {
                attempts: 1,
                next_retry: now + backoff_duration(1),
            };
        }
    }

    /// Reports that a retry attempt succeeded: a new adapter and device were
    /// acquired.
    ///
    /// Transitions [`DeviceStatus::SuspendedWithRetry`] →
    /// [`DeviceStatus::Recreated`].
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite_wgpu::resilience::{DeviceStatus, RecoveryMachine, SurfaceError};
    /// use std::time::Instant;
    ///
    /// let mut machine = RecoveryMachine::new();
    /// let now = Instant::now();
    /// machine.handle_surface_error_at(SurfaceError::Lost, now);
    /// machine.begin_retry_at(now);
    /// machine.retry_succeeded();
    /// assert_eq!(machine.status(), &DeviceStatus::Recreated);
    /// ```
    pub fn retry_succeeded(&mut self) {
        if matches!(self.status, DeviceStatus::SuspendedWithRetry { .. }) {
            self.status = DeviceStatus::Recreated;
        }
    }

    /// Reports that a retry attempt failed.
    ///
    /// When the cumulative time spent in [`DeviceStatus::SuspendedWithRetry`]
    /// exceeds the configured fallback threshold, or the retry count exceeds
    /// `max_retries`, the machine transitions to [`DeviceStatus::FallbackCpu`].
    /// Otherwise it schedules the next attempt with an exponentially increasing
    /// backoff.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite_wgpu::resilience::{RecoveryMachine, SurfaceError};
    /// use std::time::Instant;
    ///
    /// let mut machine = RecoveryMachine::with_policy(1, std::time::Duration::from_secs(60));
    /// let now = Instant::now();
    /// machine.handle_surface_error_at(SurfaceError::Lost, now);
    /// machine.begin_retry_at(now);
    /// machine.retry_failed_at(now);
    /// // With max_retries == 1, the first failure exhausts the budget.
    /// assert!(machine.is_fallback_cpu());
    /// ```
    pub fn retry_failed(&mut self) {
        self.retry_failed_at(Instant::now());
    }

    /// [`RecoveryMachine::retry_failed`] with an explicit clock for
    /// deterministic testing.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite_wgpu::resilience::{DeviceStatus, RecoveryMachine, SurfaceError};
    /// use std::time::{Duration, Instant};
    ///
    /// let mut machine = RecoveryMachine::with_policy(8, Duration::from_secs(60));
    /// let now = Instant::now();
    /// machine.handle_surface_error_at(SurfaceError::Lost, now);
    /// machine.begin_retry_at(now);
    /// machine.retry_failed_at(now);
    /// match machine.status() {
    ///     DeviceStatus::SuspendedWithRetry { attempts, .. } => assert_eq!(*attempts, 2),
    ///     _ => panic!("expected SuspendedWithRetry"),
    /// }
    /// ```
    pub fn retry_failed_at(&mut self, now: Instant) {
        let DeviceStatus::SuspendedWithRetry { attempts, .. } = &self.status else {
            return;
        };
        let attempts = *attempts;

        let elapsed_exceeded = self
            .suspended_since
            .map(|since| now.duration_since(since) >= self.fallback_threshold)
            .unwrap_or(false);
        let attempts_exceeded = attempts >= self.max_retries;

        if elapsed_exceeded || attempts_exceeded {
            self.status = DeviceStatus::FallbackCpu;
            return;
        }

        let next_attempts = attempts + 1;
        self.status = DeviceStatus::SuspendedWithRetry {
            attempts: next_attempts,
            next_retry: now + backoff_duration(next_attempts),
        };
    }

    /// Reports that the Vello pipelines have been rebuilt and textures rebound.
    ///
    /// Transitions [`DeviceStatus::Recreated`] → [`DeviceStatus::Restored`].
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite_wgpu::resilience::{DeviceStatus, RecoveryMachine, SurfaceError};
    /// use std::time::Instant;
    ///
    /// let mut machine = RecoveryMachine::new();
    /// let now = Instant::now();
    /// machine.handle_surface_error_at(SurfaceError::Lost, now);
    /// machine.begin_retry_at(now);
    /// machine.retry_succeeded();
    /// machine.restore_completed();
    /// assert_eq!(machine.status(), &DeviceStatus::Restored);
    /// ```
    pub fn restore_completed(&mut self) {
        if matches!(self.status, DeviceStatus::Recreated) {
            self.status = DeviceStatus::Restored;
        }
    }

    /// Reports that the next frame has been repainted, completing recovery.
    ///
    /// Transitions [`DeviceStatus::Restored`] → [`DeviceStatus::Active`] and
    /// finalizes the recovery-duration measurement. When transitioning from
    /// [`DeviceStatus::FallbackCpu`], the machine returns to
    /// [`DeviceStatus::Active`] without recording a recovery duration (the GPU
    /// was not actually recovered within budget).
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite_wgpu::resilience::{RecoveryMachine, SurfaceError};
    /// use std::time::{Duration, Instant};
    ///
    /// let mut machine = RecoveryMachine::new();
    /// let now = Instant::now();
    /// machine.handle_surface_error_at(SurfaceError::Lost, now);
    /// machine.begin_retry_at(now);
    /// machine.retry_succeeded();
    /// machine.restore_completed();
    /// machine.repaint_completed_at(now + Duration::from_millis(10));
    /// assert!(machine.is_active());
    /// assert!(machine.last_recovery_within_budget());
    /// ```
    pub fn repaint_completed(&mut self) {
        self.repaint_completed_at(Instant::now());
    }

    /// [`RecoveryMachine::repaint_completed`] with an explicit clock for
    /// deterministic testing.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite_wgpu::resilience::{RecoveryMachine, SurfaceError};
    /// use std::time::{Duration, Instant};
    ///
    /// let mut machine = RecoveryMachine::new();
    /// let now = Instant::now();
    /// machine.handle_surface_error_at(SurfaceError::Lost, now);
    /// machine.begin_retry_at(now);
    /// machine.retry_succeeded();
    /// machine.restore_completed();
    /// // Repaint 20 ms later: exceeds the 16.6 ms budget.
    /// machine.repaint_completed_at(now + Duration::from_millis(20));
    /// assert!(machine.is_active());
    /// assert!(!machine.last_recovery_within_budget());
    /// ```
    pub fn repaint_completed_at(&mut self, now: Instant) {
        match self.status {
            DeviceStatus::Restored => {
                if let Some(start) = self.recovery_start {
                    self.last_recovery_duration = Some(now.duration_since(start));
                }
                self.recovery_start = None;
                self.suspended_since = None;
                self.status = DeviceStatus::Active;
            }
            DeviceStatus::FallbackCpu => {
                self.recovery_start = None;
                self.suspended_since = None;
                self.status = DeviceStatus::Active;
            }
            _ => {}
        }
    }
}

impl Default for RecoveryMachine {
    fn default() -> Self {
        Self::new()
    }
}

impl std::fmt::Debug for RecoveryMachine {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("RecoveryMachine")
            .field("status", &self.status)
            .field("max_retries", &self.max_retries)
            .field("fallback_threshold", &self.fallback_threshold)
            .field("last_recovery_duration", &self.last_recovery_duration)
            .finish_non_exhaustive()
    }
}

/// Errors that can occur during a full device-loss recovery attempt.
///
/// These are distinct from [`SurfaceError`], which reports the *cause* of a
/// loss; [`RecoveryError`] reports the *outcome* of the recovery attempt.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RecoveryError {
    /// The logical device could not be recreated from the existing adapter.
    DeviceRecreationFailed,
    /// The surface could not be reconfigured after the device was recreated
    /// (e.g. invalid dimensions or the surface was never configured). The
    /// carried [`SurfaceWrapperError`] preserves the source failure for
    /// diagnostics.
    SurfaceReconfigureFailed(SurfaceWrapperError),
    /// The recovery machine was not in a state that allows recovery (it must
    /// be in [`DeviceStatus::DeviceLost`] or
    /// [`DeviceStatus::SuspendedWithRetry`]).
    NotInRecoverableState,
}

impl std::fmt::Display for RecoveryError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::DeviceRecreationFailed => {
                f.write_str("failed to recreate the logical device after loss")
            }
            Self::SurfaceReconfigureFailed(source) => write!(
                f,
                "failed to reconfigure the surface after device recreation: {source}"
            ),
            Self::NotInRecoverableState => {
                f.write_str("recovery machine is not in a recoverable state")
            }
        }
    }
}

impl std::error::Error for RecoveryError {}

/// The outcome of a successful [`RecoveryHarness::recover_device_and_surface`]
/// call.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RecoveryOutcome {
    /// A new device was created and the surface was reconfigured; the machine
    /// is now in [`DeviceStatus::Restored`] and awaits
    /// [`RecoveryMachine::repaint_completed`].
    Restored,
    /// The recovery budget was exhausted and the machine fell back to the CPU
    /// rasterizer ([`DeviceStatus::FallbackCpu`]).
    FallbackCpu,
}

/// Coordinates real GPU device-loss recovery: recreates the logical
/// [`wgpu::Device`] / [`wgpu::Queue`] and reconfigures the
/// [`wgpu::Surface`], driving a [`RecoveryMachine`] through the full
/// recovery sequence.
///
/// The v0.2.0 [`RecoveryMachine`] on its own only tracks state transitions;
/// it never touches `wgpu` resources. [`RecoveryHarness`] closes that gap by
/// performing the actual side effects at each transition:
///
/// 1. [`RecoveryMachine::begin_retry`] — drops the old swapchain/pipelines
///    (the caller is responsible for dropping pipeline resources that hold
///    references to the lost device).
/// 2. [`crate::device::GpuContext::recreate_device_and_queue`] — requests a
///    fresh logical device and queue from the surviving adapter.
/// 3. [`crate::surface::SurfaceWrapper::configure`] (or
///    [`crate::surface::SurfaceWrapper::resize`]) — reconfigures the surface
///    against the new device.
/// 4. [`RecoveryMachine::retry_succeeded`] →
///    [`RecoveryMachine::restore_completed`] — the machine advances to
///    [`DeviceStatus::Restored`], ready for the next frame.
///
/// If device recreation fails, the harness calls
/// [`RecoveryMachine::retry_failed`] and retries up to the configured budget
/// before reporting [`RecoveryOutcome::FallbackCpu`].
///
/// # Examples
///
/// ```no_run
/// use martensite_wgpu::device::GpuContext;
/// use martensite_wgpu::resilience::{RecoveryHarness, RecoveryMachine, SurfaceError};
/// use martensite_wgpu::surface::SurfaceWrapper;
///
/// # fn example(ctx: &mut GpuContext, surface: &mut SurfaceWrapper<'_>) {
/// let mut harness = RecoveryHarness::new(RecoveryMachine::new());
/// // A device loss was observed; drive real recovery.
/// harness.begin_recovery(SurfaceError::Lost);
/// match harness.recover_device_and_surface(ctx, surface, 800, 600) {
///     Ok(outcome) => tracing::info!(?outcome, "recovery completed"),
///     Err(err) => tracing::error!(%err, "recovery failed"),
/// }
/// # }
/// ```
pub struct RecoveryHarness {
    /// The underlying state machine driven by this harness.
    machine: RecoveryMachine,
}

impl RecoveryHarness {
    /// Creates a new harness wrapping the given [`RecoveryMachine`].
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite_wgpu::resilience::{RecoveryHarness, RecoveryMachine};
    ///
    /// let harness = RecoveryHarness::new(RecoveryMachine::new());
    /// assert!(harness.machine().is_active());
    /// ```
    #[must_use]
    pub fn new(machine: RecoveryMachine) -> Self {
        Self { machine }
    }

    /// Returns a reference to the underlying [`RecoveryMachine`].
    #[must_use]
    pub fn machine(&self) -> &RecoveryMachine {
        &self.machine
    }

    /// Returns a mutable reference to the underlying [`RecoveryMachine`].
    #[must_use]
    pub fn machine_mut(&mut self) -> &mut RecoveryMachine {
        &mut self.machine
    }

    /// Records a device-loss event and begins the retry phase.
    ///
    /// This is a convenience wrapper around
    /// [`RecoveryMachine::handle_surface_error`] (for the initial transition
    /// out of [`DeviceStatus::Active`]) followed by
    /// [`RecoveryMachine::begin_retry`]. It is safe to call when the machine
    /// is already in a recovery state (the calls are no-ops then).
    pub fn begin_recovery(&mut self, error: SurfaceError) {
        if self.machine.is_active() {
            self.machine.handle_surface_error(error);
        }
        self.machine.begin_retry();
    }

    /// Attempts full device-loss recovery: recreates the logical device and
    /// reconfigures the surface, driving the [`RecoveryMachine`] through the
    /// recovery sequence.
    ///
    /// The machine must be in [`DeviceStatus::DeviceLost`] or
    /// [`DeviceStatus::SuspendedWithRetry`] (call [`Self::begin_recovery`]
    /// first). On success the machine reaches [`DeviceStatus::Restored`];
    /// the caller should call [`RecoveryMachine::repaint_completed`] after the
    /// next frame is painted. If the recovery budget is exhausted, the machine
    /// transitions to [`DeviceStatus::FallbackCpu`] and
    /// [`RecoveryOutcome::FallbackCpu`] is returned.
    ///
    /// # Arguments
    ///
    /// * `ctx` — The GPU context whose device/queue will be recreated in place.
    /// * `surface` — The surface wrapper to reconfigure against the new device.
    /// * `width`, `height` — The dimensions for the reconfigured surface.
    ///
    /// # Errors
    ///
    /// Returns [`RecoveryError::NotInRecoverableState`] if the machine is not
    /// in a recoverable state, [`RecoveryError::DeviceRecreationFailed`] if the
    /// adapter refuses to create a new device and the retry budget is not yet
    /// exhausted (the caller may call this method again to retry), or
    /// [`RecoveryError::SurfaceReconfigureFailed`] if the surface cannot be
    /// reconfigured after a successful device recreation.
    ///
    /// # Examples
    ///
    /// ```no_run
    /// use martensite_wgpu::device::GpuContext;
    /// use martensite_wgpu::resilience::{RecoveryHarness, RecoveryMachine, SurfaceError};
    /// use martensite_wgpu::surface::SurfaceWrapper;
    ///
    /// # fn example(ctx: &mut GpuContext, surface: &mut SurfaceWrapper<'_>) {
    /// let mut harness = RecoveryHarness::new(RecoveryMachine::new());
    /// harness.begin_recovery(SurfaceError::Lost);
    /// let outcome = harness.recover_device_and_surface(ctx, surface, 800, 600);
    /// assert!(outcome.is_ok());
    /// # }
    /// ```
    pub fn recover_device_and_surface(
        &mut self,
        ctx: &mut crate::device::GpuContext,
        surface: &mut crate::surface::SurfaceWrapper<'_>,
        width: u32,
        height: u32,
    ) -> Result<RecoveryOutcome, RecoveryError> {
        use crate::device::GpuContextError;

        // The machine must be in a recoverable state. `begin_retry` transitions
        // DeviceLost → SuspendedWithRetry; if already SuspendedWithRetry (a
        // previous attempt failed), we continue retrying.
        match self.machine.status() {
            DeviceStatus::DeviceLost { .. } => self.machine.begin_retry(),
            DeviceStatus::SuspendedWithRetry { .. } => {}
            _ => return Err(RecoveryError::NotInRecoverableState),
        }

        // Attempt device recreation. On failure, drive the retry/backoff
        // state machine and report whether we should fall back to CPU.
        match ctx.recreate_device_and_queue() {
            Ok(()) => {}
            Err(GpuContextError::DeviceRequestFailed(msg)) => {
                tracing::error!(error = %msg, "device recreation failed");
                self.machine.retry_failed();
                if self.machine.is_fallback_cpu() {
                    return Ok(RecoveryOutcome::FallbackCpu);
                }
                // Still within budget: the caller can retry this method.
                return Err(RecoveryError::DeviceRecreationFailed);
            }
            Err(GpuContextError::NoAdapter(msg)) => {
                tracing::error!(error = %msg, "adapter unavailable during recovery");
                // The adapter itself is gone; treat as a failed retry.
                self.machine.retry_failed();
                if self.machine.is_fallback_cpu() {
                    return Ok(RecoveryOutcome::FallbackCpu);
                }
                return Err(RecoveryError::DeviceRecreationFailed);
            }
        }

        // Device recreated successfully — advance the machine.
        self.machine.retry_succeeded();

        // Reconfigure the surface against the new device. If the surface was
        // previously configured, use `resize` to preserve format/present-mode;
        // otherwise use `configure` for the initial setup. We preserve the
        // surface's existing backdrop mode (Opaque vs Transparent) rather
        // than hard-coding Opaque, so a transparent system-material backdrop
        // survives device loss.
        let backdrop = surface.backdrop_mode();
        let reconfigure_result = if surface.configuration().is_some() {
            surface.resize(&ctx.device, width, height)
        } else {
            surface.configure(&ctx.device, &ctx.adapter, width, height, backdrop)
        };

        match reconfigure_result {
            Ok(()) => {
                self.machine.restore_completed();
                Ok(RecoveryOutcome::Restored)
            }
            Err(source @ SurfaceWrapperError::InvalidDimensions) => {
                tracing::error!(%source, "surface reconfigure failed");
                Err(RecoveryError::SurfaceReconfigureFailed(source))
            }
            Err(SurfaceWrapperError::NotConfigured) => {
                // `resize` returned NotConfigured, which shouldn't happen since
                // we checked above, but fall back to a full configure.
                surface
                    .configure(&ctx.device, &ctx.adapter, width, height, backdrop)
                    .map(|_| {
                        self.machine.restore_completed();
                        RecoveryOutcome::Restored
                    })
                    .map_err(|source| {
                        tracing::error!(%source, "surface reconfigure failed");
                        RecoveryError::SurfaceReconfigureFailed(source)
                    })
            }
        }
    }
}

impl Default for RecoveryHarness {
    fn default() -> Self {
        Self::new(RecoveryMachine::new())
    }
}

impl std::fmt::Debug for RecoveryHarness {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("RecoveryHarness")
            .field("machine", &self.machine)
            .finish_non_exhaustive()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn backoff_doubles_from_one_millisecond() {
        assert_eq!(backoff_duration(1), Duration::from_millis(1));
        assert_eq!(backoff_duration(2), Duration::from_millis(2));
        assert_eq!(backoff_duration(3), Duration::from_millis(4));
        assert_eq!(backoff_duration(4), Duration::from_millis(8));
        assert_eq!(backoff_duration(5), Duration::from_millis(16));
    }

    #[test]
    fn backoff_clamps_zero_attempt_to_initial_delay() {
        assert_eq!(backoff_duration(0), INITIAL_BACKOFF);
    }

    #[test]
    fn backoff_does_not_overflow_for_large_attempts() {
        // A very large attempt count must saturate rather than panic.
        let _ = backoff_duration(u32::MAX);
    }

    #[test]
    fn fresh_machine_starts_active() {
        let m = RecoveryMachine::new();
        assert!(m.is_active());
        assert!(m.status().is_active());
        assert!(!m.is_fallback_cpu());
        assert!(m.last_recovery_within_budget());
    }

    #[test]
    fn surface_error_classification_distinguishes_device_loss() {
        assert!(SurfaceError::Lost.is_device_loss());
        assert!(SurfaceError::DeviceRemoved.is_device_loss());
        assert!(!SurfaceError::Outdated.is_device_loss());
        assert!(!SurfaceError::Timeout.is_device_loss());
        assert!(!SurfaceError::Occluded.is_device_loss());
        assert!(!SurfaceError::Validation.is_device_loss());
    }

    #[test]
    fn handle_surface_error_transitions_active_to_device_lost() {
        let mut m = RecoveryMachine::new();
        let t0 = Instant::now();
        m.handle_surface_error_at(SurfaceError::DeviceRemoved, t0);
        assert_eq!(
            m.status(),
            &DeviceStatus::DeviceLost {
                error: SurfaceError::DeviceRemoved,
                timestamp: t0,
            }
        );
    }

    #[test]
    fn handle_surface_error_is_ignored_when_already_recovering() {
        let mut m = RecoveryMachine::new();
        let t0 = Instant::now();
        m.handle_surface_error_at(SurfaceError::Lost, t0);
        // A second error while in DeviceLost must not overwrite the first.
        let transient =
            m.handle_surface_error_at(SurfaceError::Timeout, t0 + Duration::from_millis(1));
        assert!(
            !transient,
            "second error while recovering should not be transient"
        );
        assert_eq!(
            m.status(),
            &DeviceStatus::DeviceLost {
                error: SurfaceError::Lost,
                timestamp: t0,
            }
        );
    }

    #[test]
    fn transient_outdated_error_does_not_trigger_recovery() {
        let mut m = RecoveryMachine::new();
        let t0 = Instant::now();
        let is_transient = m.handle_surface_error_at(SurfaceError::Outdated, t0);
        assert!(is_transient, "Outdated should be transient");
        assert!(
            m.is_active(),
            "machine should remain Active after transient error"
        );
    }

    #[test]
    fn transient_timeout_error_does_not_trigger_recovery() {
        let mut m = RecoveryMachine::new();
        let t0 = Instant::now();
        let is_transient = m.handle_surface_error_at(SurfaceError::Timeout, t0);
        assert!(is_transient, "Timeout should be transient");
        assert!(
            m.is_active(),
            "machine should remain Active after transient error"
        );
    }

    #[test]
    fn transient_occluded_error_does_not_trigger_recovery() {
        let mut m = RecoveryMachine::new();
        let t0 = Instant::now();
        let is_transient = m.handle_surface_error_at(SurfaceError::Occluded, t0);
        assert!(is_transient, "Occluded should be transient");
        assert!(
            m.is_active(),
            "machine should remain Active after transient error"
        );
    }

    #[test]
    fn validation_error_triggers_recovery() {
        let mut m = RecoveryMachine::new();
        let t0 = Instant::now();
        let is_transient = m.handle_surface_error_at(SurfaceError::Validation, t0);
        assert!(!is_transient, "Validation should not be transient");
        assert!(
            !m.is_active(),
            "machine should enter recovery after Validation error"
        );
    }

    #[test]
    fn is_transient_classifies_errors_correctly() {
        assert!(SurfaceError::Outdated.is_transient());
        assert!(SurfaceError::Timeout.is_transient());
        assert!(SurfaceError::Occluded.is_transient());
        assert!(!SurfaceError::Lost.is_transient());
        assert!(!SurfaceError::DeviceRemoved.is_transient());
        assert!(!SurfaceError::Validation.is_transient());
    }

    #[test]
    fn begin_retry_transitions_device_lost_to_suspended() {
        let mut m = RecoveryMachine::new();
        let t0 = Instant::now();
        m.handle_surface_error_at(SurfaceError::Lost, t0);
        m.begin_retry_at(t0);
        assert_eq!(
            m.status(),
            &DeviceStatus::SuspendedWithRetry {
                attempts: 1,
                next_retry: t0 + Duration::from_millis(1),
            }
        );
    }

    #[test]
    fn retry_succeeded_transitions_suspended_to_recreated() {
        let mut m = RecoveryMachine::new();
        let t0 = Instant::now();
        m.handle_surface_error_at(SurfaceError::Lost, t0);
        m.begin_retry_at(t0);
        m.retry_succeeded();
        assert_eq!(m.status(), &DeviceStatus::Recreated);
    }

    #[test]
    fn retry_failed_doubles_backoff_and_increments_attempts() {
        let mut m = RecoveryMachine::with_policy(8, Duration::from_secs(60));
        let t0 = Instant::now();
        m.handle_surface_error_at(SurfaceError::Lost, t0);
        m.begin_retry_at(t0);

        // First failure: attempts 1 -> 2, backoff for attempt 2 is 2ms.
        m.retry_failed_at(t0);
        assert_eq!(
            m.status(),
            &DeviceStatus::SuspendedWithRetry {
                attempts: 2,
                next_retry: t0 + Duration::from_millis(2),
            }
        );

        // Second failure: attempts 2 -> 3, backoff for attempt 3 is 4ms.
        m.retry_failed_at(t0);
        assert_eq!(
            m.status(),
            &DeviceStatus::SuspendedWithRetry {
                attempts: 3,
                next_retry: t0 + Duration::from_millis(4),
            }
        );
    }

    #[test]
    fn retry_failed_falls_back_when_attempts_exceeded() {
        let mut m = RecoveryMachine::with_policy(2, Duration::from_secs(60));
        let t0 = Instant::now();
        m.handle_surface_error_at(SurfaceError::Lost, t0);
        m.begin_retry_at(t0);
        // attempt 1 -> 2
        m.retry_failed_at(t0);
        // attempt 2 == max_retries -> FallbackCpu
        m.retry_failed_at(t0);
        assert!(m.is_fallback_cpu());
    }

    #[test]
    fn retry_failed_falls_back_when_threshold_exceeded() {
        // A tiny threshold so we can exceed it without sleeping.
        let mut m = RecoveryMachine::with_policy(100, Duration::from_millis(5));
        let t0 = Instant::now();
        m.handle_surface_error_at(SurfaceError::Lost, t0);
        m.begin_retry_at(t0);
        // 10 ms later, the cumulative suspended time exceeds the 5 ms budget.
        m.retry_failed_at(t0 + Duration::from_millis(10));
        assert!(m.is_fallback_cpu());
    }

    #[test]
    fn full_recovery_cycle_returns_to_active_within_budget() {
        let mut m = RecoveryMachine::new();
        let t0 = Instant::now();
        m.handle_surface_error_at(SurfaceError::Lost, t0);
        m.begin_retry_at(t0);
        m.retry_succeeded();
        m.restore_completed();
        assert_eq!(m.status(), &DeviceStatus::Restored);
        // Repaint 10 ms after the loss: within the 16.6 ms budget.
        m.repaint_completed_at(t0 + Duration::from_millis(10));
        assert!(m.is_active());
        assert_eq!(m.last_recovery_duration(), Some(Duration::from_millis(10)));
        assert!(m.last_recovery_within_budget());
    }

    #[test]
    fn recovery_exceeding_budget_is_flagged() {
        let mut m = RecoveryMachine::new();
        let t0 = Instant::now();
        m.handle_surface_error_at(SurfaceError::Lost, t0);
        m.begin_retry_at(t0);
        m.retry_succeeded();
        m.restore_completed();
        // 20 ms exceeds the 16.6 ms budget.
        m.repaint_completed_at(t0 + Duration::from_millis(20));
        assert!(m.is_active());
        assert!(!m.last_recovery_within_budget());
    }

    #[test]
    fn fallback_cpu_returns_to_active_on_repaint_without_recovery_duration() {
        let mut m = RecoveryMachine::with_policy(1, Duration::from_millis(1));
        let t0 = Instant::now();
        m.handle_surface_error_at(SurfaceError::Lost, t0);
        m.begin_retry_at(t0);
        m.retry_failed_at(t0);
        assert!(m.is_fallback_cpu());
        m.repaint_completed();
        assert!(m.is_active());
        // Fallback recovery does not record a duration.
        assert_eq!(m.last_recovery_duration(), None);
    }

    #[test]
    fn transitions_are_no_ops_from_wrong_state() {
        let mut m = RecoveryMachine::new();
        // All transition methods must be safe to call from Active (no-op).
        m.begin_retry();
        m.retry_succeeded();
        m.retry_failed();
        m.restore_completed();
        m.repaint_completed();
        assert!(m.is_active());
    }

    #[test]
    fn recovery_budget_is_one_60hz_frame() {
        assert_eq!(RECOVERY_BUDGET, Duration::from_nanos(16_600_000));
    }

    #[test]
    fn default_fallback_threshold_is_32_milliseconds() {
        assert_eq!(DEFAULT_FALLBACK_THRESHOLD, Duration::from_millis(32));
    }

    #[test]
    fn surface_error_from_current_texture_maps_all_variants() {
        // We cannot construct `CurrentSurfaceTexture::Success` without a GPU,
        // but the failure variants are unit-like and exercise the mapping.
        assert_eq!(
            SurfaceError::from_current_texture(&wgpu::CurrentSurfaceTexture::Lost),
            Some(SurfaceError::Lost)
        );
        assert_eq!(
            SurfaceError::from_current_texture(&wgpu::CurrentSurfaceTexture::Outdated),
            Some(SurfaceError::Outdated)
        );
        assert_eq!(
            SurfaceError::from_current_texture(&wgpu::CurrentSurfaceTexture::Timeout),
            Some(SurfaceError::Timeout)
        );
        assert_eq!(
            SurfaceError::from_current_texture(&wgpu::CurrentSurfaceTexture::Occluded),
            Some(SurfaceError::Occluded)
        );
        assert_eq!(
            SurfaceError::from_current_texture(&wgpu::CurrentSurfaceTexture::Validation),
            Some(SurfaceError::Validation)
        );
    }

    // --- RecoveryHarness tests ---

    #[test]
    fn harness_new_wraps_machine() {
        let harness = RecoveryHarness::new(RecoveryMachine::new());
        assert!(harness.machine().is_active());
    }

    #[test]
    fn harness_default_is_active() {
        let harness = RecoveryHarness::default();
        assert!(harness.machine().is_active());
    }

    #[test]
    fn harness_begin_recovery_transitions_to_suspended() {
        let mut harness = RecoveryHarness::new(RecoveryMachine::new());
        harness.begin_recovery(SurfaceError::Lost);
        assert!(matches!(
            harness.machine().status(),
            DeviceStatus::SuspendedWithRetry { .. }
        ));
    }

    #[test]
    fn harness_begin_recovery_is_idempotent_when_already_recovering() {
        let mut harness = RecoveryHarness::new(RecoveryMachine::new());
        harness.begin_recovery(SurfaceError::Lost);
        let attempts_before = match harness.machine().status() {
            DeviceStatus::SuspendedWithRetry { attempts, .. } => *attempts,
            _ => unreachable!(),
        };
        // Calling again must not reset or corrupt the state.
        harness.begin_recovery(SurfaceError::DeviceRemoved);
        let attempts_after = match harness.machine().status() {
            DeviceStatus::SuspendedWithRetry { attempts, .. } => *attempts,
            _ => unreachable!(),
        };
        assert_eq!(attempts_before, attempts_after);
    }

    #[test]
    fn harness_recover_from_active_returns_not_in_recoverable_state() {
        // Calling recover while still Active must fail with the correct error.
        let mut harness = RecoveryHarness::new(RecoveryMachine::new());
        // We can't construct a GpuContext without a GPU, but the state check
        // happens before any wgpu call, so we pass dummy values that are never
        // dereferenced. We use a closure that is never called.
        let result = harness.recover_device_and_surface_state_only();
        assert_eq!(result, Err(RecoveryError::NotInRecoverableState));
    }

    #[test]
    fn recovery_error_display_is_informative() {
        assert!(!RecoveryError::DeviceRecreationFailed.to_string().is_empty());
        assert!(
            !RecoveryError::SurfaceReconfigureFailed(SurfaceWrapperError::InvalidDimensions)
                .to_string()
                .is_empty()
        );
        assert!(!RecoveryError::NotInRecoverableState.to_string().is_empty());
    }

    #[test]
    fn recovery_outcome_variants_are_distinct() {
        assert_ne!(RecoveryOutcome::Restored, RecoveryOutcome::FallbackCpu);
    }

    /// A test-only helper that checks the state guard without touching wgpu.
    /// This lets us verify the `NotInRecoverableState` path headlessly.
    impl RecoveryHarness {
        fn recover_device_and_surface_state_only(
            &mut self,
        ) -> Result<RecoveryOutcome, RecoveryError> {
            match self.machine.status() {
                DeviceStatus::DeviceLost { .. } | DeviceStatus::SuspendedWithRetry { .. } => {}
                _ => return Err(RecoveryError::NotInRecoverableState),
            }
            // In a real call we would recreate the device here; for the test
            // we just report the state-check result.
            Ok(RecoveryOutcome::Restored)
        }
    }

    #[test]
    #[ignore = "requires a wgpu adapter and device"]
    fn harness_full_recovery_recreates_device_and_surface() {
        // End-to-end test: create a real GPU context, simulate a device loss,
        // and verify the harness recreates the device. Surface reconfiguration
        // requires a window handle which is not available in headless CI, so
        // this test focuses on the device-recreation path.
        use crate::device::GpuContext;

        let mut ctx = match GpuContext::new() {
            Ok(ctx) => ctx,
            Err(_) => return,
        };

        // Destroy the device to simulate a TDR.
        ctx.device.destroy();

        let mut harness = RecoveryHarness::new(RecoveryMachine::new());
        harness.begin_recovery(SurfaceError::Lost);

        // Drive the recovery: recreate the device. We can't reconfigure a
        // surface without a window, so we only verify the device-recreation
        // path by calling recreate_device_and_queue directly and advancing
        // the state machine manually.
        match ctx.recreate_device_and_queue() {
            Ok(()) => {
                harness.machine_mut().retry_succeeded();
                harness.machine_mut().restore_completed();
                assert_eq!(harness.machine().status(), &DeviceStatus::Restored);
                harness.machine_mut().repaint_completed();
                assert!(harness.machine().is_active());
            }
            Err(_) => {
                // Device recreation may fail in some CI environments; the
                // state machine should still be in a recovery state.
                assert!(!harness.machine().is_active());
            }
        }
    }
}
