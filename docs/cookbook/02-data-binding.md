# Cookbook 02 — Reactive Data Binding

State management in Martensite is powered by `martensite-reactive`: a fine-grained,
**push-pull signal DAG** with topological scheduling, transactional batching, and
cycle detection.

This recipe demonstrates how to bind application domain data to widgets, manage
derived computations with memos, batch state mutations atomically, and schedule
repaints efficiently without full-tree rebuilds.

---

## 1. Goal

Connect dynamic business models to UI components so that:
1. When state changes, only the exact widgets reading that state are dirty-marked.
2. Complex derived values are lazily evaluated and cached via `create_memo`.
3. Multi-property updates are committed atomically inside `batch` blocks.
4. UI updates trigger minimal repaints using `NodeFlags::DIRTY_PAINT` and `NodeFlags::DIRTY_LAYOUT`.

---

## 2. Complete Runnable Pattern

The following pattern implements a live Financial Ticker & Currency Converter. It demonstrates writable source signals, derived calculation memos, transactional batching, and reactive UI widget synchronization.

```rust
use martensite::prelude::*;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;

/// Business domain state managed via Martensite reactive signals.
#[derive(Clone)]
pub struct PortfolioState {
    /// Writable source signals
    pub usd_balance: Signal<f64>,
    pub eur_usd_rate: Signal<f64>,
    pub transaction_count: Signal<usize>,

    /// Lazily evaluated, cached derived computations
    pub eur_balance: Memo<f64>,
    pub formatted_summary: Memo<String>,
}

impl PortfolioState {
    pub fn new(initial_usd: f64, initial_rate: f64) -> Self {
        let usd_balance = create_signal(initial_usd);
        let eur_usd_rate = create_signal(initial_rate);
        let transaction_count = create_signal(0);

        // Memo 1: Convert USD to EUR. Automatically subscribes to usd_balance and eur_usd_rate.
        let eur_balance = create_memo({
            let usd = usd_balance.clone();
            let rate = eur_usd_rate.clone();
            move || {
                let r = rate.get();
                if r > 0.0 {
                    usd.get() / r
                } else {
                    0.0
                }
            }
        });

        // Memo 2: Formatted presentation string.
        let formatted_summary = create_memo({
            let usd = usd_balance.clone();
            let eur = eur_balance.clone();
            let tx = transaction_count.clone();
            move || {
                format!(
                    "USD: ${:.2} | EUR: €{:.2} ({} txs)",
                    usd.get(),
                    eur.get(),
                    tx.get()
                )
            }
        });

        Self {
            usd_balance,
            eur_usd_rate,
            transaction_count,
            eur_balance,
            formatted_summary,
        }
    }

    /// Perform a multi-variable atomic update inside a transaction batch.
    pub fn deposit_with_rate_update(&self, deposit_usd: f64, new_rate: f64) {
        batch(|| {
            self.usd_balance.update(|bal| *bal += deposit_usd);
            self.eur_usd_rate.set(new_rate);
            self.transaction_count.update(|count| *count += 1);
        });
        // Downstream effects and memos evaluate only ONCE after the batch exits.
    }
}

/// A reactive widget that observes signals and updates its text content.
pub struct ReactiveSummaryView {
    state: PortfolioState,
    cached_bounds: Rect,
    text_view: Text,
}

impl ReactiveSummaryView {
    pub fn new(state: PortfolioState) -> Self {
        let initial_text = state.formatted_summary.get();
        Self {
            state,
            cached_bounds: Rect::default(),
            text_view: Text::new(initial_text).font_size(15.0),
        }
    }
}

impl Widget for ReactiveSummaryView {
    fn measure(&mut self, cx: &mut LayoutContext, constraints: LayoutConstraints) -> glam::Vec2 {
        self.text_view.measure(cx, constraints)
    }

    fn layout(&mut self, cx: &mut LayoutContext, bounds: Rect) {
        self.cached_bounds = bounds;
        cx.layout_child(&mut self.text_view, bounds);
    }

    fn paint(&self, cx: &mut PaintContext) {
        // Read memo: returns cached string unless dependencies were marked dirty
        let current_text = self.state.formatted_summary.get();
        
        // If content changed, we can draw or pass it to our text child
        self.text_view.paint(cx);
    }

    fn child_count(&self) -> usize { 1 }
    fn child(&self, i: usize) -> Option<&dyn Widget> {
        if i == 0 { Some(&self.text_view) } else { None }
    }
    fn child_mut(&mut self, i: usize) -> Option<&mut dyn Widget> {
        if i == 0 { Some(&mut self.text_view) } else { None }
    }
    fn child_bounds(&self, i: usize) -> Option<Rect> {
        if i == 0 { Some(self.cached_bounds) } else { None }
    }
}

/// Wiring reactive state to the arena with an effect
pub fn bind_portfolio_to_arena(arena: &mut WidgetArena, state: PortfolioState) -> (WidgetId, Effect) {
    let summary_widget = ReactiveSummaryView::new(state.clone());
    let widget_id = arena.insert_with_widget(
        HotNode::default(),
        Box::new(summary_widget),
    );

    // Create an effect that dirty-marks the arena widget whenever state changes
    let effect_runs = Arc::new(AtomicUsize::new(0));
    let effect = create_effect({
        let summary = state.formatted_summary.clone();
        let runs = effect_runs.clone();
        move || {
            // Register reactive dependency by reading the memo
            let _val = summary.get();
            runs.fetch_add(1, Ordering::SeqCst);

            // In an actual window loop, mark the arena node dirty for repaint:
            // arena.mark_dirty(widget_id, NodeFlags::DIRTY_PAINT | NodeFlags::DIRTY_LAYOUT);
        }
    });

    (widget_id, effect)
}
```

