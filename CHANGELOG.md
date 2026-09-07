# Changelog

All notable changes to Martensite are documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

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
