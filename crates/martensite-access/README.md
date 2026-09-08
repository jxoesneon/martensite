# martensite-access

[![Crates.io](https://img.shields.io/crates/v/martensite-access.svg)](https://crates.io/crates/martensite-access)
[![Documentation](https://docs.rs/martensite-access/badge.svg)](https://docs.rs/martensite-access)
[![License](https://img.shields.io/badge/license-MIT%20OR%20Apache--2.0-blue.svg)](https://github.com/jxoesneon/martensite#license)

> **Native platform accessibility adapter for Martensite via AccessKit with incremental TreeUpdate synchronization.**

---

## Overview

`martensite-access` bridges the Martensite `WidgetArena` to platform screen readers and assistive technologies (Windows UI Automation, macOS NSAccessibility, and Linux AT-SPI2) through [AccessKit](https://crates.io/crates/accesskit).

Rather than transmitting full accessibility tree snapshots every frame, `martensite-access` relies on **incremental updates**. By monitoring the `NodeFlags::DIRTY_A11Y` bitset, the adapter serializes only mutated accessibility nodes into lightweight `TreeUpdate` batches, eliminating lag for users of screen readers.

The entire crate is built under `#![forbid(unsafe_code)]`.

---

## Key Features

- **Cross-Platform Accessibility**: First-class integration with Windows UIA, macOS VoiceOver, and Linux Orca/AT-SPI2.
- **Incremental Synchronization (`AccessKitAdapter`)**: Emits minimal diffs via `TreeUpdate` only for nodes marked with `NodeFlags::DIRTY_A11Y`.
- **Stable Node Identity (`widget_id_to_node_id`)**: Maps 64-bit generational `WidgetId` handles directly to AccessKit `NodeId`s, preserving focus continuity across frame transitions.
- **Semantic Action Routing**: Dispatches accessibility actions (`Action::Click`, `Action::Focus`, `Action::SetValue`) from assistive software directly back into widget event callbacks.
- **Declarative Builder (`AccessibilityBuilder`)**: Helper API for configuring accessible roles (`Role::Button`, `Role::CheckBox`, `Role::TextInput`), values, descriptions, and focus states.

---

## Quick Start

Add `martensite-access` to your `Cargo.toml`:

```toml
[dependencies]
martensite-access = "0.7.0"
```

Configuring an accessible node:

```rust
use martensite_access::{AccessibilityBuilder, Role};

fn main() {
    let node = AccessibilityBuilder::new(Role::Button)
        .name("Submit Form")
        .build();

    assert_eq!(node.role(), Role::Button);
}
```

---

## Part of Martensite

This crate provides accessibility services for the [Martensite](https://github.com/jxoesneon/martensite) GUI framework. For top-level widgets and application integration, see [`martensite`](https://crates.io/crates/martensite).

---

## License

Licensed under either of:

- Apache License, Version 2.0 ([LICENSE-APACHE](https://github.com/jxoesneon/martensite/blob/main/LICENSE-APACHE) or <http://www.apache.org/licenses/LICENSE-2.0>)
- MIT license ([LICENSE-MIT](https://github.com/jxoesneon/martensite/blob/main/LICENSE-MIT) or <http://opensource.org/licenses/MIT>)

at your option.
