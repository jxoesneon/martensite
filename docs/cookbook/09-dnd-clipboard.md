# Cookbook 09 — Drag & Drop and System Clipboard Integration

Exchanging data between widgets, across application windows, and with external operating system
applications requires handling asynchronous platform protocols, MIME type negotiation, and
memory-efficient payload rendering.

This recipe demonstrates how to integrate Martensite's multi-MIME delayed-rendering clipboard engine
(`martensite-clipboard`) with the detached process-wide drag-and-drop subsystem (`martensite-dnd`).

---

## 1. Goal

Implement an asset manager and spatial drop workspace that:
1. Registers multi-MIME representations (`text/plain`, `text/html`, and custom binary formats) on the system clipboard.
2. Defers expensive payload synthesis (such as high-resolution bitmap rasterization or scene serialization) using lazy MIME evaluation (`ClipboardPayload::Lazy`) guarded by deadlines.
3. Initiates internal and external drag operations (`DndSession`, `DragPayload`) from source widgets.
4. Manages spatial drop targets (`DropTarget`, `DropTargetRegistry`) with drop effect validation (`DropEffect::Copy`, `Move`, `Link`, `None`).
5. Renders interactive visual feedback (drag ghosts, insertion indicators, hover borders).
6. Ensures drag operations survive source widget destruction or source window closure mid-flight.

---

## 2. Complete Runnable Pattern

The following pattern constructs a complete Drag & Drop and Clipboard workspace. It includes an `AssetPalette` with draggable asset cards supporting multi-MIME clipboard copying, and a `DropCanvas` that accepts internal moves as well as external OS file drops.

