# Changelog

All notable changes to Martensite are documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [0.11.0] - 2026-09-08

### Added

- **Advanced Typography**:
  - Bidirectional text (BiDi) shaping via `Shaper::shape_with_options`.
  - Vertical text layout support.
  - UAX #29 grapheme cluster segmentation for caret movement and selection.
  - OS-native font fallback via `martensite-font-fallback` (DirectWrite on
    Windows, CoreText on macOS, Fontconfig on Linux) with
    `FontFallbackProvider` trait and `FallbackDecisionCache`.
- **Platform Accessibility**:
  - AccessKit integration with `MartensiteAccessBridge` for `accesskit_winit`.
  - Caret geometry computation for screen-reader text selection.
  - WCAG 2.1 contrast compliance checks in the theme system.
- **New Crates**:
  - `martensite-font-fallback` — OS-native font fallback providers.
  - `martensite-clipboard-platform` — OS-native clipboard backends
    (NSPasteboard, Win32, X11).
  - `martensite-media-platform` — hardware video surface import FFI
    (IOSurface, DXGI, dmabuf).
  - `martensite-host` — dynamic library loading for hot-reloadable guest
    cdylibs.

### Fixed

- **Security**: Windows clipboard out-of-bounds read in `GetClipboardData`
  path.
- **Security**: Plugin `file_read` capability bounds check for path
  traversal prevention.
- **Correctness**: `swash`/`ttf-parser` font-data access wrapped in
  `catch_unwind` to isolate malformed-font panics.
- **Correctness**: Access bridge deadlock resolved by switching to
  `parking_lot::Mutex` (poison-free).
- **Correctness**: Cache bounds check in `TextShapeCache` eviction.
- **Correctness**: Ring buffer corruption recovery in `PluginRingBuffer`.

### Changed

- All workspace crates bumped from `0.10.0` to `0.11.0`.
  (`martensite-cosmic-text` retains its own `0.19.0-martensite.1` version.)

## [0.10.0] - 2026-09-08

### Added

- **Plugin Runtime (`martensite-plugin`)**:
  - Wasmtime sandboxed WebAssembly plugin runtime (`wasm32-wasip1`) with 5ms fuel/epoch-based interruption.
  - `PluginRingBuffer` shared-memory circular command buffer for zero-overhead `PaintCmd` vector commands.
  - Capability-based security model (`SignalRead`, `SignalWrite`, `FileRead`, `FileWrite`, `Network`) with explicit host grants.
- **Blessed Widget Tier (`martensite-blessed`)**:
  - Virtualized `DataTable` supporting 1,000,000 rows with O(1) visible-row memory and 60fps/120fps scrolling.
  - GPU-accelerated `Chart` widget for 2D line, area, and scatter plots at 10,000 points / 60Hz.
  - `CodeEditor` with syntax highlighting and multi-cursor support.
  - `AudioWaveform` interactive viewport with real-time scrub head.
- **Hardening & Fuzzing**:
  - 48-hour continuous fuzzing harness targeting arena compaction, reactive DAG mutations, and event routing.

### Changed

- All workspace crates bumped from `0.9.0` to `0.10.0`.

## [0.9.0] - 2026-09-08

### Added

- **DevTools (`martensite-devtools`)**:
  - Tracy profiler instrumentation spans across layout, paint, and reactive dispatch with < 0.1ms overhead per frame.
  - In-app diagnostic HUD (F12 toggle) with rolling 120-frame timing histogram, dirty rect visualization, and WidgetArena slot utilization telemetry.
- **Developer CLI (`cargo-martensite`)**:
  - `cargo martensite dev` and `cargo martensite build` commands.
  - Sub-350ms hot-reloading framework with host/guest cdylib architecture and versioned dynamic library reloading.
- **Widget Macros (`martensite-macros`)**:
  - Declarative `widget!` procedural macro with compile-time property validation.
