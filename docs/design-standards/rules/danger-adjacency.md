# danger-adjacency

**Standard(s):** [info-design](../standards/info-design.md)
**Default severity:** `warn`
**Confidence:** heuristic — destructive styling is name/marker-detected (@destructive or danger|delete|destruct names).

## What it measures

Destructive-styled controls adjacent to safe controls
flag — one slip from a destructive act.

## The evidence

- Error-cost principle (Norman): the cost of a slip
  scales with consequence; separate the red button from the gray
  ones.

## Configuration

```toml
[rules.danger-adjacency]
severity = "warn"
adjacent_pt = 4   # gap under this counts as adjacent (scale-aware)
```

## How to fix

- Isolate destructive actions (menu, footer, confirmation
  step) from routine controls.

## Legitimate exceptions

- Toolbars with grouped undo/delete under a consistent
  pattern — allow.
