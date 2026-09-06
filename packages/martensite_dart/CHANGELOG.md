## 0.1.0

- First milestone release of Martensite, a sovereign Rust GUI framework.
- Core: generational arena storage with 64-byte HotNode cache-line invariant.
- Reactive: fine-grained signals and memoization with glitch-free diamond propagation.
- Layout, render, text, accessibility, window, focus, clipboard, DnD, theme,
  motion, history, localization, and test subsystems.
- 150 tests passing, property-based tests for core/reactive, 10M-op fuzz test.
- Benchmark exit gates: 10k-node signal propagation < 1ms, 1k diamond zero-redundancy.
- CI: fmt, clippy, test, doc, audit, deny, strict benchmarks, pana.

## 0.0.2

- Aligned Dart package version with Rust workspace (0.0.2).
- Internal metadata and CI improvements.

## 0.0.1

* Initial release of the official Dart bindings for the Martensite GUI engine.
* Native FFI bridge for generational slotmap handles and push-pull reactive signals.
* High-performance 2D vector canvas rendering integration.
