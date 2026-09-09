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
///
/// A `Memo` re-runs its evaluator only when one of the signals it read during its last
/// evaluation changes, and it caches the result so that repeated reads are cheap.
///
/// # Examples
///
/// ```
/// use martensite_reactive::{Signal, create_memo};
///
/// let width = Signal::new(4);
/// let height = Signal::new(6);
/// let area = create_memo({
///     let width = width.clone();
///     let height = height.clone();
///     move || width.get() * height.get()
/// });
///
/// assert_eq!(area.get(), 24);
/// width.set(10);
/// assert_eq!(area.get(), 60);
/// ```
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
    ///
    /// The evaluator runs once immediately to record its initial dependency set and cache
    /// the first value.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite_reactive::{Memo, Signal};
    ///
    /// let base = Signal::new(2i32);
    /// let squared = Memo::new({
    ///     let base = base.clone();
    ///     move || base.get().pow(2)
    /// });
    ///
    /// assert_eq!(squared.get(), 4);
    /// base.set(9);
    /// assert_eq!(squared.get(), 81);
    /// ```
    pub fn new(eval: impl Fn() -> T + Send + Sync + 'static) -> Self {
        let runtime = ReactiveRuntime::current();
        Self::new_with_runtime(eval, runtime)
    }

    /// Creates a new `Memo` bound to a specified `ReactiveRuntime`.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite_reactive::{Memo, ReactiveRuntime, Signal};
    ///
    /// let runtime = ReactiveRuntime::new();
    /// let value = Signal::new_with_runtime(3, runtime.clone());
    /// let doubled = Memo::new_with_runtime(
    ///     {
    ///         let value = value.clone();
    ///         move || value.get() * 2
    ///     },
    ///     runtime,
    /// );
    ///
    /// assert_eq!(doubled.get(), 6);
    /// ```
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
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite_reactive::{Signal, create_memo};
    ///
    /// let a = Signal::new(1);
    /// let b = Signal::new(2);
    /// let total = create_memo({
    ///     let a = a.clone();
    ///     let b = b.clone();
    ///     move || a.get() + b.get()
    /// });
    ///
    /// assert_eq!(total.get(), 3);
    /// a.set(10);
    /// assert_eq!(total.get(), 12);
    /// ```
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
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite_reactive::{Signal, create_memo};
    ///
    /// let a = Signal::new(5);
    /// let snapshot = create_memo({
    ///     let a = a.clone();
    ///     move || a.get()
    /// });
    ///
    /// // Read the current cached value without subscribing.
    /// let first = snapshot.get_untracked();
    /// assert_eq!(first, 5);
    /// ```
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
