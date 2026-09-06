//! Fine-grained push-pull reactive signal DAG for Martensite.
#![forbid(unsafe_code)]

use std::marker::PhantomData;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use parking_lot::RwLock;

static NEXT_SIG_ID: AtomicU64 = AtomicU64::new(1);

#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash)]
pub struct SignalId(pub u64);

#[derive(Clone)]
pub struct Signal<T: Clone + 'static> {
    pub id: SignalId,
    value: Arc<RwLock<T>>,
    _marker: PhantomData<T>,
}

impl<T: Clone + 'static> Signal<T> {
    pub fn new(initial: T) -> Self {
        Self {
            id: SignalId(NEXT_SIG_ID.fetch_add(1, Ordering::Relaxed)),
            value: Arc::new(RwLock::new(initial)),
            _marker: PhantomData,
        }
    }

    #[inline(always)]
    pub fn get(&self) -> T {
        self.value.read().clone()
    }

    #[inline(always)]
    pub fn set(&self, val: T) {
        *self.value.write() = val;
    }

    #[inline(always)]
    pub fn update(&self, f: impl FnOnce(&mut T)) {
        f(&mut *self.value.write());
    }
}

pub struct Memo<T: Clone + 'static> {
    evaluator: Arc<dyn Fn() -> T + Send + Sync>,
}

impl<T: Clone + 'static> Memo<T> {
    pub fn new(eval: impl Fn() -> T + Send + Sync + 'static) -> Self {
        Self {
            evaluator: Arc::new(eval),
        }
    }

    #[inline(always)]
    pub fn get(&self) -> T {
        (self.evaluator)()
    }
}
