# grid-drift

**Standard(s):** [info-design](../standards/info-design.md), [consistency](../standards/consistency.md)
**Default severity:** `info`
**Confidence:** heuristic — column edges across sibling rows are compared.

## What it measures

Sibling rows' column x-edges are clustered; edges
drifting off the shared column lines flag.

## The evidence

- Grid discipline — the eye reads columns even when the
  layout doesn't declare them; drift reads as misalignment.

## Configuration

```toml
[rules.grid-drift]
severity = "info"
tolerance_px = 2   # edge-cluster snap tolerance
```

## How to fix

- Align the columns or use the layout engine's grid.

## Legitimate exceptions

- Masonry/irregular layouts — allow.
