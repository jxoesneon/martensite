# Cookbook 03 — Custom Painting & Silhouettes

Martensite pairs a high-performance vector rendering pipeline (driven by
Vello GPU compute shaders with TinySkia CPU fallback) with standards-backed
design linting (`martensite-design-lint`).

This recipe demonstrates how to author bespoke custom widgets: recording drawing
operations into `PaintList`, matching interactive hit silhouettes with `Shape`,
handling degraded underflow rendering, and tagging provenance scopes to ensure
clean design audits.

---

## 1. Goal

Author a production-grade, interactive custom widget that:
1. Implements the full `Widget` trait contract (`measure`, `layout`, `paint`, `event`, `accessibility`).
2. Records declarative vector graphics into `PaintList` using theme tokens and DPI scaling.
3. Provides exact pixel-level hit testing via `Widget::hit_shape` and non-rectangular clipping via `Widget::clip_shape`.
4. Emits `PushScope` provenance markers and tags metadata for `martensite-design-lint` (`@level`, `@kpi`, `@lint:allow`).

---

## 2. Complete Runnable Pattern

The following example implements an interactive circular **`DialControl`** (an instrumentation knob / rotary potentiometer). It renders an arc gauge with a continuous squircle/circle silhouette, clamps user pointer drags, enforces minimum target sizes, and tags itself for ISA-101 HMI compliance.

