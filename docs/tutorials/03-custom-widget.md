# Tutorial 3 — Writing a custom widget

Every Martensite widget implements `martensite_core::Widget`. This
tutorial builds a complete `CounterBadge` — a clickable pill that
increments a number — and explains the two-pass layout contract the
framework enforces.

Dependencies for a crate defining widgets (versions matching v0.17.0):

```toml
[dependencies]
martensite-core = "0.20.1"
glam = "0.33"
kurbo = "0.13"
accesskit = "0.25"
```

## 1. The contract

```rust
pub trait Widget: Send + Sync + 'static {
    fn measure(&mut self, cx: &mut LayoutContext, constraints: LayoutConstraints) -> Vec2;
    fn layout(&mut self, cx: &mut LayoutContext, bounds: Rect);
    // optional overrides: event, paint, accessibility, tick,
    // a11y_prepare, a11y_fixup, sync_overlay, clips_children,
    // child_count, child, child_mut, child_bounds
}
```

Two methods are mandatory:

- **`measure`** — given `LayoutConstraints { min_size, max_size }`
  (`glam::Vec2` bounds), return the widget's desired size. Called during
  the measure pass; may be called several times with different
  constraints. Do **not** cache final geometry here.
- **`layout`** — receives the widget's final screen-space `Rect`.
  Cache it (widgets keep their own `cached_bounds: Rect` field — see
  `Button`/`Container`); this is what paint, hit-testing, and
  `child_bounds` use later. *"Two-pass layout finality"*: after
  `layout` returns, the widget's geometry is fixed for the frame.

Optional overrides you will almost always want:

- **`paint(&self, cx: &mut PaintContext)`** — record drawing commands
  into `cx.list` (a `PaintList`). `cx.bounds` is the same rect `layout`
  received. Paint only *your own chrome* — internal children are
  painted by the framework's tree walk, not by you.
- **`event(&mut self, cx: &mut EventContext) -> EventResponse`** —
  handle a `WidgetEvent`. Default implementation forwards to internal
  children (bounds-gated, topmost-first) and returns `Ignored` for a
  leaf.
- **`accessibility(&self, node: &mut accesskit::Node)`** — populate the
  AccessKit node (role, label, actions). Default is a no-op.

## 2. The widget

```rust
use glam::Vec2;
use kurbo::Shape as _; // brings `into_path` into scope for RoundedRect
use martensite_core::{
    EventContext, EventResponse, LayoutConstraints, LayoutContext,
    PaintContext, PointerButton, Rect, Widget, WidgetEvent,
};

/// A clickable pill that counts presses.
pub struct CounterBadge {
    count: u32,
    /// Geometry assigned by the last `layout` call — paint and
    /// hit-testing read this, never `measure`.
    cached_bounds: Rect,
}

impl CounterBadge {
    pub fn new() -> Self {
        Self {
            count: 0,
            cached_bounds: Rect::new(0.0, 0.0, 0.0, 0.0),
        }
    }

    pub fn count(&self) -> u32 {
        self.count
    }
}

const FACE: [u8; 4] = [64, 64, 160, 255];
const INK: [u8; 4] = [255, 255, 255, 255];

impl Widget for CounterBadge {
    fn measure(&mut self, _cx: &mut LayoutContext, constraints: LayoutConstraints) -> Vec2 {
        // Desired size: fixed pill, clamped to what we're offered.
        Vec2::new(
            96.0_f32.min(constraints.max_size.x.max(0.0)),
            28.0_f32.min(constraints.max_size.y.max(0.0)),
        )
    }

    fn layout(&mut self, _cx: &mut LayoutContext, bounds: Rect) {
        self.cached_bounds = bounds;
    }

    fn event(&mut self, cx: &mut EventContext) -> EventResponse {
        match cx.event {
            WidgetEvent::PointerReleased {
                button: PointerButton::Primary,
                ..
            } => {
                self.count += 1;
                // `RequestRepaint` marks the node DIRTY_PAINT and stops
                // propagation — the next frame repaints the badge.
                EventResponse::RequestRepaint
            }
            // Assistive-technology activation arrives through the same
            // event pipeline as `SemanticAction`.
            WidgetEvent::SemanticAction(martensite_core::SemanticAction::Click) => {
                self.count += 1;
                EventResponse::RequestRepaint
            }
            _ => EventResponse::Ignored,
        }
    }

    fn accessibility(&self, node: &mut accesskit::Node) {
        node.set_role(accesskit::Role::Button);
        node.set_label("counter badge");
        node.add_action(accesskit::Action::Click);
        node.set_value(format!("{}", self.count));
    }

    fn paint(&self, cx: &mut PaintContext) {
        let b = cx.bounds;
        let rect = kurbo::Rect::new(
            f64::from(b.min_x()),
            f64::from(b.min_y()),
            f64::from(b.max_x()),
            f64::from(b.max_y()),
        );
        let pill = kurbo::RoundedRect::from_rect(rect, b.size.y as f64 / 2.0).into_path(0.1);
        cx.list.push_path(pill, FACE);
        cx.list.push_text(
            kurbo::Point::new(
                f64::from(b.origin.x + 12.0),
                f64::from(b.origin.y + b.size.y / 2.0 + 4.0),
            ),
            format!("{}", self.count),
            13.0,
            INK,
        );
    }
}
```

