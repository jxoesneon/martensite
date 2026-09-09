//! Reactive side-effects with automatic dependency tracking and cancellation.
#![forbid(unsafe_code)]

use std::fmt;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

use parking_lot::Mutex;

use crate::runtime::{NodeEvaluator, ReactiveRuntime};
use crate::signal::SignalId;

struct EffectEvaluator {
    runner: Arc<Mutex<Box<dyn FnMut() + Send + Sync>>>,
    disposed: Arc<AtomicBool>,
}

impl NodeEvaluator for EffectEvaluator {
    fn evaluate(&self) -> bool {
        if self.disposed.load(Ordering::SeqCst) {
            return false;
        }
        let mut guard = self.runner.lock();
        guard();
        false
    }
}

/// A reactive side-effect that executes automatically whenever its tracked dependencies mutate.
///
/// An `Effect` runs its closure once on construction to establish its initial dependency
/// set, then re-runs it whenever any of those dependencies change. Effects are the
/// primary bridge between the reactive graph and the outside world (DOM updates,
/// logging, network requests, etc.).
///
/// # Examples
///
/// ```
/// use martensite_reactive::{Signal, create_effect};
/// use std::sync::atomic::{AtomicUsize, Ordering};
/// use std::sync::Arc;
///
/// let count = Signal::new(0);
/// let seen = Arc::new(AtomicUsize::new(0));
///
/// let seen_for_effect = seen.clone();
/// create_effect({
///     let count = count.clone();
///     move || {
///         let _ = count.get();
///         seen_for_effect.fetch_add(1, Ordering::SeqCst);
///     }
/// });
///
/// // The effect runs once immediately on creation.
/// assert_eq!(seen.load(Ordering::SeqCst), 1);
/// count.set(1);
/// assert_eq!(seen.load(Ordering::SeqCst), 2);
/// ```
#[derive(Clone)]
pub struct Effect {
    /// Unique identifier for this effect node in the dependency graph.
    pub id: SignalId,
    runtime: Arc<ReactiveRuntime>,
    disposed: Arc<AtomicBool>,
}

impl Effect {
    /// Creates a new reactive side-effect in the ambient runtime and triggers initial execution.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite_reactive::{Effect, Signal};
    /// use std::sync::atomic::{AtomicUsize, Ordering};
    /// use std::sync::Arc;
    ///
    /// let value = Signal::new(0);
    /// let runs = Arc::new(AtomicUsize::new(0));
    /// let runs_for_effect = runs.clone();
    /// let _effect = Effect::new({
    ///     let value = value.clone();
    ///     move || {
    ///         let _ = value.get();
    ///         runs_for_effect.fetch_add(1, Ordering::SeqCst);
    ///     }
    /// });
    ///
    /// assert_eq!(runs.load(Ordering::SeqCst), 1);
    /// value.set(42);
    /// assert_eq!(runs.load(Ordering::SeqCst), 2);
    /// ```
    pub fn new(effect: impl FnMut() + Send + Sync + 'static) -> Self {
        let runtime = ReactiveRuntime::current();
        Self::new_with_runtime(effect, runtime)
    }

    /// Creates a new reactive side-effect bound explicitly to a specified runtime.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite_reactive::{Effect, ReactiveRuntime, Signal};
    /// use std::sync::atomic::{AtomicUsize, Ordering};
    /// use std::sync::Arc;
    ///
    /// let runtime = ReactiveRuntime::new();
    /// let value = Signal::new_with_runtime(0, runtime.clone());
    /// let runs = Arc::new(AtomicUsize::new(0));
    /// let runs_for_effect = runs.clone();
    /// let _effect = Effect::new_with_runtime(
    ///     {
    ///         let value = value.clone();
    ///         move || {
    ///             let _ = value.get();
    ///             runs_for_effect.fetch_add(1, Ordering::SeqCst);
    ///         }
    ///     },
    ///     runtime,
    /// );
    ///
    /// assert_eq!(runs.load(Ordering::SeqCst), 1);
    /// ```
    pub fn new_with_runtime(
        effect: impl FnMut() + Send + Sync + 'static,
        runtime: Arc<ReactiveRuntime>,
    ) -> Self {
        let id = SignalId::next();
        let disposed = Arc::new(AtomicBool::new(false));
        let runner = Arc::new(Mutex::new(
            Box::new(effect) as Box<dyn FnMut() + Send + Sync>
        ));

        let evaluator = Arc::new(EffectEvaluator {
            runner,
            disposed: Arc::clone(&disposed),
        });

        runtime.register_derived(id, evaluator);
        runtime.evaluate_node(id);

        Self {
            id,
            runtime,
            disposed,
        }
    }

