# martensite-core

[![Crates.io](https://img.shields.io/crates/v/martensite-core.svg)](https://crates.io/crates/martensite-core)
[![Documentation](https://docs.rs/martensite-core/badge.svg)](https://docs.rs/martensite-core)
[![License](https://img.shields.io/badge/license-MIT%20OR%20Apache--2.0-blue.svg)](https://github.com/jxoesneon/martensite#license)

> **Generational slotmap arena, SIMD-aligned 64-byte HotNode cache line storage, and the foundational `Widget` trait for Martensite.**

---

## Overview

`martensite-core` is the foundational memory and scenegraph layer of the Martensite GUI framework. It decouples interface state into a high-performance, cache-conscious dual-storage hierarchy:

1. **`HotNode` (64 bytes)**: Exactly fits a single modern CPU L1/L2 cache line. Houses spatial bounds (`Rect`), traversal pointers (parent, siblings, children), topological depth ranks, and bitflags. Critical layout, hit-testing, and dirty-bit passes iterate strictly across contiguous slices of `HotNode` without touching cold heap memory.
2. **`ColdNode`**: Houses infrequently accessed metadata, boxed `Widget` trait implementations, event handler closures, and auxiliary styling properties.

References between nodes are tracked through copyable 64-bit `WidgetId` handles (`u32` slot index + `u32` generation counter), completely eliminating reference counting cycles (`Rc`/`Arc`), interior mutability overhead (`RefCell`), and dangling pointer bugs.

---

## Key Features

- **Cache-Conscious Dual Storage**:
  - `HotNode` array stored contiguously in dense memory for maximum memory bandwidth during hit-testing and render-list compilation.
  - `ColdNode` storage allocated alongside hot nodes without polluting the CPU cache during traversal.
- **Generational Slotmap (`WidgetArena`)**:
  - $O(1)$ allocation, $O(1)$ handle validation, and $O(1)$ removal with generation rollover protection.
  - Automatic slot recycling to maintain predictable memory footprints without runtime GC.
- **Tree Hierarchy Operations**:
  - Native hierarchical methods: `append_child`, `prepend_child`, `insert_before`, `insert_after`, `detach`, and `remove`.
  - Topological depth ranking and ancestor validation (`is_ancestor_of`) to prevent cycle formation.
- **Foundational `Widget` Trait**:
  - Defines the core lifecycle for UI elements: `measure` (two-pass constraint solving), `layout` (spatial assignment), `paint` (command stream recording), and `event` (event handling).

---

## Quick Start

Add `martensite-core` to your `Cargo.toml`:

```toml
[dependencies]
martensite-core = "0.7.0"
```

Managing an interface tree in the arena:

```rust
use martensite_core::{WidgetArena, HotNode, ColdNode, Rect, NodeFlags};

fn main() {
    let mut arena = WidgetArena::new();

    // Create a root node
    let root = arena.insert(
        HotNode::new(Rect::from_xywh(0.0, 0.0, 800.0, 600.0)),
        ColdNode::default(),
    );

    // Create and attach child nodes
    let child = arena.insert(
        HotNode::new(Rect::from_xywh(10.0, 10.0, 200.0, 40.0)),
        ColdNode::default(),
    );

    arena.append_child(root, child).expect("valid hierarchy");

    // Verify parent-child relationship
    assert_eq!(arena.parent(child), Some(root));
    assert_eq!(arena.first_child(root), Some(child));
    assert_eq!(arena.len(), 2);
}
```

---

## Memory Layout Guarantees

```text
HotNode Memory Layout (64 bytes = 1 Cache Line):
+-----------------------+----------+---------------------------------------------+
| Field                 | Size     | Purpose                                     |
+-----------------------+----------+---------------------------------------------+
| bounds: Rect          | 16 bytes | Computed x, y, width, height (f32)          |
| flags: NodeFlags      |  4 bytes | Visibility, focusability, dirty states      |
| depth_rank: u16       |  2 bytes | Topological hierarchy depth                 |
| _padding              |  2 bytes | SIMD alignment padding                      |
| parent: WidgetId      |  8 bytes | Parent handle in generational slotmap       |
| first_child: WidgetId |  8 bytes | First child handle                          |
| last_child: WidgetId  |  8 bytes | Last child handle                           |
| next_sibling: WidgetId|  8 bytes | Next sibling handle                         |
| prev_sibling: WidgetId|  8 bytes | Previous sibling handle                     |
+-----------------------+----------+---------------------------------------------+
Total: 64 bytes (Packed)
```

---

## Part of Martensite

This crate is the core memory foundation of the [Martensite](https://github.com/jxoesneon/martensite) GUI framework. For high-level widgets and application tools, see the primary [`martensite`](https://crates.io/crates/martensite) crate.

---

## License

Licensed under either of:

- Apache License, Version 2.0 ([LICENSE-APACHE](https://github.com/jxoesneon/martensite/blob/main/LICENSE-APACHE) or <http://www.apache.org/licenses/LICENSE-2.0>)
- MIT license ([LICENSE-MIT](https://github.com/jxoesneon/martensite/blob/main/LICENSE-MIT) or <http://opensource.org/licenses/MIT>)

at your option.
