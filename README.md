# Martensite

[![Crates.io](https://img.shields.io/crates/v/martensite.svg)](https://crates.io/crates/martensite)
[![Documentation](https://docs.rs/martensite/badge.svg)](https://docs.rs/martensite)
[![License](https://img.shields.io/badge/license-MIT%20OR%20Apache--2.0-blue.svg)](LICENSE)
[![Pure Rust](https://img.shields.io/badge/supply--chain-100%25%20Pure%20Rust-brightgreen.svg)](deny.toml)

> **A retained-mode, GPU-accelerated graphical user interface framework for Rust.**

---

## What is Martensite?

Named after the crystalline phase of hardened steel, **Martensite** is a pure-Rust, retained-mode GUI framework designed for desktop applications and technical software.

The framework provides direct GPU compute rendering via WGPU, a fine-grained push-pull reactive state model, and an arena-based widget hierarchy. It is architected for predictable latency, low idle resource consumption, and straightforward cross-platform deployment using standard Rust toolchains.

---

## Core Architectural Principles

1. **Direct GPU Rendering**: Renders interface elements directly through hardware compute pipelines (`Vello` / `wgpu`) rather than wrapping host platform controls.
2. **Bounded Memory & Zero Runtime GC**: No garbage collection or virtual machines; zero dynamic heap allocations in the active interaction loop.
3. **Event-Driven Sleep**: When interface state is static and no inputs arrive, the event loop yields to kernel wait states.
4. **Arena-Based Storage**: Widgets reside in a generational slotmap; references are lightweight 64-bit copyable handles (`WidgetId`) without interior mutability wrappers.
5. **Fine-Grained Reactive Signals**: State mutations update dirty bitsets directly on affected leaf nodes without virtual DOM diffing.
6. **Pure-Rust Toolchain**: 100% idiomatic Rust compiling cleanly with standard `rustc` and `cargo` without external C/C++ dependencies.
7. **Two-Pass Layout Geometry**: Intrinsic measurement is decoupled from placement, preventing single-frame layout oscillations.
8. **Integrated Accessibility**: Day-one native screen-reader synchronization through AccessKit.
9. **Comprehensive Typography**: HarfBuzz shaping, bidirectional text (Unicode UAX #9), and font fallback via `cosmic-text`.
10. **Permissive Open-Source Licensing**: Dual-licensed under MIT and Apache 2.0.

---

## Workspace Crate Topology

| Crate | Purpose |
| :--- | :--- |
| [`martensite`](crates/martensite) | Facade crate re-exporting the unified prelude. |
| [`martensite-core`](crates/martensite-core) | Generational slotmap arena, 64-byte node memory, and the `Widget` trait. |
| [`martensite-reactive`](crates/martensite-reactive) | Push-pull reactive signal DAG (`Signal<T>`, `Memo<T>`, `Transactional<T>`). |
| [`martensite-layout`](crates/martensite-layout) | W3C CSS Flexbox, Grid, and Block layout integration via Taffy. |
| [`martensite-wgpu`](crates/martensite-wgpu) | Vello GPU compute rasterizer, in-register tile blending, and device loss recovery. |
| [`martensite-render`](crates/martensite-render) | Intermediate `PaintList` command stream and pure-Rust `TinySkiaBackend` CPU SIMD fallback. |
| [`martensite-text`](crates/martensite-text) | Cosmic-Text HarfBuzz shaping, BiDi, and IME candidate window coordinate projection. |
| [`martensite-access`](crates/martensite-access) | Incremental `TreeUpdate` adapter for AccessKit (Windows UIA, macOS NSAccessibility, Linux AT-SPI2). |
| [`martensite-window`](crates/martensite-window) | Winit multi-window management, per-monitor DPI scaling, and presentation control. |
| [`martensite-focus`](crates/martensite-focus) | 2D spatial directional keyboard navigation, modal focus traps, and focus indicator styling. |
| [`martensite-clipboard`](crates/martensite-clipboard) | Multi-format delayed rendering clipboard engine (Windows OLE, macOS Cocoa, Linux Wayland/X11). |
| [`martensite-dnd`](crates/martensite-dnd) | Unified scenegraph reordering and platform drag-and-drop subsystem. |
| [`martensite-theme`](crates/martensite-theme) | Semantic design tokens, Oklab perceptual uniform GPU blending, and spring transitions. |
| [`martensite-motion`](crates/martensite-motion) | Analytical damped harmonic oscillator spring physics solver ($C^1$ velocity continuity). |
| [`martensite-media`](crates/martensite-media) | Hardware media surface passthrough (DXGI, IOSurface, dma-buf) and external 3D interop. |
| [`martensite-history`](crates/martensite-history) | Transactional signal journal, input event coalescing, and non-linear undo tree. |
| [`martensite-assets`](crates/martensite-assets) | Virtual File System (VFS), AOT naga shader validation, and dynamic texture streaming. |
| [`martensite-l10n`](crates/martensite-l10n) | Localization and BiDi mirroring via Project Fluent. |
| [`martensite-devtools`](crates/martensite-devtools) | Tracing spans, GPU profiling timestamps, and in-app developer diagnostics overlay. |
| [`martensite-macros`](crates/martensite-macros) | Declarative procedural widget construction macros. |
| [`martensite-test`](crates/martensite-test) | Headless CI mock windowing, deterministic virtual clock, and image diff testing. |
| [`cargo-martensite`](tools/cargo-martensite) | Developer CLI toolchain for hot-reloading and asset packaging. |

---

## Quick Start

```rust
use martensite::prelude::*;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    App::build()
        .title("Martensite Application")
        .size(1280.0, 800.0)
        .run(|cx| {
            let counter = cx.signal(0);

            column()
                .padding(24.0)
                .gap(16.0)
                .child(
                    text(cx.memo(move || format!("Count: {}", counter.get())))
                        .size(24.0)
                        .weight(FontWeight::Bold)
                )
                .child(
                    button("Increment")
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
