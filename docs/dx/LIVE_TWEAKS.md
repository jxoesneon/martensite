# Spec: Live Property Tweaks (W5)

**Constraints:** D4 (state identity contract), D8 (dev-only, compiled
out in release).
**Crates:** `martensite-devtools` (`tweak` module), `martensite-macros`
(`#[tweak]` attribute), inspector Properties panel (W1).

## Goal

The builder-API equivalent of Slint's data editor and Flutter's
property editor: nudge a padding, color, or font size *on the running
app* without a rebuild — and optionally write the result back to
source.

Since Martensite has no DSL to reinterpret, tweaks address the live
arena and signal graph directly.

## Two mechanisms, one surface

### A. Signal tweaks (the easy, honest 80%)

Most visually interesting values already live in `Signal<T>`s —
theme tokens, spacing scales, KPI values. Signals are mutable at
runtime *by design*; the reactive graph propagates the change
correctly.

```rust
// Registration is explicit and named — this is the developer's
// contract that the value is safe to mutate live.
#[tweak("theme/gap-scale")]
let gap = Signal::new(4.0f32);
```

The `#[tweak]` attribute (or `signal.tweak("name")` method form —
decide at implementation; method form has zero macro cost) registers
the signal in a dev-only `TweakRegistry { name → SignalId, type tag,
current value }`. The inspector Properties panel lists registered
tweaks for the selected widget's subscriptions with type-appropriate
editors (slider for f32 range, color well for `[u8;4]`/Oklab, text for
String).

### B. Style/geometry tweaks (the honest 20%)

Builder-baked values (a literal `.padding(12.0)`) aren't reachable at
runtime — they're compiled in. For these:

- **Dev-mode:** the Properties panel edits the *live resolved value*
  where the widget exposes a setter (`container.set_padding`),
  clearly labeled `transient — does not survive reload`.
- **Write-back (opt-in, separate step):** with
  `devtools-source-spans`, builder calls record their callsite span
  (`#[track_caller]` → `file:line:col`). A tweak can then emit a
  *source patch* — `src/ui.rs:142: .padding(12.0) → .padding(16.0)` —
  applied via the CLI (`cargo martensite tweak --apply`) or printed
  for the user. Applying it rebuilds through the normal hot-reload
  loop. This matches Flutter's property-editor write-back without
  needing a DSL.

## The honesty contract (D4, D8)

- **Explicitly transient.** Every tweaked value displays a `~` badge;
  a "tweaks applied" list shows what differs from compiled state, and
  `Reset all` restores it. Following the Compose/Dash lesson, the UI
  must always answer "what state am I actually looking at?"
- **Survival across hot reload:** signal tweaks re-apply after a
  cdylib swap *by name* (the `TweakRegistry` is rebuilt by the new
  dylib, and the host re-asserts values onto matching names —
  documented as name-identity, same class as D4's `debug_name` path
  identity). A tweak whose name vanished in the rebuild is reported
  dropped, not silently lost.
- **Compiled out.** `TweakRegistry`, `#[tweak]`, and the write-back
  spans exist only under `devtools` / `devtools-source-spans`.
  Release builds: the attribute is inert, the registry is absent, and
  there is no runtime mutation surface. Verified by a release-mode
  `cargo expand`-level test asserting no `tweak` symbols.

## Acceptance gates

1. Tweak a registered `gap-scale` signal in the dashboard → layout
   visibly updates next frame; `~` badge and tweak list reflect it.
2. Hot reload with a tweak active → value re-asserts by name;
   removing the signal's name in code → tweak reported dropped in the
   inspector, no panic.
3. `devtools-source-spans` write-back emits a correct `file:line`
   patch for a literal `.padding(N)`; applying it + reload shows the
   new value with no transient badge.
4. Release build: no `TweakRegistry`, no span tables — `nm`/feature
   gate test proves absence.
5. `Reset all` returns every tweaked value to compiled defaults and
   clears the transient markers.
