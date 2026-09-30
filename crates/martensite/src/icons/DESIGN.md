# Native Icon Design Contract

Every icon in `martensite::icons::builtin` MUST satisfy this contract.
It codifies the Lucide design language (our canonical stroke idiom),
Material keyline geometry, and Octicons optical-volume practice into
rules a reviewer — human or automated — can check.

## Geometry

- **Canvas**: 24 × 24 units. Strokes centered, so path coordinates
  must stay inside **[2, 22]** (2-unit stroke + 1-unit safe zone).
- **Stroke**: 2 units at 24px, centered, round caps, round joins —
  the `MorphIcon` renderer applies these; authors supply raw `d`.
- **Pixel grid**: all coordinates are integers or, where optically
  required, quarter increments (`.25`/`.5`/`.75`). Arc centers and
  diagonal endpoints align to the grid.
- **Corner radii**: 2 units for elements ≥ 8 units wide; 1 unit for
  smaller elements; sharp only where more than two lines meet.
- **Spacing**: ≥ 2 units of clear space between distinct elements and
  inside shapes (the "2-unit circle" test).

## Visual weight & balance

- Match the optical volume of `circle`/`square` reference glyphs.
  Blur-test: an icon must not read noticeably lighter or darker than
  its siblings.
- Symmetrical icons are geometrically centered; asymmetrical ones may
  be nudged ≤ 0.5 unit for optical centering.
- Density stays low — every element earns its place; recognizable
  first, literal second.

## Path data

- Commands allowed: `M` `L` `C` `Z` `H` `V` `A` (absolute). No `S`/`Q`/`T`,
  relative commands, transforms, or implicit lineto chains.
- Subpaths per icon: keep ≤ 12 (engine limit is checked in tests).
- Related icons reuse the base geometry **unchanged** — variants add
  or replace one modifier element (off-slash, badge, arrow), placed
  consistently (bottom-right for badges, diagonal for off-states).

## Naming

`namespace.kebab-name`. State variants carry the state suffix:
`-off` (suppressed/crossed-out), `-on`, `-open`, `-closed`,
`-selected`, `-alt`. Directional variants: `up|down|left|right`
(+ `up-left` etc.). Paired-state icons are declared in `PAIRS` so
controls can `resolve_pair()` a toggle target.

## Verification

- Every `d` parses via `MorphIcon::icon` (enforced by `icons::tests`).
- A bounds test rejects coordinates outside [2, 22].
- Authors MUST rasterize their icons (see `examples/icon_sheet`) and
  self-inspect the sheet for spacing, weight, and pixel alignment
  before submitting.