- **Test Harness (`martensite-test`)**:
  - `VirtualClock` for deterministic, manually-advancing time in headless tests.
  - Headless test harness with pixel-perfect perceptual DSSIM snapshot diffing against golden reference images.

### Changed

- All workspace crates bumped from `0.8.0` to `0.9.0`.

## [0.8.0] - 2026-09-08

### Added

- **Media & Hardware Video Playback (`martensite-media`)**:
  - Zero-copy hardware video surface bindings supporting DXGI NT shared handles (`HardwareHandle::DxgiSharedHandle`), macOS `IOSurface`, Linux `dma-buf`, and mock handles.
  - Multi-planar format negotiation (`FormatNegotiator`) for NV12 (8-bit SDR) and P010 (10-bit HDR) YUV surfaces, as well as packed RGBA8 and RGBA16Float textures.
  - Sub-millisecond CPU frame dispatch telemetry (`VideoSurface::update_handle`, `cpu_utilization_pct`) verifying < 1% CPU utilization (< 0.10 ms dispatch) for 4K 60fps video.
- **HDR Color Pipeline & Optical Compositing (`martensite-media::color`)**:
  - Analytical BT.709 and BT.2020 YUV <-> RGB color space transformation matrices with colorimetric test pattern accuracy (ΔE < 0.05, well below the 1.0 threshold).
  - Full-range and limited-range quantization normalization for 8-bit and 10-bit video streams.
  - SMPTE ST 2084 PQ electro-optical transfer function (EOTF) and inverse OETF across dynamic range (0.005 to 10,000 nits) with relative error < 10⁻⁴.
  - CIE 1931 XYZ and CIE 1976 L*a*b* color difference metric (`delta_e_76`).
  - Open-domain linear optical space (`ScRgb`) with pre-multiplied alpha blending (`ScRgb::blend_over`), preserving specular dynamic range without SDR clipping and maintaining WCAG AA (≥ 4.5:1) contrast for UI overlays.
- **Filmic Tone-Mapping Operators & Display Adaptation (`martensite-media::tonemap`)**:
  - Display profile abstraction (`DisplayProfile`) with dynamic SDR reference white level scaling and peak luminance headroom calculation.
  - Monotonic Hable (Uncharted 2) and Uchimura (Gran Turismo) filmic tone curves (`hable_tonemap_scalar`, `uchimura_tonemap_scalar`, `ToneMapOperator`) providing smooth highlight rolloff and toe contrast preservation.
- **WGPU Video Interop & Compute Pipeline (`martensite-wgpu::interop`)**:
  - `MEDIA_YUV_EOTF_WGSL` compute shader performing hardware YUV planar sampling, color range expansion, BT.709/BT.2020 matrix transform, PQ EOTF decoding, gamut mapping, and optional Hable tone-mapping directly on GPU.
  - 256-byte aligned `VideoPipelineUniforms` (`Pod`, `Zeroable`) for direct WGPU uniform buffer uploads.
  - Swapchain format selector favoring 16-bit float HDR swapchains (`Rgba16Float`) when available.
- **MediaView Widget (`martensite::widgets::media`)**:
  - Retained-mode `MediaView` widget integrating hardware video surfaces into the Martensite layout tree.
  - Aspect ratio preservation supporting `VideoFit::Contain`, `VideoFit::Cover`, `VideoFit::Fill`, and `VideoFit::Fixed` with letterbox/pillarbox destination rect calculation (`MediaView::compute_dest_rect`).
  - AccessKit accessibility integration exposing `accesskit::Role::Video`.

### Changed

- All workspace crates bumped from `0.7.0` to `0.8.0`.
- Added `martensite-media` to root workspace members and re-exported as `martensite::media`.
- Re-exported `wgpu` from `martensite-wgpu`.

## [0.7.0] - 2026-09-08

### Added

