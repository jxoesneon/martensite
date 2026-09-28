# [ADR-0040] Native Loading State — Widget-Declared, Arena-Enforced Skeleton Mode

* **Status:** Accepted
* **Date:** 2026-09-27
* **Deciders:** Martensite Architecture Working Group
* **Technical Domain:** `martensite-core` (widget protocol, arena, node flags), `martensite` (widget library, `Skeleton`), `martensite-access` (AccessKit adapter), `martensite-window` (hit-testing, dispatch), `martensite-focus`, `martensite-design-lint`, `martensite-devtools`/`martensite-mcp` (dev-channel forcing)
* **Amends:** Companion to ADR-0038 (dev channel) and ADR-0039 (MCP server); extends the `UnderflowPolicy` degraded-state protocol with its temporal sibling.

---

## Context and Problem Statement

Martensite applications load content asynchronously — fetches, decodes, enumerations, streams. The cookbook already teaches an `AsyncState::{Loading, Ready, Error}` pattern, but the `Loading` leg has no first-class render path: apps either paint an empty surface (reads as broken), compose `EmptyState`/`Spinner` ad hoc, or wrap widgets in the `Skeleton` widget.

`Skeleton::wrap` is a real widget shipped and stable, but it is structurally unable to express the states that matter most:

1. **Overlay-hosted content is unreachable.** `AutoComplete`, `CommandPalette`, `Mention`, `TreeSelect`, and `Dropdown` render their option lists as `OverlayLayer` entries — raw `Box<dyn Widget>` trees painted via `paint_widget_recursive` outside the wrapped subtree. A wrapper around the face cannot skeleton a pending suggestion list.
2. **Partial loading is inexpressible.** Wrap is binary: `child_count() == 0` while loading, so the whole subtree vanishes from paint, hit-test, and a11y at once. "First 20 rows loaded, next 10 pending" — the canonical virtualized `ListView`/`Table`/`TreeView` pattern — can only be painted by the widget itself.
3. **Chrome cannot be preserved.** `Table::paint` draws face → body rows → pinned sortable header in one call. Wrapping hides the header with the data; a native path can keep the header live while the body shimmers.
4. **Semantics degrade.** A wrapped `Table` emits `Role::GenericContainer + "Loading" + busy` — losing `Role::Table`, headers, and set-size metadata. A loading node that keeps its own role and reports `busy` is strictly better for assistive technology.
5. **Measure instability.** Wrap-mode `Skeleton::measure` returns the *placeholder's* preferred size (and carries a dead-expression bug at `skeleton.rs:267-269` — `constraints.max_size.x.min(constraints.max_size.x)` — collapsing loading width to the constraint max), so reveal causes a layout pop.

Conversely, a naive rollout — a self-managed `loading` field and `if loading` paint branch on ~119 widgets — leaks: `paint_node` recurses arena children unconditionally, `hit_test_node` visits children before testing the node, the AccessKit adapter calls `widget.accessibility()` unconditionally (a skeletonized `AlarmPanel` would still announce "3 active"), overlay popups paint last and unclipped above the shimmer, and focus/dispatch can still reach descendants entered before the flag flipped. The leakage must be closed once, at the framework's chokepoints — not remembered 119 times.

## Decision Drivers

* **Established precedent:** `RenderMinimum`/`UnderflowPolicy`/`paint_underflow` already implements the exact topology needed — *widget declares* (`min_render()`), *node may override* (`ColdNode::effective_render_minimum`), *arena consumes lazily at ~11 sites* (paint, hit-test, dispatch, focus, a11y, layout). Loading is the temporal twin of underflow's spatial degradation.
* **Semantic honesty (D9):** a loading surface must not fabricate a11y children, stale labels, or hittable controls; pending is a real state, not a paint trick.
* **Zero release overhead (D8):** the mechanism is inert unless a widget opts in; the shimmer painter is shared, not duplicated.
* **API stability:** `v1.0.0-rc.1` freeze looms; all additions must be defaulted trait methods, bitflags, or additive builders. `ColdNode` is not `#[non_exhaustive]` — no new pub fields there.
* **Determinism:** lint sweeps and golden frame dumps tick at fixed dt; shimmer must run in a pinned phase mode for tests.

