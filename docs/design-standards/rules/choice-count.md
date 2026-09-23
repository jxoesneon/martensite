# choice-count

**Standard(s):** [hci-laws](../standards/hci-laws.md), [info-design](../standards/info-design.md)
**Default severity:** `warn`
**Confidence:** heuristic — a count over a threshold is a *review
prompt*, not a violation. Seven toolbar buttons and seven equally-
weighted menu commands are both "7", but they are not the same
interface. The rule flags the group for a human to look at.

## What it measures

Counts interactive leaf controls — buttons, fields, pickers, toggles —
under one *decision surface*: the innermost container whose subtree
exceeds the budget. Controls nested inside other controls are not
double-counted.

The rule deliberately reports the **deepest** overloaded group, not the
window that happens to contain it: if a toolbar of 9 buttons sits in a
40-control window, the finding lands on the toolbar.

## The evidence

- **Hick's law** (Hick 1952, Hyman 1953): choice time grows with
  `log₂(n + 1)`. Decision cost rises with every added alternative —
  logarithmically, so the jump from 4→8 hurts more than 8→12, but it
  always costs.
- **Working memory** (Miller 1956's "7±2", revised by **Cowan 2001 to
  4±1 chunks**): users cannot simultaneously weigh more than a handful
  of options; larger sets force serial scanning and re-reading.
- **Few's dashboard canon** ([info-design](../standards/info-design.md)):
  one decision surface should present few, clearly-differentiated
  choices; a wall of equal-weight controls is a design failure mode in
  its own right.

**Default `max = 7`** is the generous classic bound (Miller). Setting
`max = 4` applies Cowan's defensible strict bound — appropriate for
safety-relevant or time-pressured surfaces.

## Configuration

```toml
[rules.choice-count]
severity = "warn"
max = 7    # max simultaneous interactive leaves per decision surface
```

## How to fix

- **Group into categories.** Hick's law is logarithmic *within* a set —
  two grouped sets of 4 are faster to scan than one flat set of 8.
- **Overflow menu.** Rarely-used actions belong behind a `Menu` or
  `Popover`, which is also the [progressive-disclosure](progressive-disclosure.md)
  fix.
- **Split the surface.** If the group mixes unrelated actions, it may
  be two surfaces pretending to be one.
- **Check classification.** A flagged `DataGrid` whose rows count as
  interactive is a data surface, not a decision surface — reclassify it
  in `[classify]` rather than redesigning it.

## When it's OK to allow

- **Dense data grids and tables** where every row is a row, not a
  choice — the legitimate exception this rule exists alongside. If
  `classify` can't capture it, allow the path.
- **Tool palettes** (drawing apps) where the entire point is many
  parallel options.

```toml
[[allow]]
path = "App/DataGrid/**"
rules = ["choice-count"]
```
