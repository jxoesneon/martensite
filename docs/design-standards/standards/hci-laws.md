# HCI Laws — Hick, Miller/Cowan, Fitts

**Config key:** `hci-laws`

The quantitative laws of human-computer interaction: small, old, and
unusually well-replicated experimental results that put *numbers* on
the cost of choices, targets, and memory load. They are the reason
"how many buttons are on this screen?" is a measurable question rather
than a taste debate.

## Why it matters

Three laws do most of the work:

- **Hick's law** (Hick 1952; Hyman 1953): choice time grows with
  `log₂(n + 1)` — decision cost is *logarithmic* in the number of
  alternatives, which is why grouping choices into categories beats
  flattening them, and why adding one more button to an already-large
  set costs less than adding the first one to a small set — but still
  costs.
- **Working-memory limits** (Miller 1956's famous "7±2", revised down
  by Cowan 2001 to **4±1 chunks**): the number of items a user can
  hold and compare at once is small. Choice sets that exceed it force
  serial scanning and re-reading.
- **Fitts's law** (Fitts 1954): pointing time grows with distance and
  shrinks with target size — `T = a + b·log₂(1 + D/W)`. Small,
  crowded targets are slow *and* error-prone; crowding multiplies the
  mis-click risk because neighboring targets steal overshoots.

## Rules that enforce it

- [nav-depth](../rules/nav-depth.md) — each nested navigation layer is
  another "where am I" decision the user must hold in working memory.
- [choice-count](../rules/choice-count.md) — simultaneous actions on
  one decision surface, bounded by Hick + Cowan.
- [density](../rules/density.md) — controls per unit area; Fitts's-law
  mis-click risk and visual search cost scale with crowding.

## External references

- Hick, W. E. (1952). "On the rate of gain of information."
  *Quarterly Journal of Experimental Psychology* 4(1), 11–26.
- Hyman, R. (1953). "Stimulus information as a determinant of reaction
  time." *Journal of Experimental Psychology* 45(3), 188–196.
- Miller, G. A. (1956). "The magical number seven, plus or minus two."
  *Psychological Review* 63(2), 81–97.
- Cowan, N. (2001). "The magical number 4 in short-term memory: a
  reconsideration of mental storage capacity." *Behavioral and Brain
  Sciences* 24(1), 87–114 — the 4±1 revision.
- Fitts, P. M. (1954). "The information capacity of the human motor
  system in controlling the amplitude of movement." *Journal of
  Experimental Psychology* 47(6), 381–391.
