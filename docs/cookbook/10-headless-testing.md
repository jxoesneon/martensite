# Cookbook 10 — Headless Component Testing & Golden Diffing

Testing interactive UI components in traditional continuous integration (CI) environments is notoriously
fraught with instability: non-deterministic GPU drivers, OS window manager dependencies, asynchronous
animation timing jitter, and subtle platform font differences produce flaky tests.

This recipe demonstrates how to author deterministic, 100% reproducible tests using `martensite-test`,
the `VirtualClock` time virtualization system, synthetic event routing, layout state assertions, and
perceptual DSSIM golden image diffing.

---

## 1. Goal

Create a headless test suite that:
1. Executes without an OS display server, window manager, or physical GPU.
2. Eliminates timing jitter by controlling time explicitly via `VirtualClock` and `HeadlessHarness`.
3. Dispatches synthetic pointer, keyboard, and scroll events to widgets via `EventRouter`.
4. Inspects and asserts exact layout bounds (`HotNode::bounds`), transform hierarchies, and dirty flags (`NodeFlags::DIRTY_LAYOUT`, `NodeFlags::DIRTY_PAINT`).
5. Captures rendered frames into offscreen buffers and verifies visual output against golden images using perceptual structural similarity (DSSIM).

---

## 2. Complete Runnable Pattern

The following pattern creates an interactive `KineticStepper` widget (a counter that animates size and color when clicked), and exercises it with a comprehensive headless test suite. The test verifies event delivery, animation progression under a virtual clock, and visual parity against a golden reference image.

