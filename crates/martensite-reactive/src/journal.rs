//! Source-write journal and signal snapshots for time-travel debugging.
//!
//! This module is compiled only with the `devtools-timemachine` feature.
//! It provides two primitives used by
//! `martensite_devtools::timemachine`:
//!
//! - [`SourceJournal`] — a monotonic record of every `Signal::set` (and
//!   `Signal::set_if_changed`) write against a [`ReactiveRuntime`]. Each
//!   [`WriteRecord`] stores the signal's *previous* value, captured via
//!   in-place replacement, so recording imposes no `Clone` bound on the
//!   payload type. `Signal::update` mutations cannot produce an old
//!   value and are intentionally not journaled.
//! - [`SignalSnapshot`] — a type-erased capture of all registered source
//!   signal values at a checkpoint. Sources register lazily on their
//!   first `Clone`-bounded read/write (`get`, `get_untracked`,
//!   `set_if_changed` — `Clone` being the only bound under which a value
//!   can be copied out of storage); non-`Clone` sources can be
//!   journaled but not snapshotted.
//!
//! Only *source* writes are recorded. [`Memo`](crate::Memo) values are
//! derived state and recompute lazily during the pull phase after a
//! restore — they are never journaled.
//!
//! # Replay suppression
//!
//! While a replay is in flight, replayed commands must not re-journal
//! themselves. [`ReactiveRuntime::suppress_journal`] returns a RAII
//! [`JournalGuard`] that suppresses recording until dropped — the
//! `debug::disable`-style guard of the determinism contract.
//!
//! # Examples
//!
//! ```
//! use martensite_reactive::{ReactiveRuntime, Signal};
//!
//! let runtime = ReactiveRuntime::new();
//! let count = Signal::new_with_runtime(0i32, runtime.clone());
//!
//! count.set(1);
//! count.set(2);
//!
//! let journal = runtime.journal();
//! assert_eq!(journal.len(), 2);
//! assert_eq!(journal.get(0).unwrap().previous_as::<i32>(), Some(&0));
//! assert_eq!(journal.get(1).unwrap().previous_as::<i32>(), Some(&1));
//! ```
#![forbid(unsafe_code)]

use std::any::Any;

use crate::signal::SignalId;

/// A single recorded write to a source signal.
///
/// The record captures the value the signal held *immediately before*
/// the write. The new value is recoverable as the `previous` value of
/// the next record on the same signal, or by reading the signal live.
///
/// # Examples
///
/// ```
/// use martensite_reactive::{ReactiveRuntime, Signal};
///
/// let runtime = ReactiveRuntime::new();
/// let flag = Signal::new_with_runtime(false, runtime.clone());
/// flag.set(true);
///
/// let journal = runtime.journal();
/// let record = journal.records().next().unwrap();
/// assert_eq!(record.index, 0);
/// assert_eq!(record.signal, flag.id());
/// assert_eq!(record.previous_as::<bool>(), Some(&false));
/// ```
pub struct WriteRecord {
    /// Monotonic write index, assigned in record order starting at 0.
    pub index: u64,
    /// The source signal that was written.
    pub signal: SignalId,
    /// Value the signal held immediately before this write.
    previous: Box<dyn Any + Send + Sync>,
}

impl WriteRecord {
    /// Returns the pre-write value downcast to the concrete payload
    /// type, or `None` if `T` does not match.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite_reactive::{ReactiveRuntime, Signal};
    ///
    /// let runtime = ReactiveRuntime::new();
    /// let name = Signal::new_with_runtime(String::from("a"), runtime.clone());
    /// name.set(String::from("b"));
    ///
    /// let journal = runtime.journal();
    /// let record = journal.records().next().unwrap();
    /// assert_eq!(record.previous_as::<String>().map(String::as_str), Some("a"));
    /// assert!(record.previous_as::<i32>().is_none());
    /// ```
    pub fn previous_as<T: 'static>(&self) -> Option<&T> {
        self.previous.downcast_ref::<T>()
    }
}

