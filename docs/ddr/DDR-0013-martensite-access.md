# Detailed Design Record: DDR-0013
## Title: `martensite-access` AccessKit Incremental Tree Diffing

### 1. Architectural Role & Invariants
`martensite-access` translates the `martensite-core` widget arena into standard OS accessibility trees via `accesskit`.
* **Invariant 1.1**: Every interactive widget strictly maps to an `accesskit::Role`.
* **Invariant 1.2**: Incremental updates (`TreeUpdate`) emit only changed nodes. No full tree rebuilds unless forced by OS.
* **Invariant 1.3**: Accessibility updates must run asynchronously or lazily outside the hot rendering path to avoid blocking 60/120Hz frames.

### 2. TreeUpdate Diffing Algorithm
```rust
use accesskit::{TreeUpdate, NodeId, Node};

pub fn build_a11y_update(arena: &WidgetArena) -> TreeUpdate {
    let mut update = TreeUpdate::default();
    
    for (widget_id, hot_node) in arena.hot_nodes.iter() {
        if hot_node.flags.contains(NodeFlags::DIRTY_A11Y) {
            let cold_node = &arena.cold_nodes[widget_id.slot_idx as usize];
            let accesskit_node = build_node(hot_node, cold_node);
            
            update.nodes.push((
                accesskit::NodeId(widget_id.slot_idx as u64),
                accesskit_node
            ));
        }
    }
    
    update
}
```

### 3. Role Mapping Table
| Martensite Role | AccessKit Role | Platform Specifics (macOS/UIA/AT-SPI) |
| --- | --- | --- |
| Button | `Role::Button` | UIA: InvokePattern |
| TextInput | `Role::TextInput` | NSAccessibility: Value/Text patterns |
| Checkbox | `Role::CheckBox` | UIA: TogglePattern |
| LiveRegion | `Role::Log` / `Role::Marquee` | NSAccessibility: liveRegionStatus |

### 4. Error Conditions
- If the OS Accessibility daemon restarts (e.g. Windows narrators), the AccessKit adapter triggers a full `TreeUpdate`. The entire `DIRTY_A11Y` bitset is temporarily forced high for one frame.

### 5. Performance Invariants
- Normal incremental updates: $O(K)$ where $K$ is the number of mutated nodes.
- Latency budget: < 1.0ms payload generation. Serialization is offloaded to AccessKit's background threads.
