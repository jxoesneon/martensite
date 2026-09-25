# Cookbook 12 — Automated Design Linting & CI Gating

Ensuring that a large application satisfies accessibility regulations (WCAG 2.2 AA/AAA) and industrial
usability standards (ANSI/ISA-101, ISA-18.2 alarm management, Hick/Fitts laws) typically requires tedious,
inconsistent manual visual audits. Small regressions—such as an operator control shrinking below touch
targets or a secondary button dropping below 4.5:1 contrast—easily slip past code reviews.

`martensite-design-lint` brings **compiler-grade static analysis to UI design**. It replays the
widget provenance markers emitted in a `PaintList` into a structured `LintScene` and verifies it
against standards-backed rules.

This recipe demonstrates how to configure `design-lint.toml`, apply inline suppression markers and
semantic annotations, execute design linting in CI pipelines, and run the `autofix` convergence engine.

---

## 1. Goal

Set up automated design verification so that:
1. Every UI view and test fixture is evaluated against WCAG 2.2, ISA-101, and cognitive design standards.
2. The rules and severity thresholds are codified in a version-controlled `design-lint.toml` configuration.
3. Intentional exceptions are cleanly scoped using inline `@lint:` markers and path glob allowances (`[[allow]]`).
4. Alignment, spacing, and contrast issues can be automatically resolved via the `autofix` engine.
5. Continuous Integration (CI) gates pull requests, failing builds on `Warn` or `Forbid` findings while reporting non-blocking `Info` recommendations.

---

## 2. Complete Runnable Pattern

The following pattern illustrates a complete end-to-end design linting workflow. It includes:
1. A production `design-lint.toml` configuration file.
2. An industrial dashboard widget annotated with semantic tags and inline suppressions.
3. A test harness executing the linter and running the automated fix engine.

### Configuration: `design-lint.toml`

```toml
# Project-wide design linting configuration
standards = ["wcag", "isa-101", "hci-laws", "consistency"]
scale_factor = 1.0

# Customize rule thresholds and severities
[rules.text-contrast]
severity = "forbid"         # Text contrast failures cannot be suppressed!

[rules.target-size]
severity = "warn"
min_width = 44.0            # WCAG 2.5.5 touch target size
min_height = 44.0

[rules.reserved-hue]
severity = "forbid"         # Safety: Red is strictly reserved for ISA-18.2 alarms

[rules.density]
severity = "info"

# Scoped path allowances for legacy or specialized panels
[[allow]]
path = "Root/LegacyToolbar/**"
rule = "target-size"
reason = "Legacy high-density desktop toolbar scheduled for refactoring in v1.2"
```

### Rust Implementation: Widget and Verification Harness

