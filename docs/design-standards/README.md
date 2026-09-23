# Martensite Design Standards

`martensite-design-lint` is **ESLint for UI design**. It replays a
`PaintList`'s `PushScope { id, name, bounds }` provenance markers into a
scene tree — every widget scope becomes a node with a name, a kind,
bounds, painted colors, and text sizes — and evaluates a catalog of
standards-backed rules over that tree. No app instrumentation is
required: the paint list your widgets already emit *is* the input, so
the same check runs against live frames, headless tests, and golden
fixtures.

Every finding carries:

- the **rule id** (e.g. `nav-depth`),
- a **citation** naming the standard or research behind the threshold,
- the **standard(s)** it enforces,
- a **confidence** tier — `deterministic` (a measured fact against a
  stated threshold) or `heuristic` (a review prompt, not a verdict),
- a **doc reference** of the form `{docs_base}/rules/{rule-id}.md` —
  these pages. `docs_base` defaults to `docs/design-standards` and may
  be a URL, so findings in CI can link straight to the fix guide.

## The evidence base

Rules cite real bodies of guidance, grouped into seven selectable
standards:

| Key           | Standard                                             | Rules enforcing it |
| ------------- | ---------------------------------------------------- | ------------------ |
| `wcag`        | [WCAG 2.2 accessibility floors](standards/wcag.md)   | [target-size](rules/target-size.md), [alert-saturation](rules/alert-saturation.md), [color-only-info](rules/color-only-info.md) |
| `isa-101`     | [ANSI/ISA-101 High-Performance HMI](standards/isa-101.md) | [nav-depth](rules/nav-depth.md), [chrome-ratio](rules/chrome-ratio.md), [color-budget](rules/color-budget.md), [color-only-info](rules/color-only-info.md), [progressive-disclosure](rules/progressive-disclosure.md) |
| `isa-18-2`    | [ANSI/ISA-18.2 alarm management](standards/isa-18-2.md) | [alert-saturation](rules/alert-saturation.md) |
| `hci-laws`    | [Hick, Miller/Cowan, Fitts](standards/hci-laws.md)   | [nav-depth](rules/nav-depth.md), [choice-count](rules/choice-count.md), [density](rules/density.md) |
| `info-design` | [Tufte, Few, Gestalt, Nielsen](standards/info-design.md) | [choice-count](rules/choice-count.md), [chrome-ratio](rules/chrome-ratio.md), [whitespace](rules/whitespace.md), [progressive-disclosure](rules/progressive-disclosure.md) |
| `perception`  | [Miniukovich, Rosenholtz clutter metrics](standards/perception.md) | [alignment](rules/alignment.md), [whitespace](rules/whitespace.md), [density](rules/density.md), [edge-density](rules/edge-density.md) |
| `consistency` | [Design-system & type discipline](standards/consistency.md) | [alignment](rules/alignment.md), [type-scale](rules/type-scale.md), [color-budget](rules/color-budget.md), [token-drift](rules/token-drift.md) |

A rule is active when **at least one** of its cited standards is
enabled.

## Rule catalog

| Rule | Default severity | Confidence | One-line summary |
| ---- | ---------------- | ---------- | ---------------- |
| [nav-depth](rules/nav-depth.md) | warn | deterministic | stacked navigation/orientation layers per surface |
| [choice-count](rules/choice-count.md) | warn | heuristic | simultaneous choices in one decision surface |
| [chrome-ratio](rules/chrome-ratio.md) | warn | heuristic | share of surface area spent on chrome, not content |
| [alignment](rules/alignment.md) | info | heuristic | sibling edge alignment regularity |
| [target-size](rules/target-size.md) | warn | deterministic | interactive elements below the WCAG minimum target size |
| [type-scale](rules/type-scale.md) | info | heuristic | distinct font sizes per surface |
| [whitespace](rules/whitespace.md) | info | heuristic | insufficient spacing / cramped grouping |
| [alert-saturation](rules/alert-saturation.md) | warn | heuristic | simultaneous alert-level signals in one view |
| [color-budget](rules/color-budget.md) | warn | heuristic | distinct saturated hues competing for attention |
| [density](rules/density.md) | info | heuristic | interactive controls per unit surface area |
| [token-drift](rules/token-drift.md) | info | heuristic | painted colors outside the declared palette |
| [edge-density](rules/edge-density.md) | info | heuristic | painted coverage as a clutter proxy |
| [color-only-info](rules/color-only-info.md) | warn | heuristic | state possibly conveyed by color alone |
| [progressive-disclosure](rules/progressive-disclosure.md) | info | heuristic | many controls and no disclosure affordance |

## The control model

Everything is on by default; every layer below composes.

### Severity

Each rule resolves to one of five levels:

| Severity | Meaning |
| -------- | ------- |
| `off`    | The rule produces no findings. |
| `info`   | Awareness only — never fails a build. |
| `warn`   | A warning — the default for guidance rules. |
| `error`  | Intended for CI gating. |
| `forbid` | An error that **no allow can suppress** — neither `[[allow]]` nor inline `@lint:` markers. Use sparingly, for rules where a legitimate exception does not exist. |

A finding records both its *intrinsic* (evidence-backed default) and
*effective* (configured) severity, so overrides stay visible in the
report.

### Standard selection

- `standards = ["wcag", "hci-laws"]` — cherry-pick: only rules citing
  these standards run.
- `disabled_standards = ["perception"]` — subtract from the full set.

Omit both and all seven standards are enabled.

### Per-rule configuration

`[rules.<id>]` tables take `severity` plus arbitrary numeric parameters
the rule documents (each rule page lists its keys and defaults):

