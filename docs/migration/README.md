# Martensite Migration Guides

Welcome to the Martensite Migration Hub. If you are migrating an existing Rust
desktop application or embedded HMI to Martensite, these guides explain the
architectural differences, provide direct mental-model crosswalks, and offer
side-by-side code translations.

---

## Paradigm Comparison Matrix

| Capability | Immediate Mode (`egui`) | Elm Architecture (`iced`) | Martensite |
|---|---|---|---|
| **Primary Architecture** | Immediate mode (per-frame loop) | Elm / TEA (Model-Update-View) | Retained-Mode Generational Arena (`WidgetArena`) |
| **State Lifetime** | Transient / Stored in custom structs or per-frame ID maps | Centralized `Model` struct | Fine-grained Push-Pull Reactive DAG (`Signal<T>`, `Memo<T>`) |
| **Repaint Model** | Continuous 60–120 FPS or full repaint requests | Message-driven full `view()` tree rebuild | Event-driven Quiescence (0% CPU at idle; dirty flags `DIRTY_PAINT`) |
| **Layout System** | Single-pass cursor allocation | Custom flex tree with rebuilds | Two-Level Taffy Layout + Internal Children Protocol + `UnderflowPolicy` |
| **Rendering Backend** | CPU vertex tessellation $\rightarrow$ OpenGL/Glow | Tessellation $\rightarrow$ WGPU | Compute-shader 2D rendering via Vello (GPU) + TinySkia (CPU fallback) |
| **Accessibility** | Partial / Retrofitted AccessKit | Basic AccessKit integration | Day-Zero First-Class AccessKit (`Widget::accessibility`, `a11y_fixup`) |
| **UI Verification** | Manual visual inspection | Manual / Snapshot testing | Automated standards linting (`martensite-design-lint`, WCAG 2.2, ISA-101) |

---

## Migration Guides

### 1. [Migrating from egui (`from-egui.md`)](from-egui.md)
For developers transitioning from immediate-mode GUI programming.
- **Mental Model**: Moving from "re-run everything every frame" to "declare once in an arena, update via signals".
- **Key Focus**: State persistence, eliminating per-frame CPU burn, two-pass layout caching, decoupled event routing, and AccessKit tree lifetimes.

### 2. [Migrating from Iced (`from-iced.md`)](from-iced.md)
For developers transitioning from The Elm Architecture (TEA).
- **Mental Model**: Moving from "central message enum + view tree rebuild" to "fine-grained reactive signals and retained arena widgets".
- **Key Focus**: Eliminating full-tree rebuilds, replacing monolithic messages with atomic transaction batches (`batch`), Taffy two-level layout, and custom widget authoring.

### 3. Additional Guides (Upcoming)
- **Migrating from Slint (`from-slint.md`)** — Mapping `.slint` markup DSL and property bindings to Martensite builder APIs and reactive signals.
- **Migrating from React Web (`from-react-web.md`)** — Virtual DOM $\rightarrow$ Retained Arena, Hooks $\rightarrow$ Signals, CSS Flexbox $\rightarrow$ Taffy.
- **Migrating from Qt / C++ (`from-qt.md`)** — QObject signals/slots $\rightarrow$ Push-pull reactive DAG, QWidget $\rightarrow$ `Widget` trait, QAbstractItemModel $\rightarrow$ `DataTable`.

---

## General 4-Step Migration Strategy

When porting an existing application to Martensite, follow this phased strategy:

### Phase 1: State Decomposition
Identify your application's state and extract it into a dedicated reactive model module:
- Replace mutable struct fields with `Signal<T>`.
- Replace manual calculation methods with `Memo<T>`.
- Isolate asynchronous operations (HTTP requests, hardware ports) to push data into signals via `signal.set(...)`.

### Phase 2: Shell & Window Setup
Configure your window harness using `martensite::app::App`:
```rust
use martensite::prelude::*;

let app = App::build()
    .title("Migrated Application")
    .size(1280.0, 800.0)
    .build();
```

### Phase 3: Layout & Hierarchy Translation
Translate container structures into Martensite's layout primitives:
- Translate linear horizontal/vertical layouts to `Flex::row()` and `Flex::column()`.
- Add padding and backgrounds using `Container::new()`.
- Configure overlay badges, popups, and floating cards using `Stack` and `OverlayLayer`.
- Assign `RenderMinimum` floors with appropriate `UnderflowPolicy` degradation (`Clip`, `Fallback`, `Collapse`).

### Phase 4: Event Handling & Verification
- Replace polling loops with declarative event handlers in `Widget::event`.
- Annotate widgets with semantic accessibility roles (`node.set_role(...)`).
- Run `cargo martensite lint` to catch WCAG contrast violations, target size issues, and layout defects before release.

---

## Cross References

- [Task Cookbook](../cookbook/README.md) — Practical recipes for layout, data binding, and custom widgets.
- [Martensite Tutorials](../tutorials/README.md) — Foundation tutorials from hello-world to full apps.
- [Public API Design Specification](../PUBLIC_API_DESIGN.md) — Framework contracts and architectural invariants.
