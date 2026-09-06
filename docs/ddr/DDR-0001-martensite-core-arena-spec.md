# Detailed Design Record: DDR-0001
## Title: `martensite-core` Generational Arena Specification & Hot/Cold Memory Layout

### 1. Architectural Role & Invariants
`martensite-core` provides the primary memory allocation, scene-graph topology, and node lifecycle management for Martensite. It completely replaces standard tree pointer graphs (`Rc<RefCell<Node>>`) with a two-tier dense-sparse Generational SlotMap.
* **Invariant 1.1**: Direct node-to-node pointers or nested `Box<dyn Widget>` references are strictly prohibited. All intra-tree references use unboxed, copyable `WidgetId` tokens.
* **Invariant 1.2**: `HotNode` data must strictly fit within exactly 64 bytes (`#[repr(C, align(64))]`) to guarantee 100% L1 CPU cache-line alignment and prevent false sharing.
* **Invariant 1.3**: The `hot_nodes` array must maintain continuous packing with **zero tombstones/holes**. Deletion operations execute via O(1) swap-remove compaction.

---

### 2. Memory Layout & Bit-Exact Struct Specifications

#### 2.1 The 64-Byte Cache-Line Aligned `HotNode`
```rust
use glam::Vec2;

#[repr(C)]
#[derive(Copy, Clone, Debug, Default, PartialEq)]
pub struct Rect {
    pub origin: Vec2, // 8 bytes (x: f32, y: f32)
    pub size: Vec2,   // 8 bytes (width: f32, height: f32)
}

bitflags::bitflags! {
    #[derive(Copy, Clone, Debug, PartialEq, Eq)]
    pub struct NodeFlags: u32 {
        const DIRTY_LAYOUT       = 1 << 0;
        const DIRTY_PAINT        = 1 << 1;
        const DIRTY_A11Y         = 1 << 2;
        const VISIBLE            = 1 << 3;
        const HIT_TEST_ENABLED   = 1 << 4;
        const FOCUSABLE          = 1 << 5;
        const CLIPS_CHILDREN     = 1 << 6;
        const HOVERED            = 1 << 7;
        const PRESSED            = 1 << 8;
        const INERT              = 1 << 9;
    }
}

/// Strictly 64 Bytes — Exactly 1 CPU Cache Line
#[repr(C, align(64))]
#[derive(Copy, Clone, Debug)]
pub struct HotNode {
    pub layout_id: taffy::NodeId,         // 8 bytes (Taffy layout node handle)
    pub bounds: Rect,                     // 16 bytes (Computed visual bounds)
    pub flags: NodeFlags,                 // 4 bytes (State & dirty bitflags)
    pub layer_depth: u32,                 // 4 bytes (Z-order stacking context depth)
    pub parent: Option<WidgetId>,         // 8 bytes (8-byte Option<WidgetId>)
    pub first_child: Option<WidgetId>,    // 8 bytes
    pub next_sibling: Option<WidgetId>,   // 8 bytes
    pub prev_sibling: Option<WidgetId>,   // 8 bytes
}
// TOTAL SIZE: 8 + 16 + 4 + 4 + 8 + 8 + 8 + 8 = 64 bytes.
```

#### 2.2 The Parallel `ColdNode` Struct
```rust
pub struct ColdNode {
    pub debug_name: Option<&'static str>,
    pub tooltip: Option<String>,
    pub a11y_role: accesskit::Role,
    pub a11y_name: Option<String>,
    pub widget: Box<dyn crate::widget::Widget>,
}
```

#### 2.3 The 64-Bit Unboxed `WidgetId` Handle
```rust
#[repr(C)]
#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash)]
pub struct WidgetId {
    pub slot_idx: u32,
    pub generation: u32,
}
```

---

### 3. O(1) Compaction & Swap-Remove Algorithm

```
Step 1: Invalidate Slot Generation
Slot[RemovedId.slot_idx].generation += 1

Step 2: Swap-Remove Hot & Cold Nodes
hot_nodes.swap_remove(dense_idx)
cold_nodes.swap_remove(dense_idx)
dense_to_slot.swap_remove(dense_idx)

Step 3: Update Redirect Slot for Relocated Node
if dense_idx < hot_nodes.len() {
    relocated_slot = dense_to_slot[dense_idx];
    Slot[relocated_slot].dense_idx = dense_idx;
}
```
