//! Extended consistency rules — the mechanical-uniformity checks a
//! design system exists to enforce: spacing grids, sibling geometry,
//! tab/menu budgets, minimum content allocation, and edge padding.

use crate::config::LintConfig;
use crate::fix::{FixOp, FixSafety, LintFix};
use crate::report::Finding;
use crate::rule::{param, Confidence, LintRule};
use crate::rules::{kind_of, sibling_gaps};
use crate::scene::{LintNode, LintScene, NodeKind};
use crate::severity::Severity;
use crate::standard::Standard;

/// This module's rules, appended to the registry by `all_rules()`.
pub(crate) fn rules() -> Vec<&'static dyn LintRule> {
    vec![
        &SpacingToken,
        &SiblingVariance,
        &TabCount,
        &MinSurface,
        &MenuBreadth,
        &MenuDepth,
        &EdgeTouch,
    ]
}

/// — `spacing-token`: sibling gaps off the spacing grid.
///
/// Design systems quantize spacing (4pt base is the industry norm)
/// so layout arithmetic stays predictable — every off-grid gap is a
/// one-off that future edits can't reason about. Complements
/// `token-drift` (color/radius literals) on the spacing axis.
struct SpacingToken;
impl LintRule for SpacingToken {
    fn id(&self) -> &'static str {
        "spacing-token"
    }
    fn standards(&self) -> &'static [Standard] {
        &[Standard::Consistency, Standard::InfoDesign]
    }
    fn default_severity(&self) -> Severity {
        Severity::Info
    }
    fn confidence(&self) -> Confidence {
        Confidence::Heuristic
    }
    fn title(&self) -> &'static str {
        "sibling gaps off the spacing grid"
    }
    fn citation(&self) -> &'static str {
        "design-token practice — quantized spacing keeps layout math \
         composable; off-grid gaps are unmaintainable one-offs"
    }
    fn check(&self, scene: &LintScene, cfg: &LintConfig) -> Vec<Finding> {
        // Floor of 1px — a zero/negative grid would silently disable
        // the rule via NaN modulo.
        let grid = (param(cfg, self.id(), "grid_pt", 4.0) * f64::from(scene.scale_factor)).max(1.0);
        let tolerance = param(cfg, self.id(), "tolerance_px", 0.75);
        let min_gap = param(cfg, self.id(), "min_gap_px", 0.5);
        let mut out = Vec::new();
        for n in scene.walk() {
            if n.children.len() < 2 {
                continue;
            }
            let (gaps, horizontal) = sibling_gaps(n);
            let off: Vec<f64> = gaps
                .iter()
                .copied()
                .filter(|g| {
                    // Distance to the NEAREST multiple — a gap of 7.9px
                    // on a 4px grid is 0.1 off (remainder 3.9 ≈ grid).
                    let r = g.rem_euclid(grid);
                    *g > min_gap && r.min(grid - r) > tolerance
                })
                .collect();
            if off.is_empty() {
                continue;
            }
            out.push(
                Finding::new(
                    "spacing-token",
                    &n.path,
                    format!(
                        "{} gap(s) off the {:.0}pt grid ({}) — tokenized spacing is what \
                         keeps future layout edits predictable",
                        off.len(),
                        grid / f64::from(scene.scale_factor),
                        off.iter()
                            .map(|g| format!("{g:.0}px"))
                            .collect::<Vec<_>>()
                            .join(", ")
                    ),
                )
                .at(n.bounds)
                .with_fix(LintFix::new(
                    format!(
                        "snap gaps to the {:.0}pt grid ({})",
                        grid / f64::from(scene.scale_factor),
                        if horizontal { "horizontal" } else { "vertical" }
                    ),
                    FixSafety::Safe,
                    FixOp::SnapGapsToGrid {
                        parent: n.path.clone(),
                        grid,
                    },
                )),
            );
        }
        out
    }
}

