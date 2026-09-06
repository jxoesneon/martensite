# Architecture Review 01: Memory & Arena Architecture

**Status:** SEVERE VULNERABILITIES DETECTED  
**Auditor:** Architecture Review Team  
**Target:** `martensite-core` / `martensite-arena` / DDR-0001 / DDR-0021  

---

## 1. HotNode Cache Line Verification (128-Byte Alignment Hazard)

### **The Attack**
`DDR-0001` mandates that `HotNode` is strictly 64 bytes (`#[repr(C, align(64))]`) to guarantee 100% L1 cache-line alignment and prevent false sharing. However, the specified layout violently fails this invariant.

The struct uses `Option<WidgetId>` for tree pointers. 
`WidgetId` consists of two `u32` fields (`slot_idx` and `generation`). Because a standard `u32` utilizes all 32 bits (no niche), the Rust compiler is forced to add a 1-byte discriminant to `Option<WidgetId>`, plus 3 bytes of padding to maintain 4-byte alignment.
As a result, `Option<WidgetId>` consumes 12 bytes instead of 8.

**Byte Layout Calculation:**
* `layout_id`: 8 bytes
* `bounds`: 16 bytes
* `flags`: 4 bytes
* `layer_depth`: 4 bytes
* 4 × `Option<WidgetId>`: 4 × 12 = 48 bytes
**Total Intrinsic Size = 80 bytes.**

Because `HotNode` is marked with `#[repr(C, align(64))]`, the compiler pads the 80 bytes to the next multiple of 64. **The final compiled size of `HotNode` is exactly 128 bytes.** The architecture is silently wasting 64 bytes (50%) per node in useless padding, halving L1 cache capacity.

### **Architectural Remediation**
Change the `generation` field in `WidgetId` to `std::num::NonZeroU32`. This provides a niche for the compiler, allowing `Option<WidgetId>` to be represented as exactly 8 bytes.
```rust
#[repr(C)]
#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash)]
pub struct WidgetId {
    pub slot_idx: u32,
    pub generation: std::num::NonZeroU32, // Unlocks Option niche optimization
}
```
This forces `HotNode` down to exactly 64 bytes, perfectly fulfilling `Invariant 1.2`.

---

## 2. Dense-Sparse ABA Hazards & Generational Wrap-Around

### **The Attack**
In `martensite-core/src/arena.rs`, when a node is removed, the generation is incremented to invalidate stale handles:
```rust
slot.generation = slot.generation.wrapping_add(1);
```
Under prolonged execution on a workstation (weeks or months), a pathological UI state—such as a loading spinner or an intense data-stream generating and destroying ephemeral nodes in a loop—will easily wrap a 32-bit integer. When it wraps, a stale `WidgetId` held deeply in a side-channel or reactive state will mistakenly validate against the newly incremented generation.

Furthermore, if the remediation for Vulnerability #1 is applied (`NonZeroU32`), a naive `wrapping_add` will eventually hit `0`, which triggers immediate Undefined Behavior or panics in `NonZeroU32`.

### **Architectural Remediation**
The generation increment must explicitly guard against wrapping to 0.
```rust
slot.generation = std::num::NonZeroU32::new(slot.generation.get().wrapping_add(1))
    .unwrap_or(std::num::NonZeroU32::MIN);
```
This ensures a guaranteed safe wraparound, preserving the niche optimization while mitigating standard overflow panics.

---

## 3. The DDR-0021 Preemption Use-After-Free (UAF) Hazard

### **The Attack**
`DDR-0021` introduces a lock-free `FrameFence` to coordinate arena compaction, leveraging an atomic `in_flight_count`.
The main thread waits:
```rust
while self.in_flight_count.load(Ordering::Acquire) > 0 { std::hint::spin_loop(); }
```
The worker thread concludes its snapshot processing:
```rust
self.in_flight_count.fetch_sub(1, Ordering::Release);
// --> OS PREEMPTION OCCURS HERE <--
// PaintList struct drops
```
If a worker thread manually invokes `end_frame()` and is immediately preempted by the OS kernel, the main thread wakes up instantly. The main thread will blindly execute `swap_remove` on the `WidgetArena`, violently mutating the memory locations of `HotNode`.
When the worker thread resumes, it proceeds to execute the `Drop` handler for `PaintList` (which may contain internal `&HotNode` references) or trailing instructions. This results in a catastrophic memory race and Use-After-Free.

### **Architectural Remediation**
`end_frame()` must NEVER be a manually invoked function. The `FrameFence` must utilize an RAII guard (`FrameGuard`). The `PaintList` must physically encapsulate this guard, ensuring the atomic counter is decremented strictly at the bottom of `PaintList::drop()`, mathematically guaranteeing that all references to the arena are fully extinguished *before* the main thread is unblocked.

---

## 4. Idle Allocator Poisoning via MADV_FREE

### **The Attack**
ADR-0008 dictates that the engine will aggressively compact and use OS-level purge hooks (e.g., `madvise(MADV_FREE)` on macOS) to drop RSS during idle states.
When `MADV_FREE` pages are reclaimed by the kernel under memory pressure, subsequent reads to those pages do not segfault—they silently yield **zero-filled pages**.

If the sparse `slots` array relies on a standard `u32` for `generation`, a reclaimed and zeroed page will present a slot with `generation = 0` and `dense_idx = 0`.
If a stale handle has `generation = 0` (or if it wrapped to 0), validating `slot.generation == id.generation` will mysteriously succeed. The UI engine will then erroneously map the handle to `dense_idx = 0` (typically the root window or a critical node), resulting in severe state corruption.

### **Architectural Remediation**
The `NonZeroU32` remediation (from Vulnerability #1) perfectly immunizes the architecture against this attack. Because a zeroed page reads as `generation = 0`, it fundamentally cannot match any valid `WidgetId`, and safely aborts lookup. The specification must explicitly mandate `NonZeroU32` to survive `MADV_FREE` zeroing semantics.