```rust
use glam::Vec2;
use kurbo::{Point, Rect as KurboRect, RoundedRect};
use martensite::prelude::*;
use martensite_core::PaintList;
use martensite_design_lint::{
    autofix, lint_paint_list, FixOptions, Finding, LintConfig, LintReport,
    LintScene, Severity, Standard,
};

/// An industrial telemetry panel with semantic annotations and design markers.
pub struct CriticalControlPanel {
    cached_bounds: Rect,
    emergency_stop_pressed: bool,
}

impl CriticalControlPanel {
    pub fn new() -> Self {
        Self {
            cached_bounds: Rect::default(),
            emergency_stop_pressed: false,
        }
    }
}

impl Widget for CriticalControlPanel {
    fn measure(&mut self, cx: &mut LayoutContext, constraints: LayoutConstraints) -> Vec2 {
        let size = Vec2::new(cx.pt(360.0), cx.pt(160.0));
        Vec2::new(
            size.x.clamp(constraints.min_size.x, constraints.max_size.x),
            size.y.clamp(constraints.min_size.y, constraints.max_size.y),
        )
    }

    fn layout(&mut self, _cx: &mut LayoutContext, bounds: Rect) {
        self.cached_bounds = bounds;
    }

    fn paint(&self, cx: &mut PaintContext) {
        let b = cx.bounds;
        let k_rect = KurboRect::new(
            b.min_x() as f64,
            b.min_y() as f64,
            b.max_x() as f64,
            b.max_y() as f64,
        );

        // 1. Declare ISA-101 Display Level 2 (Subsystem Control)
        // Markers in debug_name inform domain-specific rules
        cx.list.push_scope(1, "CriticalControlPanel@level:2", b);

        // Card background (Dark neutral, 60:1 contrast with text)
        cx.list.push_fill_rounded_rect(k_rect, cx.pt(8.0), [24, 26, 30, 255]);
        cx.list.push_stroke_rect(k_rect, [55, 60, 72, 255], cx.pt(1.0));

        // 2. Telemetry KPI Readout (Annotated with @kpi)
        let kpi_bounds = Rect::new(b.origin.x + cx.pt(16.0), b.origin.y + cx.pt(16.0), cx.pt(140.0), cx.pt(48.0));
        cx.list.push_scope(2, "PressureReadout@kpi", kpi_bounds);
        cx.list.push_text(
            Point::new((kpi_bounds.origin.x + cx.pt(8.0)) as f64, (kpi_bounds.origin.y + cx.pt(28.0)) as f64),
            "1,420 PSI".to_string(),
            cx.pt(18.0),
            [235, 240, 250, 255], // WCAG AAA compliant text contrast (> 7:1)
        );
        cx.list.pop_scope();

        // 3. Emergency Stop Button (Annotated with @alarm and @destructive)
        // Red hue is permitted because node lineage contains @alarm
        let btn_bounds = Rect::new(b.origin.x + cx.pt(180.0), b.origin.y + cx.pt(16.0), cx.pt(150.0), cx.pt(48.0));
        let btn_k_rect = KurboRect::new(
            btn_bounds.min_x() as f64,
            btn_bounds.min_y() as f64,
            btn_bounds.max_x() as f64,
            btn_bounds.max_y() as f64,
        );

        cx.list.push_scope(3, "EmergencyStopBtn@alarm@destructive@priority:1", btn_bounds);
        cx.list.push_fill_rounded_rect(btn_k_rect, cx.pt(6.0), [190, 35, 35, 255]); // Saturated Red
        cx.list.push_text(
            Point::new((btn_bounds.origin.x + cx.pt(24.0)) as f64, (btn_bounds.origin.y + cx.pt(30.0)) as f64),
            "E-STOP".to_string(),
            cx.pt(14.0),
            [255, 255, 255, 255],
        );
        cx.list.pop_scope();

        // 4. Compact Diagnostic Toggle (Using inline suppression for known sub-44px target)
        let diag_bounds = Rect::new(b.origin.x + cx.pt(16.0), b.origin.y + cx.pt(100.0), cx.pt(80.0), cx.pt(28.0));
        cx.list.push_scope(4, "DiagToggle@lint:target-size", diag_bounds);
        cx.list.push_fill_rounded_rect(
            KurboRect::new(diag_bounds.min_x() as f64, diag_bounds.min_y() as f64, diag_bounds.max_x() as f64, diag_bounds.max_y() as f64),
            cx.pt(4.0),
            [45, 50, 60, 255],
        );
        cx.list.push_text(
            Point::new((diag_bounds.origin.x + cx.pt(12.0)) as f64, (diag_bounds.origin.y + cx.pt(18.0)) as f64),
            "TRACE".to_string(),
            cx.pt(11.0),
            [200, 210, 220, 255],
        );
        cx.list.pop_scope();

        cx.list.pop_scope(); // Close CriticalControlPanel
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn verify_critical_panel_design_compliance() {
        // 1. Paint widget to capture PaintList with PushScope provenance
        let mut panel = CriticalControlPanel::new();
        let mut arena = WidgetArena::new();
        let id = arena.insert_with_widget(HotNode::default(), Box::new(panel));

        let bounds = Rect::new(0.0, 0.0, 400.0, 200.0);
        let mut paint_cx = PaintContext::new(1.0, bounds);
        if let Some(w) = arena.widget(id) {
            w.paint(&mut paint_cx);
        }

        // 2. Load configuration from TOML
        let toml_str = r#"
            standards = ["wcag", "isa-101"]
            scale_factor = 1.0
            [rules.text-contrast]
            severity = "forbid"
        "#;
        let config = LintConfig::from_toml(toml_str).expect("valid config");

        // 3. Run design linter
        let report = lint_paint_list(&paint_cx.list, &config);

        // 4. Assert zero blocking findings
        // Warning: DiagToggle was suppressed with @lint:target-size
        assert!(
            report.suppressed.iter().any(|f| f.rule == "target-size"),
            "target-size must be reported in suppressed list, never silently dropped"
        );

        // Ensure no active errors or warnings break CI
        let blocking_violations: Vec<&Finding> = report
            .findings
            .iter()
            .filter(|f| f.severity >= Severity::Warn)
            .collect();

        assert!(
            blocking_violations.is_empty(),
            "Design lint must pass with zero blocking violations: {:?}",
            blocking_violations
        );
    }
}
```