```rust
use glam::Vec2;
use kurbo::{Point, Rect as KurboRect, RoundedRect};
use martensite::prelude::*;
use martensite_clipboard::{
    canonicalize_mime, ClipboardItem, ClipboardPayload, ClipboardService,
    InMemoryClipboard, PlatformClipboard, DEFAULT_LAZY_DEADLINE,
};
use martensite_dnd::{
    DndPlatform, DndSession, DndSessionManager, DndStatus, DragPayload,
    DropEffect, DropEffectMask, DropOutcome, DropTarget, DropTargetRegistry,
    DropTargetState, DropTypeHint, SessionId, TargetId,
};
use std::sync::Arc;
use std::time::Duration;

/// Standard application-specific MIME identifier for node data.
pub const MIME_MARTENSITE_NODE: &str = "application/x-martensite-node+json";

/// Domain payload representing a draggable visual asset.
#[derive(Clone, Debug, PartialEq)]
pub struct AssetData {
    pub id: String,
    pub title: String,
    pub asset_type: String,
    pub payload_bytes: Vec<u8>,
}

/// Draggable item card supporting both clipboard copy and spatial drag initiation.
pub struct DraggableAssetCard {
    data: AssetData,
    cached_bounds: Rect,
    is_pressed: bool,
    drag_initiated: bool,
    press_origin: Vec2,
}

impl DraggableAssetCard {
    pub fn new(data: AssetData) -> Self {
        Self {
            data,
            cached_bounds: Rect::default(),
            is_pressed: false,
            drag_initiated: false,
            press_origin: Vec2::ZERO,
        }
    }

    /// Exports multi-MIME representation to the system clipboard.
    /// Demonstrates both eager text payloads and lazy delayed-rendering binary serialization.
    pub fn copy_to_clipboard(&self, clipboard: &mut dyn ClipboardService) {
        let title = self.data.title.clone();
        let asset_id = self.data.id.clone();
        let asset_type = self.data.asset_type.clone();
        let heavy_bytes = self.data.payload_bytes.clone();

        let item = ClipboardItem::new()
            // 1. Plain text representation for standard paste targets
            .offer_text(format!("Asset: {} ({})", title, asset_id))
            // 2. HTML snippet with rich styling
            .offer_html(format!(
                "<div class=\"asset-card\" data-id=\"{}\"><strong>{}</strong> ({})</div>",
                asset_id, title, asset_type
            ))
            // 3. Lazy custom MIME payload: synthesized only when an authorized consumer requests it
            .offer_custom(
                MIME_MARTENSITE_NODE,
                ClipboardPayload::lazy({
                    let title = title.clone();
                    let asset_id = asset_id.clone();
                    move || {
                        // Expensive encoding/compression occurs here, never on the UI thread during copy
                        format!(
                            "{{\"id\":\"{}\",\"title\":\"{}\",\"size\":{}}}",
                            asset_id, title, heavy_bytes.len()
                        )
                        .into_bytes()
                    }
                })
                .with_deadline(Duration::from_millis(500)),
            );

        clipboard.set_contents(&item);
    }

    /// Initiates a detached, process-wide drag session.
    pub fn start_drag(
        &mut self,
        session_mgr: &mut DndSessionManager,
        origin_window: u64,
    ) -> SessionId {
        let serialized = format!(
            "{{\"id\":\"{}\",\"title\":\"{}\"}}",
            self.data.id, self.data.title
        );

        // Package drag payload with multi-type hints
        let payload = DragPayload::from_bytes(
            serialized.into_bytes(),
            DropTypeHint::Custom(MIME_MARTENSITE_NODE.to_string()),
        );

        // Create detached session surviving source destruction
        session_mgr.create_session(
            origin_window,
            payload,
            DropEffectMask::COPY | DropEffectMask::MOVE,
        )
    }
}

impl Widget for DraggableAssetCard {
    fn measure(&mut self, cx: &mut LayoutContext, constraints: LayoutConstraints) -> Vec2 {
        let desired = Vec2::new(cx.pt(160.0), cx.pt(64.0));
        Vec2::new(
            desired.x.clamp(constraints.min_size.x, constraints.max_size.x),
            desired.y.clamp(constraints.min_size.y, constraints.max_size.y),
        )
    }

    fn layout(&mut self, _cx: &mut LayoutContext, bounds: Rect) {
        self.cached_bounds = bounds;
    }

    fn paint(&self, cx: &mut PaintContext) {
        let b = cx.bounds;
        let k_rect = KurboRect::new(
            b.min_x() as f64,
            b.min_y() as f64,
            b.max_x() as f64,
            b.max_y() as f64,
        );
        let radius = cx.pt(6.0) as f64;

        // Visual feedback when pressed or dragged
        let bg_color = if self.is_pressed {
            [48, 54, 64, 255]
        } else {
            [36, 40, 48, 255]
        };

        cx.list.push_fill_rounded_rect(k_rect, radius, bg_color);
        cx.list.push_stroke_rect(k_rect, [70, 80, 95, 255], cx.pt(1.0));

        // Draw asset title
        cx.list.push_text(
            Point::new((b.origin.x + cx.pt(12.0)) as f64, (b.origin.y + cx.pt(28.0)) as f64),
            self.data.title.clone(),
            cx.pt(13.0),
            [235, 240, 245, 255],
        );

        // Draw asset type badge
        cx.list.push_text(
            Point::new((b.origin.x + cx.pt(12.0)) as f64, (b.origin.y + cx.pt(48.0)) as f64),
            self.data.asset_type.to_uppercase(),
            cx.pt(9.0),
            [140, 150, 165, 255],
        );
    }
}

/// Canvas workspace acting as a registered spatial drop target.
pub struct DropCanvas {
    target_id: TargetId,
    cached_bounds: Rect,
    hover_state: DropTargetState,
    dropped_assets: Vec<String>,
}

impl DropCanvas {
    pub fn new(target_id: TargetId) -> Self {
        Self {
            target_id,
            cached_bounds: Rect::default(),
            hover_state: DropTargetState::Idle,
            dropped_assets: Vec::new(),
        }
    }

    /// Registers this widget's bounds and supported MIME types with the DND registry.
    pub fn register_target(&self, registry: &mut DropTargetRegistry) {
        let mut target = DropTarget::new(
            self.target_id,
            self.cached_bounds,
            DropEffectMask::COPY | DropEffectMask::MOVE,
        );
        target.accept_mime(MIME_MARTENSITE_NODE);
        target.accept_mime("text/uri-list"); // Accept external files from OS desktop/explorer
        registry.register(target);
    }

    /// Handles drop commit when a drag operation completes over this canvas.
    pub fn handle_drop(&mut self, payload: &DragPayload, effect: DropEffect) -> DropOutcome {
        if effect == DropEffect::None {
            return DropOutcome::Rejected;
        }

        // Check if payload matches our domain type
        if payload.has_mime(MIME_MARTENSITE_NODE) {
            if let Some(bytes) = payload.data_for_mime(MIME_MARTENSITE_NODE) {
                if let Ok(text) = std::str::from_utf8(bytes) {
                    self.dropped_assets.push(format!("Internal Node: {}", text));
                    self.hover_state = DropTargetState::Idle;
                    return DropOutcome::Accepted(effect);
                }
            }
        } else if payload.has_mime("text/uri-list") {
            // External OS file drop
            if let Some(bytes) = payload.data_for_mime("text/uri-list") {
                if let Ok(uris) = std::str::from_utf8(bytes) {
                    self.dropped_assets.push(format!("External Files: {}", uris));
                    self.hover_state = DropTargetState::Idle;
                    return DropOutcome::Accepted(DropEffect::Copy);
                }
            }
        }

        DropOutcome::Rejected
    }
}

impl Widget for DropCanvas {
    fn measure(&mut self, _cx: &mut LayoutContext, constraints: LayoutConstraints) -> Vec2 {
        constraints.max_size
    }

    fn layout(&mut self, _cx: &mut LayoutContext, bounds: Rect) {
        self.cached_bounds = bounds;
    }

    fn paint(&self, cx: &mut PaintContext) {
        let b = cx.bounds;
        let k_rect = KurboRect::new(
            b.min_x() as f64,
            b.min_y() as f64,
            b.max_x() as f64,
            b.max_y() as f64,
        );

        // Distinct visual state based on active drag hover
        let (bg_color, border_color, border_width) = match self.hover_state {
            DropTargetState::HoveredValid => {
                ([28, 42, 36, 255], [46, 180, 100, 255], cx.pt(2.5))
            }
            DropTargetState::HoveredInvalid => {
                ([42, 28, 28, 255], [200, 60, 60, 255], cx.pt(2.0))
            }
            DropTargetState::Idle => {
                ([22, 22, 26, 255], [45, 45, 52, 255], cx.pt(1.0))
            }
        };

        cx.list.push_fill_rect(k_rect, bg_color);
        cx.list.push_stroke_rect(k_rect, border_color, border_width);

        // Header and item count
        cx.list.push_text(
            Point::new((b.origin.x + cx.pt(16.0)) as f64, (b.origin.y + cx.pt(32.0)) as f64),
            format!("Canvas Drop Target — {} items deposited", self.dropped_assets.len()),
            cx.pt(14.0),
            [210, 215, 225, 255],
        );

        // Render deposited asset entries
        let mut y_offset = cx.pt(60.0);
        for item in self.dropped_assets.iter().rev().take(5) {
            cx.list.push_text(
                Point::new((b.origin.x + cx.pt(24.0)) as f64, (b.origin.y + y_offset) as f64),
                item.clone(),
                cx.pt(11.0),
                [160, 175, 190, 255],
            );
            y_offset += cx.pt(20.0);
        }
    }
}
```

