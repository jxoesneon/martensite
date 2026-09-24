# scroll-competition

**Standard(s):** [info-design](../standards/info-design.md)
**Default severity:** `warn`
**Confidence:** heuristic — multiple scroll regions per surface; Few calls this the dashboard anti-pattern.

## What it measures

Surfaces containing more than `max_regions` scroll
regions flag.

## The evidence

- Few: nested scrolling panes fracture attention — one
  page, one scroll.

## Configuration

```toml
[rules.scroll-competition]
severity = "warn"
# thresholds: >1 scroll-named region per surface flags (fixed)
```

## How to fix

- One scroll container per surface; let sections size to
  content or paginate.

## Legitimate exceptions

- Chat/log panes legitimately scroll independently —
  allow.
