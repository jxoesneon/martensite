# martensite-window

[![Crates.io](https://img.shields.io/crates/v/martensite-window.svg)](https://crates.io/crates/martensite-window)
[![Documentation](https://docs.rs/martensite-window/badge.svg)](https://docs.rs/martensite-window)
[![License](https://img.shields.io/badge/license-MIT%20OR%20Apache--2.0-blue.svg)](https://github.com/jxoesneon/martensite#license)

> **Multi-window management, fractional per-monitor DPI scaling, and normalized pointer event routing via Winit.**

---

## Overview

`martensite-window` manages the operating system windowing lifecycle for the Martensite GUI framework. Built atop [winit](https://crates.io/crates/winit), it coordinates multi-window desktop applications, cross-monitor dynamic DPI transitions, and hardware event normalization.

The crate decouples low-level OS event loops from Martensite's internal scenegraph by providing a dedicated `WindowManager` and `EventRouter` that normalize touch, mouse, stylus, and gesture inputs into unified `PointerEvent` streams.

---

## Key Features

- **Multi-Window Orchestration (`WindowManager`)**: Slotmap-backed registry that manages independent rendering loops, swapchains, and state across multiple top-level OS windows.
- **Fractional DPI Scaling (`DpiScale`)**: Seamless conversions between physical screen coordinates and logical layout points supporting fractional display scale factors (125%, 150%, 175%, 200%).
- **Dynamic Cross-Monitor Migration**: Dynamically recalculates layout constraints and re-tunes text shaping caches when windows move between displays with differing pixel densities.
- **Normalized Event Routing (`EventRouter`)**:
  - Pointer capture (e.g. for sliders, drag-and-drop, and window splitters).
  - Hover state propagation and mouse exit/enter tracking.
  - Keyboard focus and window close lifecycle handling.

---

## Quick Start

Add `martensite-window` to your `Cargo.toml`:

```toml
[dependencies]
martensite-window = "0.7.0"
```

Managing fractional DPI scaling:

```rust
use martensite_window::dpi::DpiScale;

fn main() {
    // 150% fractional display scaling (e.g. Windows / High-DPI Linux)
    let dpi = DpiScale::new(1.5);

    let logical_width = 800.0;
    let physical_width = dpi.to_physical(logical_width);

    assert_eq!(physical_width, 1200.0);
    assert_eq!(dpi.to_logical(physical_width), 800.0);
}
```

---

## Part of Martensite

This crate provides the windowing and input integration for the [Martensite](https://github.com/jxoesneon/martensite) GUI framework. For top-level widgets and application tools, see [`martensite`](https://crates.io/crates/martensite).

---

## License

Licensed under either of:

- Apache License, Version 2.0 ([LICENSE-APACHE](https://github.com/jxoesneon/martensite/blob/main/LICENSE-APACHE) or <http://www.apache.org/licenses/LICENSE-2.0>)
- MIT license ([LICENSE-MIT](https://github.com/jxoesneon/martensite/blob/main/LICENSE-MIT) or <http://opensource.org/licenses/MIT>)

at your option.