---

## 3. Key Architectural Invariants

### 1. Multi-MIME Simultaneous Registration & Canonicalization
Operating systems and destination applications vary widely in their format preferences:
- A text editor expects `text/plain;charset=utf-8`.
- A browser expects `text/html`.
- Another instance of your Martensite application expects `application/x-martensite-node+json`.

`martensite-clipboard` enforces **multi-format simultaneous offering**:
```
ClipboardItem
  ├── "text/plain"               --> ClipboardPayload::Text (Eager)
  ├── "text/html"                --> ClipboardPayload::Text (Eager)
  └── "application/x-martensite" --> ClipboardPayload::Lazy (Delayed)
```
- **MIME Canonicalization**: All MIME types passed through `offer_custom` or `get_contents` are passed through `canonicalize_mime`. Whitespace is trimmed, case is lowered, and standard parameters (e.g. `;charset=utf-8`) are normalized to prevent lookup misses across platform boundaries.

### 2. Zero-Allocation Delayed Rendering & Lazy Deadlines
When a user copies a 50 MB high-resolution image layer or an entire 3D project hierarchy, serializing that data immediately on `Ctrl+C` produces severe frame drops and wastes memory if the user only pastes plain text.

`ClipboardPayload::Lazy` solves this by wrapping an uninvoked closure:
```rust
ClipboardPayload::lazy(move || {
    // Heavy compression or formatting
    encode_heavy_payload()
}).with_deadline(Duration::from_millis(500))
```
- **Deadline Protection**: Lazy evaluations are guarded by `DEFAULT_LAZY_DEADLINE` (typically 500ms). If an external paste operation triggers the lazy closure and it hangs or exceeds the deadline, the platform worker aborts the request rather than deadlocking the OS clipboard daemon.
- **Single Evaluation**: Once evaluated, the payload is cached for subsequent paste queries until the clipboard contents change.

