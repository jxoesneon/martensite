# [DDR-0008] Universal Undo/Redo Engine & Branching History DAG Specification

* **Subsystem:** `martensite-history`
* **Status:** Approved
* **Authors:** Martensite Architecture Working Group
* **Related ADRs:** ADR-0002

## 1. System Topology & History Graph Invariants

`martensite-history` delivers an industrial-grade, non-linear undo/redo transaction ledger.
Rather than restricting the user to a lossy linear stack, Martensite models application history as a **Directed Acyclic Graph (DAG)** of immutable state deltas, ensuring that accidental typing after an undo never permanently destroys redo branches.

### 1.1 Structural Topology
```
                  [Root Node: v0]
                         │
                    ┌────┴────┐
                    ▼         ▼
                 [Node v1]  [Node v1b] (Alternative Branch)
                    │
                    ▼
                 [Node v2] <── Active Head
```

* **Invariant 1.1 (Reversibility)**: Every transaction recorded in the history ledger must supply a mathematically exact inverse operation: $\Delta^{-1}(\Delta(S)) = S$.
* **Invariant 1.2 (Branch Preservation)**: Forked edit streams create a new child node on the DAG; old sibling nodes remain addressable.
* **Invariant 1.3 (Gesture Coalescing)**: Continuous 120Hz micro-gestures (such as dragging a slider or typing characters rapidly) coalesce into a single discrete undo transaction based on idle settling windows ($>300\text{ms}$).
