# martensite-theme

[![Crates.io](https://img.shields.io/crates/v/martensite-theme.svg)](https://crates.io/crates/martensite-theme)
[![Documentation](https://docs.rs/martensite-theme/badge.svg)](https://docs.rs/martensite-theme)
[![License](https://img.shields.io/badge/license-MIT%20OR%20Apache--2.0-blue.svg)](https://github.com/jxoesneon/martensite#license)

> **Semantic design tokens, Oklab perceptual color space blending, and GPU theme-transition shaders for Martensite.**

---

## Overview

`martensite-theme` provides the styling and color pipeline for the Martensite GUI framework. Traditional sRGB color interpolation suffers from notorious perceptual non-linearities (such as muddy brown transitions between red and green, or perceived brightness spikes).

`martensite-theme` performs all gradient math, palette generation, and theme cross-fades in **Oklab** and **Oklch** perceptual color spaces. It incorporates built-in WCAG 2.1 and APCA contrast verification algorithms alongside a dedicated GPU transition shader (`THEME_TRANSITION_WGSL`) for 60/120 FPS light-to-dark mode transitions.

The entire crate is built under `#![forbid(unsafe_code)]`.

---

## Key Features

- **Perceptually Uniform Color (`Oklab`, `Oklch`)**:
  - Consistent perceived lightness and chroma throughout color animations and gradients.
  - Smooth gamut mapping (`gamut_map`, `Gamut`) to keep out-of-gamut wide-gamut colors within sRGB/Display-P3 display capabilities.
- **Accessibility Contrast Calculators**:
  - `wcag_contrast`: Standard WCAG 2.1 relative luminance ratio verification (AA 4.5:1, AAA 7:1).
  - `apca_contrast`: Advanced Perceptual Contrast Algorithm (APCA) scoring for modern readability standards.
- **Semantic Design Tokens (`Theme`, `ThemeToken`)**:
  - Structured token dictionary with hierarchical scoping and runtime theme diffing (`ThemeDiff`).
  - Supports light, dark, and high-contrast accessibility modes (`ThemeMode`).
- **GPU Transition Shaders (`gpu_transition`)**: Uniform buffer generation (`ThemeUniformBuffer`) driving instant or spring-eased palette cross-fades directly in GPU compute/fragment shaders.

---

## Quick Start

Add `martensite-theme` to your `Cargo.toml`:

```toml
[dependencies]
martensite-theme = "0.7.0"
```

Computing perceptual contrast in Oklab:

```rust
use martensite_theme::{Oklab, wcag_contrast};

fn main() {
    let background = Oklab { l: 0.95, a: 0.0, b: 0.0, alpha: 1.0 }; // Light gray
    let text = Oklab { l: 0.20, a: 0.0, b: 0.0, alpha: 1.0 };       // Dark gray

    let ratio = wcag_contrast(background, text);
    println!("Contrast ratio: {:.2}:1", ratio);
    assert!(ratio >= 4.5); // Conforms to WCAG AA
}
```

---

## Part of Martensite

This crate provides styling and design tokens for the [Martensite](https://github.com/jxoesneon/martensite) GUI framework. For top-level widgets and application integration, see [`martensite`](https://crates.io/crates/martensite).

---

## License

Licensed under either of:

- Apache License, Version 2.0 ([LICENSE-APACHE](https://github.com/jxoesneon/martensite/blob/main/LICENSE-APACHE) or <http://www.apache.org/licenses/LICENSE-2.0>)
- MIT license ([LICENSE-MIT](https://github.com/jxoesneon/martensite/blob/main/LICENSE-MIT) or <http://opensource.org/licenses/MIT>)

at your option.
