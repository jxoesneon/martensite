# martensite-vello

[![License](https://img.shields.io/badge/license-MIT%20OR%20Apache--2.0-blue.svg)](https://github.com/jxoesneon/martensite#license)

> **Martensite-project vendored copy of Vello 0.10 built against wgpu 30.**

---

## Overview

`martensite-vello` vendors the source of [`netrender-vello`](https://crates.io/crates/netrender-vello) 0.10.0 — a byte-compatible republish of upstream [Vello](https://github.com/linebender/vello) 0.10 — as a first-party workspace crate for the Martensite GUI framework.

The crate retains the library name `vello` to preserve drop-in API compatibility (`use vello::...`), and keeps all upstream feature flags unchanged.

---

## Why this crate exists

- **wgpu unification**: official `vello` 0.10 pins `wgpu` 29, which cannot coexist with the workspace's `wgpu` 30 as a single type. This build is compiled against `wgpu` 30 so `vello::Renderer` can consume the `wgpu::Device`/`wgpu::Queue` owned by `martensite-wgpu`'s `GpuContext`.
- **Supply-chain hardening**: `netrender-vello` is a single-maintainer republish flagged in audit. Vendoring the identical, byte-compatible source in-tree removes the third-party publisher from the registry dependency graph without changing any code.

---

## Cargo Feature Flags

| Feature | Description | Default |
| :--- | :--- | :--- |
| `default` | `wgpu` + `wgpu_default`. | Yes |
| `wgpu` | Enables the wgpu-based `Renderer` (pulls in `vello_shaders`). | Yes |
| `wgpu_default` | Enables wgpu's default features. Disable to customise the wgpu feature set. | Yes |
| `bump_estimate` | GPU memory usage estimation for bump-allocated buffers. | No |
| `debug_layers` | Debug features for the "async" pipeline (Vello development only). | No |
| `wgpu-profiler` | Embeds a wgpu-profiler profiler (Vello development only). | No |
| `hot_reload` | Hot reloading of Vello shaders (Vello development only). | No |

---

## Usage

Depend on it under the `vello` library name:

```toml
[dependencies]
vello = { package = "martensite-vello", version = "0.10.0-martensite.1", default-features = false, features = ["wgpu"] }
```

```rust
use vello::{Renderer, RendererOptions, Scene};
```

---

## Part of Martensite

This crate supplies the GPU compute renderer for [`martensite-render`](https://crates.io/crates/martensite-render) / `martensite-wgpu` and the [Martensite](https://github.com/jxoesneon/martensite) GUI framework.

---

## License

Licensed under either of:

- Apache License, Version 2.0 ([LICENSE-APACHE](LICENSE-APACHE) or <http://www.apache.org/licenses/LICENSE-2.0>)
- MIT license ([LICENSE-MIT](LICENSE-MIT) or <http://opensource.org/licenses/MIT>)

at your option. Upstream Vello is copyright its authors; see the license files for details.
