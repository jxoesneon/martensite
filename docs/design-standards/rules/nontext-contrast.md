# nontext-contrast

**Standard(s):** [wcag](../standards/wcag.md)
**Default severity:** `warn`
**Confidence:** deterministic — measured contrast between edge-sharing sibling fills.

## What it measures

Sibling nodes sharing an edge (touching on one axis,
overlapping on the other) have their fills compared pairwise. Any
adjacent pair below 3:1 is flagged — boundaries the user literally
cannot see.

## The evidence

- **WCAG 2.2 SC 1.4.11, Non-text Contrast**
  ([w3.org/TR/WCAG22/#non-text-contrast](https://www.w3.org/TR/WCAG22/#non-text-contrast)):
  visual information identifying a component's boundary needs 3:1
  against adjacent colors.

## Configuration

```toml
[rules.nontext-contrast]
severity = "warn"
min_ratio = 3.0
max_fills = 50   # skip pathological fill counts (gradient strips)
```

## How to fix

- The autofix (risky) raises the lighter/darker fill to 3:1.
- A visible border at 3:1 also satisfies the SC without changing fills.

## Legitimate exceptions

- Intentional tone-on-tone grouping (cards on a near-match
  panel) where boundaries are conveyed by shadow or layout — suppress.
