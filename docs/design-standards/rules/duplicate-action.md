# duplicate-action

**Standard(s):** [consistency](../standards/consistency.md), [info-design](../standards/info-design.md)
**Default severity:** `warn`
**Confidence:** heuristic — identical labels on distinct controls; legitimate in master-detail but ambiguous in one surface.

## What it measures

Same normalized label on multiple interactive leaves
under one surface.

## The evidence

- Ambiguous labeling — identical action names force
  disambiguation-by-position every time.

## Configuration

```toml
[rules.duplicate-action]
severity = "warn"
# no thresholds — identical normalized labels on distinct controls flag
```

## How to fix

- Rename for context ('Save draft' vs 'Save & close').

## Legitimate exceptions

- Repeated row-level actions (per-row 'Edit') are the
  legitimate case — allow the list container.
