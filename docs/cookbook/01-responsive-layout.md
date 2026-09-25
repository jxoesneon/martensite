# Cookbook 01 — Responsive Layout & Underflow Policies

Building adaptive user interfaces in desktop and embedded environments requires
handling arbitrary window dimensions, HiDPI scale changes, docking panels, and
restricted screen real-estate.

This recipe demonstrates how to design responsive UIs using Martensite's
**two-level layout architecture** (`Flex`, `Container`, `Stack`, and Taffy
arena layout) and how to manage constrained space gracefully using
**`UnderflowPolicy`**.

---

## 1. Goal

Create a responsive layout that:
1. Adapts dynamically to window resizes without clipping or layout instability.
2. Arranges top-level panels via Taffy flexbox in the `WidgetArena`.
3. Manages nested widget hierarchies cleanly via `Flex`, `Container`, and `Stack`.
4. Handles space shortages gracefully using the 7 variants of `UnderflowPolicy` (`Allow`, `Lint`, `Clip`, `Hide`, `Scrim`, `Fallback`, `Collapse`).

---

## 2. Complete Runnable Pattern

The following pattern constructs a responsive telemetry dashboard card. When space is ample, it displays a complete chart, metrics grid, and action controls; when space shrinks below designated floors, underflow policies degrade individual components systematically.

