# martensite-history

[![Crates.io](https://img.shields.io/crates/v/martensite-history.svg)](https://crates.io/crates/martensite-history)
[![Documentation](https://docs.rs/martensite-history/badge.svg)](https://docs.rs/martensite-history)
[![License](https://img.shields.io/badge/license-MIT%20OR%20Apache--2.0-blue.svg)](https://github.com/jxoesneon/martensite#license)

> **Non-linear branching undo/redo tree with Lowest Common Ancestor (LCA) state navigation and bounded depth pruning.**

---

## Overview

`martensite-history` provides a robust, non-linear undo/redo ledger for the Martensite GUI framework. Traditional linear undo stacks discard future history the moment a user undos and performs a new action, causing accidental data loss.

`martensite-history` organizes changes as a **directed history tree**. When edits diverge from a previous point in time, a new branch is created, preserving all alternate historical paths. Navigating between any arbitrary commits in the tree employs a **Lowest Common Ancestor (LCA)** algorithm that deterministically rolls back state to the common ancestor and replays forward down the target branch.

The entire crate is built under `#![forbid(unsafe_code)]`.

---

## Key Features

- **Non-Linear Branching Tree (`HistoryTree`)**: Preserves all historical edits even when new branches diverge after an undo.
- **Minimal Diffs via LCA Algorithm**: Arbitrary time-travel between any two nodes in the tree calculates the minimal path of `revert` and `apply` operations.
- **Reversible Operations (`ChangeOp<S>`)**: Declarative contract requiring forward `apply(&self, state: &mut S)` and inverse `revert(&self, state: &mut S)`.
- **Bounded Depth Memory Pruning**: Automatically prunes the least-recently-used branches when node counts exceed the configured capacity, guaranteeing bounded memory usage.
- **100% Safe Rust**: `#![forbid(unsafe_code)]` enforced.

---

## Quick Start

Add `martensite-history` to your `Cargo.toml`:

```toml
[dependencies]
martensite-history = "0.7.0"
```

Managing non-linear branching state:

```rust
use martensite_history::{ChangeOp, HistoryLedger};

struct AddOp { delta: i32 }
impl ChangeOp<i32> for AddOp {
    fn apply(&self, state: &mut i32) { *state += self.delta; }
    fn revert(&self, state: &mut i32) { *state -= self.delta; }
}

fn main() {
    let mut ledger = HistoryLedger::<i32>::new(0, 100);

    ledger.commit(Box::new(AddOp { delta: 5 }));
    ledger.commit(Box::new(AddOp { delta: 3 }));
    assert_eq!(*ledger.state(), 8);

    ledger.undo();
    assert_eq!(*ledger.state(), 5);

    // Creates a new branch without discarding alternate history
    ledger.commit(Box::new(AddOp { delta: 10 }));
    assert_eq!(*ledger.state(), 15);
}
```

---

## Branching Tree Visualization

```text
       [Commit A] (State: 0)
           |
       [Commit B] (State: 5)
        /        \
   (Branch 1)   (Branch 2)
       |             |
   [Commit C]    [Commit D] (State: 15)
   (State: 8)

LCA Navigation from C -> D:
1. Revert [Commit C] (returns to B)
2. Apply [Commit D] (reaches state 15)
```

---

## Part of Martensite

This crate provides state history and undo/redo services for the [Martensite](https://github.com/jxoesneon/martensite) GUI framework. For top-level widgets and application integration, see [`martensite`](https://crates.io/crates/martensite).

---

## License

Licensed under either of:

- Apache License, Version 2.0 ([LICENSE-APACHE](https://github.com/jxoesneon/martensite/blob/main/LICENSE-APACHE) or <http://www.apache.org/licenses/LICENSE-2.0>)
- MIT license ([LICENSE-MIT](https://github.com/jxoesneon/martensite/blob/main/LICENSE-MIT) or <http://opensource.org/licenses/MIT>)

at your option.
