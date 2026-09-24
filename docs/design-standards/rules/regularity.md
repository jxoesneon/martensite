# regularity

**Standard(s):** [perception](../standards/perception.md)
**Default severity:** `info`
**Confidence:** heuristic — variance threshold on sibling spacing.

## What it measures

Sibling gap variance within surfaces — ragged spacing
rhythm flags.

## The evidence

- Miniukovich & De Angeli regularity — consistent
  spacing rhythm is half the 'aligned' feeling alignment rules can't
  fully capture.

## Configuration

```toml
[rules.regularity]
severity = "info"
min_group = 4    # minimum siblings before rhythm is judged
max_cv = 0.15    # coefficient-of-variation ceiling on gaps
```

## How to fix

- Snap gaps to the spacing grid (`spacing-token` enforces the
  token side).

## Legitimate exceptions

- Intentional rhythm changes between sections — allow.