## Decision

### 1. Widget protocol — declaration

`martensite-core::widget::Widget` gains two defaulted methods:

```rust
/// Whether this widget's content is pending. Cheap and stable —
/// same contract as `min_render`: a constant or a flag cached by a
/// setter; never computed from layout. Loading is a
/// paint+input+a11y state, NEVER a layout state — `measure` MUST
/// NOT branch on it; `paint_loading` fills the allocated bounds.
/// Note: "loading" means "placeholder replaces content", not
/// "activity in progress" (cf. `WebView::loading()` = navigating).
fn is_loading(&self) -> bool { false }

/// Paints the pending placeholder into `cx.bounds`. The default is
/// the shared shimmer block; widgets override only where
/// shape-accuracy pays (row stacks for collections, chrome
/// preservation for Table headers, plot-area for charts).
fn paint_loading(&self, cx: &mut PaintContext, phase: f32) { /* shared shimmer */ }
```

Opting in costs ~10 lines per widget: a `loading: bool` field, `.loading(bool)` `#[must_use]` builder, `set_loading(&mut self, bool)` + `is_loading()` override. `set_loading` marks `DIRTY_PAINT | DIRTY_A11Y` — never `DIRTY_LAYOUT`.

Naming: `loading`/`is_loading`/`set_loading`/`paint_loading`. `pending` is taken (queue-drain semantics across Table/ListView); `busy` is reserved for the emitted a11y attribute; `paint_loading` parallels `paint_underflow`.

### 2. Instance override — arena-visible

`NodeFlags::LOADING = 1 << 10` on `HotNode` (bit 10 is free; `HotNode` stays 64 bytes, compile-time asserted). `WidgetArena::set_loading(id, bool)` sets/clears the bit and dirties once. Effective state:

```rust
effective_loading(node) = hot.flags.contains(LOADING) || cold.widget.is_loading()
```

This mirrors the `render_minimum` instance-vs-type precedence exactly. It gives the dev-channel and apps the ability to skeletonize *any* node — including third-party `dyn Widget` that never implemented `set_loading` — while widgets keep ownership of the temporal truth (only the widget knows when its data arrived; `Bound::push` mutates `&mut W` inside `tick` and never sees a `WidgetId`).

### 3. Arena enforcement — the chokepoint map

Every site that already consults `underflow_policy()`/`covers_input()` gains the loading consult. This is the whole point of the design: enforcement lives in ~6 framework-owned places, not 119 widget bodies.