---

## 3. Key Architectural Invariants

### 1. Provenance Replay Architecture
`martensite-design-lint` requires **zero app instrumentation or synthetic DOM structures**. It derives all spatial and semantic data directly from the stream of drawing commands:
```
Widget::paint()  ──>  cx.list.push_scope(id, "Button@destructive", bounds)
                 ──>  cx.list.push_fill_rect(...)
                 ──>  cx.list.push_text(...)
                 ──>  cx.list.pop_scope()
                              │
                              ▼
                     [ PaintList Stream ]
                              │
               ┌──────────────┴──────────────┐
               ▼                             ▼
       GPU / TinySkia               martensite-design-lint
     (Vello Compute Shaders)        Replays PushScope/PopScope into
                                    LintScene Tree (Nodes, Fills, Texts)
```
Because the linter evaluates the exact same `PaintList` sent to the GPU, there is zero risk of test-vs-production divergence.

### 2. The Severity Hierarchy & Unsuppressible Forbids
Rules evaluate to one of four severity levels:

$$\text{Off} < \text{Info} < \text{Warn} < \text{Forbid}$$

| Severity | Effect on CI Exit Code | Can be Suppressed via `@lint:`? | Typical Use Case |
|---|:---:|:---:|---|
| **`Off`** | None (0) | N/A | Deactivated rule. |
| **`Info`** | None (0) | Yes | Heuristic recommendations (e.g. whitespace balance, line length). |
| **`Warn`** | **Fails CI (1)** | Yes | Concrete standard violations (touch targets $<44\text{px}$, contrast $<4.5:1$). |
| **`Forbid`** | **Fails CI (1)** | **NO** | Uncompromisable invariants (safety alarms, non-negotiable legal WCAG). |

- **Security & Safety Invariant**: A rule configured as `severity = "forbid"` **cannot** be silenced by inline markers (`@lint:rule-id`) or `[[allow]]` path globs. If a developer attempts to suppress a forbidden finding, the engine flags it as an unsuppressible violation.

### 3. Dual Suppression Seams: Inline Markers vs Path Globs
Martensite provides two distinct mechanisms for managing legitimate exceptions:
1. **Inline Marker Syntax** (`debug_name` suffix):
   - `@lint:target-size`: Suppresses the `target-size` rule for this widget and its descendants.
   - `@lint:all`: Suppresses all rules for this subtree (use sparingly).
   - `@lint:standard:wcag`: Suppresses all rules originating from the WCAG standard.
   - `@level:1..4`: Declares the ANSI/ISA-101 display level (L1 Overview, L2 Control, L3 Detail, L4 Diagnostics).
   - Semantic markers (`@alarm`, `@priority:1`, `@kpi`, `@destructive`): Feed contextual data into domain rules (e.g. `@alarm` authorizes the use of saturated red without triggering `reserved-hue`).
2. **Config Path Globs** (`design-lint.toml`):
   - Scope path matches (e.g. `Root/Header/**/Icon`).
   - Keeps source code free of suppression comments when migrating third-party code.
- **Unused Allow Auditing**: Suppressed findings land in `LintReport::suppressed`. If a path allow in `design-lint.toml` matches zero findings, it is reported in `LintReport::unused_allows`, preventing configuration rot.

### 4. The Autofix Convergence Engine
Findings may provide an automated fix (`LintFix`). Fix operations are categorized into two safety tiers:
- **`Safe` Ops**: Subtle spacing nudges, grid snaps (`SnapGapsToGrid`), and edge alignments (`AlignSiblings`).
- **`Risky` Ops**: Palette recoloring, font size adjustments, or bounding box expansions (gated on `--force`).

