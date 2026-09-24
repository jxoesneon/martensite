# Spec: In-App Widget Inspector (W1)

**Constraints:** D1 (in-app, version-locked), D2 (Martensite-built,
<0.1 ms/frame hot path), D7 (lazy tree, select-mode-first, node-level
findings).
**Crates:** `martensite-devtools` (new `inspector` module), reads
`martensite-core`, `martensite-access`, `martensite-design-lint`.
**ADR:** ADR-0036 (in-app hosting model).

## Goal

Give a developer the Chrome-DevTools gesture — *click a pixel, get the
widget, see why it's there* — inside a running Martensite app, without
any external process.

## Activation

```rust
// Debug builds only; a no-op without the `devtools` feature.
window.enable_devtools(DevToolsOptions {
    inspector_key: KeyCombo::F12,
    select_mode_key: KeyCombo::CTRL_SHIFT_C, // Chrome convention
    ..Default::default()
});
```

F12 toggles the inspector overlay. Ctrl+Shift+C arms select mode: the
next click does not reach the app — it resolves the hit-test path and
opens the inspector on the topmost widget, with the full ancestry chain
highlighted.

## Panels

### 1. Tree panel (lazy — D7)

- Renders the `WidgetArena` as an indented tree: `debug_name`,
  `WidgetId`, bounds, `NodeKind`.
- **Lazy expansion**: children are materialized on expand, matching
  Chrome's a11y-tree lesson — eager construction of a 1M-row DataGrid's
  arena is a non-starter. Virtualized list children show a `+N rows`
  placeholder.
- Hover a tree row → outline the widget in the app (reverse of select
  mode).
- Badges: `⚠` for active lint findings, `↻` for widgets whose signals
  fired this frame, `⛔` for suppressed findings.

### 2. Layout panel — "why is it this size?"

For the selected widget:

- Final bounds vs. intrinsic measure, and the Taffy style that produced
  it (flex/grid/block params resolved, not raw).
- The constraint chain: `parent offered X → style resolved Y →
  allocated Z`, one line per ancestor up to the root.
- Constraint violations in red, with the same visual language as
  Flutter's flex explorer: main/cross axis diagram, overflow tape on
  the widget itself in the app view.

### 3. Properties panel

- Widget metadata the framework already knows: `debug_name`, semantic
  markers (`@level`, `@lint`, `@alarm`…), class/role, AccessKit role,
  focus chain position, scroll-region membership.
- Reactive bindings: which `Signal`s this widget subscribes to and
  their current values (read-only display; mutation lives in W5).
- Jump affordance: `debug_name` carries `#[track_caller]`-style source
  location when the `devtools-source-spans` feature is on — the panel
  prints `src/pages/overview.rs:142` as a clickable path (same
  terminal-link convention as design-lint `see:` lines).

### 4. Lint panel

- Design-lint findings for the selected node and its subtree, run live
  against the last frame's `LintScene` (see DEV_LINT.md).
- Grouped Active / Suppressed (with `suppressed_by` provenance) —
  three-bucket report exactly as the CLI prints it.
- Each finding links `docs/design-standards/rules/<id>.md` and offers
  the autofix preview where `LintFix` exists.

### 5. Accessibility panel

- The AccessKit `TreeUpdate` as emitted: role, name, states, actions —
  the computed-properties view Chrome DevTools exposes.
- Toggle: swap the tree panel between *widget tree* and *a11y tree*
  (the Chrome full-tree toggle pattern), with selection synced
  bidirectionally between the two.
- Screen-reader announcement log: live-region events as they fire.

### 6. Events panel

- Ring buffer from W6: last N dispatched events with hit-test path and
  disposition (`Handled`/`Ignored`/`bubbled to …`).
- Filter by widget; selecting an entry highlights the dispatch path.

## Implementation notes

- The overlay is a second `OverlayLayer` tree above the app — built
  from ordinary Martensite widgets (`Tree`, `DataGrid`, `Tabs`). It
  must never appear in the app's own hit-test, focus, a11y, or lint
  scenes: the inspector subtree is tagged and excluded from
  `LintScene::from_paint_list` and the AccessKit adapter (like Chrome's
  DevTools UI not being part of the page DOM).
- Overhead gate: inspector closed ⇒ zero per-frame cost (the overlay
  tree is not built). Inspector open ⇒ ≤ the existing devtools
  `< 0.1 ms/frame` budget for data collection; rendering is ordinary
  retained painting.
- Select mode reuses the production hit-test path — the inspector
  must see *exactly* what a user click would see, including
  `hit_test_enabled` and occlusion semantics. No parallel geometry
  implementation (D7: node-level truth).

## Acceptance gates

1. Select-mode click on any dashboard widget resolves the identical
   `WidgetId` the production router logs for a real click.
2. Tree panel stays interactive with a 1M-row DataGrid visible
   (lazy-expansion test; expansion of a virtualized subtree is O(visible
   rows), not O(arena)).
3. Inspector closed → measurable zero overhead (no arena walk, no
   overlay paint).
4. Inspector + hot reload: the inspector survives a cdylib swap and
   re-resolves the selected widget by `debug_name` path (D4 identity
   contract).
5. Inspector is absent — symbols and overlay — in release builds.
