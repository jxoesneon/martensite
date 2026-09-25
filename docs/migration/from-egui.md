# Migrating from egui to Martensite

This guide is designed for developers transitioning from **`egui`** (immediate mode)
to **Martensite** (retained-mode reactive arena). It explains the fundamental
mental-model shifts, contrasts runtime lifecycles, and provides direct code
translations.

---

## 1. The Core Mental Model Shift

The defining difference between `egui` and Martensite lies in the **lifecycle of the user interface**:

```
+--------------------------------------------------------------------------+
| egui (Immediate Mode)                                                    |
|                                                                          |
|  Loop every frame (60-120 FPS):                                          |
|  App::update(&mut self, ctx: &Context, frame: &mut Frame) {              |
|      ui.label("Hello");           // Recreated, remeasured, repainted    |
|      if ui.button("Click").clicked() {                                    |
|          self.count += 1;         // Immediate boolean response          |
|      }                                                                   |
|  }                                                                       |
+--------------------------------------------------------------------------+
                                    vs
+--------------------------------------------------------------------------+
| Martensite (Retained-Mode Reactive Arena)                                |
|                                                                          |
|  Run once at initialization:                                             |
|  let count = create_signal(0);                                           |
|  let button = Button::new("Click")                                       |
|      .on_click({ let count = count.clone(); move || count.update(|c| *c += 1) });|
|  let label = Text::bind(count.clone());                                  |
|  arena.insert(Flex::row().child(label).child(button));                  |
|                                                                          |
|  At runtime:                                                             |
|  Idle: 0% CPU. Window sits quiescent.                                    |
|  Click: Event dispatched -> Signal modified -> ONLY label repainted.     |
+--------------------------------------------------------------------------+
```

In `egui`, the UI is an ephemeral side-effect of a function that runs 60 or 120 times every second.
In Martensite, the UI is a **persistent tree of arena nodes** (`WidgetArena`) coupled with a **fine-grained reactive signal graph** (`martensite-reactive`).

---

## 2. Key Architectural Contrasts

### 1. State Lifetime & Storage
- **`egui`**: Widgets have no persistent heap identity across frames. Any widget state (cursor position, scroll offset, open accordion) must either be stored in your application's root struct or stashed in `ctx.data_mut()` / `ui.memory_mut()` using hashed `egui::Id` keys.
- **Martensite**: Widgets live permanently in the `WidgetArena`. Each widget is identified by a generational `WidgetId` (`(slot, generation)`), preventing use-after-free or dangling handles. Persistent properties (scroll position, layout cache, text buffers) are stored directly inside the widget struct (`Box<dyn Widget>`). Application business state is encapsulated in `Signal<T>` handles.

### 2. Repaint Budget & Idle Quiescence
- **`egui`**: Requires continuous rendering or constant calls to `ctx.request_repaint()`. Even at idle, mouse hover triggers complete re-execution of your entire UI code path, followed by CPU vertex tessellation and index buffer uploads.
- **Martensite**: Implements strict **event-driven quiescence**. When the user is not actively interacting with the window and no timers or spring animations are ticking, CPU utilization drops to **0%**. Repaints are fine-grained: modifying a `Signal<T>` only marks dependent nodes `NodeFlags::DIRTY_PAINT`; the unchanged chrome of parent and sibling nodes is not recomputed.

### 3. Layout Caching vs Single-Pass Cursor Placement
- **`egui`**: Operates on a single-pass cursor placement model (`ui.allocate_rect`, `ui.cursor()`). The parent allocates space sequentially. This makes two-way constraints (such as expanding table columns to match the widest cell in subsequent rows) difficult, requiring two-frame hacks or stored memory keys.
- **Martensite**: Employs a **two-level, two-pass layout architecture**:
  1. **Arena Level**: Driven by Taffy (`LayoutEngine`). Pass 1 queries `Widget::measure` bottom-up (cached via `InlineTextCache`); Pass 2 assigns definitive `Rect` coordinates top-down into `HotNode::bounds`.
  2. **Internal Level**: Composite widgets (`Flex`, `Container`, `Stack`) resolve their internal children in their own `Widget::layout` method.
  All geometry is cached in `HotNode::bounds` and validated until explicitly dirtied by `NodeFlags::DIRTY_LAYOUT`.

