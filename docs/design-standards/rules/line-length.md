# line-length

**Standard(s):** [info-design](../standards/info-design.md)
**Default severity:** `info`
**Confidence:** heuristic — text-block width estimated from run geometry.

## What it measures

Text blocks outside the readable 45–75ch band (as
rendered width, approximated by run extents).

## The evidence

- Typographic canon (Bringhurst): ~66 chars/line is the
  readability sweet spot; 120ch lines lose the return sweep.

## Configuration

```toml
[rules.line-length]
severity = "info"
max_chars = 75   # the readability ceiling (Bringhurst ~66 midpoint)
```

## How to fix

- Constrain text width with a max-width column.

## Legitimate exceptions

- Code/log views and tabular data — allow.
