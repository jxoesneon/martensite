# text-contrast

**Standard(s):** [wcag](../standards/wcag.md)
**Default severity:** `warn`
**Confidence:** deterministic — painted colors against painted backgrounds, same WCAG 2.x luminance math a renderer applies.

## What it measures

Every `TextStat` (from `DrawText` and glyph runs) is
measured against the smallest opaque `FillRect` containing its origin —
the background the reader's eye actually sits on. Large text (≥18pt)
gets the relaxed 3:1 requirement; everything else needs 4.5:1.

## The evidence

- **WCAG 2.2 SC 1.4.3, Contrast (Minimum)**
  ([w3.org/TR/WCAG22/#contrast-minimum](https://www.w3.org/TR/WCAG22/#contrast-minimum)):
  4.5:1 normal, 3:1 large — the AA floor.
- **Why paint-list contrast**: measuring the *painted* colors catches
  both token-level mistakes and "correct token on wrong surface"
  defects a theme audit can't see.

## Configuration

```toml
[rules.text-contrast]
severity = "warn"
min_ratio = 4.5        # normal text floor (WCAG AA)
min_ratio_large = 3.0  # large text (≥24px = 18pt per WCAG)
```

## How to fix

- The autofix (risky, needs `--force`) recolors the text toward
  black or white — whichever reaches the ratio with less movement.
- Prefer fixing the *background* when the text color is brand-bearing.

## Legitimate exceptions

- Decorative text, inactive controls, and logotype are
  exempt per the SC — suppress with an `[[allow]]` scoped to the path.