### 4. Event Routing & Interaction
- **`egui`**: Inline polling. `if ui.button("Submit").clicked() { ... }`. Input events are consumed during the paint walk.
- **Martensite**: Decoupled, asynchronous event dispatch:
  1. `martensite-window` converts raw OS (`winit`) events into normalized `WidgetEvent`s (`PointerPressed`, `KeyPressed`, `Scroll`).
  2. The `EventRouter` performs spatial hit-testing against `HotNode::bounds` and `Widget::hit_shape`.
  3. Events are routed to `Widget::event(&mut self, cx: &mut EventContext) -> EventResponse`.
  4. Widgets return structured responses (`EventResponse::Handled`, `RequestRepaint`, `CaptureFocus`, `CapturePointer`).

### 5. Day-Zero Accessibility (AccessKit)
- **`egui`**: AccessKit support was added to an immediate-mode architecture later in its lifecycle, leading to complications where nodes must be reconstructed every frame and node IDs must remain stable across dynamic branches.
- **Martensite**: AccessKit was architected on Day Zero (ADR-0006). `WidgetArena` directly mirrors the accessibility tree. The framework calls `Widget::accessibility(&self, node: &mut accesskit::Node)` when nodes are created or dirtied, and `Widget::a11y_fixup` to wire complex relational attributes (`aria-controls`, `aria-describedby`) across internal and popup hierarchies.

---

## 3. Side-by-Side Code Translations

### Example 1: Counter Component

#### `egui` Implementation
```rust
struct CounterApp {
    count: i32,
}

impl eframe::App for CounterApp {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        egui::CentralPanel::default().show(ctx, |ui| {
            ui.horizontal(|ui| {
                ui.label(format!("Count: {}", self.count));
                if ui.button("Increment").clicked() {
                    self.count += 1;
                }
            });
        });
    }
}
```

#### Martensite Equivalent
```rust
use martensite::prelude::*;

pub fn build_counter_ui(arena: &mut WidgetArena) -> WidgetId {
    let count = create_signal(0i32);

    // Derived memo for display string
    let display_text = create_memo({
        let count = count.clone();
        move || format!("Count: {}", count.get())
    });

    let label = Text::new(display_text.get()).font_size(16.0);
    
    // Button with direct signal update
    let btn = Button::new("Increment");

    let row = Flex::row()
        .gap(12.0)
        .cross_axis_alignment(martensite::widgets::flex::CrossAxisAlignment::Center)
        .child(label)
        .child(btn);

    arena.insert_with_widget(HotNode::default(), Box::new(row))
}
```

---

### Example 2: Form Input with Dynamic Validation

#### `egui` Implementation
```rust
struct LoginForm {
    email: String,
    submitted: bool,
}

impl LoginForm {
    fn ui(&mut self, ui: &mut egui::Ui) {
        ui.text_edit_singleline(&mut self.email);
        
        let is_valid = self.email.contains('@');
        if !is_valid && !self.email.is_empty() {
            ui.colored_label(egui::Color32::RED, "Invalid email address");
        }

        if ui.add_enabled(is_valid, egui::Button::new("Login")).clicked() {
            self.submitted = true;
        }
    }
}
```

#### Martensite Equivalent
```rust
use martensite::prelude::*;

pub struct LoginForm {
    pub email: Signal<String>,
    pub is_valid: Memo<bool>,
    pub error_message: Memo<Option<&'static str>>,
}

impl LoginForm {
    pub fn new() -> Self {
        let email = create_signal(String::new());
        
        let is_valid = create_memo({
            let email = email.clone();
            move || email.get().contains('@')
        });

        let error_message = create_memo({
            let email = email.clone();
            let is_valid = is_valid.clone();
            move || {
                let text = email.get();
                if !text.is_empty() && !is_valid.get() {
                    Some("Invalid email address")
                } else {
                    None
                }
            }
        });

        Self { email, is_valid, error_message }
    }
}

pub fn build_login_form(arena: &mut WidgetArena, form: LoginForm) -> WidgetId {
    let email_input = TextInput::new().placeholder("name@example.com");
    let error_label = Text::new("").font_size(12.0);
    let submit_button = Button::new("Login");

    let form_layout = Flex::column()
        .gap(8.0)
        .child(email_input)
        .child(error_label)
        .child(submit_button);

    arena.insert_with_widget(HotNode::default(), Box::new(form_layout))
}
```