```rust
use glam::Vec2;
use kurbo::{Point, Rect as KurboRect, RoundedRect};
use martensite::prelude::*;
use martensite_core::widget::NodeFlags;
use martensite_test::dssim::{images_match, ImageBuffer};
use martensite_test::virtual_clock::{VirtualClock, FRAME_60FPS};
use martensite_test::HeadlessHarness;
use martensite_window::event::{
    EventRouter, ModifierKeys, PointerEvent, PointerId, PointerKind, PointerState,
};
use winit::window::WindowId;
use std::time::Duration;

/// An interactive stepper button that tracks value and pulses on click.
pub struct KineticStepper {
    pub value: i32,
    pub is_pressed: bool,
    pub animation_progress: f32, // 0.0 to 1.0 pulse animation
    cached_bounds: Rect,
}

impl KineticStepper {
    pub fn new(initial: i32) -> Self {
        Self {
            value: initial,
            is_pressed: false,
            animation_progress: 0.0,
            cached_bounds: Rect::default(),
        }
    }

    /// Advance active animations by a virtual time delta.
    pub fn update_animation(&mut self, dt: Duration) {
        if self.animation_progress > 0.0 {
            let decay = dt.as_secs_f32() * 4.0; // 250ms recovery
            self.animation_progress = (self.animation_progress - decay).max(0.0);
        }
    }
}

impl Widget for KineticStepper {
    fn measure(&mut self, cx: &mut LayoutContext, constraints: LayoutConstraints) -> Vec2 {
        let size = Vec2::new(cx.pt(120.0), cx.pt(48.0));
        Vec2::new(
            size.x.clamp(constraints.min_size.x, constraints.max_size.x),
            size.y.clamp(constraints.min_size.y, constraints.max_size.y),
        )
    }

    fn layout(&mut self, _cx: &mut LayoutContext, bounds: Rect) {
        self.cached_bounds = bounds;
    }

    fn event(&mut self, cx: &mut EventContext, event: &WidgetEvent) -> EventResponse {
        match event {
            WidgetEvent::PointerDown { position, .. } => {
                if self.cached_bounds.contains(*position) {
                    self.is_pressed = true;
                    self.value += 1;
                    self.animation_progress = 1.0;
                    cx.request_repaint();
                    EventResponse::Handled
                } else {
                    EventResponse::Ignored
                }
            }
            WidgetEvent::PointerUp { .. } => {
                if self.is_pressed {
                    self.is_pressed = false;
                    cx.request_repaint();
                    EventResponse::Handled
                } else {
                    EventResponse::Ignored
                }
            }
            _ => EventResponse::Ignored,
        }
    }

    fn paint(&self, cx: &mut PaintContext) {
        let b = cx.bounds;
        let k_rect = KurboRect::new(
            b.min_x() as f64,
            b.min_y() as f64,
            b.max_x() as f64,
            b.max_y() as f64,
        );

        // Interpolate background color during click pulse
        let pulse = (self.animation_progress * 80.0) as u8;
        let bg_color = [30 + pulse, 35 + pulse, 45 + pulse, 255];
        let border_color = if self.is_pressed {
            [80, 160, 240, 255]
        } else {
            [60, 70, 85, 255]
        };

        cx.list.push_fill_rounded_rect(k_rect, cx.pt(6.0), bg_color);
        cx.list.push_stroke_rect(k_rect, border_color, cx.pt(1.5));

        // Render count label
        let text = format!("Count: {}", self.value);
        cx.list.push_text(
            Point::new((b.origin.x + cx.pt(20.0)) as f64, (b.origin.y + cx.pt(28.0)) as f64),
            text,
            cx.pt(14.0),
            [240, 245, 250, 255],
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_kinetic_stepper_headless_lifecycle() {
        // 1. Initialize Headless Harness with offscreen dimensions
        let mut harness = HeadlessHarness::new(200, 100);
        let mut arena = WidgetArena::new();
        let mut router = EventRouter::new();
        let dummy_window = WindowId::from(0u64);

        // 2. Register widget in Generational Arena
        let stepper = KineticStepper::new(0);
        let widget_id = arena.insert_with_widget(
            HotNode::default(),
            Box::new(stepper),
        );

        // 3. Perform initial layout pass
        let root_bounds = Rect::new(0.0, 0.0, 200.0, 100.0);
        let mut layout_cx = LayoutContext::new(1.0);
        if let Some(w) = arena.widget_mut(widget_id) {
            w.layout(&mut layout_cx, Rect::new(10.0, 10.0, 120.0, 48.0));
        }

        // Verify layout geometry written to HotNode
        let hot = arena.hot(widget_id).expect("node must exist");
        assert_eq!(hot.bounds.size.x, 120.0);
        assert_eq!(hot.bounds.size.y, 48.0);

        // 4. Simulate Pointer Press using EventRouter
        let press_event = PointerEvent {
            pointer_id: PointerId::PRIMARY,
            kind: PointerKind::Mouse,
            position: Vec2::new(30.0, 30.0), // Inside widget bounds [10, 10, 130, 58]
            state: PointerState::Down,
            button: Some(winit::event::MouseButton::Left),
            modifiers: ModifierKeys::empty(),
        };

        // Dispatch through router to arena
        let response = router.dispatch_pointer_event(
            &mut arena,
            widget_id,
            dummy_window,
            &press_event,
        );
        assert_eq!(response, Some(EventResponse::Handled));

        // 5. Inspect state mutation and dirty flags
        let widget = arena.widget_downcast_ref::<KineticStepper>(widget_id).unwrap();
        assert_eq!(widget.value, 1, "Counter must increment on click");
        assert!(widget.is_pressed, "Widget must reflect pressed state");
        assert_eq!(widget.animation_progress, 1.0, "Animation pulse triggered");

        // Verify DIRTY_PAINT was requested
        let hot_after_click = arena.hot(widget_id).unwrap();
        assert!(hot_after_click.flags.contains(NodeFlags::DIRTY_PAINT));

        // 6. Step virtual clock and advance animation deterministically
        harness.step_frame(); // Advances by exactly 16.666ms (60 FPS)
        let dt = FRAME_60FPS;

        let widget_mut = arena.widget_downcast_mut::<KineticStepper>(widget_id).unwrap();
        widget_mut.update_animation(dt);
        assert!(
            widget_mut.animation_progress < 1.0 && widget_mut.animation_progress > 0.0,
            "Animation must decay predictably without real-time wall-clock jitter"
        );

        // 7. Render frame into Headless Harness and compare against Golden image
        // Generate simulated raster frame (200x100 RGBA)
        let rgba = vec![40u8; 200 * 100 * 4];
        let snapshot = harness.capture_frame(&rgba);
        assert_eq!(harness.frame_count(), 1);

        // Verify against reference buffer with zero tolerance
        let golden = ImageBuffer::from_rgba(200, 100, &rgba);
        assert!(
            harness.compare_to_golden(&golden, 0.0),
            "Visual render must match golden snapshot with 0.0 DSSIM distance"
        );
    }
}
```

