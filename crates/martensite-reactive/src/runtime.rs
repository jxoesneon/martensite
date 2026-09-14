//! Ambient reactive runtime managing DAG execution, batching, and evaluation contexts.
#![forbid(unsafe_code)]
// `missing_const_for_thread_local` false-positives on targets without
// `#[thread_local]` attribute support (e.g. Android): the initializers
// are already `const`, but std's fallback expansion makes clippy see a
// non-const path. Allowed module-wide so `-D warnings` stays green on
// every target.
#![allow(clippy::missing_const_for_thread_local)]

use std::cell::RefCell;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, OnceLock};

use parking_lot::Mutex;

use crate::cycle::CycleError;
use crate::effect::Effect;
#[cfg(feature = "devtools-timemachine")]
use crate::journal::{JournalGuard, SignalSnapshot, SourceAccess, SourceJournal};
use crate::memo::Memo;
#[cfg(feature = "devtools-timemachine")]
use crate::scheduler::FastBuildHasher;
use crate::scheduler::SchedulerState;
use crate::signal::{Signal, SignalId};

thread_local! {
    static ACTIVE_EVAL_STACK: RefCell<Vec<SignalId>> = const { RefCell::new(Vec::new()) };
    static ACTIVE_READ_JOURNAL: RefCell<Vec<smallvec::SmallVec<[SignalId; 4]>>> = const { RefCell::new(Vec::new()) };
    static AMBIENT_RUNTIME: RefCell<Option<Arc<ReactiveRuntime>>> = const { RefCell::new(None) };
}

static GLOBAL_RUNTIME: OnceLock<Arc<ReactiveRuntime>> = OnceLock::new();

/// Trait for dynamic reactive node evaluation.
///
/// Implementors are stored inside the scheduler as trait objects and invoked during
/// Phase 2 topological evaluation. [`Memo`] and [`Effect`]
/// provide their own implementations; custom reactive nodes can plug in here.
///
/// # Examples
///
/// ```
/// use martensite_reactive::{NodeEvaluator, ReactiveRuntime, SignalId};
/// use std::sync::Arc;
///
/// struct Counter {
///     count: std::sync::atomic::AtomicU32,
/// }
///
/// impl NodeEvaluator for Counter {
///     fn evaluate(&self) -> bool {
///         let prev = self.count.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
///         // Report "changed" whenever the counter actually advanced.
///         prev != self.count.load(std::sync::atomic::Ordering::SeqCst)
///     }
/// }
///
/// let runtime = ReactiveRuntime::new();
/// let id = SignalId::next();
/// runtime.register_derived(id, Arc::new(Counter { count: 0.into() }));
/// assert!(runtime.evaluate_node(id));
/// ```
pub trait NodeEvaluator: Send + Sync {
    /// Evaluates the node and returns `true` if the derived value changed.
    fn evaluate(&self) -> bool;
}

/// Runtime errors captured during signal DAG operations and cycle detection.
///
/// # Examples
///
/// ```
/// use martensite_reactive::{ReactiveError, SignalId};
///
/// let err = ReactiveError::PoisonedNode(SignalId::next());
/// assert!(err.to_string().contains("poisoned"));
/// ```
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ReactiveError {
    /// A cyclic dependency was detected and isolated.
    Cycle(CycleError),
    /// An operation targeted a poisoned node.
    PoisonedNode(SignalId),
}

impl std::fmt::Display for ReactiveError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Cycle(err) => write!(f, "{}", err),
            Self::PoisonedNode(id) => write!(f, "node {:?} is poisoned by circuit breaker", id),
        }
    }
}

impl std::error::Error for ReactiveError {}

/// Central reactive engine coordinating signal storage, topological scheduling, and transactional batches.
///
/// A `ReactiveRuntime` owns the scheduler DAG and the batch transaction depth. Most
/// applications use the ambient runtime via [`Signal::new`](crate::Signal::new) and the
/// `create_*` free functions, but constructing a dedicated runtime is useful for tests
/// and for isolating independent reactive graphs.
///
/// # Examples
///
/// ```
/// use martensite_reactive::ReactiveRuntime;
///
/// let runtime = ReactiveRuntime::new();
/// let count = runtime.create_signal(0);
/// let doubled = runtime.create_memo({
///     let count = count.clone();
///     move || count.get() * 2
/// });
///
/// assert_eq!(doubled.get(), 0);
/// count.set(5);
/// assert_eq!(doubled.get(), 10);
/// ```
pub struct ReactiveRuntime {
    state: Mutex<SchedulerState>,
    batch_depth: AtomicUsize,
    /// Source-write journal recording `Signal::set` mutations.
    #[cfg(feature = "devtools-timemachine")]
    journal: Mutex<SourceJournal>,
    /// Type-erased accessors to live source-signal storage, used to
    /// capture and restore [`SignalSnapshot`]s.
    #[cfg(feature = "devtools-timemachine")]
    source_access: Mutex<std::collections::HashMap<SignalId, SourceAccess, FastBuildHasher>>,
}

