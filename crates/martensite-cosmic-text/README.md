# martensite-cosmic-text

[![Crates.io](https://img.shields.io/crates/v/martensite-cosmic-text.svg)](https://crates.io/crates/martensite-cosmic-text)
[![Documentation](https://docs.rs/martensite-cosmic-text/badge.svg)](https://docs.rs/martensite-cosmic-text)
[![License](https://img.shields.io/badge/license-MIT%20OR%20Apache--2.0-blue.svg)](https://github.com/jxoesneon/martensite#license)

> **Martensite-project supply-chain fork of cosmic-text with updated fontdb dependencies and audit hardening.**

---

## Overview

`martensite-cosmic-text` is a security-hardened fork of [cosmic-text](https://github.com/pop-os/cosmic-text) maintained for the Martensite GUI framework.

Upstream `cosmic-text` 0.19 pins `fontdb` to `^0.23`, which transitively depends on an unmaintained version of the `ttf-parser` crate affected by [RUSTSEC-2026-0192](https://rustsec.org/advisories/RUSTSEC-2026-0192). This fork upgrades `fontdb` to `0.24`, remediating the vulnerability and ensuring 100% clean `cargo audit` and `cargo deny` compliance across the entire workspace.

The crate retains the library name `cosmic_text` to preserve drop-in API compatibility with all upstream code (`use cosmic_text::...`).

---

## Key Changes from Upstream

- **Remediated Vulnerability**: Upgrades `fontdb` from `0.23` to `0.24` to eliminate the unmaintained `ttf-parser` transitive dependency.
- **Drop-in Compatibility**: Retains identical public types, function signatures, and modules (`Attrs`, `Buffer`, `Family`, `FontSystem`, `Metrics`, `Shaping`).
- **Cargo Deny & Audit Verified**: Passes all supply-chain bans, advisories, and license checks.

---

## Cargo Feature Flags

| Feature | Description | Default |
| :--- | :--- | :--- |
| `default` | Standard feature set including `std`, `fontconfig` (on Linux), and system fonts. | Yes |
| `std` | Enables standard library features. | Yes |
| `shape-run-cache` | Enables zero-frame jitter caching of shaped text runs. | Yes |
| `fontconfig` | Enables font discovery via Fontconfig on Unix platforms. | Yes |
| `no_std` | Compiles for embedded or `no_std` environments without OS font discovery. | No |
| `peniko` | Integrates color types from Peniko/Vello. | No |
| `vi` | Enables modal vi-style cursor navigation keys. | No |
| `wasm-web` | WebAssembly support for browser canvas/DOM environments. | No |

---

## Quick Start

Add `martensite-cosmic-text` as `cosmic_text` in your `Cargo.toml`:

```toml
[dependencies]
cosmic_text = { package = "martensite-cosmic-text", version = "0.19.0-martensite.1" }
```

```rust
use cosmic_text::{Attrs, Buffer, FontSystem, Metrics, Shaping};

fn main() {
    let mut font_system = FontSystem::new();
    let metrics = Metrics::new(14.0, 20.0);
    let mut buffer = Buffer::new(&mut font_system, metrics);

    buffer.set_text(&mut font_system, "Safe & Audited Text", Attrs::new(), Shaping::Advanced);
    buffer.shape_until_scroll(&mut font_system, false);
}
```

---

## Part of Martensite

This crate supplies the typography engine for [`martensite-text`](https://crates.io/crates/martensite-text) and the [Martensite](https://github.com/jxoesneon/martensite) GUI framework.

---

## License

Licensed under either of:

- Apache License, Version 2.0 ([LICENSE-APACHE](https://github.com/jxoesneon/martensite/blob/main/LICENSE-APACHE) or <http://www.apache.org/licenses/LICENSE-2.0>)
- MIT license ([LICENSE-MIT](https://github.com/jxoesneon/martensite/blob/main/LICENSE-MIT) or <http://opensource.org/licenses/MIT>)

at your option.
