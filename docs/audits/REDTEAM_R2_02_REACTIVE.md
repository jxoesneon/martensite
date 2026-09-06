# RED TEAM AUDIT R2 02: REACTIVE DAG ARMOR & TRANSACTIONAL RESILIENCE

**Author:** Red Team Saboteur 2  
**Target:** `BLUETEAM_02_REACTIVE_DAG.md`

## 1. Async Suspension Across Epochs (The Await-Amnesia Hazard)

**Vulnerability:**
The epoch-based dynamic edge pruning algorithm uses a thread-local `ReactiveContext` to intercept reads during the "Access Phase." However, this design fundamentally breaks down for asynchronous derived computations (e.g., `async { ... }`). 

**Attack Scenario:**
Consider a derived async node that fetches data and depends on multiple signals:
```rust
let derived = Memo::new_async(async {
    let a = sig_a.get(); // Intercepted by thread-local context.
    fetch_data().await;  // Suspend execution!
    let b = sig_b.get(); // Resumes on a DIFFERENT worker thread!
});
```
1. The `eval_epoch` increments. `sig_a.get()` records `last_used_epoch` correctly.
2. The future hits `.await` and yields. The closure technically hasn't "finished," but the executor takes over.
3. If the Post-Eval Pruning Phase runs eagerly upon `Poll::Pending`, it will immediately prune `sig_b` because it hasn't been awaited/read yet. 
4. Alternatively, if pruning waits for `Poll::Ready`, the future may resume on a completely different executor thread. The standard `thread_local!` context is lost. When `sig_b.get()` executes, it either panics (no context) or silently fails to register the dependency. 
5. When the future finally completes, the post-eval phase runs, sees `sig_b`'s `last_used_epoch` is outdated, and severs the edge. 
**Result:** Silent reactivity loss. `sig_b` updates will never trigger the memo again.

**Remediation:** 
Standard `thread_local!` is insufficient. You must use asynchronous task-locals (e.g., `tokio::task_local!`) and ensure the engine's lifecycle management handles `Poll::Pending` without triggering the Post-Eval Pruning Phase prematurely.

---

## 2. Bounded Recursion Poisoning vs. Valid Data Topologies

**Vulnerability:**
The Blue Team introduces a circuit breaker `MAX_DAG_DEPTH = 1024` to prevent infinite loops, poisoning nodes that exceed this limit. However, mapping recursion/iteration depth limits linearly onto a DAG traversal is a catastrophic ergonomic trap for data-heavy applications.

**Attack Scenario:**
In applications like spreadsheet engines, audio DSP pipelines, or complex node graphs, user-generated reactive chains routinely exceed a depth of 1024 purely linear dependencies ($A \to B \to C \to \dots \to N_{2000}$).
1. A spreadsheet imports a column where each row depends on the previous row (e.g., running total). The DAG depth is 2,000.
2. The user modifies the first row. 
3. The topological resolution (Phase 2) begins processing the cascading updates.
4. If the traversal tracks depth or recursion and trips `current_depth > MAX_DAG_DEPTH`, it halts the traversal. 
5. Valid cells $N_{1025}$ through $N_{2000}$ are permanently marked as `POISONED`.
**Result:** The application silently stops updating half the spreadsheet, permanently corrupting the reactive graph without a single cyclic bug existing.

**Remediation:** 
Cycles should be detected via proper topological sort validation (e.g., Kahn's algorithm cycle detection or Tarjan's strongly connected components), not arbitrary depth limits. If a node legitimately evaluates *more than once* in a batch, the topological sort is already flawed.

---

## 3. MVCC Memory Reclamation (The Safe-Rust Concurrency Paradox)

**Vulnerability:**
The Blue Team proposes `AtomicPtr<SignalValue<T>>` for lock-free MVCC, allocating new versions via `Box::new` and committing via CAS. This completely ignores the memory reclamation problem inherent to wait-free algorithms.

**Attack Scenario:**
```rust
// A mutation occurs at 120Hz:
let new_state = Box::new(SignalValue { value: new_val, version: v + 1 });
let old_ptr = state.swap(Box::into_raw(new_state), Ordering::Release);
```
Once the CAS succeeds, what happens to `old_ptr`?
- **Option A (Leak):** If the old pointer is simply forgotten, the application leaks memory on every single signal update. At 120Hz, a GUI application will quickly exhaust available RAM (OOM crash).
- **Option B (Use-After-Free):** If the writer re-boxes and drops it (`drop(Box::from_raw(old_ptr))`), concurrent readers that acquired the pointer microseconds earlier via wait-free `Acquire` load will read freed memory. This is immediate Undefined Behavior (UB), leading to segfaults.
- **Option C (Safe Rust):** `AtomicPtr` fundamentally requires `unsafe` to dereference or manage lifetimes. Claiming this is a "100% pure-safe Rust" solution without an epoch-based GC (like `crossbeam-epoch`) is a paradox.

**Result:** You cannot achieve lock-free GC-less wait-free reads in pure safe Rust using raw `AtomicPtr`. You either leak memory fatally or introduce UB. 

**Remediation:** 
You must either integrate a robust epoch-based reclamation system (like `crossbeam-epoch`, accepting its internal `unsafe` blocks), switch to lock-free hazard pointers, or compromise on wait-free reads by using `Arc` combined with a spinlock or standard `RwLock`.
