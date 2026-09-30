# packing-density

**Standard(s):** [nureg-0700](../standards/nureg-0700.md)
**Default severity:** `warn`
**Confidence:** heuristic — the union area is measured, but which cap
applies (alphanumeric? critical-information?) is a classification
judgment.

## What it measures

The share of a display's area occupied by its *information* — what
"information" means depends on the display:

- **Alphanumeric-dominant displays** measure **character space used** —
  advance-width × em-box per text run, summed over the subtree and
  divided by the display's area. NUREG-0700's 25% alphanumeric cap is a
  text metric; full-bleed row bands and panel headers are background,
  not characters.
- **Other displays** measure the **union** of leaf-widget bounds
  clipped to the display scope. Union, not sum: overlapping elements
  count once. Only leaves that actually paint something count — a
  bare `Flex`/`Container` shell left behind when scrolled-out
  children are culled is layout structure, not an element.
  Coverage alone is not enough on this path: NUREG-0700 relaxes
  packing for graphics-dominant displays because its operative
  attention constraint is *element count*, so a finding requires
  **both** coverage over the cap **and** at least `max_elements`
  (default **8**) distinct elements competing. One painted candidate
  leaf is one element — except that leaves sharing a single widget
  kind (a wall of `VideoTile`s, or one `VideoGrid`/`OrgChart` leaf
  that *is* the mosaic) read as **one configural element**, so a
  homogeneous mosaic measures 1 no matter how much it covers. A few
  large elements filling a display is a layout choice; many distinct
  elements crowding it is packing. The alphanumeric path ignores
  element count — dense text is dense at any element count.

**Document surfaces opt out of the alphanumeric cap via `@prose`.**
NUREG-0700's 25% cap exists for at-a-glance readouts — an operator
scanning a panel for digits. A code editor, log viewer, or rendered
manual is a *document surface*: its body text is the payload the
surface exists to carry, not crowding. A widget or scope that declares
`@prose` in its name (`Markdown@prose`, `JsonView@prose`,
`HexView@prose`, `LogView@prose`, a `CodeEditor@prose` paint scope, or
any ancestor carrying the marker) contributes neither to the
alphanumeric-dominant classification nor to the character-space
numerator — the surface's other leaves still classify and measure
normally, and its painted coverage still counts on the non-alphanumeric
path (a document leaf is one element, so it rarely trips
`max_elements` on its own). `text-density`'s own character coverage
still applies — a 90% character-wall document remains its concern.

**Graphic displays' label ink is annotation, not alphanumeric
payload.** A Gantt's row names, a chart's axis labels, or a week
grid's event captions accompany bars and cells — the surface's
payload is the graphic, so that ink counts toward neither the
alphanumeric-dominant classification nor the character-space
numerator. Text-payload displays keep counting — `table`/`datatable`/
`datagrid`/`log`/`terminal`/`ticker`/`marquee`/`codeeditor`, trees,
and lists: a dense table IS an alphanumeric display. The displays
themselves still measure on the non-alphanumeric path (coverage +
`max_elements`).

Either way the numerator is clipped to the display's own bounds —
scrolled-off content never counts — and **controls never count**:
interactive leaves (buttons, pickers, segments) and scrollbar chrome
are governed by `choice-count`/`target-size`, so a cluster of nothing
but controls measures zero (see
[edge-density](edge-density.md) for the paint-coverage sibling). For
alphanumeric displays this applies to the character numerator too:
a tab's or button's caption is the control, not displayed
information — text runs inside `Interactive`/`Navigation` scopes are
skipped.

The applicable cap is the tightest that matches the scope:

| Condition | Cap | Default |
| --------- | --- | ------- |
| Text-dominant display (alphanumeric elements cover ≥50% of leaf area) | `max_alphanumeric` | 25% |
| Inside a `@level:1` lineage (critical-information tier) | `max_critical` | 35% |
| Otherwise | `max` | 50% |

An L1 text wall takes the alphanumeric cap — tags combine. Only the
**outermost** offender is reported; a dense child inside a dense
display is the same problem, not two findings.

## The evidence

- **NUREG-0700** §1.1: packing density ≤50% generally, ≤25% for
  alphanumeric-heavy displays, minimized for critical information.
  NUREG also relaxes the cap for *graphics-dominant* displays — the
  `max_elements` floor models that relaxation: a homogeneous mosaic
  or a few large elements read as one configural whole, so coverage
  alone on fewer than 8 elements is not treated as packing.
- **ISO 9241-125** §5.1.4 — density of displayed information is the
  international-standard citation for the same construct.
- The industrial dashboard's `@700` column frames measured 90–99%
  coverage — the failure mode this rule exists to catch.

## Configuration

```toml
[rules.packing-density]
severity = "warn"
max = 0.50               # general cap — NUREG-0700's published number
max_alphanumeric = 0.25  # text-dominant cap — NUREG-0700's published number
max_critical = 0.35      # conservative proxy for "minimized for critical info"
alnum_share = 0.5        # leaf-area share that makes a scope "text-dominant"
max_elements = 8         # non-text displays flag only with ≥8 distinct elements
min_surface_pt = 20000   # skip small scopes — a badge row is not a display
```

A leaf counts as an alphanumeric element when its text cells cover
≥5% of its bounds *and* it carries ≥3 estimated characters — icon
buttons and captioned charts don't make a surface "text-dominant".

## How to fix

- **Restyle before splitting** — the ISA-101 density ladder applies:
  consolidate related elements, reduce chrome, then disclose.
- **Split along task lines** — NUREG-0700's own counterweight: do not
  break a unitary task across displays to satisfy the cap. If the
  content must stay together, the finding is asking whether it all
  must be *visible* together (scrolling region, tabs, pagination).
- **Whitespace is the budget** — raise [whitespace](whitespace.md)
  enforcement on the same surface; the two rules measure crowding
  from different sides.

## When it's OK to allow

- A dense data grid whose contract is density — allow by path.
- Transient overlay states (loading splash, empty states) that fill
  their scope deliberately.

```toml
[[allow]]
path = "App/ZonePanel/RegistryGrid"
rules = ["packing-density"]
```
