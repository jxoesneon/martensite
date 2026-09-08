# martensite-assets

[![Crates.io](https://img.shields.io/crates/v/martensite-assets.svg)](https://crates.io/crates/martensite-assets)
[![Documentation](https://docs.rs/martensite-assets/badge.svg)](https://docs.rs/martensite-assets)
[![License](https://img.shields.io/badge/license-MIT%20OR%20Apache--2.0-blue.svg)](https://github.com/jxoesneon/martensite#license)

> **Dual-mode Virtual File System (VFS) and Ahead-Of-Time (AOT) WGSL shader validation and reflection via Naga.**

---

## Overview

`martensite-assets` provides asset virtualization and shader reflection for the Martensite GUI framework. It consists of two cooperating subsystems:

1. **Dual-Mode Virtual File System (`vfs`)**:
   - *Development*: A disk-backed VFS (`DiskVfs`) that monitors the filesystem for live asset hot-reloading.
   - *Release*: An embedded, zero-copy VFS (`EmbeddedVfs`) that bundles images, fonts, localization catalogs, and shaders directly into the final application binary.
2. **AOT WGSL Shader Validator (`shader`)**:
   - Validates WebGPU Shading Language (WGSL) source code ahead-of-time using [naga](https://crates.io/crates/naga).
   - Catches syntax, binding mismatch, and pipeline incompatibility errors before GPU device submission.
   - Automatically extracts reflection metadata (bind groups, uniform buffers, and entry points).

The entire crate is built under `#![forbid(unsafe_code)]`.

---

## Key Features

- **Zero-Copy Embedded Assets (`EmbeddedVfs`)**: Resolves static byte slices baked into application binaries without runtime filesystem dependencies.
- **Development Disk VFS (`DiskVfs`)**: Resolves assets from local folders during active development with path sanitization preventing directory traversal attacks.
- **Naga AOT Shader Validation (`ShaderValidator`)**: Compiles and checks WGSL compute and raster pipelines before deployment.
- **Automatic Pipeline Reflection (`ShaderReflection`)**: Extracts entry points, shader stages (`Vertex`, `Fragment`, `Compute`), and binding configurations directly from shader source.
- **100% Safe Rust**: `#![forbid(unsafe_code)]` enforced.

---

## Quick Start

Add `martensite-assets` to your `Cargo.toml`:

```toml
[dependencies]
martensite-assets = "0.7.0"
```

Embedding assets and validating shaders:

```rust
use martensite_assets::shader::ShaderValidator;
use martensite_assets::vfs::{EmbeddedVfs, Vfs};

static ASSETS: &[(&str, &[u8])] = &[(
    "shaders/passthrough.wgsl",
    b"@vertex fn vs_main() -> @builtin(position) vec4<f32> { return vec4<f32>(0.0); }",
)];

fn main() {
    let vfs = EmbeddedVfs::new(ASSETS);

    // Resolve embedded asset
    let source_bytes = vfs.resolve("shaders/passthrough.wgsl").expect("shader exists");
    let source_str = std::str::from_utf8(source_bytes).expect("valid utf-8");

    // Validate WGSL and reflect entry points
    let mut validator = ShaderValidator::new();
    let reflection = validator.validate(source_str).expect("valid wgsl");

    assert_eq!(reflection.entry_points.len(), 1);
    assert_eq!(reflection.entry_points[0].name, "vs_main");
}
```

---

## Cargo Feature Flags

| Feature | Description | Default |
| :--- | :--- | :--- |
| `default` | Standard asset pipeline including `disk` support. | Yes |
| `disk` | Enables disk-backed filesystem access (`DiskVfs`) using `std::fs`. | Yes |

---

## Part of Martensite

This crate provides asset management and shader tools for the [Martensite](https://github.com/jxoesneon/martensite) GUI framework. For top-level widgets and application integration, see [`martensite`](https://crates.io/crates/martensite).

---

## License

Licensed under either of:

- Apache License, Version 2.0 ([LICENSE-APACHE](https://github.com/jxoesneon/martensite/blob/main/LICENSE-APACHE) or <http://www.apache.org/licenses/LICENSE-2.0>)
- MIT license ([LICENSE-MIT](https://github.com/jxoesneon/martensite/blob/main/LICENSE-MIT) or <http://opensource.org/licenses/MIT>)

at your option.
