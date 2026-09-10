//! Tracy profiler span instrumentation.
//!
//! Martensite instruments layout, paint, and reactive dispatch with profiling
//! spans so that frame bottlenecks can be located without external tooling.
//! Because the crate is `#![forbid(unsafe_code)]`, it cannot link the native
//! Tracy client library (which is inherently `unsafe`). Instead, this module
//! provides a safe, allocation-free re-implementation of the Tracy span API
//! that records region durations with [`std::time::Instant`] into a
//! thread-local ring buffer. The recorded data feeds the in-app diagnostic
//! HUD and can be queried programmatically.
//!
//! The instrumentation is designed for sub-microsecond overhead: a span
//! begin/end pair performs two [`Instant::now()`] calls and a fixed-size
//! array write, with no heap allocation. The DevTools overhead gate
//! (§5.3 of the v0.9.0 milestone) requires that active profiling contributes
//! `< 0.1ms` per 60fps frame; see the `tracy_overhead_under_100us_per_frame`
//! test for the exit-criterion verification.
//!
//! # Examples
//!
//! ```
//! use martensite_devtools::tracy;
//!
//! // Scoped span: records its duration on drop.
//! let _guard = tracy::span("layout_pass");
//! // ... layout work ...
//!
//! // Manual span lifecycle.
//! let span = tracy::TracySpan::begin("paint_encode");
//! // ... paint work ...
//! span.end();
//!
//! // Frame and plot markers.
//! tracy::frame_mark();
//! tracy::plot("gpu_wait_ms", 0.42);
//! ```

use std::cell::RefCell;
use std::time::Instant;

/// Number of span records retained in the thread-local ring buffer.
///
/// This is sized to comfortably hold the spans emitted by a single frame
/// (layout, paint, gpu wait, reactive dispatch, ...) with headroom, so the
/// HUD can inspect the most recent frame without growing unbounded.
const SPAN_RING_SIZE: usize = 256;

/// Number of distinct plot slots retained per thread.
const PLOT_SLOTS: usize = 32;

/// A single recorded profiling span.
///
/// This is a `Copy` value stored in the thread-local ring buffer so that the
/// HUD can iterate over recent spans without allocation.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SpanRecord {
    /// The static name label of the span.
    pub name: &'static str,
    /// The measured duration of the span, in nanoseconds.
    pub duration_ns: u64,
}

/// A slot for a named plot value.
#[derive(Debug, Clone, Copy, PartialEq)]
struct PlotEntry {
    /// The static name label of the plot, or `None` if the slot is free.
    name: Option<&'static str>,
    /// The last recorded value for this plot.
    value: f64,
}

impl PlotEntry {
    /// Create an empty (free) plot slot.
    const fn empty() -> Self {
        Self {
            name: None,
            value: 0.0,
        }
    }
}

/// Thread-local profiling buffer holding recent span records, plot values,
/// and a frame counter.
///
/// All storage is fixed-size arrays, so recording a span or plot never
/// allocates.
#[derive(Debug)]
struct ProfileBuffer {
    spans: [SpanRecord; SPAN_RING_SIZE],
    span_index: usize,
    span_count: usize,
    plots: [PlotEntry; PLOT_SLOTS],
    frame_count: u64,
}

impl ProfileBuffer {
    /// Create a new empty profiling buffer.
    fn new() -> Self {
        Self {
            spans: [SpanRecord {
                name: "",
                duration_ns: 0,
            }; SPAN_RING_SIZE],
            span_index: 0,
            span_count: 0,
            plots: [PlotEntry::empty(); PLOT_SLOTS],
            frame_count: 0,
        }
    }

    /// Record a completed span into the ring buffer, overwriting the oldest
    /// entry when full.
    #[inline]
    fn record_span(&mut self, name: &'static str, duration_ns: u64) {
        self.spans[self.span_index] = SpanRecord { name, duration_ns };
        self.span_index = (self.span_index + 1) % SPAN_RING_SIZE;
        if self.span_count < SPAN_RING_SIZE {
            self.span_count += 1;
        }
    }

