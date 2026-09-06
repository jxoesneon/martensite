# Detailed Design Record: DDR-0021
## Title: Lock-Free Arena Reads During Compaction

### 1. Architectural Role & Invariants
This document defines the safe concurrent read protocol for `WidgetArena` during compaction phases. Martensite relies on snapshot-based parallelism rather than complex epoch-based garbage collection (EBR) or hazard pointers.
* **Invariant 1.1**: The `WidgetArena` itself is strictly `!Send` and `!Sync` and resides entirely on the main UI thread. Render and layout worker threads receive read-only, deeply immutable snapshots (e.g., `PaintList`).
* **Invariant 1.2**: Arena compaction must **never** execute while a frame is currently in-flight on any worker thread.
* **Invariant 1.3**: The synchronization of compaction is strictly managed by a lock-free `FrameFence` counter, ensuring zero overhead during normal frame submission.

---

### 2. The `FrameFence` Synchronization Primitive

The `FrameFence` tracks the number of in-flight frames currently being processed by worker threads or the GPU.

```rust
use std::sync::atomic::{AtomicU64, Ordering};

/// A lock-free synchronization primitive for coordinating arena compaction.
pub struct FrameFence {
    /// Tracks the number of actively processing frames.
    in_flight_count: AtomicU64,
}

impl FrameFence {
    pub const fn new() -> Self {
        Self {
            in_flight_count: AtomicU64::new(0),
        }
    }

    /// Called by the main thread immediately before dispatching a frame to workers.
    #[inline(always)]
    pub fn begin_frame(&self) {
        self.in_flight_count.fetch_add(1, Ordering::Acquire);
    }

    /// Called by the worker thread (or GPU callback) upon frame completion.
    #[inline(always)]
    pub fn end_frame(&self) {
        self.in_flight_count.fetch_sub(1, Ordering::Release);
    }

    /// Blocks the current thread until all in-flight frames have completed.
    /// Used exclusively by the arena compactor.
    pub fn wait_for_zero(&self) {
        // Spin loop with exponential backoff / thread yielding
        while self.in_flight_count.load(Ordering::Acquire) > 0 {
            std::hint::spin_loop();
        }
    }
}
```

---

### 3. Two-Phase Compaction Protocol

Martensite rejects epoch-based reclamation (EBR) due to its unnecessary complexity for GUI workloads. Instead, Martensite uses a single-arena-thread model with snapshot-based parallelism. The two-phase compaction protocol guarantees absolute memory safety without locking:

1. **Phase 1: Quiescence Wait**
   The compactor calls `FrameFence::wait_for_zero()`. Because `WidgetArena` is constrained to the main thread, no new frames can be initiated while the main thread is blocked waiting for quiescence.
2. **Phase 2: Compaction**
   Once `in_flight_count == 0`, the compactor executes an O(1) swap-remove logic (as defined in DDR-0001). Since there are no outstanding read-only snapshots (`PaintList` instances) accessing the arena memory, the mutation is provably safe and free of data races.

---

### 4. Send + Sync Safety Proof

Because `WidgetArena` is `!Send + !Sync`, Rust's compiler guarantees it cannot be shared across threads. Worker threads only operate on `PaintList` snapshots which contain structurally unlinked, flattened data required for the frame. The `FrameFence` guarantees temporally that the underlying arena memory from which a `PaintList` was derived will not be re-ordered or deallocated until that `PaintList` is dropped at the end of the frame lifecycle.