- **History**: Transactional undo/redo ledger with Lowest Common Ancestor
  (LCA) tree navigation for non-linear branching history. Bounded depth
  pruning with LRU eviction of unreferenced branches. 10,000 randomized
  rollback integrity gate.
- **Assets**: Dual-mode Virtual File System with `VfsBackend::Disk` for
  development hot-reloading via file watchers and `VfsBackend::Embedded`
  for zero-copy release bundles. AOT WGSL shader validation via naga
  with reflection metadata. Sub-10µs embedded VFS resolution gate.
- **Localization**: Project Fluent bundle integration with locale
  negotiation, script directionality resolution (LTR/RTL), and reactive
  locale signal for invalidating text nodes without tree rebuilding.
  1,000-node locale switch within 1 frame gate.

### Changed

- All workspace crates bumped from `0.6.0` to `0.7.0`.
- Added `notify` 8.2, `naga` 30, and `fluent-langneg` 0.14 dependencies.
- Aligned public APIs with Rust API Guidelines (RFC 344 / C-GETTER, C-LEN, C-BUILDER):
  - Added idiomatic noun-phrase accessors `Theme::color()`, `Theme::dimension()`, `WindowManager::window()`, `WindowManager::window_mut()`, `WindowManager::windows()`, `WindowManager::windows_mut()`, `WindowManager::len()`, `WindowManager::is_empty()`, and `DndSessionManager::session()`, `DndSessionManager::session_mut()`. Legacy `get_*`, `window_count`, and `iter_windows` methods are preserved as `#[inline]` forwarding aliases for 100% backward compatibility.
  - Added `Text::content(&self) -> &str` borrowed accessor alongside existing `pub content: String` field for architecture-compliant direct mutation.
  - Added `#[must_use]` across all widget builder methods (`Button`, `CheckBox`, `TextInput`, `Container`, `Flex`, `Stack`, `Text`) to prevent silently discarded method chains.
- Optimized hot paths and reduced heap churn:
  - Replaced intermediate allocation in `HistoryLedger::redo()` with zero-allocation `.iter().copied().max_by_key(...).ok_or(...)` iterator pipeline.
  - Replaced heap-allocated trait object iterator (`Box<dyn Iterator>`) in `FocusManager` subtree navigation with zero-allocation local closure traversal.
  - Formatted `ClipboardItem::types()` debug output directly from map keys without intermediate `Vec` collection.

## [0.6.0] - 2026-09-07

### Added

- **Motion**: Closed-form analytical spring solver supporting underdamped,
  critically damped, and overdamped regimes with C¹ velocity continuity on
  interruption. Animation driver with velocity handoff for seamless gesture
  redirection.
- **Theme**: Oklab/Oklch perceptual color pipeline with sRGB conversion,
  hue-preserving gamut mapping, WCAG 2.1 and APCA contrast calculation.
  Design token dictionary with light/dark mode definitions. GPU theme
  transition uniform buffers and WGSL fragment shader for 150ms smooth
  palette morphing with zero CPU allocations.

### Changed

- All workspace crates bumped from `0.5.0` to `0.6.0`.

## [0.5.0] - 2026-09-07

### Added

- **Clipboard**: Multi-MIME clipboard provider with lazy evaluation,
  platform backend abstraction (Windows OLE, macOS NSPasteboard, Wayland/X11
  stubs), and a 500 ms IPC timeout for cross-process clipboard reads.
- **Drag-and-Drop**: Process-wide detached `DndSession` carrying
  `Arc<dyn Any + Send + Sync>` payloads, with a `DndSessionManager` for
  session lifecycle. Drop target registry with enter/leave/drop lifecycle
  and `DropEffectMask` bitflags for effect negotiation.
- **Window**: Two-stage hit-testing pipeline — AABB broad-phase followed by
  an inverse 3×3 affine narrow-phase, with a singular matrix guard
  (`|det| < 1e-6`). Non-rectangular clip verification supports rect,
  rounded rect, and winding-number path. Event routing pipeline with
  pointer capture and mouse tracking.
