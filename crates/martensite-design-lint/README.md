# martensite-design-lint

Design-standard linting for Martensite user interfaces — "ESLint for UI
design." The engine replays a `PaintList`'s `PushScope` provenance
markers into a normalized scene tree (hierarchy, geometry, text sizes,
colors), then evaluates evidence-backed rules:

- **WCAG 2.2** — target size, floors a11y can't drop below
- **ANSI/ISA-101** — High-Performance HMI: display hierarchy, color discipline
- **ANSI/ISA-18.2** — alarm-saturation analogs
- **HCI laws** — Hick (choice count), Miller/Cowan (chunks), Fitts (density)
- **Information design** — Tufte data-ink, Few dashboards, Gestalt grouping
- **Perception research** — Miniukovich alignment metrics, Rosenholtz clutter
- **Consistency** — type scale, token discipline

Every finding cites the standard behind it — the report teaches, not
just flags.

## Control model

All standards on by default; everything overridable:

```toml
# design-lint.toml
scale_factor = 2.0
standards = ["wcag", "hci-laws", "consistency"]   # cherry-pick; omit = all
disabled_standards = ["perception"]

[rules.choice-count]
severity = "error"          # off | info | warn | error | forbid
max = 7                     # per-rule thresholds are tunable

[classify]
"MyStatusBar" = "chrome"    # reclassify widget kinds

[[allow]]
path = "App/Media/**"       # scope-path glob; * = segment, ** = depth
rules = ["color-budget"]    # or "all", or "standard:isa-101"
```

Per-element ignores without touching code paths: a widget's
`debug_name` suffix `@lint:rule-id` (or `@lint:all`,
`@lint:standard:wcag`) suppresses that subtree. `@level:1..4` declares
an ISA-101 display level.

Suppressed findings are reported in `LintReport::suppressed` — never
silently dropped — and allows that suppress nothing appear in
`LintReport::unused_allows` so stale ignores self-clean.

## Usage

```rust
use martensite_design_lint::{lint_paint_list, LintConfig};

let report = lint_paint_list(&paint_list, &LintConfig::new());
for f in &report.findings {
    eprintln!("[{}] {} @ {}: {}", f.severity, f.rule, f.path, f.message);
}
eprintln!("{}", report.to_text());
```
