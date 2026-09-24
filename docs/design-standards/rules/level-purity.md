# level-purity

**Standard(s):** [isa-101](../standards/isa-101.md), [nureg-0700](../standards/nureg-0700.md)
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
- **NUREG-0700**: density should be minimized on displays carrying
  critical information — an L1 overview polluted with control affordances
  is exactly the "critical display carrying non-essential load" pattern
  NUREG's density clause targets. The numeric packing caps live on
  [packing-density](packing-density.md); this rule owns the purity check.

**Standard-selection note:** because the rule cites both standards, a
`disabled_standards = ["isa-101"]` config keeps it alive under
`nureg-0700`. In practice it can only fire on `@level:1` markers — an
ISA-101 vocabulary — so for a `nureg-0700`-only project it is inert
rather than harmful. To fully disable it, silence the rule id.

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