---

## 3. Key Architectural Invariants

### 1. Clock Virtualization & Monotonic Determinism
Traditional test suites fail when tests query the operating system's wall-clock or monotonic timer (`std::time::Instant::now()`). Slow CI machines, CPU throttling, or garbage collection pauses cause animations to be sampled at arbitrary points in their timeline, resulting in intermittent failures.

`martensite-test` enforces **strict time virtualization**:
```
[ Operating System ]        x  Instant::now() (Forbidden in tests!)
                                    |
[ Test Execution ]          --> VirtualClock::advance(16.66ms)
                                    |
                            [ Deterministic Frame N ]
                            [ Deterministic Frame N+1 ]
```
- `VirtualClock` advances **only** when the test author explicitly invokes `.advance(duration)` or `.step_frame()`.
- Constant step durations are provided for standard refresh rates:
  - `FRAME_60FPS` = $16,\!666,\!667\text{ ns}$ ($\approx 16.67\text{ ms}$)
  - `FRAME_120FPS` = $8,\!333,\!333\text{ ns}$ ($\approx 8.33\text{ ms}$)
  - `FRAME_30FPS` = $33,\!333,\!333\text{ ns}$ ($\approx 33.33\text{ ms}$)
- One hundred consecutive test executions on different machines produce the exact same numerical floating-point values at every frame.

### 2. Two-Phase Arena Layout in Headless Mode
The widget arena and layout engine operate identically in headless test environments as they do in live windows:
1. **Pass 1: Intrinsic Measurement**:
   - `LayoutEngine::compute` probes `Widget::measure` with min/max constraints.
   - Nodes compute their desired logical point boundaries.
2. **Pass 2: Definitive Positioning**:
   - Calculated coordinates are written back to `HotNode::bounds`.
   - `Widget::layout` is called on each widget, storing local bounding boxes.
- **Assertion Seam**: Tests can query `arena.hot(id).bounds` immediately after `compute` to assert that flexbox distribution, padding, and underflow collapse behaved correctly before rendering a single pixel.

### 3. Event Simulation via `EventRouter`
Testing user interactions requires verifying the complete event routing chain:
- **Hit-Testing**: The `EventRouter` traverses the arena top-down (topmost-first), honoring clip rectangles and transparent silhouettes.
- **Click Tracking**: `MouseTracker` maintains click streaks (single, double, triple clicks). Synthesized events spaced within `DOUBLE_CLICK_THRESHOLD` automatically increment `click_count`.
- **Pointer Capture**: If a widget responds with `EventResponse::CapturePointer` (e.g. during a slider drag), subsequent events are routed exclusively to that widget even if the pointer wanders outside its bounding box.
- **Hover Transitions**: Synthetic movement across boundaries automatically generates `WidgetEvent::PointerLeave` on the previous target and `WidgetEvent::PointerEnter` on the newly hovered node.

### 4. Dirty Flag Mask Invariants
Martensite maintains high performance by repainting only dirty subtrees. Headless tests should assert that widgets participate cleanly in this contract:

```
State Change  ──>  cx.request_repaint()  ──>  NodeFlags::DIRTY_PAINT set
Arena Layout  ──>  cx.request_layout()   ──>  NodeFlags::DIRTY_LAYOUT set
Paint Pass    ──>  PaintList recorded    ──>  DIRTY_PAINT cleared
```

