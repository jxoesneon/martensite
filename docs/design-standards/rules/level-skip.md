# level-skip

**Standard(s):** [isa-101](../standards/isa-101.md)
**Default severity:** `warn`
**Confidence:** deterministic given markers — an `@level:N` under `@level:N-2` or lower is a measured hierarchy jump.

## What it measures

Each node with `display_level` finds its nearest
`display_level` ancestor; a jump >1 level flags.

## The evidence

- **ISA-101.01**: each level provides the context the
  next assumes — reaching L3 detail without the L2 unit picture
  strands the operator.

## Configuration

```toml
[rules.level-skip]
severity = "warn"
# no thresholds — a >1-level jump in @level lineage flags
```

## How to fix

- Insert the intermediate level, or mark the intermediate
  container with `@level:N` so the chain reads correctly.

## Legitimate exceptions

- Diagnostics screens legitimately reachable from overview
  (L4 under L1 is the standard's own pattern) — allow.
