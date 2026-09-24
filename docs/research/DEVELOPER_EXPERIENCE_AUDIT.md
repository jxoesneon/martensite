# Developer Experience Audit — Competitive Research & Mistakes Harvest

**Status:** Research complete. Basis for the DX initiative specs in
`docs/dx/` and `docs/milestones/vNEXT-developer-experience.md`.
**Date:** Post-v0.18.0 design-lint landing.
**Method:** Two online research rounds — (1) capability inventory of
best-in-class toolchains, (2) targeted harvesting of their documented
failure modes (GitHub issues, postmortems, documented pitfalls).

---

## 1. Executive summary

Martensite's *framework internals* are ahead of its *tooling surface*.
We already ship three things nobody else has — a 51-rule
standards-backed design lint with autofix, a journal+checkpoint
time-travel debugger over the real arena/signal graph, and a cdylib
hot-reload loop — but they are reachable only by reading source or by
running an example's test harness. The separation from best-in-class DX
is almost entirely **surfacing**: inspector, CLI breadth, scaffolding,
runtime lint bridge, and event observability. None of it requires new
architecture; all of it requires instrumentation and distribution.

The mistakes harvest (§4) is the more valuable half of this document.
Every major competitor has a documented, expensive failure mode that we
can avoid *by design decision now*, at zero cost, because the relevant
choices have not yet been made.

---

## 2. Competitive capability inventory

| Capability | Best-in-class implementation | Martensite today |
| --- | --- | --- |
| Hot reload | Dioxus `dx serve` — RSX template reload + `subsecond` jump-table patching (ms); Slint interpreter live-preview (state preserved, old UI survives syntax errors); Makepad Live system | `cargo martensite dev` cdylib split (~350 ms claimed target) |
| Widget inspector | Flutter DevTools: select mode, tree view, layout/flex explorer, constraint visualization, overflow tape | HUD only — frame histogram, dirty rects, arena telemetry. No tree, no select mode |
| Property editing | Flutter property editor writes values back to source; Slint live-preview data editor + outline panel drag-drop + undo | None |
| CLI breadth | `dx`: new/serve/build/bundle/check/translate/autoformat/doctor/self-update (13 commands); `slint-viewer`: --check/--auto-reload/--screenshot/--remote | `dev`, `build`, `help`, `version` |
| Scaffolding | `dx new`, `cargo-generate` ecosystem, `create-*` everywhere; Rosace scaffolds `AGENTS.md` + `CLI.md` into user projects | None — first run is "write Cargo.toml by hand" |
| Design linting | **Nobody** | 51 rules, autofix, suppressions — but only reachable via the dashboard example's test harness |
| Time-travel debug | iced `comet` (message log); nothing else in Rust GUI | `timemachine` (journal + arena/signal checkpoints, forward replay) — **ahead of field** |
| Event debugging | Chrome DevTools event-listener panel; Flutter logging | Two `tracing::debug!` calls in `quiescent.rs` |
| A11y inspection | Chrome DevTools full a11y tree (lazy, bidirectionally synced with DOM); Accessibility Insights FastPass/Assessment tiers; **no Rust GUI framework ships one** | AccessKit adapter emits `TreeUpdate`s; no viewer |
| Error surface | Flutter red error widget + overflow yellow-tape in dev builds | Panic or silent no-op depending on path |
| IDE integration | Slint LSP + VS Code live preview; Makepad Studio | Plain Rust — rust-analyzer suffices (no DSL = less needed) |
| Online playground | Slint live-preview web, DartPad | `examples/web` is a compile smoke test, not a playground |
| Agent-native docs | Rosace `AGENTS.md`/`CLI.md` scaffold output; Makepad agent skills; `llms.txt` convention emerging | Excellent `AGENTS.md` — for *contributors*, not scaffolded for *users* |

### 2.1 Where we already lead

- **Design lint** — no GUI framework in any language lints rendered UI
  against WCAG/ISA-101/Gestalt with citations and autofix.