impl std::fmt::Debug for WriteRecord {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("WriteRecord")
            .field("index", &self.index)
            .field("signal", &self.signal)
            .finish_non_exhaustive()
    }
}

/// Monotonic journal of `Signal::set` source writes on one runtime.
///
/// Obtained via [`ReactiveRuntime::journal`]. Recording is suppressed
/// while a [`JournalGuard`] is alive (see
/// [`ReactiveRuntime::suppress_journal`]).
///
/// The journal is a bounded ring: at most
/// [`DEFAULT_MAX_RECORDS`](SourceJournal::DEFAULT_MAX_RECORDS) records
/// are retained (configurable via [`with_max_records`](SourceJournal::with_max_records)),
/// and the oldest records are evicted as new ones arrive. [`WriteRecord::index`]
/// remains globally monotonic — eviction only raises
/// [`first_index`](SourceJournal::first_index), the index of the oldest
/// retained record.
///
/// # Examples
///
/// ```
/// use martensite_reactive::{ReactiveRuntime, Signal};
///
/// let runtime = ReactiveRuntime::new();
/// let value = Signal::new_with_runtime(0, runtime.clone());
/// value.set(10);
///
/// let journal = runtime.journal();
/// assert_eq!(journal.len(), 1);
/// assert!(!journal.is_empty());
/// assert_eq!(journal.next_index(), 1);
/// ```
#[derive(Debug)]
pub struct SourceJournal {
    /// Retained records in monotonic index order.
    records: std::collections::VecDeque<WriteRecord>,
    /// Index of `records.front()` — records before this were evicted.
    base_index: u64,
    /// Maximum retained records; oldest evicted past this.
    max_records: usize,
    /// Number of live [`JournalGuard`]s suppressing recording.
    suppression_depth: usize,
    /// Bumped by [`clear`](Self::clear): record indices are only
    /// unique within a generation.
    generation: u64,
}

impl Default for SourceJournal {
    fn default() -> Self {
        Self::new()
    }
}

impl SourceJournal {
    /// Default retention bound: 8192 records.
    pub const DEFAULT_MAX_RECORDS: usize = 8192;

    /// Creates an empty journal retaining at most
    /// [`DEFAULT_MAX_RECORDS`](Self::DEFAULT_MAX_RECORDS) records.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite_reactive::journal::SourceJournal;
    ///
    /// let journal = SourceJournal::new();
    /// assert!(journal.is_empty());
    /// assert_eq!(journal.max_records(), SourceJournal::DEFAULT_MAX_RECORDS);
    /// ```
    pub fn new() -> Self {
        Self::with_max_records(Self::DEFAULT_MAX_RECORDS)
    }

    /// Creates an empty journal retaining at most `max_records`
    /// records (clamped to a minimum of 1).
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite_reactive::journal::SourceJournal;
    ///
    /// let journal = SourceJournal::with_max_records(4);
    /// assert_eq!(journal.max_records(), 4);
    /// ```
    pub fn with_max_records(max_records: usize) -> Self {
        Self {
            records: std::collections::VecDeque::new(),
            base_index: 0,
            max_records: max_records.max(1),
            suppression_depth: 0,
            generation: 0,
        }
    }

    /// Returns the maximum number of retained records.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite_reactive::journal::SourceJournal;
    ///
    /// let journal = SourceJournal::with_max_records(64);
    /// assert_eq!(journal.max_records(), 64);
    /// ```
    #[inline]
    pub fn max_records(&self) -> usize {
        self.max_records
    }

    /// Returns the index of the oldest retained record — records with
    /// lower indices have been evicted.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite_reactive::journal::SourceJournal;
    ///
    /// let journal = SourceJournal::new();
    /// assert_eq!(journal.first_index(), 0);
    /// ```
    #[inline]
    pub fn first_index(&self) -> u64 {
        self.base_index
    }

    /// Returns the number of recorded writes.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite_reactive::journal::SourceJournal;
    ///
    /// let journal = SourceJournal::new();
    /// assert_eq!(journal.len(), 0);
    /// ```
    #[inline]
    pub fn len(&self) -> usize {
        self.records.len()
    }

