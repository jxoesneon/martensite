# BLUE TEAM AUDIT 02: REACTIVE DAG ARMOR & TRANSACTIONAL RESILIENCE

**Author:** Blue Team Specialist 2 (Reactive DAG Armor & Transactional Resilience Specialist)  
**Target:** `martensite-reactive` architecture, `REDTEAM_02_REACTIVE_DAG.md`

## EXECUTIVE SUMMARY
This document outlines the definitive defensive architecture and algorithmic guarantees resolving the vulnerabilities exposed by the Red Team. It establishes the mathematical soundness, structural integrity, and multi-threaded transactional resilience of Martensite's push-pull reactive engine, adhering strictly to The Ten Golden Laws (specifically Law V: The Zero-VDOM Signal Law).

---

## 1. Epoch-Based Dynamic Edge Pruning Algorithm

### Data Structures & Strict Types
To resolve the rank inversion and memory leak hazards caused by dynamic conditional dependencies (`if toggle.get() { a.get() } else { b.get() }`), Martensite introduces an Epoch-Based Garbage Collection seamlessly integrated into the DAG traversal state machine.

```rust
#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash)]
pub struct SignalId(pub u64);

#[derive(Clone, Debug)]
pub struct DependencyLink {
    pub target: SignalId,
    pub last_used_epoch: u32,
}

pub struct ReactiveNode {
    pub id: SignalId,
    pub eval_epoch: u32,
    pub dependencies: Vec<DependencyLink>,
    pub rank: u32,
    // ...
}
```

### The Two-Phase Cleanup Protocol
1. **Evaluation Phase:** Before a derived node (`Memo<T>`) evaluates its closure, its internal `eval_epoch` counter is strictly incremented.
2. **Access Phase:** During evaluation, whenever a dependency is queried (e.g., via `signal.get()`), the thread-local `ReactiveContext` intercepts the read. It locates the corresponding `DependencyLink` in the node's list and updates `last_used_epoch = current_epoch`. If the edge is new, it is inserted initializing `last_used_epoch` to the `current_epoch`.
3. **Pruning Phase (Post-Eval):** After the closure successfully returns, the engine iterates over the node's `dependencies`. Any `DependencyLink` where `last_used_epoch < current_epoch` is aggressively pruned (and the node is deregistered from the stale dependency's subscriber list).

### Formal Proof of Soundness
**Eradication of Leaks:** Let $e$ be the current epoch for node $N$. A dependency $D$ is in the conditionally executed branch if and only if $D$ is read during evaluation. If $D$ is not read, its `last_used_epoch` remains $e_0 < e$. In the post-eval phase, $D$ is strictly pruned. Thus, abandoned edges are categorically destroyed, preventing $O(N)$ graph bloat.
**Phantom Wakeup Prevention:** Because stale edges are severed immediately upon completing the evaluation of the new branch, when the old dependency $D$ subsequently mutates, it will no longer traverse to $N$. Thus, no phantom dirty propagation or rank inversion artifacts can occur.

---

## 2. Glitch-Free Diamond Batch Propagation Guarantee

### Transactional Protocol
To prevent intermediate stale evaluations in diamond dependency structures during batch updates, we formalize a strict Phase Isolation mechanism enforcing a two-phase transactional state machine.

- **Phase 1 (Push - Accumulate & Mark):** 
  When a batch update initiates, the scheduler enters the `Push` phase. Mutations flag source signals as dirty. A Breadth-First Search (BFS) traverses the DAG strictly updating dirty bitsets and enqueuing nodes into a `pending_eval_queue`, prioritizing by their topological depth rank. **Crucially, no closures are executed and no values are evaluated during Phase 1.** All intermediate evaluations are completely deferred.

- **Phase 2 (Pull - Topological Resolution):**
  Only after the batch closure returns (draining Phase 1 completely), the scheduler transitions to the `Pull` phase. The `pending_eval_queue` processes nodes strictly via a priority queue sorted in **ascending** topological rank ($\lambda$).

### Formal Proof of Glitch-Freedom
Given a diamond dependency: $A \to B, A \to C, B \to D, C \to D$.
Let $\lambda(A)=0, \lambda(B)=1, \lambda(C)=1, \lambda(D)=2$.
1. During Phase 1, mutating $A$ flags $B, C, D$ as dirty and queues them.
2. During Phase 2, nodes are popped by ascending rank.
3. Ranks $1$ ($B$ and $C$) evaluate first, reliably reading $A$'s committed value.
4. Rank $2$ ($D$) evaluates next. By the time $D$ evaluates, all nodes with $\lambda < 2$ ($B$ and $C$) have fully settled.
Thus, $D$ will never read an intermediate state where $B$ is updated but $C$ is not, guaranteeing mathematically robust glitch-freedom.

---

## 3. Release-Mode Cycle Circuit Breaker

### Bounded Recursion Contract
To uphold Charter Mandate III (0.00% Idle Resource Consumption) and prevent infinite `flush` loops triggered by reactive cycles (e.g., $A \to B \to A$), we define a strict constant recursion limit:

```rust
const MAX_DAG_DEPTH: u32 = 1024;
```

### Graceful Isolation Behavior
During Phase 2 (Pull), the scheduler tracks a counter for evaluations per node within a single global epoch.
If `evaluations_this_epoch > MAX_DAG_DEPTH` for any node:
1. The scheduler immediately **halts traversal** along that specific cyclical edge.
2. The offending node is transitioned to a `POISONED` typestate, forever preventing its evaluation.
3. A structured tracing event is emitted without bringing down the runtime: 
   `tracing::error!(target: "martensite::dag", node_id = ?id, "Reactive cycle detected. Node poisoned to preserve event loop integrity.")`
4. The event loop resumes. In debug mode, this trips a `panic!`, but in release mode, the GUI thread remains responsive, lockups are averted, and idle CPU utilization drops perfectly back to 0.00%.

---

## 4. Lock-Free MVCC / Thread-Local Journaling

### Atomic Multi-Version Concurrency
To categorically abolish the self-deadlock and AB-BA lock convoy vectors associated with `parking_lot::RwLock`, we implement a Lock-Free Multi-Version Concurrency Control (MVCC) model paired with Thread-Local Journaling.

```rust
use std::sync::atomic::{AtomicPtr, Ordering};
use std::ptr;

pub struct SignalValue<T> {
    pub value: T,
    pub version: u64,
}

pub struct Signal<T: Clone + 'static> {
    pub id: SignalId,
    // Atomic pointer to a generationally-allocated version snapshot
    state: AtomicPtr<SignalValue<T>>,
}
```

### Lock-Free Execution & Snapshots
- **Reads (`get`):** Worker threads perform a wait-free `Acquire` load on the `AtomicPtr`. Because the pointer references an immutable heap/arena snapshot, any number of threads can read concurrently without any lock convoying, desynchronization, or thread blocking.
- **Writes (`set` / `update`):** Mutations allocate to a thread-local transaction journal, never mutating the global state in place:
  ```rust
  let new_state = Box::new(SignalValue { value: new_val, version: current_version + 1 });
  ```
- **Commit Phase:** During the global `flush()`, mutations are atomically committed using a Compare-And-Swap (CAS) loop on the `AtomicPtr`. 
- **Zero-Contention Rollback:** If a transaction must be aborted (e.g. constraint solver fault), the thread-local journal is simply discarded without requiring complex locks or rollback mechanisms. This guarantees pure transactional properties and zero lock contention overhead.