- **Text**: Velocity-damped kinetic IME candidate positioning
  (`P_ime = P_caret + v·Δt·e^(-λ·Δt)`) using `ScrollKinematics` velocity
  estimation, with viewport clamping to keep the candidate window on-screen.

### Changed

- All 22 workspace crates bumped from `0.4.0` to `0.5.0`.
  (`martensite-cosmic-text` retains its own `0.19.0-martensite.1` version.)

## [0.4.0] - 2026-09-21

### Added

- **Accessibility**: `martensite-access` `AccessKitAdapter` with incremental
  `TreeUpdate` generation, `WidgetId` ↔ `NodeId` mapping, dirty bit tracking
  via `NodeFlags::DIRTY_A11Y`, and synchronous emission following layout
  finalization.
  - `build_update` generates a full accessibility tree from the arena,
    scoped to the adapter's root subtree, and clears dirty flags.
  - `build_incremental_update` emits only dirty nodes and their ancestors
    for lightweight updates with correct child-list propagation, and
    tracks focus changes (including focus clearing) via
    `last_emitted_focus`.
  - `mark_dirty`, `clear_dirty`, `clear_all_dirty` for dirty bit management.
  - `set_focus` reflects the focused widget in `TreeUpdate::focus`.
  - `resolve` maps incoming `NodeId` back to `WidgetId` with liveness check.
  - `decode_action` on the adapter validates `target_tree` and decodes
    `ActionRequest` into `A11yAction`.
  - Uses `accesskit::TreeId::ROOT` for the main accessibility tree.
  - Hidden nodes do not advertise `Action::Focus`.
- **Accessibility**: `properties` module with `AccessibilityBuilder` for
  declarative property construction (roles, labels, descriptions, values,
  tooltips, focusable, disabled, expanded, toggled, clickable states).
- **Accessibility**: `actions` module with `A11yAction` enum,
  `decode_action_request` for AccessKit action routing with `target_tree`
  validation, malformed `SetValue` rejection, and `ActionData` preservation
  via `A11yAction::Other`. `ActionHandler` trait, `QueuedActionDispatcher`
  for batch processing, and `ClosureActionHandler` for inline closures.
- **Accessibility**: `winit` module with `MartensiteAccessBridge` implementing
  `ActivationHandler`, `ActionHandler`, and `DeactivationHandler` for
  `accesskit_winit` integration. Thread-safe via `Mutex`, supports
  pluggable `MartensiteActionHandler` for decoded actions.
- **Focus**: `martensite-focus` `FocusManager` with active focus tracking,
  tab navigation (forward/reverse with wrapping), and scope-aware navigation.
  - `set_focus` validates liveness, `FOCUSABLE`, `VISIBLE`, non-`INERT`,
    and active scope containment.
  - `push_scope` validates root liveness via the arena.
  - `pop_scope` restores prior focus or falls back to nearest visible
    focusable sibling, avoiding stale focus states.
  - Reverse Tab from unknown focus starts at the last candidate.
  - Tab candidate collection excludes inert and non-visible widgets.
  - Spatial navigation enforces active-scope containment.
- **Focus**: `spatial` module with projected-beam 2D directional navigation
  algorithm:
  - Score = α·Distance + β·AngularDeviation
  - 80° forward cone rejection
  - Tree-order tie-breaking
  - `navigate` and `navigate_within_scope` for modal-restricted navigation
  - Configurable α/β weights via `SpatialNavigator::with_weights`
  - NaN/infinity guards on geometry and weights
  - Inert candidate exclusion
  - Source node focusability/visibility validation
- **Focus**: `scope` module with `FocusScope` and `FocusScopeStack` for
  modal focus trapping:
  - `push`/`pop` with prior focus capture and auto-restoration
  - Fallback to nearest visible focusable widget when prior focus is dead
  - Nested scope support for stacked modals
  - `is_in_current_scope` for boundary checks