/// — `sibling-variance`: same-kind siblings with divergent geometry.
///
/// Two `Card`s in one strip that differ 30% in height read as a bug,
/// not a choice — same-role elements carry an implicit "we're
/// equivalent" contract. Wildly divergent sibling geometry breaks
/// that contract.
struct SiblingVariance;
impl LintRule for SiblingVariance {
    fn id(&self) -> &'static str {
        "sibling-variance"
    }
    fn standards(&self) -> &'static [Standard] {
        &[Standard::Consistency, Standard::InfoDesign]
    }
    fn default_severity(&self) -> Severity {
        Severity::Info
    }
    fn confidence(&self) -> Confidence {
        Confidence::Heuristic
    }
    fn title(&self) -> &'static str {
        "same-name siblings with divergent geometry"
    }
    fn citation(&self) -> &'static str {
        "regularity (Miniukovich & De Angeli) — same-role elements set \
         an equivalence expectation; variance reads as defect"
    }
    fn check(&self, scene: &LintScene, cfg: &LintConfig) -> Vec<Finding> {
        let max_ratio = param(cfg, self.id(), "max_ratio", 1.5);
        let mut out = Vec::new();
        for n in scene.walk() {
            // Group children by name — same-name siblings are the
            // equivalence set.
            let mut groups: std::collections::BTreeMap<&str, Vec<&LintNode>> =
                std::collections::BTreeMap::new();
            for c in &n.children {
                if c.area() > 1.0 {
                    groups.entry(c.name.as_str()).or_default().push(c);
                }
            }
            for (name, group) in groups {
                if group.len() < 2 {
                    continue;
                }
                let w_min = group
                    .iter()
                    .map(|c| c.bounds.width())
                    .fold(f64::INFINITY, f64::min);
                let w_max = group.iter().map(|c| c.bounds.width()).fold(0.0, f64::max);
                let h_min = group
                    .iter()
                    .map(|c| c.bounds.height())
                    .fold(f64::INFINITY, f64::min);
                let h_max = group.iter().map(|c| c.bounds.height()).fold(0.0, f64::max);
                let worst = (w_max / w_min.max(1.0)).max(h_max / h_min.max(1.0));
                if worst > max_ratio {
                    out.push(
                        Finding::new(
                            "sibling-variance",
                            &n.path,
                            format!(
                                "{} `{name}` siblings differ {:.1}× in size — equal-role \
                                 elements at unequal geometry read as a layout bug",
                                group.len(),
                                worst
                            ),
                        )
                        .at(n.bounds),
                    );
                }
            }
        }
        out
    }
}

/// — `tab-count`: a tab strip past the scanning budget.
///
/// Tabs are a scan-and-select surface; past ~7 entries the strip
/// stops being scannable and becomes an overflow menu wearing tabs'
/// clothes — the same Hick's-law budget as `choice-count`, applied
/// where tabs are the specific idiom.
struct TabCount;
impl LintRule for TabCount {
    fn id(&self) -> &'static str {
        "tab-count"
    }
    fn standards(&self) -> &'static [Standard] {
        &[Standard::Consistency, Standard::HciLaws]
    }
    fn default_severity(&self) -> Severity {
        Severity::Warn
    }
    fn title(&self) -> &'static str {
        "tab strip past the scanning budget"
    }
    fn citation(&self) -> &'static str {
        "Hick's law on a fixed idiom — tabs are scan targets; >7 is a \
         menu pretending to be tabs"
    }
    fn check(&self, scene: &LintScene, cfg: &LintConfig) -> Vec<Finding> {
        let max = param(cfg, self.id(), "max_tabs", 7.0) as usize;
        let mut out = Vec::new();
        for n in scene.walk() {
            // "tab" — but not "table"/"tabular" (grid children aren't
            // a tab strip).
            let tabs: Vec<&LintNode> = n
                .children
                .iter()
                .filter(|c| {
                    let l = c.name.to_ascii_lowercase();
                    l.contains("tab") && !l.contains("table") && !l.contains("tabular")
                })
                .collect();
            if tabs.len() > max {
                out.push(
                    Finding::new(
                        "tab-count",
                        &n.path,
                        format!(
                            "{} tabs in one strip — past ~{max} the strip stops being \
                             scannable; group or overflow the tail",
                            tabs.len()
                        ),
                    )
                    .at(n.bounds),
                );
            }
        }
        out
    }
}

