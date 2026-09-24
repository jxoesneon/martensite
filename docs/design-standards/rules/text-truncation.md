# text-truncation

**Standard(s):** [wcag](../standards/wcag.md), [consistency](../standards/consistency.md)
**Default severity:** `info`
**Confidence:** heuristic — estimated advance width; a run crossing its scope edge is clipped or bleeding, but glyph metrics are approximated.

## What it measures

Each text run's advance (measured glyph width, or
`chars × size × char_width_ratio` when content width is unknown) is
checked against its node's right edge. Overruns past `tolerance_px`
flag.

## The evidence

- **Clipped text reads as a defect** — users can't
  distinguish "intentional ellipsis" from "broken layout" without a
  truncation affordance (…).
- WCAG 1.4.4/1.4.10 context: overflow is what resize/reflow failures
  look like in practice.

## Configuration

```toml
[rules.text-truncation]
severity = "info"
tolerance_px = 2
char_width_ratio = 0.55   # advance estimate when width is unknown
```

## How to fix

- Give the text room, shrink it, or add an explicit truncation
  affordance (ellipsis + tooltip).

## Legitimate exceptions

- Marquee/scrolling text and intentional bleed designs —
  suppress with a scoped allow.
