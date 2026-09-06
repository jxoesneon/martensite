//! Ambient reactive runtime managing DAG execution, batching, and evaluation contexts.
#![forbid(unsafe_code)]

use std::cell::RefCell;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, OnceLock};

use parking_lot::Mutex;

use crate::cycle::CycleError;
use crate::effect::Effect;
use crate::memo::Memo;
use crate::scheduler::SchedulerState;
use crate::signal::{Signal, SignalId};

thread_local! {
    static ACTIVE_EVAL_STACK: RefCell<Vec<SignalId>> = const { RefCell::new(Vec::new()) };
    static ACTIVE_READ_JOURNAL: RefCell<Vec<smallvec::SmallVec<[SignalId; 4]>>> = const { RefCell::new(Vec::new()) };
    static AMBIENT_RUNTIME: RefCell<Option<Arc<ReactiveRuntime>>> = const { RefCell::new(None) };
}

static GLOBAL_RUNTIME: OnceLock<Arc<ReactiveRuntime>> = OnceLock::new();

/// Trait for dynamic reactive node evaluation.
pub trait NodeEvaluator: Send + Sync {
    /// Evaluates the node and returns `true` if the derived value changed.
    fn evaluate(&self) -> bool;
}

/// Runtime errors captured during signal DAG operations and cycle detection.
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
pub struct ReactiveRuntime {
    state: Mutex<SchedulerState>,
    batch_depth: AtomicUsize,
}

impl Default for ReactiveRuntime {
    fn default() -> Self {
        Self {
            state: Mutex::new(SchedulerState::new()),
            batch_depth: AtomicUsize::new(0),
        }
    }
}

impl ReactiveRuntime {
    /// Creates a new, isolated reactive runtime wrapped in an `Arc`.
    pub fn new() -> Arc<Self> {
        Arc::new(Self::default())
    }

    /// Returns the current thread's ambient runtime, falling back to the global singleton.
    pub fn current() -> Arc<Self> {
        AMBIENT_RUNTIME.with(|ambient| {
            if let Some(rt) = ambient.borrow().as_ref() {
                return Arc::clone(rt);
            }
            GLOBAL_RUNTIME.get_or_init(Self::new).clone()
        })
    }

    /// Overrides the thread-local ambient runtime with the specified instance.
    pub fn set_current(runtime: &Arc<Self>) {
        AMBIENT_RUNTIME.with(|ambient| {
            *ambient.borrow_mut() = Some(Arc::clone(runtime));
        });
    }

    /// Executes a closure within the scope of a specified ambient runtime.
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
    pub fn create_signal<T: Send + Sync + 'static>(self: &Arc<Self>, initial: T) -> Signal<T> {
        Signal::new_with_runtime(initial, Arc::clone(self))
    }

    /// Creates a new derived memo node bound to this runtime.
    pub fn create_memo<T: Send + Sync + 'static>(
        self: &Arc<Self>,
        eval: impl Fn() -> T + Send + Sync + 'static,
    ) -> Memo<T> {
        Memo::new_with_runtime(eval, Arc::clone(self))
    }

    /// Creates a new reactive side-effect bound to this runtime.
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
    pub fn detect_cycles(&self) -> Result<(), CycleError> {
        self.state.lock().detect_cycles()
    }

    /// Returns a copy of all accumulated errors.
    pub fn errors(&self) -> Vec<ReactiveError> {
        self.state.lock().errors.clone()
    }

    /// Clears the accumulated error log.
    pub fn clear_errors(&self) {
        self.state.lock().errors.clear();
    }
}

/// Creates a new state signal bound to the ambient runtime.
pub fn create_signal<T: Send + Sync + 'static>(initial: T) -> Signal<T> {
    ReactiveRuntime::current().create_signal(initial)
}

/// Creates a new derived memo bound to the ambient runtime.
pub fn create_memo<T: Send + Sync + 'static>(
    eval: impl Fn() -> T + Send + Sync + 'static,
) -> Memo<T> {
    ReactiveRuntime::current().create_memo(eval)
}

/// Creates a new reactive side-effect bound to the ambient runtime.
pub fn create_effect(effect: impl FnMut() + Send + Sync + 'static) -> Effect {
    ReactiveRuntime::current().create_effect(effect)
}

/// Coalesces state updates across a transactional batch closure.
pub fn batch<R>(f: impl FnOnce() -> R) -> R {
    ReactiveRuntime::current().batch(f)
}

/// Flushes all pending dirty nodes in the ambient runtime.
pub fn flush() {
    ReactiveRuntime::current().flush();
}
