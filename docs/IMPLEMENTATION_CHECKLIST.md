# Martensite Implementation Checklist

## Crate: `martensite-core` (WG-Core)
- [ ] Implement `WidgetArena` with dense/sparse generation indices.
- [ ] Implement arena compaction algorithm (idle defrag + `madvise`).
- [ ] Define `WidgetId { slot_idx: u32, generation: u32 }`.
- [ ] Implement `HotNode` and `ColdNode` structures.
- [ ] Define `Widget` trait and baseline widget contexts (`LayoutContext`, `PaintContext`, `EventContext`, `AccessibilityContext`).
- [ ] Ensure zero heap allocation in hot paths.

## Crate: `martensite-reactive` (WG-Core)
- [ ] Implement `Signal<T>` with push-pull topological evaluation.
- [ ] Implement `Memo<T>` with invalidation tracking.
- [ ] Implement dirty bitset for scheduler.
- [ ] Write 10k-node DAG signal propagation latency Criterion benchmarks.

## Crate: `martensite-wgpu` (WG-Graphics)
- [ ] Implement `GpuRenderer` and `wgpu::Device` initialization.
- [ ] Build swapchain surface acquisition.
- [ ] Implement device loss resurrection pipeline (`SurfaceError::Lost` handling).
- [ ] Expose shader extensions and 3D interop APIs.

## Crate: `martensite-render` (WG-Graphics)
- [ ] Define `PaintList` data structure.
- [ ] Build Vello `Scene` translation layer.
- [ ] Implement CPU fallback pipeline via `TinySkiaBackend`.

## Crate: `martensite-window` (WG-Platform)
- [ ] Implement `WindowManager` mapping to Winit event loop.
- [ ] Handle per-monitor DPI scaling dynamically.
- [ ] Intercept GPU device loss at window close/sleep cycles.
- [ ] Enable multi-window shared arena management.

## Crate: `martensite-text` (WG-Human)
- [ ] Integrate `cosmic-text` pipeline.
- [ ] Configure `FontSystem` and HarfBuzz shaping.
- [ ] Expose IME candidate bounds projection for Winit IME API.

## Crate: `martensite-layout` (WG-Human)
- [ ] Implement `LayoutEngine`.
- [ ] Build `TraversePartialTree` bridge from Taffy to `WidgetArena`.
- [ ] Enforce zero 1-frame lag layout calculation invariant.

## Crate: `martensite-access` (WG-Human)
- [ ] Build AccessKit incremental `TreeUpdate` adapter.
- [ ] Integrate accessibility properties into widget definitions.

## Crate: `martensite-focus` (WG-Human)
- [ ] Implement `FocusManager`.
- [ ] Write 2D projected-beam spatial focus algorithm.
- [ ] Build modal `FocusScope` stack with auto-restore logic.

## Crate: `martensite-clipboard` (WG-Platform)
- [ ] Define `ClipboardItem`.
- [ ] Implement Multi-MIME OLE/Cocoa/Wayland endpoints with lazy evaluation.

## Crate: `martensite-dnd` (WG-Platform)
- [ ] Build unified internal cross-OS DnD pipeline.
- [ ] Manage arena reparenting boundaries on drop operations.

## Crate: `martensite-motion` (WG-Human)
- [ ] Define `SpringConfig` and analytical continuous spring equations.
- [ ] Build `SpringSolver` for critical/under/overdamped interpolations.
- [ ] Enforce C1 continuity on gesture interruption.

## Crate: `martensite-theme` (WG-Graphics)
- [ ] Implement `Oklab` struct for linear color blending.
- [ ] Write GPU uniform bindings for theme state transitions.
- [ ] Implement dark/light mode context.

## Crate: `martensite-history` (WG-Core)
- [ ] Implement transactional undo/redo ledger.
- [ ] Build LCA (Lowest Common Ancestor) tree for state rollbacks.

## Crate: `martensite-assets` (WG-Platform)
- [ ] Implement dual-mode VFS (development/release).
- [ ] Write AOT shader loading utilities.

## Crate: `martensite-l10n` (WG-Human)
- [ ] Integrate Project Fluent string maps.

## Crate: `martensite-media` (WG-Graphics)
- [ ] Implement zero-copy hardware surface passthrough (DXGI/IOSurface/dma-buf).

## Crate: `martensite-devtools` (WG-DX)
- [ ] Integrate Tracy canonical tracing spans.
- [ ] Build F12 in-app HUD (histogram, dirty-rect overlay).

## Crate: `cargo-martensite` (WG-DX)
- [ ] Create `cargo martensite dev` CLI.
- [ ] Implement hot-reload host/guest cdylib splitting.
- [ ] Build state preservation injection across reloads.

## Crate: `martensite-macros` (WG-DX)
- [ ] Write `widget!` declarative construction macro.

## Crate: `martensite-test` (WG-DX)
- [ ] Implement `VirtualClock` for fixed-time tests.
- [ ] Build YIQ/SSIM perceptual diff engine for headless frame capture.

## Crate: `martensite-blessed` (TBD - Needs Creation)
- [ ] Scaffold new crate.
- [ ] Curate higher-level composed widgets for ecosystem reuse.
