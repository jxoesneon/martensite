# redundant-border

**Standard(s):** [info-design](../standards/info-design.md)
**Default severity:** `info`
**Confidence:** heuristic — nested same-color borders are detected; whether they're redundant vs structural is judgment.

## What it measures

Nested fills forming border rings where whitespace
already groups — double encoding.

## The evidence

- Tufte: every non-data pixel competes with data —
  borders around already-grouped regions are pure chrome.

## Configuration

```toml
[rules.redundant-border]
severity = "info"
divider_max_px = 3   # stroke thickness that reads as a border/divider
min_gap_pt = 8       # whitespace that already groups (border redundant)
min_span_frac = 0.4  # divider must span this share of the surface
```

## How to fix

- Delete the inner border; let whitespace do the grouping.

## Legitimate exceptions

- Focus/safety boundaries that must stay visible —
  allow.
