//! Thread-safe reactive state signals.
#![forbid(unsafe_code)]

use std::fmt;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;

use parking_lot::RwLock;

use crate::runtime::ReactiveRuntime;

static NEXT_SIG_ID: AtomicU64 = AtomicU64::new(1);

/// Unique 64-bit identifier for a reactive node in the dependency graph.
///
/// `SignalId`s are allocated monotonically and never repeat within a process,
/// making them suitable as hash-map keys and for stable equality comparisons
/// across the reactive DAG.
///
/// # Examples
///
/// ```
/// use martensite_reactive::SignalId;
///
/// let a = SignalId::next();
/// let b = SignalId::next();
///
/// assert_ne!(a, b, "each allocated id must be unique");
/// assert!(b.raw() > a.raw(), "ids must increase monotonically");
/// ```
#[derive(Copy, Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct SignalId(pub u64);

impl SignalId {
    /// Allocates a new monotonically increasing `SignalId`.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite_reactive::SignalId;
    ///
    /// let id = SignalId::next();
    /// assert!(id.raw() > 0);
    /// ```
    #[inline]
    pub fn next() -> Self {
        Self(NEXT_SIG_ID.fetch_add(1, Ordering::Relaxed))
    }

    /// Returns the raw integer value of this signal ID.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite_reactive::SignalId;
    ///
    /// let id = SignalId::next();
    /// let raw = id.raw();
    /// assert_eq!(SignalId(raw).raw(), raw);
    /// ```
    #[inline]
    pub const fn raw(self) -> u64 {
        self.0
    }
}

impl fmt::Display for SignalId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "SignalId({})", self.0)
    }
}

/// A reactive source state variable that propagates mutations to downstream subscribers.
///
/// A `Signal` is the root of a reactive DAG: reading it inside a [`Memo`](crate::Memo) or
/// [`Effect`](crate::Effect) registers a dependency edge, and writing to it schedules
/// every downstream subscriber for re-evaluation.
///
/// # Examples
///
/// ```
/// use martensite_reactive::{Signal, create_memo};
///
/// let count = Signal::new(1);
/// let doubled = create_memo({
///     let count = count.clone();
///     move || count.get() * 2
/// });
///
/// assert_eq!(doubled.get(), 2);
/// count.set(5);
/// assert_eq!(doubled.get(), 10);
/// ```
pub struct Signal<T: 'static> {
    /// Unique identifier for this signal node.
    pub id: SignalId,
    runtime: Arc<ReactiveRuntime>,
    value: Arc<RwLock<T>>,
    /// Whether this signal's storage accessor has been registered with
    /// the runtime's snapshot registry. Exists only under
    /// `devtools-timemachine`; zero cost otherwise.
    #[cfg(feature = "devtools-timemachine")]
    access_registered: std::sync::atomic::AtomicBool,
}

impl<T: 'static> Clone for Signal<T> {
    fn clone(&self) -> Self {
        Self {
            id: self.id,
            runtime: Arc::clone(&self.runtime),
            value: Arc::clone(&self.value),
            #[cfg(feature = "devtools-timemachine")]
            access_registered: std::sync::atomic::AtomicBool::new(
                self.access_registered
                    .load(std::sync::atomic::Ordering::Relaxed),
            ),
        }
    }
}