    /// Returns `true` if no writes have been recorded.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite_reactive::journal::SourceJournal;
    ///
    /// let journal = SourceJournal::new();
    /// assert!(journal.is_empty());
    /// ```
    #[inline]
    pub fn is_empty(&self) -> bool {
        self.records.is_empty()
    }

    /// Returns the index the next recorded write will be assigned.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite_reactive::journal::SourceJournal;
    ///
    /// let journal = SourceJournal::new();
    /// assert_eq!(journal.next_index(), 0);
    /// ```
    #[inline]
    pub fn next_index(&self) -> u64 {
        self.base_index + self.records.len() as u64
    }

    /// Iterates the retained records in monotonic index order.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite_reactive::journal::SourceJournal;
    ///
    /// let journal = SourceJournal::new();
    /// assert_eq!(journal.records().count(), 0);
    /// ```
    #[inline]
    pub fn records(&self) -> impl ExactSizeIterator<Item = &WriteRecord> + '_ {
        self.records.iter()
    }

    /// Returns the retained write at `index`, or `None` if out of
    /// range or already evicted.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite_reactive::journal::SourceJournal;
    ///
    /// let journal = SourceJournal::new();
    /// assert!(journal.get(0).is_none());
    /// ```
    #[inline]
    pub fn get(&self, index: u64) -> Option<&WriteRecord> {
        index
            .checked_sub(self.base_index)
            .and_then(|i| self.records.get(i as usize))
    }

    /// Returns `true` while a [`JournalGuard`] is suppressing recording.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite_reactive::journal::SourceJournal;
    ///
    /// let journal = SourceJournal::new();
    /// assert!(!journal.is_suppressed());
    /// ```
    #[inline]
    pub fn is_suppressed(&self) -> bool {
        self.suppression_depth > 0
    }

    /// Returns the journal generation.
    ///
    /// Record indices restart at 0 on every [`clear`](Self::clear), so
    /// a `WriteRecord::index` observed before a clear aliases with
    /// post-clear indices. Callers that correlate indices across a
    /// possible clear must compare generations.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite_reactive::journal::SourceJournal;
    ///
    /// let mut journal = SourceJournal::new();
    /// assert_eq!(journal.generation(), 0);
    /// journal.clear();
    /// assert_eq!(journal.generation(), 1);
    /// ```
    #[inline]
    pub fn generation(&self) -> u64 {
        self.generation
    }

    /// Drops all retained records, restarts the index counter at 0,
    /// and bumps the [`generation`](Self::generation).
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite_reactive::journal::SourceJournal;
    ///
    /// let mut journal = SourceJournal::new();
    /// journal.clear();
    /// assert!(journal.is_empty());
    /// ```
    pub fn clear(&mut self) {
        self.records.clear();
        self.base_index = 0;
        self.generation += 1;
    }

    /// Records a write; returns the assigned index or `None` when
    /// recording is suppressed by a [`JournalGuard`]. Evicts the oldest
    /// record when at capacity.
    pub(crate) fn record(
        &mut self,
        signal: SignalId,
        previous: Box<dyn Any + Send + Sync>,
    ) -> Option<u64> {
        if self.is_suppressed() {
            return None;
        }
        if self.records.len() == self.max_records {
            self.records.pop_front();
            self.base_index += 1;
        }
        let index = self.next_index();
        self.records.push_back(WriteRecord {
            index,
            signal,
            previous,
        });
        Some(index)
    }

    /// Increments the suppression depth (called by [`JournalGuard`]).
    pub(crate) fn suppress(&mut self) {
        self.suppression_depth += 1;
    }

    /// Decrements the suppression depth (called by [`JournalGuard`]).
    pub(crate) fn unsuppress(&mut self) {
        self.suppression_depth = self.suppression_depth.saturating_sub(1);
    }
}