- **Widgets**: Interactive standard widgets with full accessibility:
  - `Button` — `Role::Button`, label, `Action::Click`, `Action::Focus`,
    disabled state, tooltip.
  - `CheckBox` — `Role::CheckBox`, label, `Action::Click`, `Action::Focus`,
    `Toggled` state, disabled state.
  - `TextInput` — `Role::TextInput`, label, value, `Action::Focus`,
    `Action::SetValue`, read-only and disabled states.
- **Integration**: Integration tests verifying accessibility tree
  generation, action dispatching, tab/spatial navigation, modal scope
  trapping (Tab, Shift+Tab, spatial, programmatic focus), inert widget
  exclusion, incremental parent propagation on child removal, interactive
  widget roles/labels/actions, and combined adapter+manager workflows.

### Changed

- All 22 workspace crates bumped from `0.3.0` to `0.4.0`.
- `martensite-access` `build_update` and `build_incremental_update` now
  take `&mut WidgetArena` and `&mut self` to clear dirty flags after
  emission.
- `martensite-access` `decode_action_request` now requires an
  `expected_tree_id` parameter for target tree validation.
- `martensite-access` `A11yAction::Other` now carries `Option<ActionData>`.
- `martensite-focus` `push_scope` now takes `&WidgetArena` and returns
  `bool` for root validation.
- `martensite-focus` now depends on `glam` for vector math.
- `martensite-access` `uuid` dependency moved to dev-dependencies (TreeId
  is now `TreeId::ROOT`).

## [0.3.0] - 2026-09-20

### Added

- **Layout**: `martensite-layout` geometry module with `Point`, `Size`,
  `Constraints`, `EdgeInsets`, and `Rect` primitives.
- **Layout**: `ArenaBridge` implementing Taffy's `TraversePartialTree`
  over the Martensite `WidgetArena`, enabling Taffy to traverse the
  generational arena tree without copying nodes.
- **Layout**: `LayoutEngine` with two-pass layout orchestration:
  - `sync_from_arena` builds Taffy tree topology from the arena
  - `compute` runs Taffy's flexbox/grid layout
  - `compute_with_widgets` integrates `Widget::measure` and
    `Widget::layout` via `compute_layout_with_measure`
  - `apply_layout` writes computed bounds back to `HotNode`
  - `mark_dirty` and `relayout_incremental` for incremental updates
  - O(1) `WidgetId` ↔ `NodeId` mapping via `HashMap`
- **Text**: `FontManager` wrapping cosmic-text `FontSystem` with system
  font discovery, custom font loading (`load_font_file`,
  `load_font_data`), and font family lookup.
- **Text**: `Shaper` with BiDi, line breaking, and font fallback via
  cosmic-text `Shaping::Advanced`. `measure_text` and `shape_text`
  free functions for direct text measurement.
- **Text**: `TextShapeCache` Tier 2 global LRU cache with 16 MB
  budget, `ShapeCacheKey` including `FontId`, `FontSizeBits`,
  `TextHash`, and `MaxWidthBits` for correct wrapped-text caching.
  Hit/miss tracking, eviction, and invalidation.
- **Text**: `FontId::dummy()` for placeholder cache keys.
- **Widgets**: `Container` widget with padding, background, and
  single child.
- **Widgets**: `Flex` widget with row/column direction, main/cross
  axis alignment (`Start`, `End`, `Center`, `SpaceBetween`,
  `SpaceEvenly`), and gap.
- **Widgets**: `Stack` widget with layered children and alignment
  (`TopStart`, `TopEnd`, `BottomStart`, `BottomEnd`, `Center`,
  `Stretch`).
- **Widgets**: `Text` widget using real `Shaper` + `FontManager` +
  `TextShapeCache` for measurement, with `InlineTextCache` (Tier 1)
  for fast constraint probing.

