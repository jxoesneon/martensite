# {{project_name}} — Industrial Dashboard Context & Agent Guardrails

This file provides architectural context and operational conventions for AI
assistants and human contributors working on `{{project_name}}`.

## 1. The 10-Line Mental Model

Martensite is a retained-mode, GPU-accelerated GUI framework built on WGPU and Vello:
- Applications are structured hierarchically: `App` → dock layouts / zones → widgets.
- Generational storage: `WidgetArena` maintains UI nodes with generational `WidgetId`s.
- Reactive state: Fine-grained signal DAG (`Signal`, `Memo`, `Effect`) drives view updates.
- No virtual DOM diffing: Reactive signals trigger targeted dirty-flag updates (`DIRTY_PAINT`, `DIRTY_LAYOUT`).
- Two-level layout: Taffy handles flexbox/grid layout; compound widgets manage internal children.
- Retained drawing: Visible widgets record drawing primitives into a persistent `PaintList`.
- GPU rendering: Vello compiles paint lists into compute shader passes executed via WGPU.
- Direct-to-host FFI: Native platform backends handle system integration without unsafe glue in apps.

## 2. Industrial Dashboard & HMI Architecture

This project implements an industrial workstation dashboard following ISA-101 / ISA-18.2 principles:
- **Dock layout**: Backed by `martensite::blessed::DockTree`, supporting binary space partitioned zones.
- **Zone hierarchy**: Each operational area is mounted as a distinct zone panel (`Zone Panel` or `Zone`).
- **ISA-101 visual grammar**: Grayscale surfaces by default; color is reserved strictly for abnormal states and alarms.
- **Signal-driven telemetry**: Instruments and gauges bind directly to reactive signals.

### Blessed & Production Widgets
- `martensite::blessed::DockTree`: Binary space partitioning dock layout for multi-monitor and multi-zone operations.
- `martensite::blessed::DataTable`: Virtualized telemetry table capable of rendering large historical logs.
- `martensite::blessed::Chart`: GPU-accelerated time-series visualization for sensor telemetry.
- `martensite::blessed::CodeEditor`: Multi-cursor editor for script and logic inspection.

### Debug Name & Lint Conventions
Assign `debug_name` to key zones and widgets:
- Suffix `@level:1..4` to declare ISA-101 operational level (Level 1 overview down to Level 4 diagnostics).
- Mark alarm widgets with `@alarm` or `@priority:1..3`.
- Suppress specific false-positive rules with `@lint:rule-id`.

## 3. Reactive Patterns

### Correct Patterns
- **Store signals in state structs**: Allocate `Signal` and `Memo` instances during initialization.
- **Derived computations**: Use `create_memo(|| derived_calculation())` for computed alarm states or metrics.
- **Transactional batching**: Wrap multiple sensor updates in `batch(|| { ... })`.

### Anti-Patterns (Do NOT use)
- **Per-frame signal creation**: Never call `create_signal` inside a `view()` or `paint()` method.
- **Side-effects in paint**: `paint()` must be pure and deterministic; never mutate reactive state in paint.
- **Unbounded effects**: Avoid circular signal writes inside `create_effect` without termination guards.

## 4. Project Toolchain Commands

Use `cargo-martensite` for standard development workflows:
- `cargo martensite dev`: Launch hot-reload development loop with live file watching.
- `cargo martensite lint`: Run standards-backed design lint against WCAG and ISA-101 rules.
- `cargo martensite check`: Run combined fmt, clippy, and design lint verification.
- `cargo martensite doctor`: Diagnose local toolchain, GPU, and accessibility drivers.

## 5. Honest Capability Notes (v{{martensite_version}})

- **Stable**: Core arena, signals DAG, Taffy flexbox layout, Vello WGPU rasterization, blessed `DockTree` and `DataTable`.
- **Unstable**: External engine embedding (`martensite-bevy`, `martensite-godot`).
- **Platform support**: Desktop (Windows, macOS, Linux) is fully supported; iOS/Android require platform feature flags.

## 6. Design Linting (`design-lint.toml`)

UI standards compliance is enforced by `martensite-design-lint` with ISA-101 HMI rules active:
- Validates contrast ratios, choice count thresholds, and alarm color discipline.
- Path-based allows in `design-lint.toml` suppress known intentional exceptions.
- Rule severities range from `off` and `info` to `warn`, `error`, and `forbid`.
