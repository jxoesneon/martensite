# alignment

**Standard(s):** [perception](../standards/perception.md), [consistency](../standards/consistency.md)
**Default severity:** `info`
**Confidence:** heuristic — edge coordinates are measured exactly, but
whether irregular edges read as "scattered" or "deliberately staggered"
is an aesthetic judgment. The finding is a review prompt, not a verdict.

## What it measures

For each container with enough children to matter, collects the
distinct x-coordinates of sibling **left edges** and y-coordinates of
**top edges** (with a configurable pixel tolerance). Every unique edge
is a visual seam the eye must trace.

- A clean column has ~1 distinct left edge.
- A clean row has ~1 distinct top edge.
- A grid has few of both.

A container whose children align in **neither** axis — many distinct
lefts *and* many distinct tops — is flagged as scattered.

## The evidence

- **Miniukovich & De Angeli (CHI 2015,
  [DOI 10.1145/2702123.2702575](https://doi.org/10.1145/2702123.2702575))**:
  computable layout metrics predict aesthetic judgment, and alignment
  alone explains roughly half the variance. Sibling-edge regularity is
  the single strongest measurable proxy for "this looks designed."
- **Gestalt grouping** ([perception](../standards/perception.md)):
  aligned edges form perceived groups; scattered edges break
  continuation and read as noise before any content is parsed.
- **[Consistency](../standards/consistency.md)**: repeated irregular
  alignment usually means ad-hoc layout values where a spacing rhythm
  was intended.

## Configuration

```toml
[rules.alignment]
severity = "info"
max_edges = 4        # max distinct sibling edges per axis before flagging
min_children = 4     # containers with fewer children are skipped
tolerance_px = 2     # edges within this distance count as one
```

## How to fix

- **Snap to a spacing rhythm.** Route offsets through the layout
  system (Flex gaps, grid columns) rather than hand-tuned insets —
  irregular edges are usually a symptom of per-widget pixel pushing.
- **Align one axis deliberately.** If children can't share a left
  edge, give them a shared top edge — one clean axis reads as
  intentional where zero reads as scattered.
- **Check for classification noise.** A container whose "children"
  include absolutely-positioned decorations will report phantom edges.

## When it's OK to allow

- **Intentionally staggered layouts** — masonry, timeline zig-zags,
  dashboards with deliberately offset cards.
- **Freeform canvases** where children are user-positioned.

```toml
[[allow]]
path = "App/Timeline/**"
rules = ["alignment"]
```

Or on the widget: `"TimelineBoard@lint:alignment"`.
