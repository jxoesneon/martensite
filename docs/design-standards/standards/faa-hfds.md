# FAA HFDS / FAA-CT-96-1 — Human Factors Design Standard

**Config key:** `faa-hfds`

The FAA's Human Factors Design Standard (the successor to the earlier
Human Factors Design Guide, FAA-CT-96-1) is the acquisition-standard
companion to NUREG-0700: a quantified checklist for displays that must
work under workload. Where NUREG-0700 counts *packing*, HFDS counts
*characters* and *channels*.

## Why it matters

Three operationalizable rules:

- **Text density** (§8.1.1.3, *screen density*) — a text display's
  character-to-blank ratio should not exceed **60%**. Pure-text screens
  that exceed it read as walls, not fields.
- **Minimal information density / simultaneity** (§8.1.1.2) — present
  only the information essential *at a given time*. The design-time
  analog is a budget on concurrently visible data channels (charts,
  gauges, tables): every additional channel is attention the operator
  must triage.
- **Integrated information** — the counterweight: data needed together
  for a task belongs on one integrated display. A density rule that
  rewards splitting integrated task information has inverted the
  standard's intent — the rules here therefore measure and flag, never
  auto-split.

## Rules that enforce it

- [text-density](../rules/text-density.md) — character-cell coverage
  of alphanumeric-dominant scopes vs. the 60% cap.
- [simultaneous-channels](../rules/simultaneous-channels.md) — count of
  distinct live data displays concurrently visible per surface.

## Design-time proxies

The character:blank ratio is measured as estimated character-cell area
(advance width × em box) over display area — real advance widths when
the paint list recorded them, ~0.5em per character otherwise. The
channel count is a name-based classifier for live data displays
(`Chart`, `Gauge`, `Sparkline`, `Table`, …), topmost only — chart
internals aren't channels. Both findings are `heuristic` confidence.

## External references

- FAA Human Factors Design Standard (HFDS), Federal Aviation
  Administration — the current acquisition standard.
- FAA-CT-96-1, *Human Factors Design Guide* — the predecessor document
  containing the §"minimal information density" and text-density
  guidance.
- MIL-STD-1472G §5.2.2 — display-content essentialism; overlaps HFDS.