| Site | Behavior while `effective_loading` |
|---|---|
| `paint_node` (`arena.rs`) | New arm beside the `Fallback` arm: `push_scope` (same debug-name path — baseline paths don't churn) → `paint_loading(cx, phase)` → `pop_scope` → `return`. Skips `paint_widget_body`'s internal-child walk AND the arena-children recursion — both leak classes closed in one place. Underflow stays outermost: `Hide`/`Collapse`/`Fallback`/`Scrim` precede the loading arm. |
| `paint_widget_body` / `paint_underflowed_child` (`arena.rs`) | Internal children of a loading widget aren't walked (the paint arm above returns first); a loading *internal* child paints its `paint_loading` — covers popups' recursive paint path. |
| `hit_test_node` (`martensite-window/src/hit_test.rs`) | Early return alongside the `covers_input` check → subtree unhittable. |
| `dispatch_event_ex` (`arena.rs`) | Covered check extended to `|| effective_loading`, applied to target AND ancestors (keyboard focus and `SemanticAction` delivery bypass hit-testing). |
| `OverlayLayer` (`overlay.rs`) | `sync_overlays` closes popups whose owner entered loading (the existing "invisible widget floating a live surface" predicate); `OverlayLayer::paint`/`dispatch_event` gate on `entry.content().is_loading()` — popup content bypasses `paint_node` entirely. |
| AccessKit adapter (`martensite-access/src/adapter.rs`) | Loading node: `set_busy()` + sanitized label; `ancestor_state` propagation marks descendants covered/hidden; the node's own `widget.accessibility()` is skipped so computed labels ("3 active alarms") can't leak. Pending rows in virtualized collections MUST NOT emit fabricated `posinset`/`setsize` — real indices or nothing. |
| `FocusManager` (`martensite-focus`) | Loading-covered nodes can't gain focus; focus held when the flag flips (direct or ancestor entry) is relocated — mirrors the `UnderflowPolicy::Hide` contract. |

Wrappers (`Bound`, `Clamp`, `Stack`, `Container`, …) that want a child's declared loading to surface must forward `is_loading`/`paint_loading` — same forwarding convention as `min_render`/`paint_underflow`. The `NodeFlags` override is unaffected by wrapper depth, so forwarding is convention, not correctness.

### 4. Shared machinery

* **`paint_placeholder` extraction.** `Skeleton`'s shimmer internals (`BASE`/`SHIMMER`/`BAND_FRAC`/`SWEEP_SECS`, `paint_placeholder`) move to a shared location (`martensite-core` paint helpers, or facade `widgets::skeleton_paint`); `Skeleton` delegates. One paint path to audit; `SkeletonShape` gains `Rows{count, row_height}` and `Grid{cols, rows}`.
* **Arena phase clock.** `WidgetArena::tick` advances a single `loading_elapsed` and marks `effective_loading` nodes `DIRTY_PAINT` only — all skeletons phase-lock, no per-widget `phase` fields, no per-widget `tick` impls. `paint_loading(cx, phase)` receives the phase as a parameter so `PaintContext` grows no field.
* **Paint-only dirty path.** A `mark_dirty_paint` route that sets `DIRTY_PAINT` without `DIRTY_A11Y` — shimmer frames must not re-emit AccessKit trees at 60fps. This benefits every animated widget (~36 `tick` impls), not just skeletons. Transitions dirty `PAINT|A11Y` once.
* **Reduced-motion flag.** A framework-level ambient flag on `WidgetArena` (theme-token equivalent) consumed by the shared painter — static placeholder or non-translating treatment. OS `prefers-reduced-motion` probing (`martensite-shell`/`window` territory) is phase 2.
* **`Skeleton::measure` fix.** Dead expression at `skeleton.rs:267-269` replaced by the intended min-clamp; wrap mode defers `measure` to the child while loading so reveal doesn't pop layout.

### 5. Semantics and safety contracts

* **`is_loading` default is `false` everywhere.** Only opt-in widgets carry the field.
* **Alarm/status channels excluded from generic shimmer.** `StackLight`, `AlarmPanel`, `StatusDot`, and alarm-state gauges must be excluded or paint a certified-distinct "data unknown" treatment (per ISA-101: hatched/bad-quality overlay or explicit `?`) — generic neutral shimmer over an andon tower reads as "all-clear", which is a hazard, not a style bug. `status_dot`/`badge`/`signal_strength`/`battery` stay skeleton-free (the host row's skeleton covers them).
* **Stuck-loading is lint policy, not runtime timers.** A design-lint audit rule ("skeleton visible across N sampled frames" — `audit_loading_stuck`, mirroring `audit_underflow`) plus inspector badge/MCP-visible diagnostic. Quiescence blocking while shimmer runs is documented as intentional.
* **Test determinism.** Lint sweep and golden frames run `animated(false)`/fixed-phase; `@skeleton` scope marker available for rule exemption via the existing `Name@marker` lineage convention; `LINT_GATING_BASELINE.txt` regenerated deliberately when demo surfaces adopt loading.
* **`Skeleton` widget unchanged.** It remains the explicit standalone placeholder for composite regions and third-party widgets — the `loading` flag covers "this widget's own content is pending", the wrapper covers everything else. Document the division in one sentence per type; `AsyncState::Loading`→flag, `Ready(empty)`→`EmptyState`, `Error`→`ResultPage` (already the cookbook triad).

### 6. Scope — derived, not hand-listed

* **Phase 1** (pre-rc.1): shared painter + trait defaults + `NodeFlags::LOADING` override + all chokepoint consults + paint-only dirty path + arena phase clock + arena reduced-motion flag + `Skeleton::measure` fix + **~15–25 high-value adopters**: overlay-popup widgets (`auto_complete`, `command_palette`, `mention`, `tree_select`, `dropdown`), virtualized collections (`table`, `list_view`, `tree_view`, `property_grid`, `check_list`, `message_list`, `comment_thread`, `notification_center`, `kanban`, `cascader`), one chart pilot, and a dashboard "simulate latency" toggle driven through the `Bound::push` seam.
* **Phase 2:** mechanically-derived coverage test (widgets declare skeleton participation; no hand enumeration — AGENTS §7), partial-loading vocabulary (`Option<Row>`/pending-range slots inside virtualized collections' data models), "data unknown" presentation for safety-channel widgets, `audit_loading_stuck` lint rule, OS reduced-motion detection.
* **Phase 3:** dev-channel exposure — `signal_set` on a registered latency adapter plus node-level `set_loading` through the session, giving agents a live verify loop (`set_signal` → `a11y_tree` shows `busy` → `capture_node` shows placeholder).

Everything not in the adopter set still gets a correct generic shimmer free via the trait default — "shape-accurate" is refinement, not a migration gate.

## Rejected Alternatives

* **119 self-managed flags** (`if loading` in each paint/event/accessibility): leaks paint, input, focus, and a11y through arena children and overlay popups; ~5–14k LOC of duplicated shimmer; vetoed on safety grounds.
* **Arena-only `NodeFlags`/`ColdNode::with_skeleton(spec)`**: structurally blind to internal children and `OverlayLayer` content (they have no `ColdNode`), can't express partial loading (the arena sees a node, not which rows are pending), and forces an `arena.set_loading(id)` write path the `Bound` binding model can't express.
* **Composition-only (`Skeleton::wrap`)**: proven unreachable for popups, binary-only for collections, measure-pop on reveal, generic shapes only. Kept as the escape hatch, not the mechanism.
* **`ColdNode` pub field**: breaking change under `cargo-semver-checks` (struct isn't `#[non_exhaustive]`); `NodeFlags` bit is free.
* **Per-widget `phase` fields / shimmer easing / staggered reveals / fade-in transitions**: gold-plating; the linear shared-clock sweep suffices.
* **Feature gate**: `Skeleton` is always-on stable API; gating `is_loading` would fragment the widget surface across CI's two test legs. The *driving* path is already `dev-channel`-gated.

## Consequences

* ~200–280 lines of core machinery + ~10 lines per opt-in widget + bespoke `paint_loading` only where shape pays. Universal coverage (every widget, user-defined included) with zero per-widget enforcement code.
* Two new defaulted `Widget` methods — the established additive extension path; minor-version safe, must land before `v1.0.0-rc.1`.
* New shared paint-only dirty path halves the per-frame cost of all animated widgets.
* Known debt: `Skeleton` wrapper vs. flag bifurcation documented rather than unified; alarm-channel "data unknown" presentation deferred to phase 2.

## Test Matrix

| Layer | Coverage |
|---|---|
| Trait/painter | `paint_loading` default emits ≥N commands; shape variants; zero-bounds early-out; phase pinning |
| Per-widget | `set_loading`/`is_loading` round-trip; loading suppresses internal children via `child_count`; opt-in widgets paint shape-accurate placeholders |
| Arena | paint arm substitutes and suppresses both child domains; scope balance; hit-test/dispatch/a11y/focus consults; overlay popup closure on owner-loading |
| A11y | `busy` emitted, sanitized label, descendants pruned, no fabricated `posinset`/`setsize` |
| Golden/lint | fixed-phase determinism; baseline regen; `dump_frames` golden stability |
| Dev-channel | `signal_set` → `a11y_tree` busy → `capture_node` placeholder round-trip |
| Features | default + `--all-features`; doctests on every new public item |

## Links

* `crates/martensite/src/widgets/skeleton.rs` — existing shimmer implementation being extracted
* `crates/martensite-core/src/widget.rs` — `RenderMinimum`/`UnderflowPolicy`/`paint_underflow` precedent
* `docs/cookbook/06-async-data.md` — `AsyncState` pattern this completes
* ADR-0038 (Dev Channel), ADR-0039 (MCP Server)
