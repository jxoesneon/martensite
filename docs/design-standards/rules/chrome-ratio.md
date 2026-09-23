# chrome-ratio

**Standard(s):** [info-design](../standards/info-design.md)
**Default severity:** `warn`
**Confidence:** heuristic — area share is measured deterministically,
but "too much chrome" is a design judgment the threshold only
approximates. Treat the finding as a prompt to look at the surface,
not a bug.

## What it measures

For each surface (a node with enough children and area to be a real
region, not a control cluster), sums the bounds of its **navigation**
and **chrome** children — tab bars, toolbars, title bars, status bars,
rails — and divides by the surface's own area. That share is the
chrome ratio: the fraction of the window spent on orientation furniture
rather than the content the window exists to show.

## The evidence

- **Tufte's data-ink ratio** (*The Visual Display of Quantitative
  Information*): the share of ink carrying actual information should
  approach 1. Chrome is the structural non-data ink — every pixel of
  toolbar is a pixel of data that isn't there.
- **Few, *Information Dashboard Design* ch. 3** ([info-design](../standards/info-design.md)):
  the canonical dashboard failure is furniture out-competing data —
  "the content is fighting its own frame."
- **ISA-101** ([isa-101](../standards/isa-101.md)): a high-performance
  display devotes its surface to process information; orientation
  chrome is deliberately thin and peripheral.

**Default `max = 0.45`** is a conservative bound, not a standard's
number — the research gives a direction ("content should dominate"),
not a threshold. 45% is the point where chrome is nearly half the
screen and the imbalance is hard to defend.

## Configuration

```toml
[rules.chrome-ratio]
severity = "warn"
max = 0.45            # max chrome area share per surface
min_surface_pt = 20000  # nodes below this area are control clusters, not surfaces
```

## How to fix

- **Shrink or collapse chrome.** Reduce persistent bars; move
  rarely-needed controls into a `Popover`/`Menu` (see
  [progressive-disclosure](progressive-disclosure.md)).
- **Check classification.** A `Sidebar` that *is* the content (e.g. a
  chat panel) classified as navigation inflates the ratio — fix
  `[classify]`, not the layout.
- **Combine with [nav-depth](nav-depth.md).** A high chrome ratio with
  stacked nav layers is the same problem seen from two sides — merge
  the layers.

## When it's OK to allow

- **Tool-first apps** (editors, IDEs) where chrome *is* the product —
  allow the specific surface, not the rule.
- **Compact utility windows** that are all controls by design.

```toml
[[allow]]
path = "App/ToolPalette"
rules = ["chrome-ratio"]
```
