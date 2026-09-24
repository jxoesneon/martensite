# balance

**Standard(s):** [info-design](../standards/info-design.md), [perception](../standards/perception.md)
**Default severity:** `info`
**Confidence:** heuristic — visual-weight centroid is approximated by painted area.

## What it measures

Each surface's painted mass centroid is compared to its
geometric center. Off-center past the threshold fraction = finding.

## The evidence

- **Miniukovich & De Angeli (CHI 2015)**: balance is one
  of the validated aesthetic metrics — lopsided surfaces read as
  unfinished even when every element is fine.

## Configuration

```toml
[rules.balance]
severity = "info"
max_offset_frac = 0.25   # centroid offset as fraction of surface extent
```

## How to fix

- Rebalance by resizing or moving heavy elements, or add
  counterweight whitespace.

## Legitimate exceptions

- Intentional asymmetric layouts (master-detail) — the
  heuristic sees imbalance; you see a sidebar. Allow.
