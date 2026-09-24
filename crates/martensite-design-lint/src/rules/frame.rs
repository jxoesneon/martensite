//! Rendered-complexity rules — clutter metrics computed from the
//! paint command stream rather than a rasterized frame.
//!
//! True Rosenholtz feature congestion needs pixels; this module's
//! geometric proxy (paint-op density per unit area) tracks the same
//! signal the scope tree can actually see — how much drawing work a
//! surface packs into its bounds.

use crate::config::LintConfig;
use crate::report::Finding;
use crate::rule::{param, Confidence, LintRule};
use crate::scene::{LintNode, LintScene};
use crate::severity::Severity;
use crate::standard::Standard;

/// This module's rules, appended to the registry by `all_rules()`.
pub(crate) fn rules() -> Vec<&'static dyn LintRule> {
    vec![&PaintComplexity]
}

/// — `paint-complexity`: drawing-op density past the clutter ceiling.
///
/// Counts every fill + text run in a surface's subtree per 10,000px² —
/// the geometric proxy for Rosenholtz feature congestion. A surface
/// that needs 200 paint ops to say what it says is visually loud
/// whether or not any individual element is wrong.
struct PaintComplexity;
impl LintRule for PaintComplexity {
    fn id(&self) -> &'static str {
        "paint-complexity"
    }
    fn standards(&self) -> &'static [Standard] {
        &[Standard::Perception, Standard::InfoDesign]
    }
    fn default_severity(&self) -> Severity {
        Severity::Info
    }
    fn confidence(&self) -> Confidence {
        Confidence::Heuristic
    }
    fn title(&self) -> &'static str {
        "paint-op density past the clutter ceiling"
    }
    fn citation(&self) -> &'static str {
        "Rosenholtz feature congestion (geometric proxy) — visual search \
         time tracks element density, not element quality"
    }
    fn check(&self, scene: &LintScene, cfg: &LintConfig) -> Vec<Finding> {
        // Ops per 10,000 px² — tuned so a calm toolbar (~5 ops/10k)
        // passes and a packed chart grid (>40) flags.
        let max_density = param(cfg, self.id(), "max_ops_per_10k", 40.0);
        let min_area = param(cfg, self.id(), "min_surface_pt", 20_000.0)
            * f64::from(scene.scale_factor).powi(2);
        // Any node over the area floor — paint ops may live on the
        // node itself (a grid-drawing canvas) with zero child scopes.
        // Report the innermost offender: the same op set flagged at
        // every ancestor is one dense region, not N.
        let flagged: Vec<&LintNode> = scene
            .walk()
            .filter(|n| n.area() >= min_area)
            .filter(|n| {
                let ops: usize = n.walk().map(|d| d.fills.len() + d.texts.len()).sum();
                ops as f64 / n.area() * 10_000.0 > max_density
            })
            .collect();
        let mut out = Vec::new();
        for n in &flagged {
            let covered = flagged
                .iter()
                .any(|m| m.path != n.path && m.path.starts_with(&format!("{}/", n.path)));
            if covered {
                continue;
            }
            let area = n.area();
            let ops: usize = n.walk().map(|d| d.fills.len() + d.texts.len()).sum();
            let density = ops as f64 / area * 10_000.0;
            out.push(
                Finding::new(
                    "paint-complexity",
                    &n.path,
                    format!(
                        "{ops} paint ops in {:.0}k px² ({density:.0} per 10k) — \
                         the clutter ceiling is ~{max_density:.0}; visual search \
                         slows with density whether each element is justified or not",
                        area / 1000.0
                    ),
                )
                .at(n.bounds),
            );
        }
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::lint;
    use kurbo::{Point, Rect};
    use martensite_core::{PaintCommand, PaintList};

    fn app_list() -> PaintList {
        let mut list = PaintList::new();
        list.push_scope(None, "App", Rect::new(0.0, 0.0, 800.0, 600.0));
        list.commands.push(PaintCommand::FillRect(
            Rect::new(0.0, 0.0, 800.0, 600.0),
            [0x14, 0x14, 0x18, 255],
        ));
        list
    }

    fn findings_for<'a>(report: &'a crate::LintReport, rule: &str) -> Vec<&'a crate::Finding> {
        report.findings.iter().filter(|f| f.rule == rule).collect()
    }

    #[test]
    fn paint_complexity_flags_dense_surface() {
        let mut list = app_list();
        // 200×100 = 20,000px² panel; 200 fills → 100 ops/10k.
        list.push_scope(None, "Panel", Rect::new(0.0, 0.0, 200.0, 100.0));
        for i in 0..200 {
            let x = (i % 20) as f64 * 10.0;
            let y = (i / 20) as f64 * 10.0;
            list.commands.push(PaintCommand::FillRect(
                Rect::new(x, y, x + 4.0, y + 4.0),
                [100, 100, 110, 255],
            ));
        }
        list.pop_scope();
        list.pop_scope();
        let scene = LintScene::from_paint_list(&list);
        let report = lint(&scene, &LintConfig::new());
        assert!(!findings_for(&report, "paint-complexity").is_empty());
    }

    #[test]
    fn paint_complexity_quiet_on_calm_surface() {
        let mut list = app_list();
        list.push_scope(None, "Panel", Rect::new(0.0, 0.0, 400.0, 300.0));
        list.commands.push(PaintCommand::FillRect(
            Rect::new(0.0, 0.0, 400.0, 300.0),
            [40, 40, 44, 255],
        ));
        list.commands.push(PaintCommand::DrawText(
            Point::new(20.0, 40.0),
            "Title".to_string(),
            14.0,
            [230, 230, 230, 255],
        ));
        list.pop_scope();
        list.pop_scope();
        let scene = LintScene::from_paint_list(&list);
        let report = lint(&scene, &LintConfig::new());
        assert!(findings_for(&report, "paint-complexity").is_empty());
    }
}