/// — `min-surface`: a content zone below a usable minimum.
///
/// A surface small enough to fit no meaningful content is either
/// dead allocation or content crammed past legibility — both are the
/// layout telling you the allocation is wrong.
struct MinSurface;
impl LintRule for MinSurface {
    fn id(&self) -> &'static str {
        "min-surface"
    }
    fn standards(&self) -> &'static [Standard] {
        &[Standard::Consistency, Standard::InfoDesign]
    }
    fn default_severity(&self) -> Severity {
        Severity::Info
    }
    fn confidence(&self) -> Confidence {
        Confidence::Heuristic
    }
    fn title(&self) -> &'static str {
        "content zone below a usable minimum"
    }
    fn citation(&self) -> &'static str {
        "surface allocation floor — a zone too small for its role is \
         dead space or crammed content"
    }
    fn check(&self, scene: &LintScene, cfg: &LintConfig) -> Vec<Finding> {
        let min_pt = param(cfg, self.id(), "min_pt", 48.0);
        let min_px = min_pt * f64::from(scene.scale_factor);
        let mut out = Vec::new();
        // Only flag content-bearing kinds — a 6px Divider is fine.
        for n in scene.walk() {
            if kind_of(n, cfg) != NodeKind::Container {
                continue;
            }
            let (w, h) = (n.bounds.width(), n.bounds.height());
            if (w < min_px || h < min_px) && w > 1.0 && h > 1.0 {
                out.push(
                    Finding::new(
                        "min-surface",
                        &n.path,
                        format!(
                            "surface `{}` is {:.0}×{:.0}pt — below the {min_pt:.0}pt \
                             floor for a content zone",
                            n.name,
                            w / f64::from(scene.scale_factor),
                            h / f64::from(scene.scale_factor)
                        ),
                    )
                    .at(n.bounds)
                    .with_fix(LintFix::new(
                        format!("grow the surface to ≥{min_pt:.0}pt"),
                        FixSafety::Risky,
                        FixOp::GrowBounds {
                            path: n.path.clone(),
                            min_w: min_px,
                            min_h: min_px,
                        },
                    )),
                );
            }
        }
        out
    }
}

/// — `menu-breadth`: a menu past the working-memory budget.
///
/// Flat menus over ~7 items force serial scanning — Hick's law
/// applied to the menu idiom specifically, where the fix (grouping,
/// sectioning, submenus) is known and cheap.
struct MenuBreadth;
impl LintRule for MenuBreadth {
    fn id(&self) -> &'static str {
        "menu-breadth"
    }
    fn standards(&self) -> &'static [Standard] {
        &[Standard::HciLaws, Standard::Consistency]
    }
    fn default_severity(&self) -> Severity {
        Severity::Warn
    }
    fn title(&self) -> &'static str {
        "menu past the flat-scan budget"
    }
    fn citation(&self) -> &'static str {
        "Hick's law — menu scan time grows with item count; group or \
         cascade past ~7"
    }
    fn check(&self, scene: &LintScene, cfg: &LintConfig) -> Vec<Finding> {
        let max = param(cfg, self.id(), "max_items", 7.0) as usize;
        let mut out = Vec::new();
        for n in scene.walk() {
            let lower = n.name.to_ascii_lowercase();
            if !(lower.contains("menu") || lower.contains("dropdown")) {
                continue;
            }
            // Meaningful items only — separators and spacers have no
            // label and aren't interactive.
            let items = n
                .children
                .iter()
                .filter(|c| {
                    kind_of(c, cfg) == NodeKind::Interactive
                        || c.walk().any(|d| !d.texts.is_empty())
                })
                .count();
            if items > max {
                out.push(
                    Finding::new(
                        "menu-breadth",
                        &n.path,
                        format!(
                            "{items} items in one menu — flat lists past ~{max} force \
                             serial scanning; section or cascade"
                        ),
                    )
                    .at(n.bounds),
                );
            }
        }
        out
    }
}

