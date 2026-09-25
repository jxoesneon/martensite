# Migrating from Iced to Martensite

This guide is designed for developers transitioning from **`Iced`** (The Elm
Architecture / TEA) to **Martensite** (retained-mode reactive arena). It explains
the paradigm differences, compares message passing to reactive signals, and shows
how to translate Elm-style components into Martensite widgets.

---

## 1. The Core Mental Model Shift

The fundamental difference between Iced and Martensite lies in **how state changes propagate to the screen**:

```
+--------------------------------------------------------------------------+
| Iced (The Elm Architecture — TEA)                                        |
|                                                                          |
|   1. Event generates a Message (e.g., Message::Increment).               |
|   2. Central update(&mut self, message: Message) -> Command<Message>.   |
|   3. Monolithic view(&self) -> Element<Message> REBUILDS the entire      |
|      view tree from scratch.                                             |
|   4. Layout and diffing reconciles changes against the renderer.         |
+--------------------------------------------------------------------------+
                                    vs
+--------------------------------------------------------------------------+
| Martensite (Fine-Grained Reactive Arena)                                 |
|                                                                          |
|   1. UI tree is built ONCE into the generational WidgetArena.            |
|   2. Widgets hold lightweight, cloneable Signal<T> / Memo<T> handles.    |
|   3. User interaction calls signal.update(|v| *v += 1) or batch(|| ...). |
|   4. Reactive DAG pushes dirty flags directly to subscribers.            |
|   5. ZERO full-tree rebuilds. ONLY the invalidated widgets repaint.      |
+--------------------------------------------------------------------------+
```

In Iced, your application is structured around a centralized state machine with a monolithic message enum and a `view()` function that returns an ephemeral `Element` tree on every update.
In Martensite, **the view tree is persistent** and **reactivity is decentralized and fine-grained**.

---

## 2. Key Architectural Contrasts

### 1. View Rebuilds vs Signal Memoization
- **`Iced`**: Every single message delivered to `update` causes the entire `view()` function to run again, constructing a new tree of `Element<Message>` structures. While Iced optimizes rendering underneath, building this tree on every keypress or mouse movement incurs allocation and cache churn in complex applications.
- **Martensite**: The widget tree in `WidgetArena` is constructed **once** at window creation. State is stored in `Signal<T>`. When state changes, only the specific widgets observing that signal are flagged `NodeFlags::DIRTY_PAINT` or `DIRTY_LAYOUT`. Intermediate derivations (e.g. filtered lists, formatted labels, total calculations) use `Memo<T>`, which caches results lazily and recomputes only when dependencies change.

### 2. Monolithic Message Enums vs Local Reactive Closures
- **`Iced`**: All possible interactions across your entire screen must be centralized into one or more large enums:
  ```rust
  enum Message {
      InputChanged(String),
      AddTodo,
      ToggleTodo(usize),
      DeleteTodo(usize),
      FilterChanged(Filter),
  }
  ```
  Every button or input must serialize its intent into a `Message`, requiring boilerplate routing through a central `match message { ... }` in `update`.
- **Martensite**: Eliminates the message bottleneck. Event handlers directly mutate the target signal in place:
  ```rust
  button.on_click({
      let todos = todos_signal.clone();
      move || todos.update(|list| list.push(new_item))
  });
  ```
  When multiple related signals must change simultaneously without triggering intermediate renders, wrap them in a transaction: `batch(|| { ... })`.

### 3. Layout Systems: Custom Flex vs Two-Level Taffy + Underflow
- **`Iced`**: Employs its own internal flex layout solver. Sizing rules are configured per-element using length enums (`Length::Fixed`, `Length::Fill`, `Length::Shrink`).
- **Martensite**: Implements a standards-based **two-level layout architecture**:
  1. **Arena Level**: Driven by `LayoutEngine` powered by `Taffy` (the industrial-grade CSS flexbox and grid engine in Rust).
  2. **Internal Level**: Layout primitives like `Flex`, `Container`, and `Stack` manage internal children via `Widget::layout`.
  3. **Space Shortage Recovery**: Widgets declare render floors with `RenderMinimum` and `UnderflowPolicy`. When space falls below minimums, widgets can automatically `Clip`, `Hide`, veil with a `Scrim`, degrade to a `Fallback` icon, or `Collapse` (`display: none`) with oscillation prevention.

