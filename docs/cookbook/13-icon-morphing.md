# Cookbook 13 — Morphing Stroke Icons (`MorphIcon`)

State changes that swap one icon for another — play → pause, hamburger →
close, speaker → muted — feel abrupt when the glyph hard-cuts.
`MorphIcon` (ADR-0041, a port of the morphicons core) turns the swap into
a shape transformation: both icons are resampled to point sets, a
Procrustes correspondence is solved between them, and rotation + scale
ride a `martensite-motion` spring rather than dissolving point-to-point.

This recipe shows how to drive `MorphIcon` from application state.

---

## 1. Goal

1. Build a `MorphIcon` from SVG `d` path data — fallibly, never panicking.
2. Animate between states with `morph_to` and a `SpringConfig` preset.
3. Scrub deterministically in tests and scrub-UI via `seek`.
4. Keep the accessibility tree clean (`label` vs `decorative`).
5. Honor the reduced-motion push and the stroke-only contract.

---

## 2. Complete Runnable Pattern

```rust
use martensite::widgets::morph_icon::{demo, MorphIcon};
use martensite_motion::SpringConfig;

// Rest state: hamburger. `icon(d)` is `Result`-returning — malformed,
// oversized, or non-finite path data yields `MorphError`, never a panic.
let mut icon = MorphIcon::icon(demo::MENU)
    .expect("demo constants are well-formed")
    .label("Menu")          // a11y name — the *meaning*, not the shape
    .size(24.0)             // logical points; default 24
    .stroke_width(2.0);     // the lucide/feather 2px-at-24px convention

// State transition → morph. Mid-flight calls re-enter cleanly: the
// current interpolated shape becomes the new origin and the spring
// keeps its velocity.
icon.morph_to(demo::CLOSE, SpringConfig::SNAPPY)
    .expect("demo constants are well-formed");
assert!(icon.is_animating());
```

`MorphIcon` is a leaf widget — `event` ignores input and `tick` advances
the spring. Embed it as an internal child (the `child`/`child_bounds`
protocol) inside the control that owns the state; `Volume` does exactly
this for its speaker glyph.

---

## 3. `SpringConfig` Presets

| Preset | Damping (ζ) | Feel | Use for |
|---|---|---|---|
| `SpringConfig::CRITICAL` | ≈ 1.0 | Smooth, no overshoot | Status indicators, dense toolbars |
| `SpringConfig::SNAPPY` | ≈ 0.72 | Quick, small overshoot | Button/toggle state swaps (`Volume` uses this) |
| `SpringConfig::GENTLE` | ≈ 0.77 | Slow, soft | Large hero icons |
| `SpringConfig::BOUNCY` | ≈ 0.35 | Pronounced oscillation | Playful confirmations only |

Instant jumps skip the spring entirely: `icon.set_icon(d)` lands on the
rest shape with no flight.

---

## 4. `seek` — Deterministic Frames

`icon.seek(t)` freezes the active plan at progress `t` — a parked spring,
not a simulation — so tests and scrub UIs get identical frames every run.
`t` outside `0.0..=1.0` extrapolates, matching spring-overshoot math.

```rust
icon.morph_to(demo::PAUSE, SpringConfig::SNAPPY).unwrap();
icon.seek(0.5);                    // paint now emits the exact midpoint
assert!(icon.is_animating());      // frozen, still "in flight"
assert_eq!(icon.progress(), 0.5);
```

---

## 5. Accessibility & Reduced Motion

- `.label("Muted")` sets the announced name for the **target** state —
  mid-flight shapes are never announced, and `tick` reports
  paint-only dirtiness so no `TreeUpdate` storms the AT at frame rate.
- `.decorative(true)` emits `Role::Image` + `hidden` — use it when the
  icon sits inside a control that already owns the name (a labelled
  `Button`, the `Volume` row).
- The arena pushes `set_reduced_motion` automatically (it forwards to
  internal children by default). While the flag is set, `morph_to`
  behaves as `set_icon` — an instant snap, no flight.

---

## 6. Contract & Pitfalls

- **Stroke-only.** Icons paint as stroked polylines on the 24px grid;
  a *filled* `d` paints the fill's outline — almost never intended.
- **Fallible input.** `d` strings over 16 KB, > 24 subpaths, or > 512
  segments return `MorphError::TooLarge`; non-finite geometry returns
  `Degenerate`. Supplying icons is the app's job — Martensite does not
  vendor an icon set.
- **`demo` constants** (`martensite::widgets::morph_icon::demo`) provide
  canonical pairs for tests and demos: `MENU`/`CLOSE`, `PLAY`/`PAUSE`,
  `VOLUME_ON`/`VOLUME_OFF`, `EYE_OPEN`/`EYE_CLOSED`, `CHECK`, `PLUS`/
  `MINUS`, `CHEVRON_RIGHT`/`CHEVRON_DOWN`, `LOCK`/`LOCK_OPEN`.

---

## Next Steps

- [Cookbook 03 — Custom Painting & Silhouettes](03-custom-painting.md)
- [Cookbook 10 — Headless Component Testing](10-headless-testing.md)
- [ADR-0041 — Native Icon Morphing](../adr/ADR-0041-native-icon-morphing.md)
