# type-scale

**Standard(s):** [consistency](../standards/consistency.md)
**Default severity:** `info`
**Confidence:** heuristic — the size count is exact, but whether N
sizes is "a deliberate ramp" or "a hierarchy problem" is a judgment.
The finding prompts a look at the type system, not a fix by reflex.

## What it measures

Counts the **distinct font sizes** painted anywhere in each surface's
subtree. Sizes are quantized (to ~0.5 pt granularity) before counting
so that 13.0 pt and 13.1 pt don't count as two roles.

A surface with more than `max` distinct sizes usually means text
styles were set ad hoc rather than drawn from a type ramp — the
typographic equivalent of [token-drift](token-drift.md) for colors.

## The evidence

- **Typographic canon** ([consistency](../standards/consistency.md)):
  classical scale discipline (Bringhurst's modular scales; every major
  design system's type ramp — Material, Carbon, Fluent) converges on a
  small set of *roles*: body, caption/label, title, headline, display.
  A surface that needs more than ~4–5 distinct text roles is almost
  always expressing hierarchy it doesn't have, or styling text without
  a system.
- **Gestalt similarity**: same size implies same rank. Six arbitrary
  sizes break the reader's ability to infer structure from typography;
  the hierarchy becomes noise.
- **Nielsen #4** (consistency and standards): users build a mental
  model of "what a heading looks like here" — each off-ramp size
  breaks that model.

**Default `max = 4`** — body, secondary/caption, section title, and one
display or emphasis role covers a well-formed surface.

## Configuration

```toml
[rules.type-scale]
severity = "info"
max = 5               # max distinct font sizes per surface
min_surface_pt = 20000  # surfaces below this area are skipped
```

## How to fix

- **Adopt a ramp.** Define the type roles once (a modular scale like
  12/14/18/24 or 13/15/20/28) and route every text style through it —
  the fix is usually deleting sizes, not adding them.
- **Find the one-off.** The finding's subtree walk makes outliers
  visible: one widget at 11.5 pt surrounded by a 13 pt system is the
  drift.
- **Distinguish roles, not sizes.** If two levels of hierarchy
  genuinely differ, vary weight or color *within* a size before adding
  another size.

## When it's OK to allow

- **Rich text / document surfaces** where content legitimately spans
  many sizes (an article renderer, a markdown preview).
- **Data displays** where size encodes a value (word clouds,
  proportional labels).

```toml
[[allow]]
path = "App/DocPreview/**"
rules = ["type-scale"]
```
