# Martensite v1.0.0 Implementation Roadmap

## v0.1.0 — The Hardened Foundation
**Entry Criteria:** Phase 0 (Workspace scaffold) complete.
**Deliverables:**
- `martensite-core`: Implement `WidgetArena`, `HotNode`, `ColdNode`, `WidgetId`, and arena compaction algorithm (dense/sparse).
- `martensite-reactive`: Implement topological signal scheduler, `Signal<T>`, `Memo<T>`, `SignalId`, and dirty bitset tracker.
**Exit Criteria:** Criterion benchmarks pass 10k-node DAG signal propagation latency under target limits. No raw pointers.
**Estimated LOC additions:**
- `martensite-core`: ~1500 LOC
- `martensite-reactive`: ~1200 LOC
**Key Risks:** Memory leaks in the arena generational indices; exponential evaluation paths in signal DAG.
**Mitigations:** Comprehensive fuzz testing on arena reallocations; glitch-free topological evaluation enforcement.

## v0.2.0 — The Rendering Pipeline
**Entry Criteria:** v0.1.0 complete. Stable core and reactivity primitives.
**Deliverables:**
- `martensite-wgpu`: Implement `GpuRenderer` surface acquisition, swapchain management, and device loss resurrection handling.
- `martensite-render`: Implement `PaintList` to Vello `Scene` translation layer. CPU fallback via TinySkia.
- `martensite-window`: Winit event loop integration, multi-DPI surface management, `WindowManager`.
**Exit Criteria:** First visible window successfully renders a rectangle. Device loss resurrection restores frame in <16ms.
**Estimated LOC additions:**
- `martensite-wgpu`: ~2000 LOC
- `martensite-render`: ~1800 LOC
- `martensite-window`: ~1500 LOC
**Key Risks:** WGPU device context loss causing panic; Vello integration overhead on low-end hardware.
**Mitigations:** Strict adherence to DDR-0003 for WGPU resilience; CPU fallback pipeline via TinySkia.

## v0.3.0 — Text & Layout
**Entry Criteria:** v0.2.0 complete. Functioning window and renderer.
**Deliverables:**
- `martensite-text`: Integrate `cosmic-text`, full `FontSystem`, BiDi, HarfBuzz shaping.
- `martensite-layout`: Implement `LayoutEngine`, Taffy `TraversePartialTree` bridge over arena, decoupled two-pass layout.
- Base widget set: primitive structural widgets (text, containers).
**Exit Criteria:** Text renders with correct bounds and BiDi handling. Flex layout correctly positions nodes.
**Estimated LOC additions:**
- `martensite-text`: ~1500 LOC
- `martensite-layout`: ~1200 LOC
**Key Risks:** Text shaping performance penalties on large text blocks. 1-frame lag in layout.
**Mitigations:** Aggressive caching in `martensite-text`; strict enforcement of zero 1-frame lag via ADR-0003 two-pass design.

## v0.4.0 — Accessibility & Focus
**Entry Criteria:** v0.3.0 complete. Text and layout stable.
**Deliverables:**
- `martensite-access`: AccessKit integration, `TreeUpdate` adapter, sync to UIA/NSAccessibility/AT-SPI2.
- `martensite-focus`: `FocusManager`, 2D projected-beam spatial navigation, modal `FocusScope` stack.
**Exit Criteria:** Screen readers navigate UI perfectly. Keyboard navigation behaves deterministically in 2D space. WCAG 2.1 AA compliance.
**Estimated LOC additions:**
- `martensite-access`: ~1800 LOC
- `martensite-focus`: ~1000 LOC
**Key Risks:** State desync between UI arena and AccessKit tree. Focus trapping in complex hierarchies.
**Mitigations:** Incremental tree updates; strict focus scope push/pop validation.

## v0.5.0 — Input & Platform
**Entry Criteria:** v0.4.0 complete. Usable UI with focus management.
**Deliverables:**
- `martensite-clipboard`: Implement `ClipboardItem`, multi-MIME OLE/Cocoa/Wayland engine with lazy evaluation.
- `martensite-dnd`: Cross-OS DnD, internal arena reparenting logic.
- `martensite-text`: IME candidate bounds projection.
- `martensite-window`: Multi-window winit event loop support.
**Exit Criteria:** Multi-window drag and drop works. IME overlays appear at cursor position.
**Estimated LOC additions:**
- `martensite-clipboard`: ~1000 LOC
- `martensite-dnd`: ~1200 LOC
- `martensite-text`: ~500 LOC
- `martensite-window`: ~800 LOC
**Key Risks:** Platform-specific deadlocks during DnD or clipboard operations.
**Mitigations:** Pure Rust FFI where possible; decoupling OS blocking calls from render loop.

## v0.6.0 — Motion & Theme
**Entry Criteria:** v0.5.0 complete. Stable inputs and OS integration.
**Deliverables:**
- `martensite-motion`: Analytical spring physics solver, `SpringConfig`, `SpringSolver`. C1 continuity interpolation.
- `martensite-theme`: `Oklab` struct, GPU uniform binding for theme transition shaders, dark/light mode toggle.
**Exit Criteria:** 150ms critically damped transitions complete flawlessly. Zero-stutter animations on theme swap.
**Estimated LOC additions:**
- `martensite-motion`: ~800 LOC
- `martensite-theme`: ~700 LOC
**Key Risks:** High CPU usage on mass animation recalculations.
**Mitigations:** Closed-form analytical spring solutions instead of iterative integration.

