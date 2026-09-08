# martensite-media

[![Crates.io](https://img.shields.io/crates/v/martensite-media.svg)](https://crates.io/crates/martensite-media)
[![Documentation](https://docs.rs/martensite-media/badge.svg)](https://docs.rs/martensite-media)
[![License](https://img.shields.io/badge/license-MIT%20OR%20Apache--2.0-blue.svg)](https://github.com/jxoesneon/martensite#license)

> **Zero-copy GPU hardware media surfaces (DXGI, IOSurface, dma-buf) and external 3D engine interop for Martensite.**

---

## Overview

`martensite-media` provides high-throughput, low-latency video and 3D viewport integration for the Martensite GUI framework. Traditional UI toolkits require video playback pipelines to read decoded frames back into system RAM before re-uploading them as GPU textures, incurring massive memory bandwidth penalties.

`martensite-media` provides the foundation for **zero-copy GPU texture sharing** using native platform primitives:
- **Windows**: Direct3D 11/12 DXGI Shared Handles.
- **macOS**: Apple CoreVideo `IOSurfaceRef` shared buffers.
- **Linux**: Kernel direct rendering manager `dma-buf` export and EGL image external surfaces.

This allows high-bitrate 4K/8K video players, camera feeds, WebRTC streams, and external 3D viewports (e.g. Bevy, game engines, CAD renderers) to be composited directly inside the Martensite scenegraph without CPU copying.

The entire crate is built under `#![forbid(unsafe_code)]`.

---

## Key Features

- **Zero-Copy Native Surfaces**: Architectural primitives designed to share GPU memory allocations directly between media decoders and Martensite's WGPU render pipeline.
- **Cross-Platform Abstraction**: Targets DXGI, IOSurface, and dma-buf surfaces behind a unified texture handle.
- **External 3D Viewport Interop**: Enables embedding standalone 3D graphics scenes as native widgets inside the 2D UI hierarchy.
- **100% Safe Rust**: `#![forbid(unsafe_code)]` enforced across all workspace-level media abstractions.

---

## Part of Martensite

This crate provides hardware media and 3D integration for the [Martensite](https://github.com/jxoesneon/martensite) GUI framework. For top-level widgets and application integration, see [`martensite`](https://crates.io/crates/martensite).

---

## License

Licensed under either of:

- Apache License, Version 2.0 ([LICENSE-APACHE](https://github.com/jxoesneon/martensite/blob/main/LICENSE-APACHE) or <http://www.apache.org/licenses/LICENSE-2.0>)
- MIT license ([LICENSE-MIT](https://github.com/jxoesneon/martensite/blob/main/LICENSE-MIT) or <http://opensource.org/licenses/MIT>)

at your option.
