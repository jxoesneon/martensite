# color-budget

**Standard(s):** [isa-101](../standards/isa-101.md), [consistency](../standards/consistency.md)
**Default severity:** `warn`
**Confidence:** heuristic — hue counting is exact, but "how many
saturated colors is too many" is a discipline judgment. The finding
asks whether the color spend is deliberate, not whether it exists.

## What it measures

Counts **distinct saturated hues** painted across a surface's subtree
in normal state. Hue is bucketed (30° steps); only opaque, saturated,
mid-lightness colors count — grays, near-blacks, near-whites, and
translucent fills are not "attention colors" and are excluded.

More than `max` distinct saturated hues means the surface is spending
its alarm channel on decoration.

## The evidence

- **ISA-101 / ASM Consortium** ([isa-101](../standards/isa-101.md)):
  the High-Performance HMI doctrine. Saturated color is *reserved* for
  abnormal states — it is the alarm channel. A normal-state surface
  where five saturated hues compete has spent its attention budget:
  when something actually goes wrong, nothing stands out. ASM research
  showed operators on quiet gray displays detected abnormal situations
  faster than operators on colorful ones.
- **Consistency** ([consistency](../standards/consistency.md)): a small
  deliberate hue set is a design system; a dozen is drift.
- **Endsley situation awareness**: saturated color drives SA level 1
  (perception) — spend it and the perceptual orienting response is
  exhausted on decoration.

**Default `max = 3`** — a small accent palette plus one alarm channel
is the defensible normal-state budget. `min_saturation` tunes what
counts as "saturated."

## Configuration

```toml
[rules.color-budget]
severity = "warn"
max = 3               # max distinct saturated-hue buckets per surface
min_saturation = 0.4  # colors below this saturation don't count
min_surface_pt = 20000
```

## How to fix

- **Mute normal-state surfaces.** Follow the ISA-101 palette model:
  muted gray-scale + one or two accent hues for normal operation;
  saturated color only for abnormal states.
- **Check [token-drift](token-drift.md).** Saturated hues that aren't
  in the declared palette are often hard-coded one-offs, not palette
  members — the fix may be removing drift rather than redesigning.
- **Ask what the color is *saying*.** If a saturated fill encodes
  state, it should also carry a redundant cue (see
  [color-only-info](color-only-info.md)) — and it should be in the
  alarm budget deliberately.

## When it's OK to allow

- **Media and visualization surfaces** — a video wall, photo viewer,
  or color-coded chart where saturated color *is* the content.
- **Brand/marketing surfaces** (splash screens, onboarding).

```toml
[[allow]]
path = "App/MediaZone/**"
rules = ["color-budget", "standard:isa-101"]
```
