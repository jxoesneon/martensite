# density

**Standard(s):** [hci-laws](../standards/hci-laws.md), [perception](../standards/perception.md)
**Default severity:** `info`
**Confidence:** heuristic — the count and area are measured, but "too
dense" is context-dependent. Dense data grids legitimately exceed the
default; the finding asks whether the crowding is deliberate.

## What it measures

Interactive controls per normalized surface area — specifically,
interactive leaf nodes per **10,000 pt²** (a 100×100 pt square), after
converting device px to pt via `scale_factor`. Surfaces with fewer
than ~4 controls are skipped — a small cluster is a control group, not
a density problem.

Density is the crowding measure [choice-count](choice-count.md) isn't:
a toolbar of 6 buttons spread over a phone-width strip and the same 6
packed into a thumbnail are the same count, different density.

## The evidence

- **Fitts's law** ([hci-laws](../standards/hci-laws.md)): mis-click
  risk scales with crowding — neighboring targets steal overshoots,
  and small inter-target gaps multiply pointing error even when each
  target individually meets [target-size](target-size.md).
- **Rosenholtz clutter research** ([perception](../standards/perception.md)):
  visual search time scales with feature congestion — dense control
  fields are dense feature fields; the user spends longer finding the
  control before ever clicking it.
- **Gestalt proximity**: crowded controls defeat grouping — at high
  density, related and unrelated controls sit at indistinguishable
  distances.

**Default `max = 0.6`** controls per 10,000 pt² is a conservative
guideline bound — the research gives a direction (crowding costs), not
a universal threshold. Dense data-entry surfaces legitimately tune it
up.

## Configuration

```toml
[rules.density]
severity = "info"
max = 0.6             # max controls per 10,000 pt²
min_surface_pt = 20000  # surfaces below this area are skipped
```

## How to fix

- **Spread or split.** The same controls over more area, or fewer
  controls over the same area — either lowers the ratio.
- **Disclose.** Move secondary controls into a `Popover`/`Sheet`/overflow
  `Menu` — [progressive-disclosure](progressive-disclosure.md) is the
  companion rule.
- **Combine with [choice-count](choice-count.md) and [whitespace](whitespace.md).**
  A surface tripping all three is the same problem from three sides —
  the fix is usually structural, not three separate tweaks.

## When it's OK to allow

- **Dense data grids** — the canonical legitimate exception; rows are
  data, not decisions.
- **Professional tools** (video editors, trading UIs) where density is
  the product's contract.

```toml
[[allow]]
path = "App/ZonePanel/GridView"
rules = ["density", "choice-count"]
```
