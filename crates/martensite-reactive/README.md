# martensite-reactive

[![Crates.io](https://img.shields.io/crates/v/martensite-reactive.svg)](https://crates.io/crates/martensite-reactive)
[![Documentation](https://docs.rs/martensite-reactive/badge.svg)](https://docs.rs/martensite-reactive)
[![License](https://img.shields.io/badge/license-MIT%20OR%20Apache--2.0-blue.svg)](https://github.com/jxoesneon/martensite#license)

> **Fine-grained push-pull reactive signal DAG with transactional batching, topological scheduling, and 3-color DFS cycle detection.**

---

## Overview

`martensite-reactive` implements a pure-Rust, fine-grained reactive state graph for the Martensite GUI framework. Unlike virtual DOM diffing systems that re-execute broad subtree renders, Martensite uses a **push-pull DAG**:

1. **Push Phase**: When a `Signal<T>` mutates, it pushes dirty flags down its direct dependency edges in the DAG. No derived expressions or side-effects run during this phase.
2. **Pull Phase**: When readers query a `Memo<T>` or a render frame begins, only the dirty subgraphs are lazily evaluated in strict topological order. If a memo computes a value identical to its previous output, downstream propagation halts immediately.

The entire crate is built under `#![forbid(unsafe_code)]`, delivering airtight memory safety alongside zero-allocation dirty bitset propagation.

---

## Key Features

- **Push-Pull Reactive DAG**: Minimizes recalculation by combining eager dirty-marking with lazy, memoized pull-evaluation.
- **Topological Scheduling**: Dependencies are resolved strictly in topological order, guaranteeing that derived state never observes intermediate or glitchy states.
- **Dynamic Dependency Pruning**: Branching conditionals automatically register newly read signals and prune discarded dependency edges dynamically.
- **3-Color DFS Cycle Detection**: Cyclic dependencies are caught deterministically using 3-color depth-first search (`CycleError`), preventing infinite evaluation loops.
- **Transactional Batching**: The `batch` API coalesces multiple signal updates into a single atomic propagation pass.
- **100% Safe Rust**: Guaranteed memory safety without `unsafe` blocks.

---

## Quick Start

Add `martensite-reactive` to your `Cargo.toml`:

```toml
[dependencies]
martensite-reactive = "0.7.0"
```

Using signals, derived memos, and transactional batching:

```rust
use martensite_reactive::prelude::*;

fn main() {
    // Initialize reactive signals
    let count = create_signal(1);
    let multiplier = create_signal(10);

    // Create a derived memo with dynamic tracking
    let product = create_memo(move || count.get() * multiplier.get());

    assert_eq!(product.get(), 10);

    // Batch multiple mutations atomically
    batch(|| {
        count.set(5);
        multiplier.set(2);
    });

    // Evaluated lazily on read
    assert_eq!(product.get(), 10);
}
```

---

## Architectural Guarantees

```text
State Mutation Flow:
+-------------------+       push dirty bit       +-------------------+
|    Signal::set    | -------------------------> |  Dirty Bitset Set |
+-------------------+                            +-------------------+
                                                           |
                                                           v
+-------------------+       pull on-demand       +-------------------+
|    Memo::get      | <------------------------- | Topo Sort Engine  |
+-------------------+    (evaluate if dirty)     +-------------------+
```

- **Glitch-Free Updates**: Memos with multiple paths to the same signal update exactly once per transaction.
- **Early Exit Pruning**: If `Memo` output does not change (`PartialEq`), downstream dependents remain clean and are skipped.

---

## Part of Martensite

This crate powers reactive state synchronization across the [Martensite](https://github.com/jxoesneon/martensite) GUI framework. For top-level widgets and application integration, see [`martensite`](https://crates.io/crates/martensite).

---

## License

Licensed under either of:

- Apache License, Version 2.0 ([LICENSE-APACHE](https://github.com/jxoesneon/martensite/blob/main/LICENSE-APACHE) or <http://www.apache.org/licenses/LICENSE-2.0>)
- MIT license ([LICENSE-MIT](https://github.com/jxoesneon/martensite/blob/main/LICENSE-MIT) or <http://opensource.org/licenses/MIT>)

at your option.
