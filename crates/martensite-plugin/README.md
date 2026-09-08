# martensite-plugin

[![Crates.io](https://img.shields.io/crates/v/martensite-plugin.svg)](https://crates.io/crates/martensite-plugin)
[![Documentation](https://docs.rs/martensite-plugin/badge.svg)](https://docs.rs/martensite-plugin)
[![License](https://img.shields.io/badge/license-MIT%20OR%20Apache--2.0-blue.svg)](https://github.com/jxoesneon/martensite#license)

> **Wasmtime sandboxed plugin runtime for the Martensite GUI framework.**

---

## Overview

`martensite-plugin` runs untrusted third-party widgets as `wasm32-wasip1`
WebAssembly modules inside a Wasmtime sandbox. Each plugin instance receives a
strict fuel budget and epoch interruption, so runaway code cannot stall the main
UI loop.

Host access is denied by default. Plugins must be granted explicit
capabilities—such as reading a specific reactive signal or accessing a
particular filesystem path—before any host call succeeds. Unauthorized calls trap
the guest cleanly.

---

## Key Features

- **Wasmtime Sandbox**: Loads `wasm32-wasip1` modules with fuel and epoch-based
  interruption.
- **Capability Security**: Fine-grained [`Capability`] grants for signal reads/
  writes, file reads/writes, and network access.
- **Zero-Allocation Ring Buffer**: A 256 KiB shared linear-memory
  [`PluginRingBuffer`] lets plugins push [`PluginPaintCmd`] packets directly into
  host memory without per-frame allocation.
- **Safe Rust**: The crate is built under `#![forbid(unsafe_code)]`.

---

## Quick Start

```rust
use martensite_plugin::{Capability, CapabilitySet, PluginRuntime};
use std::path::PathBuf;

let runtime = PluginRuntime::new()?;
let caps = CapabilitySet::builder()
    .grant(Capability::SignalRead(martensite_reactive::SignalId::next()))
    .grant(Capability::FileRead(PathBuf::from("/assets")))
    .build();

let mut plugin = runtime.load(wasm_bytes, caps)?;
plugin.invoke("run")?;
```

---

## Part of Martensite

This crate provides the WebAssembly plugin extension point for the
[Martensite](https://github.com/jxoesneon/martensite) GUI framework. For the
main application crate, see [`martensite`](https://crates.io/crates/martensite).

---

## License

Licensed under either of:

- Apache License, Version 2.0 ([LICENSE-APACHE](https://github.com/jxoesneon/martensite/blob/main/LICENSE-APACHE) or <http://www.apache.org/licenses/LICENSE-2.0>)
- MIT license ([LICENSE-MIT](https://github.com/jxoesneon/martensite/blob/main/LICENSE-MIT) or <http://opensource.org/licenses/MIT>)

at your option.
