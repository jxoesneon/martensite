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

/// The maximum number of recovery attempts before the machine gives up and
/// falls back to the CPU rasterizer, regardless of elapsed time.
pub const DEFAULT_MAX_RETRIES: u32 = 8;

/// The cumulative time the machine is allowed to spend in the
/// [`DeviceStatus::SuspendedWithRetry`] state before falling back to the CPU
/// rasterizer. Matches the milestone specification of 32 milliseconds.
pub const DEFAULT_FALLBACK_THRESHOLD: Duration = Duration::from_millis(32);

/// The recovery budget: the total time from device loss to a successful
/// repaint must be below this duration to satisfy the exit gate. Matches the
/// milestone specification of 16.6 milliseconds (one 60 Hz frame).
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
    #[must_use]
    pub fn is_device_loss(self) -> bool {
        matches!(self, Self::Lost | Self::DeviceRemoved)
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
    #[must_use]
    pub fn is_active(&self) -> bool {
        matches!(self, Self::Active)
    }

    /// Returns `true` when the machine is in the [`DeviceStatus::FallbackCpu`]
    /// state.
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
    #[must_use]
    pub fn new() -> Self {
        Self::with_policy(DEFAULT_MAX_RETRIES, DEFAULT_FALLBACK_THRESHOLD)
    }

    /// Creates a new recovery machine with explicit policy parameters.
    ///
    /// `max_retries` caps the number of retry attempts; `fallback_threshold` is
    /// the cumulative time allowed in the suspended state before falling back
    /// to the CPU rasterizer.
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
    #[must_use]
    pub fn status(&self) -> &DeviceStatus {
        &self.status
    }

    /// Returns `true` when the machine is in the [`DeviceStatus::Active`]
    /// state.
    #[must_use]
    pub fn is_active(&self) -> bool {
        self.status.is_active()
    }

    /// Returns `true` when the machine is in the [`DeviceStatus::FallbackCpu`]
    /// state and the CPU software rasterizer should be used.
    #[must_use]
    pub fn is_fallback_cpu(&self) -> bool {
        self.status.is_fallback_cpu()
    }

    /// Returns the duration of the most recently completed recovery, if any.
    ///
    /// A recovery is "completed" when the machine transitions back to
    /// [`DeviceStatus::Active`] via [`RecoveryMachine::repaint_completed`].
    #[must_use]
    pub fn last_recovery_duration(&self) -> Option<Duration> {
        self.last_recovery_duration
    }

    /// Returns `true` when the most recent recovery completed within the
    /// [`RECOVERY_BUDGET`] (16.6 milliseconds), satisfying the exit gate.
    ///
    /// Returns `true` when no recovery has occurred yet.
    #[must_use]
    pub fn last_recovery_within_budget(&self) -> bool {
        self.last_recovery_duration
            .map(|d| d <= RECOVERY_BUDGET)
            .unwrap_or(true)
    }

    /// Handles an observed surface or device error.
    ///
    /// When the machine is [`DeviceStatus::Active`], this transitions to
    /// [`DeviceStatus::DeviceLost`] and records the start of the recovery
    /// interval. When already in a recovery state, the error is ignored (the
    /// machine is already handling the loss).
    pub fn handle_surface_error(&mut self, error: SurfaceError) {
        self.handle_surface_error_at(error, Instant::now());
    }

    /// [`RecoveryMachine::handle_surface_error`] with an explicit clock for
    /// deterministic testing.
    pub fn handle_surface_error_at(&mut self, error: SurfaceError, now: Instant) {
        if self.status.is_active() {
            self.recovery_start = Some(now);
            self.status = DeviceStatus::DeviceLost {
                error,
                timestamp: now,
            };
        }
    }

    /// Begins the retry phase: drops the swapchain and pipelines and schedules
    /// the first retry attempt.
    ///
    /// Transitions [`DeviceStatus::DeviceLost`] →
    /// [`DeviceStatus::SuspendedWithRetry`] with `attempts = 1` and
    /// `next_retry = now + backoff_duration(1)`.
    pub fn begin_retry(&mut self) {
        self.begin_retry_at(Instant::now());
    }

    /// [`RecoveryMachine::begin_retry`] with an explicit clock for
    /// deterministic testing.
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
    pub fn retry_failed(&mut self) {
        self.retry_failed_at(Instant::now());
    }

    /// [`RecoveryMachine::retry_failed`] with an explicit clock for
    /// deterministic testing.
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
    pub fn repaint_completed(&mut self) {
        self.repaint_completed_at(Instant::now());
    }

    /// [`RecoveryMachine::repaint_completed`] with an explicit clock for
    /// deterministic testing.
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
            .finish()
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
        m.handle_surface_error_at(SurfaceError::Timeout, t0 + Duration::from_millis(1));
        assert_eq!(
            m.status(),
            &DeviceStatus::DeviceLost {
                error: SurfaceError::Lost,
                timestamp: t0,
            }
        );
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
}