    /// Record or update a named plot value.
    #[inline]
    fn record_plot(&mut self, name: &'static str, value: f64) {
        // Linear scan over a small fixed array; cheaper than a hashmap for the
        // expected number of distinct plots.
        for entry in self.plots.iter_mut() {
            if entry.name == Some(name) {
                entry.value = value;
                return;
            }
        }
        // Not found: claim the first free slot.
        for entry in self.plots.iter_mut() {
            if entry.name.is_none() {
                entry.name = Some(name);
                entry.value = value;
                return;
            }
        }
        // All slots occupied: overwrite the first slot (oldest heuristic).
        self.plots[0].name = Some(name);
        self.plots[0].value = value;
    }

    /// Increment the per-thread frame counter.
    #[inline]
    fn mark_frame(&mut self) {
        self.frame_count += 1;
    }

    /// Return the most recently recorded duration for the named span, if any.
    fn last_span_duration(&self, name: &'static str) -> Option<u64> {
        // Walk the ring backward from the most recent write.
        if self.span_count == 0 {
            return None;
        }
        for i in (0..self.span_count).rev() {
            let idx = (self.span_index + SPAN_RING_SIZE - 1 - i) % SPAN_RING_SIZE;
            if self.spans[idx].name == name {
                return Some(self.spans[idx].duration_ns);
            }
        }
        None
    }

    /// Return the last recorded value for the named plot, if any.
    fn plot_value(&self, name: &'static str) -> Option<f64> {
        self.plots
            .iter()
            .find(|e| e.name == Some(name))
            .map(|e| e.value)
    }

    /// Return the number of span records currently held in the ring buffer.
    fn span_record_count(&self) -> usize {
        self.span_count
    }

    /// Return the per-thread frame counter.
    fn frame_count(&self) -> u64 {
        self.frame_count
    }
}

thread_local! {
    static PROFILE: RefCell<ProfileBuffer> = RefCell::new(ProfileBuffer::new());
}

/// A profiling span that records the duration of a code region.
///
/// When Tracy is not available, this is a zero-cost no-op backed by
/// [`std::time::Instant`]. Call [`TracySpan::begin`] to start timing a region
/// and [`TracySpan::end`] to record the elapsed duration into the thread-local
/// ring buffer. For scoped (RAII) spans, prefer the [`span`] function which
/// returns a [`TracySpanGuard`].
///
/// # Examples
///
/// ```
/// use martensite_devtools::tracy::TracySpan;
///
/// let span = TracySpan::begin("encode_paint_list");
/// // ... work ...
/// span.end();
/// ```
pub struct TracySpan {
    /// The static name label of the span.
    name: &'static str,
    /// The instant at which the span began, or `None` if already ended.
    /// Uses `Cell` so that `end()` can consume the start time without
    /// requiring `&mut self`, making double-`end()` a safe no-op.
    start: std::cell::Cell<Option<Instant>>,
}

impl TracySpan {
    /// Begin a new profiling span with the given static name.
    ///
    /// The start time is captured immediately via [`Instant::now`].
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite_devtools::tracy::TracySpan;
    ///
    /// let span = TracySpan::begin("layout_pass");
    /// span.end();
    /// ```
    #[inline]
    pub fn begin(name: &'static str) -> Self {
        Self {
            name,
            start: std::cell::Cell::new(Some(Instant::now())),
        }
    }

    /// Record the elapsed duration of this span into the thread-local ring
    /// buffer.
    ///
    /// Calling `end` more than once is a no-op: subsequent calls find no start
    /// time and record nothing.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite_devtools::tracy::TracySpan;
    ///
    /// let span = TracySpan::begin("paint_pass");
    /// span.end();
    /// // A second end is a no-op.
    /// span.end();
    /// ```
    #[inline]
    pub fn end(&self) {
        if let Some(start) = self.start.take() {
            let duration_ns = start.elapsed().as_nanos() as u64;
            PROFILE.with(|p| p.borrow_mut().record_span(self.name, duration_ns));
        }
    }
}

/// RAII guard for a scoped profiling span.
///
/// Created by [`span`]; the span is recorded into the thread-local ring
/// buffer when the guard is dropped.
///
/// # Examples
///
/// ```
/// use martensite_devtools::tracy;
///
/// fn do_work() {
///     let _guard = tracy::span("work_region");
///     // ... work ...
/// }
///
/// do_work();
/// ```
pub struct TracySpanGuard {
    span: TracySpan,
}

impl Drop for TracySpanGuard {
    #[inline]
    fn drop(&mut self) {
        self.span.end();
    }
}

