# martensite-devtools

[![Crates.io](https://img.shields.io/crates/v/martensite-devtools.svg)](https://crates.io/crates/martensite-devtools)
[![Documentation](https://docs.rs/martensite-devtools/badge.svg)](https://docs.rs/martensite-devtools)
[![License](https://img.shields.io/badge/license-MIT%20OR%20Apache--2.0-blue.svg)](https://github.com/jxoesneon/martensite#license)

> **Telemetry spans, Tracy/Chrome GPU profiling timestamps, and in-app F12 developer diagnostics HUD for Martensite.**

---

## Overview

`martensite-devtools` provides real-time performance inspection, telemetry instrumentation, and visual diagnostics for the Martensite GUI framework.

When developing complex interfaces with high-density data visualizations, debugging frame drops and layout re-calculations requires deep insight into the rendering pipeline. `martensite-devtools` provides structured [tracing](https://crates.io/crates/tracing) spans across all subsystems (arena mutations, reactive propagation, Taffy layout passes, text shaping, and Vello GPU command recording), alongside an interactive in-app HUD toggled via F12.

The entire crate is built under `#![forbid(unsafe_code)]`.

---

## Key Features

- **In-App Developer HUD (F12)**: Overlays live frame timing graphs, 99th percentile frame latency counters, and active draw-call counts directly onto the running window.
- **Scenegraph & Arena Inspector**: Real-time visualization of active `HotNode` count, allocated slot capacity, and dirty bitset states across the generational `WidgetArena`.
- **Reactive Graph Telemetry**: Tracks signal-to-memo dependency edges, detecting unnecessary cascade recalculations and scheduling bottlenecks.
- **Profiling Exporters**: Seamless export of GPU compute timestamps to Tracy and Chrome Tracing (`about:tracing`) format.
- **100% Safe Rust**: `#![forbid(unsafe_code)]` enforced.

---

## Part of Martensite

This crate provides developer tooling and telemetry for the [Martensite](https://github.com/jxoesneon/martensite) GUI framework. For top-level widgets and application integration, see [`martensite`](https://crates.io/crates/martensite).

---

## License

Licensed under either of:

- Apache License, Version 2.0 ([LICENSE-APACHE](https://github.com/jxoesneon/martensite/blob/main/LICENSE-APACHE) or <http://www.apache.org/licenses/LICENSE-2.0>)
- MIT license ([LICENSE-MIT](https://github.com/jxoesneon/martensite/blob/main/LICENSE-MIT) or <http://opensource.org/licenses/MIT>)

at your option.