---

## 3. The Reactive Engine: Push-Pull Architecture

Martensite's reactivity system solves the twin problems of **wasted work** (eagerly recomputing unchanged views) and **glitches** (observing transient inconsistent intermediate states).

```
[ Signal: USD ] ------\
                       +---> [ Memo: EUR ] ---\
[ Signal: Rate ] -----/                        +---> [ Memo: Summary ] ---> [ Effect / Paint ]
                                               |
[ Signal: Tx Count ] --------------------------/
```

### 1. Push Phase: Dirty Flag Propagation
When `signal.set()` or `signal.update()` is called:
- The signal traverses its downstream dependency edges.
- Each subscriber is marked **Dirty**.
- **No computations run yet.** Values are not re-evaluated during this pass.
- A 3-color DFS cycle detector inspects dependency relationships, catching feedback loops at the point of creation rather than locking the UI thread.

### 2. Pull Phase: Lazy On-Demand Evaluation
When a `Memo::get()` or `Effect` execution occurs:
- The memo checks if any upstream dependency marked it dirty.
- If clean, it immediately returns its cached value with zero re-evaluation overhead.
- If dirty, it re-executes its closure, caches the new result, and clears the dirty flag.

### 3. Untracked Reads: `get_untracked()`
If you need to read a signal's current value inside a memo or effect *without* subscribing to future updates, call `.get_untracked()`:

```rust
let memo = create_memo({
    let count = count_signal.clone();
    let theme = theme_signal.clone();
    move || {
        // Subscribes to `count`, but NOT `theme`.
        // Changing `theme` will NOT invalidate this memo.
        format!("Count: {} (theme={})", count.get(), theme.get_untracked())
    }
});
```

---

## 4. The Three Reactive Primitives

| Primitive | Creation Function | Characteristics | Memory Model |
|---|---|---|---|
| **`Signal<T>`** | `create_signal(v)` / `Signal::new(v)` | Writable source of truth. Cloning produces another handle to the same cell. | Shared `Arc<ReactiveNode>` |
| **`Memo<T>`** | `create_memo(f)` / `Memo::new(f)` | Read-only derived computation. Lazily re-evaluated on `.get()` when dirty. | Cached `Arc<ReactiveNode>` |
| **`Effect`** | `create_effect(f)` / `Effect::new(f)` | Eager side-effect. Runs once at creation, then re-runs on dirty flush. | Owned handle with `.dispose()` |

### Signal Write Operations
`Signal<T>` provides three distinct write operations:

1. **`set(value: T)`**
   Overwrites the value unconditionally and marks all subscribers dirty.
   ```rust
   let name = create_signal("Alice".to_string());
   name.set("Bob".to_string());
   ```

2. **`set_if_changed(value: T) -> bool`**
   Requires `T: PartialEq + Clone`. Only marks subscribers dirty if the new value differs from the existing value. Returns `true` if mutated.
   ```rust
   let tab = create_signal(0);
   assert!(!tab.set_if_changed(0)); // Returns false, no subscribers dirtied!
   assert!(tab.set_if_changed(1));  // Returns true, marks subscribers dirty.
   ```

