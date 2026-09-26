# Migrating from React Web to Martensite

This guide is designed for frontend and full-stack developers transitioning from
**React (Web / DOM)** to **Martensite** (Rust retained-mode reactive arena). It
explains the fundamental paradigm shifts, contrasts runtime lifecycles, and
provides direct side-by-side code translations.

---

## 1. The Core Mental Model Shift

The defining difference between React and Martensite lies in **how UI trees are evaluated and updated**:

```text
+--------------------------------------------------------------------------+
| React (Virtual DOM Reconciliation)                                       |
|                                                                          |
|   1. State update triggers component function re-execution.              |
|   2. Component returns a brand-new tree of Virtual DOM (JSX) objects.    |
|   3. React Fiber diffs the new VDOM against the previous VDOM tree.      |
|   4. Reconciliation commits minimal DOM mutations to the browser DOM.    |
|   5. Browser performs style recalculation, layout reflow, and paint.     |
+--------------------------------------------------------------------------+
                                    vs
+--------------------------------------------------------------------------+
| Martensite (Retained-Mode Generational Arena + Push-Pull Reactive DAG)   |
|                                                                          |
|   1. UI tree is constructed ONCE in the generational `WidgetArena`.       |
|   2. Widgets hold persistent state and cloneable `Signal<T>` handles.     |
|   3. State mutation directly dirties dependent nodes in the reactive DAG.|
|   4. ZERO full-tree rebuilds, ZERO tree diffing, ZERO VDOM allocations.  |
|   5. Only invalidated nodes execute layout/paint passes (0% CPU at idle).|
+--------------------------------------------------------------------------+
```

In React, components are **render functions** that re-execute whenever props or
hooks change. In Martensite, **widgets are persistent objects** residing in an
arena (`WidgetArena`), updated with surgical precision by a fine-grained
push-pull reactive signal graph (`martensite-reactive`).

---

## 2. Key Architectural Contrasts

### 1. Retained Widget Tree vs Virtual DOM Reconciliation
- **React**: JSX elements (`<Button />`, `<div>`) are lightweight JavaScript
  descriptor objects allocated and discarded on every render. If a parent
  component re-renders, all child components re-render by default unless
  manually guarded with `React.memo`, `useMemo`, or `useCallback`.
