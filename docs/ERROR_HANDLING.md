# Error Handling Strategy

**Document Identifier:** DOC-0003-ERROR
**Status:** Maintained
**Target:** v1.0.0

## 1. The Core Policy

Martensite enforces a strict, brutalist error-handling policy aligned with its systems engineering mandate. 
* **No `anyhow` in Libraries:** The use of `anyhow::Error` or type-erased `Box<dyn Error>` is strictly forbidden in all `martensite-*` crates.
* **`thiserror` Exclusivity:** All crates must define explicit, per-crate error enums using `thiserror`.

## 2. Per-Crate Error Enums

Every crate defines its own exhaustive error domain. Example structure:
```rust
#[derive(Debug, thiserror::Error)]
pub enum LayoutError {
    #[error("Infinite layout recursion detected at node {0:?}")]
    InfiniteRecursion(WidgetId),
    #[error("NaN layout coordinate generated at node {0:?}")]
    NanCoordinate(WidgetId),
}
```

## 3. Panic vs. Result

* **`Result<T, E>`:** Used for all runtime failures, OS boundaries, parsing errors, GPU pipeline creations, and IO operations.
* **Panic:** Permitted **only** when an unrecoverable invariant is breached:
  - Memory corruption or out-of-bounds arena access on a validated `WidgetId`.
  - Mutex poisoning within the internal rendering scheduler.
  - Usage of `todo!()` or `unimplemented!()` in production code (forbidden by quality standards; CI will fail).

## 4. Subsystem Recovery Mechanics

### 4.1 GPU Error Recovery
* **Device Loss:** If `wgpu::Error::DeviceLost` is encountered (e.g., driver crash, OS sleep), the `martensite-wgpu` reactor automatically reconstructs the swapchain and compute pipelines. The user application does not panic.
* **OOM (Out of Memory):** If texture allocation fails due to VRAM exhaustion, Martensite evicts non-critical cached glyphs and off-screen buffers, then retries. Fatal OOM bubbles up as a `RenderError::OutOfMemory`.

### 4.2 Layout Error Recovery
* **NaN Coordinates:** Taffy constraint outputs are sanitized. If a calculation yields `NaN`, it is clamped to `0.0`, a `tracing::error!` is emitted, and rendering continues to prevent a hard crash.
* **Infinite Recursion:** Detected via a maximum depth counter during the 2-pass traversal. Defaults to truncating the sub-tree and emitting an error.

### 4.3 Signal Error Recovery
* **Cycle Detection:** The `martensite-reactive` DAG enforces topological sorting. If a cycle is detected during a state mutation, the propagation aborts, returning a `ReactiveError::CycleDetected`.

## 5. User-Facing Error Messages

Error messages must be actionable, brutalist, and precise.
* **Bad:** *"Something went wrong loading the image."*
* **Good:** *"Failed to decode PNG header at path '/foo/bar.png': Invalid checksum."*
Error messages must never hallucinate solutions unless definitively known.
