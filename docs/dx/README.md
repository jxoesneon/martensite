# Developer Experience Initiative

This directory contains the specifications for the DX workstreams
identified by `docs/research/DEVELOPER_EXPERIENCE_AUDIT.md`. Every spec
is bound to the audit's design constraints (D1–D8) — the distilled
lessons from competitor failure modes. A spec that cannot name the
constraints it satisfies is incomplete.

## Principles

1. **Feedback-loop speed is the metric.** Every deliverable is measured
   by the time between a developer's intent and a trustworthy rendered
   or reported result.
2. **The tool and the app are the same version.** No external DevTools
   artifact, no version skew, no service workers (D1).
3. **Dev features compile out.** Everything here lives behind
   `devtools`-class features and is absent — code and cost — from
   release builds (D8).
4. **Dogfood.** The inspector, HUD panels, and catalog are built with
   Martensite widgets (D2). If our own tooling is painful to build,
   that *is* the bug report.
5. **Never crash to desktop.** Every dev-tool failure degrades to a
   diagnostic, keeping the last-known-good UI live (D3, D4).
6. **Node-level truth, no scores.** Findings always identify the
   widget; we never emit an aggregate quality score (D7).

## Spec index

| Spec | Workstream | Primary constraints |
| --- | --- | --- |
| [INSPECTOR.md](INSPECTOR.md) | In-app widget inspector | D1, D2, D7 |
| [CLI.md](CLI.md) | `cargo-martensite` command surface | D3, D5 |
| [SCAFFOLDING.md](SCAFFOLDING.md) | `new`/`init` + agent-native context | D5, D6 |
| [DEV_LINT.md](DEV_LINT.md) | Runtime design-lint bridge | D1, D7 |
| [LIVE_TWEAKS.md](LIVE_TWEAKS.md) | Runtime property mutation | D4, D8 |
| [EVENT_DEBUGGING.md](EVENT_DEBUGGING.md) | Event/hit-test/focus observability | D2 |
| [ERROR_SURFACE.md](ERROR_SURFACE.md) | Dev-mode in-app diagnostics | D2, D7 |
| [ONBOARDING.md](ONBOARDING.md) | Widget catalog, cookbook, migration docs | D6 |

## Architecture in one diagram

```text
┌────────────────────────── user application ─────────────────────────┐
│  WidgetArena ──┐        ReactiveRuntime ──┐      AccessKit tree ──┐ │
│                │                          │                     │ │
│        ┌───────▼──────────────────────────▼─────────────────────▼─┐ │
│        │            martensite-devtools (feature: devtools)       │ │
│        │  Inspector overlay │ HUD │ Event log │ Lint panel        │ │
│        └───────▲──────────────────────────▲───────────────────────┘ │
│                │ same-process, same version (D1)                   │ │
│  PaintList ────┴──► LintScene ──► design-lint findings              │ │
│                                                                    │ │
│  martensite-host ── dev channel (unix socket, ADR-0038) ──┐         │ │
└───────────────────────────────────────────────────────────┼────────┘
                                                            │
┌─────────────────────────── CLI ───────────────────────────▼───────┐
│ cargo martensite dev │ lint │ inspect │ doctor │ new │ init      │
└────────────────────────────────────────────────────────────────────┘
```
