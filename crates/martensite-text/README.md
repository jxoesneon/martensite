# martensite-text

[![Crates.io](https://img.shields.io/crates/v/martensite-text.svg)](https://crates.io/crates/martensite-text)
[![Documentation](https://docs.rs/martensite-text/badge.svg)](https://docs.rs/martensite-text)
[![License](https://img.shields.io/badge/license-MIT%20OR%20Apache--2.0-blue.svg)](https://github.com/jxoesneon/martensite#license)

> **HarfBuzz text shaping, Unicode BiDi, two-tier caching, and velocity-damped IME candidate positioning.**

---

## Overview

`martensite-text` provides the internationalized text layout and typography engine for the Martensite GUI framework. Built on top of [cosmic-text](https://crates.io/crates/cosmic-text) and HarfBuzz, it solves complex text shaping, multi-script font fallback, Unicode bidirectional text (UAX #9), and line breaking.

To sustain 120 FPS interactive window resizing across thousands of text nodes, `martensite-text` implements a **two-tier caching architecture** alongside a **kinetic IME positioner** that prevents Input Method candidate popups from flickering or lagging during active container scrolling.

The entire crate is built under `#![forbid(unsafe_code)]`.

---

## Key Features

- **Complex Text Shaping & BiDi**: Seamlessly shapes Arabic, Hebrew, Devanagari, CJK, and Latin scripts with automatic bidirectional reordering and font fallback.
- **Two-Tier Text Caching**:
  - *Tier 1 (Inline)*: Stored in each node's `ColdNode` to absorb repeated min/max constraint probes during two-pass flexbox layout without global lock contention.
  - *Tier 2 (Global LRU)*: Bounded 16 MB memory budget (`TextShapeCache`) caching fully shaped glyph runs across identical text strings and style attributes.
- **Velocity-Damped IME Positioning (`ImePositioner`)**: Projects caret positions and dampens velocity during rapid mousewheel scrolling, ensuring CJK/IME candidate windows dock smoothly without visual detachment.
- **Font Management (`FontManager`)**: System font discovery via `fontdb`, system font collection querying, and zero-copy custom font asset loading.
- **100% Safe Rust**: `#![forbid(unsafe_code)]` enforced across all text buffers and shaping routines.

---

## Quick Start

Add `martensite-text` to your `Cargo.toml`:

```toml
[dependencies]
martensite-text = "0.7.0"
```

Measuring and shaping text:

```rust
use martensite_text::{FontManager, TextShapeCache, measure_text};

fn main() {
    let mut font_manager = FontManager::new();
    let mut cache = TextShapeCache::new(16 * 1024 * 1024); // 16 MB budget

    let metrics = measure_text(
        &mut font_manager,
        &mut cache,
        "Hello, Martensite!",
        16.0,
        None, // natural line height
        None, // unconstrained width
    );

    println!("Measured width: {}, height: {}", metrics.width, metrics.height);
}
```

---

## Two-Tier Cache Architecture

```text
Measure Request (Text, FontSize, MaxWidth)
                 |
                 v
      +----------------------+
      | Tier 1: Inline Cache | -----> [Hit: Return Instant Bounds]
      +----------------------+
                 | (Miss)
                 v
      +----------------------+
      | Tier 2: 16MB LRU     | -----> [Hit: Return Cached ShapeRun]
      +----------------------+
                 | (Miss)
                 v
      +----------------------+
      | Full HarfBuzz Shape  | -----> [Compute, Populate Tiers, Return]
      +----------------------+
```

---

## Part of Martensite

This crate provides typography and text layout for the [Martensite](https://github.com/jxoesneon/martensite) GUI framework. For high-level widgets and application tools, see [`martensite`](https://crates.io/crates/martensite).

---

## License

Licensed under either of:

- Apache License, Version 2.0 ([LICENSE-APACHE](https://github.com/jxoesneon/martensite/blob/main/LICENSE-APACHE) or <http://www.apache.org/licenses/LICENSE-2.0>)
- MIT license ([LICENSE-MIT](https://github.com/jxoesneon/martensite/blob/main/LICENSE-MIT) or <http://opensource.org/licenses/MIT>)

at your option.
