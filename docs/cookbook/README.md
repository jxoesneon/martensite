# Martensite Task Cookbook

Welcome to the Martensite Task Cookbook. This cookbook provides task-oriented,
production-ready recipes for building user interfaces with the Martensite GUI
framework.

In accordance with Martensite's developer experience principles (D6: task-oriented,
drift-guarded), each recipe is structured around real-world developer goals:
**Goal $\rightarrow$ Complete Runnable Pattern $\rightarrow$ Under the Hood $\rightarrow$ Common Mistakes**.

---

## Cookbook Index

### Core Architecture & Foundational Recipes

| Recipe | Focus Area | Key APIs & Concepts |
|---|---|---|
| [**01. Responsive Layout**](01-responsive-layout.md) | Adaptive sizing, two-level layout, space degradation | `Flex`, `Container`, `Stack`, `LayoutEngine`, Taffy styles, `UnderflowPolicy` (`Allow`, `Lint`, `Clip`, `Hide`, `Scrim`, `Fallback`, `Collapse`), `RenderMinimum` |
| [**02. Reactive Data Binding**](02-data-binding.md) | Push-pull state propagation, derived views, transaction batching | `Signal<T>`, `Memo<T>`, `Effect`, `batch`, `flush`, `create_signal`, `create_memo`, `create_effect`, `NodeFlags::DIRTY_PAINT` |
| [**03. Custom Painting & Silhouettes**](03-custom-painting.md) | GPU vector rendering, interactive outlines, design-lint | `Widget` trait, `PaintList`, `PaintContext`, `Shape`, `CornerStyles`, `hit_shape`, `clip_shape`, `paint_underflow`, provenance scopes, design-lint tags |

---

## Forthcoming Recipes (Roadmap)

The cookbook is continuously expanded to cover standard application development needs:

4. **Building Validated Forms** — Text inputs, numeric steppers, validation signals, focus chains, and ARIA error states.
5. **Virtualizing Large Data Sets** — Rendering 1,000,000 items at 120 FPS using windowed viewport culling and spatial indexing.
6. **Asynchronous Data & Network Streams** — Wiring async futures, Tokio channels, and streaming data into reactive signals.
7. **Design Tokens & Dynamic Theming** — Oklab color schemes, dark mode transitions, contrast ratio enforcement, and runtime token overrides.
8. **Keyboard Navigation & Focus Traps** — 2D spatial focus beams, tab order cycles, shortcut managers, and modal traps.
9. **Drag & Drop with System Clipboard** — Intra-app and OS clipboard integration, custom MIME types, and spatial drop targets.
10. **Headless Component Testing** — Golden image rendering, `VirtualClock` simulation, and automated regression testing with `martensite-test`.
11. **Localization & BiDi Layout** — Fluent integration, RTL script mirroring, and dynamic font fallback cascades.
12. **Automated Design Linting in CI** — Setting up `cargo martensite lint`, ISA-101 and WCAG rule compliance, and autofix pipelines.

---

## Architectural Quick-Reference

When implementing any recipe, keep the core Martensite architecture in mind:

```
+-------------------------------------------------------------------------+
|                              Application                                |
|   State in Signal<T> / Memo<T>  <--->  Event Routing & Focus Manager    |
+-------------------------------------------------------------------------+
                                    |
                                    v
+-------------------------------------------------------------------------+
|                  Generational Arena (WidgetArena)                       |
|   HotNode (64-byte aligned: bounds, flags) | ColdNode (Widget trait)    |
|   Two-Level Layout: Taffy (Arena Nodes) <-> Internal (Flex/Stack/Box)   |
+-------------------------------------------------------------------------+
                                    |
                                    v
+-------------------------------------------------------------------------+
|                       Paint Stream (PaintList)                          |
|   Vector Shapes, Clips, Text Runs, PushScope/PopScope Provenance        |
+-------------------------------------------------------------------------+
                                    |
                    +---------------+---------------+
                    v                               v
+---------------------------------------+   +-----------------------------+
|        Vello Compute Pipeline         |   |    martensite-design-lint   |
|   GPU Bump Allocator -> Direct WGPU   |   |  WCAG 2.2, ISA-101, Fitts   |
|   TinySkia (CPU Fallback)             |   |  Automated Design Audit     |
+---------------------------------------+   +-----------------------------+
```

1. **Retained-Mode Generational Arena**: Widgets are allocated into a `WidgetArena`. Handles (`WidgetId`) carry a generational counter to eliminate use-after-free or dangling references.
2. **Push-Pull Reactivity**: State modifications push dirty bits along a directed acyclic graph (DAG) with 3-color cycle detection. Computations pull values lazily on demand.
3. **Two-Level Layout**: Taffy computes positions for arena-level nodes; container widgets (`Flex`, `Container`, `Stack`) resolve their internal children during their own `layout` pass.
4. **Vector Compute Rendering**: Painting records declarative draw commands into a `PaintList`. The Vello backend executes GPU compute shaders; software CPU fallback is provided via TinySkia.
5. **Built-In Standards Enforcement**: Every paint command carries widget scope provenance, allowing `martensite-design-lint` and `paint_audit` to continuously verify accessibility, contrast, and cognitive layout rules.

---

## Cross References

- [Martensite Tutorials](../tutorials/README.md) — Step-by-step beginner guides from setup to accessibility.
- [Migration Guides](../migration/README.md) — Architectural transitions from egui, iced, and other toolkits.
- [Design Standards](../design-standards/README.md) — Comprehensive design-lint rules (WCAG, ISA-101, HCI laws).
- [Documentation Index](../INDEX.md) — Complete repository reference and architecture specifications.
