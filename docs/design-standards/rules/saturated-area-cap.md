# saturated-area-cap

**Standard(s):** [isa-101](../standards/isa-101.md)
**Default severity:** `warn`
**Confidence:** heuristic — 15% is a tuning point, not a standard number; the standard's principle is 'gray is the canvas'.

## What it measures

Per node, the union of saturated fills (HSV sat ≥
`min_sat`, opaque) clipped to the node's bounds is measured as a
share of node area. Over `max_pct` = finding.

## The evidence

- **ISA-101/ASM**: saturation is reserved so abnormal
  color *pops* — the budget is about coverage (what the retina sees),
  not hue count (`color-budget` measures that).

## Configuration

```toml
[rules.saturated-area-cap]
severity = "warn"
max_pct = 15
min_sat = 0.5
min_surface_pt = 20000   # area floor — small widgets are attention paint, not canvas
```

## How to fix

- Mute non-state fills toward gray; reserve saturation for
  abnormal state.

## Legitimate exceptions

- Photo/video surfaces and map tiles are saturated by
  nature — allow the subtree.
