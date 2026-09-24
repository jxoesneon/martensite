# edge-touch

**Standard(s):** [consistency](../standards/consistency.md), [info-design](../standards/info-design.md)
**Default severity:** `info`
**Confidence:** heuristic — flush edges are measured; 'cramped' is the judgment call.

## What it measures

Children flush (≤tolerance) on ≥2 edges with all edge
gaps under `min_pad_pt` count toward a per-surface finding.

## The evidence

- Gestalt common-region — padding is the boundary cue;
  edge-to-edge content reads as clipped, not contained.

## Configuration

```toml
[rules.edge-touch]
severity = "info"
min_pad_pt = 4
tolerance_px = 1.0
min_surface_pt = 20000
```

## How to fix

- Add container padding or inset the child.

## Legitimate exceptions

- Full-bleed media and intentional edge-to-edge design —
  allow.