```rust
use glam::Vec2;
use kurbo::{BezPath, Circle, Point, Rect as KurboRect, RoundedRect};
use martensite::prelude::*;
use martensite_core::shape::{CornerRadii, CornerStyle, CornerStyles, Shape};
use martensite_core::widget::{RenderMinimum, UnderflowPolicy};

/// A custom circular rotary dial control with continuous silhouette hit-testing.
pub struct DialControl {
    /// Normalized value: 0.0 ..= 1.0
    value: f32,
    /// Drag tracking state
    is_dragging: bool,
    /// Cached layout bounds
    cached_bounds: Rect,
}

impl DialControl {
    pub fn new(initial_value: f32) -> Self {
        Self {
            value: initial_value.clamp(0.0, 1.0),
            is_dragging: false,
            cached_bounds: Rect::default(),
        }
    }

    pub fn value(&self) -> f32 {
        self.value
    }

    pub fn set_value(&mut self, val: f32) {
        self.value = val.clamp(0.0, 1.0);
    }
}

impl Widget for DialControl {
    // 1. MEASURE: Report desired size (default 64x64pt knob)
    fn measure(&mut self, cx: &mut LayoutContext, constraints: LayoutConstraints) -> Vec2 {
        let size = cx.pt(64.0);
        Vec2::new(
            size.clamp(constraints.min_size.x, constraints.max_size.x),
            size.clamp(constraints.min_size.y, constraints.max_size.y),
        )
    }

    // 2. LAYOUT: Cache final geometry
    fn layout(&mut self, cx: &mut LayoutContext, bounds: Rect) {
        self.cached_bounds = bounds;
        // Mark node focusable so keyboard navigation can land here
        cx.hot.flags |= NodeFlags::FOCUSABLE;
    }

    // 3. HIT SILHOUETTE: Accept clicks only inside the circle, not the bounding box corners
    fn hit_shape(&self) -> Option<Shape> {
        let center = Vec2::new(self.cached_bounds.width() / 2.0, self.cached_bounds.height() / 2.0);
        let radius = (self.cached_bounds.width().min(self.cached_bounds.height()) / 2.0) - 2.0;
        Some(Shape::Circle { center, radius })
    }

    // 4. CHILD CLIP SHAPE: Any overlays or children are clipped to this squircle/circle
    fn clip_shape(&self) -> Option<Shape> {
        Some(Shape::Corners {
            radii: CornerRadii::uniform(12.0),
            styles: CornerStyles::uniform(CornerStyle::Squircle),
        })
    }

    // 5. UNDERFLOW DEGRADATION: If allocated under 32x32pt, degrade to sparkline fallback
    fn min_render(&self) -> RenderMinimum {
        RenderMinimum::new(Vec2::new(32.0, 32.0))
            .with_policy(UnderflowPolicy::Fallback)
    }

    fn paint_underflow(&self, cx: &mut PaintContext) {
        let b = cx.bounds;
        let k_rect = KurboRect::new(
            b.min_x() as f64,
            b.min_y() as f64,
            b.max_x() as f64,
            b.max_y() as f64,
        );
        // Fallback: simple numeric pill
        cx.list.push_fill_rounded_rect(k_rect, cx.pt(4.0), [40, 40, 48, 255]);
        cx.list.push_text(
            Point::new((b.origin.x + cx.pt(4.0)) as f64, (b.origin.y + b.size.y * 0.7) as f64),
            format!("{:.0}%", self.value * 100.0),
            cx.pt(10.0),
            [255, 255, 255, 255],
        );
    }

    // 6. PROVENANCE & DESIGN LINT TAGS: Tag as Level-2 HMI control with KPI semantics
    fn debug_name(&self) -> &'static str {
        "DialControl@level:2@kpi"
    }

    // 7. INPUT HANDLING: Pointer drags & accessibility clicks
    fn event(&mut self, cx: &mut EventContext) -> EventResponse {
        match cx.event {
            WidgetEvent::PointerPressed { position, button: PointerButton::Primary, .. } => {
                self.is_dragging = true;
                // Calculate angle from center to update value
                let center_x = cx.bounds.origin.x + cx.bounds.width() / 2.0;
                let center_y = cx.bounds.origin.y + cx.bounds.height() / 2.0;
                let dx = position.x - center_x;
                let dy = position.y - center_y;
                let angle = dy.atan2(dx); // -PI to PI
                // Map angle to 0.0 ..= 1.0
                let normalized = (angle + std::f32::consts::PI) / (std::f32::consts::PI * 2.0);
                self.set_value(normalized);
                EventResponse::CapturePointer
            }
            WidgetEvent::PointerMoved { position } if self.is_dragging => {
                let center_x = cx.bounds.origin.x + cx.bounds.width() / 2.0;
                let center_y = cx.bounds.origin.y + cx.bounds.height() / 2.0;
                let dx = position.x - center_x;
                let dy = position.y - center_y;
                let angle = dy.atan2(dx);
                let normalized = (angle + std::f32::consts::PI) / (std::f32::consts::PI * 2.0);
                self.set_value(normalized);
                EventResponse::RequestRepaint
            }
            WidgetEvent::PointerReleased { button: PointerButton::Primary, .. } if self.is_dragging => {
                self.is_dragging = false;
                EventResponse::ReleasePointer
            }
            WidgetEvent::SemanticAction(SemanticAction::Increment) => {
                self.set_value(self.value + 0.05);
                EventResponse::RequestRepaint
            }
            WidgetEvent::SemanticAction(SemanticAction::Decrement) => {
                self.set_value(self.value - 0.05);
                EventResponse::RequestRepaint
            }
            _ => EventResponse::Ignored,
        }
    }

    // 8. ACCESSIBILITY: Expose semantic role, value, and adjustment actions to screen readers
    fn accessibility(&self, node: &mut accesskit::Node) {
        node.set_role(accesskit::Role::Slider);
        node.set_label("Rotary Gain Dial");
        node.set_numeric_value(self.value as f64);
        node.set_min_numeric_value(0.0);
        node.set_max_numeric_value(1.0);
        node.set_numeric_value_step(0.05);
        node.add_action(accesskit::Action::Increment);
        node.add_action(accesskit::Action::Decrement);
    }

    // 9. PAINT: Declarative drawing operations using PaintList
    fn paint(&self, cx: &mut PaintContext) {
        let b = cx.bounds;
        let center_x = (b.origin.x + b.size.x / 2.0) as f64;
        let center_y = (b.origin.y + b.size.y / 2.0) as f64;
        let radius = (b.size.x.min(b.size.y) as f64 / 2.0) - cx.ptf(4.0);

        // Track background: dial face
        let face_color = cx.color(TokenKey::SurfaceColor, [32, 34, 40, 255]);
        let dial_circle = Circle::new(Point::new(center_x, center_y), radius);
        cx.list.push_fill_shape(
            KurboRect::new(b.min_x() as f64, b.min_y() as f64, b.max_x() as f64, b.max_y() as f64),
            &Shape::Circle {
                center: Vec2::new(b.size.x / 2.0, b.size.y / 2.0),
                radius: radius as f32,
            },
            face_color,
        );

        // Dial rim border (maintaining 3:1 non-text contrast against background)
        let rim_color = cx.color(TokenKey::BorderColor, [90, 95, 110, 255]);
        cx.list.push_stroke_shape(
            KurboRect::new(b.min_x() as f64, b.min_y() as f64, b.max_x() as f64, b.max_y() as f64),
            &Shape::Circle {
                center: Vec2::new(b.size.x / 2.0, b.size.y / 2.0),
                radius: radius as f32,
            },
            rim_color,
            cx.pt(2.0),
        );

        // Indicator needle
        let angle = (self.value * std::f32::consts::PI * 2.0) - std::f32::consts::PI;
        let needle_len = radius * 0.75;
        let needle_end_x = center_x + (angle.cos() as f64 * needle_len);
        let needle_end_y = center_y + (angle.sin() as f64 * needle_len);

        let mut needle_path = BezPath::new();
        needle_path.move_to(Point::new(center_x, center_y));
        needle_path.line_to(Point::new(needle_end_x, needle_end_y));

        let accent_color = cx.color(TokenKey::PrimaryColor, [80, 140, 255, 255]);
        cx.list.push_stroke_path(needle_path, accent_color, cx.pt(3.0));

        // Center hub
        let hub_color = [240, 240, 250, 255];
        let hub = Circle::new(Point::new(center_x, center_y), cx.ptf(4.0));
        cx.list.push_path(hub.to_path(0.1), hub_color);

        // Center readout text (4.5:1 WCAG contrast against face_color)
        let text_y = center_y + (radius * 0.45);
        cx.list.push_text(
            Point::new(center_x - cx.ptf(12.0), text_y),
            format!("{:.0}%", self.value * 100.0),
            cx.pt(11.0),
            [240, 240, 245, 255],
        );
    }
}
```

