# whitespace

**Standard(s):** [info-design](../standards/info-design.md), [perception](../standards/perception.md)
**Default severity:** `info`
**Confidence:** heuristic — spacing deficits are a strongly subjective
call. `info` severity reflects that: a nudge to look at the layout,
never a build blocker.

## What it measures

Looks for **insufficient spacing between elements** — sibling gaps
that fall below the spacing rhythm the layout implies — and cramped
grouping, where related and unrelated elements sit at indistinguishable
distances so no visual grouping forms.

Whitespace is not empty waste; it is the *mechanism* by which the eye
parses "these belong together, those don't." A surface where every gap
is the same size, or where gaps are too small to register, reads as
one undifferentiated block regardless of how logical the hierarchy is.

## The evidence

- **Gestalt proximity** ([perception](../standards/perception.md)):
  elements closer together are perceived as a group — this is the
  strongest and most reliable grouping cue, stronger than color or
  shape. When inter-item and inter-group gaps are equal, the grouping
  signal is destroyed; users must reconstruct structure by reading
  every element.
- **Few, *Information Dashboard Design*** ([info-design](../standards/info-design.md)):
  cramped dashboards force serial scanning; whitespace is what lets a
  user find the number that matters at a glance.
- **Rosenholtz clutter research**: insufficient spacing raises
  perceptual crowding — features interfere with each other in the
  visual periphery, which is the mechanism behind "it feels cramped."

The rule is deliberately conservative — `info` level — because no
single gap threshold is right for every design language.

## Configuration

```toml
[rules.whitespace]
severity = "info"
min_gap_pt = 4        # gaps below this between siblings count as cramped
min_children = 3      # containers with fewer children are skipped
```

## How to fix

- **Differentiate the gaps.** Inter-group spacing should be visibly
  larger than intra-group spacing — that's the grouping signal. Route
  gaps through layout spacing tokens rather than per-widget margins.
- **Add breathing room at boundaries.** The cheapest readability win
  in a dense surface is padding around the edge, not between items.
- **If spacing is fine and the complaint is wrong**, check whether
  absolutely-positioned decorations are being counted as siblings —
  `min_gap_pt` and `min_children` tune the sensitivity.

## When it's OK to allow

- **Dense professional tools** (DAs, trading UIs, code editors) where
  tight spacing is the product's contract with its users.
- **Kiosk/table-like surfaces** that are deliberately uniform.

```toml
[[allow]]
path = "App/TerminalPane/**"
rules = ["whitespace"]
```
