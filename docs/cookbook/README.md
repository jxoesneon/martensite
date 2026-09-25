# Martensite Task Cookbook

Welcome to the Martensite Task Cookbook. This cookbook provides task-oriented,
production-ready recipes for building user interfaces with the Martensite GUI
framework.

In accordance with Martensite's developer experience principles (D6: task-oriented,
drift-guarded), each recipe is structured around real-world developer goals:
**Goal $\rightarrow$ Complete Runnable Pattern $\rightarrow$ Key Architectural Invariants $\rightarrow$ Common Pitfalls & Antipatterns**.

---

## Cookbook Index

The complete 12-recipe suite covers the full lifecycle of application development in Martensite, from basic layout and reactive state to advanced hardware integration, headless testing, and CI verification:

| Recipe | Description | Difficulty | Primary Crates | Key APIs & Concepts |
|---|---|:---:|---|---|
| [**01. Responsive Layout**](01-responsive-layout.md) | Adaptive sizing, two-level layout, space degradation | Intermediate | `martensite-core`, `martensite-layout` | `Flex`, `Container`, `Stack`, `LayoutEngine`, Taffy styles, `UnderflowPolicy` (`Allow`, `Lint`, `Clip`, `Hide`, `Scrim`, `Fallback`, `Collapse`), `RenderMinimum` |
| [**02. Reactive Data Binding**](02-data-binding.md) | Push-pull state propagation, derived views, transaction batching | Intermediate | `martensite-reactive`, `martensite-core` | `Signal<T>`, `Memo<T>`, `Effect`, `batch`, `flush`, `create_signal`, `create_memo`, `create_effect`, `NodeFlags::DIRTY_PAINT` |
| [**03. Custom Painting & Silhouettes**](03-custom-painting.md) | GPU vector rendering, interactive outlines, design-lint | Advanced | `martensite-render`, `martensite-core` | `Widget` trait, `PaintList`, `PaintContext`, `Shape`, `CornerStyles`, `hit_shape`, `clip_shape`, `paint_underflow`, provenance scopes, design-lint tags |
| [**04. Building Validated Forms**](04-form-validation.md) | Text inputs, numeric steppers, validation signals, focus chains | Beginner | `martensite`, `martensite-access` | `TextInput`, `ValidationSignal`, `FocusChain`, ARIA error states, AccessKit bridges |
| [**05. Virtualizing Large Data Sets**](05-virtualized-lists.md) | Rendering 1,000,000 items at 120 FPS via windowed culling | Advanced | `martensite-layout`, `martensite-render` | `VirtualList`, `ViewportCull`, spatial indexing, dynamic item heights, scroll recycling |
| [**06. Asynchronous Data Streams**](06-async-data.md) | Wiring async futures and Tokio streams into reactive signals | Intermediate | `martensite-reactive`, `tokio` | Async channels, background executors, cancellation tokens, signal bridging |
| [**07. Design Tokens & Dynamic Theming**](07-theming-tokens.md) | Oklab color schemes, dark mode transitions, contrast ratio enforcement | Intermediate | `martensite-theme`, `martensite-access` | `TokenKey`, `Oklab`, WCAG contrast ratios, dynamic palette switching, token overrides |
| [**08. Keyboard Navigation & Focus Traps**](08-keyboard-focus.md) | 2D spatial focus beams, tab order cycles, and modal traps | Advanced | `martensite-focus`, `martensite-window` | `FocusManager`, `SpatialBeam`, tab order rings, modal traps, directional arrow navigation |
| [**09. Drag & Drop and System Clipboard**](09-dnd-clipboard.md) | Multi-MIME clipboard, lazy payload evaluation, detached drag sessions | Advanced | `martensite-clipboard`, `martensite-dnd` | `ClipboardItem`, `ClipboardPayload::Lazy`, `DndSession`, `DropTarget`, `DropEffectMask`, multi-window survival |
| [**10. Headless Component Testing**](10-headless-testing.md) | Zero-jitter testing via VirtualClock, event routing, DSSIM golden diffing | Intermediate | `martensite-test`, `martensite-window` | `HeadlessHarness`, `VirtualClock`, `EventRouter`, `HotNode::bounds`, DSSIM perceptual diffing, `GoldenImages` |
| [**11. Localization & BiDi Layout**](11-localization-bidi.md) | Fluent localization, UAX #9 BiDi mirroring, UAX #50 vertical text, font cascades | Advanced | `martensite-l10n`, `martensite-text`, `martensite-font-fallback` | `L10n`, `FluentArgs`, `ScriptDirection::Rtl`, `WritingMode::VerticalRl`, `PlatformCascadeResolver`, `DirectWriteFontFallback` |
| [**12. Automated Design Linting in CI**](12-ci-design-lint.md) | Static UI design linting in CI, design-lint.toml, inline markers, autofix engine | Intermediate | `martensite-design-lint`, `cargo-martensite` | `lint_paint_list`, `design-lint.toml`, `@lint:` inline markers, `autofix` convergence, WCAG 2.2 and ISA-101 gates |

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
