# Martensite v1.0.0 API Specification

**Document Identifier:** SPEC-0001-API
**Status:** Invariant Core Specification

This document provides complete API specifications for all 21 crates within the Martensite project workspace. It provides precise, objective, and clear definitions with measurable constraints. 

---

## 1. `martensite`
**1. Purpose:** The facade crate. It re-exports the workspace in a unified namespace and provides top-level initialization. Delegates entirely to sub-crates.
**2. API Surface:**
```rust
pub fn init() -> AppBuilder;
pub struct AppBuilder { /* ... */ }
impl AppBuilder {
    pub fn run(self) -> Result<(), MartensiteError>;
}
```
**3. Invariants:** No direct hardware interaction. Exclusively orchestrates the startup cascade.
**4. Error Handling:** `MartensiteError` enum encompassing all fatal startup failures.
**5. Features:** `devtools`, `wayland`, `x11`.
**6. Dependencies:** Depends on all other top-level crates.
**7. Thread Safety:** `AppBuilder` is `!Send + !Sync` (must run on the main thread).
**8. Memory Layout:** N/A.
**9. Phase:** Phase 1.
**10. Limitations v0.x:** Single window only; multi-window support delayed to v1.0.

---

## 2. `martensite-core`
**1. Purpose:** Core manager of the generational widget arena, node topology, and unified scene graph.
**2. API Surface:**
```rust
pub struct WidgetArena { /* ... */ }
impl WidgetArena {
    pub fn new() -> Self;
    pub fn insert(&mut self, hot: HotNode, cold: ColdNode) -> WidgetId;
    pub fn remove(&mut self, id: WidgetId) -> Option<(HotNode, ColdNode)>;
}
#[repr(C, align(64))] pub struct HotNode { /* ... */ }
pub struct ColdNode { /* ... */ }
pub trait Widget: Send + Sync + 'static { /* ... */ }
```
**3. Invariants:** `HotNode` is strictly 64 bytes. Zero holes in `hot_nodes` array (O(1) compaction).
**4. Error Handling:** Operations on invalid `WidgetId` silently return `None` (safe handle invalidation).
**5. Features:** `tracing`.
**6. Dependencies:** `martensite-layout`, `martensite-reactive`.
**7. Thread Safety:** Arena is `Send + Sync`, lock-free reads planned.
**8. Memory Layout:** `HotNode` is 64-byte aligned (1 CPU cache line).
**9. Phase:** Phase 1.
**10. Limitations v0.x:** O(1) compaction limits parallel insertion; resolved by v1.0 chunked allocation.

---

## 3. `martensite-reactive`
**1. Purpose:** Push-pull reactive signal DAG engine.
**2. API Surface:**
```rust
pub struct Signal<T: Clone + 'static> { /* ... */ }
impl<T> Signal<T> {
    pub fn new(initial: T) -> Self;
    pub fn get(&self) -> T;
    pub fn set(&self, val: T);
    pub fn update(&self, f: impl FnOnce(&mut T));
}
pub struct Memo<T> { /* ... */ }
```
**3. Invariants:** Zero memory allocations during update cascades. Mathematically proven glitch-free topological evaluation.
**4. Error Handling:** Panics on cycle detection during debug builds.
**5. Features:** None.
**6. Dependencies:** `parking_lot`.
**7. Thread Safety:** `Signal<T>` is `Send + Sync`.
**8. Memory Layout:** Boxed closures for memos.
**9. Phase:** Phase 1.
**10. Limitations v0.x:** Lock contention on hot signals; v1.0 will introduce thread-local lock-free signals.

---

## 4. `martensite-layout`
**1. Purpose:** Taffy layout engine bridge for W3C flexbox and grid.
**2. API Surface:**
```rust
pub struct LayoutEngine { pub tree: taffy::TaffyTree }
impl LayoutEngine {
    pub fn compute_layout(&mut self, root: taffy::NodeId, space: taffy::Size<taffy::AvailableSpace>);
}
```
**3. Invariants:** Pass 1 (Intrinsic) and Pass 2 (Constraint) strictly separated.
**4. Error Handling:** `LayoutError` for invalid constraints.
**5. Features:** `grid`.
**6. Dependencies:** `taffy`.
**7. Thread Safety:** `LayoutEngine` is `Send + Sync`.
**8. Memory Layout:** Dense flat arrays (internal to Taffy).
**9. Phase:** Phase 2.
**10. Limitations v0.x:** Subgrid not fully supported; v1.0 compliance target.

