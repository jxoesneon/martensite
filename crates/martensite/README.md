# martensite

[![Crates.io](https://img.shields.io/crates/v/martensite.svg)](https://crates.io/crates/martensite)
[![Documentation](https://docs.rs/martensite/badge.svg)](https://docs.rs/martensite)
[![License](https://img.shields.io/badge/license-MIT%20OR%20Apache--2.0-blue.svg)](https://github.com/jxoesneon/martensite#license)

> **The primary facade and foundational widget library for the Martensite GUI framework.**

---

## Overview

`martensite` is the primary entry point to the Martensite GUI framework. It re-exports the unified `martensite::prelude` alongside the foundational widget library (`Button`, `CheckBox`, `Container`, `Flex`, `Stack`, `Text`, and `TextInput`) and top-level application orchestration (`App`, `AppBuilder`, `AppConfig`).

Whether building rich desktop technical tools or lightweight reactive interfaces, `martensite` brings together hardware-accelerated compute rendering, fine-grained push-pull reactive state, and an arena-based widget hierarchy under a single cohesive crate.

---

## Key Features

- **Unified Prelude (`martensite::prelude::*`)**: One import provides access to core arena types (`WidgetArena`, `WidgetId`, `HotNode`, `ColdNode`), reactive primitives (`Signal`, `Memo`), layout types (`Rect`), motion solvers (`SpringConfig`, `SpringSolver`), color tokens (`Oklab`), and the foundational widgets.
- **Foundational Widget Suite**:
  - `Button`: Interactive push button with click and focus actions and accessible roles.
  - `CheckBox`: Toggleable stateful checkbox with custom styling and accessible toggle states.
  - `Container`: Padding, background color styling, and child positioning primitive.
  - `Flex`: Multi-child row and column flexbox layout with gap distribution and alignment.
  - `Stack`: Z-ordered layering of overlapping child widgets with configurable anchoring.
  - `Text`: High-performance shaped text with two-tier glyph caching and Unicode BiDi support.
  - `TextInput`: Accessible editable text entry with placeholder support and focus management.
- **Two-Level Layout Architecture**:
  1. *Arena-level layout* handled by Taffy via `LayoutEngine` for top-level widget bounds.
  2. *Widget-internal layout* allowing compound widgets (`Flex`, `Container`, `Stack`) to deterministically manage internal children without bloating the global arena.
- **Self-Healing Rendering Pipeline**: Configurable via `AppBuilder` to transparently fall back from Vello GPU compute to TinySkia CPU SIMD rasterization upon GPU device loss or headless virtualization.

---

## Quick Start

Add `martensite` to your `Cargo.toml`:

```toml
[dependencies]
martensite = "0.7.0"
```

Create a reactive interface:

```rust
use martensite::prelude::*;

fn main() {
    // Configure application behavior and fallback tolerances
    let app = App::build()
        .allow_software_fallback(true)
        .build();

    println!("Fallback enabled: {}", app.allow_software_fallback());

    // Construct foundational widget hierarchy
    let button = Button::new("Increment")
        .enabled(true);

    let view = Flex::column()
        .gap(12.0)
        .child(Text::new("Counter Application").font_size(20.0))
        .child(button);

    assert_eq!(view.child_count(), 2);
}
```

---

## Cargo Feature Flags

| Feature | Description | Default |
| :--- | :--- | :--- |
| `default` | Standard Martensite feature set including full rendering and windowing pipelines. | Yes |
| `docs-rs` | Enables documentation build optimizations for docs.rs. | No |

---

## Workspace Subsystem Crates

`martensite` re-exports the individual specialized crates that make up the framework:

- [`martensite-core`](https://crates.io/crates/martensite-core) — Generational slotmap arena, 64-byte `HotNode` layout, and the `Widget` trait.
- [`martensite-reactive`](https://crates.io/crates/martensite-reactive) — Fine-grained push-pull reactive signal DAG (`Signal<T>`, `Memo<T>`).
- [`martensite-layout`](https://crates.io/crates/martensite-layout) — W3C CSS Flexbox, Grid, and Block layout engine via Taffy.
- [`martensite-wgpu`](https://crates.io/crates/martensite-wgpu) — Vello GPU compute rasterizer and device loss recovery.
- [`martensite-render`](https://crates.io/crates/martensite-render) — `PaintList` command stream and pure-Rust TinySkia CPU fallback.
- [`martensite-text`](https://crates.io/crates/martensite-text) — HarfBuzz shaping, BiDi, and font fallback via `cosmic-text`.
- [`martensite-access`](https://crates.io/crates/martensite-access) — Native OS accessibility adapter via AccessKit.
- [`martensite-motion`](https://crates.io/crates/martensite-motion) — Analytical damped harmonic spring physics solver.
- [`martensite-theme`](https://crates.io/crates/martensite-theme) — Semantic design tokens and perceptual Oklab color blending.

---

## License

Licensed under either of:

- Apache License, Version 2.0 ([LICENSE-APACHE](https://github.com/jxoesneon/martensite/blob/main/LICENSE-APACHE) or <http://www.apache.org/licenses/LICENSE-2.0>)
- MIT license ([LICENSE-MIT](https://github.com/jxoesneon/martensite/blob/main/LICENSE-MIT) or <http://opensource.org/licenses/MIT>)

at your option.