---

## 3. Emitting `PaintList` Commands

The `PaintList` is an append-only linear command buffer. It contains zero GPU device logic; instead, it is serialized and rendered directly by either **Vello** (via compute shaders writing to GPU storage buffers) or **TinySkia** (software scanline rasterizer).

### Common Paint Operations

| Paint Method | Description |
|---|---|
| `cx.list.push_fill_rect(rect, rgba)` | Solid axis-aligned rectangle. |
| `cx.list.push_stroke_rect(rect, rgba, width)` | Outlined rectangle with specified stroke width. |
| `cx.list.push_fill_rounded_rect(rect, r, rgba)` | Rectangle with uniform corner radius. |
| `cx.list.push_fill_shape(bounds, shape, rgba)` | Fills a `Shape` (`Pill`, `Squircle`, `Circle`, `Path`). |
| `cx.list.push_stroke_shape(bounds, shape, rgba, w)` | Strokes a `Shape`. |
| `cx.list.push_path(bezpath, rgba)` | Fills an arbitrary Bézier outline. |
| `cx.list.push_stroke_path(bezpath, rgba, w)` | Strokes an arbitrary Bézier outline. |
| `cx.list.push_text(point, string, size, rgba)` | Emits text run at baseline point. |
| `cx.list.push_blurred_rect(rect, radius, rgba)` | Two-pass Gaussian blurred rectangle (shadows, backdrops). |
| `cx.list.push_fill_linear_gradient_path(...)` | Bézier path filled with linear color stops. |
| `cx.list.push_clip(rect)` / `cx.list.pop_clip()` | Scissor-clip stack discipline. |

### Scaling & Coordinates
- Layout bounds (`cx.bounds`) and paint coordinates are in **device pixels**.
- Logical points must be multiplied by display scale: use `cx.pt(val: f32)` or `cx.ptf(val: f64)`.
- Theme tokens are queried through `cx.color(key, fallback)` and `cx.dim(key, fallback)`.

---

## 4. Interactive Silhouettes with `Shape`

Standard UI toolkits hit-test widgets using rectangular bounding boxes. In dense cockpits, circular knobs or chamfered tags lead to **hit-test leakage**: clicking empty space in the corner of a bounding box activates the wrong underlying widget.

Martensite solves this via `Shape`:
```rust
pub enum Shape {
    Rect,
    Corners { radii: CornerRadii, styles: CornerStyles },
    Pill,
    Ellipse,
    Circle { center: Vec2, radius: f32 },
    Path(BezPath),
}
```

