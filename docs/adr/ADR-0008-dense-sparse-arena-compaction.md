# [ADR-0008] Dense-Sparse Arena Compaction & Idle Allocator Purging

* **Status:** Accepted
* **Date:** 2026-09-06
* **Deciders:** Master (Sovereign Architect), Ciel (Systems & Quality Guilds)
* **Technical Domain:** `martensite-arena`, `martensite-core`

## Context and Problem Statement

Mission-critical workstation software (telemetry monitors, industrial control panels, algorithmic execution desks) remains active for weeks or months without restarts. Under continuous runtime operation, standard GUI memory allocation exhibits three fatal failure modes:
1. **Monotonic Capacity Ratcheting**: Temporary UI bursts (e.g., loading a 50,000-row dataset or expanding a deep log hierarchy) force underlying `Vec` buffers to reallocate at double capacity. When the view is collapsed or cleared, the heap memory remains pinned at the peak water mark indefinitely.
2. **Cache-Line Inefficiency from Fragmented Holes**: Freeing nodes leaves gaps across memory buffers. Traversing a sparse node array during 120 FPS layout and draw-encoding passes causes frequent L1/L2 CPU cache misses, degrading frame dispatch times.
3. **Allocator RSS Hoarding**: Memory allocators (`jemalloc`, `mimalloc`) retain committed virtual pages in thread-local caches and arena bins instead of returning physical frames to the operating system kernel via `madvise(MADV_DONTNEED)`, causing system monitors to report unbounded memory growth.

We must establish a memory architecture that guarantees cache-dense traversals, bounded peak capacity, and proactive memory return to the OS.

## Decision Drivers

* O(1) constant-time node lookup by handle with zero reference counting.
* 100% packed, cache-contiguous memory layout during traversal passes.
* Guaranteed memory reclamation during idle quiescent periods.
* Elimination of unmounted signal subscriber memory leaks.

## Considered Options

* **Option 1**: Standard standard-library `Rc<RefCell<Node>>` tree hierarchy.
* **Option 2**: Flat `SlotMap<WidgetId, Node>` with generational sparse tombstones.
* **Option 3**: **Two-Tier Dense-Sparse SlotMap + 64-Byte Hot/Cold Cache-Line Splitting + Idle Allocator Purging**.

## Decision Outcome

Chosen option: **Option 3**, because it combines the safety of generational index handles with the hardware efficiency of packed sequential arrays and automated physical page return.

### Positive Consequences

* **Cache Contiguity**: The `dense` array has **zero holes**. Traversing nodes during layout or rendering streams through linear memory, maximizing CPU pre-fetching and hardware cache efficiency.
* **Bounded RSS**: During idle windows (>5s quiescent inactivity), Martensite compacts its dense capacity via `shrink_to_fit` and invokes allocator-level purge hooks (`arenas.purge` / `mi_collect`), reducing Resident Set Size back to baseline (~15MB).
* **Hardware Alignment**: Critical layout fields are packed into a 64-byte `HotNode` struct aligning perfectly with standard x86/ARM CPU cache lines.

### Negative Consequences

* **Indirection Overhead on Lookup**: Accessing a widget by handle requires two memory dereferences (`slots[handle.slot_idx].dense_idx` -> `dense[dense_idx]`).
