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
}

impl<T: 'static> Clone for Signal<T> {
    fn clone(&self) -> Self {
        Self {
            id: self.id,
            runtime: Arc::clone(&self.runtime),
            value: Arc::clone(&self.value),
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
    pub fn set(&self, val: T) {
        {
            let mut guard = self.value.write();
            *guard = val;
        }
        self.runtime.mark_dirty(self.id);
    }
}

impl<T: Clone + Send + Sync + 'static> Signal<T> {
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
        let changed = {
            let mut guard = self.value.write();
            if *guard != val {
                *guard = val;
                true
            } else {
                false
            }
        };

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
