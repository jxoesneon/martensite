# Perception — Clutter & Aesthetic Metrics

**Config key:** `perception`

The psychophysics end of the catalog: computational metrics that
predict how a layout *feels* before anyone reads a word of it — visual
clutter, alignment regularity, edge congestion, grouping quality. Where
[hci-laws](hci-laws.md) measures interaction cost, this standard
measures perception cost: how hard the visual system works just to
parse the screen.

## Why it matters

Two research programs anchor the rules:

- **Miniukovich & De Angeli (CHI 2015)** distilled interface
  screenshots into computable quantities — alignment, density,
  balance, symmetry — and showed they predict users' aesthetic
  judgments. Alignment alone explains roughly half of the variance:
  every distinct sibling edge is a line the eye must trace, and
  scattered edges read instantly as "messy" before any content is
  parsed.
- **Rosenholtz et al.** modeled *clutter* itself: feature-congestion
  and edge-density measures predict how long visual search takes. A
  cluttered display isn't merely ugly — search time scales with it.
  Their work also produced the foundational critique that raw "how
  busy does it look" proxies (like painted-coverage ratios) are cheap
  approximations of true congestion — which is why this crate's
  [edge-density](../rules/edge-density.md) rule is `info`-severity
  heuristic and raster feature-congestion is a future tier.

## Rules that enforce it

- [alignment](../rules/alignment.md) — distinct sibling-edge
  coordinates per container (Miniukovich's alignment metric).
- [whitespace](../rules/whitespace.md) — insufficient inter-element
  spacing; cramped grouping defeats Gestalt proximity.
- [density](../rules/density.md) — control crowding; also a
  [hci-laws](hci-laws.md) rule (Fitts mis-click risk).
- [edge-density](../rules/edge-density.md) — painted-area coverage as
  a first-order Rosenholtz-style clutter proxy.

## External references

- Miniukovich, A. & De Angeli, A. (2015). "Computation of Interface
  Aesthetics." *Proc. CHI 2015*.
  [DOI 10.1145/2702123.2702575](https://doi.org/10.1145/2702123.2702575)
- Rosenholtz, R., Li, Y., & Nakano, L. (2007). "Measuring Visual
  Clutter." *Journal of Vision* 7(2):17.
  [DOI 10.1167/7.2.17](https://doi.org/10.1167/7.2.17)
- Chen et al. (2021). "VizLinter: A Linter and Fixer Framework for
  Data Visualization." *IEEE TVCG* 27(2) — the direct inspiration
  for linting visual output rather than code.
  [DOI 10.1109/TVCG.2020.3030372](https://doi.org/10.1109/TVCG.2020.3030372)
- Gestalt grouping principles (Wertheimer 1923) — the mechanism
  whitespace serves.
