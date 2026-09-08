# martensite-dnd

[![Crates.io](https://img.shields.io/crates/v/martensite-dnd.svg)](https://crates.io/crates/martensite-dnd)
[![Documentation](https://docs.rs/martensite-dnd/badge.svg)](https://docs.rs/martensite-dnd)
[![License](https://img.shields.io/badge/license-MIT%20OR%20Apache--2.0-blue.svg)](https://github.com/jxoesneon/martensite#license)

> **Detached, process-wide drag-and-drop engine with lifecycle tracking and drop target validation.**

---

## Overview

`martensite-dnd` provides a unified drag-and-drop subsystem for the Martensite GUI framework. Its core architectural principle is **detached session survival**: an active drag operation is managed through a process-wide `DndSession` holding an opaque, thread-safe payload.

This ensures that even if the originating widget or window is removed, unmounted, or evicted from the `WidgetArena` mid-flight, the in-progress drag session remains fully valid until the operating system signals an explicit drop completion or user cancellation.

The entire crate is built under `#![forbid(unsafe_code)]`.

---

## Key Features

- **Detached Session Lifecycle (`DndSessionManager`)**: Tracks active drag sessions with unique 64-bit `SessionId`s, insulating gestures against sudden component unmounts.
- **Drop Target Registry (`DropTargetRegistry`)**: Manages spatial hit-testing for drop targets, firing deterministic `drag_enter`, `drag_over`, `drag_leave`, and `drop` lifecycle events.
- **Drop Effects & Masks (`DropEffect`)**: Full support for standard platform drop actions: `DropEffect::Copy`, `DropEffect::Move`, and `DropEffect::Link`.
- **Thread-Safe Payloads**: Supports passing arbitrary `Send + Sync + 'static` data structures across window boundaries.
- **100% Safe Rust**: `#![forbid(unsafe_code)]` enforced across all session management and target routing.

---

## Quick Start

Add `martensite-dnd` to your `Cargo.toml`:

```toml
[dependencies]
martensite-dnd = "0.7.0"
```

Registering a drop target and managing effects:

```rust
use martensite_dnd::{DropEffect, DropTarget, DropTargetRegistry, DropEffectMask};

fn main() {
    let mut registry = DropTargetRegistry::new();

    // Register a target accepting Move and Copy operations
    let mask = DropEffectMask::COPY | DropEffectMask::MOVE;
    let target = DropTarget::new(mask);
    let target_id = registry.register(target);

    assert!(registry.contains(target_id));
}
```

---

## Part of Martensite

This crate provides drag-and-drop interaction services for the [Martensite](https://github.com/jxoesneon/martensite) GUI framework. For top-level widgets and application integration, see [`martensite`](https://crates.io/crates/martensite).

---

## License

Licensed under either of:

- Apache License, Version 2.0 ([LICENSE-APACHE](https://github.com/jxoesneon/martensite/blob/main/LICENSE-APACHE) or <http://www.apache.org/licenses/LICENSE-2.0>)
- MIT license ([LICENSE-MIT](https://github.com/jxoesneon/martensite/blob/main/LICENSE-MIT) or <http://opensource.org/licenses/MIT>)

at your option.