- **TimeMachine** — forward-replay over real arena+signal state with
  fingerprint verification is beyond `comet`'s message-log model.
- **Test infrastructure** — `martensite-test`, `render-test`,
  `VirtualClock`, doctest enforcement on every public item.
- **Docs discipline** — doctests enforced, migration guide, deprecation
  policy, honest-limitation docs (`VENDORED_FORKS.md`, `†`-marked
  benchmark targets).

### 2.2 The gap ranking (by developer-hours cost)

1. **Widget inspector** — the single largest gap; Chrome DevTools /
   Flutter DevTools select-mode is the ecosystem's reference point and
   the most-cited reason developers stay on web tech.
2. **CLI breadth** — `new` + `lint` + `doctor` are expected on day one.
3. **Lint→runtime bridge** — our moat is unwired; no app author can
   reach it today.
4. **Scaffolding + agent-native context** — cheap, high first-run
   impact, and the ecosystem is converging on it now.
5. **Live property tweaking** — medium; no DSL means we need an
   arena-mutation channel instead of a file interpreter.
6. **Event debugging** — small change, outsized return.
7. **Onboarding depth** — 4 tutorials, no cookbook, no widget catalog.
8. **Dev-mode error surface** — visible in-app diagnostics.

---

## 3. What the research says "best" actually means

Three independent sources converge on the same conclusion:

- The egui issue tracker (#7147, "archaeology instead of development"):
  the complaint is not missing features but missing *guidance and
  feedback* — no pattern guide, no layout debugging, no way to see what
  the framework decided.
- The HN Rust-GUI thread: "DevTools is the killer feature" — right-
  click → inspect → tweak live is the workflow developers refuse to
  give up. Editing blind + recompiling is "the old paradigm."
- The five-framework shootout (Tauri/Slint/egui/Dioxus/Flutter): the
  winner was decided by *testability and feedback loops*, not renderer
  performance.

DX is not documentation volume; it is **the speed and honesty of the
feedback loop between an intent and its rendered result**.

---

## 4. Mistakes harvest — what to avoid, with evidence

This section is the specification constraint set. Every spec in
`docs/dx/` cites the rows it defends against.

### 4.1 Version skew between tool and app — Flutter's recurring wound

- `flutter/devtools#8822`: DevTools is a service-worker-cached web app;
  switching Flutter channels served the *wrong DevTools version* to the
  app. Users had to manually purge service workers.
- `flutter/flutter#100247`: three different version strings for the
  same tool (CLI output, cache, live instance) diverged.
- `flutter/devtools#9728`: DevTools auto-updating its pinned Flutter
  version broke the pairing; the postmortem note is that integration
  tests would have caught it had they run the *combination*.

**Constraint D1:** the inspector ships *inside* the application
process, compiled from the same crate version as the framework.
No external server, no service worker, no cached web artifact.
Any out-of-process channel must carry a protocol version handshake and
refuse mismatches loudly (never silently degrade).

### 4.2 The tool's own dependencies becoming the problem

- `flutter/devtools#7477` (P1): a Perfetto upgrade caused a severe
  DevTools performance regression; the fix was pinning an older
  Perfetto with a local patch.

**Constraint D2:** DevTools UI is built *with Martensite on Martensite*
(dogfooding, no foreign UI stack, no embedded webview). The
observability hot path keeps the existing `< 0.1 ms/frame` overhead
gate; heavy trace viewing is a cold-path export format, not a bundled
viewer.

### 4.3 Runtime code-patching is fragile — keep the cdylib split

Dioxus `subsecond` (jump-table patching) is the most ambitious hot-
reload in the ecosystem and its issue tracker shows the cost:

- `#4632`: stack overflow crash hot-patching any app with ≥5 routes.
- `#5279`: Windows ASLR-reference failure — cryptic
  `InvalidModule("ASLR reference is less than the main module's
  address")`.
- `#5532`: wasm `apply_patch` race — `memory.grow` during patch awaits
  detaches the ArrayBuffer; patch data was written over live host
  memory.
- `#5540`: workspace regression — the patcher tried replaying crates it
  never captured rustc args for → `Missing rustc args for replay`.
- `#4768`: crash on server-code reload instead of falling back to a
  full rebuild; the CLI itself documents `--hot-patch` as "may lead to
  unexpected segfaults."

**Constraint D3:** Martensite's hot path stays **whole-cdylib swap**
(the `martensite-host` model). Patching strategies (subsecond-style
jump tables, incremental dylibs) are rejected as primary — see
ADR-0037. Any future fast path must degrade to a full reload on
*any* anomaly, never crash. Workspace membership must be resolved at
watch time, not patch time.

### 4.4 State preservation is an identity problem, not a runtime trick

- JetBrains `compose-hot-reload#461`: `remember` state is lost when a
  reload invalidates a *group* — inline functions merge state groups,
  so moving a declaration changes what survives. State identity keyed
  to source position is fragile.
- Blinc keys widget state by `InstanceKey` (`#[track_caller]` + call
  counter) rather than closure identity — `use_state` survives patches.
- Compose Hot Reload docs: global state (singletons, caches,
  ViewModels) cannot be auto-invalidated → stale data and
  `ClassCastException`; the framework's answer is explicit
  `AfterHotReload` reset hooks, not magic.
- Blinc/subsecond limitation: `const` data and `include_bytes!` live in
  rodata — patches never touch them (stale CSS-in-const bug).
- Slint live-preview: renaming a property/callback on the compiled
  boundary can *terminate the app*; conversely its syntax-error
  behavior (keep the old UI on screen until fixed) is the correct
  pattern.
- Dash: state restore must distinguish "reload of the same app" from
  "a different app that happens to reuse the port."

**Constraint D4:** the reload contract is documented *up front*:
arena `WidgetId` + `debug_name` path is the state-identity key (stable
across rebuilds by design); `on_hot_reload` reset hooks are the
documented escape for global state; `const`/rodata data does not
reload — runtime assets go through `martensite-assets` VFS, which
*watches and invalidates*; a failed reload keeps the previous dylib
mapped and the old UI live (Slint's correct behavior), printing a
compiler-error diagnostic, never a crash.

### 4.5 Scaffolding drifts — templates need CI, not just authorship

- cargo-generate docs (pitfalls): undefined placeholders silently emit
  empty strings; Liquid `{{ }}` collides with GitHub Actions `${{ }}`;
  templates rot because nobody rebuilds them.
- thoughtbot's cargo-generate lessons: the generator runs once —
  encode *structure*, not specifics that will drift; anything that
  changes should be applicable to existing projects via a tool
  (`cargo xtask`-style), not only baked into the template.
- staratlas `aeaa195` (real-world hardening): generated-project smoke
  checks in CI (fmt, clippy --all-features, build, test on the
  *generated* output), atomic scaffold via staging dir + rename,
  reject existing paths, validate crate-safe names, print actionable
  next steps.

**Constraint D5:** `cargo martensite new` renders from templates that
are *generated and tested in CI on every PR* (the generated project is
fmt/clippy/build/test-clean); scaffolding is atomic (staging dir +
rename, refuse existing non-empty dir); strict placeholder checking —
no silent empties; anything that must stay current (lint config,
AGENTS.md) is refreshable post-hoc via `cargo martensite init --agents`
on an existing project.

### 4.6 Documentation archaeology — the egui failure mode

- `egui#7147` (high-engagement issue): "archaeology instead of
  development" — sparse docs, outdated examples, no central pattern
  guide, paradigm mismatch unexplained. The failure is not coverage
  but *stale-ness and missing map*.

**Constraint D6:** every DX doc names its owner and its drift guard
(doctest, CI-checked snippet, or generated content). The widget
catalog example doubles as the executable API map. Onboarding docs
target *tasks* ("build a form", "virtualize a list") not API surfaces.

### 4.7 Inspector perf and scope discipline

- Chrome DevTools builds the full a11y tree *lazily on expansion* —
  eager construction was unusable on large pages.
- axe beats Lighthouse for devs precisely because it *highlights the
  failing node* and refuses to emit an aggregate score (the
  Lighthouse-100 gaming problem; the accessiBe legal precedent for
  score-driven claims).
- Accessibility Insights splits **FastPass** (2-min automated sweep)
  from **Assessment** (guided manual) — matching our
  `Confidence::Deterministic` vs `Heuristic` split.

**Constraint D7:** the inspector is lazy (expand-on-demand, subtree
fetch), hit-test-first (select mode drives tree reveal, not the
reverse), and the lint panel always links the *node* — never a score.

### 4.8 Live-editing needs a data channel, not a DSL

- Slint's data editor works because `.slint` is interpreted; Flutter's
  property editor writes back to source because the IDE owns the file.
  Makepad's Live system is a full DSL fork — the cost is a parallel
  language.
- Martensite has no DSL; properties are builder-call results compiled
  in. The equivalent surface is a **dev-mode mutation channel**: named
  tweakable parameters (`#[tweak]`/`debug_name`-addressed) adjustable
  at runtime, with optional source write-back via recorded spans —
  and it must be explicitly dev-only so it can never ship.

**Constraint D8:** live tweaks mutate the running arena/signals only;
source write-back is a separate, opt-in, rustc-span-anchored step;
the feature is compiled out without the `devtools` feature and hard-
disabled in release.

---

## 5. Consolidated design constraints

| ID | Constraint | Defends against |
| --- | --- | --- |
| D1 | In-app, version-locked inspector; handshake on any remote channel | Flutter version skew (#8822, #100247, #9728) |
| D2 | DevTools built on Martensite itself; <0.1 ms/frame hot path | Perfetto regression (#7477) |
| D3 | cdylib swap primary; graceful full-reload fallback; no patching | subsecond crashes (#4632, #5279, #5532, #5540, #4768) |
| D4 | Documented state-identity + reset-hook + rodata-limit contract | Compose state loss (#461), Slint termination |
| D5 | CI-tested scaffolding, atomic writes, strict placeholders | cargo-generate pitfalls, template drift |
| D6 | Task-oriented docs with drift guards; executable widget map | egui #7147 archaeology |
| D7 | Lazy tree, select-mode-first, node-level findings, no scores | Chrome eager-tree perf; axe/Lighthouse lesson |
| D8 | Dev-only mutation channel, compiled out in release | DSL fork cost; shipping-debug-features risk |

---

## 6. Initiative decomposition

The audit decomposes into eight workstreams, each specified in
`docs/dx/`:

| WS | Spec | What it delivers |
| --- | --- | --- |
| W1 | `docs/dx/INSPECTOR.md` | In-app widget inspector: select mode, lazy tree, layout panel, lint panel, a11y tree view |
| W2 | `docs/dx/CLI.md` | `cargo martensite` expansion: `new`, `init`, `lint`, `doctor`, `check`, `inspect` |
| W3 | `docs/dx/SCAFFOLDING.md` | Project templates + scaffolded `AGENTS.md`/`llms.txt` + `design-lint.toml` |
| W4 | `docs/dx/DEV_LINT.md` | Runtime lint bridge: dev channel, HUD panel, `cargo martensite lint` attach |
| W5 | `docs/dx/LIVE_TWEAKS.md` | Dev-mode property mutation channel + optional source write-back |
| W6 | `docs/dx/EVENT_DEBUGGING.md` | Event observability: hit-test/focus/dispatch tracing, HUD event log |
| W7 | `docs/dx/ERROR_SURFACE.md` | Dev-mode in-app diagnostics overlay (layout violations, paint errors, lint) |
| W8 | `docs/dx/ONBOARDING.md` | Widget catalog example, task cookbook, migration guides, docs drift guards |

Cross-cutting decisions are recorded in ADR-0036 (inspector hosting
model), ADR-0037 (hot-reload contract), ADR-0038 (dev-channel
transport). Milestone rollup: `docs/milestones/vNEXT-developer-
experience.md`.