## v0.7.0 — Advanced Subsystems
**Entry Criteria:** v0.6.0 complete. Complete fundamental UI framework.
**Deliverables:**
- `martensite-history`: Transactional undo/redo, LCA tree algorithm for history nodes.
- `martensite-assets`: Dual-mode VFS, AOT shader loading.
- `martensite-l10n`: Project Fluent integration for localization strings.
**Exit Criteria:** Undo/redo correctly reverts nested UI state. Assets dynamically reload. Localization swaps strings instantly.
**Estimated LOC additions:**
- `martensite-history`: ~1500 LOC
- `martensite-assets`: ~1000 LOC
- `martensite-l10n`: ~900 LOC
**Key Risks:** Memory leaks in undo ledger; I/O blocking from asset loading.
**Mitigations:** Strict upper limits on history depth; asynchronous VFS backend.

## v0.8.0 — Media & Advanced GPU
**Entry Criteria:** v0.7.0 complete.
**Deliverables:**
- `martensite-media`: Zero-copy hardware surface passthrough (DXGI NT Handles, IOSurface, Vulkan dma-buf) supporting both **NV12 (8-bit SDR)** and **P010 (10-bit HDR)** formats.
- Complete HDR color pipeline: BT.709 SDR and BT.2020 PQ EOTF WGSL compute shaders, HDR swapchain negotiation (DXGI FP16, macOS EDR, Vulkan HDR10), and Hable/Uchimura filmic tone-mapping fallback for SDR displays.
- `martensite-wgpu`: Shader extensions and 3D interop APIs.
**Exit Criteria:** Hardware-accelerated 4K HDR and SDR video playback renders seamlessly inside the widget hierarchy with zero CPU frame copies (~0.0ms CPU dispatch).
**Estimated LOC additions:**
- `martensite-media`: ~3200 LOC
- `martensite-wgpu`: ~1000 LOC
**Key Risks:** Graphics API fragmentation (Vulkan/Metal/DX12) causing surface mapping failures; display HDR capability desync.
**Mitigations:** Rely heavily on `wgpu` HAL primitives; automatic tone-mapping to SDR when HDR swapchain is unavailable.

## v0.9.0 — Developer Experience
**Entry Criteria:** v0.8.0 complete. Core framework complete.
**Deliverables:**
- `martensite-devtools`: Tracy spans integration, F12 in-app HUD.
- `cargo-martensite`: Implement hot-reload host/guest cdylib split.
- `martensite-macros`: Declarative `widget!` construction macros.
- `martensite-test`: Headless golden-frame CI harness (`VirtualClock`), perceptual diffs.
**Exit Criteria:** Hot-reload functions under 350ms. Headless tests yield deterministic perceptual diffs.
**Estimated LOC additions:**
- `martensite-devtools`: ~1200 LOC
- `cargo-martensite`: ~2000 LOC
- `martensite-macros`: ~1500 LOC
- `martensite-test`: ~1800 LOC
**Key Risks:** Hot-reload crashing state boundaries; non-deterministic golden frame generation.
**Mitigations:** Strict state definition isolation in cdylib; fixed-step `VirtualClock`.

## v0.10.0 — Hardening, Plugins & Ecosystem
**Entry Criteria:** v0.9.0 complete. Full feature set implemented.
**Deliverables:**
- `martensite-plugin`: WebAssembly runtime sandbox powered by `wasmtime`. Linear memory isolation, Plugin ABI v1, and granular capability grants (`SignalRead`, `SignalWrite`, `FileRead`, `Network`).
- `martensite-blessed`: Curated higher-level crate tier for common patterns (data tables, charting, code editor, audio viewports).
- Comprehensive fuzzing campaign across all subsystems (48-hour soak).
- Accessibility audit (WCAG 2.1 AA verification via AccessKit).
- Benchmark publication comparing throughput/latency against egui and iced baselines.
**Exit Criteria:** Wasmtime sandboxed plugins mount and render safely. Fuzzers run 48 hours with zero panics. A11y audit passes. Benchmarks published.
**Estimated LOC additions:**
- `martensite-plugin`: ~2500 LOC
- `martensite-blessed`: ~3000 LOC
- Tests/Bench: ~2000 LOC
**Key Risks:** Wasmtime trampoline latency exceeding frame budget; unexpected edge-case crashes exposed by fuzzing delaying v1.0.0.
**Mitigations:** Inline trampoline optimization; begin fuzzing campaign infrastructure early in Phase 7.

## v1.0.0 — Production Stability
**Entry Criteria:** v0.10.0 complete. Zero known critical bugs.
**Deliverables:**
- API Freeze.
- Full docs.rs coverage (100%).
- Crates.io publication.
- GitHub Release.
**Exit Criteria:** Project live on crates.io, zero `todo!()`, complete test coverage.
**Estimated LOC additions:** 0 (Documentation only).
**Key Risks:** Breaking changes identified post-publication.
**Mitigations:** Aggressive beta-testing period during v0.10.0 with industrial dashboard example.
