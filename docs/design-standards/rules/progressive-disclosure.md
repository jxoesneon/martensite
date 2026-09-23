# progressive-disclosure

**Standard(s):** [isa-101](../standards/isa-101.md), [info-design](../standards/info-design.md)
**Default severity:** `info`
**Confidence:** heuristic — the absence of a disclosure affordance is a
measured fact, but "this surface *should* hide some controls" is the
most contextual judgment in the catalog. `info` severity is honest
about that.

## What it measures

Flags surfaces with **many interactive controls and no disclosure
affordance anywhere in their descendants** — no `Expander`, `Sheet`,
`Drawer`, `Popover`, or `Menu` (matched by name). The finding names a
structural shape: a surface presenting all its controls at once, all
the time, with no mechanism for progressive disclosure.

It's the ISA-101 question made concrete: is this an L4-everything
display, or does detail get disclosed progressively?

## The evidence

- **ANSI/ISA-101** ([isa-101](../standards/isa-101.md)): the
  four-level progressive display hierarchy (L1 overview → L4
  diagnostic) exists because operators can't maintain situation
  awareness when every level of detail is simultaneously visible.
  Endsley's situation-awareness model gives the mechanism — detail
  competes for the perceptual and comprehension resources needed for
  the abnormal-state detection the display exists for.
- **Nielsen #8** ([info-design](../standards/info-design.md),
  "aesthetic and minimalist design"): every unit of rarely-needed
  information competes with the relevant units. Disclosure affordances
  are the structural fix — the control exists, but doesn't cost
  attention until summoned.
- **Companion to [choice-count](choice-count.md) and
  [density](density.md)**: a surface tripping all three is almost
  always the same underlying shape — flat, dense, undisclosed.

The rule only fires above a control-count threshold — a 3-control
panel doesn't need disclosure; a 15-control one probably does.

## Configuration

```toml
[rules.progressive-disclosure]
severity = "info"
max = 10              # interactive controls a surface may hold before
                      # it needs a disclosure affordance
```

## How to fix

- **Add a disclosure affordance.** Move secondary or rarely-used
  controls into an `Expander`, `Sheet`, `Drawer`, `Popover`, or
  `Menu` — the rule checks for these by name, and adding one is the
  intended fix, not a loophole.
- **Split the surface.** If controls span genuinely different tasks,
  the surface may be two surfaces pretending to be one.
- **Declare the level.** `@level:N` on the widget marks its ISA-101
  display level — the fix for "flat L4" is often *declaring* that a
  surface is L4 detail summoned on demand, not making it always
  visible.

## When it's OK to allow

- **Control panels by design** — a mixing board, a debugger, a
  settings page where the whole point is parallel access.
- **Small fixed palettes** where the count is the interface.

```toml
[[allow]]
path = "App/ControlPanel/**"
rules = ["progressive-disclosure"]
```

Or inline: `"ControlPanel@lint:progressive-disclosure"`.