### 4. Custom Widget Authoring Contract
- **`Iced`**: To create a custom widget, you implement `iced_core::Widget<Message, Theme, Renderer>`, defining methods for `size`, `layout`, `draw`, `on_event`, and `operate`. Events must be translated back into your app's `Message` type via a `Shell`.
- **Martensite**: Implement `martensite_core::Widget`:
  - `measure(&mut self, cx, constraints) -> Vec2`
  - `layout(&mut self, cx, bounds: Rect)`
  - `paint(&self, cx: &mut PaintContext)` — record commands into `PaintList`
  - `event(&mut self, cx: &mut EventContext) -> EventResponse` — return `Handled`, `RequestRepaint`, or `CapturePointer`
  - `hit_shape(&self) -> Option<Shape>` — specify continuous non-rectangular hit silhouettes
  - `accessibility(&self, node: &mut accesskit::Node)` — full AccessKit integration

### 5. Design Verification & Linting
- **`Iced`**: Relies on manual developer testing and visual inspection.
- **Martensite**: Ships with `martensite-design-lint`. Every `PaintList` embeds provenance scopes (`PushScope`/`PopScope`). The design-lint engine continuously analyzes your running UI against WCAG 2.2, ISA-101 HMI standards, and Fitts's law, catching color contrast failures and cramped hit targets automatically.

---

## 3. Side-by-Side Code Translation: Task Manager (Todo App)

To see the architectural transition in action, compare an Iced application with its Martensite equivalent.

### Iced Implementation (The Elm Architecture)

```rust
use iced::widget::{button, column, row, text, text_input};
use iced::{Element, Sandbox, Settings};

#[derive(Default)]
struct TodoApp {
    input_value: String,
    tasks: Vec<String>,
}

#[derive(Debug, Clone)]
enum Message {
    InputChanged(String),
    AddTask,
    DeleteTask(usize),
}

impl Sandbox for TodoApp {
    type Message = Message;

    fn new() -> Self {
        Self::default()
    }

    fn title(&self) -> String {
        String::from("Iced Todo App")
    }

    fn update(&mut self, message: Message) {
        match message {
            Message::InputChanged(val) => {
                self.input_value = val;
            }
            Message::AddTask => {
                if !self.input_value.trim().is_empty() {
                    self.tasks.push(self.input_value.clone());
                    self.input_value.clear();
                }
            }
            Message::DeleteTask(index) => {
                if index < self.tasks.len() {
                    self.tasks.remove(index);
                }
            }
        }
    }

    // Rebuilds the entire widget tree on every single keystroke or click!
    fn view(&self) -> Element<Message> {
        let input = text_input("What needs to be done?", &self.input_value)
            .on_input(Message::InputChanged)
            .on_submit(Message::AddTask);

        let add_btn = button("Add").on_press(Message::AddTask);
        let header = row![input, add_btn].spacing(10);

        let mut task_list = column![].spacing(8);
        for (i, task) in self.tasks.iter().enumerate() {
            let task_row = row![
                text(task),
                button("Delete").on_press(Message::DeleteTask(i))
            ].spacing(12);
            task_list = task_list.push(task_row);
        }

        column![header, task_list].spacing(20).padding(20).into()
    }
}
```

---

### Martensite Equivalent (Retained Reactive Arena)

```rust
use martensite::prelude::*;

/// State model: fine-grained reactive signals
#[derive(Clone)]
pub struct TodoModel {
    pub input_text: Signal<String>,
    pub tasks: Signal<Vec<String>>,
    pub task_count: Memo<usize>,
}

impl TodoModel {
    pub fn new() -> Self {
        let input_text = create_signal(String::new());
        let tasks = create_signal(Vec::new());

        // Derived memo: total active tasks count
        let task_count = create_memo({
            let tasks = tasks.clone();
            move || tasks.get().len()
        });

        Self { input_text, tasks, task_count }
    }

    pub fn add_task(&self) {
        let text = self.input_text.get();
        if !text.trim().is_empty() {
            batch(|| {
                self.tasks.update(|list| list.push(text));
                self.input_text.set(String::new());
            });
        }
    }

    pub fn delete_task(&self, index: usize) {
        self.tasks.update(|list| {
            if index < list.len() {
                list.remove(index);
            }
        });
    }
}

/// Constructs the UI tree ONCE into the arena
pub fn build_todo_ui(arena: &mut WidgetArena, model: TodoModel) -> WidgetId {
    // 1. Header controls
    let input = TextInput::new().placeholder("What needs to be done?");
    let add_btn = Button::new("Add");

    let header_row = Flex::row()
        .gap(10.0)
        .flex_child(input, 1.0)
        .child(add_btn);

    // 2. Summary counter (memoized)
    let counter_label = Text::new(format!("Tasks: {}", model.task_count.get())).font_size(14.0);

    // 3. Main container
    let main_column = Flex::column()
        .gap(16.0)
        .child(header_row)
        .child(counter_label);

    let container = Container::new()
        .padding_uniform(20.0)
        .child(main_column);

    arena.insert_with_widget(HotNode::default(), Box::new(container))
}
```

