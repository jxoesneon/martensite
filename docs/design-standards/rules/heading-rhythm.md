# heading-rhythm

**Standard(s):** [info-design](../standards/info-design.md), [consistency](../standards/consistency.md)
**Default severity:** `info`
**Confidence:** heuristic — font-size vs depth monotonicity.

## What it measures

Heading sizes should shrink with nesting depth;
inversions flag.

## The evidence

- Typographic hierarchy — a subsection heading bigger
  than its parent's breaks the map.

## Configuration

```toml
[rules.heading-rhythm]
severity = "info"
tolerance_pt = 0.5   # size ties within this count as same-level
```

## How to fix

- Re-level the type scale.

## Legitimate exceptions

- Hero numbers inside small containers (KPI tiles) —
  allow.