impl<T: Send + Sync + 'static> Signal<T> {
    /// Creates a new `Signal` bound to the ambient reactive runtime.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite_reactive::Signal;
    ///
    /// let name = Signal::new(String::from("Ada"));
    /// assert_eq!(name.get_untracked(), "Ada");
    /// ```
    pub fn new(initial: T) -> Self {
        let runtime = ReactiveRuntime::current();
        Self::new_with_runtime(initial, runtime)
    }

    /// Creates a new `Signal` bound explicitly to a specified runtime.
    ///
    /// Use this when you need to isolate a signal graph inside a dedicated
    /// [`ReactiveRuntime`] rather than the ambient one.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite_reactive::{ReactiveRuntime, Signal};
    ///
    /// let runtime = ReactiveRuntime::new();
    /// let value = Signal::new_with_runtime(42, runtime);
    /// assert_eq!(value.get_untracked(), 42);
    /// ```
    pub fn new_with_runtime(initial: T, runtime: Arc<ReactiveRuntime>) -> Self {
        let id = SignalId::next();
        runtime.register_source(id);
        Self {
            id,
            runtime,
            value: Arc::new(RwLock::new(initial)),
            #[cfg(feature = "devtools-timemachine")]
            access_registered: std::sync::atomic::AtomicBool::new(false),
        }
    }

    /// Returns the unique `SignalId` for this signal.
    #[inline(always)]
    pub fn id(&self) -> SignalId {
        self.id
    }

    /// Returns a reference to the bound `ReactiveRuntime`.
    #[inline(always)]
    pub fn runtime(&self) -> &Arc<ReactiveRuntime> {
        &self.runtime
    }

    /// Mutates the stored value in place via a closure and flags downstream subscribers dirty.
    ///
    /// With `devtools-timemachine`: `update` mutations are **not**
    /// journaled (no capturable previous value), and a write-only
    /// source that is never read via `get`/`get_untracked` — and never
    /// passes through `set_if_changed` — never registers its snapshot
    /// accessor, so it is absent from `SignalSnapshot`s and is not
    /// restored on replay.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite_reactive::Signal;
    ///
    /// let counter = Signal::new(0);
    /// counter.update(|c| *c += 1);
    /// counter.update(|c| *c += 1);
    /// assert_eq!(counter.get_untracked(), 2);
    /// ```
    pub fn update(&self, f: impl FnOnce(&mut T)) {
        {
            let mut guard = self.value.write();
            f(&mut *guard);
        }
        self.runtime.mark_dirty(self.id);
    }

    /// Overwrites the stored value and flags downstream subscribers dirty.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite_reactive::Signal;
    ///
    /// let active = Signal::new(false);
    /// active.set(true);
    /// assert_eq!(active.get_untracked(), true);
    /// ```
    #[cfg(not(feature = "devtools-timemachine"))]
    pub fn set(&self, val: T) {
        {
            let mut guard = self.value.write();
            *guard = val;
        }
        self.runtime.mark_dirty(self.id);
    }

    /// Overwrites the stored value, journals the previous value in the
    /// runtime's [`SourceJournal`](crate::journal::SourceJournal), and
    /// flags downstream subscribers dirty.
    ///
    /// The previous value is captured via in-place replacement, so no
    /// `Clone` bound is required on `T`. When journaling is suppressed
    /// by [`ReactiveRuntime::suppress_journal`] the write is applied but
    /// not recorded.
    ///
    /// Note: the [`SourceJournal`] is an *audit log* only — a write
    /// recorded here is not a history command and is invisible to
    /// time-travel replay. Scrubbing with
    /// `martensite_devtools::timemachine` replays commands committed to
    /// the command journal (e.g. `TimeMachine::set_signal`/`commit`),
    /// not raw `Signal::set` calls.
    ///
    /// A `Clone` source that is *only ever written* — never read via
    /// `get`/`get_untracked` and never passed through `set_if_changed`
    /// — never registers its snapshot accessor: it is journaled here
    /// but stays absent from `SignalSnapshot`s, so replay does not
    /// restore it.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite_reactive::Signal;
    ///
    /// let active = Signal::new(false);
    /// active.set(true);
    /// assert_eq!(active.get_untracked(), true);
    /// ```
    #[cfg(feature = "devtools-timemachine")]
    pub fn set(&self, val: T) {
        let previous = {
            let mut guard = self.value.write();
            std::mem::replace(&mut *guard, val)
        };
        // Journaled outside the signal write lock so the journal
        // critical section never nests inside a signal's lock. The
        // suppressed fast-path skips boxing a record that would be
        // discarded anyway.
        if !self.runtime.journal_suppressed() {
            self.runtime
                .record_source_write(self.id, Box::new(previous));
        }
        self.runtime.mark_dirty(self.id);
    }
}

