# Migrating from Slint to Martensite

This guide is designed for developers transitioning from **`Slint`** (DSL-based
declarative GUI) to **Martensite** (Rust retained-mode reactive arena). It
explains the differences between Slint's markup language and Martensite's
code-first composition, contrasts property bindings with reactive signals, and
provides side-by-side code translations.

---

## 1. The Core Mental Model Shift

The primary architectural shift from Slint to Martensite is moving from an
**external domain-specific language (DSL)** to **pure, idiomatic Rust code**:

```text
+--------------------------------------------------------------------------+
| Slint (Separate DSL + Code Generator)                                    |
|                                                                          |
|   1. Write layout and bindings in a `.slint` markup file.                |
|   2. `build.rs` compiles `.slint` into generated Rust structs & getters. |
|   3. Application state bound via generated properties:                   |
|      `ui.set_counter(val)`, `ui.on_clicked(...)`.                        |
|   4. Internal C++ or Rust interpreter updates property bindings.         |
+--------------------------------------------------------------------------+
                                    vs
+--------------------------------------------------------------------------+
| Martensite (Pure Rust Code-First Composition + Reactive Signals)          |
|                                                                          |
|   1. Zero DSL files, zero `build.rs` code generators, 100% pure Rust.    |
|   2. Widgets composed directly using strongly typed builder APIs.        |
|   3. State managed via fine-grained push-pull DAG (`Signal<T>`, `Memo<T>`).|
|   4. Full rust-analyzer autocomplete, refactoring, and compiler checks.  |
|   5. Retained arena nodes updated only when signals change (0% CPU idle).|
+--------------------------------------------------------------------------+
```

In Slint, the user interface structure is declared in a separate language
(`.slint`) that must be compiled into generated Rust code at build time.  
In Martensite, **the UI is standard Rust code**. You have full access to Rust's
type system, traits, closures, pattern matching, and tooling across your entire
application.

---

## 2. Key Architectural Contrasts

### 1. Code-First Widget Composition vs Markup DSL
- **Slint**: Requires learning and maintaining a custom markup syntax
  (`.slint`), configuring build script build dependencies (`slint-build`), and
  interacting with generated types (`AppWindow::new()`). Moving widgets or
  abstracting components requires defining `.slint` component interfaces.
