# sibling-variance

**Standard(s):** [consistency](../standards/consistency.md), [info-design](../standards/info-design.md)
**Default severity:** `info`
**Confidence:** heuristic — same-name siblings compared on geometry ratio.

## What it measures

Same-name children grouped; min/max width and height
ratios over `max_ratio` flag.

## The evidence

- Regularity (Miniukovich): equal-role elements set an
  equivalence contract — 2× size difference reads as a bug.

## Configuration

```toml
[rules.sibling-variance]
severity = "info"
max_ratio = 1.5
```

## How to fix

- Equalize the geometry or rename the outlier so the contract
  isn't implied.

## Legitimate exceptions

- Feature cards among standard cards — rename or allow.
