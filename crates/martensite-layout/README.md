# martensite-layout

[![Crates.io](https://img.shields.io/crates/v/martensite-layout.svg)](https://crates.io/crates/martensite-layout)
[![Documentation](https://docs.rs/martensite-layout/badge.svg)](https://docs.rs/martensite-layout)
[![License](https://img.shields.io/badge/license-MIT%20OR%20Apache--2.0-blue.svg)](https://github.com/jxoesneon/martensite#license)

> **W3C CSS Flexbox, Grid, and Block layout bridge via Taffy with a two-pass constraint-solving engine.**

---

## Overview

`martensite-layout` bridges the Martensite `WidgetArena` to the [Taffy](https://crates.io/crates/taffy) layout library, providing a high-performance, specification-compliant two-pass layout solver.

By decoupling **intrinsic measurement** (Pass 1: min/max content probing under layout constraints) from **spatial placement** (Pass 2: bounding box assignment and alignment), the engine completely prevents single-frame layout oscillation and feedback cycles.

The entire crate is built under `#![forbid(unsafe_code)]`.

---

## Key Features

- **W3C Standards Compliance**: Full support for CSS Flexbox, CSS Grid, and Block layout models powered by Taffy.
- **Arena Bridge (`ArenaBridge`)**: Adapts Martensite's generational `WidgetArena` to Taffy's `TraversePartialTree` trait without duplicating tree structures in memory.
- **Two-Pass Solver (`LayoutEngine`)**:
  - *Pass 1 (Measure)*: Solves intrinsic sizing and child constraint probing with cache-friendly pruning.
  - *Pass 2 (Layout)*: Resolves final relative and absolute coordinates, computing tight `Rect` boundaries for every active widget node.
- **Geometry Primitives**: First-class spatial types including `Point`, `Size`, `Constraints`, and `EdgeInsets` (with `uniform`, `symmetric`, and `new` constructors).
- **Two-Level Layout Compatibility**: Solves arena-level top-level widget boundaries while providing sub-constraints for compound internal widgets (`Flex`, `Container`, `Stack`).

---

## Quick Start

Add `martensite-layout` to your `Cargo.toml`:

```toml
[dependencies]
martensite-layout = "0.7.0"
```

Computing layout geometry:

```rust
use martensite_layout::geometry::{Constraints, EdgeInsets, Size};
use martensite_layout::LayoutEngine;

fn main() {
    let mut engine = LayoutEngine::new();

    // Configure layout constraints
    let constraints = Constraints::loose(Size::new(800.0, 600.0));
    let padding = EdgeInsets::uniform(16.0);

    assert_eq!(padding.top, 16.0);
    assert_eq!(padding.horizontal(), 32.0);

    // Compute layout for registered node hierarchy
    println!("Engine ready with {} active nodes", engine.node_count());
}
```

---

## Part of Martensite

This crate is the spatial layout engine for the [Martensite](https://github.com/jxoesneon/martensite) GUI framework. For top-level widgets and application tools, see [`martensite`](https://crates.io/crates/martensite).

---

## License

Licensed under either of:

- Apache License, Version 2.0 ([LICENSE-APACHE](https://github.com/jxoesneon/martensite/blob/main/LICENSE-APACHE) or <http://www.apache.org/licenses/LICENSE-2.0>)
- MIT license ([LICENSE-MIT](https://github.com/jxoesneon/martensite/blob/main/LICENSE-MIT) or <http://opensource.org/licenses/MIT>)

at your option.