impl Default for ReactiveRuntime {
    fn default() -> Self {
        Self {
            state: Mutex::new(SchedulerState::new()),
            batch_depth: AtomicUsize::new(0),
            #[cfg(feature = "devtools-timemachine")]
            journal: Mutex::new(SourceJournal::new()),
            #[cfg(feature = "devtools-timemachine")]
            source_access: Mutex::new(std::collections::HashMap::with_hasher(
                FastBuildHasher::default(),
            )),
        }
    }
}

impl ReactiveRuntime {
    /// Creates a new, isolated reactive runtime wrapped in an `Arc`.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite_reactive::ReactiveRuntime;
    ///
    /// let runtime = ReactiveRuntime::new();
    /// let signal = runtime.create_signal(true);
    /// assert_eq!(signal.get_untracked(), true);
    /// ```
    pub fn new() -> Arc<Self> {
        Arc::new(Self::default())
    }

    /// Returns the current thread's ambient runtime, falling back to the global singleton.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite_reactive::ReactiveRuntime;
    /// use std::sync::Arc;
    ///
    /// // The ambient runtime is lazily initialized on first access.
    /// let a = ReactiveRuntime::current();
    /// let b = ReactiveRuntime::current();
    /// assert!(Arc::ptr_eq(&a, &b), "repeated calls return the same singleton");
    /// ```
    pub fn current() -> Arc<Self> {
        AMBIENT_RUNTIME.with(|ambient| {
            if let Some(rt) = ambient.borrow().as_ref() {
                return Arc::clone(rt);
            }
            GLOBAL_RUNTIME.get_or_init(Self::new).clone()
        })
    }

    /// Overrides the thread-local ambient runtime with the specified instance.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite_reactive::ReactiveRuntime;
    /// use std::sync::Arc;
    ///
    /// let runtime = ReactiveRuntime::new();
    /// ReactiveRuntime::set_current(&runtime);
    /// assert!(Arc::ptr_eq(&ReactiveRuntime::current(), &runtime));
    /// ```
    pub fn set_current(runtime: &Arc<Self>) {
        AMBIENT_RUNTIME.with(|ambient| {
            *ambient.borrow_mut() = Some(Arc::clone(runtime));
        });
    }

    /// Executes a closure within the scope of a specified ambient runtime.
    ///
    /// The previous ambient runtime is restored when the closure returns or unwinds,
    /// making this safe for nested scopes.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite_reactive::{ReactiveRuntime, Signal};
    ///
    /// let isolated = ReactiveRuntime::new();
    /// let outer = Signal::new(0);
    ///
    /// ReactiveRuntime::with_current(&isolated, || {
    ///     let inner = Signal::new(99);
    ///     assert_eq!(inner.get_untracked(), 99);
    /// });
    ///
    /// // Back in the outer ambient runtime.
    /// assert_eq!(outer.get_untracked(), 0);
    /// ```
    pub fn with_current<R>(runtime: &Arc<Self>, f: impl FnOnce() -> R) -> R {
        let prev =
            AMBIENT_RUNTIME.with(|ambient| ambient.borrow_mut().replace(Arc::clone(runtime)));
        struct ResetGuard(Option<Arc<ReactiveRuntime>>);
        impl Drop for ResetGuard {
            fn drop(&mut self) {
                AMBIENT_RUNTIME.with(|ambient| {
                    *ambient.borrow_mut() = self.0.take();
                });
            }
        }
        let _guard = ResetGuard(prev);
        f()
    }

