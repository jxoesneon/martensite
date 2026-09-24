# Tutorial 2 — Reactive state management

Martensite's reactivity lives in `martensite-reactive`: a fine-grained
**push-pull** signal DAG with transactional batching, topological
scheduling, dynamic dependency pruning, and 3-color DFS cycle detection.
Zero `unsafe`.

The three primitives:

| Primitive | Role | Created by |
| --- | --- | --- |
| `Signal<T>` | Writable source of truth | `Signal::new(v)` / `create_signal(v)` |
| `Memo<T>` | Cached derived value, lazily re-evaluated | `Memo::new(f)` / `create_memo(f)` |
| `Effect` | Side-effect re-run when dependencies change | `Effect::new(f)` / `create_effect(f)` |

All examples below compile against `martensite-reactive = "0.19.0"`.

## 1. Signals

```rust
use martensite_reactive::Signal;

let name = Signal::new(String::from("Ada"));

// `get` clones the value and registers a dependency when called inside
// a memo/effect. Outside a reactive context it just reads.
assert_eq!(name.get(), "Ada");

// `set` overwrites and marks downstream subscribers dirty.
name.set(String::from("Grace"));
assert_eq!(name.get(), "Grace");

// `update` mutates in place — no Clone needed for the write itself.
let count = Signal::new(0);
count.update(|c| *c += 1);
count.update(|c| *c += 1);
assert_eq!(count.get(), 2);
```

`Signal<T>` is `Clone` — cloning produces another handle to the *same*
underlying value (shared `Arc` storage), which is how closures capture
it:

```rust
let a = Signal::new(10);
let a2 = a.clone();
a2.set(20);
assert_eq!(a.get(), 20);
```

Bounds: `T: Send + Sync + 'static`. `get`/`get_untracked` additionally
require `T: Clone` (the value is cloned out of the lock). For a
conditional write that only dirties subscribers on change, use
`set_if_changed` (requires `T: PartialEq + Clone`):

```rust
let selected = Signal::new(3);
assert!(!selected.set_if_changed(3)); // no-op, returns false
assert!(selected.set_if_changed(7));  // changed, returns true
```

## 2. Memos — derived state

A `Memo` wraps an `Fn() -> T` closure. Dependency edges are recorded
automatically: any `signal.get()` inside the closure subscribes the memo
to that signal. Re-evaluation is lazy — it happens on the next `get()`
after a dependency is marked dirty, and the result is cached.

```rust
use martensite_reactive::{create_memo, Signal};

let a = Signal::new(10);
let b = Signal::new(32);

let sum = create_memo({
    let a = a.clone();
    let b = b.clone();
    move || a.get() + b.get()
});

assert_eq!(sum.get(), 42);
a.set(20);              // marks `sum` dirty
assert_eq!(sum.get(), 52); // re-evaluated on read
```

Use `get_untracked()` inside a memo when you want to read a signal
*without* subscribing — the memo will not re-evaluate when that signal
changes:

```rust
use martensite_reactive::{create_memo, Signal};

let a = Signal::new(7);
let independent = create_memo({
    let a = a.clone();
    move || a.get_untracked() + 100
});

assert_eq!(independent.get(), 107);
a.set(99);
// The memo never subscribed, so it keeps its cached value.
assert_eq!(independent.get(), 107);
```

## 3. Effects — side effects

`create_effect` runs the closure immediately, records its dependencies,
and re-runs it each time a dependency flushes dirty:

```rust
use martensite_reactive::{create_effect, create_signal};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;

let value = create_signal(0);
let runs = Arc::new(AtomicUsize::new(0));

create_effect({
    let value = value.clone();
    let runs = runs.clone();
    move || {
        let _ = value.get();
        runs.fetch_add(1, Ordering::SeqCst);
    }
});

assert_eq!(runs.load(Ordering::SeqCst), 1); // ran once on creation
value.set(1);
assert_eq!(runs.load(Ordering::SeqCst), 2); // re-ran on change
```

Effects are disposed with `effect.dispose()` (`is_disposed()` reports
the state); a disposed effect never re-runs.

## 4. Batching and flushing

Multiple writes inside `batch` coalesce into a single propagation — an
effect depending on both signals runs **once**, not twice:

```rust
use martensite_reactive::{batch, create_effect, create_signal};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;

let a = create_signal(1);
let b = create_signal(1);
let runs = Arc::new(AtomicUsize::new(0));

create_effect({
    let a = a.clone();
    let b = b.clone();
    let runs = runs.clone();
    move || {
        let _ = (a.get(), b.get());
        runs.fetch_add(1, Ordering::SeqCst);
    }
});

let before = runs.load(Ordering::SeqCst);
batch(|| {
    a.set(10);
    b.set(20);
    // Nothing has run yet inside the batch.
    assert_eq!(runs.load(Ordering::SeqCst), before);
});
// Exactly one coalesced run after the batch closed.
assert_eq!(runs.load(Ordering::SeqCst), before + 1);
```

`flush()` forces evaluation of all pending dirty nodes in the ambient
runtime; batch already flushes on exit, so `flush()` is for draining
work queued outside a batch.

## 5. Patterns and pitfalls

- **Custom widgets read signals in `measure`/`paint`.** The migration
  rule of thumb: hold state in `Signal<T>`, derive with `Memo<T>`, and
  read via `.get()` inside the widget pass that needs it.
- **Cycles are detected, not silently recursed.** A write path that
  would form a cycle surfaces `CycleError`/`ReactiveError` from the
  3-color DFS scheduler rather than hanging.
- **`get_untracked` is the escape hatch** for reads that must not create
  edges — but it also means the consumer goes stale, as shown above.
- **Isolation:** `Signal::new_with_runtime`,
  `Memo::new_with_runtime`, and `Effect::new_with_runtime` bind nodes to
  an explicit `Arc<ReactiveRuntime>` instead of the ambient one —
  useful for per-window graphs or tests.
- **Time-travel note:** with the `devtools-timemachine` feature,
  `Signal::set` additionally journals the previous value into the
  runtime's `SourceJournal` (an audit log, not a history command). This
  changes nothing about the read/write semantics above.

## Next steps

- [Tutorial 3 — Writing a custom widget](03-custom-widget.md)
