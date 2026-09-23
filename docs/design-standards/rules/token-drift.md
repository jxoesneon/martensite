# token-drift

**Standard(s):** [consistency](../standards/consistency.md)
**Default severity:** `info`
**Confidence:** heuristic — color distance to the palette is exact, but
whether an off-palette color is "drift" or "a deliberate one-off" is a
judgment. `info` severity reflects that; and the rule only runs when
you've declared a palette — without `palette_entries` it is inert.

## What it measures

Compares every painted color in the scene against the declared
palette and flags colors that match no token within tolerance. These
are the hard-coded hex values and one-off literal colors that
bypassed the theme system — drift between what the design system
declares and what the UI actually paints.

The rule is **opt-in by configuration**: no declared palette, no
findings. It cannot false-positive against a palette that doesn't
exist.

## The evidence

- **Design-token discipline** ([consistency](../standards/consistency.md)):
  a theme system's entire value proposition is that rendered output
  uses declared values. Drift is invisible in code review — the
  hard-coded `#3a7bd5` looks fine in the diff — but shows up in the
  painted scene immediately. This rule is the lint analog of "no magic
  numbers."
- **Nielsen #4** (consistency and standards): off-palette colors break
  the implicit contract that the same color means the same thing —
  each drift is a small lie in the visual vocabulary.
- **Maintenance canary**: drift findings cluster where code bypasses
  the theme — often the same places [color-budget](color-budget.md)
  fires, since hard-coded colors tend to be saturated.

## Configuration

The palette is declared as numeric rule params (config params are
`f64`-valued): `palette_entries` plus `palette_0 .. palette_{N-1}` as
packed `0xRRGGBB` integers — TOML hex literals work:

```toml
[rules.token-drift]
severity = "info"
palette_entries = 4
palette_0 = 0x1e1f24
palette_1 = 0x2d6cdf
palette_2 = 0x8a8f98
palette_3 = 0xe6e8eb
tolerance = 24      # per-channel Manhattan distance to nearest token
```

With `palette_entries` unset (0) the rule is registered-but-inert —
no declared palette means nothing to drift from.

## How to fix

- **Find the literal.** The finding names the path and the drifted
  color — grep the widget for the hard-coded value and route it
  through the theme.
- **Widen the palette deliberately** if the color is a real missing
  token — adding a `palette_N` entry makes the fix a declaration, not
  a suppress.
- **Tune `tolerance`** if your pipeline introduces legitimate
  sub-threshold variation (gradients, alpha compositing).

## When it's OK to allow

- **Media/content regions** where painted colors come from user data,
  not the theme — an image viewer's pixels are not token violations.
- **Visualization surfaces** with data-driven color scales.

```toml
[[allow]]
path = "App/ImageViewport/**"
rules = ["token-drift"]
```

Or allow the whole surface while keeping the rule on everywhere else —
token-drift is exactly the rule where "this region is exempt" beats
"lower the severity globally."