/// RAII guard that suppresses source-write journaling while alive.
///
/// Created by [`ReactiveRuntime::suppress_journal`]. Guards nest: the
/// journal resumes recording only when every outstanding guard has been
/// dropped. This is the `debug::disable`-style guard of the replay
/// determinism contract — signal writes performed by replayed commands
/// are applied to state but never re-journaled.
///
/// # Caveats
///
/// - Suppression is **runtime-global**, not thread-scoped: while any
///   guard is alive, source writes on *every* thread sharing the
///   runtime are skipped by the journal.
/// - [`std::mem::forget`]ing a guard leaks the suppression: the depth
///   stays incremented forever, silencing the journal until process
///   end (later guards cannot fix it — each drop only undoes its own
///   increment). Never forget a `JournalGuard`.
///
/// # Examples
///
/// ```
/// use martensite_reactive::{ReactiveRuntime, Signal};
///
/// let runtime = ReactiveRuntime::new();
/// let value = Signal::new_with_runtime(0, runtime.clone());
///
/// {
///     let _guard = runtime.suppress_journal();
///     value.set(1); // applied, but not journaled
///     assert_eq!(runtime.journal().len(), 0);
/// }
/// value.set(2);
/// assert_eq!(runtime.journal().len(), 1);
/// ```
pub struct JournalGuard<'a> {
    journal: &'a parking_lot::Mutex<SourceJournal>,
}

impl<'a> JournalGuard<'a> {
    pub(crate) fn new(journal: &'a parking_lot::Mutex<SourceJournal>) -> Self {
        journal.lock().suppress();
        Self { journal }
    }
}

impl Drop for JournalGuard<'_> {
    fn drop(&mut self) {
        self.journal.lock().unsuppress();
    }
}

/// Type-erased signal read closure.
type SourceReadFn = Box<dyn Fn() -> Option<Box<dyn Any + Send + Sync>> + Send + Sync>;
/// Type-erased signal write closure.
type SourceWriteFn = Box<dyn Fn(&(dyn Any + Send + Sync)) -> bool + Send + Sync>;

/// Type-erased read/write accessor for a source signal's storage.
///
/// Both closures capture a `Weak` reference to the signal's `RwLock<T>`
/// so dropped signals report as dead instead of being kept alive by the
/// registry. Constructed by
/// [`ReactiveRuntime::register_source_access`].
pub(crate) struct SourceAccess {
    /// Reads the current value, or `None` if the signal was dropped.
    pub read: SourceReadFn,
    /// Writes a recorded value back; `false` if the signal was dropped
    /// or the payload type does not match.
    pub write: SourceWriteFn,
}

/// A type-erased capture of all registered source-signal values.
///
/// Produced by [`ReactiveRuntime::snapshot_signals`] and consumed by
/// [`ReactiveRuntime::restore_signals`]. Entries are sorted by
/// [`SignalId`] for deterministic iteration order.
///
/// # Examples
///
/// ```
/// use martensite_reactive::{ReactiveRuntime, Signal};
///
/// let runtime = ReactiveRuntime::new();
/// let a = Signal::new_with_runtime(1i32, runtime.clone());
/// let b = Signal::new_with_runtime(String::from("x"), runtime.clone());
/// // Sources register lazily on their first read.
/// let _ = a.get_untracked();
/// let _ = b.get_untracked();
///
/// let snapshot = runtime.snapshot_signals();
/// assert_eq!(snapshot.len(), 2);
/// assert_eq!(snapshot.get::<i32>(a.id()), Some(&1));
/// ```
pub struct SignalSnapshot {
    /// `(signal, value)` pairs sorted by signal id.
    values: Vec<(SignalId, Box<dyn Any + Send + Sync>)>,
}

impl SignalSnapshot {
    pub(crate) fn from_parts(values: Vec<(SignalId, Box<dyn Any + Send + Sync>)>) -> Self {
        Self { values }
    }

    /// Returns the number of captured signal values.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite_reactive::ReactiveRuntime;
    ///
    /// let runtime = ReactiveRuntime::new();
    /// assert_eq!(runtime.snapshot_signals().len(), 0);
    /// ```
    #[inline]
    pub fn len(&self) -> usize {
        self.values.len()
    }

