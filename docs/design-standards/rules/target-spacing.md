# target-spacing

**Standard(s):** [wcag](../standards/wcag.md)
**Default severity:** `info`
**Confidence:** heuristic — the spacing floor complements size; what's 'too close' depends on target size too.

## What it measures

Adjacent interactive siblings whose band-overlap exceeds
half the shorter side have their gap measured on both axes. Pairs under
`min_gap_pt` count toward a per-parent finding.

## The evidence

- **WCAG 2.2 SC 2.5.8** spacing clause — undersized
  targets pass when spacing compensates; the inverse (adequate size,
  zero spacing) still multiplies mis-clicks.
- **Fitts's law**: crowding shrinks the effective target width.

## Configuration

```toml
[rules.target-spacing]
severity = "info"
min_gap_pt = 4
```

## How to fix

- The autofix (safe) applies `SetGap` to space the row out.

## Legitimate exceptions

- Segmented controls and ribbon groups where flush adjacency
  *is* the visual contract — suppress.
