# [ADR-0001] Generational SlotMap Arena Architecture

* **Status:** Accepted
* **Date:** 2026-09-06
* **Deciders:** Martensite Architecture Working Group
* **Technical Domain:** `martensite-core`, `martensite-arena`

## Context and Problem Statement

Graphical user interface architectures fundamentally model hierarchical and spatial graphs (trees with parent, child, and sibling pointers, alongside spatial focus graphs). In idiomatic safe Rust, cyclic, self-referential graph structures are famously difficult to express due to strict single-ownership and borrowing rules.

Historical GUI toolkits in the Rust ecosystem have attempted three failed patterns:
1. **The Shared Pointer Swamp (`Rc<RefCell<Node>>`)**: Used in early toolkits and GTK-rs wrappers. This introduces heavy reference-counting overhead, cache-locality destruction via scattered heap allocations, and frequent runtime panic risks (`BorrowMutError`) during event propagation cascades.
2. **Immediate-mode state re-evaluation**: Reconstructing UI representations each frame eliminates reference cycles, but can increase CPU overhead during continuous updates and requires synthetic bridge layers to maintain persistent accessibility hierarchies.
3. **The Global Index Tree with Sparse Fragmentation**: Flat arrays with unchecked generational reuse leading to ABA use-after-free bugs and sparse arrays that destroy CPU cache-line prefetching.

We must decide on a unified, high-performance, strictly safe memory architecture for storing, referencing, and traversing widget hierarchies in Martensite.

## Decision Drivers

* **Zero Undefined Behavior & Zero Runtime Borrow Panics**: Eliminate `Rc<RefCell<T>>` and unchecked raw pointers across the public API.
* **O(1) Spatial Lookups & Validations**: Instantaneous node retrieval by handle with guaranteed detection of deleted nodes.
* **L1/L2 Cache Friendliness**: Maximize cache-line density during recursive layout and paint traversal passes.
* **Bounded RSS & Compact Memory Footprint**: Prevent memory leaks from dangling handles.

## Considered Options

* **Option 1**: `Rc<RefCell<WidgetNode>>` Pointer Graph.
* **Option 2**: Flat `Vec<Option<WidgetNode>>` with integer IDs and free lists.
* **Option 3**: **Dense-Sparse Generational SlotMap Arena with 64-byte Hot/Cold Node Partitioning**.

## Decision Outcome

Chosen option: **Option 3**, because it solves cyclic reference safety without heap allocation overhead, guarantees O(1) handle validation, and aligns data structures directly with CPU cache line boundaries.

### Positive Consequences

* **Unboxed Copyable Handles**: References across the widget tree and reactive signal graphs are lightweight 64-bit copyable tokens: `WidgetId { slot_idx: u32, generation: u32 }`.
* **Zero Runtime Borrow Panics**: Mutating or querying a widget requires a reference to the centralized `WidgetArena`, enforcing Rust's compile-time borrowing rules cleanly at the system boundary.
* **Elimination of ABA Bugs**: Reused slots increment a 32-bit generation counter. Stale handles from deleted components evaluate safely to `None` in O(1) time.
* **Cache-Dense Sequential Traversals**: Active nodes are packed contiguously into a dense array. Tree traversals stream through linear memory without cache misses.

### Negative Consequences

* All widget lookups require passing a context reference (`cx` or `&WidgetArena`).
* Deleting a widget requires an O(1) swap-remove operation that updates the sparse redirection slot of the relocated item.
