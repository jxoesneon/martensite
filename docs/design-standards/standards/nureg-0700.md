# NUREG-0700 — Human-System Interface Design Review Guidelines

**Config key:** `nureg-0700`

NUREG-0700 (NRC Human-System Interface Design Review Guidelines, Rev. 3/
Rev. 4 review draft) is the most quantified display-design standard in
print. Written for nuclear power plant control rooms — the domain where
display clutter has the worst audited consequences — its review
checklist assigns numbers where other standards offer adjectives.

## Why it matters

Its *Information Display Density* guidance is the load-bearing citation
for packing-density:

- **Packing density should not exceed 50%** of a display's usable area.
- **Alphanumeric-dominant displays should not exceed 25%** — text-heavy
  screens fatigue and mislead at roughly half the graphic cap.
- **Graphics-dominant displays may be more dense** — the published
  relaxation. The lint doesn't model it (it can't reliably tell a
  mimic diagram from a packed control panel); a legitimately
  graphics-heavy display that trips the general cap should be
  `[[allow]]`ed by path.
- **Density should be minimized for critical information** — the linter
  reads `@level:1` (ISA-101 overview) lineages as the project's critical
  tier and applies a stricter cap there.
- **Over-full displays should be split or paginated** — but *not* when
  splitting would break a unitary task. The counterweight keeps the
  rule honest: integrate what the task integrates, paginate the rest.

## Rules that enforce it

- [packing-density](../rules/packing-density.md) — union of element
  footprints per display scope vs. the 50% / 25% / critical caps.
- [level-purity](../rules/level-purity.md) — cites the critical-
  information density guidance alongside its ISA-101 L1 basis.

## Design-time proxies

The published thresholds describe *used area* in a display. The lint
approximates "used" as the union of element bounds — background fill
coverage is not occupancy, and overlapping elements are counted once.
Findings are `heuristic` confidence: the measurement is real, the
judgment (does this display warrant the alphanumeric cap?) remains a
review prompt.

## External references

- NUREG-0700 Rev. 3, *Human-System Interface Design Review Guidelines*
  (U.S. NRC, 2020), §1.1 information display density — public domain,
  freely available from the NRC. (Rev. 2, 2002, contains the same
  density guidance.)
- Rev. 4 review draft retains the same density guidance.
