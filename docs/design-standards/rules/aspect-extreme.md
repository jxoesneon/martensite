# aspect-extreme

**Standard(s):** [info-design](../standards/info-design.md)
**Default severity:** `info`
**Confidence:** heuristic — extreme aspect ratios are measured; whether a ribbon is unusable depends on content.

## What it measures

Surface aspect ratios beyond `max_ratio` flag as
unusable ribbons.

## The evidence

- A 40:1 strip can host a status line and nothing else —
  extreme geometry signals accidental allocation.

## Configuration

```toml
[rules.aspect-extreme]
severity = "info"
min_area_pt = 4000   # tiny strips are dividers, not ribbons
max_aspect = 12      # longer:shorter axis ratio ceiling
```

## How to fix

- Rebalance the split or give the strip a real role.

## Legitimate exceptions

- True ribbons (status lines, tickers) — allow.