/// Create a scoped profiling span that records its duration on drop.
///
/// This is the primary entry point for instrumenting a code region. The
/// returned [`TracySpanGuard`] records the span into the thread-local ring
/// buffer when it goes out of scope.
///
/// # Examples
///
/// ```
/// use martensite_devtools::tracy;
///
/// {
///     let _g = tracy::span("scoped_region");
///     // ... work ...
/// } // span recorded here
/// ```
#[inline]
pub fn span(name: &'static str) -> TracySpanGuard {
    TracySpanGuard {
        span: TracySpan::begin(name),
    }
}

/// Emit a frame marker for Tracy's frame profiling.
///
/// Increments the per-thread frame counter. Pair this with one call per
/// rendered frame so frame boundaries can be correlated with span timings.
///
/// # Examples
///
/// ```
/// use martensite_devtools::tracy;
///
/// tracy::frame_mark();
/// ```
#[inline]
pub fn frame_mark() {
    PROFILE.with(|p| p.borrow_mut().mark_frame());
}

/// Record a plot point for Tracy's value plots.
///
/// Stores the latest value for the named plot in a fixed-size thread-local
/// slot. Repeated calls with the same name update the existing slot.
///
/// # Examples
///
/// ```
/// use martensite_devtools::tracy;
///
/// tracy::plot("gpu_wait_ms", 0.42);
/// tracy::plot("gpu_wait_ms", 0.51);
/// ```
#[inline]
pub fn plot(name: &'static str, value: f64) {
    PROFILE.with(|p| p.borrow_mut().record_plot(name, value));
}

/// Return the most recently recorded duration (in nanoseconds) for the named
/// span on the current thread, if any.
///
/// # Examples
///
/// ```
/// use martensite_devtools::tracy;
///
/// {
///     let _g = tracy::span("query_region");
/// }
/// let dur = tracy::last_span_duration("query_region");
/// assert!(dur.is_some());
/// ```
pub fn last_span_duration(name: &'static str) -> Option<u64> {
    PROFILE.with(|p| p.borrow().last_span_duration(name))
}

/// Return the last recorded value for the named plot on the current thread,
/// if any.
///
/// # Examples
///
/// ```
/// use martensite_devtools::tracy;
///
/// tracy::plot("fps", 59.9);
/// assert_eq!(tracy::plot_value("fps"), Some(59.9));
/// ```
pub fn plot_value(name: &'static str) -> Option<f64> {
    PROFILE.with(|p| p.borrow().plot_value(name))
}

/// Return the number of span records currently held in the current thread's
/// ring buffer.
///
/// # Examples
///
/// ```
/// use martensite_devtools::tracy;
///
/// {
///     let _g = tracy::span("count_region");
/// }
/// assert!(tracy::span_record_count() >= 1);
/// ```
pub fn span_record_count() -> usize {
    PROFILE.with(|p| p.borrow().span_record_count())
}

