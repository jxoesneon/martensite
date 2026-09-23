# color-only-info

**Standard(s):** [wcag](../standards/wcag.md), [isa-101](../standards/isa-101.md)
**Default severity:** `warn`
**Confidence:** heuristic — the shape of the signal (saturated fill,
no text in the subtree) is measured exactly, but "this fill is
conveying state" is an inference. A colored node with no text might be
a status indicator — or might just be a colored panel. The finding is
a review prompt asking which.

## What it measures

Flags nodes painted as a **saturated fill with no text anywhere in
their subtree**. The reasoning: if a node carries a strongly saturated
color and no text explains what the color means, the state it encodes
(if any) is conveyed by color alone — invisible to users who can't
distinguish the hue, and unlabeled for everyone.

This is the scene-tree approximation of WCAG's "don't rely on color
alone": the rule can't know whether the fill *means* something, but a
saturated fill with zero redundant coding is exactly the shape the
success criterion exists to catch.

## The evidence

- **WCAG 2.2 SC 1.4.1, Use of Color**
  ([w3.org/TR/WCAG22/#use-of-color](https://www.w3.org/TR/WCAG22/#use-of-color))
  ([wcag](../standards/wcag.md)): color must not be the only visual
  means of conveying information, indicating an action, or
  distinguishing an element. ~8% of males (and ~0.5% of females) have
  red-green color vision deficiency — a red-vs-green status cell with
  no label is literally invisible state to them.
- **ISA-101** ([isa-101](../standards/isa-101.md)): redundant coding is
  the standard's answer to the same problem — alarm and abnormal
  states carry color *plus* a shape, label, or symbol, never color
  alone. ISA-101's display-design guidance explicitly requires
  redundant indicators for exactly this reason.
- **Endsley SA level 1**: color-only signals fail at the perception
  step — before comprehension is even possible.

## Configuration

```toml
[rules.color-only-info]
severity = "warn"
min_saturation = 0.4  # fills below this saturation don't count as signals
min_area_pt = 100     # fills below this area are decoration, not state
```

## How to fix

- **Add redundant coding.** A label, icon, shape difference, or text
  inside the subtree makes the state robust — this is the fix both
  WCAG and ISA-101 prescribe, and it usually resolves the companion
  [color-budget](color-budget.md) concern too.
- **If the fill isn't state**, it's decoration — which is a
  [color-budget](color-budget.md) question, not a color-only-info one.
  Desaturating it often fixes both.
- **Check the subtree.** A node flagged because its label is *just*
  outside its scope (a sibling caption) may genuinely have text
  explaining it — the fix is making the scope honest, not adding
  redundant text.

## When it's OK to allow

- **Genuinely decorative fills** — a colored accent bar that encodes
  nothing.
- **Data-driven color fields** (heat maps, charts) where the color
  *is* the data and a legend elsewhere provides the redundant channel
  the rule can't see cross-scope.

```toml
[[allow]]
path = "App/HeatmapView/**"
rules = ["color-only-info"]
```
