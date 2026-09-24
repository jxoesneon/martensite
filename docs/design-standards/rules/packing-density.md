# packing-density

**Standard(s):** [nureg-0700](../standards/nureg-0700.md)
**Default severity:** `warn`
**Confidence:** heuristic — the union area is measured, but which cap
applies (alphanumeric? critical-information?) is a classification
judgment.

## What it measures

The share of a display's area occupied by its element footprints —
the **union** of leaf-widget bounds clipped to the display scope,
divided by the scope's area. Union, not sum: overlapping elements are
counted once, and full-surface background fills do not count as
occupancy (element bounds, not painted coverage — see
[edge-density](edge-density.md) for the paint-coverage sibling).

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
  lint can't reliably tell a mimic diagram from a packed panel, so
  that relaxation is deliberately unmodeled: allow legitimately
  graphics-heavy displays by path instead.
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
