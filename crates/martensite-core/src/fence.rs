//! Reader synchronization primitives coordinating concurrent reads with arena compaction.
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{Duration, Instant};

/// Bit mask for the compaction-in-progress flag in the combined fence state.
const COMPACTING: u64 = 1 << 63;
/// Bit mask for the active reader count in the combined fence state.
const READER_MASK: u64 = !COMPACTING;

/// Default lease timeout before forced reader reclamation during compaction (500ms).
pub const DEFAULT_LEASE_TIMEOUT: Duration = Duration::from_millis(500);

/// Lock-free synchronization fence tracking active reader leases during arena compaction.
///
/// The fence packs a compaction flag and a reader-count into a single `AtomicU64`
/// so that acquiring a reader lease and checking the compaction flag are one atomic
/// compare-exchange operation. This eliminates the race where a reader could increment
/// the counter after a compaction phase has already finished waiting for readers.
///
/// Ensures worker threads reading immutable scene snapshots (e.g. `PaintList`) synchronize
/// with the UI thread's memory compaction passes without blocking normal frame execution.
///
/// # Examples
///
/// Acquiring a reader lease increments the active reader count; dropping the guard
/// decrements it back:
///
/// ```
/// use martensite_core::FrameFence;
///
/// let fence = FrameFence::new();
/// assert_eq!(fence.active_readers(), 0);
/// assert_eq!(fence.epoch(), 1);
///
/// {
///     let guard = fence.read();
///     assert_eq!(fence.active_readers(), 1);
///     assert!(guard.is_valid());
///     assert_eq!(guard.epoch(), fence.epoch());
/// } // guard dropped here
///
/// assert_eq!(fence.active_readers(), 0);
/// ```
///
/// Marking compaction start/end toggles the compaction flag without affecting the epoch:
///
/// ```
/// use martensite_core::FrameFence;
///
/// let fence = FrameFence::new();
/// let epoch_before = fence.epoch();
/// fence.mark_compaction_start();
/// assert!(fence.is_compaction_in_progress());
/// fence.mark_compaction_end();
/// assert!(!fence.is_compaction_in_progress());
/// assert_eq!(fence.epoch(), epoch_before);
/// ```
#[derive(Debug)]
pub struct FrameFence {
    /// Combined state: high bit is the compaction-in-progress flag, low 63 bits are
    /// the number of active reader leases.
    state: AtomicU64,
    /// Compaction epoch, monotonically incremented during each compaction pass.
    epoch: AtomicU64,
    /// Maximum allowed lease duration before the fence forcibly reclaims stale reader slots.
    lease_timeout: Duration,
    /// Total count of reader leases reclaimed due to timeout.
    reclaimed_leases: AtomicU64,
}

impl Default for FrameFence {
    fn default() -> Self {
        Self::new()
    }
}

impl FrameFence {
    /// Create a new FrameFence with the default 500ms lease timeout.
    pub const fn new() -> Self {
        Self::with_timeout(DEFAULT_LEASE_TIMEOUT)
    }

    /// Create a new FrameFence with a custom reader lease timeout.
    pub const fn with_timeout(lease_timeout: Duration) -> Self {
        Self {
            state: AtomicU64::new(0),
            epoch: AtomicU64::new(1),
            lease_timeout,
            reclaimed_leases: AtomicU64::new(0),
        }
    }

