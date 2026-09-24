# Spec: Event & Dispatch Observability (W6)

**Constraints:** D2 (<0.1 ms/frame hot path; ring buffers, no
allocation).
**Crates:** `martensite-window` (dispatch instrumentation),
`martensite-devtools` (event log + HUD/inspector panels),
`martensite-focus`, `martensite-dnd`.

## Goal

Answer the questions developers actually ask, with data instead of
guesswork:

- *Why didn't my click land?* — hit-test path + rejection reason.
- *Why didn't my key reach the widget?* — focus chain + trap capture.
- *Who ate my scroll?* — scroll-routing chain + capture.
- *What fired this frame?* — event ledger per frame.

Today the entire observability surface is two `tracing::debug!` calls
in `quiescent.rs`. This spec instruments the *production* dispatch
paths — the same ones the inspector's select mode relies on (D7: one
truth, no parallel implementation).

## The event ledger

```rust
/// One entry in the per-frame event ledger. Fixed-size, Copy, no
/// allocation — the ledger is a preallocated ring buffer.
pub struct EventRecord {
    pub seq: u64,
    pub frame: u64,
    pub kind: EventKind,            // Pointer, Key, Scroll, Ime, Focus, Dnd
    pub position: Option<Point>,
    pub hit_path: Vec<WidgetId>,    // bounded inline capacity (e.g. 16)
    pub hit_rejection: Option<HitRejection>,
    pub disposition: Disposition,   // Handled(id) | Ignored | BubbledTo(id) | Captured(id)
    pub focus_from: Option<WidgetId>,
    pub focus_to: Option<WidgetId>,
}
```

`HitRejection` names the cause: `OutsideBounds`, `OccludedBy(id)`,
`HitTestDisabled`, `UnderModal`, `CapturedByOther(id)`.

## Instrumentation points

All behind `cfg!(feature = "devtools")` with a runtime toggle; zero
cost when off — the flag check is a single branch per dispatch, and
the ledger is preallocated:

1. **`EventRouter::dispatch_pointer_event`** — record hit-test result
   path + disposition + pointer-capture short-circuits.
2. **`dispatch_keyboard_event`** — record focused target, the route
   taken (focused widget vs modal trap vs app-level), disposition.
3. **`dispatch_scroll_event`** — record scroll-region resolution and
   which ancestor captured.
4. **Focus transitions** — record old→new + cause (click, Tab,
   programmatic, trap release).
5. **Hit-test rejections** — the top-level `hit_test` returning `None`
   still records *why* (the rejecting boundary).
6. **Drag & drop** — drag start target, drop resolution, MIME
   negotiation outcome.

## Surfaces

- **`MARTENSITE_DEBUG_EVENTS=1`** — env-gated stderr stream, one line
  per event: `ptr@ 412,301 → hit[App/ZonePanel/Grid/Cell(r42,c7)]
  handled`. Format designed for grep and for agent consumption.
- **Inspector Events panel (W1 §6)** — the ring buffer rendered:
  filterable by widget/kind; selecting a record highlights the hit
  path in the app view.
- **Inspector select-mode "why"** — a click that hit nothing shows the
  rejection reason in the overlay instead of silence.

## Volume & cost discipline (D2)

- Ring buffer: 1024 entries × ~200 B ≈ 200 KB fixed — no growth, no
  alloc in dispatch.
- `Vec<WidgetId>` hit path: `SmallVec`-style inline capacity 16, spill
  marks truncated flag.
- When the ledger is disabled: one branch + one timestamp skipped per
  dispatch — measured inside the <0.1 ms/frame gate.
- `tracing` spans *also* emitted at `trace` level for integration with
  the existing tracy pipeline — the ledger is the structured view, not
  a replacement for spans.

## Acceptance gates

1. Dashboard: click a disabled control → ledger shows `hit_path` +
   `HitTestDisabled`/`OccludedBy`; stderr stream prints the same
   single line.
2. Tab through a modal → focus records show `focus_from/to` +
   `CapturedByOther` for keys that didn't reach background widgets.
3. Nested scroll: wheel over inner region → record shows inner
   capture; over outer → outer capture. No ambiguity.
4. Ledger off: dispatch microbenchmark delta < measurement noise
   (assert via criterion gate or counter test).
5. 10k synthetic events: ledger keeps last 1024, zero allocations
   (alloc-counter test).