```rust
use glam::Vec2;
use kurbo::{Point, Rect as KurboRect};
use martensite::prelude::*;
use martensite::widgets::flex::{CrossAxisAlignment, MainAxisAlignment};
use martensite_core::widget::{RenderMinimum, UnderflowPolicy};
use martensite_layout::geometry::EdgeInsets;

/// A responsive telemetry panel demonstrating custom layout and underflow fallback.
pub struct TelemetryCard {
    cached_bounds: Rect,
    detailed_metrics: Flex,
    primary_label: Text,
}

impl TelemetryCard {
    pub fn new() -> Self {
        // Build internal children hierarchy
        let detailed_metrics = Flex::row()
            .gap(8.0)
            .main_axis_alignment(MainAxisAlignment::SpaceBetween)
            .cross_axis_alignment(CrossAxisAlignment::Center)
            .child(Text::new("CPU: 42%").font_size(12.0))
            .child(Text::new("RAM: 3.4GB").font_size(12.0))
            .child(Text::new("NET: 1.2MB/s").font_size(12.0));

        Self {
            cached_bounds: Rect::default(),
            detailed_metrics,
            primary_label: Text::new("System Telemetry").font_size(14.0),
        }
    }
}

impl Widget for TelemetryCard {
    fn measure(&mut self, cx: &mut LayoutContext, constraints: LayoutConstraints) -> Vec2 {
        let label_size = self.primary_label.measure(cx, constraints);
        let metrics_size = self.detailed_metrics.measure(cx, constraints);

        // Desired size: height of label + metrics + padding
        let desired_x = label_size.x.max(metrics_size.x) + cx.pt(16.0);
        let desired_y = label_size.y + metrics_size.y + cx.pt(20.0);

        Vec2::new(
            desired_x.clamp(constraints.min_size.x, constraints.max_size.x),
            desired_y.clamp(constraints.min_size.y, constraints.max_size.y),
        )
    }

    fn layout(&mut self, cx: &mut LayoutContext, bounds: Rect) {
        self.cached_bounds = bounds;
        let pad = cx.pt(8.0);

        // Position primary label at the top
        let label_h = cx.pt(18.0);
        let label_rect = Rect::new(
            bounds.origin.x + pad,
            bounds.origin.y + pad,
            bounds.size.x - (pad * 2.0),
            label_h,
        );
        cx.layout_child(&mut self.primary_label, label_rect);

        // Position detailed metrics below
        let metrics_rect = Rect::new(
            bounds.origin.x + pad,
            bounds.origin.y + pad + label_h + cx.pt(4.0),
            bounds.size.x - (pad * 2.0),
            (bounds.size.y - label_h - (pad * 2.0) - cx.pt(4.0)).max(0.0),
        );
        cx.layout_child(&mut self.detailed_metrics, metrics_rect);
    }

    // Declare an intrinsic render minimum floor and the degradation policy
    fn min_render(&self) -> RenderMinimum {
        // Requires at least 180x60pt. If allocated less, drop to Fallback mode.
        RenderMinimum::new(Vec2::new(180.0, 60.0))
            .with_policy(UnderflowPolicy::Fallback)
    }

    // Paint degraded chrome when space falls below min_render under UnderflowPolicy::Fallback
    fn paint_underflow(&self, cx: &mut PaintContext) {
        let b = cx.bounds;
        let k_rect = KurboRect::new(
            b.min_x() as f64,
            b.min_y() as f64,
            b.max_x() as f64,
            b.max_y() as f64,
        );

        // Draw compact status badge instead of complex multi-line metrics
        cx.list.push_fill_rounded_rect(k_rect, cx.pt(4.0), [45, 45, 50, 255]);
        cx.list.push_text(
            Point::new(
                (b.origin.x + cx.pt(6.0)) as f64,
                (b.origin.y + b.size.y * 0.65) as f64,
            ),
            "SYS: 42% (Compact)".to_string(),
            cx.pt(11.0),
            [220, 220, 230, 255],
        );
    }

    fn paint(&self, cx: &mut PaintContext) {
        let b = cx.bounds;
        let k_rect = KurboRect::new(
            b.min_x() as f64,
            b.min_y() as f64,
            b.max_x() as f64,
            b.max_y() as f64,
        );

        // Draw card background
        let bg_color = cx.color(TokenKey::SurfaceColor, [28, 28, 32, 255]);
        cx.list.push_fill_rounded_rect(k_rect, cx.pt(8.0), bg_color);

        // Card border
        let border_color = cx.color(TokenKey::BorderColor, [60, 60, 68, 255]);
        cx.list.push_stroke_rect(k_rect, border_color, cx.pt(1.0));
    }

    // Forward internal children to the framework
    fn child_count(&self) -> usize { 2 }
    fn child(&self, i: usize) -> Option<&dyn Widget> {
        match i {
            0 => Some(&self.primary_label),
            1 => Some(&self.detailed_metrics),
            _ => None,
        }
    }
    fn child_mut(&mut self, i: usize) -> Option<&mut dyn Widget> {
        match i {
            0 => Some(&mut self.primary_label),
            1 => Some(&mut self.detailed_metrics),
            _ => None,
        }
    }
    fn child_bounds(&self, i: usize) -> Option<Rect> {
        let pad = 8.0;
        let label_h = 18.0;
        match i {
            0 => Some(Rect::new(
                self.cached_bounds.origin.x + pad,
                self.cached_bounds.origin.y + pad,
                self.cached_bounds.size.x - (pad * 2.0),
                label_h,
            )),
            1 => Some(Rect::new(
                self.cached_bounds.origin.x + pad,
                self.cached_bounds.origin.y + pad + label_h + 4.0,
                self.cached_bounds.size.x - (pad * 2.0),
                (self.cached_bounds.size.y - label_h - (pad * 2.0) - 4.0).max(0.0),
            )),
            _ => None,
        }
    }
}

/// Setting up a responsive workspace layout in the WidgetArena
pub fn build_responsive_workspace(arena: &mut WidgetArena) -> WidgetId {
    // 1. Create child cards
    let telemetry = TelemetryCard::new();
    let card1_id = arena.insert_with_widget(
        HotNode::default(),
        Box::new(telemetry),
    );

    // 2. Wrap a second card with an instance-level underflow override
    let card2_id = arena.insert(
        HotNode::default(),
        ColdNode::new(Box::new(Text::new("Secondary Stream").font_size(14.0)))
            .with_render_minimum(
                RenderMinimum::new(Vec2::new(120.0, 30.0))
                    .with_policy(UnderflowPolicy::Collapse),
            )
            .with_debug_name("SidebarCard"),
    );

    // 3. Compose top-level page with Flex
    let main_row = Flex::row()
        .gap(12.0)
        .flex_child(Container::new().padding_uniform(8.0), 2.0) // Takes 2/3 of space
        .flex_child(Container::new().padding_uniform(8.0), 1.0); // Takes 1/3 of space

    let root_id = arena.insert_with_widget(
        HotNode::default(),
        Box::new(main_row),
    );

    arena.set_parent(card1_id, root_id);
    arena.set_parent(card2_id, root_id);

    root_id
}
```

---

## 3. The Two-Level Layout Architecture

Martensite deliberately avoids a single monolithic layout pass. It enforces a clean separation of concerns between top-level arena nodes and composite widget internals:

```
[ WidgetArena (Taffy Tree) ]
         |
         +--> Node 1: Header (Taffy Leaf, measured by Widget::measure)
         |
         +--> Node 2: Workspace Flex (Taffy Leaf, intrinsic bounds)
                   |
                   +--> [ Widget-Internal Layout: Flex::layout() ]
                             |
                             +--> child[0]: Container::layout()
                             +--> child[1]: TelemetryCard::layout()
                             +--> child[2]: Stack::layout()
```