    /// Returns the unique `SignalId` for this effect.
    #[inline]
    #[must_use]
    pub fn id(&self) -> SignalId {
        self.id
    }

    /// Returns a reference to the bound `ReactiveRuntime`.
    #[inline]
    #[must_use]
    pub fn runtime(&self) -> &Arc<ReactiveRuntime> {
        &self.runtime
    }

    /// Explicitly triggers immediate evaluation of the side-effect.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite_reactive::{Effect, Signal};
    /// use std::sync::atomic::{AtomicUsize, Ordering};
    /// use std::sync::Arc;
    ///
    /// let value = Signal::new(0);
    /// let runs = Arc::new(AtomicUsize::new(0));
    /// let runs_for_effect = runs.clone();
    /// let effect = Effect::new({
    ///     let value = value.clone();
    ///     move || {
    ///         let _ = value.get();
    ///         runs_for_effect.fetch_add(1, Ordering::SeqCst);
    ///     }
    /// });
    ///
    /// assert_eq!(runs.load(Ordering::SeqCst), 1);
    /// effect.run(); // manually re-run without changing a dependency
    /// assert_eq!(runs.load(Ordering::SeqCst), 2);
    /// ```
    pub fn run(&self) {
        if !self.is_disposed() {
            self.runtime.evaluate_node(self.id);
        }
    }

    /// Disposes of this effect, unlinking all dependency subscriptions and preventing future executions.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite_reactive::{Effect, Signal};
    /// use std::sync::atomic::{AtomicUsize, Ordering};
    /// use std::sync::Arc;
    ///
    /// let value = Signal::new(0);
    /// let runs = Arc::new(AtomicUsize::new(0));
    /// let runs_for_effect = runs.clone();
    /// let effect = Effect::new({
    ///     let value = value.clone();
    ///     move || {
    ///         let _ = value.get();
    ///         runs_for_effect.fetch_add(1, Ordering::SeqCst);
    ///     }
    /// });
    ///
    /// assert_eq!(runs.load(Ordering::SeqCst), 1);
    /// effect.dispose();
    /// assert!(effect.is_disposed());
    /// value.set(99); // no further executions after disposal
    /// assert_eq!(runs.load(Ordering::SeqCst), 1);
    /// ```
    pub fn dispose(&self) {
        if !self.disposed.swap(true, Ordering::SeqCst) {
            self.runtime.unregister_node(self.id);
        }
    }

    /// Returns `true` if this effect has been cancelled or disposed.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite_reactive::Effect;
    ///
    /// let effect = Effect::new(|| {});
    /// assert!(!effect.is_disposed());
    /// effect.dispose();
    /// assert!(effect.is_disposed());
    /// ```
    #[inline(always)]
    pub fn is_disposed(&self) -> bool {
        self.disposed.load(Ordering::SeqCst)
    }
}

impl fmt::Debug for Effect {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Effect")
            .field("id", &self.id)
            .field("disposed", &self.is_disposed())
            .finish()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_evaluator_when_disposed() {
        let disposed = Arc::new(AtomicBool::new(true));
        let runner = Arc::new(Mutex::new(Box::new(|| ()) as Box<dyn FnMut() + Send + Sync>));
        let evaluator = EffectEvaluator { runner, disposed };
        assert!(!evaluator.evaluate());
    }
}
