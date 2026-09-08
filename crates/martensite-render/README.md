# martensite-render

[![Crates.io](https://img.shields.io/crates/v/martensite-render.svg)](https://crates.io/crates/martensite-render)
[![Documentation](https://docs.rs/martensite-render/badge.svg)](https://docs.rs/martensite-render)
[![License](https://img.shields.io/badge/license-MIT%20OR%20Apache--2.0-blue.svg)](https://github.com/jxoesneon/martensite#license)

> **Hardware-agnostic PaintList command stream with TinySkia CPU SIMD fallback and Vello GPU backends.**

---

## Overview

`martensite-render` decouples visual command generation from concrete graphics hardware. Widget layout passes emit an intermediate, serializable **`PaintList`** command stream containing paths, gradients, glyph runs, clips, and transforms.

This command stream can be consumed by multiple backends implementing the `RenderBackend` trait:

1. **`VelloRenderer` (GPU Compute)**: Translates the command stream directly into a GPU-evaluated Vello scene for high-performance rasterization.
2. **`TinySkiaBackend` (CPU SIMD)**: Pure-Rust CPU rasterizer using AVX2/NEON vector intrinsics for software fallback, headless CI rendering, and automated reftest comparison.

---

## Key Features

- **Intermediate Display List (`PaintList`)**: Record-and-replay drawing pipeline supporting fills, strokes, linear/radial gradients, glyph runs, and rounded rectangle clipping.
- **Pure-Rust Software Fallback (`TinySkiaBackend`)**: Enables Martensite applications to run on headless servers, CI runners, and machines without GPU drivers.
- **Window Presentation (`SoftbufferPresenter`)**: Blits CPU-rasterized RGBA framebuffers directly to window surfaces using the `softbuffer` protocol.
- **Visual Regression Diffing (`diff`)**: Built-in perceptual image diffing algorithm for pixel-perfect reftest verification across rendering backends.
- **100% Safe Rust**: `#![forbid(unsafe_code)]` enforced across all command parsing and presentation routines.

---

## Quick Start

Add `martensite-render` to your `Cargo.toml`:

```toml
[dependencies]
martensite-render = "0.7.0"
```

Recording draw commands and rasterizing to a software buffer:

```rust
use martensite_render::{PaintCommand, PaintList, RenderBackend, TinySkiaBackend};
use martensite_render::kurbo::Rect;

fn main() {
    let mut paint_list = PaintList::new();

    // Record draw operations
    paint_list.push(PaintCommand::FillRect {
        rect: Rect::new(0.0, 0.0, 400.0, 300.0),
        color: [0.1, 0.2, 0.3, 1.0],
    });

    // Render using CPU SIMD backend
    let mut backend = TinySkiaBackend::new(400, 300);
    backend.render(&paint_list);

    let pixels = backend.rgba_data();
    assert_eq!(pixels.len(), 400 * 300 * 4);
}
```

---

## Cargo Feature Flags

| Feature | Description | Default |
| :--- | :--- | :--- |
| `default` | Standard software rendering via `TinySkiaBackend`. | Yes |
| `vello` | Enables GPU compute vector rendering via `VelloRenderer`. | Yes |

---

## Part of Martensite

This crate provides the intermediate rendering architecture for the [Martensite](https://github.com/jxoesneon/martensite) GUI framework. For top-level widgets and application integration, see [`martensite`](https://crates.io/crates/martensite).

---

## License

Licensed under either of:

- Apache License, Version 2.0 ([LICENSE-APACHE](https://github.com/jxoesneon/martensite/blob/main/LICENSE-APACHE) or <http://www.apache.org/licenses/LICENSE-2.0>)
- MIT license ([LICENSE-MIT](https://github.com/jxoesneon/martensite/blob/main/LICENSE-MIT) or <http://opensource.org/licenses/MIT>)

at your option.
