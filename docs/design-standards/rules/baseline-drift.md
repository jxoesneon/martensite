# baseline-drift

**Standard(s):** [consistency](../standards/consistency.md)
**Default severity:** `info`
**Confidence:** heuristic — sibling baselines compared within a row.

## What it measures

Sibling text runs' baselines (y origin) in a shared row
band; drift past tolerance flags.

## The evidence

- Baseline alignment is what makes a label+value row
  read as one unit.

## Configuration

```toml
[rules.baseline-drift]
severity = "info"
tolerance_px = 1.0   # baseline offset tolerance
```

## How to fix

- Align the runs or let the layout baseline-align them.

## Legitimate exceptions

- Intentional mixed-size callouts — allow.
