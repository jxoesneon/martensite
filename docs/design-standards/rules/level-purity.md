# level-purity

**Standard(s):** [isa-101](../standards/isa-101.md)
**Default severity:** `warn`
**Confidence:** heuristic — requires `@level:1` markers; what counts as 'detail control' is the interactive-leaf count.

## What it measures

Nodes marked `@level:1` are checked for interactive
leaves. More than `max_interactive` = finding. Unmarked scenes never
fire.

## The evidence

- **ANSI/ISA-101.01 display hierarchy**: L1 = situation
  awareness (KPIs, trends, alarm access) — explicitly *no* equipment
  control. Detail control at L1 recreates the dense-console problem
  the standard exists to end.

## Configuration

```toml
[rules.level-purity]
severity = "warn"
max_interactive = 0
```

## How to fix

- Move controls to L2/L3 displays; keep L1 to KPIs + trends +
  alarm access.

## Legitimate exceptions

- A 'acknowledge-all' or navigation control on the overview
  is the standard's own exception — allow it by path.