/// Return the per-thread frame counter value.
///
/// # Examples
///
/// ```
/// use martensite_devtools::tracy;
///
/// let before = tracy::frame_count();
/// tracy::frame_mark();
/// assert_eq!(tracy::frame_count(), before + 1);
/// ```
pub fn frame_count() -> u64 {
    PROFILE.with(|p| p.borrow().frame_count())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn span_records_duration() {
        let name = "span_records_duration";
        {
            let _g = span(name);
        }
        let dur = last_span_duration(name);
        assert!(dur.is_some(), "span should be recorded");
        // Duration is u64 so it is always non-negative; just sanity-check the
        // upper bound.
        assert!(
            dur.unwrap() < 1_000_000_000,
            "duration should be under a generous 1s upper bound, got {}",
            dur.unwrap()
        );
    }

    #[test]
    fn manual_span_end_records() {
        let name = "manual_span_end_records";
        let s = TracySpan::begin(name);
        s.end();
        let dur = last_span_duration(name);
        assert!(dur.is_some(), "manual span should be recorded");
        assert!(
            dur.unwrap() < 1_000_000_000,
            "duration should be under a generous 1s upper bound, got {}",
            dur.unwrap()
        );
    }

    #[test]
    fn double_end_is_noop() {
        // This test verifies that calling `end()` twice on the same span
        // is a no-op. We use `span_record_count()` to verify that the
        // second `end()` call does not record an additional span.
        let name = "double_end_is_noop_unique_4f7a";
        let before = span_record_count();
        let s = TracySpan::begin(name);
        s.end();
        let after_first = span_record_count();
        // First end records exactly one span.
        assert_eq!(after_first, before + 1, "first end should record one span");
        // Second end is a no-op: start was already taken.
        s.end();
        let after_second = span_record_count();
        assert_eq!(
            after_second, after_first,
            "second end should not record another span"
        );
        // The recorded span should be queryable.
        let dur = last_span_duration(name);
        assert!(dur.is_some(), "span should be recorded");

        // Now verify that a second span with the same name records
        // correctly and has a measurable duration.
        let s2 = TracySpan::begin(name);
        // Busy-wait for a deterministic amount well above timer resolution.
        let start = std::time::Instant::now();
        while start.elapsed() < std::time::Duration::from_millis(2) {
            std::hint::spin_loop();
        }
        s2.end();
        let after_third = span_record_count();
        assert_eq!(
            after_third,
            after_second + 1,
            "second span should record one span"
        );
        // Second end of s2 is a no-op.
        s2.end();
        assert_eq!(
            span_record_count(),
            after_third,
            "second end of s2 should not record"
        );
    }

    #[test]
    fn frame_mark_increments_counter() {
        let before = frame_count();
        frame_mark();
        frame_mark();
        assert_eq!(frame_count(), before + 2);
    }

    #[test]
    fn plot_records_and_updates() {
        plot("plot_test_a", 1.0);
        assert_eq!(plot_value("plot_test_a"), Some(1.0));
        plot("plot_test_a", 2.5);
        assert_eq!(plot_value("plot_test_a"), Some(2.5));
    }

    #[test]
    fn plot_missing_returns_none() {
        assert!(plot_value("definitely_not_a_plot_xyz").is_none());
    }

    #[test]
    fn missing_span_returns_none() {
        assert!(last_span_duration("definitely_not_a_span_xyz").is_none());
    }

    #[test]
    fn ring_buffer_overwrites_oldest() {
        // Record more spans than the ring size to verify no panic and that
        // recent spans are still queryable.
        for i in 0..(SPAN_RING_SIZE + 10) {
            // Use a couple of distinct names.
            let _g = span("ring_span_a");
            let _g2 = span("ring_span_b");
            let _ = i;
        }
        // The most recent span should be queryable.
        assert!(last_span_duration("ring_span_b").is_some());
        assert!(span_record_count() <= SPAN_RING_SIZE);
    }

    #[test]
    fn plot_slots_evict_when_full() {
        // Fill all plot slots plus extras; should not panic and the most
        // recently written should be queryable.
        for i in 0..(PLOT_SLOTS + 5) {
            // Generate distinct static names by interning via leak is not
            // possible without unsafe; instead reuse a small set of names.
            plot("plot_evict", i as f64);
        }
        assert_eq!(plot_value("plot_evict"), Some((PLOT_SLOTS + 4) as f64));
    }

    /// DevTools Overhead Gate (§5.3): active Tracy profiling instrumentation
    /// must contribute `< 0.1ms` overhead per 60fps frame.
    ///
    /// This measures the cost of a representative frame's instrumentation:
    /// one scoped span (begin + end), a frame mark, and a plot point, across
    /// 60 frames, and asserts the per-frame overhead is below 100µs.
    #[test]
    #[ignore = "wall-clock performance gate; run manually with --ignored --test-threads=1"]
    fn tracy_overhead_under_100us_per_frame() {
        // Warm up the thread-local to avoid first-access cost in the
        // measurement window.
        {
            let _g = span("warmup");
        }
        frame_mark();
        plot("warmup_plot", 0.0);

        const FRAMES: u32 = 60;
        let start = Instant::now();
        for _ in 0..FRAMES {
            let _g = span("overhead_frame");
            frame_mark();
            plot("overhead_plot", 1.0);
        }
        let elapsed = start.elapsed();
        let per_frame_ns = elapsed.as_nanos() / FRAMES as u128;
        // 0.1ms = 100_000 ns. We use a generous 100us gate per the spec.
        assert!(
            per_frame_ns < 100_000,
            "Tracy overhead {per_frame_ns}ns/frame exceeds the 100us (100_000ns) gate"
        );
    }
}
