//! Thread-safe reactive state signals.
#![forbid(unsafe_code)]

use std::fmt;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;

use parking_lot::RwLock;

use crate::runtime::ReactiveRuntime;

static NEXT_SIG_ID: AtomicU64 = AtomicU64::new(1);

/// Unique 64-bit identifier for a reactive node in the dependency graph.
#[derive(Copy, Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct SignalId(pub u64);

impl SignalId {
    /// Allocates a new monotonically increasing `SignalId`.
    #[inline]
    pub fn next() -> Self {
        Self(NEXT_SIG_ID.fetch_add(1, Ordering::Relaxed))
    }

    /// Returns the raw integer value of this signal ID.
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
    pub fn new(initial: T) -> Self {
        let runtime = ReactiveRuntime::current();
        Self::new_with_runtime(initial, runtime)
    }

    /// Creates a new `Signal` bound explicitly to a specified runtime.
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
    pub fn update(&self, f: impl FnOnce(&mut T)) {
        {
            let mut guard = self.value.write();
            f(&mut *guard);
        }
        self.runtime.mark_dirty(self.id);
    }

    /// Overwrites the stored value and flags downstream subscribers dirty.
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
    #[inline(always)]
    pub fn get(&self) -> T {
        self.runtime.track_read(self.id);
        self.value.read().clone()
    }

    /// Reads the current value without registering a dependency edge.
    #[inline(always)]
    pub fn get_untracked(&self) -> T {
        self.value.read().clone()
    }
}

impl<T: PartialEq + Clone + Send + Sync + 'static> Signal<T> {
    /// Overwrites the stored value only if it differs from the current value.
    ///
    /// Returns `true` if the value changed and dirty flags were pushed, `false` otherwise.
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