`accesskit::Node` methods used here (`set_role`, `set_label`,
`add_action`, `set_value`) are the upstream AccessKit builder API —
`martensite-core` passes you the real node it will emit.

## 3. Putting it in the arena

Widgets live in the `WidgetArena`; the arena hands back a generational
`WidgetId`:

```rust
use martensite_core::{ColdNode, HotNode, WidgetArena};

let mut arena = WidgetArena::new();
let id = arena.insert_with_widget(HotNode::default(), Box::new(CounterBadge::new()));
assert!(arena.is_alive(id));

// With accessibility metadata:
let named = arena.insert(
    HotNode::default(),
    ColdNode::new(Box::new(CounterBadge::new()))
        .with_role(accesskit::Role::Button)
        .with_a11y_name("click counter"),
);
assert!(arena.is_alive(named));
```

`WidgetId` is a `(slot, generation)` handle — stale ids from removed
widgets fail `is_alive` instead of aliasing a recycled widget.

## 4. Container widgets: the internal-children protocol

If your widget manages children internally (the way `Flex` and
`Container` do), keep `Vec<Box<dyn Widget>>` and implement four more
methods:

```rust
fn child_count(&self) -> usize { self.children.len() }
fn child(&self, i: usize) -> Option<&dyn Widget> {
    self.children.get(i).map(|c| &**c as &dyn Widget)
}
fn child_mut(&mut self, i: usize) -> Option<&mut dyn Widget> {
    self.children.get_mut(i).map(|c| &mut **c as &mut dyn Widget)
}
fn child_bounds(&self, i: usize) -> Option<Rect> {
    self.cached_child_bounds.get(i).copied()
}
```

The contract:

- Internal children are **not** arena nodes — the framework reaches
  them exclusively through this protocol for event dispatch, paint
  traversal, and AccessKit emission.
- `measure` calls `child.measure(cx, tighter_constraints)` and folds
  the results (see `Container::measure`, which subtracts padding).
- `layout` computes each child's rect and calls `child.layout(cx, rect)`,
  storing the rects for `child_bounds` — the event router bounds-gates
  positional events on them.
- You do **not** paint children yourself; the framework recurses after
  your `paint` returns. Set `clips_children() -> true` if descendants
  must not draw outside your bounds (scrollable regions).
- `event`'s default implementation already forwards topmost-first; a
  pure container can leave it unimplemented.

## 5. Behavior reference (from real widgets)

| Need | How |
| --- | --- |
| Request keyboard focus on click | return `EventResponse::CaptureFocus` and set `NodeFlags::FOCUSABLE` on `cx.hot.flags` in `layout` |
| Keep tracking a drag outside bounds | return `EventResponse::CapturePointer`, later `ReleasePointer` (slider/scroll thumbs) |
| Animate per frame | override `tick(&mut self, dt: Duration) -> bool`; return `true` while repainting |
| AT activation (screen-reader "click") | match `WidgetEvent::SemanticAction(SemanticAction::Click)` |
| `aria-controls`/`aria-describedby` | `a11y_fixup` — descendant `NodeId`s are minted after `accessibility` runs |
| Popups (dropdown listbox, tooltip) | `sync_overlay` + `martensite_core::OverlayLayer` |

## Next steps

- [Tutorial 4 — Accessibility validation](04-accessibility.md)