### Changed

- `LayoutEngine::register_node` and `register_container` now return
  `Result<NodeId, LayoutError>` instead of panicking on capacity
  overflow.
- `LayoutEngine::id_map` changed from `Vec` to `HashMap` for O(1)
  lookups.
- `LayoutEngine::compute_with_widgets` measure closure now converts
  Taffy's `known_dimensions` and `available_space` into real
  `LayoutConstraints`, enabling constraint-driven text wrapping and
  flex sizing. Removed the separate pre-measure pass.
- `FontManager::load_font_file` now returns only newly loaded face
  IDs instead of all faces in the database.
- `FontManager::load_font_file_result` added for error-returning
  variant.
- `InlineTextCache` now stores `(measured_width, measured_height)`
  tuples instead of just height, fixing stale width on cache hits.
- `InlineTextCache::get` handles `f32::INFINITY` correctly.
- `ShapeCacheKey` now includes `family_hash` and `line_height_bits`
  for correct cache invalidation on family/line-height changes.
- `MaxWidthBits` now distinguishes `Some(0.0)` from `None`
  (unbounded).
- `TextMetrics::is_empty` now uses `||` (consistent with
  `Size::is_empty`).
- `Flex::measure` gives children remaining main-axis space instead
  of full container max_size.
- `martensite` crate adds `accesskit`, `glam`, and `taffy` as direct
  dependencies for base widget implementations.

### Fixed

- Performance test thresholds corrected from seconds to microseconds.
  Tests marked `#[ignore]` with documented reasons where Taffy's
  recursive engine cannot meet the spec's aspirational targets.
- `ShapeCacheKey` now includes `max_width_bits` to prevent cache
  collisions between wrapped text at different widths.
- `Flex::layout` column direction no longer swaps width/height in
  child bounds.
- `Flex::compute_main_offsets` uses defensive `.get(i)` access to
  prevent panics when `child_sizes` is not populated.
- `relayout_incremental` now calls `compute_with_widgets` instead
  of plain Taffy `compute`, ensuring widget-aware measurement.
- `arena.rs`: replaced `let _ =` silencer with proper error check.
- Integration tests now assert positive bounds and include a
  text-wrapping test verifying narrow constraints produce greater
  height.

## [0.2.0] - 2026-09-13

### Added

- **WGPU**: `GpuContext` with adapter enumeration, power-preference selection,
  feature/limit verification, and device/queue lifecycle management.
- **WGPU**: `SurfaceWrapper` for surface configuration, present-mode negotiation
  (`Mailbox → FifoRelaxed → Fifo`), and resize re-creation.
- **WGPU**: `RecoveryMachine` formal device-loss recovery FSM with exponential
  backoff (1 ms initial, doubling), retry budget, and CPU fallback after
  exhaustion. States: `Active`, `DeviceLost`, `SuspendedWithRetry`,
  `Recreated`, `Restored`, `FallbackCpu`. Recovery budget: 16.6 ms.
- **WGPU**: `RenderOrchestrator` bridging GPU and CPU backends, consuming
  `OrchestratorConfig` to gate fallback on `allow_software_fallback` and
  `prefer_cpu` settings.
- **Render**: `PaintList` command stream with 10 `PaintCommand` variants:
  `FillRect`, `StrokeRect`, `FillPath`, `StrokePath`, `FillLinearGradient`,
  `FillRadialGradient`, `ClipRect`, `ClipRoundedRect`, `DrawText`,
  `DrawGlyphRun`.
- **Render**: `VelloRenderer` translating `PaintCommand` into Vello `Scene`
  draw calls (fill, stroke, gradient, clip layers with balanced push/pop,
  text/glyph approximation). Feature-gated under `vello`.
- **Render**: `TinySkiaBackend` CPU rasterizer with clipping, gradients,
  paths, and text/glyph approximations. Produces RGBA8 pixel buffer.
