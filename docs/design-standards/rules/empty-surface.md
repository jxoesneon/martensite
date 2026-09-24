# empty-surface

**Standard(s):** [info-design](../standards/info-design.md)
**Default severity:** `info`
**Confidence:** heuristic — inverse density; <5% painted on a large zone suggests dead allocation.

## What it measures

Surfaces whose painted area is under the floor share of
their bounds.

## The evidence

- Inverse of `density` — a zone that paints almost
  nothing is either placeholder or wasted allocation.

## Configuration

```toml
[rules.empty-surface]
severity = "info"
min_coverage = 0.03   # painted-area share below this = dead allocation
```

## How to fix

- Fill it, shrink it, or fold it into a neighbor.

## Legitimate exceptions

- Deliberate whitespace regions — allow.
