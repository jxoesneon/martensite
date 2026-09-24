# icon-only-control

**Standard(s):** [wcag](../standards/wcag.md), [info-design](../standards/info-design.md)
**Default severity:** `warn`
**Confidence:** heuristic — no text in the subtree usually means icon-only, but a glyph-font icon paints as text and won't be caught; both are still unlabeled to users who don't know the glyph.

## What it measures

Every interactive leaf on a surface is checked for any
`TextStat` in its subtree. Zero text = finding.

## The evidence

- **Nielsen #6 — recognition over recall**: unlabeled
  icons must be *remembered*, labels are *recognized*.
- **WCAG 4.1.2** — a control with no name is unnameable to assistive
  technology; an icon font is not a name.

## Configuration

```toml
[rules.icon-only-control]
severity = "warn"
# no thresholds — textless interactive leaves flag
```

## How to fix

- Add a visible label, or ensure the icon carries an accessible
  name + tooltip. The rule only sees paint — name your controls anyway.

## Legitimate exceptions

- Universally-conventional icons (×, ▶, ⚙ in context) are
  the legitimate case — suppress per-control, not globally.
