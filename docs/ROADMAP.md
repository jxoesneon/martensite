# Martensite v1.0.0 Implementation Roadmap

> For granular component specifications, verified invariants, and verification gates, see the [Master Milestone Specification Index](milestones/INDEX.md).

## v0.1.0 — The Hardened Foundation
*Detailed Specification:* [docs/milestones/v0.1.0-foundation.md](milestones/v0.1.0-foundation.md)
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
*Detailed Specification:* [docs/milestones/v0.2.0-render-pipeline.md](milestones/v0.2.0-render-pipeline.md)
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
*Detailed Specification:* [docs/milestones/v0.3.0-text-layout.md](milestones/v0.3.0-text-layout.md)
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
*Detailed Specification:* [docs/milestones/v0.4.0-accessibility-focus.md](milestones/v0.4.0-accessibility-focus.md)
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
*Detailed Specification:* [docs/milestones/v0.5.0-input-platform.md](milestones/v0.5.0-input-platform.md)
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
*Detailed Specification:* [docs/milestones/v0.6.0-motion-theme.md](milestones/v0.6.0-motion-theme.md)
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
*Detailed Specification:* [docs/milestones/v0.7.0-subsystems.md](milestones/v0.7.0-subsystems.md)
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
*Detailed Specification:* [docs/milestones/v0.8.0-media-hdr.md](milestones/v0.8.0-media-hdr.md)
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
*Detailed Specification:* [docs/milestones/v0.9.0-developer-experience.md](milestones/v0.9.0-developer-experience.md)
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
*Detailed Specification:* [docs/milestones/v0.10.0-plugins-ecosystem.md](milestones/v0.10.0-plugins-ecosystem.md)
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