impl<T: Clone + Send + Sync + 'static> Signal<T> {
    /// Registers this signal's storage accessor with the runtime so the
    /// source participates in [`SignalSnapshot`](crate::journal::SignalSnapshot)
    /// capture and restore. Idempotent — a single atomic swap on every
    /// call after the first.
    ///
    /// Called lazily from the `Clone`-bounded read/write methods
    /// because a snapshot requires copying the payload out of storage,
    /// which is only possible for `Clone` values. A source registers on
    /// its first `get`/`get_untracked`/`set_if_changed` — since
    /// [`Memo`](crate::Memo) and [`Effect`](crate::Effect) evaluation
    /// reads through `get`, every source wired into the reactive graph
    /// registers on its first pull.
    #[cfg(feature = "devtools-timemachine")]
    #[inline]
    fn ensure_source_access(&self) {
        if !self
            .access_registered
            .swap(true, std::sync::atomic::Ordering::AcqRel)
        {
            self.runtime.register_source_access(self.id, &self.value);
        }
    }

    /// Reads the current value, registering a dependency edge if called within a reactive context.
    ///
    /// Outside of a [`Memo`](crate::Memo) or [`Effect`](crate::Effect) evaluation this simply
    /// returns the current value. Inside one it records the read so that the caller is
    /// re-evaluated whenever this signal changes.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite_reactive::{Signal, create_memo};
    ///
    /// let a = Signal::new(10);
    /// let sum = create_memo({
    ///     let a = a.clone();
    ///     move || a.get() + 1
    /// });
    ///
    /// assert_eq!(sum.get(), 11);
    /// a.set(20);
    /// assert_eq!(sum.get(), 21);
    /// ```
    #[inline(always)]
    pub fn get(&self) -> T {
        #[cfg(feature = "devtools-timemachine")]
        self.ensure_source_access();
        self.runtime.track_read(self.id);
        self.value.read().clone()
    }

    /// Reads the current value without registering a dependency edge.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite_reactive::{Signal, create_memo};
    ///
    /// let a = Signal::new(7);
    /// // Reading untracked inside a memo does not subscribe to `a`.
    /// let independent = create_memo({
    ///     let a = a.clone();
    ///     move || a.get_untracked() + 100
    /// });
    ///
    /// assert_eq!(independent.get(), 107);
    /// a.set(99);
    /// // The memo was not subscribed, so it keeps its stale cached value.
    /// assert_eq!(independent.get(), 107);
    /// ```
    #[inline(always)]
    pub fn get_untracked(&self) -> T {
        #[cfg(feature = "devtools-timemachine")]
        self.ensure_source_access();
        self.value.read().clone()
    }
}

impl<T: PartialEq + Clone + Send + Sync + 'static> Signal<T> {
    /// Overwrites the stored value only if it differs from the current value.
    ///
    /// Returns `true` if the value changed and dirty flags were pushed, `false` otherwise.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite_reactive::Signal;
    ///
    /// let selected = Signal::new(3);
    ///
    /// assert!(!selected.set_if_changed(3), "no-op when value is unchanged");
    /// assert!(selected.set_if_changed(7), "returns true on change");
    /// assert_eq!(selected.get_untracked(), 7);
    /// ```
    pub fn set_if_changed(&self, val: T) -> bool {
        #[cfg(feature = "devtools-timemachine")]
        self.ensure_source_access();
        #[cfg(feature = "devtools-timemachine")]
        let mut previous = None;
        let changed = {
            let mut guard = self.value.write();
            if *guard != val {
                #[cfg(feature = "devtools-timemachine")]
                {
                    previous = Some(std::mem::replace(&mut *guard, val));
                }
                #[cfg(not(feature = "devtools-timemachine"))]
                {
                    *guard = val;
                }
                true
            } else {
                false
            }
        };

        #[cfg(feature = "devtools-timemachine")]
        if let Some(previous) = previous {
            // Journaled outside the signal write lock (see `set`).
            if !self.runtime.journal_suppressed() {
                self.runtime
                    .record_source_write(self.id, Box::new(previous));
            }
        }
        if changed {
            self.runtime.mark_dirty(self.id);
        }
        changed
    }
}

impl<T: fmt::Debug + Clone + Send + Sync + 'static> fmt::Debug for Signal<T> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Signal")
            .field("id", &self.id)
            .field("value", &self.get_untracked())
            .finish()
    }
}

impl<T: fmt::Display + Clone + Send + Sync + 'static> fmt::Display for Signal<T> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.get_untracked())
    }
}
