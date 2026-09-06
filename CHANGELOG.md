# Changelog

All notable changes to Martensite are documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

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