### Corner Styles
The `CornerStyle` vocabulary provides built-in support for advanced modern silhouettes:
- `CornerStyle::Round`: Standard CSS quarter-ellipse.
- `CornerStyle::Cut`: 45-degree chamfer / bevel.
- `CornerStyle::Notch`: Inverted square notch.
- `CornerStyle::Scoop`: Inverted concave quarter-circle.
- `CornerStyle::Squircle`: Continuous $n=4$ superellipse ($|x|^4 + |y|^4 = r^4$), matching iOS continuous corners.

### Narrow-Phase Hit Testing
By overriding `Widget::hit_shape(&self) -> Option<Shape>`, you provide a continuous mathematical outline:
```rust
fn hit_shape(&self) -> Option<Shape> {
    Some(Shape::Circle { center: self.center(), radius: self.radius() })
}
```
When a pointer event arrives, the window hit-tester tests `bounds.contains(pos)` (broad phase) followed by `shape.contains(pos)` (narrow phase).

---

## 5. Scope Tagging and Design-Lint

Every paint command in Martensite is nested between `PaintCommand::PushScope` and `PopScope` markers emitted by `WidgetArena::build_paint_list`:

```rust
PaintCommand::PushScope {
    id: Some(WidgetId),
    name: "DialControl@level:2@kpi",
    bounds: Rect,
}
```

The `martensite-design-lint` engine replays this command stream into a `LintScene` and verifies compliance against standards (WCAG 2.2, ISA-101, Fitts's Law, Tufte).

### Inline Annotations via `debug_name`
You can annotate widgets or suppress false positives directly in `Widget::debug_name`:

| Annotation Suffix | Meaning |
|---|---|
| `@level:1` .. `@level:4` | Declares an ISA-101 HMI display level (Level 1: Overview, Level 2: Unit Control, Level 3: Detail, Level 4: Diagnostics). |
| `@alarm` | Identifies an alarm indicator subject to ISA-18.2 color exclusivity rules. |
| `@priority:1` .. `@priority:4` | Designates operational alarm priority. |
| `@kpi` | Exempts key performance indicators from excessive density warnings. |
| `@destructive` | Enforces two-step confirmation or explicit visual distinction. |
| `@lint:target-size` | Suppresses `target-size` rule for this widget subtree. |
| `@lint:standard:wcag` | Suppresses all WCAG findings for this widget. |
| `@lint:all` | Suppresses all design-lint checks for this subtree. |

### Designing for a Clean Lint Pass
To pass `cargo martensite lint` with zero findings:
1. **Text Contrast**: Text on colored backgrounds must achieve at least **4.5:1** contrast (WCAG AA).
2. **Non-Text Contrast**: Interactive borders, toggles, and sliders must achieve at least **3:1** against adjacent surfaces.
3. **Touch/Pointer Target Size**: Interactive leaves must measure at least $24 \times 24$ pt on desktop or $44 \times 44$ pt on touch platforms.
4. **Designed Overhang**: If a badge or focus ring bleeds outside your layout bounds, declare `Widget::paint_extent(&self) -> Option<Rect>` so the audit doesn't report container overflow.

---

## 6. Common Mistakes & Pitfalls

### 1. Insetting kurbo Rectangles Backwards
In `kurbo`, `rect.inset(10.0)` **expands** the rectangle by 10 points outward!
To draw a stroke inside the container boundaries, supply a **negative** value:
```rust
// Shrink the rectangle by 1 pixel on all sides to fit the inner keyline:
let stroke_rect = k_rect.inset(-cx.ptf(1.0));
```

### 2. Leaking Clips onto Sibling Widgets
Every call to `cx.list.push_clip(rect)` or `push_clip_shape` MUST have a matching `cx.list.pop_clip()` before `paint()` returns. An unbalanced clip stack will clip all subsequently painted widgets on the screen.

### 3. Hardcoding Hex Colors Instead of Design Tokens
Avoid hardcoding raw RGBA arrays like `[40, 120, 255, 255]`. Hardcoded colors do not respond to theme switching (dark/light mode) or high-contrast accessibility modes. Always query `cx.color(TokenKey::PrimaryColor, fallback)`.

---

## Next Steps

- [Cookbook 01 — Responsive Layout](01-responsive-layout.md)
- [Cookbook 02 — Reactive Data Binding](02-data-binding.md)
- [Tutorial 04 — Accessibility Validation](../tutorials/04-accessibility.md)
- [Design Standards Index](../design-standards/README.md)
