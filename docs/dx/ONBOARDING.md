# Spec: Onboarding & Documentation Depth (W8)

**Constraints:** D6 (task-oriented, drift-guarded).
**Surfaces:** `examples/widget_catalog`, `docs/tutorials/`, `docs/cookbook/`,
`docs/migration/`, docs.rs.

## Goal

Close the "archaeology" gap (egui #7147 is the reference failure): a
developer should be able to go from `cargo martensite new` to a
working, idiomatic feature without reading framework source.

## Deliverables

### 1. Widget catalog example (`examples/widget_catalog`)

The browsable, executable API map — deliberately separate from the
industrial dashboard (which is *contextual*; the catalog is
*reference*):

- One page per widget family (controls, containers, overlays, data,
  navigation, media, instrumentation), each widget rendered in its
  standard states (default/hover/disabled/focused/error).
- Every catalog entry shows: the widget, its minimal-code snippet
  (extracted verbatim from the source — the snippet *is* the code
  that rendered it, not a copy that can drift), and its AccessKit
  role.
- Searchable by name + alias (Qt/GTK/WinUI/AntD/SwiftUI parity names —
  the crosswalk users actually search for).
- Doubles as the design-lint clean-slate demo: the catalog passes its
  own `design-lint.toml` with zero unsuppressed findings — the
  canonical example of a lint-clean app.
- Runs the in-app inspector: the catalog is the canonical inspector
  dogfood target (select any widget → see its tree/docs).

### 2. Task cookbook (`docs/cookbook/`)

Task-oriented, not API-oriented (D6). Each recipe: goal → complete
runnable code → the pattern → common mistakes. Initial set ordered by
how often the task occurs:

1. Build a form (inputs, validation state, focus order)
2. Virtualize a large list/grid (the 1M-row path)
3. Async data into widgets (loading → data → error, signal lifecycle)
4. Custom widget (the `Widget` trait contract end-to-end)
5. Layout a dashboard page (zones, dock, scroll regions)
6. Theme + dark mode via design tokens
7. Keyboard navigation + focus traps + shortcuts
8. Drag & drop + clipboard round-trip
9. Multi-window + docking workspaces
10. Test a widget headlessly (`martensite-test`, `VirtualClock`,
    render-test goldens)
11. Localize (Fluent, BiDi mirroring)
12. Run design-lint in CI (the `cargo martensite lint` workflow)

### 3. Migration guides (`docs/migration/`)

`MIGRATION_GUIDE_0x_to_1x.md` covers version upgrades. These are
*framework-to-framework* guides — the audience arriving from:

- `from-egui.md`: immediate → retained mental model, the three
  biggest traps (per-frame state, `ctx`-style globals → signals,
  layout model differences).
- `from-react-web.md`: component/VDOM → widget tree, hooks → signals,
  CSS → layout+theme tokens.
- `from-slint.md`: DSL → builder API, where each `.slint` concept maps.
- `from-qt.md` (later): signals/slots, model/view → DataGrid contract.

### 4. Docs drift guards (D6)

The failure in egui was *staleness*. Every doc format gets a guard:

- Cookbook recipes: each is a compilable doctest or an
  `examples/cookbook/` binary referenced from the page — CI compiles
  them; a broken API breaks the recipe's build, not silently the doc.
- Tutorials: existing 01–04 verified by the snippet-extraction check;
  numbered sequence extended to the widget-catalog entry points.
- API docs: doctest coverage stays enforced (AGENTS.md §3); each
  facade widget's doc page links its cookbook recipe + catalog entry
  (the discoverability triangle: API ↔ recipe ↔ live example).
- `llms.txt` + the scaffolded `AGENTS.md` (SCAFFOLDING.md) are the
  agent-consumable projections of this same corpus — one source of
  truth, multiple projections, all CI-checked.

### 5. First-run funnel (the 10-minute path)

The measured sequence every onboarding decision must serve:

```text
cargo install cargo-martensite
cargo martensite new my-app && cd my-app
cargo martensite doctor          # ✓ environment
cargo martensite dev             # running window in <60s
# edit a label → hot reload      # first visible change <5min
F12 → click a widget             # "I can see everything" <10min
cargo martensite lint            # "it tells me what's wrong" <15min
```

Each step has one doc destination and one failure mode documented.
This funnel is the acceptance criterion for W3's scaffold + W8's docs —
it is exercised end-to-end by the `scaffold_smoke` CI job.

## Acceptance gates

1. `examples/widget_catalog` compiles, runs, and passes design-lint
   with zero unsuppressed findings (CI-gated).
2. Every facade widget reachable from the catalog search by at least
   its canonical name + one parity alias (catalog coverage test).
3. All 12 cookbook recipes compile as CI-checked binaries/doctests.
4. The 10-minute funnel runs unattended in CI up to `dev` startup
   (scaffold_smoke extended).
5. No orphan docs: every docs/dx spec, cookbook recipe, and migration
   page is reachable from `docs/INDEX.md` (link-check in CI).
