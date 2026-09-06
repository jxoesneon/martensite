# Architecture Review 02: Reactive DAG & Concurrency
**Auditor:** Architecture Review Team  
**Target:** `martensite-reactive` architecture, `DDR-0002-martensite-reactive-scheduler.md`, `ADR-0002`, and `lib.rs`.

## EXECUTIVE SUMMARY
The current specification for the push-pull reactive DAG scheduler is critically flawed. The mathematical proofs in DDR-0002 assume a static directed acyclic graph (DAG) which contradicts the reality of modern UI programming (dynamic conditional dependencies). Furthermore, the `RwLock`-based concurrency model in `lib.rs` is a textbook breeding ground for deadlocks. If implemented as specified, Martensite will suffer from infinite loops, memory leaks of stale subgraphs, transient state glitches, and thread starvation.

---

## 1. Dynamic Dependency Graph Mutations (The Rank Inversion & Memory Leak Hazard)

### The Vulnerability
DDR-0002 defines topological rank $\lambda$ statically. However, reactive UIs routinely branch:
```rust
let memo = Memo::new(|| if toggle.get() { a.get() } else { b.get() });
```
When `toggle` shifts from `true` to `false`, `memo` drops dependency `a` and adopts dependency `b`.
1. **Rank Inversion:** If $\lambda(b) > \lambda(a)$, the `memo` was queued at its old, lower rank. When the scheduler pops it and evaluates it, it reads `b`. But if `b` is *also* dirty and queued at a higher rank, `memo` evaluates before `b` has been refreshed! This causes `memo` to observe a transient, stale state, violating the Glitch-Freedom invariant (DDR-0002 Invariant 1.2).
2. **Stale Edge Leak:** The `mark_dirty` algorithm unconditionally traverses `self.nodes[curr].subscribers`. There is zero mechanism described to detach $a \to memo$ after the branch shifts. The graph leaks abandoned edges, causing memory bloat and forcing the engine to wake up and process detached subgraphs ($O(N)$ tax on $O(1)$ operations).

### Architectural Remediation
* **Dynamic Rank Adjustments:** When a node pulls a dependency during evaluation, it must assert that $\lambda(\text{dependency}) < \lambda(\text{self})$. If the dependency has a higher rank, the node must immediately abort its execution, adopt the new rank $\lambda(\text{dependency}) + 1$, and re-insert itself into the `pending_eval_queue`.
* **Epoch-Based Edge Pruning:** During evaluation, track all accessed dependencies in a thread-local context. In the push phase, intersect the old dependencies with the new dependencies and aggressively sever edges that were not accessed.

---

## 2. Diamond Dependency Glitch Elimination (The Batching Inconsistency)

### The Vulnerability
In a diamond dependency scenario ($A \to B, A \to C, B \to D, C \to D$), a synchronous batch update exposes a critical sequencing flaw:
```rust
cx.batch(|| {
    A.set(1);
    Z.set(2); // Z is some other root that D dynamically depends on
});
```
If a developer triggers a batch update, multiple independent roots are mutated before `flush()` is called. In the current DDR-0002 design, if ranks shift dynamically or if batch evaluations interleave Push and Pull phases, a derived node could be pulled from the queue while one of its paths remains dirty. 
The theorem in DDR-0002 holds *only* if the graph is static and all dirty flags are comprehensively pushed before *any* evaluation begins.

### Architectural Remediation
* **Strict Phase Isolation:** The scheduler must enforce a transactional state machine. `Phase 1 (Push)` must comprehensively drain before `Phase 2 (Pull)` begins. `batch()` must lock the scheduler into an accumulating state, deferring all `pending_eval_queue` processing until the closure drops.

---

## 3. Cycle Detection & Release-Mode Soundness (The Infinite `flush` Loop)

### The Vulnerability
Core requirements mandate zero idle CPU utilization when quiescent. DDR-0002 lacks cycle detection. 
Consider an accidental graph cycle: $A \to B \to A$.
During Phase 1 (`mark_dirty`), the algorithm terminates because `if !self.nodes[sub].is_dirty` prevents infinite queuing.
However, in Phase 2 (`flush`):
```rust
if node.is_dirty {
    node.evaluate();
    node.is_dirty = false; 
}
```
If `node.evaluate()` (evaluating $B$) invokes `A.set()`, it triggers a new `mark_dirty(A)`. Because $A$'s `is_dirty` flag was cleared after its own evaluation, it gets re-queued. The `while let Some(...) = pop_first()` loop will spin indefinitely. In release builds, this will completely hang the GUI thread, consume 100% CPU, and lock up the event loop without any panics.

### Architectural Remediation
* **Bounded Transaction Epochs (TTL):** Every `flush()` cycle is assigned a monotonically increasing Epoch ID. Nodes track a `evaluations_this_epoch` counter. If a node evaluates more than a conservative limit (e.g., $K=128$) within a single epoch, the scheduler must violently abort the transaction, panic in debug, and in release mode *sever the incoming edge*, logging a critical error but preserving the responsiveness of the event loop.

---

## 4. Transactional Re-Entrancy & Lock Contention (The AB-BA Deadlock)

### The Vulnerability
The draft implementation in `crates/martensite-reactive/src/lib.rs` relies on atomic `RwLock` wrapping:
```rust
pub fn update(&self, f: impl FnOnce(&mut T)) {
    f(&mut *self.value.write());
}
```
1. **Self-Deadlock:** A simple `signal.update(|val| { signal.get(); })` will immediately deadlock the thread.
2. **Classic AB-BA Deadlock:** If Thread 1 executes `A.update(|_| B.get())` and Thread 2 concurrently executes `B.update(|_| A.get())`, the system suffers a terminal lock convoy.
3. **No Rollback:** If a layout constraint solver or batch transaction faults, there is no way to roll back the `RwLock` mutation because it modifies the value in place without an undo log.

### Architectural Remediation
* **Abolish Granular `RwLock`:** Individual signals must not own their own locks. Instead, move to a **Multi-Version Concurrency Control (MVCC)** or **Thread-Local Journaling** model. 
* Mutations should write to a thread-local transaction staging area. When `flush()` occurs, the transaction is atomically committed to the global topological arena. If a failure occurs, the thread-local journal is simply discarded, enabling pure transactional rollbacks with zero lock contention.
