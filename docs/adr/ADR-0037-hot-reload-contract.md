# [ADR-0037] Hot-Reload Contract — cdylib Swap Only, No Code Patching

* **Status:** Accepted
* **Date:** 2026-10-08
* **Deciders:** Martensite Architecture Working Group
* **Technical Domain:** `martensite-host`, `tools/cargo-martensite`,
  `martensite-devtools`
* **Amends:** ADR-0013 (Rust hot reloading) — narrows the mechanism
  and adds the state-preservation contract. Implements audit
  constraints **D3** and **D4**.

## Context and Problem Statement

ADR-0013 chose dynamic-library hot reloading. The ecosystem has since
produced a second mechanism — runtime code patching (Dioxus
`subsecond`'s jump-table indirection; Blinc's subsecond variant) —
which the DX research round evaluated as a candidate fast path.

The evidence against patching as a *primary* path is substantial:

- `dioxus#4632` — stack overflow patching any app with ≥5 routes.
- `dioxus#5279` — Windows ASLR-reference failure, cryptic module error.
- `dioxus#5532` — wasm `apply_patch` race: `memory.grow` during patch
  awaits detached the ArrayBuffer; patch data landed in live host
  memory.
- `dioxus#5540` — workspace regression replaying crates whose rustc
  args were never captured (`Missing rustc args for replay`).
- `dioxus#4768` — crash on reload instead of falling back to a full
  rebuild; the CLI documents `--hot-patch` as capable of "unexpected
  segfaults."

Meanwhile the state-preservation problem is mechanism-independent:
Compose Hot Reload loses `remember` state on group invalidation
(`#461`); global state can't be auto-invalidated and produces stale
data; `const`/rodata data is un-patchable; Slint's boundary rename can
terminate the app.

## Decision Drivers

* A failed reload must never crash or corrupt the running app —
  degrade to full rebuild, keep last-known-good live (D3).
* Reload correctness must be explainable: what state survives and why
  must be a *documented contract*, not emergent behavior (D4).
* Workspace-scale correctness: changes in any member crate resolve at
  watch time (the #5540 class is a watch-scope bug).
* The mechanism must not make claims the architecture can't keep —
  patching advertises ms-level reloads it can't reliably deliver.

## Considered Options

* **Option 1**: Jump-table/function patching (subsecond-style) as the
  primary loop.
* **Option 2**: cdylib swap only, no state contract — keep today's
  mechanism, leave survival undocumented.
* **Option 3**: **cdylib swap as the sole mechanism + an explicit
  state-preservation contract** — and reserve patching experiments
  behind a non-default, clearly-experimental flag *if* ever pursued.

## Decision Outcome

Chosen option: **Option 3**.

* **Mechanism:** whole-`cdylib` swap via `martensite-host` (libloading/
  `LoadLibrary`). The unit of reload is the crate boundary — coarse,
  ABI-checked, and materially simpler than intra-binary patching. On
  *any* anomaly — failed build, ABI/version mismatch, dlopen error,
  unwind across the boundary — the host keeps the previous library
  mapped and the previous UI live, prints the diagnostic, and offers
  full rebuild. Never crash to desktop (Slint's correct pattern; the
  inverse of its rename-terminates-app failure).
* **State-identity contract (D4).** State survives reload keyed by
  **`debug_name` path + `WidgetId`**, not by call-site position — the
  Compose inline-group lesson. Arena structure, `Signal` values, and
  scroll/focus position are carried across the swap by the host; the
  framework documents this identity model so developers can write
  reload-safe code deliberately (stable `debug_name`s, no positional
  assumptions).
* **Global-state escape hatch.** An `on_hot_reload` reset hook
  (documented, opt-in) for caches/singletons/service handles — the
  Compose `AfterHotReloadEffect` pattern — because global state can
  never be auto-invalidated correctly.
* **Rodata limitation is documented.** `const` data and
  `include_bytes!` are baked into the old image; the reload contract
  states plainly that runtime-mutable assets belong in
  `martensite-assets` VFS (which watches and invalidates on change),
  not in `const`s (the Blinc stale-CSS lesson).
* **Workspace watch-scope.** `cargo martensite dev` resolves the
  watched crate graph from `cargo metadata` at startup; a change in an
  unbuilt member triggers a rebuild prompt, never a partial/stale
  patch (the #5540 class cannot occur — there's no replay of
  un-captured crates).

### Positive Consequences

* The fragile class of patching bugs (stack overflow, ASLR, memory
  races, workspace replay) is declined wholesale, not mitigated —
  we never ship the mechanism that produces them.
* State survival becomes *predictable and teachable* — a documented
  identity model rather than a hope. This is a material DX win the
  patching ecosystem still doesn't fully offer.
* ~350 ms cdylib turnaround is honest and reliable — slower than
  subsecond's advertised best case, faster than its average case
  counting crashes.
* `debug_name` paths gain a second paying customer (reload identity),
  strengthening the convention the design-lint already relies on.

### Negative Consequences

* Reload is coarser than function-level patching: a one-line change
  rebuilds and swaps the whole guest crate (~350 ms target, not ~50
  ms). Accepted — correctness and crash-freedom outrank marginal
  latency in the dev loop, and the gap narrows as the guest crate
  stays lean.
* Reload-unsafe code is possible (positional state assumptions,
  globals). Mitigated by the documented contract and the
  `on_hot_reload` hook — an explicit API, not magic.
* The cdylib boundary imposes its own constraints (no cross-boundary
  `&T` borrows into the guest, ABI versioning) — already absorbed by
  ADR-0013's design.

## Links

* `docs/research/DEVELOPER_EXPERIENCE_AUDIT.md` §4.3, §4.4
* `docs/dx/LIVE_TWEAKS.md` (tweak re-assertion by name across reload)
* ADR-0013 (original hot-reloading decision)