    /// Creates a new state signal bound to this runtime.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite_reactive::ReactiveRuntime;
    ///
    /// let runtime = ReactiveRuntime::new();
    /// let greeting = runtime.create_signal(String::from("hi"));
    /// assert_eq!(greeting.get_untracked(), "hi");
    /// ```
    pub fn create_signal<T: Send + Sync + 'static>(self: &Arc<Self>, initial: T) -> Signal<T> {
        Signal::new_with_runtime(initial, Arc::clone(self))
    }

    /// Creates a new derived memo node bound to this runtime.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite_reactive::ReactiveRuntime;
    ///
    /// let runtime = ReactiveRuntime::new();
    /// let base = runtime.create_signal(3);
    /// let next = runtime.create_memo({
    ///     let base = base.clone();
    ///     move || base.get() + 1
    /// });
    ///
    /// assert_eq!(next.get(), 4);
    /// ```
    pub fn create_memo<T: Send + Sync + 'static>(
        self: &Arc<Self>,
        eval: impl Fn() -> T + Send + Sync + 'static,
    ) -> Memo<T> {
        Memo::new_with_runtime(eval, Arc::clone(self))
    }

    /// Creates a new reactive side-effect bound to this runtime.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite_reactive::ReactiveRuntime;
    /// use std::sync::atomic::{AtomicUsize, Ordering};
    /// use std::sync::Arc;
    ///
    /// let runtime = ReactiveRuntime::new();
    /// let value = runtime.create_signal(0);
    /// let runs = Arc::new(AtomicUsize::new(0));
    /// let runs_for_effect = runs.clone();
    /// let _effect = runtime.create_effect({
    ///     let value = value.clone();
    ///     move || {
    ///         let _ = value.get();
    ///         runs_for_effect.fetch_add(1, Ordering::SeqCst);
    ///     }
    /// });
    ///
    /// assert_eq!(runs.load(Ordering::SeqCst), 1);
    /// ```
    pub fn create_effect(self: &Arc<Self>, effect: impl FnMut() + Send + Sync + 'static) -> Effect {
        Effect::new_with_runtime(effect, Arc::clone(self))
    }

    /// Registers a source state signal node.
    pub fn register_source(&self, id: SignalId) {
        self.state.lock().register_source(id);
    }

    /// Registers a derived memo or effect node with an evaluation runner.
    pub fn register_derived(&self, id: SignalId, evaluator: Arc<dyn NodeEvaluator + Send + Sync>) {
        self.state.lock().register_derived(id, evaluator);
    }

    /// Registers a derived memo or effect node with an attached evaluator and atomic dirty flag mirror.
    pub fn register_derived_with_flag(
        &self,
        id: SignalId,
        evaluator: Arc<dyn NodeEvaluator + Send + Sync>,
        dirty_flag: Option<Arc<std::sync::atomic::AtomicBool>>,
    ) {
        self.state
            .lock()
            .register_derived_with_flag(id, evaluator, dirty_flag);
    }

    /// Unregisters a node and unlinks its dependency subscriptions.
    pub fn unregister_node(&self, id: SignalId) {
        self.state.lock().unregister_node(id);
    }

    /// Checks whether a node is flagged dirty.
    pub fn is_dirty(&self, id: SignalId) -> bool {
        self.state.lock().nodes.get(&id).is_some_and(|n| n.is_dirty)
    }

    /// Checks whether a node has been poisoned by the circuit breaker.
    pub fn is_poisoned(&self, id: SignalId) -> bool {
        self.state.lock().is_poisoned(id)
    }

    /// Flags a node as poisoned by the circuit breaker.
    pub fn poison_node(&self, id: SignalId) {
        self.state.lock().poison_node(id);
    }

    /// Explicitly triggers post-evaluation dependency pruning on a node.
    pub fn post_eval_prune(&self, id: SignalId) {
        self.state.lock().post_eval_prune(id);
    }

    /// Directly marks a specific node dirty without triggering propagation (test utility).
    pub fn mark_node_dirty_for_test(&self, id: SignalId) {
        if let Some(node) = self.state.lock().nodes.get_mut(&id) {
            node.is_dirty = true;
            if let Some(ref flag) = node.dirty_flag {
                flag.store(true, Ordering::Release);
            }
        }
    }

    /// Sets the eval epoch of a node directly (test utility).
    pub fn set_eval_epoch_for_test(&self, id: SignalId, epoch: u32) {
        if let Some(node) = self.state.lock().nodes.get_mut(&id) {
            node.eval_epoch = epoch;
        }
    }

    /// Directly invokes run_evaluator (test utility).
    pub fn run_evaluator_for_test(&self, id: SignalId, evaluator: &dyn NodeEvaluator) -> bool {
        self.run_evaluator(id, evaluator)
    }

    /// Returns true if the current thread is actively within a reactive node evaluation context.
    #[inline(always)]
    pub fn is_evaluating(&self) -> bool {
        ACTIVE_EVAL_STACK.with(|s| !s.borrow().is_empty())
    }

    /// Marks a source signal dirty and triggers Phase 1 push propagation.
    ///
    /// If outside a batch, Phase 2 topological evaluation is immediately flushed.
    pub fn mark_dirty(&self, source: SignalId) {
        {
            let mut state = self.state.lock();
            state.mark_dirty_bfs(source);
        }

        if self.batch_depth.load(Ordering::SeqCst) == 0 {
            self.flush();
        }
    }

    /// Tracks a signal read during the current evaluation, registering a dependency.
    ///
    /// Called internally by `Signal::get` and `Memo::get` to record that the
    /// currently-evaluating node depends on `id`. Cycle detection is performed
    /// and the read is pruned if a cycle is detected.
    pub fn track_read(&self, id: SignalId) {
        let (caller, is_cycle) = ACTIVE_EVAL_STACK.with(|stack| {
            let s = stack.borrow();
            let Some(&caller) = s.last() else {
                return (None, false);
            };
            let is_cycle = caller == id || s.contains(&id);
            (Some(caller), is_cycle)
        });

        let Some(caller) = caller else {
            return;
        };

        if is_cycle {
            let mut state = self.state.lock();
            state.poison_node(caller);
            let err = CycleError {
                from: id,
                to: caller,
            };
            state.errors.push(ReactiveError::Cycle(err));
            return;
        }

        ACTIVE_READ_JOURNAL.with(|j| {
            let mut journals = j.borrow_mut();
            if let Some(journal) = journals.last_mut() {
                if !journal.contains(&id) {
                    journal.push(id);
                }
            }
        });
    }

    /// Manually links a dependency edge from `dep` to `parent` ($dep \to parent$).
    pub fn track_read_manual(&self, parent: SignalId, dep: SignalId) -> Result<(), CycleError> {
        let mut state = self.state.lock();
        state.add_dependency_link(parent, dep)
    }

    /// Evaluates a specific node, pulling dirty dependencies first if necessary.
    pub fn evaluate_node(&self, id: SignalId) -> bool {
        if self.is_poisoned(id) {
            return false;
        }

        // Lazy evaluation: ensure dirty dependencies are evaluated before this node
        let dirty_deps: Vec<SignalId> = {
            let state = self.state.lock();
            if let Some(node) = state.nodes.get(&id) {
                node.dependencies
                    .iter()
                    .map(|&(dep_id, _)| dep_id)
                    .filter(|dep_id| state.nodes.get(dep_id).is_some_and(|d| d.is_dirty))
                    .collect()
            } else {
                Vec::new()
            }
        };

        for dep_id in dirty_deps {
            self.evaluate_node(dep_id);
        }

        let evaluator = {
            let state = self.state.lock();
            state.nodes.get(&id).and_then(|n| n.evaluator.clone())
        };

        if let Some(evaluator) = evaluator {
            self.run_evaluator(id, &*evaluator)
        } else {
            false
        }
    }

    /// Evaluates a node within the 3-color active evaluation stack context.
    pub(crate) fn run_evaluator(&self, id: SignalId, evaluator: &dyn NodeEvaluator) -> bool {
        if self.is_poisoned(id) {
            return false;
        }

        let is_cycle = ACTIVE_EVAL_STACK.with(|stack| {
            let mut s = stack.borrow_mut();
            if s.contains(&id) {
                true
            } else {
                s.push(id);
                false
            }
        });

        if is_cycle {
            let mut state = self.state.lock();
            state.poison_node(id);
            state
                .errors
                .push(ReactiveError::Cycle(CycleError { from: id, to: id }));
            return false;
        }

        {
            let mut state = self.state.lock();
            if let Some(node) = state.nodes.get_mut(&id) {
                node.eval_epoch = node.eval_epoch.wrapping_add(1);
                if node.eval_epoch == 0 {
                    node.eval_epoch = 1;
                }
            }
        }

        ACTIVE_READ_JOURNAL.with(|j| j.borrow_mut().push(smallvec::SmallVec::new()));

        struct EvalGuard<'a> {
            runtime: &'a ReactiveRuntime,
            id: SignalId,
        }

        impl<'a> Drop for EvalGuard<'a> {
            fn drop(&mut self) {
                ACTIVE_EVAL_STACK.with(|stack| {
                    let mut s = stack.borrow_mut();
                    if let Some(pos) = s.iter().rposition(|&x| x == self.id) {
                        s.remove(pos);
                    }
                });
                let reads = ACTIVE_READ_JOURNAL.with(|j| j.borrow_mut().pop().unwrap_or_default());
                let mut state = self.runtime.state.lock();
                for dep in reads {
                    let _ = state.add_dependency_link(self.id, dep);
                }
                state.post_eval_prune(self.id);
                if let Some(node) = state.nodes.get_mut(&self.id) {
                    node.is_dirty = false;
                    if let Some(ref flag) = node.dirty_flag {
                        flag.store(false, Ordering::Release);
                    }
                }
            }
        }

        let _guard = EvalGuard { runtime: self, id };
        evaluator.evaluate()
    }

    /// Increments the batch transaction depth.
    pub fn begin_batch(&self) {
        self.batch_depth.fetch_add(1, Ordering::SeqCst);
    }

    /// Decrements the batch transaction depth. Returns `true` if the outermost batch completed.
    pub fn end_batch(&self) -> bool {
        self.batch_depth.fetch_sub(1, Ordering::SeqCst) == 1
    }

    /// Coalesces dirty notifications across a transactional batch closure.
    ///
    /// Phase 2 topological evaluation executes exactly once when the outermost batch completes.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite_reactive::ReactiveRuntime;
    /// use std::sync::atomic::{AtomicUsize, Ordering};
    /// use std::sync::Arc;
    ///
    /// let runtime = ReactiveRuntime::new();
    /// let a = runtime.create_signal(1);
    /// let b = runtime.create_signal(1);
    /// let runs = Arc::new(AtomicUsize::new(0));
    /// let runs_for_effect = runs.clone();
    /// runtime.create_effect({
    ///     let a = a.clone();
    ///     let b = b.clone();
    ///     move || {
    ///         let _ = (a.get(), b.get());
    ///         runs_for_effect.fetch_add(1, Ordering::SeqCst);
    ///     }
    /// });
    ///
    /// let before = runs.load(Ordering::SeqCst);
    /// runtime.batch(|| {
    ///     a.set(10);
    ///     b.set(20);
    ///     // Effect has not run yet inside the batch.
    ///     assert_eq!(runs.load(Ordering::SeqCst), before);
    /// });
    /// // A single coalesced execution after the batch completes.
    /// assert_eq!(runs.load(Ordering::SeqCst), before + 1);
    /// ```
    pub fn batch<R>(&self, f: impl FnOnce() -> R) -> R {
        self.begin_batch();
        struct BatchGuard<'a>(&'a ReactiveRuntime);
        impl<'a> Drop for BatchGuard<'a> {
            fn drop(&mut self) {
                if self.0.end_batch() {
                    self.0.flush();
                }
            }
        }
        let _guard = BatchGuard(self);
        f()
    }

    /// Phase 2: Pull and evaluate all pending dirty nodes in strictly ascending topological depth rank order.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite_reactive::ReactiveRuntime;
    /// use std::sync::atomic::{AtomicUsize, Ordering};
    /// use std::sync::Arc;
    ///
    /// let runtime = ReactiveRuntime::new();
    /// let value = runtime.create_signal(0);
    /// let runs = Arc::new(AtomicUsize::new(0));
    /// let runs_for_effect = runs.clone();
    /// runtime.create_effect({
    ///     let value = value.clone();
    ///     move || {
    ///         let _ = value.get();
    ///         runs_for_effect.fetch_add(1, Ordering::SeqCst);
    ///     }
    /// });
    ///
    /// let before = runs.load(Ordering::SeqCst);
    /// // Enter a batch so writes are deferred, then flush on exit.
    /// runtime.batch(|| {
    ///     value.set(7);
    /// });
    /// // The batch already flushed on exit; calling flush again is a no-op.
    /// runtime.flush();
    /// assert_eq!(runs.load(Ordering::SeqCst), before + 1);
    /// ```
    pub fn flush(&self) {
        loop {
            let next = {
                let mut state = self.state.lock();
                state.pop_next_pending()
            };

            let Some((_rank, id, evaluator)) = next else {
                break;
            };

            if let Some(evaluator) = evaluator {
                self.run_evaluator(id, &*evaluator);
            }
        }
    }

    /// Runs full 3-color DFS cycle detection over the reactive graph.
    ///
    /// Returns `Ok(())` when the graph is acyclic, or the first detected
    /// [`CycleError`] otherwise.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite_reactive::ReactiveRuntime;
    ///
    /// let runtime = ReactiveRuntime::new();
    /// let _signal = runtime.create_signal(1);
    /// // An acyclic graph reports no cycles.
    /// assert!(runtime.detect_cycles().is_ok());
    /// ```
    pub fn detect_cycles(&self) -> Result<(), CycleError> {
        self.state.lock().detect_cycles()
    }

    /// Returns a copy of all accumulated errors.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite_reactive::ReactiveRuntime;
    ///
    /// let runtime = ReactiveRuntime::new();
    /// let _signal = runtime.create_signal(1);
    /// // A healthy graph accumulates no errors.
    /// assert!(runtime.errors().is_empty());
    /// ```
    pub fn errors(&self) -> Vec<ReactiveError> {
        self.state.lock().errors.clone()
    }

    /// Clears the accumulated error log.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite_reactive::ReactiveRuntime;
    ///
    /// let runtime = ReactiveRuntime::new();
    /// runtime.clear_errors();
    /// assert!(runtime.errors().is_empty());
    /// ```
    pub fn clear_errors(&self) {
        self.state.lock().errors.clear();
    }

    /// Registers a type-erased accessor for a source signal's storage.
    ///
    /// Called lazily by `Signal`'s `Clone`-bounded read/write methods
    /// (first `get`/`get_untracked`/`set_if_changed`) so the source
    /// participates in [`snapshot_signals`](Self::snapshot_signals)
    /// and [`restore_signals`](Self::restore_signals). Re-registering
    /// the same `id` replaces the previous accessor.
    #[cfg(feature = "devtools-timemachine")]
    pub(crate) fn register_source_access<T: Clone + Send + Sync + 'static>(
        &self,
        id: SignalId,
        value: &Arc<parking_lot::RwLock<T>>,
    ) {
        let read = {
            let value = Arc::downgrade(value);
            move || -> Option<Box<dyn std::any::Any + Send + Sync>> {
                let value = value.upgrade()?;
                let snapshot = value.read().clone();
                Some(Box::new(snapshot))
            }
        };
        let write = {
            let value = Arc::downgrade(value);
            move |new: &(dyn std::any::Any + Send + Sync)| -> bool {
                let Some(value) = value.upgrade() else {
                    return false;
                };
                let Some(new) = new.downcast_ref::<T>() else {
                    return false;
                };
                *value.write() = new.clone();
                true
            }
        };
        self.source_access.lock().insert(
            id,
            SourceAccess {
                read: Box::new(read),
                write: Box::new(write),
            },
        );
    }

    /// Records a `Signal::set` write in the source journal.
    ///
    /// `previous` is the value the signal held immediately before the
    /// write. The record is dropped when the journal is suppressed by a
    /// [`JournalGuard`].
    #[cfg(feature = "devtools-timemachine")]
    pub(crate) fn record_source_write(
        &self,
        id: SignalId,
        previous: Box<dyn std::any::Any + Send + Sync>,
    ) {
        self.journal.lock().record(id, previous);
    }

    /// Returns `true` while source-write journaling is suppressed —
    /// lets `Signal::set` skip boxing the previous value when the
    /// record would be discarded anyway. The check inside
    /// [`record_source_write`](Self::record_source_write) remains
    /// authoritative; this fast path only elides the allocation.
    #[cfg(feature = "devtools-timemachine")]
    pub(crate) fn journal_suppressed(&self) -> bool {
        self.journal.lock().is_suppressed()
    }

    /// Returns a lock guard on this runtime's [`SourceJournal`].
    ///
    /// The journal records every `Signal::set` and `Signal::set_if_changed`
    /// write with a monotonic index. `Signal::update` mutations are not
    /// journaled (no capturable previous value); [`Memo`](crate::Memo)
    /// recomputation is never journaled — derived values recompute lazily.
    ///
    /// **Deadlock warning**: `Signal::set`/`set_if_changed` take the
    /// journal lock while writing. Do not call `set` on any signal
    /// bound to this runtime while holding the returned guard — the
    /// write will block on the journal lock you already hold.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite_reactive::ReactiveRuntime;
    ///
    /// let runtime = ReactiveRuntime::new();
    /// let count = runtime.create_signal(0);
    /// count.set(5);
    /// assert_eq!(runtime.journal().len(), 1);
    /// ```
    #[cfg(feature = "devtools-timemachine")]
    pub fn journal(&self) -> parking_lot::MutexGuard<'_, SourceJournal> {
        self.journal.lock()
    }

    /// Suppresses source-write journaling until the returned guard drops.
    ///
    /// This is the `debug::disable`-style guard of the replay determinism
    /// contract: while a [`JournalGuard`] is alive, `Signal::set` writes
    /// are applied to state but not journaled, so replayed commands do
    /// not re-journal themselves. Guards nest — recording resumes once
    /// every guard has dropped. Suppression is **runtime-global**, not
    /// thread-scoped.
    ///
    /// **Deadlock warning**: acquiring the guard locks the journal
    /// mutex. Do not call this while holding the [`journal`](Self::journal)
    /// guard — `JournalGuard::new` will block on the lock you already
    /// hold. For the same reason, do not [`std::mem::forget`] a guard:
    /// it leaks the suppression permanently.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite_reactive::ReactiveRuntime;
    ///
    /// let runtime = ReactiveRuntime::new();
    /// let count = runtime.create_signal(0);
    ///
    /// {
    ///     let _guard = runtime.suppress_journal();
    ///     count.set(1);
    /// }
    /// assert_eq!(runtime.journal().len(), 0, "suppressed write not journaled");
    /// ```
    #[cfg(feature = "devtools-timemachine")]
    pub fn suppress_journal(&self) -> JournalGuard<'_> {
        JournalGuard::new(&self.journal)
    }

    /// Captures a [`SignalSnapshot`] of every registered source signal.
    ///
    /// Sources register a type-erased accessor **lazily** — on their
    /// first `Clone`-bounded read or write (`get`, `get_untracked`,
    /// `set_if_changed`), because only `Clone` payloads can be copied
    /// out of storage. Consequence: a source that has never been read
    /// or `set_if_changed` is absent from the snapshot and is *not*
    /// restored by [`restore_signals`](Self::restore_signals). Sources
    /// wired into [`Memo`](crate::Memo)/[`Effect`](crate::Effect)
    /// evaluation register on their first pull. Dropped signals are
    /// skipped and their accessors pruned. Entries are sorted by
    /// [`SignalId`] so snapshot iteration order is deterministic.
    ///
    /// The capture is **not atomic** against concurrent writers: each
    /// signal's value is read under its own lock, so a `Signal::set`
    /// racing `snapshot_signals` may yield a snapshot mixing pre- and
    /// post-write values across signals. Call at a quiescent point.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite_reactive::ReactiveRuntime;
    ///
    /// let runtime = ReactiveRuntime::new();
    /// let a = runtime.create_signal(1i32);
    /// let b = runtime.create_signal(String::from("x"));
    /// // Sources register lazily on their first read.
    /// let _ = a.get_untracked();
    /// let _ = b.get_untracked();
    ///
    /// let snapshot = runtime.snapshot_signals();
    /// assert_eq!(snapshot.len(), 2);
    /// assert_eq!(snapshot.get::<i32>(a.id()), Some(&1));
    /// ```
    #[cfg(feature = "devtools-timemachine")]
    pub fn snapshot_signals(&self) -> SignalSnapshot {
        let mut access = self.source_access.lock();
        let mut values = Vec::with_capacity(access.len());
        let mut dead = Vec::new();
        for (&id, entry) in access.iter() {
            match (entry.read)() {
                Some(value) => values.push((id, value)),
                None => dead.push(id),
            }
        }
        for id in dead {
            access.remove(&id);
        }
        values.sort_by_key(|(id, _)| *id);
        SignalSnapshot::from_parts(values)
    }

    /// Restores source-signal values from a [`SignalSnapshot`].
    ///
    /// Each restored value is written directly into the signal's storage
    /// (bypassing `Signal::set`, so no journal records are produced) and
    /// the signal is marked dirty *without* flushing — downstream
    /// [`Memo`](crate::Memo)s and effects recompute lazily during the
    /// pull phase. Signals that were dropped or whose payload type
    /// mismatches are skipped. Returns the number of signals restored.
    ///
    /// The restore is **not atomic** against concurrent writers: a
    /// `Signal::set` racing `restore_signals` can interleave with the
    /// per-signal writes. Call at a quiescent point (e.g., under
    /// `TimeMachine::replay_to` on the UI thread).
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite_reactive::ReactiveRuntime;
    ///
    /// let runtime = ReactiveRuntime::new();
    /// let count = runtime.create_signal(0i32);
    /// let _ = count.get_untracked(); // register the source
    /// let snapshot = runtime.snapshot_signals();
    ///
    /// count.set(42);
    /// assert_eq!(runtime.restore_signals(&snapshot), 1);
    /// assert_eq!(count.get_untracked(), 0);
    /// ```
    #[cfg(feature = "devtools-timemachine")]
    pub fn restore_signals(&self, snapshot: &SignalSnapshot) -> usize {
        let mut restored = Vec::with_capacity(snapshot.len());
        {
            let access = self.source_access.lock();
            for (id, value) in snapshot.iter() {
                if access.get(&id).is_some_and(|a| (a.write)(value)) {
                    restored.push(id);
                }
            }
        }
        {
            let mut state = self.state.lock();
            for id in restored.iter().copied() {
                state.mark_dirty_bfs(id);
            }
        }
        restored.len()
    }
}

