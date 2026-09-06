# Martensite

[![Crates.io](https://img.shields.io/crates/v/martensite.svg)](https://crates.io/crates/martensite)
[![Documentation](https://docs.rs/martensite/badge.svg)](https://docs.rs/martensite)
[![License](https://img.shields.io/badge/license-MIT%20OR%20Apache--2.0-blue.svg)](LICENSE)
[![Pure Rust](https://img.shields.io/badge/supply--chain-100%25%20Pure%20Rust-brightgreen.svg)](deny.toml)

> **The sovereign, retained-mode, GPU-accelerated graphical user interface engine for Rust.**

---

## What is Martensite?

Named after the hardest, most resilient crystalline phase of hardened steel—formed by an instantaneous, diffusionless shear transformation—**Martensite** is engineered from first principles to permanently close the ten-year void in Rust client application development.

Martensite is a pure-Rust, retained-mode, compute-driven workstation GUI framework. It rejects the immediate-mode battery drain of `egui`, the monolithic message enum explosion of `iced`, the foreign domain-specific languages and commercial dual-licensing of `slint`, and the multi-process memory bloat of webview wrappers like `tauri`.

---

## The Ten Golden Laws of Martensite

1. **The Pixel Sovereignty Law**: Render every pixel directly through WGPU compute. Never wrap host OS widgets.
2. **The Zero-GC Law**: No garbage collection, no virtual machines, zero allocations in the active interaction loop.
3. **The Event-Sleep Law**: When state is static and no inputs arrive, CPU and GPU utilization must be strictly **0.00%**.
4. **The Single-Tree Arena Law**: All widgets reside within a flat Generational SlotMap. References are 64-bit copyable handles. No `Rc<RefCell<T>>`.
5. **The Zero-VDOM Signal Law**: State mutations update dirty bitsets on exact leaf nodes in $O(1)$ time without Virtual DOM diffing.
6. **The Pure-Rust Homogeneity Law**: 100% compile-time verified, idiomatic Rust. Zero foreign DSLs, zero XML, zero preprocessors.
7. **The Two-Pass Geometry Law**: Layout measurement is strictly decoupled from placement. Single-frame visual lag is mathematically forbidden.
8. **The Accessibility-First Law**: Native AccessKit screen-reader synchronization is a Day-Zero primitive.
9. **The World Typography Law**: Universal HarfBuzz text shaping, BiDi (Unicode UAX #9), and font fallback via `cosmic-text`. Zero tofu glyphs.
10. **The Permissive Freedom Law**: Permanent dual-licensing under MIT and Apache 2.0 in perpetuity.

---

## Workspace Crate Topology

| Crate | Purpose |
| :--- | :--- |
| [`martensite`](crates/martensite) | Sovereign facade crate re-exporting the unified prelude. |
| [`martensite-core`](crates/martensite-core) | Generational SlotMap arena, Hot/Cold 64-byte node memory, and the object-safe `Widget` trait. |
| [`martensite-reactive`](crates/martensite-reactive) | Diffusionless push-pull signal DAG (`Signal<T>`, `Memo<T>`, `Transactional<T>`). |
| [`martensite-layout`](crates/martensite-layout) | W3C CSS Flexbox, Grid, and Block layout bridge via Taffy. |
| [`martensite-wgpu`](crates/martensite-wgpu) | Vello GPU compute rasterizer, in-register tile blending, and <16ms single-frame resurrection. |
| [`martensite-render`](crates/martensite-render) | Intermediate `PaintList` command stream and pure-Rust `TinySkiaBackend` CPU SIMD fallback. |
| [`martensite-text`](crates/martensite-text) | Cosmic-Text HarfBuzz shaping, BiDi, and sub-pixel IME candidate window coordinate projection. |
| [`martensite-access`](crates/martensite-access) | Real-time incremental `TreeUpdate` adapter for AccessKit (Windows UIA, macOS NSAccessibility, Linux AT-SPI2). |
| [`martensite-window`](crates/martensite-window) | Winit multi-window management, per-monitor fractional DPI scaling, and decoupled VSync. |
| [`martensite-focus`](crates/martensite-focus) | 2D spatial projected-beam keyboard navigation, modal focus traps, and SDF focus rings. |
| [`martensite-clipboard`](crates/martensite-clipboard) | Multi-MIME delayed rendering clipboard engine (Windows OLE, macOS Cocoa, Linux Wayland/X11). |
| [`martensite-dnd`](crates/martensite-dnd) | Unified internal scenegraph reordering and cross-OS native drag-and-drop subsystem. |
| [`martensite-theme`](crates/martensite-theme) | Semantic design tokens, Oklab perceptual uniform GPU blending, and 150ms spring transitions. |
| [`martensite-motion`](crates/martensite-motion) | Closed-form analytical damped harmonic oscillator spring physics solver ($C^1$ velocity continuity). |
| [`martensite-media`](crates/martensite-media) | Zero-copy hardware media surfaces (DXGI, IOSurface, dma-buf) and external Bevy/CAD 3D interop. |
| [`martensite-history`](crates/martensite-history) | Transactional signal journal, 120Hz gesture coalescing, and non-linear LCA branching undo tree. |
| [`martensite-assets`](crates/martensite-assets) | Dual-mode Virtual File System (VFS), AOT naga shader validation, and dynamic texture streaming. |
| [`martensite-l10n`](crates/martensite-l10n) | Natural language grammatical localization and BiDi mirroring via Project Fluent. |
| [`martensite-devtools`](crates/martensite-devtools) | Tracing spans, Tracy/Chrome GPU timestamps, and in-app F12 developer HUD overlay. |
| [`martensite-macros`](crates/martensite-macros) | Declarative procedural widget construction macros. |
| [`martensite-test`](crates/martensite-test) | Headless CI mock windowing, deterministic virtual clock, and perceptual diffing test harness. |
| [`cargo-martensite`](tools/cargo-martensite) | Developer CLI toolchain for sub-second hot-reloading and asset packaging. |

---

## Quick Start

```rust
use martensite::prelude::*;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    App::build()
        .title("Martensite Industrial Workstation")
        .size(1280.0, 800.0)
        .run(|cx| {
            let counter = cx.signal(0);

            column()
                .padding(24.0)
                .gap(16.0)
                .child(
                    text(cx.memo(move || format!("Quenched State: {}", counter.get())))
                        .size(24.0)
                        .weight(FontWeight::Bold)
                )
                .child(
                    button("Transform Phase")
                        .padding(12.0)
                        .on_click(move |_| counter.update(|c| *c + 1))
                )
        })
}
```

## Licensing

Martensite is dual-licensed under either:

- **MIT License** ([LICENSE-MIT](LICENSE-MIT))
- **Apache License, Version 2.0** ([LICENSE-APACHE](LICENSE-APACHE))

at your option.