---

## 5. `martensite-wgpu`
**1. Purpose:** Raw GPU abstraction layer and compute shader dispatch.
**2. API Surface:**
```rust
pub struct GpuContext { /* ... */ }
pub fn initialize_gpu() -> GpuContext;
```
**3. Invariants:** 0.00% GPU utilization when idle.
**4. Error Handling:** `GpuError` (DeviceLost, OutOfMemory).
**5. Features:** `vulkan`, `metal`, `dx12`.
**6. Dependencies:** `wgpu`.
**7. Thread Safety:** `Send + Sync` command encoders.
**8. Memory Layout:** Strictly adheres to WGSL memory alignments (std140/std430).
**9. Phase:** Phase 3.
**10. Limitations v0.x:** Pipeline caching unoptimized.

---

## 6. `martensite-render`
**1. Purpose:** 2D Vector graphics generation via Vello.
**2. API Surface:**
```rust
pub struct SceneBuilder { /* ... */ }
impl SceneBuilder {
    pub fn fill_rect(&mut self, rect: Rect, color: Oklab);
}
```
**3. Invariants:** All draw commands evaluated exactly once per frame.
**4. Error Handling:** Silent failure on invalid geometries (clipped).
**5. Features:** None.
**6. Dependencies:** `martensite-wgpu`, `vello`.
**7. Thread Safety:** Multithreaded command recording.
**8. Memory Layout:** Bump-allocated scene graph for Vello.
**9. Phase:** Phase 3.
**10. Limitations v0.x:** CPU-side stroke expansion bottleneck.

---

## 7. `martensite-text`
**1. Purpose:** Multi-script typography, bidirectional layout, and font shaping.
**2. API Surface:**
```rust
pub struct TextLayout { /* ... */ }
pub fn shape_text(text: &str, font: &Font) -> TextLayout;
```
**3. Invariants:** No tofu glyphs; absolute fallback chain resolution.
**4. Error Handling:** `FontError` (MissingGlyph, LoadError).
**5. Features:** `rustybuzz`.
**6. Dependencies:** `cosmic-text`, `fontdb`.
**7. Thread Safety:** `TextLayout` is `Send + Sync`.
**8. Memory Layout:** Contiguous glyph buffer.
**9. Phase:** Phase 4.
**10. Limitations v0.x:** Color emoji (COLRv1) performance overhead.

---

## 8. `martensite-access`
**1. Purpose:** Real-time semantic tree extraction for native accessibility APIs.
**2. API Surface:**
```rust
pub fn sync_accessibility_tree(arena: &WidgetArena, cx: &mut AccessibilityContext);
```
**3. Invariants:** Incremental AccessKit tree updates matching visual bounds exactly.
**4. Error Handling:** Ignored OS accessibility daemon disconnections.
**5. Features:** `accesskit`.
**6. Dependencies:** `martensite-core`.
**7. Thread Safety:** Single-threaded daemon communication.
**8. Memory Layout:** N/A.
**9. Phase:** Phase 2.
**10. Limitations v0.x:** Custom widget roles lack full platform parity.

---

## 9. `martensite-window`
**1. Purpose:** OS window creation, swapchain management, and raw input event looping.
**2. API Surface:**
```rust
pub struct Window { /* ... */ }
pub fn create_window() -> Window;
```
**3. Invariants:** Sleeps via kernel wait calls (`epoll`/`GetMessageW`) when idle.
**4. Error Handling:** `WindowError`.
**5. Features:** `winit`.
**6. Dependencies:** None.
**7. Thread Safety:** `!Send` (bound to main thread).
**8. Memory Layout:** N/A.
**9. Phase:** Phase 1.
**10. Limitations v0.x:** Transparent window composition unsupported on X11.