/// Creates a new state signal bound to the ambient runtime.
///
/// # Examples
///
/// ```
/// use martensite_reactive::{create_signal, create_memo};
///
/// let count = create_signal(0);
/// let next = create_memo({
///     let count = count.clone();
///     move || count.get() + 1
/// });
///
/// assert_eq!(next.get(), 1);
/// count.set(9);
/// assert_eq!(next.get(), 10);
/// ```
pub fn create_signal<T: Send + Sync + 'static>(initial: T) -> Signal<T> {
    ReactiveRuntime::current().create_signal(initial)
}

/// Creates a new derived memo bound to the ambient runtime.
///
/// # Examples
///
/// ```
/// use martensite_reactive::{create_memo, create_signal};
///
/// let a = create_signal(2);
/// let b = create_signal(3);
/// let sum = create_memo({
///     let a = a.clone();
///     let b = b.clone();
///     move || a.get() + b.get()
/// });
///
/// assert_eq!(sum.get(), 5);
/// ```
pub fn create_memo<T: Send + Sync + 'static>(
    eval: impl Fn() -> T + Send + Sync + 'static,
) -> Memo<T> {
    ReactiveRuntime::current().create_memo(eval)
}

/// Creates a new reactive side-effect bound to the ambient runtime.
///
/// # Examples
///
/// ```
/// use martensite_reactive::{create_effect, create_signal};
/// use std::sync::atomic::{AtomicUsize, Ordering};
/// use std::sync::Arc;
///
/// let value = create_signal(0);
/// let runs = Arc::new(AtomicUsize::new(0));
/// let runs_for_effect = runs.clone();
/// create_effect({
///     let value = value.clone();
///     move || {
///         let _ = value.get();
///         runs_for_effect.fetch_add(1, Ordering::SeqCst);
///     }
/// });
///
/// assert_eq!(runs.load(Ordering::SeqCst), 1);
/// value.set(1);
/// assert_eq!(runs.load(Ordering::SeqCst), 2);
/// ```
pub fn create_effect(effect: impl FnMut() + Send + Sync + 'static) -> Effect {
    ReactiveRuntime::current().create_effect(effect)
}

