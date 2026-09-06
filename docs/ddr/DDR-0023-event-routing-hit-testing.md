# Detailed Design Record: DDR-0023
## Title: Event Routing and Hit-Testing Protocol

### 1. Architectural Role & Invariants
This document details the event propagation subsystem, determining how raw OS input events are normalized, mapped to UI nodes via hit-testing, and routed through the scene graph using a capture-and-bubble phase model.
* **Invariant 1.1**: Hit testing relies on a reverse Z-order traversal. Visual occlusions perfectly map to event occlusions unless a node is explicitly configured otherwise.
* **Invariant 1.2**: Event bubbling respects tree hierarchy but allows nodes to halt propagation (`cx.stop_propagation()`).
* **Invariant 1.3**: Hit testing must respect clipping regions established by parent nodes with the `CLIPS_CHILDREN` flag.

---

### 2. Rust Type Definitions

```rust
use glam::Vec2;
use crate::core::WidgetId;

/// Represents an active pointer/mouse or keyboard event.
#[derive(Debug, Clone, PartialEq)]
pub enum Event {
    PointerDown { position: Vec2, button: u8 },
    PointerUp { position: Vec2, button: u8 },
    PointerMove { position: Vec2 },
    KeyDown { key_code: u32, modifiers: u32 },
    KeyUp { key_code: u32, modifiers: u32 },
    HoverEnter,
    HoverExit,
}

/// The result of a hit-test operation.
#[derive(Debug, Clone, Copy)]
pub struct HitTestResult {
    /// The specific node that was hit.
    pub widget: WidgetId,
    /// The coordinates of the hit relative to the node's local origin.
    pub local_position: Vec2,
}

/// Defines how the event router should proceed after a node handles an event.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EventResponse {
    /// The event was ignored; continue routing.
    PassThrough,
    /// The event was handled; stop routing to other nodes.
    Consumed,
}
```

---

### 3. Hit-Testing & Clipping Protocol

Hit-testing converts a global screen coordinate into a specific `HitTestResult`. 

1. **Reverse Z-Order Traversal**: The scene graph is traversed starting from the highest `layer_depth` (front-most) down to the root. Only leaf nodes or nodes with specific event handlers are evaluated.
2. **AABB Intersection**: A node is considered a hit if the pointer coordinates fall within its `bounds` (Axis-Aligned Bounding Box).
3. **Clipping Rules**: If a parent node has the `NodeFlags::CLIPS_CHILDREN` flag set, any hit-test against its descendants will first intersect with the parent's bounding box. If the pointer is outside the parent's box, the entire child subtree is immediately culled from the hit-test search, operating in O(1) time relative to the subtree depth.

---

### 4. Event Routing Pipeline

The routing pipeline follows a strict multi-stage lifecycle:

1. **Normalization**: Raw `winit` OS events are converted into Martensite's backend-agnostic `Event` enum.
2. **Hit-Testing (For Pointer Events)**: Determine the target `WidgetId`.
3. **Target Routing**: Deliver the event to the target widget.
4. **Capture Phase**: (Optional) Ancestors of the target can intercept the event before it reaches the target.
5. **Bubble Phase**: If the target does not return `EventResponse::Consumed`, the event propagates upwards to its parent.

#### Bubbling vs. Non-Bubbling Events
* **Bubbling Events**: `PointerDown`, `PointerUp`, `KeyDown`, `KeyUp`. These events intuitively propagate up the tree (e.g., clicking a text label inside a button triggers the button's click handler).
* **Non-Bubbling Events**: `HoverEnter`, `HoverExit`. These are discrete edge-trigger events specific to a single node's bounding box and do not bubble.

#### Focus vs. Pointer Routing Paths
* **Pointer Events** use spatial routing based on the hit-test target.
* **Focus Events** (e.g., keyboard input) bypass hit-testing entirely. The router maintains a `focused_widget: Option<WidgetId>`. Keyboard events are routed directly to the focused widget, bubbling up its specific ancestor chain.

---

### 5. The EventContext API

Widgets handle events via an `EventContext` which provides mutation access to the routing state:

```rust
pub struct EventContext<'a> {
    pub(crate) current_widget: WidgetId,
    pub(crate) propagation_stopped: bool,
    // internal fields omitted
}

impl<'a> EventContext<'a> {
    /// Halts the bubbling phase. Ancestor nodes will not receive this event.
    pub fn stop_propagation(&mut self) {
        self.propagation_stopped = true;
    }

    /// Requests exclusive capture of all future pointer events until released.
    /// Used for drag-and-drop or slider interactions.
    pub fn capture_pointer(&mut self) {
        // Implementation delegates to the root event router
    }
}
```