---

## 4. Concept Mapping & Translation Cheat Sheet

| Iced Concept | Martensite Equivalent | Notes |
|---|---|---|
| `struct Model` | `Signal<T>` / App state struct | State lives in fine-grained signals instead of a monolithic struct. |
| `enum Message` | Direct signal writes or closures | Avoids centralized message dispatching. |
| `fn update(&mut self, msg)` | `signal.set()` / `signal.update()` | State mutations are performed locally where events happen. |
| `fn view(&self) -> Element` | Executed **once** during arena setup | The widget tree is persistent; only dirty nodes repaint. |
| `iced::Command<Message>` | Async tasks spawning `signal.set()` | Run async futures in background runtimes (e.g. Tokio) and update signals directly. |
| `iced::widget::column` | `Flex::column()` | Two-pass Taffy flexbox layout. |
| `iced::widget::row` | `Flex::row()` | Horizontal flexbox layout with `.gap(f32)`. |
| `iced::widget::container` | `Container::new()` | Supports `EdgeInsets` padding, background `Oklab` colors, and single-child sizing. |
| `iced::widget::stack` | `Stack::new()` | Z-ordered layered composition with `StackAlignment`. |
| `Length::Fill` / `Length::Portion(n)` | `.flex_child(widget, weight)` | Assigns proportional share of available main-axis space. |
| `Renderer` | `PaintList` $\rightarrow$ Vello compute | Vector command recording executed via GPU compute shaders. |
| `iced::Subscription` | `martensite_reactive::create_effect` | Subscribes to time ticks or signal updates. |

---

## 5. Common Migration Traps & Gotchas

### 1. The "Single Giant State Signal" Trap
**The Trap**: Placing your entire Iced `Model` into a single `Signal<Model>`.
```rust
// ANTI-PATTERN:
let app_state = create_signal(EntireAppModel::default());
```
**Why it fails**: Any tiny change (such as typing a single character into a search bar) will dirty-mark the entire application, destroying fine-grained reactivity and forcing unnecessary repaints.
**The Fix**: Decompose state into granular signals (`search_query`, `selected_id`, `items_list`).

### 2. Trying to Return a New View Tree on Every Action
**The Trap**: Writing an `update()` function that deletes all widgets from the arena and re-inserts a new tree.
**The Fix**: Widgets are retained. Modify their properties in place or bind them to signals/memos. If you need dynamic child lists, use list virtualization widgets (`TreeView`, `DataTable`) or mutate child lists in container widgets.

### 3. Overusing Async Channels for Local Updates
**The Trap**: Creating Tokio channels or mpsc queues just to communicate button clicks to nearby widgets.
**The Fix**: Martensite's `Signal<T>` is thread-safe (`Arc`-backed, `Send + Sync + 'static`). Clone the signal handle and pass it directly into the closure.

### 4. Neglecting `batch` on Multi-Field Updates
**The Trap**: Updating three related signals sequentially in an event handler, causing subscribers to re-evaluate three separate times.
**The Fix**: Wrap multi-signal updates in `batch(|| { sig1.set(...); sig2.set(...); })` so effects and subscribers evaluate only once.

---

## Next Steps

- [Migrating from egui](from-egui.md)
- [Cookbook 01 — Responsive Layout](../cookbook/01-responsive-layout.md)
- [Cookbook 02 — Reactive Data Binding](../cookbook/02-data-binding.md)
- [Cookbook 03 — Custom Painting & Silhouettes](../cookbook/03-custom-painting.md)
