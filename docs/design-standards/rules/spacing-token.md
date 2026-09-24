# spacing-token

**Standard(s):** [consistency](../standards/consistency.md), [info-design](../standards/info-design.md)
**Default severity:** `info`
**Confidence:** heuristic — off-grid gaps are measured; the grid is convention (4pt), not law.

## What it measures

Sibling gaps (via the shared `sibling_gaps` helper) not
on `grid_pt` within `tolerance_px` flag — the spacing-axis complement
of `token-drift`.

## The evidence

- Design-token practice: quantized spacing keeps layout
  math composable; off-grid gaps are unmaintainable one-offs.

## Configuration

```toml
[rules.spacing-token]
severity = "info"
grid_pt = 4
tolerance_px = 0.75
```

## How to fix

- The autofix (safe) snaps gaps to the grid via
  `SnapGapsToGrid`.

## Legitimate exceptions

- Optical alignment nudges (icon optical correction) —
  allow the specific pair.
