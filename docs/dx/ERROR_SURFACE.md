# Spec: Dev-Mode Error Surface (W7)

**Constraints:** D2 (cold-path only — errors are rare), D7 (node-level,
link to docs).
**Crates:** `martensite-devtools` (overlay), `martensite-layout`
(constraint violations), `martensite-render` (paint errors),
`martensite-design-lint` (findings).

## Goal

When something is wrong, the running app *shows it* — Flutter's
overflow yellow-tape and error-widget pattern — instead of silently
producing a subtly-wrong frame or a bare panic message.

## Three severity tiers, three surfaces

### Tier 1 — Inline annotation (per-widget, ambient)

Dev-mode paint decorations drawn *into the frame* over offending
widgets, always-on when `devtools` + `error_surface` enabled:

- **Layout overflow:** the standard diagnostic — diagonal hatch
  overlay on the overflowing edge + the overflow amount in px,
  matching Flutter's tape convention so the meaning is
  cross-ecosystem legible.
- **Clipped text:** underline marker where `text-truncation` would
  fire — visible without opening the inspector.
- **Lint errors** (Error severity only): a corner tick; hover/select
  in the inspector shows the finding. Warn/Info stay in the panel —
  ambient display is reserved for actionable errors so it doesn't
  become wallpaper (the red-screen noise lesson).

### Tier 2 — Diagnostics overlay (per-frame, on demand)

A HUD section + inspector tab listing the current frame's active
diagnostics: layout violations, paint errors, dropped frames over
budget, lint Warn+. Each entry: node path, one-line cause, doc link.
Selectable → reveals the node (same reveal path as the tree panel).

### Tier 3 — Structured panic (app-fatal)

Panic handler for dev mode (opt-in via
`window.enable_devtools` or `MARTENSITE_DEV_PANIC=overlay`):

- Catches the panic, renders an *in-app* error card instead of an
  immediate abort: panic message, the widget tree position being
  processed when it fired (if the panic happened during layout/paint
  dispatch — the arena knows the in-flight node), and a `debug_name`
  path.
- The card offers `Copy report` (message + in-flight path + last N
  event-ledger records from W6 — the crash bundle) and `Continue`
  where the in-flight node can be pruned from the frame safely
  (paint-phase panics only; layout-phase panics offer only restart —
  stated honestly).
- Never swallowed: `RUST_BACKTRACE` honored; the card is a *surface*
  for the panic, not a suppression. Release builds keep default abort
  semantics — the handler is dev-only (D8).

## What feeds it

- `martensite-layout`: constraint resolution already detects
  violations — surface them as structured `LayoutDiagnostic`s (node,
  offered, resolved, overflow amount) instead of only debug-asserts.
- `martensite-render`: `PaintError`s and recoverable backend failures
  carry the paint-list scope path where they occurred (the same
  provenance design-lint already consumes).
- `martensite-design-lint`: the W4 bridge's Error-severity findings.
- Event ledger (W6): the crash bundle's context.

## Anti-noise rule

Flutter's error widget works because it fires on *exceptional* states.
Ours must not become ambient decoration on a healthy app:

- Severity floor: inline (Tier 1) shows Error-class only.
- Dedup: same diagnostic on same node across frames counts once, with
  a frame-age badge (`for 240f`) instead of re-announcing.
- Cap: at >N simultaneous diagnostics the overlay collapses to
  `N diagnostics — open inspector` rather than carpeting the UI.

## Acceptance gates

1. Force a layout overflow in the dashboard → hatch + amount renders
   inline; the overlay lists it once with the node path.
2. A widget-paint panic in dev mode → error card shows the
   `debug_name` path + crash bundle offers the last events; release
   build → default panic behavior unchanged.
3. A frame with zero diagnostics → zero overlay cost and zero drawn
   decorations.
4. 500 simultaneous forced violations → collapsed summary, no
   per-item paint.
5. Every diagnostic entry carries a linkable node path resolvable by
   the inspector's reveal (round-trip test).