## v0.11.0 — Typography & Accessibility Expansion
*Detailed Specification:* [docs/milestones/v0.11.0-typography-a11y.md](milestones/v0.11.0-typography-a11y.md)
**Status:** SHIPPED.
**Deliverables:**
- `martensite-text`: Swash/HarfBuzz BiDi (UAX #9), vertical-rl (UAX #50), system font cascades via `martensite-font-fallback` (DirectWrite `MapCharacters`, CoreText `CTFontCreateForStringWithLanguage`, Fontconfig `FcFontSort`).
- `martensite-access`: WCAG 2.2 AA/AAA, Section 508 VPAT, caret tracking.
**Exit Criteria:** 100% BiDi/vertical layout parity; 0 missing glyphs; WCAG 2.2 AAA pass; caret sync <16.6ms.

## v0.12.0 — Blessed Widgets & Kinematics
*Detailed Specification:* [docs/milestones/v0.12.0-blessed-kinematics.md](milestones/v0.12.0-blessed-kinematics.md)
**Status:** SHIPPED.
**Deliverables:**
- `martensite-blessed`: 1,000,000-row virtualized DataGrid (sort/filter/select), BSP docking tree with multi-swapchain panels, code editor, charts, audio waveform.
- `martensite-motion`: 0.55 rubber-band overscroll; `martensite-window`: 6-DoF Kalman stylus prediction.
**Exit Criteria:** 1M-row scroll at steady 120fps; zero-alloc docking split/merge; Kalman latency <2.0ms.

## v0.13.0 — Modern Shell & Platform
*Detailed Specification:* [docs/milestones/v0.13.0-modern-shell.md](milestones/v0.13.0-modern-shell.md)
**Status:** SHIPPED (v0.13.0 released on crates.io).
**Deliverables:**
- `martensite-shell`: Windows 11 Mica/MicaAlt/Acrylic + Snap Layouts, macOS `NSVisualEffectView`/`NSGlassEffectView` Liquid Glass + Reduce Transparency detection, Wayland `wp_fractional_scale_v1` + CSD + StatusNotifierItem tray.
- `martensite-render`: real GPU Gaussian `BlurredRect` (Vello `draw_blurred_rounded_rect`) + CPU three-pass box blur.
**Exit Criteria:** DWM backdrop switch <8ms; zero CSD blur under fractional DPI; native hit-testing across GNOME/KDE/wlroots.

## v0.14.0 — External Surface Foundation
*Detailed Specification:* [docs/milestones/v0.14.0-external-surfaces.md](milestones/v0.14.0-external-surfaces.md)
**Entry Criteria:** v0.13.0 complete.
**Deliverables:**
- `martensite-engine-bridge` (NEW): `Engine`/`Frame`/`FrameSync`/`Viewport` producer-consumer protocol, `BridgeHandle` damage signaling, two-slot frame ring.
- `martensite` `ExternalEngine` widget: Taffy replaced-element leaf emitting `PaintCommand::External`.
- `martensite-wgpu` `WgpuHost`: direct-`TextureView` composite pipeline (fullscreen triangle, no atlas copy).
**Exit Criteria:** Same-device composite at zero GPU copy; damage-driven redraw; documented TinySkia fallback.
**Key Risks:** wgpu version pinning vs ecosystem; frame-ready → redraw latency.
**Mitigations:** Same-queue `submit` ordering; `on_submitted_work_done` for async producers.
**Note:** Re-scoped from the original "Engine Embedding & Media" entry after v0.13.0 competitive analysis — zero-copy surface import and BT.2408 scaling already shipped in v0.8.0; engine adapters moved to v0.15.0. Direction A (host-mode embed) recorded in ADR-0033.

## v0.15.0 — Engine Showcase
*Detailed Specification:* [docs/milestones/v0.15.0-engine-showcase.md](milestones/v0.15.0-engine-showcase.md)
**Status:** In progress.
**Entry Criteria:** v0.14.0 complete.
**Deliverables:**
- `martensite-bevy`: headless Bevy app (`WinitPlugin` disabled), `RenderCreation::Manual` device injection, `RenderTarget::TextureView` viewport, input forwarding via `bevy_picking` `PointerInput`.
- `martensite-godot`: GDExtension (`godot` crate 0.5.x) — `texture_get_data_async` readback path (shipped); feature-gated shared-texture blit path (experimental, one GPU copy).
- `examples/viewport_showcase`: Bevy 3D scene and Godot viewport side-by-side inside a Martensite window with native shell chrome.
**Exit Criteria:** Bevy viewport at 120fps with zero GPU copy; Godot viewport via readback with published throughput/latency.
**Key Risks:** Bevy wgpu-30 coupling (needs git pin or 0.20); Godot cannot do true zero-copy without engine patches.
**Mitigations:** Adapter crates are `publish = false` and excluded from the default workspace build (checked by the dedicated `adapters` CI job); Godot limitation documented, upstream contribution path noted.

## v0.16.0 — Hardware Media Pipeline
*Detailed Specification:* [docs/milestones/v0.16.0-media-pipeline.md](milestones/v0.16.0-media-pipeline.md)
**Entry Criteria:** v0.14.0 complete.
**Deliverables:**
- `martensite-media` `VideoDecoder` trait + `FrameQueue` + `HdrMetadata`.
- `martensite-media-platform` decoder backends: VideoToolbox→IOSurface (macOS), MF+D3D11→DXGI shared handle (Windows), `cros-libva`→dma-buf (Linux), `ffmpeg-next` software fallback.
- Multi-plane NV12/P010 import fix (Y+UV as two textures).
**Exit Criteria:** 4K 120fps <0.1% frame drops, <1% CPU dispatch on dedicated GPU runner; noop-wgpu + CPU paths verified in CI.
**Key Risks:** 4K120 gate requires self-hosted GPU hardware; pure-Rust software decode cannot reach 4K120.
**Mitigations:** `#[ignore]`-gated hardware tests; CI covers mock/noop/software paths.

## v0.17.0 — Platform Expansion
*Detailed Specification:* [docs/milestones/v0.17.0-platform-expansion.md](milestones/v0.17.0-platform-expansion.md)
**Entry Criteria:** v0.14.0–v0.16.0 complete.
**Deliverables:**
- Widget breadth: slider, radio group, dropdown/listbox, scrollview (chaining + anchoring), tabs, tooltip — full ARIA APG + AccessKit contracts; new overlay/popup layer.
- Web: `wasm32-unknown-unknown` via wgpu WebGPU + TinySkia fallback; bundled fonts; hidden-input IME; minimal hidden-DOM a11y bridge (no upstream AccessKit web adapter exists).
- Mobile: iOS (UIKit + Metal + `accesskit_ios`) and Android (`GameActivity` + Vulkan/GLES + `accesskit_android`).
- `martensite-devtools::timemachine`: hybrid command-ledger (`martensite-history`) + periodic arena/signal snapshots + deterministic replay.
**Exit Criteria:** APG conformance on all six widgets; wasm renders via WebGPU; iOS/Android example apps; deterministic replay verified under `VirtualClock`.
**Key Risks:** Web accessibility has no upstream adapter; `accesskit_winit` fork must track winit 0.31; Android safe-area not in winit.
**Mitigations:** Scope web a11y to minimal viable bridge; keep vendored fork maintained; Android `WindowInsets` platform code.

## v1.0.0 — Production Stability
*Detailed Specification:* [docs/milestones/v1.0.0-production-release.md](milestones/v1.0.0-production-release.md)
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
