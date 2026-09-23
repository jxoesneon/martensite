# WCAG — Web Content Accessibility Guidelines

**Config key:** `wcag`

The W3C's Web Content Accessibility Guidelines are the closest thing
interface design has to law: they are the reference behind legal
accessibility requirements in most jurisdictions, and — more usefully
for a lint system — they publish *testable numeric floors* rather than
aspirations. Where a WCAG success criterion gives a number, the rule
enforcing it can be fully deterministic.

## Why it matters

Accessibility floors are not edge-case charity. Roughly 8% of males
(and ~0.5% of females) have red-green color vision deficiency; motor
impairments, tremor, and situational limitations (one hand, a bumpy
bus, a bright screen) affect everyone eventually. A control that is
physically too small to hit or a state that exists only as a hue is a
defect measured against a published minimum — the most defensible
class of finding this system produces.

## Rules that enforce it

- [target-size](../rules/target-size.md) — WCAG 2.2 **SC 2.5.8 Target
  Size (Minimum)**: interactive targets must be at least 24×24 CSS
  pixels (converted to device px via `scale_factor`).
- [color-only-info](../rules/color-only-info.md) — WCAG 2.2 **SC 1.4.1
  Use of Color**: color must not be the only channel conveying state.
- [alert-saturation](../rules/alert-saturation.md) — adjacent to **SC
  1.4.1** and the general principle that critical signals must remain
  distinguishable; a screen full of alert-colored elements is
  functionally a screen with none.

Related criteria not yet covered by a rule but relevant context: **SC
1.4.11 Non-text Contrast** (3:1 for control boundaries), **SC 1.4.10
Reflow**, and **SC 2.5.5 Target Size (Enhanced)** (44×44, the AAA
level Apple HIG's 44 pt and Material's 48 dp both land near).

## External references

- [WCAG 2.2 — W3C Recommendation](https://www.w3.org/TR/WCAG22/)
- [SC 2.5.8 Target Size (Minimum)](https://www.w3.org/TR/WCAG22/#target-size-minimum)
- [SC 1.4.1 Use of Color](https://www.w3.org/TR/WCAG22/#use-of-color)
- [SC 1.4.11 Non-text Contrast](https://www.w3.org/TR/WCAG22/#non-text-contrast)
- [Understanding SC 2.5.8](https://www.w3.org/WAI/WCAG22/Understanding/target-size-minimum.html)
