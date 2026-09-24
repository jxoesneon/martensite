# text-density

**Standard(s):** [faa-hfds](../standards/faa-hfds.md)
**Default severity:** `warn`
**Confidence:** heuristic — character-cell coverage is estimated
(~0.5em advance per character when no recorded width is available),
and whether a scope is "a text display" is classification.

## What it measures

For scopes whose leaf area is mostly alphanumeric elements, the share
of display area covered by **character cells** — the estimated advance
width × font size per text run, summed, divided by the scope's area.
This is the FAA's character-to-blank ratio, not a line count: 100 short
labels and 100 packed log lines can have equal run counts and wildly
different coverage.

The alphanumeric test is the same leaf-classification the
[packing-density](packing-density.md) caps use — a leaf whose text
cells cover ≥5% of its bounds *and* carries ≥3 estimated characters
counts as an alphanumeric element, so a `Chart` with a caption is a
display element and an icon button is a control element, not text.
Overlapping text runs can push coverage past 100% of the scope area;
the finding reports the measured number.

Only scopes with ≥2 children and area ≥ `min_surface_pt` are
evaluated, and only the **outermost** offender is reported — a dense
child inside a flagged display is the same problem, not two findings.

## The evidence

- **FAA-CT-96-1 / HFDS §8.1.1.3** (screen density): character-to-blank
  ratio ≤60% on text displays. The 0.60 default is the published
  number; the published metric is filled character positions of
  available character spaces — the lint approximates it as cell area
  over surface area.
- **ISO 9241-125** §5.1.4 — density of displayed information.

## Configuration

```toml
[rules.text-density]
severity = "warn"
max = 0.60              # character:blank cap — the FAA number
alnum_share = 0.5       # leaf-area share that makes a scope "text-dominant"
min_surface_pt = 20000  # skip small scopes
```

## How to fix

- **Truncate with intent** — elide, summarize, or sample long runs;
  a log view that keeps the last N lines is denser-readable than one
  that keeps all of them.
- **Columnar layout** — a table of aligned fields beats a wrapped
  paragraph at equal information content.
- **Disclose the tail** — full text behind expand/hover; the standard
  asks for essential information, not abbreviated information.
- Do **not** split integrated task text to satisfy the cap — HFDS's
  integrated-information clause outranks the density number.

## When it's OK to allow

- Terminals, diff viewers, and log tails — surfaces whose contract is
  verbatim text density.
- Read-only content pages (help, about) that are deliberately prose.

```toml
[[allow]]
path = "App/TerminalPane"
rules = ["text-density"]
```
