# edge-density

**Standard(s):** [perception](../standards/perception.md)
**Default severity:** `info`
**Confidence:** heuristic — painted coverage is measured exactly, but
it is a *proxy* for clutter, not clutter itself. `info` severity is
honest about that: a cheap first-order signal, not a verdict.

## What it measures

The **painted-area coverage ratio** per node: summed descendant fill
area (clipped to bounds) ÷ the node's own area. A ratio near 1 means
the surface is painted edge-to-edge — no breathing room, no
uncommitted space, everything is content.

This is the structural analog of Rosenholtz's edge-density clutter
measure: the intuition that a surface approaching full coverage is
perceptually "loud." It is deliberately the cheap version — true
feature-congestion requires rasterizing the scene and measuring actual
edge energy, which is a documented future tier. Today's coverage ratio
is the honest approximation: it catches the extreme (full-bleed
surfaces) without pretending to measure what it can't.

## The evidence

- **Rosenholtz, Li & Nakano, J. Vision 2007
  ([DOI 10.1167/7.2.17](https://doi.org/10.1167/7.2.17))**
  ([perception](../standards/perception.md)): visual clutter —
  operationalized as feature congestion and edge density — predicts
  search time. The same work motivates *why this rule is info-level*:
  raw coverage is a weak proxy for congestion, and the research itself
  cautions against overclaiming cheap surrogates.
- **Gestalt grouping**: whitespace is the grouping signal; a
  full-coverage surface has spent all of it and reads as one
  undifferentiated block (see [whitespace](whitespace.md) — the
  companion rule at the *inter-element* scale).
- **VizLinter (TVCG 2021)** — the linter-for-visual-output precedent
  this whole crate follows.

## Configuration

```toml
[rules.edge-density]
severity = "info"
max_coverage = 0.85     # painted-coverage ratio above this is flagged
max_descendants = 40    # subtree node count above this is flagged
min_area = 20000        # scopes below this area (px²) skip the coverage check
```

## How to fix

- **Reclaim the margins.** Full-bleed fills are usually a background
  rect the surface doesn't need, or padding that was never added —
  edge padding is the cheapest fix in the catalog.
- **Check what "painted" means.** Coverage counts fills; a surface
  flagged here with low [density](density.md) is probably one big
  background fill, not actual crowding — a different fix than the
  density finding would suggest.
- **If the coverage is content** (a photo, a chart, a map), this is the
  documented weak-proxy case — allow it.

## When it's OK to allow

- **Media surfaces** — video, image, map, and canvas content is
  *supposed* to be full-bleed.
- **Kiosk and ambient displays** where full coverage is the design.

```toml
[[allow]]
path = "App/VideoWall/**"
rules = ["edge-density"]
```