/// — `menu-depth`: cascades past the second level.
///
/// Each submenu level multiplies the steering cost (Fitts's law on a
/// narrowing corridor) and the chance of accidental dismissal — the
/// "menu tunnel" problem. Two levels is the practical ceiling.
struct MenuDepth;
impl LintRule for MenuDepth {
    fn id(&self) -> &'static str {
        "menu-depth"
    }
    fn standards(&self) -> &'static [Standard] {
        &[Standard::HciLaws, Standard::Consistency]
    }
    fn default_severity(&self) -> Severity {
        Severity::Warn
    }
    fn title(&self) -> &'static str {
        "menu cascade deeper than two levels"
    }
    fn citation(&self) -> &'static str {
        "Fitts's law on nested corridors — each cascade level narrows \
         the steering tunnel and raises dismissal risk"
    }
    fn check(&self, scene: &LintScene, cfg: &LintConfig) -> Vec<Finding> {
        let max_depth = param(cfg, self.id(), "max_depth", 2.0) as usize;
        // A menu node = a menu container, not a menuitem leaf.
        let is_menu = |n: &LintNode| {
            let l = n.name.to_ascii_lowercase();
            (l.contains("menu") || l.contains("submenu"))
                && !l.contains("menuitem")
                && !l.contains("menubar")
        };
        // Menu-chain depth below `node` — non-menu wrappers
        // (a MenuItem holding a Submenu) pass through transparently.
        fn chain(node: &LintNode, is_menu: &dyn Fn(&LintNode) -> bool) -> usize {
            node.children
                .iter()
                .map(|c| {
                    if is_menu(c) {
                        1 + chain(c, is_menu)
                    } else {
                        chain(c, is_menu)
                    }
                })
                .max()
                .unwrap_or(0)
        }
        let mut out = Vec::new();
        for n in scene.walk() {
            if !is_menu(n) {
                continue;
            }
            let depth = 1 + chain(n, &is_menu);
            if depth > max_depth {
                // Report the outermost offender only — every menu in
                // an over-deep chain would flag the same cascade.
                let ancestor_is_menu = {
                    let by_path = scene.by_path();
                    let mut path = n.path.as_str();
                    let mut found = false;
                    while let Some(i) = path.rfind('/') {
                        path = &path[..i];
                        if let Some(a) = by_path.get(path) {
                            if is_menu(a) {
                                found = true;
                                break;
                            }
                        }
                    }
                    found
                };
                if ancestor_is_menu {
                    continue;
                }
                out.push(
                    Finding::new(
                        "menu-depth",
                        &n.path,
                        format!(
                            "menu cascade runs {depth} levels deep — past {max_depth} the \
                             steering tunnel collapses"
                        ),
                    )
                    .at(n.bounds),
                );
            }
        }
        out
    }
}

