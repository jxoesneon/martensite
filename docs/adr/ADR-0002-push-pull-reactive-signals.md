# [ADR-0002] Push-Pull Fine-Grained Reactive Signals

* **Status:** Accepted
* **Date:** 2026-09-06
* **Deciders:** Martensite Architecture Working Group
* **Technical Domain:** `martensite-reactive`, `martensite-core`

## Context and Problem Statement

Modern declarative UI programming requires UI components to respond synchronously or reactively to underlying application state mutations ($\text{UI} = f(\text{state})$). Existing GUI paradigms suffer from severe architectural trade-offs:
1. **Virtual DOM Reconciliation (React, Flutter)**: State changes re-invoke entire component subtrees, generating a temporary virtual tree that must be recursively diffed against the previous tree. This wastes significant CPU cycles, churns the heap allocator, and requires complex memoization heuristics (`useMemo`, `@Stable`).
2. **Coarse-Grained Recomposition (Jetpack Compose)**: Re-executes whole functions when parameters mutate, leading to slot-table overhead and unpredictable recomposition cascades.
3. **Coarse-Grained Message Architecture (Iced, Elm)**: Centralizes all updates into a monolithic message enum, requiring global view re-generation and manual component routing.

We require a reactive state engine that updates exact visual leaves in $O(1)$ time without diffing trees, while guaranteeing zero execution glitches and zero allocations during state changes.

## Decision Drivers

* **Zero-Allocation State Mutations**: Updating a signal must never allocate heap memory.
* **Glitch-Free Evaluation**: Derived computations (`Memo<T>`) must never compute transient, stale, or contradictory values during intermediate dependency updates (solving the diamond dependency problem).
* **Direct Leaf Dirty-Bit Propagation**: Mutating state must directly flag only the specific layout or paint nodes that depend on that state.

## Considered Options

* **Option 1**: Virtual DOM Tree Diffing.
* **Option 2**: Pure Push-Based Reactive Streams (Rx, Callbacks).
* **Option 3**: **Topological Push-Pull Reactive Signal Graph (Signals & Memos)**.

## Decision Outcome

Chosen option: **Option 3**, because it combines minimal push notifications (marking dirty bitsets) with lazy, topological pull evaluations (computing values only when requested for layout or rendering).

### Positive Consequences

* **Declarative Components Run Once**: Component functions execute exactly once to forge the layout tree in the arena. They never re-execute to diff state.
* **Glitch-Free Guarantees**: A directed acyclic graph (DAG) topological sort ensures that derived values evaluate in strict dependency order, mathematically preventing duplicate computations and transient state glitches.
* **Direct Leaf Dirtying**: Mutating a `Signal<T>` marks downstream nodes dirty in an $O(1)$ bitset, triggering targeted layout or paint passes without whole-tree reconciliation.
