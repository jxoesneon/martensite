//! Pure synchronous derived reactive computations with memoization.
#![forbid(unsafe_code)]

use std::fmt;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

use parking_lot::RwLock;

use crate::runtime::{NodeEvaluator, ReactiveRuntime};
use crate::signal::SignalId;

struct MemoInner<T> {
    cached_value: Option<T>,
    evaluator: Arc<dyn Fn() -> T + Send + Sync>,
}

struct MemoEvaluator<T> {
    inner: Arc<RwLock<MemoInner<T>>>,
}

impl<T: Send + Sync + 'static> NodeEvaluator for MemoEvaluator<T> {
    fn evaluate(&self) -> bool {
        let eval_fn = {
            let guard = self.inner.read();
            Arc::clone(&guard.evaluator)
        };
        let new_value = eval_fn();
        let mut guard = self.inner.write();
        guard.cached_value = Some(new_value);
        true
    }
}

/// A derived reactive node that lazily evaluates and caches a synchronous pure function.
pub struct Memo<T: 'static> {
    /// Unique identifier for this memo node in the dependency graph.
    pub id: SignalId,
    runtime: Arc<ReactiveRuntime>,
    inner: Arc<RwLock<MemoInner<T>>>,
    dirty_flag: Arc<AtomicBool>,
}

impl<T: 'static> Clone for Memo<T> {
    fn clone(&self) -> Self {
        Self {
            id: self.id,
            runtime: Arc::clone(&self.runtime),
            inner: Arc::clone(&self.inner),
            dirty_flag: Arc::clone(&self.dirty_flag),
        }
    }
}

impl<T: Send + Sync + 'static> Memo<T> {
    /// Creates a new `Memo` bound to the ambient reactive runtime and establishes initial dependencies.
    pub fn new(eval: impl Fn() -> T + Send + Sync + 'static) -> Self {
        let runtime = ReactiveRuntime::current();
        Self::new_with_runtime(eval, runtime)
    }

    /// Creates a new `Memo` bound to a specified `ReactiveRuntime`.
    pub fn new_with_runtime(
        eval: impl Fn() -> T + Send + Sync + 'static,
        runtime: Arc<ReactiveRuntime>,
    ) -> Self {
        let id = SignalId::next();
        let inner = Arc::new(RwLock::new(MemoInner {
            cached_value: None,
            evaluator: Arc::new(eval),
        }));

        let evaluator = Arc::new(MemoEvaluator {
            inner: Arc::clone(&inner),
        });

        let dirty_flag = Arc::new(AtomicBool::new(true));
        runtime.register_derived_with_flag(id, evaluator, Some(Arc::clone(&dirty_flag)));
        // Execute initial evaluation to establish initial dependency edges and rank
        runtime.evaluate_node(id);

        Self {
            id,
            runtime,
            inner,
            dirty_flag,
        }
    }

    /// Returns the unique `SignalId` for this memo node.
    #[inline(always)]
    pub fn id(&self) -> SignalId {
        self.id
    }

    /// Returns a reference to the bound `ReactiveRuntime`.
    #[inline(always)]
    pub fn runtime(&self) -> &Arc<ReactiveRuntime> {
        &self.runtime
    }

    fn ensure_clean(&self) {
        if !self.dirty_flag.load(Ordering::Acquire) {
            return;
        }
        if self.runtime.is_dirty(self.id) {
            self.runtime.evaluate_node(self.id);
        }
    }
}

impl<T: Clone + Send + Sync + 'static> Memo<T> {
    /// Reads the cached value, evaluating if dirty and registering this memo as a dependency
    /// to any actively evaluating ancestor.
    pub fn get(&self) -> T {
        self.runtime.track_read(self.id);
        self.ensure_clean();
        let guard = self.inner.read();
        guard
            .cached_value
            .as_ref()
            .expect("memo must contain a cached value after evaluation")
            .clone()
    }

    /// Reads the cached value without registering a dependency edge.
    pub fn get_untracked(&self) -> T {
        self.ensure_clean();
        let guard = self.inner.read();
        guard
            .cached_value
            .as_ref()
            .expect("memo must contain a cached value after evaluation")
            .clone()
    }
}

impl<T: fmt::Debug + Clone + Send + Sync + 'static> fmt::Debug for Memo<T> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Memo")
            .field("id", &self.id)
            .field("cached_value", &self.get_untracked())
            .finish()
    }
}

impl<T: fmt::Display + Clone + Send + Sync + 'static> fmt::Display for Memo<T> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.get_untracked())
    }
}
