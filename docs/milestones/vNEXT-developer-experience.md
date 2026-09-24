# Milestone Specification: Developer Experience Initiative

**Status:** Draft — **version assignment pending council.** The
`v0.19.0` slot shipped as Widget Breadth & Developer Experience
(2026-09-23), so candidates are `v0.20.0` (shared with or after
Distribution) or post-1.0. This doc defines the workstreams; it does
not presume the version.

## 1. Executive Summary & Objectives

Close the DX gap identified by
`docs/research/DEVELOPER_EXPERIENCE_AUDIT.md`: the framework internals
are ahead of the tooling surface. This milestone surfaces what exists
(design lint, TimeMachine, hot reload) and adds what best-in-class
toolchains have that we don't — a widget inspector, a real CLI,
scaffolding, and event observability — while *avoiding the documented
failure modes* of Flutter DevTools, Dioxus subsecond, Slint live
preview, and cargo-generate templates (constraints D1–D8).

The governing principle: **feedback-loop speed and honesty**. Every
workstream is measured by the time between a developer's intent and a
trustworthy rendered or reported result.

## 2. Evidence Base & Constraints

* Audit: `docs/research/DEVELOPER_EXPERIENCE_AUDIT.md` — competitive
  inventory, gap ranking, mistakes harvest, design constraints D1–D8.
* ADRs: ADR-0036 (in-app inspector), ADR-0037 (hot-reload contract),
  ADR-0038 (dev-channel transport).
* Specs: `docs/dx/` — one per workstream, each bound to the D-
  constraints it defends.

## 3. Target Crates & Modules

- `tools/cargo-martensite` — CLI expansion (W2, W3)
- `martensite-devtools` — inspector, lint bridge, tweak registry,
  event ledger, error surface (W1, W4, W5, W6, W7)
- `martensite-macros` — `#[tweak]` / source spans (W5)
- `martensite-window` / `-focus` / `-dnd` — dispatch instrumentation
  (W6)
- `martensite-layout` / `-render` — structured diagnostics (W7)
- `martensite-host` — dev channel, reload contract (ADR-0037/38)
- `examples/widget_catalog`, `docs/cookbook`, `docs/migration` (W8)
- `martensite-design-lint` — engine unchanged; bridge consumes it (W4)

## 4. Entry Criteria

- `v0.18.0` released (production hardening complete; API audit gives
  the stable surface the docs and scaffold will teach).
- Version slot assigned by council (see Status).
- ADRs 0036–0038 ratified.

## 5. Architectural Deliverables — Workstreams

| WS | Spec | Deliverable | Exit gate (headline) |
| --- | --- | --- | --- |
| W1 | `docs/dx/INSPECTOR.md` | In-app inspector: select mode, lazy tree, layout chain, properties, lint, a11y tree, events | Select-mode resolves the identical `WidgetId` production hit-test logs; 1M-row tree stays interactive |
| W2 | `docs/dx/CLI.md` | `new`, `init`, `lint`, `inspect`, `doctor`, `check` | `new→doctor` is the 60s first run; `lint` attach ≡ offline; version-mismatch exits loudly |
| W3 | `docs/dx/SCAFFOLDING.md` | 3 templates + scaffolded `AGENTS.md`/`llms.txt`/`design-lint.toml`; CI `scaffold_smoke` | Generated project fmt/clippy/build/test/lint-clean in CI on every PR |
| W4 | `docs/dx/DEV_LINT.md` | `LintBridge` + inspector panel + CLI attach + `--scene` dump | HUD badge count ≡ CLI attach count ≡ offline count on same frame |
| W5 | `docs/dx/LIVE_TWEAKS.md` | `TweakRegistry` + inspector editors + span write-back | Tweak survives hot reload by name; release build has zero tweak symbols |
| W6 | `docs/dx/EVENT_DEBUGGING.md` | `EventRecord` ledger + dispatch instrumentation + env stream + panel | Click on disabled widget → ledger records path + rejection; ledger-off cost ≈ 0 |
| W7 | `docs/dx/ERROR_SURFACE.md` | Inline overflow tape + diagnostics overlay + structured dev panic | Forced overflow renders hatch+amount; dev panic shows node path + crash bundle |
| W8 | `docs/dx/ONBOARDING.md` | Widget catalog, 12-recipe cookbook, 3+ migration guides, drift guards | Catalog lint-clean; all recipes CI-compiled; 10-min funnel runs in CI |

## 6. Sequencing

Dependency order (parallelizable within a tier):

- **Tier 0 (foundation):** ADR-0038 dev channel + `LintBridge` (W4
  engine-side) + event ledger (W6) — these are the data sources the
  inspector renders.
- **Tier 1 (surface):** W1 inspector panels, W2 CLI commands, W3
  scaffolding — can proceed in parallel once Tier 0 lands.
- **Tier 2 (integration):** W5 tweaks (needs inspector panel + reload
  contract), W7 error surface (needs W4 + W6), W8 onboarding (needs W3
  scaffold for the funnel).
- Suggested subagent slots per the established double-loop method:
  implementation agents on disjoint crate/file sets, then adversarial
  reviewers per workstream.

## 7. Verified Invariants

1. **Zero-cost-when-off:** every dev feature behind `devtools`-class
   features is absent — symbols, cost, and surface — in release
   builds. Verified by a release `nm`/feature-gate test.
2. **One truth:** the inspector, event ledger, and lint bridge read
   the *production* hit-test/arena/paint paths — no parallel geometry
   or shadow scene that can drift (D7).
3. **Never crash:** every dev-tool failure degrades to a diagnostic
   keeping last-known-good UI live (D3).
4. **No scores:** findings always identify a node; no aggregate
   quality metric exists anywhere in the tooling (D7).
5. **Version lock:** any wire carries a handshake that fails loudly
   on mismatch (D1).

## 8. Exit Criteria & Verification Gates

1. All eight workstream acceptance gates (spec docs) pass.
2. The 10-minute first-run funnel executes in CI end-to-end
   (scaffold_smoke extended).
3. Standard local gates: fmt, clippy `-D warnings` both feature sets,
   workspace tests, doctests, `cargo doc -D warnings`.
4. Release-mode binary of the dashboard contains no `devtools`,
   `tweak`, or dev-channel symbols (invariant 1 gate).
5. `docs/INDEX.md` and `WORKING_ON.md` updated; each workstream spec
   linked from both.

## 9. Risks

- **Scope breadth** — eight workstreams is a lot; Tier-0 sequencing +
  hotswappable subagent slots (the method used for the design-lint
  expansion) is the mitigation. Each WS is independently shippable —
  the milestone degrades gracefully to "W1+W2+W3 land" without losing
  coherence.
- **Inspector self-inclusion bugs** — the overlay must be excluded
  from a11y/lint/focus; the exclusion contract is a named deliverable
  with its own test, not a side note.
- **Docs rot recurrence** — mitigated structurally: cookbook recipes
  compile in CI, catalog is lint-gated, scaffold smoke-runs on PR.
- **Channel creep** — the dev channel stays read-mostly; any write
  RPC requires a new ADR (ADR-0038 makes mutation a separate door).
