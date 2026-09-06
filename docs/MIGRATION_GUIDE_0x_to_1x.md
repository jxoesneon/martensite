# Martensite Migration Guide (0.x to 1.x)

**Document Identifier:** DOC-MIGRATE-0x-1x

This guide outlines necessary changes to migrate from the experimental `0.x` series to the stable, production-grade `1.0.0` release.

## Architectural Shifts

### 1. The Generational SlotMap Arena ([ADR-0001](adr/ADR-0001-generational-slotmap-arena.md))
Pre-1.0 code utilizing `Rc<RefCell<Node>>` has been permanently purged. If you previously held explicit references to widget nodes, you must now rely on 64-bit `WidgetId` tokens or structure your state purely via `Signal<T>`.

* **0.x:** `let child = Rc::new(RefCell::new(TextWidget::new()));`
* **1.x:** Define UI declaratively and drive via `Signal<T>`. The `WidgetArena` owns all components internally.

### 2. Push-Pull Reactivity ([ADR-0002](adr/ADR-0002-push-pull-reactive-signals.md))
The reactivity engine now enforces a directed acyclic graph (DAG) topological sort.

* **0.x:** Modifying a signal could trigger immediate, unbounded re-renders, causing visual glitches or infinite loops.
* **1.x:** `Signal::set` sets dirty bitsets. Derived values (`Memo<T>`) are evaluated lazily only when requested by layout/paint logic. Custom widgets must query signal state in `measure` or `paint` using `signal.get()`.

## API Replacements

### App Bootstrapping
* **0.x:** `martensite::run(main_widget)`
* **1.x:** `App::build().title("App").run(|cx| { ... })` - App building is strictly explicit with explicit sizing and themes.

### Traits
* **`Widget::draw`** has been renamed to **`Widget::paint`** and now requires a `&mut PaintContext` rather than raw WGPU command encoders.
* Added mandatory `Widget::measure` and `Widget::layout` for the two-pass geometry engine (Law VII).

### Modifiers
* The `Style` struct has been flattened into chained trait methods via `WidgetExt`.
* **0.x:** `text("Hello").style(Style { padding: 10.0, .. })`
* **1.x:** `text("Hello").padding(10.0)`

## Breaking Change Matrix

| Component | 0.x | 1.x | Migration Action |
|-----------|-----|-----|------------------|
| State | `State<T>` | `Signal<T>` | Use `cx.signal()` and `.get()`/`.set()`. |
| Async | `tokio::spawn` | `cx.spawn()` | Use native `cx.spawn` to ensure UI event loop wake-up on resolution. |
| Theming | Global `THEME` | `Theme::builder` | Inject via `AppBuilder::theme()` and read via `cx.theme()`. |

Follow compiler diagnostics and `#![forbid(unsafe_code)]` constraints when updating implementations.
