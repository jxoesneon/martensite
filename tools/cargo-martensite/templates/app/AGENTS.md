# {{project_name}} — Agent Context & Developer Guardrails

This file provides architectural context and operational conventions for AI
assistants and human contributors working on `{{project_name}}`.

## 1. The 10-Line Mental Model

Martensite is a retained-mode, GPU-accelerated GUI framework built on WGPU and Vello:
- Applications are structured hierarchically: `App` → windows/zones → widgets.
- Generational storage: `WidgetArena` maintains UI nodes with generational `WidgetId`s.
- Reactive state: Fine-grained signal DAG (`Signal`, `Memo`, `Effect`) drives view updates.
- No virtual DOM diffing: Reactive signals trigger targeted dirty-flag updates (`DIRTY_PAINT`, `DIRTY_LAYOUT`).
- Two-level layout: Taffy handles flexbox/grid layout; compound widgets manage internal children.
- Retained drawing: Visible widgets record drawing primitives into a persistent `PaintList`.
- GPU rendering: Vello compiles paint lists into compute shader passes executed via WGPU.
- Direct-to-host FFI: Native platform backends handle system integration without unsafe glue in apps.

## 2. Widget Map & Debug Conventions

### Core Widgets
- `martensite::widgets::Button`: Interactive push button with primary and disabled variants.
- `martensite::widgets::Text`: Shaped text element with font size, family, and layout constraints.
- `martensite::widgets::TextInput`: Single-line editable text input with cursor and selection tracking.
- `martensite::widgets::Flex`: Flexbox container supporting row and column layout directions.
- `martensite::widgets::Container`: Single-child styling container for padding, borders, and backgrounds.
- `martensite::widgets::Stack`: Overlay layout stacking children back-to-front.
- `martensite::widgets::ScrollView`: Scrollable viewport for unbounded content dimensions.
- `martensite::widgets::Slider`: Continuous or stepped numeric range selection control.
- `martensite::widgets::CheckBox`: Two-state toggle with accessible label.

### Blessed & Production Widgets
- `martensite::blessed::DockTree`: Binary space partitioning (BSP) dock layout with draggable tabs.
- `martensite::blessed::DataTable`: High-performance virtualized grid for large datasets.
- `martensite::blessed::Chart`: GPU-accelerated time-series and scatter data visualization.
- `martensite::blessed::CodeEditor`: Multi-cursor text editor with syntax token highlighting.

### Debug Name Conventions
Assign `debug_name` to key widgets for linting and devtools:
- Use PascalCase descriptive names: `HeaderBar`, `NavigationRail`, `CounterDisplay`.
- Add inline lint suppression suffixes when intentional: `MyWidget@lint:contrast|choice-count`.
- Add ISA-101 level tags for industrial dashboards: `OverviewZone@level:1`.

## 3. Reactive Patterns

### Correct Patterns
- **Store signals in state structs**: Allocate `Signal` and `Memo` instances during initialization.
- **Derived computations**: Use `create_memo(|| derived_calculation())` for expensive derived values.
- **Transactional batching**: Wrap multiple related signal updates in `batch(|| { ... })`.

### Anti-Patterns (Do NOT use)
- **Per-frame signal creation**: Never call `create_signal` inside a `view()` or `paint()` method.
- **Side-effects in paint**: `paint()` must be pure and deterministic; never mutate reactive state in paint.
- **Unbounded effects**: Avoid circular signal writes inside `create_effect` without termination guards.

## 4. Project Toolchain Commands

Use `cargo-martensite` for standard development workflows:
- `cargo martensite dev`: Launch hot-reload development loop with live file watching.
- `cargo martensite lint`: Run standards-backed design lint against WCAG and ergonomics rules.
- `cargo martensite check`: Run combined fmt, clippy, and design lint verification.
- `cargo martensite doctor`: Diagnose local toolchain, GPU, and accessibility drivers.

## 5. Honest Capability Notes (v{{martensite_version}})

- **Stable**: Core arena, signals DAG, Taffy flexbox layout, Vello WGPU rasterization, basic widgets.
- **Unstable**: GDExtension (`martensite-godot`) and Bevy engine integration bridges.
- **Platform support**: Desktop (Windows, macOS, Linux) is fully supported; iOS/Android require platform feature flags.

## 6. Design Linting (`design-lint.toml`)

UI standards compliance is enforced by `martensite-design-lint`:
- Rules cover WCAG 2.2 AA contrast, touch/click target sizing, and typography hierarchy.
- Path-based allows in `design-lint.toml` suppress known intentional exceptions.
- Rule severities range from `off` and `info` to `warn`, `error`, and `forbid`.
