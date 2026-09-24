# paint-complexity

**Standard(s):** [perception](../standards/perception.md), [info-design](../standards/info-design.md)
**Default severity:** `info`
**Confidence:** heuristic — geometric proxy for Rosenholtz feature congestion (which needs a rasterized frame); tuned so calm toolbars pass and packed grids flag.

## What it measures

Every fill + text run in a node's subtree per 10,000px²
of node area. Over `max_ops_per_10k` flags.

## The evidence

- **Rosenholtz et al., Feature Congestion (J. Vision
  2007)**: visual-search time tracks element density — the proxy
  measures drawing-op density, the strongest correlate available
  without pixels.

## Configuration

```toml
[rules.paint-complexity]
severity = "info"
max_ops_per_10k = 40
min_surface_pt = 20000
```

## How to fix

- Reduce decoration (grid lines, redundant borders), or split
  the surface.

## Legitimate exceptions

- Data-dense visualizations (heatmaps, scatters) are
  dense by nature — allow the chart subtree.
