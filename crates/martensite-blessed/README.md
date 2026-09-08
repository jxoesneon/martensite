# martensite-blessed

[![Crates.io](https://img.shields.io/crates/v/martensite-blessed.svg)](https://crates.io/crates/martensite-blessed)
[![Documentation](https://docs.rs/martensite-blessed/badge.svg)](https://docs.rs/martensite-blessed)
[![License](https://img.shields.io/badge/license-MIT%20OR%20Apache--2.0-blue.svg)](https://github.com/jxoesneon/martensite#license)

> **Curated Tier-1 ecosystem extensions and verified integration presets for the Martensite GUI framework.**

---

## Overview

`martensite-blessed` is a curated ecosystem meta-package for the Martensite GUI framework. While the core framework crates focus strictly on foundational primitives (memory arenas, reactive DAGs, layout, and rendering pipelines), production technical applications demand specialized domain widgets:
- High-performance charting and telemetry graphs
- Virtualized high-density data tables
- Monospace code editors with syntax highlighting
- Complex tree views, splitters, and dockable tabs

`martensite-blessed` provides a single audited umbrella ensuring that verified ecosystem components meet Martensite's strict architectural standards: zero dynamic allocations in active interaction loops, `#![forbid(unsafe_code)]`, and complete AccessKit accessibility coverage.

The entire crate is built under `#![forbid(unsafe_code)]`.

---

## Key Principles of Blessed Extensions

1. **Pure Rust Supply Chain**: No external C/C++ build scripts, shared library dependencies, or unmaintained transitive crates.
2. **Deterministic Memory**: Zero runtime garbage collection and bounded allocation budgets.
3. **Reactive Integration**: Native interoperability with `martensite-reactive` signals and memos.
4. **Accessible by Default**: Screen reader semantics declared upfront via `martensite-access`.

---

## Part of Martensite

This crate provides curated ecosystem components for the [Martensite](https://github.com/jxoesneon/martensite) GUI framework. For top-level widgets and application integration, see [`martensite`](https://crates.io/crates/martensite).

---

## License

Licensed under either of:

- Apache License, Version 2.0 ([LICENSE-APACHE](https://github.com/jxoesneon/martensite/blob/main/LICENSE-APACHE) or <http://www.apache.org/licenses/LICENSE-2.0>)
- MIT license ([LICENSE-MIT](https://github.com/jxoesneon/martensite/blob/main/LICENSE-MIT) or <http://opensource.org/licenses/MIT>)

at your option.
