# kpi-context

**Standard(s):** [isa-101](../standards/isa-101.md), [info-design](../standards/info-design.md)
**Default severity:** `warn`
**Confidence:** heuristic — 'context' is name-matched (trend|spark|target|compare|chart); a project naming its trend widgets differently sees false positives until it aligns or suppresses.

## What it measures

Each `@kpi` node is checked for a context-bearing
sibling or descendant (name match). Bare value = finding.

## The evidence

- **Few, *Information Dashboard Design***: a number
  without context is data, not information — operators need to know
  if 72.3 is *good*.

## Configuration

```toml
[rules.kpi-context]
severity = "warn"
# context is name-matched: trend|spark|target|compare|chart|vs
```

## How to fix

- Pair every KPI with its trend sparkline, target line, or
  period comparison.

## Legitimate exceptions

- Setpoint displays where the value IS the context —
  allow per-tile.
