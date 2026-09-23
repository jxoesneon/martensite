# target-size

**Standard(s):** [wcag](../standards/wcag.md)
**Default severity:** `warn`
**Confidence:** deterministic — a measured bound against a published
numeric floor. This is the most defensible class of finding in the
catalog.

## What it measures

Every interactive-kind node's painted bounds, converted from device
pixels to logical points via the scene's `scale_factor`. Any target
smaller than `min_pt` on either axis is flagged.

The check is per-control, not per-surface: a single 16 pt icon button
in an otherwise fine toolbar is a finding.

## The evidence

- **WCAG 2.2 SC 2.5.8, Target Size (Minimum)**
  ([w3.org/TR/WCAG22/#target-size-minimum](https://www.w3.org/TR/WCAG22/#target-size-minimum)):
  pointer targets must be at least **24×24 CSS pixels**. This is the
  AA floor — the standard documents exceptions (inline text targets,
  spacing-equivalent layouts, targets where size is essential), which
  is why the rule is `error` rather than `forbid`.
- **Fitts's law** (1954, [hci-laws](../standards/hci-laws.md)): pointing
  time and error rate scale with `log₂(1 + D/W)` — small targets are
  slower *and* missed more, and motor-impaired users miss them
  disproportionately.
- **Higher-touch floors**: Apple HIG specifies 44×44 pt, Material 48×48
  dp (the WCAG AAA SC 2.5.5 territory). For touch-first products, raise
  `min_pt` accordingly — 24 pt is a floor, not a target.

**Why `scale_factor` matters**: a 24 pt target is 48 device px at 2×.
Lint at the same scale factor you paint at, or the rule measures the
wrong thing.

## Configuration

```toml
[rules.target-size]
severity = "warn"
min_pt = 24    # WCAG 2.2 SC 2.5.8 floor; 44 for touch-first products
```

## How to fix

- **Grow the hit target, not necessarily the visual.** The standard
  measures the *interactive* region — padding and slop count. An icon
  can stay 16 pt if its clickable bounds are 24 pt.
- **Space-out fallback.** SC 2.5.8's spacing exception: undersized
  targets separated by enough empty space from other targets can pass
  — but growing the target is the better fix.
- **Check classification.** A flagged `Badge`/`Indicator` that only
  *looks* interactive (name contains "button"-like substrings) may be
  misclassified — fix `[classify]`.

## When it's OK to allow

The standard's own exceptions are the legitimate cases:

- **Inline text links** inside a paragraph — the inline exception.
- **Dense data grids** where row/cell size is essential to the
  information density.
- **Spacing-equivalent** layouts — documented undersize with
  surrounding clearance.

```toml
[[allow]]
path = "App/DocView/**"   # inline links inside prose
rules = ["target-size"]
```

Because the default is `error`, prefer targeted `[[allow]]`s over
lowering severity — and remember `forbid` is available if your product
treats unhittable targets as non-negotiable.