### 3. Detached Process-Wide Session Lifecycle & Multi-Window Survival
In standard window frameworks, drag sessions are often coupled to the originating window or widget. If a user drags an item out of a modal dialog or floating tool palette, and that window closes mid-drag, traditional architectures crash or trigger dangling pointer faults.

Martensite decouples the drag session entirely from the widget hierarchy:
```
[ Window A ]                     [ Process-Wide DndSessionManager ]             [ Window B ]
Widget::start_drag()  ------->   Allocates SessionId (Arc<Payload>)
Window A Closes       ------->   Session remains fully ALIVE and VALID!
                                           |
Pointer moves over Window B -------------->| Drops onto DropCanvas
                                           | Target receives valid Arc<Payload>
```
1. `DndSessionManager` is owned process-wide, outside any single `WidgetArena`.
2. When `start_drag` is invoked, the payload is moved into a thread-safe `Arc<DragPayload>`.
3. If the originating widget or even the parent OS window is unmounted or destroyed while the pointer is in flight, the drag session continues unabated.
4. The session is only destroyed when the platform bridge signals an OS-level completion (`DropOutcome::Accepted` or `DropOutcome::Cancelled`).

### 4. Unified Event Stream: Bridging Internal and External Drops
Martensite uses `DropBridge` in `martensite-dnd` to normalize OS-level drag events (from winit, macOS `NSDraggingDestination`, Windows OLE `IDropTarget`, or Wayland `wl_data_device`) into the same unified event model as internal virtual drags:

| Event Type | Internal Drag Origin | External OS Origin |
|---|---|---|
| **Drag Source** | In-process widget (`AssetCard`) | Desktop, Explorer, Finder, Web Browser |
| **Payload Delivery** | In-memory `Vec<u8>` or shared reference | OS file descriptor / COM stream read |
| **Available Types** | Explicitly declared MIME list | Synthesized MIME list (`text/uri-list`, etc.) |
| **Drop Effect** | Governed by keyboard modifiers + source mask | Governed by OS compositor negotiation |