- **Martensite**: Every widget is a normal Rust struct implementing
  [`Widget`](file:///data/data/com.termux/files/home/martensite/crates/martensite-core/src/widget.rs). Composition uses clear, fluent builder APIs:
  ```rust
  let card = Container::new()
      .padding_uniform(16.0)
      .child(Flex::column().gap(8.0).child(header).child(content));
  ```
  Refactoring, extracting reusable widgets, and composing dynamic layouts uses
  familiar Rust functions and modules without any code-generation step.

### 2. Signal Graph vs Slint Properties & Two-Way Bindings
- **Slint**:
  - Properties (`in-out property <int> counter: 0;`) hold component state.
  - Two-way bindings (`checked <=> root.is_active;`) automatically synchronize
    child widget state with parent properties.
  - Property evaluation is managed by Slint's internal runtime dependency graph.
- **Martensite**:
  - State is modeled explicitly with [`Signal<T>`](file:///data/data/com.termux/files/home/martensite/crates/martensite-reactive/src/signal.rs) handles from
    `martensite-reactive`.
  - Derived values use [`Memo<T>`](file:///data/data/com.termux/files/home/martensite/crates/martensite-reactive/src/memo.rs), which automatically records dependencies
    at runtime and caches results lazily.
  - Mutations happen explicitly via `signal.set(...)` or `signal.update(...)`.
  - Multiple updates can be grouped into an atomic transaction with `batch(|| ...)`.

### 3. Event Dispatch & Callbacks
- **Slint**:
  - Callbacks are declared in `.slint` (`callback request_increase();`).
  - Handlers are wired in Rust via `ui.on_request_increase(move || { ... })`.
  - Event propagation and hit testing are handled inside Slint's runtime.
- **Martensite**:
  - The [`Widget`](file:///data/data/com.termux/files/home/martensite/crates/martensite-core/src/widget.rs) trait defines a clean, decoupled event protocol:
    `Widget::event(&mut self, cx: &mut EventContext) -> EventResponse`.
  - Standard widgets accept closure callbacks or expose polling sinks
    (e.g., [`take_activated()`](file:///data/data/com.termux/files/home/martensite/crates/martensite/src/widgets/button.rs)).
  - Responses communicate directly with the arena: `EventResponse::Handled`,
    `RequestRepaint`, `CapturePointer`, `CaptureFocus`.

### 4. Layout Architecture (Taffy Flexbox vs Slint Layouts)
- **Slint**: Uses built-in layout elements (`VerticalBox`, `HorizontalBox`,
  `GridLayout`). Spacing and padding are often managed via global styling
  palettes or hardcoded properties.
- **Martensite**: Employs a strict **two-level layout engine**:
  1. Top-level arena nodes are solved with [Taffy](https://github.com/DioxusLabs/taffy)
     (`martensite-layout`).
  2. Internal containers ([`Flex`](file:///data/data/com.termux/files/home/martensite/crates/martensite/src/widgets/flex.rs), [`Container`](file:///data/data/com.termux/files/home/martensite/crates/martensite/src/widgets/container.rs),
     [`Stack`](file:///data/data/com.termux/files/home/martensite/crates/martensite/src/widgets/stack.rs)) lay out their own internal children with sub-pixel
     precision.
  Gaps and padding follow design tokens or explicit pt dimensions.

### 5. Automated Standards Verification
- **Slint**: Relies on manual testing and visual inspection to find contrast or
  layout issues.
- **Martensite**: Features `martensite-design-lint`. Every widget's geometry and
  colors are automatically validated against WCAG 2.2 contrast floors (4.5:1),
  target sizes (24pt), and cognitive clutter thresholds at build or test time.

---

## 3. Step-by-Step Translation Example

Let's translate an interactive Slint component with properties, two-way
bindings, and callbacks into Martensite.

### The Slint Version

#### `ui/counter.slint`
```slint
import { Button, VerticalBox, HorizontalBox } from "std-widgets.slint";

export component CounterWindow inherits Window {
    in-out property <int> counter: 42;
    callback increment();
    callback decrement();

    title: "Slint Counter";
    min-width: 280px;
    min-height: 160px;

    VerticalBox {
        alignment: center;
        spacing: 16px;

        Text {
            text: "Value: \{root.counter}";
            font-size: 20px;
            horizontal-alignment: center;
        }

        HorizontalBox {
            alignment: center;
            spacing: 12px;

            Button {
                text: "-";
                clicked => { root.decrement(); }
            }

            Button {
                text: "+";
                clicked => { root.increment(); }
            }
        }
    }
}
```

#### `src/main.rs` (Slint Host)
```rust
slint::include_modules!();

fn main() -> Result<(), slint::PlatformError> {
    let ui = CounterWindow::new()?;

    ui.on_increment({
        let ui_handle = ui.as_weak();
        move || {
            let ui = ui_handle.unwrap();
            ui.set_counter(ui.get_counter() + 1);
        }
    });

    ui.on_decrement({
        let ui_handle = ui.as_weak();
        move || {
            let ui = ui_handle.unwrap();
            ui.set_counter(ui.get_counter() - 1);
        }
    });

    ui.run()
}
```

---

### The Martensite Equivalent (100% Pure Rust)

#### `src/counter.rs`
```rust
use martensite::prelude::*;

pub struct CounterModel {
    pub count: Signal<i32>,
    pub display_text: Memo<String>,
}

impl CounterModel {
    pub fn new(initial: i32) -> Self {
        let count = create_signal(initial);
        let display_text = create_memo({
            let count = count.clone();
            move || format!("Value: {}", count.get())
        });
        Self { count, display_text }
    }

    pub fn increment(&self) {
        self.count.update(|c| *c += 1);
    }

    pub fn decrement(&self) {
        self.count.update(|c| *c -= 1);
    }
}

pub fn build_counter_ui(arena: &mut WidgetArena, model: CounterModel) -> WidgetId {
    let label = Text::new(model.display_text.get())
        .font_size(20.0);

    let btn_minus = Button::new("-");
    let btn_plus = Button::new("+");

    let button_row = Flex::row()
        .gap(12.0)
        .cross_axis_alignment(martensite::widgets::flex::CrossAxisAlignment::Center)
        .child(btn_minus)
        .child(btn_plus);

    let root_column = Flex::column()
        .gap(16.0)
        .cross_axis_alignment(martensite::widgets::flex::CrossAxisAlignment::Center)
        .child(label)
        .child(button_row);

    let mut hot = HotNode::default();
    hot.flags |= NodeFlags::VISIBLE;
    arena.insert_with_widget(hot, Box::new(root_column))
}
```

Notice what changed:
1. **No `.slint` file and no code generation**: The UI structure, styling, and
   logic live together in pure Rust.
2. **Explicit Reactive Model**: `CounterModel` encapsulates the state in a
   reusable, unit-testable struct without requiring a running window.
3. **No `Weak` handle gymnastics**: `Signal<T>` handles are easily cloned and
   passed directly without risk of UI handle deadlocks or reference leaks.

---

## 4. Concept Mapping & Translation Cheat Sheet

| Slint Concept | Martensite Equivalent | Notes |
|---|---|---|
| `.slint` markup file | Pure Rust module (`src/ui/...`) | No build script code generation or DSL compiler required. |
| `component App inherits Window` | `App::build()` / `WidgetArena` | Top-level window and generational widget arena. |
| `VerticalBox { ... }` | `Flex::column()` | Vertical flex container with `.gap(f32)`. |
| `HorizontalBox { ... }` | `Flex::row()` | Horizontal flex container with `.gap(f32)`. |
| `GridLayout { ... }` | `martensite::widgets::Grid` / `Flex` | Taffy-powered 2D grid or nested flexboxes. |
| `Rectangle { background: ...; }` | `Container::new().background(...)` | Single-child container with padding and Oklab fill. |
| `Text { text: "..."; }` | `Text::new("...")` | HarfBuzz shaping, BiDi cascades, and OpenType fonts. |
| `Button { text: "..."; }` | `Button::new("...")` | Native button with focus rings and AccessKit role. |
| `LineEdit { text: ...; }` | `TextInput::new("...")` | Text input with IME, selection, and undo stack. |
| `CheckBox { checked: ...; }` | `CheckBox::new("...")` | Checkbox with tri-state support. |
| `Slider { value: ...; }` | `Slider::new(min, max)` | APG-compliant range slider. |
| `Switch { checked: ...; }` | `Switch::new("...")` | Pill-shaped toggle switch. |
| `in-out property <int> foo: 0;` | `Signal<i32>` | Fine-grained reactive signal handle. |
| `property <string> bar: ...;` | `Memo<String>` | Derived memoized computation with auto-tracked dependencies. |
| `callback my_event();` | Builder callbacks / Event methods | Handled via `Widget::event` or widget action sinks. |
| `for item in model: ...` | `ListView::new()` / `Table::new()` | Virtualized data displays designed for large datasets. |

---

## 5. Common Migration Traps & Gotchas

### 1. Expecting Automatic Two-Way Property Binding
**The Trap**: Writing code and expecting child widget changes to automatically
write back to parent variables without an explicit update callback.  
**The Fix**: In Martensite, data flows down through values and up through
explicit signal mutations. When an input changes, wire its event or action to
mutate the corresponding `Signal<T>`:
```rust
let email = create_signal(String::new());
let email_for_handler = email.clone();
let input = TextInput::new("Email");
// Event handler updates signal explicitly:
// email_for_handler.set(new_value);
```

### 2. Looking for `build.rs` or `slint-build` Setup
**The Trap**: Trying to find where to place markup templates or how to invoke
build-time compilation.  
**The Fix**: Martensite requires **no build script** for UI components. Just add
`martensite` to `[dependencies]` in your `Cargo.toml`, and write pure Rust code
in your `src/` directory.

### 3. Unit Systems and Sizing
**The Trap**: Mixing up Slint's `px` unit with Martensite's logical point system.  
**The Fix**: Martensite measures layout coordinates in **logical points (`pt`)**.
HiDPI scaling is handled systematically across the entire pipeline:
```rust
// Layout measure and builder methods receive points (pt):
let button_height = 32.0; // logical points
// Inside custom Widget::paint, scale to device pixels via cx.pt():
let scaled_pad = cx.pt(8.0);
```

### 4. Over-Using Monolithic Components
**The Trap**: Defining one giant `AppWindow` struct with dozens of properties.  
**The Fix**: Deconstruct your application into small, focused models and
component builder functions. Because signals are lightweight and cloneable,
models can be instantiated independently and unit-tested headlessly without
launching a graphics window.

---

## Next Steps

- [Migrating from React Web](from-react-web.md)
- [Migrating from egui](from-egui.md)
- [Migrating from Iced](from-iced.md)
- [Task Cookbook — Form Validation](../cookbook/04-form-validation.md)
- [Task Cookbook — Headless Component Testing](../cookbook/10-headless-testing.md)