The engine runs a convergent feedback loop:
```
Lint Scene ──> Findings ──> Apply Fixes ──> Fingerprint Scene ──> Re-lint
     ▲                                                                │
     └─────────────────────── Iterate until clean ────────────────────┘
```
- **Loop Termination**: The engine fingerprints scene geometry and colors at each pass. If a fingerprint repeats (detecting an oscillating fix where Rule A undoes Rule B), the engine terminates and reports the conflict rather than hanging indefinitely.

---

## 4. Common Pitfalls & Antipatterns

### 1. Mismatched Scale Factor Between Paint and Lint
**Wrong:**
```rust
let mut paint_cx = PaintContext::new(2.0, bounds); // Painted at 2.0x HiDPI
let mut config = LintConfig::new();
config.scale_factor = 1.0; // BAD! Mismatched scale factor
let report = lint_paint_list(&paint_cx.list, &config);
```
**Right:**
```rust
let scale = 2.0;
let mut paint_cx = PaintContext::new(scale, bounds);
let mut config = LintConfig::new();
config.scale_factor = scale; // Correct: Matches rendering scale
let report = lint_paint_list(&paint_cx.list, &config);
```
If the scale factor diverges between painting and linting, all reported font sizes and touch target dimensions will be doubled or halved, generating spurious contrast and target-size failures.

### 2. Using `@lint:all` Blanket Suppressions
**Wrong:**
```rust
cx.list.push_scope(1, "Toolbar@lint:all", bounds); // BAD! Silences everything
```
**Right:**
```rust
cx.list.push_scope(1, "Toolbar@lint:target-size", bounds); // Precise: Suppresses only target-size
```
Blanket suppressions hide accidental contrast regressions, clipped text runs, and missing labels introduced in future commits. Always target specific rule IDs.

### 3. Filling Text Containers with Stroke Tokens
**Wrong:**
```rust
// BAD: Filling a text container with a divider stroke token
cx.list.push_fill_rect(bounds, cx.color(TokenKey::BorderColor, [60, 60, 68, 255]));
cx.list.push_text(origin, "Label", size, [140, 140, 140, 255]); // Fails 4.5:1 contrast!
```
**Right:**
```rust
// Correct: Use RaisedColor for text-hosting surfaces
cx.list.push_fill_rect(bounds, cx.color(TokenKey::RaisedColor, [36, 38, 44, 255]));
cx.list.push_text(origin, "Label", size, cx.color(TokenKey::TextColor, [240, 240, 245, 255]));
```
Stroke tokens (`BorderColor`, `DividerColor`) are calibrated for 3:1 non-text contrast against background surfaces, not for hosting text at 4.5:1. Never fill interactive controls or cards with stroke tokens.

### 4. Running Autofix Without Convergence Limits
In scripts or CI jobs, always supply a bounded `max_depth` (default is 10) in `FixOptions`. Conflicting layout constraints (e.g. minimum width vs maximum container boundary) can otherwise cause layout thrashing.

---

## 5. Integrating with GitHub Actions CI

To gate every pull request on design standards, add this step to `.github/workflows/ci.yml`:

```yaml
jobs:
  design-lint:
    name: Design Standards & A11y Gate
    runs-on: ubuntu-latest
    steps:
      - uses: actions/checkout@v4
      - uses: dtolnay/rust-toolchain@stable

      # 1. Run workspace composite pre-commit check
      - name: Cargo Martensite Check
        run: cargo run -p cargo-martensite -- martensite check --workspace

      # 2. Execute dashboard headless design-lint harness
      - name: Design Lint Verification
        run: cargo test -p industrial_dashboard dump_design_lints -- --nocapture
```

---

## Next Steps

- [Cookbook 10 — Headless Component Testing](10-headless-testing.md)
- [Design Standards Catalog](../design-standards/README.md)
- [ANSI/ISA-101 Standard Specification](../design-standards/standards/isa-101.md)
- [WCAG 2.2 Standard Specification](../design-standards/standards/wcag.md)
