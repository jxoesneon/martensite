# martensite-focus

[![Crates.io](https://img.shields.io/crates/v/martensite-focus.svg)](https://crates.io/crates/martensite-focus)
[![Documentation](https://docs.rs/martensite-focus/badge.svg)](https://docs.rs/martensite-focus)
[![License](https://img.shields.io/badge/license-MIT%20OR%20Apache--2.0-blue.svg)](https://github.com/jxoesneon/martensite#license)

> **2D spatial projected-beam navigation, linear Tab cycling, and modal focus traps for Martensite.**

---

## Overview

`martensite-focus` provides keyboard, gamepad, and remote-control focus navigation for the Martensite GUI framework. Unlike simple 1D linear tab sequences, technical software and entertainment interfaces often arrange widgets in 2D irregular grids, splitters, and canvas layers.

`martensite-focus` introduces a **projected-beam 2D directional navigation algorithm** (`SpatialNavigator`) that casts a directional cone across the layout to score and select optimal candidate nodes when pressing arrow keys or directional pads. It also provides a **modal focus trap stack** (`FocusScopeStack`) that confines tab navigation within active dialogs and restores previous focus upon dismissal.

The entire crate is built under `#![forbid(unsafe_code)]`.

---

## Key Features

- **2D Projected-Beam Spatial Navigation**:
  - Casts an angular forward projection cone (`FORWARD_CONE_DEGREES = 60°`) from the current focused element's `Rect`.
  - Calculates candidate fitness using distance and angular alignment penalties (`DEFAULT_ALPHA`, `DEFAULT_BETA`).
  - Supports `FocusDirection::Up`, `FocusDirection::Down`, `FocusDirection::Left`, and `FocusDirection::Right`.
- **Linear Tab Traversal (`TabNavigation`)**: Predictable depth-first preorder tab cycling respecting `NodeFlags::FOCUSABLE`.
- **Modal Focus Traps (`FocusScopeStack`)**:
  - Traps keyboard focus within popups, context menus, and modal dialogs.
  - Automatically restores focus to the preceding element upon modal closure.
- **Focus Ring Styling**: Computes active focus rectangles for rendering high-contrast accessibility focus indicators.

---

## Quick Start

Add `martensite-focus` to your `Cargo.toml`:

```toml
[dependencies]
martensite-focus = "0.7.0"
```

Configuring spatial navigation:

```rust
use martensite_focus::{FocusDirection, SpatialNavigator};
use martensite_core::Rect;

fn main() {
    let navigator = SpatialNavigator::new();

    let current_bounds = Rect::from_xywh(100.0, 100.0, 80.0, 32.0);
    let candidate_right = Rect::from_xywh(220.0, 100.0, 80.0, 32.0);

    // Score candidate fitness along the Right direction vector
    let score = navigator.score_candidate(current_bounds, candidate_right, FocusDirection::Right);
    println!("Candidate fitness score: {:?}", score);
}
```

---

## Part of Martensite

This crate provides keyboard and spatial navigation for the [Martensite](https://github.com/jxoesneon/martensite) GUI framework. For top-level widgets and application integration, see [`martensite`](https://crates.io/crates/martensite).

---

## License

Licensed under either of:

- Apache License, Version 2.0 ([LICENSE-APACHE](https://github.com/jxoesneon/martensite/blob/main/LICENSE-APACHE) or <http://www.apache.org/licenses/LICENSE-2.0>)
- MIT license ([LICENSE-MIT](https://github.com/jxoesneon/martensite/blob/main/LICENSE-MIT) or <http://opensource.org/licenses/MIT>)

at your option.