    /// Acquire an active reader lease guarded by RAII drop semantics.
    ///
    /// Spins briefly if compaction is currently in progress and retries until the
    /// compare-exchange succeeds with the compaction flag cleared.
    #[inline]
    pub fn read(&self) -> FrameGuard<'_> {
        loop {
            let s = self.state.load(Ordering::Acquire);
            if s & COMPACTING != 0 {
                std::hint::spin_loop();
                continue;
            }
            // 63-bit reader count is effectively unbounded for GUI workloads.
            let new_s = s + 1;
            match self
                .state
                .compare_exchange(s, new_s, Ordering::SeqCst, Ordering::Acquire)
            {
                Ok(_) => {
                    let epoch = self.epoch.load(Ordering::Acquire);
                    return FrameGuard {
                        fence: self,
                        epoch,
                        active: true,
                    };
                }
                Err(_) => continue,
            }
        }
    }

    /// Alias for `read()`: acquire a reader lease.
    #[inline(always)]
    pub fn acquire_lease(&self) -> FrameGuard<'_> {
        self.read()
    }

    /// Alias for `read()`: acquire an active reader lease guarded by RAII drop semantics.
    #[inline(always)]
    pub fn acquire(&self) -> FrameGuard<'_> {
        self.read()
    }

    /// Increments the reader lease counter (DDR-0021 compatible API).
    ///
    /// Blocks if a compaction pass is currently active.
    #[inline]
    pub fn begin_frame(&self) {
        loop {
            let s = self.state.load(Ordering::Acquire);
            if s & COMPACTING != 0 {
                std::hint::spin_loop();
                continue;
            }
            match self
                .state
                .compare_exchange(s, s + 1, Ordering::SeqCst, Ordering::Acquire)
            {
                Ok(_) => break,
                Err(_) => continue,
            }
        }
    }

    /// Decrements the reader lease counter (DDR-0021 compatible API).
    #[inline(always)]
    pub fn end_frame(&self) {
        self.state.fetch_sub(1, Ordering::SeqCst);
    }

    /// Spin-waits until all active in-flight readers have completed.
    pub fn wait_for_zero(&self) {
        while self.state.load(Ordering::Acquire) & READER_MASK != 0 {
            std::hint::spin_loop();
        }
    }

    /// Waits for active readers to reach zero, forcibly reclaiming stale leases if timeout occurs.
    ///
    /// Returns the new compaction epoch.
    pub fn wait_for_quiescence(&self) -> u64 {
        let start = Instant::now();
        let mut spins: u32 = 0;

        loop {
            let s = self.state.load(Ordering::Acquire);
            if s & READER_MASK == 0 {
                break self.bump_epoch();
            }

            if start.elapsed() >= self.lease_timeout {
                // Timeout exceeded: force-reclaim stale leases. Because the compaction
                // flag is set, no new readers can enter while we reset the counter.
                let stale = s & READER_MASK;
                self.reclaimed_leases.fetch_add(stale, Ordering::SeqCst);
                // Preserve the compaction flag and zero the reader count.
                self.state.fetch_and(COMPACTING, Ordering::SeqCst);
                tracing::warn!(
                    "FrameFence: reclaimed {} timed-out reader leases at epoch {}",
                    stale,
                    self.epoch.load(Ordering::Acquire)
                );
                return self.bump_epoch();
            }

            spins = spins.wrapping_add(1);
            if spins < 32 {
                std::hint::spin_loop();
            } else {
                std::thread::yield_now();
            }
        }
    }

    /// Atomically increments and returns the compaction epoch.
    pub fn bump_epoch(&self) -> u64 {
        self.epoch.fetch_add(1, Ordering::SeqCst) + 1
    }

    /// Forcibly reclaims all active reader leases on timeout, resetting reader count to 0.
    pub fn reclaim_stale_leases(&self) -> u64 {
        let s = self.state.load(Ordering::Acquire);
        let stale = s & READER_MASK;
        self.reclaimed_leases.fetch_add(stale, Ordering::SeqCst);
        // Preserve the compaction flag and zero the reader count.
        self.state.fetch_and(COMPACTING, Ordering::SeqCst);
        self.bump_epoch();
        stale
    }

    /// Returns the number of currently active reader leases.
    #[inline(always)]
    pub fn active_readers(&self) -> u64 {
        self.state.load(Ordering::Acquire) & READER_MASK
    }

    /// Returns the current compaction epoch.
    #[inline(always)]
    pub fn epoch(&self) -> u64 {
        self.epoch.load(Ordering::Acquire)
    }

    /// Returns the cumulative number of leases reclaimed via timeout expiration.
    #[inline(always)]
    pub fn reclaimed_leases(&self) -> u64 {
        self.reclaimed_leases.load(Ordering::Acquire)
    }

    /// Returns the configured lease timeout.
    #[inline(always)]
    pub fn lease_timeout(&self) -> Duration {
        self.lease_timeout
    }

    /// Returns true if compaction is currently active.
    #[inline(always)]
    pub fn is_compaction_in_progress(&self) -> bool {
        self.state.load(Ordering::Acquire) & COMPACTING != 0
    }

    /// Marks that compaction has begun.
    #[inline(always)]
    pub fn mark_compaction_start(&self) {
        self.state.fetch_or(COMPACTING, Ordering::SeqCst);
    }

    /// Marks that compaction has completed.
    #[inline(always)]
    pub fn mark_compaction_end(&self) {
        self.state.fetch_and(!COMPACTING, Ordering::SeqCst);
    }
}

/// RAII reader guard ensuring reader lease registration is released upon drop.
///
/// # Examples
///
/// Disarming the guard prevents the reader count from being decremented on drop,
/// which is useful when a compactor has already reclaimed the lease:
///
/// ```
/// use martensite_core::FrameFence;
///
/// let fence = FrameFence::new();
/// let mut guard = fence.read();
/// assert_eq!(fence.active_readers(), 1);
/// guard.dismiss();
/// drop(guard);
/// // `dismiss` prevented the drop handler from decrementing, so the
/// // reader count is still 1 — the compactor is responsible for it.
/// assert_eq!(fence.active_readers(), 1);
/// fence.end_frame();
/// assert_eq!(fence.active_readers(), 0);
/// ```
#[derive(Debug)]
pub struct FrameGuard<'a> {
    fence: &'a FrameFence,
    epoch: u64,
    active: bool,
}

impl<'a> FrameGuard<'a> {
    /// Retrieve the epoch under which this reader lease was granted.
    #[inline(always)]
    pub fn epoch(&self) -> u64 {
        self.epoch
    }

    /// Check whether this lease is still valid (i.e. has not been force-expired by compaction).
    #[inline(always)]
    pub fn is_valid(&self) -> bool {
        self.active && self.fence.epoch.load(Ordering::Acquire) == self.epoch
    }

    /// Disarm the guard so dropping it does not decrement the active reader count.
    #[inline(always)]
    pub fn dismiss(&mut self) {
        self.active = false;
    }
}

impl<'a> Drop for FrameGuard<'a> {
    fn drop(&mut self) {
        if !self.active {
            return;
        }
        self.active = false;
        // Only decrement the reader count if the epoch has not changed. If the
        // compactor reclaimed this lease via timeout, the count was reset to zero
        // and the epoch bumped; decrementing would corrupt the counter.
        if self.fence.epoch.load(Ordering::Acquire) == self.epoch {
            self.fence.state.fetch_sub(1, Ordering::SeqCst);
        }
    }
}