---

## 10. `martensite-focus`
**1. Purpose:** Spatial and tabular focus traversal.
**2. API Surface:**
```rust
pub struct FocusManager { /* ... */ }
pub fn advance_focus(arena: &WidgetArena, forward: bool);
```
**3. Invariants:** Cyclic traversal bounds.
**4. Error Handling:** N/A.
**5. Features:** None.
**6. Dependencies:** `martensite-core`.
**7. Thread Safety:** `Send + Sync`.
**8. Memory Layout:** N/A.
**9. Phase:** Phase 4.
**10. Limitations v0.x:** Spatial focus lacks predictive geometry.

---

## 11. `martensite-clipboard`
**1. Purpose:** System clipboard access.
**2. API Surface:**
```rust
pub fn get_string() -> Option<String>;
pub fn set_string(val: &str);
```
**3. Invariants:** Non-blocking asynchronous reads where OS requires.
**4. Error Handling:** `ClipboardError` for unsupported formats.
**5. Features:** `image_clipboard`.
**6. Dependencies:** `arboard`.
**7. Thread Safety:** `Send + Sync`.
**8. Memory Layout:** N/A.
**9. Phase:** Phase 5.
**10. Limitations v0.x:** Custom binary MIME types unimplemented.

---

## 12. `martensite-dnd`
**1. Purpose:** Drag and drop system integration.
**2. API Surface:**
```rust
pub struct DragContext { /* ... */ }
```
**3. Invariants:** Synchronizes visual drag proxy with OS cursor.
**4. Error Handling:** Cancellation yields `DragCanceled`.
**5. Features:** None.
**6. Dependencies:** `martensite-window`.
**7. Thread Safety:** `!Send`.
**8. Memory Layout:** N/A.
**9. Phase:** Phase 6.
**10. Limitations v0.x:** Multi-touch drag interactions untested.

---

## 13. `martensite-theme`
**1. Purpose:** Design tokens and Oklab uniform blending.
**2. API Surface:**
```rust
#[repr(C)] pub struct Oklab { pub l: f32, pub a: f32, pub b: f32, pub alpha: f32 }
impl Oklab { pub fn lerp(self, other: Self, t: f32) -> Self; }
```
**3. Invariants:** 100% perceptual uniformity in all color transitions.
**4. Error Handling:** N/A.
**5. Features:** None.
**6. Dependencies:** `bytemuck`.
**7. Thread Safety:** `Send + Sync`.
**8. Memory Layout:** 16 bytes per color (`Pod`, `Zeroable`).
**9. Phase:** Phase 2.
**10. Limitations v0.x:** P3 color gamut incomplete.

---

## 14. `martensite-motion`
**1. Purpose:** Analytical closed-form spring motion.
**2. API Surface:**
```rust
pub struct SpringConfig { pub mass: f32, pub stiffness: f32, pub damping: f32 }
pub struct SpringSolver { /* ... */ }
impl SpringSolver {
    pub fn new(config: SpringConfig, initial: f32, target: f32, initial_vel: f32) -> Self;
    pub fn sample(&self) -> (f32, f32);
}
```
**3. Invariants:** Settles precisely to 0 velocity; automatically quenches event loop.
**4. Error Handling:** N/A.
**5. Features:** None.
**6. Dependencies:** None.
**7. Thread Safety:** `Send + Sync`.
**8. Memory Layout:** 28 bytes per solver.
**9. Phase:** Phase 3.
**10. Limitations v0.x:** Fluid dynamically damped curves pending.

---

## 15. `martensite-media`
**1. Purpose:** Image and video decoding pipelines.
**2. API Surface:**
```rust
pub struct ImageTexture { /* ... */ }
pub fn load_image(bytes: &[u8]) -> Result<ImageTexture, MediaError>;
```
**3. Invariants:** Zero-copy GPU upload where hardware allows.
**4. Error Handling:** `MediaError` on corrupt headers.
**5. Features:** `jpeg`, `png`, `webp`.
**6. Dependencies:** `martensite-wgpu`.
**7. Thread Safety:** `Send + Sync`.
**8. Memory Layout:** Mapped staging buffers.
**9. Phase:** Phase 5.
**10. Limitations v0.x:** Hardware video decoding (VAAPI) deferred to v1.1.