```toml
[rules.choice-count]
severity = "error"   # raise the stakes
max = 5              # tighten the Hick's-law budget
```

### Name classification

Rules infer each node's *kind* (`navigation`, `interactive`, `content`,
`container`, `chrome`) from the last segment of its `debug_name`
(`Tabs` → navigation, `Button` → interactive, …). When a name fools
the heuristic, `[classify]` overrides it:

```toml
[classify]
"Dashboard" = "content"      # it's data, not a grouping panel
"RibbonBar" = "navigation"
```

### Path allows

`[[allow]]` suppresses findings under a scope-path glob. Paths are
`/`-joined widget names (`App/ZonePanel/Tabs`). In the glob, `*`
matches within one path segment and `**` matches any depth. `rules`
accepts rule ids, `"all"`, and `"standard:<key>"`:

```toml
[[allow]]
path = "App/MediaZone/**"                    # the whole media subtree
rules = ["color-budget", "standard:isa-101"] # this rule + the whole standard

[[allow]]
path = "App/*/StatusStrip"                   # one segment wildcard
rules = ["alignment"]
```

An allow matches a finding when the glob matches the finding's path
**or any ancestor prefix** — `App/Media/**` covers
`App/Media/Grid/Button`.

### Inline markers

Append markers to a widget's `debug_name` — no code structure changes
needed:

- `"Toolbar@lint:choice-count"` — suppress one rule for this subtree
- `"Panel@lint:color-budget,nav-depth"` — comma-separate several
- `"Media@lint:all"` — suppress everything below
- `"Hmi@lint:standard:isa-101"` — suppress a whole standard
- `"OverviewPanel@level:2"` — declare an ISA-101 display level
  (`@level:N`, N = 1–4)

Markers are **inherited by the whole subtree** (the `tools:ignore`
model): an `@lint:` on a container covers all its descendants. Markers
combine freely: `"Panel@level:2@lint:color-budget"`.

### Other top-level keys

- `docs_base` — where finding doc links point. Defaults to
  `docs/design-standards`; set it to a URL (e.g.
  `https://example.com/design-standards`) so report links resolve on
  the web.
- `scale_factor` — display scale (device px per logical pt). Rules with
  pt-denominated thresholds — `target-size`'s 24 pt WCAG floor, type
  sizes — convert through it. Lint at the same scale factor you paint
  at, or measurements drift by that factor.
- `[rules.token-drift]` palette params — `palette_entries` plus
  `palette_0 .. palette_{N-1}` packed `0xRRGGBB` integers declare the
  color palette and activate [token-drift](rules/token-drift.md).

## Suppression is auditable, never silent

The report has three buckets:

- `findings` — active findings after suppression.
- `suppressed` — findings an allow caught. **Shown, not dropped**, each
  with `suppressed_by` naming the `[[allow]]` or `@lint:` marker that
  caught it. Suppressed rules that quietly swallow real problems are
  how lint systems lose trust.
- `unused_allows` — allows that suppressed *nothing*. Stale ignores
  self-report (rustc `#[expect]` / ESLint
  `reportUnusedDisableDirectives` semantics) so they get cleaned up
  instead of rotting in place.

`forbid`-severity findings bypass suppression entirely.

## Complete example: `design-lint.toml`

```toml
# design-lint.toml — annotated reference for every key.

# Display scale: 2.0 on a Retina/HiDPI target, 1.0 for standard
# density. Pt-denominated thresholds (target-size) convert through it.
scale_factor = 2.0

# Where finding doc links point. A repo-relative path or a URL.
docs_base = "docs/design-standards"

# Cherry-pick standards — omit for all seven. This industrial product
# wants the HMI and accessibility canon but not perception metrics.
standards = ["wcag", "isa-101", "isa-18-2", "hci-laws", "info-design", "consistency"]
# Alternatively, subtract from the full set:
# disabled_standards = ["perception"]

# The design-token palette — activates `token-drift`. Painted colors
# outside this set (beyond `tolerance`) are reported. Params are
# numeric, so the palette is packed 0xRRGGBB entries.
[rules.token-drift]
palette_entries = 4
palette_0 = 0x1e1f24
palette_1 = 0x2d6cdf
palette_2 = 0x8a8f98
palette_3 = 0xe6e8eb

# Per-rule severity + thresholds.
[rules.target-size]
severity = "error"   # WCAG floor — gate CI on it
min_pt = 24          # WCAG 2.2 SC 2.5.8; raise to 44 for touch-first

[rules.choice-count]
severity = "warn"
max = 7              # Hick/Miller classic bound; 4 is the Cowan strict bound

[rules.chrome-ratio]
max = 0.45           # >45% chrome means the frame outweighs the content

[rules.nav-depth]
severity = "error"   # stacked nav is a hard fail in this product
max = 2

[rules.density]
max = 0.6            # controls per 10,000 pt²

# The name heuristics misclassify these widgets — correct them.
[classify]
"Dashboard" = "content"     # a data surface, not a container
"RailStrip" = "navigation"  # project-specific nav widget

# The media wall legitimately uses saturated video colors — the
# alarm-channel budget does not apply there.
[[allow]]
path = "App/MediaZone/**"
rules = ["color-budget", "standard:isa-101"]

# Dense data grid: the density default is wrong for it by design.
[[allow]]
path = "App/ZonePanel/GridView"
rules = ["density", "choice-count"]
```

## Related documentation

- `standards/` — one page per standard: what it is, why it matters,
  which rules enforce it, and the primary citations.
- `rules/` — one page per rule: what it measures, the evidence and
  threshold origin, configuration, fixes, and legitimate exceptions.
