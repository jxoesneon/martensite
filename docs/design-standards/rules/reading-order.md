# reading-order

**Standard(s):** [wcag](../standards/wcag.md), [info-design](../standards/info-design.md)
**Default severity:** `info`
**Confidence:** heuristic — row-major visual order is an approximation; counting inversions past a threshold keeps it robust.

## What it measures

Each parent's children are sorted into visual order
(row-band y0, then x0) and inversions against document (paint) order
are counted. `min_inversions`+ = finding.

## The evidence

- **WCAG 2.2 SC 1.3.2 Meaningful Sequence** — assistive
  tech reads paint order; sighted users read position. Divergence =
  two different interfaces.

## Configuration

```toml
[rules.reading-order]
severity = "info"
min_inversions = 2
row_band_frac = 0.25   # y0 tolerance as fraction of row height
```

## How to fix

- Reorder the children in the widget tree so paint order
  matches screen position.

## Legitimate exceptions

- Z-ordered overlays and intentionally-reversed strips
  (RTL-adjacent layouts) — suppress.