- **Martensite**: Widgets are retained structs implementing the
  [`Widget`](file:///data/data/com.termux/files/home/martensite/crates/martensite-core/src/widget.rs) trait, stored in
  `WidgetArena` with generational [`WidgetId`](file:///data/data/com.termux/files/home/martensite/crates/martensite-core/src/id.rs) handles. The tree is
  built **once** during initialization. There is no diffing overhead, no
  ephemeral tree allocation, and no garbage collection pauses.

### 2. Signals vs Hooks (`useState` / `useEffect`)
- **React**:
  - `useState`: Triggering a state setter queues a complete re-run of the
    component function.
  - `useEffect`: Runs after the browser paint phase. Requires manual dependency
    arrays (`[dep1, dep2]`), which frequently introduce stale closures, missing
    dependencies, or infinite update loops.
- **Martensite**:
  - `create_signal(initial)`: Returns a [`Signal<T>`](file:///data/data/com.termux/files/home/martensite/crates/martensite-reactive/src/signal.rs) handle. Mutating a signal
    (`signal.set(...)` or `signal.update(...)`) notifies only the specific
    widgets or computations subscribed to that signal.
  - `create_memo(f)`: Derives reactive values with automatic dependency tracking.
    Dependencies are recorded at runtime during execution—no manual dependency
    arrays required.
  - `create_effect(f)`: Automatically re-executes whenever any tracked signal
    changes.
  - `batch(|| ...)`: Batches multiple signal mutations into a single atomic
    propagation pass, preventing intermediate glitch states.

### 3. Taffy Two-Level Layout vs CSS Box Model & Cascades
- **React**: Layout relies on the browser's CSS engine (Flexbox, CSS Grid,
  floats, inline-block, absolute positioning, CSS Cascades, and specificity rules).
- **Martensite**: Implements an explicit **two-level layout architecture**:
  1. **Arena Level**: Uses [Taffy](https://github.com/DioxusLabs/taffy)
     (`martensite-layout`) to compute top-level bounding boxes for widgets
     registered in `WidgetArena`.
  2. **Internal Level**: Layout containers ([`Flex`](file:///data/data/com.termux/files/home/martensite/crates/martensite/src/widgets/flex.rs),
     [`Container`](file:///data/data/com.termux/files/home/martensite/crates/martensite/src/widgets/container.rs), [`Stack`](file:///data/data/com.termux/files/home/martensite/crates/martensite/src/widgets/stack.rs)) arrange their
     internal children in `Widget::layout`.
  All styling and geometry are deterministic and free from unexpected CSS cascade
  inheritance.

### 4. Semantic Design Tokens vs CSS Classes
- **React**: Styling is typically handled via CSS classes, CSS modules, or
  Tailwind utility classes (`bg-blue-500 text-white p-4 rounded-md`).
- **Martensite**: Powered by `martensite-theme` using the perceptually uniform
  **Oklab** color space. Widgets query semantic design tokens via
  [`TokenKey`](file:///data/data/com.termux/files/home/martensite/crates/martensite-core/src/token.rs) (`PrimaryColor`, `SurfaceColor`, `TextColor`,
  `BorderColor`, `RaisedColor`). Tokens ensure strict WCAG contrast compliance
  and can be verified at build time with `martensite-design-lint`.

### 5. Day-Zero Accessibility vs HTML Attributes
- **React**: Accessibility relies on adding `aria-*` attributes and semantic
  HTML tags (`<button>`, `<nav>`, `<main>`), which assistive technologies read
  through the browser DOM.
- **Martensite**: Integrates natively with [AccessKit](https://accesskit.dev/) on
  Day Zero (ADR-0006). Every widget implements `Widget::accessibility`, emitting
  accessibility nodes with roles, labels, and actions directly to platform OS
  assistive technologies (macOS VoiceOver, Windows Narrator, Linux Orca) without
  any HTML translation layer.

---

## 3. Side-by-Side Code Translations

### Example 1: Interactive Counter

#### React Implementation
```tsx
import React, { useState, useMemo } from 'react';

export function Counter() {
  const [count, setCount] = useState(0);
  const isPositive = useMemo(() => count > 0, [count]);

  return (
    <div style={{ display: 'flex', flexDirection: 'row', gap: 12, alignItems: 'center' }}>
      <button onClick={() => setCount(c => c - 1)}>-</button>
      <span style={{ color: isPositive ? 'green' : 'black', fontSize: 16 }}>
        Count: {count}
      </span>
      <button onClick={() => setCount(c => c + 1)}>+</button>
    </div>
  );
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

    let btn_decrement = Button::new("-");
    let btn_increment = Button::new("+");
    let label = Text::new(display_text.get()).font_size(16.0);

    let row = Flex::row()
        .gap(12.0)
        .cross_axis_alignment(martensite::widgets::flex::CrossAxisAlignment::Center)
        .child(btn_decrement)
        .child(label)
        .child(btn_increment);

    let mut hot = HotNode::default();
    hot.flags |= NodeFlags::VISIBLE;
    arena.insert_with_widget(hot, Box::new(row))
}
```

---

### Example 2: Form Input with Dynamic Validation

#### React Implementation
```tsx
import React, { useState, useMemo } from 'react';

export function LoginForm({ onSubmit }: { onSubmit: (email: string) => void }) {
  const [email, setEmail] = useState('');
  const [touched, setTouched] = useState(false);

  const isValid = useMemo(() => email.includes('@'), [email]);
  const showError = touched && !isValid;

  return (
    <div style={{ display: 'flex', flexDirection: 'column', gap: 8, maxWidth: 320 }}>
      <label>Email Address</label>
      <input
        type="email"
        placeholder="user@example.com"
        value={email}
        onChange={e => {
          setEmail(e.target.value);
          setTouched(true);
        }}
      />
      {showError && <span style={{ color: 'red', fontSize: 12 }}>Invalid email</span>}
      <button disabled={!isValid} onClick={() => onSubmit(email)}>
        Submit
      </button>
    </div>
  );
}
```

#### Martensite Equivalent
```rust
use martensite::prelude::*;
use martensite::widgets::text_input::ValidationState;

pub struct LoginFormModel {
    pub email: Signal<String>,
    pub is_valid: Memo<bool>,
}

impl LoginFormModel {
    pub fn new() -> Self {
        let email = create_signal(String::new());
        let is_valid = create_memo({
            let email = email.clone();
            move || email.get().contains('@')
        });
        Self { email, is_valid }
    }
}

pub fn build_login_form(arena: &mut WidgetArena, model: LoginFormModel) -> WidgetId {
    let input = TextInput::new("Email Address")
        .placeholder("user@example.com");

    let error_text = Text::new("Invalid email address")
        .font_size(12.0);

    let submit_btn = Button::new("Submit")
        .primary(true)
        .enabled(model.is_valid.get());

    let form = Flex::column()
        .gap(8.0)
        .child(input)
        .child(error_text)
        .child(submit_btn);

    let mut hot = HotNode::default();
    hot.flags |= NodeFlags::VISIBLE;
    arena.insert_with_widget(hot, Box::new(form))
}
```

---

### Example 3: Filterable Task List

#### React Implementation
```tsx
import React, { useState, useMemo } from 'react';

export function TaskList() {
  const [tasks] = useState(['Write documentation', 'Implement catalog', 'Verify design lints']);
  const [filter, setFilter] = useState('');

  const filtered = useMemo(() => {
    return tasks.filter(t => t.toLowerCase().includes(filter.toLowerCase()));
  }, [tasks, filter]);

  return (
    <div style={{ display: 'flex', flexDirection: 'column', gap: 12, padding: 16 }}>
      <input
        placeholder="Filter tasks..."
        value={filter}
        onChange={e => setFilter(e.target.value)}
      />
      <div style={{ display: 'flex', flexDirection: 'column', gap: 4 }}>
        {filtered.map(t => (
          <div key={t} style={{ padding: '8px 12px', background: '#f5f5f5' }}>
            {t}
          </div>
        ))}
      </div>
    </div>
  );
}
```

#### Martensite Equivalent
```rust
use martensite::prelude::*;
use martensite::widgets::ListView;

pub struct TaskListModel {
    pub tasks: Signal<Vec<String>>,
    pub filter: Signal<String>,
    pub filtered_tasks: Memo<Vec<String>>,
}

impl TaskListModel {
    pub fn new() -> Self {
        let tasks = create_signal(vec![
            "Write documentation".to_string(),
            "Implement catalog".to_string(),
            "Verify design lints".to_string(),
        ]);
        let filter = create_signal(String::new());

        let filtered_tasks = create_memo({
            let tasks = tasks.clone();
            let filter = filter.clone();
            move || {
                let q = filter.get().to_lowercase();
                tasks.get()
                    .into_iter()
                    .filter(|t| t.to_lowercase().contains(&q))
                    .collect()
            }
        });

        Self { tasks, filter, filtered_tasks }
    }
}

pub fn build_task_list(arena: &mut WidgetArena, model: TaskListModel) -> WidgetId {
    let filter_input = TextInput::new("Filter tasks").placeholder("Filter tasks...");
    
    let list_view = ListView::new()
        .items(model.filtered_tasks.get())
        .row_height(36.0);

    let container = Flex::column()
        .gap(12.0)
        .padding(16.0)
        .child(filter_input)
        .child(list_view);

    let mut hot = HotNode::default();
    hot.flags |= NodeFlags::VISIBLE;
    arena.insert_with_widget(hot, Box::new(container))
}
```

---

## 4. Concept Mapping & Translation Cheat Sheet

| React (Web) Concept | Martensite Equivalent | Notes |
|---|---|---|
| `<div style={{ display: 'flex', flexDirection: 'row' }}>` | `Flex::row()` | Taffy flexbox with `.gap(px)` and cross/main alignment. |
| `<div style={{ display: 'flex', flexDirection: 'column' }}>` | `Flex::column()` | Vertical flex layout container. |
| `<div style={{ position: 'relative' }}>` | `Stack::new()` | Z-ordered layering container. |
| `<div style={{ padding: 16 }}>` | `Container::new().padding_uniform(16.0)` | Single-child container with padding and background. |
| `<div style={{ overflow: 'auto' }}>` | `ScrollView::new(child)` | Virtualized or bounded scrollable viewport. |
| `<button onClick={...}>` | `Button::new("Label")` | Native button with AccessKit role and keyboard activation. |
| `<input type="text">` | `TextInput::new("Label")` | Native text field with IME pre-edit, selection, and undo. |
| `<input type="checkbox">` | `CheckBox::new("Label")` | Accessible checkbox supporting tri-state (`CheckState`). |
| `<input type="range">` | `Slider::new(min, max)` | ARIA APG compliant slider. |
| `<input type="checkbox" role="switch">` | `Switch::new("Label")` | Pill-shaped toggle switch. |
| `<table>` | `Table::new()` / `martensite_blessed::DataTable` | Virtualized tabular data grid with sortable columns. |
| `<ul><li>...</li></ul>` | `ListView::new()` | Virtualized list view with single/multi selection. |
| `useState(initial)` | `create_signal(initial)` | Reactive signal with push-pull dependency notification. |
| `useMemo(() => compute, [deps])` | `create_memo(move || compute)` | Automatic runtime dependency tracking; no dependency array. |
| `useEffect(() => ..., [deps])` | `create_effect(move || ...)` | Side effect executing on dependency changes. |
| `React.createContext` / `useContext` | Shared `Signal<T>` / `arena.theme()` | Direct signal sharing or arena-wide theme token resolution. |
| CSS Classes / Tailwind | `ThemeToken` + Builder methods | Oklab tokens (`PrimaryColor`, `SurfaceColor`, etc.). |
| Browser DOM Nodes | `WidgetArena` + Generational `WidgetId` | Deterministic slot-map arena with generational keys. |

---

## 5. Common Migration Traps & Gotchas

### 1. Trying to Re-render by Calling Builder Functions Repeatedly
**The Trap**: Calling `build_ui()` every time a signal changes, trying to recreate the tree like a React component render pass.  
**The Fix**: In Martensite, construct your widget hierarchy **once** at startup. When application state changes, call `signal.set(...)` or `signal.update(...)`. The reactive DAG marks only the dependent widgets dirty (`NodeFlags::DIRTY_PAINT` / `DIRTY_LAYOUT`), repainting them in-place.

### 2. Rust Ownership & Stale Closures
**The Trap**: Passing a `Signal<T>` into a closure without cloning it first, causing "use of moved value" compile errors:
```rust
// ❌ Compile error: count moved into closure
let count = create_signal(0);
let memo = create_memo(move || count.get() + 1);
let btn = Button::new("+").on_click(move || count.update(|c| *c += 1));
```
**The Fix**: `Signal<T>` and `Memo<T>` are lightweight, reference-counted handles (`Copy` or cheap `Clone`). Explicitly clone handles before moving them into separate closures:
```rust
// ✅ Correct idiom: clone handle before moving
let count = create_signal(0);
let count_clone1 = count.clone();
let memo = create_memo(move || count_clone1.get() + 1);
let count_clone2 = count.clone();
let btn = Button::new("+").on_click(move || count_clone2.update(|c| *c += 1));
```

### 3. Forgetting to Batch Multiple State Updates
**The Trap**: Updating three signals sequentially, causing intermediate updates:
```rust
user_name.set("Alice".to_string());
user_role.set("Admin".to_string());
permissions.set(vec!["read", "write"]);
```
**The Fix**: Use [`batch`](file:///data/data/com.termux/files/home/martensite/crates/martensite-reactive/src/batch.rs) to execute all modifications in a single atomic propagation step:
```rust
batch(|| {
    user_name.set("Alice".to_string());
    user_role.set("Admin".to_string());
    permissions.set(vec!["read", "write"]);
});
```

### 4. Expecting CSS Cascade Inheritance
**The Trap**: Setting a font size or text color on an outer `Container` and expecting all nested `Text` widgets to inherit it automatically.  
**The Fix**: In Martensite, styling does not cascade implicitly down an arbitrary DOM tree. Instead:
- Set font size directly on `Text::new(...).font_size(14.0)`.
- Or set semantic theme tokens on `arena.theme()`, which all widgets query consistently.

---

## Next Steps

- [Migrating from Slint](from-slint.md)
- [Migrating from egui](from-egui.md)
- [Migrating from Iced](from-iced.md)
- [Task Cookbook — Reactive Data Binding](../cookbook/02-data-binding.md)
- [Task Cookbook — Automated Design Linting in CI](../cookbook/12-ci-design-lint.md)