    /// Returns `true` if no signal values were captured.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite_reactive::ReactiveRuntime;
    ///
    /// let runtime = ReactiveRuntime::new();
    /// assert!(runtime.snapshot_signals().is_empty());
    /// ```
    #[inline]
    pub fn is_empty(&self) -> bool {
        self.values.is_empty()
    }

    /// Returns the ids of all captured signals in sorted order.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite_reactive::{ReactiveRuntime, Signal};
    ///
    /// let runtime = ReactiveRuntime::new();
    /// let a = Signal::new_with_runtime(1, runtime.clone());
    /// let _ = a.get_untracked(); // register the source
    /// let snapshot = runtime.snapshot_signals();
    /// assert_eq!(snapshot.signals(), &[a.id()]);
    /// ```
    pub fn signals(&self) -> Vec<SignalId> {
        self.values.iter().map(|(id, _)| *id).collect()
    }

    /// Returns the captured value for `signal` downcast to `T`, or
    /// `None` if the signal is absent or the type does not match.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite_reactive::{ReactiveRuntime, Signal};
    ///
    /// let runtime = ReactiveRuntime::new();
    /// let a = Signal::new_with_runtime(7i32, runtime.clone());
    /// let _ = a.get_untracked(); // register the source
    /// let snapshot = runtime.snapshot_signals();
    /// assert_eq!(snapshot.get::<i32>(a.id()), Some(&7));
    /// assert_eq!(snapshot.get::<u32>(a.id()), None);
    /// ```
    pub fn get<T: 'static>(&self, signal: SignalId) -> Option<&T> {
        self.values
            .iter()
            .find(|(id, _)| *id == signal)
            .and_then(|(_, v)| v.downcast_ref::<T>())
    }

    /// Iterates the captured `(signal, value)` pairs in sorted order.
    pub(crate) fn iter(&self) -> impl Iterator<Item = (SignalId, &(dyn Any + Send + Sync))> + '_ {
        self.values.iter().map(|(id, v)| (*id, &**v))
    }
}

impl std::fmt::Debug for SignalSnapshot {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("SignalSnapshot")
            .field("len", &self.values.len())
            .finish()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ring_eviction_keeps_indices_monotonic() {
        let mut journal = SourceJournal::with_max_records(3);
        let sig = SignalId(0);
        for _ in 0..5 {
            journal.record(sig, Box::new(0i32));
        }
        assert_eq!(journal.len(), 3);
        assert_eq!(journal.first_index(), 2);
        assert_eq!(journal.next_index(), 5);
        assert!(journal.get(0).is_none());
        assert!(journal.get(1).is_none());
        assert_eq!(journal.get(2).unwrap().index, 2);
        assert_eq!(journal.get(4).unwrap().index, 4);
        assert!(journal.get(5).is_none());
        // Iteration stays in monotonic index order.
        let indices: Vec<u64> = journal.records().map(|r| r.index).collect();
        assert_eq!(indices, vec![2, 3, 4]);
    }

    #[test]
    fn clear_resets_indices_and_bumps_generation() {
        let mut journal = SourceJournal::with_max_records(2);
        journal.record(SignalId(0), Box::new(1i32));
        assert_eq!(journal.generation(), 0);
        journal.clear();
        assert_eq!(journal.generation(), 1);
        assert_eq!(journal.first_index(), 0);
        assert_eq!(journal.next_index(), 0);
        // Indices restart — generation disambiguates the reuse.
        assert_eq!(journal.record(SignalId(0), Box::new(2i32)), Some(0));
        assert_eq!(journal.get(0).unwrap().index, 0);
    }

    #[test]
    fn suppressed_records_are_dropped() {
        let mut journal = SourceJournal::with_max_records(4);
        journal.suppress();
        assert_eq!(journal.record(SignalId(0), Box::new(1i32)), None);
        journal.unsuppress();
        journal.unsuppress(); // saturates at 0
        assert_eq!(journal.record(SignalId(0), Box::new(1i32)), Some(0));
    }
}
