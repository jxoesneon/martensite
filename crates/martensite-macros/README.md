# martensite-macros

[![Crates.io](https://img.shields.io/crates/v/martensite-macros.svg)](https://crates.io/crates/martensite-macros)
[![Documentation](https://docs.rs/martensite-macros/badge.svg)](https://docs.rs/martensite-macros)
[![License](https://img.shields.io/badge/license-MIT%20OR%20Apache--2.0-blue.svg)](https://github.com/jxoesneon/martensite#license)

> **Declarative procedural macros for widget declaration and reactive composition in Martensite.**

---

## Overview

`martensite-macros` provides compile-time procedural macros that eliminate boilerplate when defining custom widgets and reactive bindings within the Martensite GUI framework.

By generating structural scaffolding and default implementations at compile time, `martensite-macros` ensures that widget types adhere strictly to framework expectations with zero runtime cost.

The entire crate is built under `#![forbid(unsafe_code)]`.

---

## Key Features

- **Declarative Widget Generation (`widget!`)**: Generates custom widget structures with appropriate derivations (`Default`, `Debug`) and documentation.
- **Compile-Time Error Checking**: Emits expressive, compiler-friendly diagnostics for invalid syntax or missing required widget attributes.
- **Zero Runtime Overhead**: Expands entirely during compilation without adding reflection or dynamic overhead to your binary.
- **100% Safe Rust**: `#![forbid(unsafe_code)]` enforced.

---

## Quick Start

Add `martensite-macros` to your `Cargo.toml`:

```toml
[dependencies]
martensite-macros = "0.7.0"
```

Declaring a custom widget:

```rust
use martensite_macros::widget;

// Generates a custom widget structure with Default implementations
widget!(MyCustomCard);

fn main() {
    let _card = MyCustomCard::default();
}
```

---

## Part of Martensite

This crate provides macro syntax for the [Martensite](https://github.com/jxoesneon/martensite) GUI framework. For top-level widgets and application integration, see [`martensite`](https://crates.io/crates/martensite).

---

## License

Licensed under either of:

- Apache License, Version 2.0 ([LICENSE-APACHE](https://github.com/jxoesneon/martensite/blob/main/LICENSE-APACHE) or <http://www.apache.org/licenses/LICENSE-2.0>)
- MIT license ([LICENSE-MIT](https://github.com/jxoesneon/martensite/blob/main/LICENSE-MIT) or <http://opensource.org/licenses/MIT>)

at your option.
