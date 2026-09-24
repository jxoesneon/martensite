# Spec: Runtime Design-Lint Bridge (W4)

**Constraints:** D1 (in-process, version-locked), D7 (node-level
findings, lazy).
**Crates:** `martensite-devtools` (bridge), `martensite-design-lint`
(engine, unchanged), `tools/cargo-martensite` (`lint` attach mode).
**ADR:** ADR-0038 (dev channel).

## Goal

Make the 51-rule design lint reachable from a *running application*
instead of only from the dashboard example's test harness. Three
surfaces, one engine:

```text
PaintList (per frame)
   │
   ├─► in-app: LintScene + lint() ──► inspector Lint panel (W1)
   │                                  HUD "lint" badge count
   ├─► dev channel (ADR-0038) ──► cargo martensite lint (attach)
   └─► env dump: MARTENSITE_LINT_DUMP=out.bin ──► lint --scene
```

## The bridge API

```rust
// martensite-devtools, behind `devtools` feature.
pub struct LintBridge {
    config: LintConfig,      // loaded from the app's design-lint.toml
    last_scene: Option<LintScene>,
    last_report: Option<LintReport>,
}

impl LintBridge {
    /// Feed the frame's paint list. Cheap early-out when nothing
    /// changed: if the scene fingerprint equals the previous frame's,
    /// the previous report is reused — lint is O(frame-change), not
    /// O(frame).
    pub fn on_frame(&mut self, list: &PaintList) -> Option<&LintReport>;

    pub fn report(&self) -> Option<&LintReport>;
    pub fn scene(&self) -> Option<&LintScene>;
    pub fn findings_for(&self, node_path: &str) -> &[Finding];
}
```

- The engine is unchanged; the bridge is a caching owner. `LintScene`
  gains a `fingerprint()` (the same hash autofix's cycle-detector
  already computes — reuse, don't duplicate).
- `on_frame` is called by the render loop only when the `devtools`
  feature is on and lint is enabled in `martensite.toml`. Skipped
  frames cost one hash of the paint list — inside the <0.1 ms budget
  for typical scenes; documented cap with a warning when a scene
  exceeds it (D2).

## Surfaces

### 1. Inspector Lint panel (W1 §4)

Findings for selected node + subtree; three-bucket grouping; `see:`
doc links; autofix *preview* (the fix model mutates a LintScene, so
the panel can show the post-fix diff against the live scene — a
convergence preview, exactly as documented for the engine).

### 2. HUD badge

`lint: 12w 3e` in the existing HUD corner — zero interaction cost,
always visible, opens the Lint panel on click.

### 3. CLI attach (`cargo martensite lint`)

Dev-channel request `LintPull { window_id }` → app serializes the last
`LintReport` + scene → CLI renders the standard report. `--fix`
requests `LintApply { ops }` — the app applies `FixOp`s to a *copy* of
the scene and reports the converged result; **fixes never mutate the
live widget tree** (the LintScene is a model; applying to the real
arena is W5's separate, deliberate mechanism).

### 4. Offline dump

`MARTENSITE_LINT_DUMP=path` (or `--emit-lint-dump`) writes the frame's
serialized `LintScene` + `PaintList` on exit or on demand. Format:
`bincode`/`postcard` snapshot with a version tag — gated for
`cargo martensite lint --scene`, replayable in tests, and the substrate
for future visual-diff tooling.

## What this deliberately is not

- Not a CI gate replacement: attach mode is for the dev loop; CI still
  lints via the test harness or `--scene` dumps (deterministic).
- Not a score or dashboard: findings stay node-level with provenance
  (D7). No aggregate number anywhere in the UI.
- Not a mutation path: the bridge reads frames; it cannot move a
  widget. Keeping lint read-only preserves its trust model.

## Acceptance gates

1. Dashboard app with `devtools` on: HUD badge count equals the
   `cargo martensite lint` attach-mode finding count for the same
   frame (cross-surface consistency test).
2. `on_frame` on an unchanged scene performs no re-lint (fingerprint
   short-circuit — assert via instrumentation counter).
3. `--scene` offline lint of a dump produces byte-identical findings
   to attach mode on the same frame.
4. Lint disabled in `martensite.toml` ⇒ bridge does not build the
   `LintScene` at all (zero-cost-off test).
5. `design-lint.toml` edited → `LintBridge` reloads config and re-
   evaluates on next frame (watch test; same reload semantics as
   SCAFFOLDING's `init --lint`).
