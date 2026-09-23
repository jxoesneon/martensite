# Information Design — Tufte, Few, Gestalt, Nielsen

**Config key:** `info-design`

The editorial-design canon: the accumulated, widely-taught practice of
making information-dense displays legible. It is less quantitative than
[hci-laws](hci-laws.md) but considerably older — the principles come
from cartography, statistical graphics, and newspaper layout, where the
cost of wasted pixels was measured in ink and comprehension failures
were measured in misread data.

## Why it matters

Four strands, all converging on "spend pixels on data":

- **Tufte's data-ink ratio** — the share of ink carrying actual
  information. Non-data ink (decoration, redundant chrome, heavy
  gridlines) should be erased. The lint analog is structural: pixels
  spent on navigation furniture are pixels not spent on content.
- **Few's dashboard canon** — *Information Dashboard Design* catalogs
  how dashboards fail: too many controls competing for one glance,
  decoration masquerading as data, everything highlighted so nothing
  is.
- **Gestalt grouping** — proximity, similarity, common region, and
  whitespace are the *mechanism* by which users perceive structure.
  Cramped, evenly-spaced layouts destroy grouping; whitespace is not
  waste, it is the signal.
- **Nielsen's heuristics** — especially #8, "aesthetic and minimalist
  design": every unit of irrelevant or rarely-needed information
  competes with the relevant units for attention.

## Rules that enforce it

- [choice-count](../rules/choice-count.md) — Few's "one decision
  surface, few choices" principle.
- [chrome-ratio](../rules/chrome-ratio.md) — the data-ink ratio as a
  structural area measurement.
- [whitespace](../rules/whitespace.md) — Gestalt grouping requires
  air; cramped surfaces read as one undifferentiated block.
- [progressive-disclosure](../rules/progressive-disclosure.md) —
  Nielsen #8 made structural: rarely-needed controls belong behind a
  disclosure affordance.

## External references

- Tufte, E. R. (1983/2001). *The Visual Display of Quantitative
  Information* — the data-ink ratio.
- Few, S. (2006). *Information Dashboard Design: The Effective Visual
  Communication of Data*, O'Reilly — esp. ch. 3 on dashboard
  clutter.
- Nielsen, J. (1994). "10 Usability Heuristics for User Interface
  Design." — [nngroup.com/articles/ten-usability-heuristics](https://www.nngroup.com/articles/ten-usability-heuristics/)
- Wertheimer's Gestalt principles of grouping (1923) — proximity,
  similarity, common region; summarized in any modern perception
  text (e.g. Ware, *Information Visualization*).