### 1. Arena-Level Layout (`LayoutEngine`)
- Driven by `taffy::TaffyTree<WidgetId>` in `martensite-layout`.
- Top-level arena widgets registered in `WidgetArena` correspond to nodes in the Taffy tree.
- When `LayoutEngine::compute` executes:
  - **Pass 1 (Intrinsic Measurement)**: Taffy walks the tree bottom-up, querying `Widget::measure(&mut self, cx, constraints)` on leaves. Recursion is guarded by `MAX_LAYOUT_DEPTH = 512` to prevent stack overflows.
  - **Pass 2 (Definitive Positioning)**: Taffy calculates final coordinates and writes them back into each `HotNode::bounds`.

### 2. Widget-Internal Layout (`Widget::layout`)
- Composite widgets (`Flex`, `Container`, `Stack`) own their children internally as `Vec<Box<dyn Widget>>`.
- They appear as **leaf nodes** to the outer Taffy engine.
- During `Widget::layout(&mut self, cx: &mut LayoutContext, bounds: Rect)`, the container receives its screen-space bounds and computes the local positions of its internal children.
- Call `cx.layout_child(child, child_bounds)` to lay out internal children while preserving accessibility and `NodeFlags::FOCUSABLE` flags.

### 3. The Child Protocol
Internal children are not registered in the arena, but the framework reaches them through four mandatory methods on `Widget`:
- `child_count(&self) -> usize`
- `child(&self, i: usize) -> Option<&dyn Widget>`
- `child_mut(&mut self, i: usize) -> Option<&mut dyn Widget>`
- `child_bounds(&self, i: usize) -> Option<Rect>`

The framework automatically uses this protocol to:
1. **Route Events**: Topmost-first hit-testing checks `child_bounds` before forwarding events.
2. **Paint Traversal**: Recurses into internal children after painting the parent's chrome.
3. **Accessibility**: Bridges internal children into virtual AccessKit nodes.

---

## 4. Layout Primitives: Flex, Container, and Stack

### Flex (`martensite::widgets::Flex`)
Arranges children sequentially along a main axis (`FlexDirection::Row` or `FlexDirection::Column`):
- **Main Axis Distribution**: `MainAxisAlignment` (`Start`, `End`, `Center`, `SpaceBetween`, `SpaceEvenly`).
- **Cross Axis Alignment**: `CrossAxisAlignment` (`Stretch`, `Start`, `End`, `Center`).
- **Spacing**: `.gap(f32)` specifies logical point spacing between items.
- **Proportional Flexing**: Use `.flex_child(widget, weight)` where `weight > 0.0` allocates proportional shares of unused main-axis space (`0.0` uses intrinsic measure).

```rust
let toolbar = Flex::row()
    .gap(6.0)
    .main_axis_alignment(MainAxisAlignment::Start)
    .cross_axis_alignment(CrossAxisAlignment::Center)
    .child(Button::new("New"))
    .child(Button::new("Open"))
    .flex_child(Container::new(), 1.0) // Flexible spacer
    .child(Button::new("Settings"));
```

### Container (`martensite::widgets::Container`)
Wraps a single child with inner padding and optional styling:
- **Padding**: `.padding(EdgeInsets)` or `.padding_uniform(f32)`.
- **Background**: `.background(Oklab)`.
- Intrinsic measure expands by horizontal and vertical padding.

### Stack (`martensite::widgets::Stack`)
Layers multiple children along the Z-axis:
- Children render in list order (later children painted on top of earlier ones).
- **Alignment**: `StackAlignment` (`TopStart`, `TopEnd`, `BottomStart`, `BottomEnd`, `Center`, `Stretch`).
- Useful for badges, notifications, watermarks, and floating overlay controls.

---

## 5. Underflow Policies — Deep Dive

In dense interfaces (industrial dashboards, split views, mobile screens), windows or containers may be shrunk below a widget's minimum readable or operable area. Rather than letting text truncate illegibly or controls overlap destructively, Martensite provides **`UnderflowPolicy`**.

Each widget declares an intrinsic render floor via `Widget::min_render(&self) -> RenderMinimum`. Alternatively, callers can override this per node via `ColdNode::with_render_minimum`.

```rust
pub struct RenderMinimum {
    pub size: Vec2,               // Logical points floor
    pub policy: UnderflowPolicy,  // Degradation strategy
}
```

