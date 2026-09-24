# overflow-clip

**Standard(s):** [wcag](../standards/wcag.md), [consistency](../standards/consistency.md)
**Default severity:** `warn`
**Confidence:** deterministic — fill geometry vs scope bounds is measured, not estimated.

## What it measures

Every `FillStat` is checked against its owning node's
bounds. Overruns past `tolerance_px` on any edge flag the node.

## The evidence

- Layout-contract overflow relies on an ancestor clip to
  stay invisible — it's the paint-audit defect class, expressed as a
  design rule so the cause is caught, not just the symptom.

## Configuration

```toml
[rules.overflow-clip]
severity = "warn"
tolerance_px = 2
```

## How to fix

- Fix the geometry — shrink the fill to its bounds or grow the
  container. Overflow that needs clipping to be invisible is a bug
  waiting for a resize.

## Legitimate exceptions

- Intentional bleed (shadows, glows, focus rings drawn
  outside the control) — suppress.
