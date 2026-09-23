# Consistency — Design-System Discipline

**Config key:** `consistency`

The internal-consistency layer: not an external standard but the
design-system canon every mature lint ecosystem converges on. A UI is
a *system* — colors, type sizes, and spacing form a vocabulary. Rules
in this standard check that the rendered scene actually speaks it:
declared tokens are the ones painted, type sizes form a deliberate
scale, and repeated structures stay regular.

## Why it matters

Consistency is load-bearing in two directions:

- **For users**, consistent surfaces are learnable — the same blue
  always means the same action, the same 13 pt is always a caption.
  Violations don't just look sloppy; they break the implicit contract
  that lets users transfer knowledge between screens. Nielsen's
  heuristic #4 ("consistency and standards") and the Gestalt
  similarity principle both point here.
- **For the codebase**, drift is the canary. `token-drift` catches
  hard-coded hex values that bypassed the theme; `type-scale` catches
  one-off font sizes that bypassed the type ramp. These are the
  findings that scale with team size — the rules a design system
  exists to enforce.

## Rules that enforce it

- [type-scale](../rules/type-scale.md) — distinct font sizes per
  surface; typographic scale discipline.
- [token-drift](../rules/token-drift.md) — painted colors outside the
  declared palette (active only when `palette_entries` is
  configured).
- [color-budget](../rules/color-budget.md) — saturated-hue discipline;
  also [isa-101](isa-101.md) (the alarm-channel argument).
- [alignment](../rules/alignment.md) — sibling-edge regularity; also
  [perception](perception.md) (the measured-aesthetics argument).

## External references

- Nielsen, J. (1994). Heuristic #4, "Consistency and standards" —
  [nngroup.com/articles/ten-usability-heuristics](https://www.nngroup.com/articles/ten-usability-heuristics/)
- Gestalt principle of similarity — same appearance implies same
  meaning; drift severs that implication.
- Bringhurst, R. *The Elements of Typographic Style* — the modular
  scale tradition behind `type-scale`'s 4-role default.
- Design-token practice (W3C Design Tokens Community Group) — the
  declared palette this lint checks against.