### Comparison Matrix of the 7 Underflow Policies

| Policy | Behavior on Underflow | Covers Input? | Hides from A11y? | Typical Use Case |
|---|---|:---:|:---:|---|
| **`Allow`** | Normal paint & hit-test (default). Underflow audit flags if audited. | No | No | Non-critical informational labels, decorative spacers. |
| **`Lint`** | Paints normally, but triggers a hard warning during `martensite-design-lint` or `paint_audit`. | No | No | Correctness-critical gauges where truncation breaks domain rules. |
| **`Clip`** | Scissor-clips the widget and its entire subtree to the allocated bounds. | No | No | Scrollable viewports, canvases, dense data tables. |
| **`Hide`** | Skips painting, hit-testing, and event delivery. Space remains allocated (Android `INVISIBLE` semantics). | **Yes** | **Yes** | Auxiliary toolbars or secondary buttons in fixed grid slots. |
| **`Scrim`** | Paints normally, then covers with a translucent frosted scrim overlay; blocks all input. | **Yes** | No | Readouts in panels that have become too small to operate safely. |
| **`Fallback`** | Skips standard paint; calls `Widget::paint_underflow` to draw an icon or sparkline. Input remains live. | No | No | Complex charts collapsing into mini-badges or summary icons. |
| **`Collapse`** | Frees layout space entirely (CSS `display:none`). Siblings reflow to fill the vacuum. | **Yes** | **Yes** | Collapsible sidebars, optional columns in responsive tables. |

### The Underflow Query Methods
The enum exposes three semantic queries used by layout and event routing:
```rust
assert!(UnderflowPolicy::Hide.covers_input());
assert!(UnderflowPolicy::Collapse.hides_from_a11y());
assert!(UnderflowPolicy::Fallback.enforces());
assert!(!UnderflowPolicy::Allow.enforces());
```

### Speculative Restore & Feasibility Streak in `Collapse`
The `Collapse` policy removes the node from Taffy (`display: none`), allowing remaining siblings to expand. However, if a sibling expands, the container size increases, which might make the collapsed widget fit again—potentially causing an infinite relayout loop (oscillation).

To guarantee stability:
1. `LayoutEngine` maintains a `CollapseState` for each collapsed node.
2. When space increases, the engine runs a **speculative measure**: it computes whether the node would fit *without* claiming the space yet.
3. It enforces a **feasibility streak** (consecutive frames where the element comfortably fits with a hysteresis margin) and exponential backoff before uncollapsing the node.

---

## 6. Common Mistakes & Pitfalls

### 1. Caching Geometry in `measure`
**Wrong:**
```rust
fn measure(&mut self, _cx: &mut LayoutContext, constraints: LayoutConstraints) -> Vec2 {
    self.cached_bounds = Rect::new(0.0, 0.0, constraints.max_size.x, 40.0); // BAD!
    Vec2::new(constraints.max_size.x, 40.0)
}
```
**Right:**
`measure` is an advisory probing pass and may be called multiple times with speculative constraints. Never cache final geometry or mutate state in `measure`. Always cache definitive geometry in `Widget::layout`.

### 2. Forgetting Display Scale Conversion
**Wrong:**
```rust
let padding = 16.0; // BAD: 16px on 4K is tiny!
```
**Right:**
```rust
let padding = cx.pt(16.0); // Correct: scales with display DPI factor
```
Constants baked in logical points must always be converted using `cx.pt(v)` in `layout` and `paint`.

### 3. Painting Internal Children Manually
**Wrong:**
```rust
fn paint(&self, cx: &mut PaintContext) {
    self.child.paint(cx); // BAD: duplicates painting, breaks clip stacks & provenance scopes!
}
```
**Right:**
`Widget::paint` should paint **only the widget's own chrome**. The framework's tree walker automatically visits internal children via `child_count` and `child` after `paint` returns.

### 4. Overlapping Keyline Insets
`kurbo::Rect::inset(d)` **expands** the rectangle when `d` is positive. To draw an inner border or keyline inside a bounding box, pass a **negative** inset:
```rust
let inner_keyline = k_rect.inset(-cx.pt(1.0) as f64);
```

---

## Next Steps

- [Cookbook 02 — Reactive Data Binding](02-data-binding.md)
- [Cookbook 03 — Custom Painting & Silhouettes](03-custom-painting.md)
- [Tutorial 03 — Writing a Custom Widget](../tutorials/03-custom-widget.md)