/// — `edge-touch`: content flush against its container edge.
///
/// Content that touches the container boundary reads as cramped
/// even when technically inside — the padding edge is the visual
/// "breathing room" contract every container implicitly makes.
struct EdgeTouch;
impl LintRule for EdgeTouch {
    fn id(&self) -> &'static str {
        "edge-touch"
    }
    fn standards(&self) -> &'static [Standard] {
        &[Standard::Consistency, Standard::InfoDesign]
    }
    fn default_severity(&self) -> Severity {
        Severity::Info
    }
    fn confidence(&self) -> Confidence {
        Confidence::Heuristic
    }
    fn title(&self) -> &'static str {
        "content flush against container edges"
    }
    fn citation(&self) -> &'static str {
        "Gestalt common-region — padding is the boundary cue; \
         edge-to-edge content reads as clipped"
    }
    fn check(&self, scene: &LintScene, cfg: &LintConfig) -> Vec<Finding> {
        let min_pad = param(cfg, self.id(), "min_pad_pt", 4.0) * f64::from(scene.scale_factor);
        let tolerance = param(cfg, self.id(), "tolerance_px", 1.0);
        let min_area = param(cfg, self.id(), "min_surface_pt", 20_000.0)
            * f64::from(scene.scale_factor).powi(2);
        let mut out = Vec::new();
        for n in scene
            .walk()
            .filter(|n| !n.children.is_empty() && n.area() >= min_area)
        {
            let mut touching = 0usize;
            for c in &n.children {
                if c.area() <= 1.0 {
                    continue;
                }
                // Flush = within tolerance of the edge AND less than
                // min_pad — a flush edge with real padding elsewhere
                // is intentional bleed; flush everywhere is cramped.
                let gaps = [
                    c.bounds.x0 - n.bounds.x0,
                    n.bounds.x1 - c.bounds.x1,
                    c.bounds.y0 - n.bounds.y0,
                    n.bounds.y1 - c.bounds.y1,
                ];
                let flush_edges = gaps.iter().filter(|g| g.abs() <= tolerance).count();
                // Protruding children (gap < -tolerance) are overflow's
                // job, not cramped padding — exclude them here.
                if flush_edges >= 2 && gaps.iter().all(|g| *g >= -tolerance && *g < min_pad) {
                    touching += 1;
                }
            }
            if touching > 0 {
                out.push(
                    Finding::new(
                        "edge-touch",
                        &n.path,
                        format!(
                            "{touching} child(ren) run flush to the container edge with \
                             no padding — the boundary reads as clipped, not contained"
                        ),
                    )
                    .at(n.bounds),
                );
            }
        }
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::lint;
    use kurbo::Rect;
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
    fn registry_exports_all() {
        assert_eq!(rules().len(), 7);
    }

    // ---------- spacing-token ----------

    #[test]
    fn spacing_token_flags_off_grid_gaps() {
        let mut list = app_list();
        list.push_scope(None, "Panel", Rect::new(0.0, 0.0, 400.0, 60.0));
        // Gaps of 13px and 7px — off the 4pt grid at scale 1.
        list.push_scope(None, "A", Rect::new(0.0, 10.0, 100.0, 50.0));
        list.pop_scope();
        list.push_scope(None, "B", Rect::new(113.0, 10.0, 213.0, 50.0));
        list.pop_scope();
        list.push_scope(None, "C", Rect::new(220.0, 10.0, 320.0, 50.0));
        list.pop_scope();
        list.pop_scope();
        list.pop_scope();
        let mut scene = LintScene::from_paint_list(&list);
        scene.scale_factor = 1.0;
        let report = lint(&scene, &LintConfig::new());
        let hits = findings_for(&report, "spacing-token");
        assert_eq!(hits.len(), 1, "expected one finding, got {hits:?}");
        assert!(hits[0].fix.is_some());
    }

    #[test]
    fn spacing_token_quiet_on_grid() {
        let mut list = app_list();
        list.push_scope(None, "Panel", Rect::new(0.0, 0.0, 400.0, 60.0));
        list.push_scope(None, "A", Rect::new(0.0, 10.0, 100.0, 50.0));
        list.pop_scope();
        list.push_scope(None, "B", Rect::new(108.0, 10.0, 208.0, 50.0));
        list.pop_scope();
        list.push_scope(None, "C", Rect::new(216.0, 10.0, 316.0, 50.0));
        list.pop_scope();
        list.pop_scope();
        list.pop_scope();
        let mut scene = LintScene::from_paint_list(&list);
        scene.scale_factor = 1.0;
        let report = lint(&scene, &LintConfig::new());
        assert!(findings_for(&report, "spacing-token").is_empty());
    }

    // ---------- sibling-variance ----------

    #[test]
    fn sibling_variance_flags_mismatched_cards() {
        let mut list = app_list();
        list.push_scope(None, "Panel", Rect::new(0.0, 0.0, 600.0, 200.0));
        list.push_scope(None, "Card", Rect::new(0.0, 0.0, 100.0, 100.0));
        list.pop_scope();
        list.push_scope(None, "Card", Rect::new(120.0, 0.0, 420.0, 180.0));
        list.pop_scope();
        list.pop_scope();
        list.pop_scope();
        let scene = LintScene::from_paint_list(&list);
        let report = lint(&scene, &LintConfig::new());
        assert!(!findings_for(&report, "sibling-variance").is_empty());
    }

    #[test]
    fn sibling_variance_quiet_on_uniform() {
        let mut list = app_list();
        list.push_scope(None, "Panel", Rect::new(0.0, 0.0, 600.0, 200.0));
        list.push_scope(None, "Card", Rect::new(0.0, 0.0, 100.0, 100.0));
        list.pop_scope();
        list.push_scope(None, "Card", Rect::new(120.0, 0.0, 220.0, 100.0));
        list.pop_scope();
        list.pop_scope();
        list.pop_scope();
        let scene = LintScene::from_paint_list(&list);
        let report = lint(&scene, &LintConfig::new());
        assert!(findings_for(&report, "sibling-variance").is_empty());
    }

    // ---------- tab-count ----------

    #[test]
    fn tab_count_flags_overfull_strip() {
        let mut list = app_list();
        list.push_scope(None, "TabStrip", Rect::new(0.0, 0.0, 800.0, 40.0));
        for i in 0..9 {
            let x = i as f64 * 80.0;
            list.push_scope(None, "Tab", Rect::new(x, 4.0, x + 76.0, 36.0));
            list.pop_scope();
        }
        list.pop_scope();
        list.pop_scope();
        let scene = LintScene::from_paint_list(&list);
        let report = lint(&scene, &LintConfig::new());
        assert!(!findings_for(&report, "tab-count").is_empty());
    }

    #[test]
    fn tab_count_quiet_within_budget() {
        let mut list = app_list();
        list.push_scope(None, "TabStrip", Rect::new(0.0, 0.0, 800.0, 40.0));
        for i in 0..5 {
            let x = i as f64 * 80.0;
            list.push_scope(None, "Tab", Rect::new(x, 4.0, x + 76.0, 36.0));
            list.pop_scope();
        }
        list.pop_scope();
        list.pop_scope();
        let scene = LintScene::from_paint_list(&list);
        let report = lint(&scene, &LintConfig::new());
        assert!(findings_for(&report, "tab-count").is_empty());
    }

    // ---------- min-surface ----------

    #[test]
    fn min_surface_flags_cramped_panel() {
        let mut list = app_list();
        list.push_scope(None, "Panel", Rect::new(10.0, 10.0, 40.0, 30.0));
        list.pop_scope();
        list.pop_scope();
        let mut scene = LintScene::from_paint_list(&list);
        scene.scale_factor = 1.0;
        let report = lint(&scene, &LintConfig::new());
        assert!(!findings_for(&report, "min-surface").is_empty());
    }

    #[test]
    fn min_surface_quiet_on_adequate() {
        let mut list = app_list();
        list.push_scope(None, "Panel", Rect::new(10.0, 10.0, 400.0, 300.0));
        list.pop_scope();
        list.pop_scope();
        let mut scene = LintScene::from_paint_list(&list);
        scene.scale_factor = 1.0;
        let report = lint(&scene, &LintConfig::new());
        assert!(findings_for(&report, "min-surface").is_empty());
    }

    // ---------- menu-breadth / menu-depth ----------

    #[test]
    fn menu_breadth_flags_long_menu() {
        let mut list = app_list();
        list.push_scope(None, "Menu", Rect::new(10.0, 10.0, 210.0, 400.0));
        for i in 0..10 {
            let y = 10.0 + i as f64 * 36.0;
            list.push_scope(None, "MenuItem", Rect::new(14.0, y, 206.0, y + 32.0));
            list.pop_scope();
        }
        list.pop_scope();
        list.pop_scope();
        let scene = LintScene::from_paint_list(&list);
        let report = lint(&scene, &LintConfig::new());
        assert!(!findings_for(&report, "menu-breadth").is_empty());
    }

    #[test]
    fn menu_depth_flags_nested_cascade() {
        let mut list = app_list();
        list.push_scope(None, "Menu", Rect::new(10.0, 10.0, 210.0, 400.0));
        list.push_scope(None, "Submenu", Rect::new(210.0, 10.0, 410.0, 200.0));
        list.push_scope(None, "Submenu", Rect::new(410.0, 10.0, 610.0, 150.0));
        list.pop_scope();
        list.pop_scope();
        list.pop_scope();
        list.pop_scope();
        let scene = LintScene::from_paint_list(&list);
        let report = lint(&scene, &LintConfig::new());
        assert!(!findings_for(&report, "menu-depth").is_empty());
    }

    #[test]
    fn menu_depth_ignores_breadth() {
        // Five submenu *siblings* is a wide menu, not a deep one —
        // menu-depth must measure chains, not counts.
        let mut list = app_list();
        list.push_scope(None, "Menu", Rect::new(10.0, 10.0, 210.0, 400.0));
        for i in 0..5 {
            let x = 10.0 + i as f64 * 60.0;
            list.push_scope(None, "Submenu", Rect::new(x, 10.0, x + 50.0, 40.0));
            list.pop_scope();
        }
        list.pop_scope();
        list.pop_scope();
        let scene = LintScene::from_paint_list(&list);
        let report = lint(&scene, &LintConfig::new());
        assert!(
            findings_for(&report, "menu-depth").is_empty(),
            "breadth counted as depth"
        );
    }

    #[test]
    fn menu_depth_ignores_menuitems() {
        // MenuItem names contain "menu" but aren't cascades.
        let mut list = app_list();
        list.push_scope(None, "Menu", Rect::new(10.0, 10.0, 210.0, 400.0));
        list.push_scope(None, "MenuItem", Rect::new(14.0, 10.0, 206.0, 40.0));
        list.push_scope(None, "MenuItem", Rect::new(18.0, 4.0, 200.0, 30.0));
        list.pop_scope();
        list.pop_scope();
        list.pop_scope();
        list.pop_scope();
        let scene = LintScene::from_paint_list(&list);
        let report = lint(&scene, &LintConfig::new());
        assert!(findings_for(&report, "menu-depth").is_empty());
    }

    #[test]
    fn menu_depth_reports_outermost_only() {
        // Menu > Sub > Sub > Sub — every level past two is in the
        // over-deep chain, but only the outermost reports it.
        let mut list = app_list();
        list.push_scope(None, "Menu", Rect::new(10.0, 10.0, 210.0, 400.0));
        list.push_scope(None, "Submenu", Rect::new(210.0, 10.0, 410.0, 200.0));
        list.push_scope(None, "Submenu", Rect::new(410.0, 10.0, 610.0, 150.0));
        list.push_scope(None, "Submenu", Rect::new(610.0, 10.0, 700.0, 100.0));
        list.pop_scope();
        list.pop_scope();
        list.pop_scope();
        list.pop_scope();
        list.pop_scope();
        let scene = LintScene::from_paint_list(&list);
        let report = lint(&scene, &LintConfig::new());
        let hits = findings_for(&report, "menu-depth");
        assert_eq!(hits.len(), 1, "cascade double-reported: {hits:?}");
    }

    // ---------- edge-touch ----------

    #[test]
    fn edge_touch_flags_flush_content() {
        let mut list = app_list();
        list.push_scope(None, "Panel", Rect::new(0.0, 0.0, 400.0, 200.0));
        // Child flush on all four edges — zero padding anywhere.
        list.push_scope(None, "Card", Rect::new(0.0, 0.0, 400.0, 200.0));
        list.pop_scope();
        list.pop_scope();
        list.pop_scope();
        let mut scene = LintScene::from_paint_list(&list);
        scene.scale_factor = 1.0;
        let report = lint(&scene, &LintConfig::new());
        assert!(!findings_for(&report, "edge-touch").is_empty());
    }

    #[test]
    fn edge_touch_ignores_protruding_children() {
        // A child sticking OUT past the edge is overflow-clip's
        // defect, not cramped padding — negative gaps aren't "flush".
        let mut list = app_list();
        list.push_scope(None, "Panel", Rect::new(0.0, 0.0, 400.0, 200.0));
        list.push_scope(None, "Card", Rect::new(-20.0, 0.0, 400.0, 200.0));
        list.pop_scope();
        list.push_scope(None, "Label", Rect::new(10.0, 10.0, 100.0, 40.0));
        list.pop_scope();
        list.pop_scope();
        list.pop_scope();
        let mut scene = LintScene::from_paint_list(&list);
        scene.scale_factor = 1.0;
        let report = lint(&scene, &LintConfig::new());
        assert!(
            findings_for(&report, "edge-touch").is_empty(),
            "overflow reported as cramped padding"
        );
    }

    #[test]
    fn edge_touch_quiet_with_padding() {
        let mut list = app_list();
        list.push_scope(None, "Panel", Rect::new(0.0, 0.0, 400.0, 200.0));
        list.push_scope(None, "Card", Rect::new(16.0, 16.0, 384.0, 184.0));
        list.pop_scope();
        list.pop_scope();
        list.pop_scope();
        let mut scene = LintScene::from_paint_list(&list);
        scene.scale_factor = 1.0;
        let report = lint(&scene, &LintConfig::new());
        assert!(findings_for(&report, "edge-touch").is_empty());
    }
}