---

## 4. Concept Mapping & Translation Cheat Sheet

| `egui` Concept | Martensite Equivalent | Notes |
|---|---|---|
| `egui::Context` | `WidgetArena` / `ReactiveRuntime` | The arena manages widget instances; the runtime manages signals. |
| `egui::Ui` | `Flex` / `Container` / `LayoutContext` | In Martensite, layout structure is declared with widgets, not a builder cursor. |
| `ui.label("text")` | `Text::new("text")` | Supports real HarfBuzz shaping, OpenType features, and BiDi cascades. |
| `ui.button("label")` | `Button::new("label")` | Built-in AccessKit button role, keyboard activation, and focus rings. |
| `ui.text_edit_singleline(&mut s)` | `TextInput::new()` | Features full IME composition pre-edit (`ImePreedit`), selection, and undo. |
| `ui.horizontal(\|ui\| ...)` | `Flex::row()` | Taffy flexbox with `.gap(f32)` and main/cross alignment. |
| `ui.vertical(\|ui\| ...)` | `Flex::column()` | Vertical flex container. |
| `ui.add_space(px)` | `.gap(px)` or `Container` padding | Avoid empty spacer nodes; prefer container margins and flex gaps. |
| `ctx.request_repaint()` | `EventResponse::RequestRepaint` / `signal.set()` | Updating a signal automatically marks subscribers dirty; no manual context repaint needed. |
| `Painter` (`ui.painter()`) | `PaintList` (`cx.list`) | Emits declarative vector commands executed via Vello GPU compute. |
| `ui.memory_mut()` | Widget struct fields | Store component state directly in your widget struct. |
| `egui::Id` | `WidgetId` | Generational arena index; eliminates ID hashing collisions. |

---

## 5. Common Migration Traps & Gotchas

### 1. Trying to Re-create Widgets Every Frame
**The Trap**: Writing an `update()` loop that calls `arena.insert(...)` on every frame tick.
**The Fix**: In Martensite, construct your widget hierarchy **once** at window initialization. When you need values to change, mutate existing `Signal<T>` handles. The signals dirty-mark the exact widgets that need to re-render.

### 2. Treating Signals Like Plain Variables
**The Trap**: Modifying an application struct field directly and wondering why the screen doesn't update.
**The Fix**: In `egui`, modifying `self.count += 1` works because the entire screen re-renders next frame. In Martensite, the framework only knows state changed if you call `count_signal.set()` or `count_signal.update()`.

### 3. Attempting Immediate Hit Checks
**The Trap**: Looking for `.clicked()` methods on `Button`.
**The Fix**: Martensite's event model is asynchronous. Handle user input in `Widget::event(&mut self, cx)` by matching on `WidgetEvent::PointerReleased` or attaching a closure callback to the widget builder.

### 4. Ignoring HiDPI Scaling
**The Trap**: Hardcoding sizes like `32.0` in paint methods.
**The Fix**: In `egui`, `ctx.pixels_per_point()` is often applied implicitly. In Martensite's `Widget::paint` and `Widget::layout`, always multiply baked points by the context scale via `cx.pt(32.0)`.

---

## Next Steps

- [Migrating from Iced](from-iced.md)
- [Cookbook 01 — Responsive Layout](../cookbook/01-responsive-layout.md)
- [Cookbook 02 — Reactive Data Binding](../cookbook/02-data-binding.md)
- [Cookbook 03 — Custom Painting & Silhouettes](../cookbook/03-custom-painting.md)
