# simultaneous-channels

**Standard(s):** [faa-hfds](../standards/faa-hfds.md), [isa-101](../standards/isa-101.md)
**Default severity:** `warn`
**Confidence:** heuristic — the channel count is a name-based
classification of "live data display", and the budget is a design-time
proxy for an attention limit the standards state qualitatively.

## What it measures

The count of distinct **live data displays** concurrently visible on
one surface — the widgets a monitoring operator must keep triangulating:
charts, graphs, plots, sparklines, gauges, dials, meters, indicators,
progress displays, tables and data grids, LED matrices, stack lights,
tickers, media feeds, maps, KPIs, gantt/timeline/treemap/fishbone
diagrams, split-flap boards, terminals, heatmaps, histograms, spectra,
and waterfalls — plus the generic `Diagram`/`Canvas`/`Marquee`
families and compound names like `DataGrid`, `VideoGrid`, `MindMap`,
`MapView`, `LedMatrix`, `FlowGraph`. Matching is word-segment exact —
a `Dialog` (contains `dial`) or `Paragraph` (contains `graph`) is not
a channel.

Only the **topmost** display scope counts — a `Chart` containing a
`Sparkline` is one channel, not two; chart internals (axes, legends,
series marks) are never separate channels. Ordinary `Label`s, buttons,
and chrome are not channels either. The finding lands on the
**innermost** scope that exceeds the budget, so a packed sub-pane is
flagged where it lives rather than propagating to the window root.

## The evidence

- **FAA HFDS**: "minimal information density — present only
  information that is essential to a user *at any given time*." The
  channel budget is that norm made countable.
- **ISA-101** display hierarchy: progressive disclosure exists because
  simultaneous load has a ceiling; the L1–L4 ladder is disclosure
  applied to plant data.
- The alarm-domain analog — ISA-18.2's ~10 alarms/10 min flood
  threshold — is already enforced by [flood-cap](flood-cap.md); this
  rule covers the *continuous* monitoring channels alarms sit on top
  of.

**Default `max = 7`** channels is deliberately the working-memory
bound (Miller/Cowan), not a published standard number — the standards
give the norm ("essential at a given time"), not a count. Tune to the
product's mission.

## Configuration

```toml
[rules.simultaneous-channels]
severity = "warn"
max = 7                 # concurrent data-display budget
min_surface_pt = 20000  # skip small scopes
```

## How to fix

- **Disclose** — move secondary channels behind tabs, a drill-down
  detail pane, or a separate L2 display ([level-purity](level-purity.md)
  is the companion rule).
- **Merge** — related series on one chart is one channel; three
  adjacent single-series charts is three.
- **Don't split integrated task data** — HFDS's own clause: information
  needed together belongs together. If the finding's channels are a
  unitary monitoring task, the right fix is usually a deliberate
  `[[allow]]` with a comment, not a fragmented display.

## When it's OK to allow

- Overview/mosaic pages whose purpose is breadth — the point of the
  screen is many channels; allow it by path.
- A widget catalog or test fixture that intentionally packs displays.

```toml
[[allow]]
path = "App/OverviewWall"
rules = ["simultaneous-channels"]
```