---

## 16. `martensite-history`
**1. Purpose:** Unified Undo/Redo stack command pattern.
**2. API Surface:**
```rust
pub struct HistoryStack { /* ... */ }
pub fn commit_action(action: Box<dyn Action>);
```
**3. Invariants:** Memory bounded by configurable capacity.
**4. Error Handling:** N/A.
**5. Features:** None.
**6. Dependencies:** `martensite-core`.
**7. Thread Safety:** `Send + Sync`.
**8. Memory Layout:** N/A.
**9. Phase:** Phase 6.
**10. Limitations v0.x:** Distributed OT (Operational Transformation) not supported.

---

## 17. `martensite-assets`
**1. Purpose:** VFS and binary asset bundling.
**2. API Surface:**
```rust
pub struct AssetBundle { /* ... */ }
pub fn get_asset(path: &str) -> Option<&[u8]>;
```
**3. Invariants:** O(1) asset resolution at runtime.
**4. Error Handling:** Missing assets yield `None`.
**5. Features:** None.
**6. Dependencies:** None.
**7. Thread Safety:** `Send + Sync`.
**8. Memory Layout:** `&'static [u8]`.
**9. Phase:** Phase 4.
**10. Limitations v0.x:** Hot-reloading requires external daemon.

---

## 18. `martensite-l10n`
**1. Purpose:** Localization and pluralization rules.
**2. API Surface:**
```rust
pub fn translate(key: &str, locale: &str) -> String;
```
**3. Invariants:** Fluent syntax compliance.
**4. Error Handling:** Fallbacks to default locale.
**5. Features:** None.
**6. Dependencies:** None.
**7. Thread Safety:** `Send + Sync`.
**8. Memory Layout:** N/A.
**9. Phase:** Phase 6.
**10. Limitations v0.x:** Dynamic locale switching requires full tree invalidation.

---

## 19. `martensite-devtools`
**1. Purpose:** Visual widget inspector and performance profiler.
**2. API Surface:**
```rust
pub fn mount_devtools();
```
**3. Invariants:** Compiles to no-op in release builds unless explicitly flagged.
**4. Error Handling:** N/A.
**5. Features:** `profile`.
**6. Dependencies:** All core crates.
**7. Thread Safety:** Main thread only.
**8. Memory Layout:** N/A.
**9. Phase:** Phase 7.
**10. Limitations v0.x:** GPU timeline tracing incomplete.

---

## 20. `martensite-macros`
**1. Purpose:** Ergonomic UI declarative macros.
**2. API Surface:**
```rust
#[macro_export]
macro_rules! widget { /* ... */ }
```
**3. Invariants:** Pure rust syntax output; zero hidden allocations.
**4. Error Handling:** Standard `syn` compiler errors.
**5. Features:** None.
**6. Dependencies:** `proc-macro2`, `syn`, `quote`.
**7. Thread Safety:** N/A (compile time).
**8. Memory Layout:** N/A.
**9. Phase:** Phase 2.
**10. Limitations v0.x:** Error spans occasionally opaque.

---

## 21. `martensite-test`
**1. Purpose:** Headless simulated environment and snapshot testing.
**2. API Surface:**
```rust
pub struct TestHarness { /* ... */ }
impl TestHarness {
    pub fn pump_frames(&mut self, count: usize);
    pub fn snapshot(&self, name: &str);
}
```
**3. Invariants:** Bit-exact pixel rendering across all host OSs for CI.
**4. Error Handling:** `panic!` on snapshot mismatch.
**5. Features:** None.
**6. Dependencies:** All crates.
**7. Thread Safety:** `!Send`.
**8. Memory Layout:** Software rasterized buffers.
**9. Phase:** Phase 7.
**10. Limitations v0.x:** Vello software rasterizer is slow.