3. **`update(f: impl FnOnce(&mut T))`**
   Mutates the underlying value in place without requiring `Clone` on `T`. Marks subscribers dirty upon completion.
   ```rust
   let list = create_signal(vec![1, 2, 3]);
   list.update(|v| v.push(4));
   ```

---

## 5. Transactional Batching with `batch`

Without batching, writing to multiple signals sequentially causes intermediate notifications to ripple through the graph:

```rust
// Without batch:
balance.set(500.0);   // Effect runs (observed intermediate state!)
currency.set("GBP");  // Effect runs again
```

With `batch(|| ...)`, all notifications are deferred until the closure completes:

```rust
batch(|| {
    balance.set(500.0);
    currency.set("GBP");
    // Subscriptions have not run yet.
});
// Subscriptions run exactly once here, observing the final consistent state.
```

### When to Call `flush()`
`batch` automatically flushes its queue on exit. If you perform signal mutations outside a batch and need all effects to execute synchronously before the next line of code (for instance, in a unit test), call:
```rust
martensite_reactive::flush();
```

---

## 6. Binding Strategies for Widgets

There are two primary paradigms for connecting reactive signals to Martensite widgets:

### Paradigm A: Direct Polling in `measure` and `paint` (Recommended)
Hold clones of `Signal<T>` or `Memo<T>` inside the widget struct. During `measure` or `paint`, read `.get()`:
- **Pros**: Widget is completely self-contained; zero callback wiring.
- **Contract**: When an action modifies the signal, return `EventResponse::RequestRepaint` from `Widget::event` or mark the widget's `HotNode` flags with `NodeFlags::DIRTY_PAINT`.

```rust
pub struct CounterBadge {
    count: Signal<u32>,
    cached_bounds: Rect,
}

impl Widget for CounterBadge {
    fn paint(&self, cx: &mut PaintContext) {
        let current_count = self.count.get();
        // Paint badge using current_count...
    }
}
```

### Paradigm B: Effect-Driven Arena Updates
For expensive widgets (e.g. `CodeEditor`, `DataTable`, or `Text`), keep an `Effect` alive alongside the widget that modifies properties directly and invalidates the layout/paint:

```rust
let title_signal = create_signal("Project X".to_string());
let text_widget_id = arena.insert_with_widget(
    HotNode::default(),
    Box::new(Text::new("Initial")),
);

let _effect = create_effect({
    let title = title_signal.clone();
    move || {
        let new_text = title.get();
        // Mutate the widget in the arena and set dirty flags
        if let Some(cold) = arena.get_cold_mut(text_widget_id) {
            if let Some(text_widget) = cold.as_mut::<Text>() {
                text_widget.set_content(new_text);
            }
        }
        if let Some(hot) = arena.get_hot_mut(text_widget_id) {
            hot.flags |= NodeFlags::DIRTY_LAYOUT | NodeFlags::DIRTY_PAINT;
        }
    }
});
```

---

## 7. Common Mistakes & Pitfalls

### 1. The Accidental Infinite Loop
**Wrong:**
```rust
create_effect({
    let count = count_signal.clone();
    move || {
        let current = count.get();
        count.set(current + 1); // CRITICAL BUG: Writes to a dependency it just read!
    }
});
```
**Detection:** Martensite's scheduler detects cycles via 3-color DFS and returns `ReactiveError::CycleDetected` rather than deadlocking or overflowing the call stack.

### 2. Calling `.get()` Outside a Reactive Scope
Calling `signal.get()` inside normal application setup code reads the current value, but **does not create a subscription**. Only calls executed inside `create_memo` or `create_effect` establish reactive dependency edges.

### 3. Cloning Complex Objects Unnecessarily
Calling `signal.get()` on a `Signal<Vec<T>>` clones the entire vector. If you only need to inspect an item or mutate it, use `.update()`:
```rust
// Instead of:
// let mut v = vec_signal.get(); v.push(x); vec_signal.set(v);
// Use:
vec_signal.update(|v| v.push(x));
```

---

## Next Steps

- [Cookbook 01 — Responsive Layout](01-responsive-layout.md)
- [Cookbook 03 — Custom Painting & Silhouettes](03-custom-painting.md)
- [Tutorial 02 — Reactive State Management](../tutorials/02-reactive-state.md)