### 5. Spatial Drop Target Registry & Hit-Testing
During cursor motion, `DropTargetRegistry` tests the cursor coordinate against all registered `DropTarget` bounds:
- **Z-Order Respect**: Targets registered later or deeper in the hierarchy shadow parent targets unless the child rejects the drop.
- **State Transition Machine**: As the pointer moves across boundaries, the registry emits:
  - `DragEnter` $\rightarrow$ Target evaluates payload types against its `DropEffectMask`.
  - `DragOver` $\rightarrow$ Target updates insertion coordinates (e.g. line markers in a reorderable list).
  - `DragLeave` $\rightarrow$ Target resets hover chrome to `DropTargetState::Idle`.
  - `Drop` $\rightarrow$ Target commits data and returns `DropOutcome`.

---

## 4. Common Pitfalls & Antipatterns

### 1. Capturing Live Arena `WidgetId`s in Drag Payloads
**Wrong:**
```rust
// DANGEROUS: Capturing WidgetId directly in the payload
let payload = DragPayload::from_bytes(
    source_widget_id.to_raw().to_le_bytes().to_vec(), // BAD!
    DropTypeHint::Custom("internal/widget".into()),
);
```
**Right:**
```rust
// SAFE: Capture stable domain identifiers or pre-serialized state
let payload = DragPayload::from_bytes(
    asset_id.as_bytes().to_vec(),
    DropTypeHint::Custom(MIME_MARTENSITE_NODE.into()),
);
```
Widgets can be reallocated, compacted, or destroyed while a drag is active. Storing an ephemeral `WidgetId` leads to generational index mismatches or invalid lookups upon drop.

### 2. Blocking Lazy Clipboard Closures with Network or Disk I/O
**Wrong:**
```rust
ClipboardPayload::lazy(|| {
    // BAD: Synchronous HTTP request inside clipboard provider!
    let response = reqwest::blocking::get("https://api.example.com/export").unwrap();
    response.bytes().unwrap().to_vec()
});
```
**Right:**
```rust
// Compute or cache heavy exports asynchronously ahead of time,
// or serialize existing memory data in the lazy closure.
ClipboardPayload::lazy({
    let cached_snapshot = memory_snapshot.clone();
    move || serialize_to_binary(&cached_snapshot)
}).with_deadline(Duration::from_millis(400))
```
Platform pasteboards query data synchronously when an external app requests it. Blocking on external network I/O will exceed the OS deadline, causing the destination application to hang or display an error dialog.

### 3. Mixing Physical Window Pixels with Logical Points
**Wrong:**
```rust
fn handle_drag_move(&mut self, physical_x: f64, physical_y: f64) {
    let hit = self.cached_bounds.contains(Vec2::new(physical_x as f32, physical_y as f32)); // BAD on HiDPI!
}
```
**Right:**
```rust
fn handle_drag_move(&mut self, physical_pos: (f64, f64), scale: f32) {
    let logical_pos = Vec2::new(
        (physical_pos.0 / scale as f64) as f32,
        (physical_pos.1 / scale as f64) as f32,
    );
    let hit = self.cached_bounds.contains(logical_pos);
}
```
All widget coordinates, layout boundaries, and target registries operate in **logical points**. Platform drag coordinates from winit or OLE must be divided by the window's `DpiScale` before hit-testing.

### 4. Forgetting `DropLeave` Cleanup on Window Blur or Esc
When a user presses `Escape` or moves the mouse outside the window, the OS cancels the drag operation. If a drop target only resets its visual state on `Drop`, cancelled operations leave permanent hover borders or insertion indicators on the screen. Always ensure the `DropBridge` routes cancel events to `registry.clear_active_hovers()`.

---

## Next Steps

- [Cookbook 10 — Headless Component Testing](10-headless-testing.md)
- [Cookbook 08 — Keyboard Navigation & Focus Traps](08-focus-navigation.md)
- [ADR-0016 — MIME-Aware Clipboard](../adr/ADR-0016-mime-aware-clipboard.md)
- [ADR-0017 — External and Internal Drag-and-Drop](../adr/ADR-0017-external-internal-dnd.md)
