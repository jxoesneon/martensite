# martensite-l10n

[![Crates.io](https://img.shields.io/crates/v/martensite-l10n.svg)](https://crates.io/crates/martensite-l10n)
[![Documentation](https://docs.rs/martensite-l10n/badge.svg)](https://docs.rs/martensite-l10n)
[![License](https://img.shields.io/badge/license-MIT%20OR%20Apache--2.0-blue.svg)](https://github.com/jxoesneon/martensite#license)

> **Natural language localization via Project Fluent with script directionality and reactive locale signal switching.**

---

## Overview

`martensite-l10n` brings expressive natural language localization to the Martensite GUI framework using Mozilla's [Project Fluent](https://projectfluent.org/) format. Unlike simple key-value string replacement maps, Fluent handles complex grammatical gender, plural rules, and context-dependent text inflections naturally.

`martensite-l10n` deeply integrates with `martensite-reactive`. The core **`L10n`** coordinator stores the active locale in a reactive signal. When the user changes their language preferences at runtime, every localized string in the active UI updates automatically and synchronously through reactive memoization without full-app restarts.

The entire crate is built under `#![forbid(unsafe_code)]`.

---

## Key Features

- **Project Fluent Integration (`FluentCatalog`)**: Industry-standard syntax supporting plurals, terms, selectors, and variable arguments.
- **Reactive Locale Switching (`L10n`)**: Bound to `martensite-reactive`, enabling instantaneous runtime language switching with minimal dirty-node propagation.
- **Script Directionality (`ScriptDirection`)**: Automatically determines whether a language requires Left-to-Right (`Ltr`) or Right-to-Left (`Rtl`) layout, triggering layout mirroring for Arabic, Hebrew, and Persian.
- **Unicode BCP47 Support**: Accurate locale handling powered by `unic_langid::LanguageIdentifier`.
- **100% Safe Rust**: `#![forbid(unsafe_code)]` enforced across all catalog parsing and localization pipelines.

---

## Quick Start

Add `martensite-l10n` to your `Cargo.toml`:

```toml
[dependencies]
martensite-l10n = "0.7.0"
```

Reactive localization with dynamic locale switching:

```rust
use martensite_l10n::reactive::L10n;
use martensite_reactive::flush;

fn main() {
    let l10n = L10n::new("en".parse().unwrap());

    // Register bundles for English and Spanish
    l10n.add_bundle(
        "en".parse().unwrap(),
        vec!["greeting = Hello, world!".to_string()],
    ).unwrap();

    l10n.add_bundle(
        "es".parse().unwrap(),
        vec!["greeting = ¡Hola, mundo!".to_string()],
    ).unwrap();

    // Create a reactive localized string memo
    let text = l10n.localized("greeting");
    assert_eq!(text.get(), "Hello, world!");

    // Switch locale dynamically
    l10n.set_locale("es".parse().unwrap()).unwrap();
    flush();
    assert_eq!(text.get(), "¡Hola, mundo!");
}
```

---

## Part of Martensite

This crate provides natural language localization for the [Martensite](https://github.com/jxoesneon/martensite) GUI framework. For top-level widgets and application integration, see [`martensite`](https://crates.io/crates/martensite).

---

## License

Licensed under either of:

- Apache License, Version 2.0 ([LICENSE-APACHE](https://github.com/jxoesneon/martensite/blob/main/LICENSE-APACHE) or <http://www.apache.org/licenses/LICENSE-2.0>)
- MIT license ([LICENSE-MIT](https://github.com/jxoesneon/martensite/blob/main/LICENSE-MIT) or <http://opensource.org/licenses/MIT>)

at your option.