/// Coalesces state updates across a transactional batch closure.
///
/// # Examples
///
/// ```
/// use martensite_reactive::{batch, create_effect, create_signal};
/// use std::sync::atomic::{AtomicUsize, Ordering};
/// use std::sync::Arc;
///
/// let a = create_signal(1);
/// let b = create_signal(1);
/// let runs = Arc::new(AtomicUsize::new(0));
/// let runs_for_effect = runs.clone();
/// create_effect({
///     let a = a.clone();
///     let b = b.clone();
///     move || {
///         let _ = (a.get(), b.get());
///         runs_for_effect.fetch_add(1, Ordering::SeqCst);
///     }
/// });
///
/// let before = runs.load(Ordering::SeqCst);
/// batch(|| {
///     a.set(10);
///     b.set(20);
///     // No execution yet inside the batch.
///     assert_eq!(runs.load(Ordering::SeqCst), before);
/// });
/// // Exactly one coalesced execution after the batch.
/// assert_eq!(runs.load(Ordering::SeqCst), before + 1);
/// ```
pub fn batch<R>(f: impl FnOnce() -> R) -> R {
    ReactiveRuntime::current().batch(f)
}

/// Flushes all pending dirty nodes in the ambient runtime.
///
/// # Examples
///
/// ```
/// use martensite_reactive::{batch, create_effect, create_signal, flush};
/// use std::sync::atomic::{AtomicUsize, Ordering};
/// use std::sync::Arc;
///
/// let value = create_signal(0);
/// let runs = Arc::new(AtomicUsize::new(0));
/// let runs_for_effect = runs.clone();
/// create_effect({
///     let value = value.clone();
///     move || {
///         let _ = value.get();
///         runs_for_effect.fetch_add(1, Ordering::SeqCst);
///     }
/// });
///
/// let before = runs.load(Ordering::SeqCst);
/// batch(|| value.set(42));
/// flush(); // no-op here, the batch already flushed on exit
/// assert_eq!(runs.load(Ordering::SeqCst), before + 1);
/// ```
pub fn flush() {
    ReactiveRuntime::current().flush();
}