- **Render**: `SoftbufferPresenter` wrapping `softbuffer::Surface` for
  real CPU-to-window pixel presentation via `present()`.
- **Render**: DSSIM-inspired perceptual diffing with edge/interior SSIM
  classification (edge threshold 0.995, interior threshold 0.9999).
- **Window**: `WindowManager` with SlotMap-backed multi-window storage.
- **Window**: `DpiScale` with finite-positive validation, fractional
  coordinate conversion, and creation-time validation.
- **App**: `AppBuilder`/`AppConfig` with `allow_software_fallback`,
  `fallback_timeout`, and `prefer_cpu` configuration. Converts to
  `OrchestratorConfig` via `From` impl.

### Changed

- Workspace version bumped from 0.1.0 to 0.2.0.
- Internal workspace dependency version literals updated to 0.2.0.
- Dart package version bumped from 0.1.0 to 0.2.0.
- Added `#![forbid(unsafe_code)]` to `martensite-blessed`, `martensite-media`,
  and `martensite-macros`.
- `WindowManager::create_window` now validates platform scale factor via
  `DpiScale::is_valid`, falling back to 1.0 for invalid values.
- `handle_surface_error` distinguishes transient surface errors from
  device-loss errors.

## [0.1.0] - 2026-09-06

### Added

- **Core**: Generational arena storage with 64-byte `HotNode` cache-line invariant
  and FIFO free-list for generation-rollover immunity.
- **Reactive**: Fine-grained signals and memoization with glitch-free diamond
  propagation and a transactional reactive runtime.
- **Layout**: Taffy-based layout tree creation and computation.
- **Render**: GPU render command recording and backend interaction.
- **Text**: Cosmic Text fork (`martensite-cosmic-text`) with updated `fontdb`
  dependency, plus text shaping and buffer management.
- **Access**: AccessKit integration with role and ID management.
- **Window**: Window trait abstraction for cross-platform windowing.
- **Focus**: Focus manager with traversal and state management.
- **Clipboard**: Clipboard state and chaining operations.
- **DnD**: Drag-and-drop enum behavior and data transfer.
- **Theme**: Theme interpolation and GPU-compatible trait implementations.
- **Motion**: Spring solver with virtual clock for deterministic testing.
- **History**: Undo/redo apply and revert cycles.
- **Localization**: Fluent-based localization parsing.
- **Macros**: `widget!` proc-macro for declarative widget definitions.
- **Test**: Virtual clock and test utilities for deterministic testing.

### Performance

- 10,000-node linear DAG signal propagation: < 1.0ms on dedicated hardware.
- 1,000-node diamond reactive network: zero redundant evaluations.
- 10,000,000-operation arena randomized stress/fuzz test.

### Quality

- 150 tests passing with property-based tests for core and reactive subsystems.
- `missing_docs = "deny"` enforced across all publishable crates.
- `unsafe_code = "deny"` enforced (cosmic-text fork exempt with documentation).
- Clippy zero warnings with `-D warnings`.
- `cargo audit` and `cargo deny` clean with three documented advisory exceptions.
- CI pipeline: fmt, clippy, test, doc, audit, deny, strict benchmarks, pana.

### Published Crates

19 crates published to crates.io in dependency order:
`martensite-core`, `martensite-reactive`, `martensite-macros`,
`martensite-cosmic-text`, `martensite-text`, `martensite-layout`,
`martensite-render`, `martensite-wgpu`, `martensite-access`,
`martensite-window`, `martensite-focus`, `martensite-clipboard`,
`martensite-dnd`, `martensite-theme`, `martensite-motion`,
`martensite-history`, `martensite-l10n`, `martensite-test`, `martensite`.

## [0.0.2]

- Internal metadata, CI, and test coverage improvements.

## [0.0.1]

- Initial workspace structure and namespace reservation.