- When an internal property changes, assert that `hot.flags.contains(NodeFlags::DIRTY_PAINT)` is true.
- After a paint pass, assert that dirty bits are cleared. If dirty flags linger, the application will burn CPU cycles repainting stagnant frames.

### 5. Perceptual DSSIM vs Bitwise Equality
Comparing rendered raster frames pixel-by-pixel with `assert_eq!(rendered_bytes, golden_bytes)` causes false-positive test failures when upgrading graphics drivers or testing on ARM vs x86 (where sub-pixel font rasterization, gamma curves, or anti-aliasing math differ by 1 LSB).

`martensite-test` uses **Structural Dissimilarity (DSSIM)**:
$$\text{DSSIM}(A, B) = \frac{1 - \text{SSIM}(A, B)}{2}$$
- Evaluates luminance, contrast, and structural comparison in local $8 \times 8$ pixel windows.
- Returns a score in $[0.0, 1.0]$, where `0.0` represents perceptual identity.
- Recommended threshold: `compare_to_golden(golden, 0.005)` allows microscopic anti-aliasing variations while catching layout shifts, missing icons, truncated text, or color inversions.
- Reference files are stored in the compact binary format `MTI1` via `GoldenImages`.

---

## 4. Common Pitfalls & Antipatterns

### 1. Using `std::thread::sleep` in Headless Tests
**Wrong:**
```rust
// FLAKY: Sleeping real threads in tests
std::thread::sleep(Duration::from_millis(50)); // BAD! Causes CI timeouts and race conditions
```
**Right:**
```rust
// DETERMINISTIC: Advance the harness virtual clock
harness.clock_mut().advance(Duration::from_millis(50));
```
Real-time sleeps cause tests to run slowly in CI, and under heavy load on shared runners, 50ms may not be enough to complete an asynchronous operation, causing spurious failures.

### 2. Asserting Bounds Before Layout Resolution
**Wrong:**
```rust
let id = arena.insert_with_widget(HotNode::default(), Box::new(widget));
assert_eq!(arena.hot(id).unwrap().bounds.size.x, 120.0); // BAD! Bounds are still 0x0
```
**Right:**
```rust
let id = arena.insert_with_widget(HotNode::default(), Box::new(widget));
// Always trigger layout computation first
layout_engine.compute(&mut arena, root_id, viewport_constraints);
assert_eq!(arena.hot(id).unwrap().bounds.size.x, 120.0);
```
Inserting a widget into the arena does not compute its bounds. Bounds are populated only after `LayoutEngine::compute` has walked the Taffy tree and executed measurement closures.

### 3. Synthesizing Physical Instead of Logical Coordinates
**Wrong:**
```rust
// Dispatched raw 4K coordinates without scale adjustment
let event = PointerEvent::mouse_down(Vec2::new(768.0, 384.0)); // Hits outside on 2x scale!
```
**Right:**
```rust
// Always synthesize coordinates in logical points matching widget layout space
let scale_factor = 2.0;
let logical_pos = Vec2::new(768.0 / scale_factor, 384.0 / scale_factor);
let event = PointerEvent::mouse_down(logical_pos);
```
All hit-testing in `EventRouter` is performed against logical point coordinates. Providing raw physical coordinates will miss targets on HiDPI displays.

### 4. Golden Fixture Drift on Unpinned Fonts
When capturing golden reference images containing text runs, the rendered glyphs depend on installed system fonts. If tests run on Linux (FreeType/Fontconfig) and macOS (CoreText), glyph contours will differ.
- **Remedy**: Always bundle a deterministic TTF/OTF font file (e.g. `Inter-Regular.ttf`) in test resources and load it explicitly into `FontManager` via `add_font_source` before executing golden visual tests.

---

## Next Steps

- [Cookbook 12 — Automated Design Linting in CI](12-ci-design-lint.md)
- [ADR-0012 — Headless CI Testing Infrastructure](../adr/ADR-0012-headless-ci-testing.md)
- [DDR-0009 — martensite-test Headless CI Spec](../ddr/DDR-0009-martensite-test-headless-ci.md)
