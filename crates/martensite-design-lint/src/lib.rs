//! # martensite-design-lint
//!
//! Design-standard linting for Martensite user interfaces. The engine
//! replays a [`PaintList`]'s `PushScope` provenance markers into a
//! [`LintScene`] — a normalized widget tree with geometry, text sizes,
//! and colors — then evaluates a catalog of evidence-backed rules:
//! WCAG floors, ISA-101 High-Performance HMI, ISA-18.2 alarm analogs,
//! Hick/Fitts cognitive-cost laws, Tufte/Few information design, and
//! perceptual-clutter research.
//!
//! ## Control model
//!
//! All standards are on by default; everything is overridable:
//!
//! - **Standard sets** — [`LintConfig::only_standards`],
//!   [`without_standard`](LintConfig::without_standard)
//! - **Per-rule severity** — [`with_rule`](LintConfig::with_rule)
//!   (`Off` disables, `Forbid` is unsuppressible)
//! - **Per-rule thresholds** — [`with_rule_param`](LintConfig::with_rule_param)
//! - **Path allows** — [`with_allow`](LintConfig::with_allow) suppresses
//!   a subtree by scope-path glob
//! - **Inline allows** — a widget's `debug_name` ending in
//!   `@lint:rule-id` (or `@lint:all`, `@lint:standard:wcag`) suppresses
//!   that subtree; `@level:1..4` declares an ISA-101 display level
//! - **Config file** — [`LintConfig::from_toml`]/[`from_file`](LintConfig::from_file)
//!
//! Suppressed findings are never dropped — they land in
//! [`LintReport::suppressed`] — and allows that suppress nothing are
//! reported in [`LintReport::unused_allows`].
//!
//! # Examples
//!
//! ```
//! use martensite_core::PaintList;
//! use martensite_design_lint::{lint_paint_list, LintConfig};
//!
//! let report = lint_paint_list(&PaintList::new(), &LintConfig::new());
//! assert!(report.is_clean());
//! ```

mod config;
mod report;
mod rule;
mod rules;
mod scene;
mod severity;
mod standard;

pub use config::{LintConfig, LintConfigError, PathAllow, RuleSetting};
pub use report::{Finding, LintReport};
pub use rule::{Confidence, LintRule};
pub use rules::all_rules;
pub use scene::{LintNode, LintScene, NodeKind};
pub use severity::Severity;
pub use standard::Standard;

use std::collections::{HashMap, HashSet};

use martensite_core::PaintList;

/// Convenience: distill `list` into a scene (scale factor taken from
/// `config`) and lint it.
///
/// # Examples
///
/// ```
/// use martensite_core::PaintList;
/// use martensite_design_lint::{lint_paint_list, LintConfig};
///
/// let report = lint_paint_list(&PaintList::new(), &LintConfig::new());
/// assert_eq!(report.findings.len(), 0);
/// ```
pub fn lint_paint_list(list: &PaintList, config: &LintConfig) -> LintReport {
    let mut scene = LintScene::from_paint_list(list);
    scene.scale_factor = config.scale_factor;
    lint(&scene, config)
}

/// Lint a scene with the built-in rule set plus caller-supplied
/// custom rules — the extension seam for project- or site-specific
/// style guides (ISA-101 explicitly expects per-site customization).
///
/// # Examples
///
/// ```
/// use martensite_design_lint::{lint_with, LintConfig, LintScene};
///
/// let report = lint_with(&LintScene::default(), &LintConfig::new(), &[]);
/// assert!(report.is_clean());
/// ```
pub fn lint_with(
    scene: &LintScene,
    config: &LintConfig,
    extra_rules: &[&'static dyn LintRule],
) -> LintReport {
    let mut rules = all_rules();
    rules.extend_from_slice(extra_rules);
    run(scene, config, &rules)
}

/// Lint a scene with the built-in rule set.
///
/// # Examples
///
/// ```
/// use martensite_design_lint::{lint, LintConfig, LintScene};
///
/// let report = lint(&LintScene::default(), &LintConfig::new());
/// assert!(report.is_clean());
/// ```
pub fn lint(scene: &LintScene, config: &LintConfig) -> LintReport {
    let rules = all_rules();
    run(scene, config, &rules)
}

/// Every registered rule's metadata — for `--list-rules` UIs and docs.
pub fn rule_catalog() -> Vec<(&'static str, &'static str, Severity, &'static [Standard])> {
    all_rules()
        .iter()
        .map(|r| (r.id(), r.title(), r.default_severity(), r.standards()))
        .collect()
}

fn run(scene: &LintScene, config: &LintConfig, rules: &[&'static dyn LintRule]) -> LintReport {
    let mut report = LintReport::default();

    // Path → node lookup for inline-allow checks.
    let by_path: HashMap<&str, &LintNode> = scene.walk().map(|n| (n.path.as_str(), n)).collect();

    let mut used_config_allows: HashSet<usize> = HashSet::new();
    // (declaring node path, spec) pairs that suppressed something.
    let mut used_inline: HashSet<(String, String)> = HashSet::new();

    for rule in rules {
        // Standard-set selection: inactive when none of its standards
        // are enabled.
        if !rule
            .standards()
            .iter()
            .any(|s| config.standards.contains(s))
        {
            continue;
        }
        let severity = config
            .rule_severity(rule.id())
            .unwrap_or_else(|| rule.default_severity());
        if severity == Severity::Off {
            continue;
        }

        for mut finding in rule.check(scene, config) {
            finding.intrinsic = rule.default_severity();
            finding.severity = severity;
            finding.confidence = rule.confidence();
            finding.standards = rule.standards();
            finding.citation = rule.citation();
            finding.doc = doc_ref(&config.docs_base, rule.id());

            // `Forbid` cannot be suppressed — rustc's model for
            // non-negotiable rules.
            if severity == Severity::Forbid {
                report.findings.push(finding);
                continue;
            }

            if let Some(decl_path) = inline_allow(&finding, *rule, &by_path) {
                for spec in allow_specs(&finding, *rule, &by_path) {
                    used_inline.insert((decl_path.clone(), spec));
                }
                finding.suppressed_by = Some(format!("@lint on {decl_path}"));
                report.suppressed.push(finding);
                continue;
            }
            if let Some(idx) = matching_config_allow(&finding, *rule, config) {
                used_config_allows.insert(idx);
                let a = &config.allows[idx];
                finding.suppressed_by =
                    Some(format!("allow {:?} [{}]", a.path, a.specs.join(", ")));
                report.suppressed.push(finding);
                continue;
            }
            report.findings.push(finding);
        }
    }

    // Expect-style stale bookkeeping: allows that suppressed nothing
    // are reported so suppressions self-clean instead of rotting.
    for (i, a) in config.allows.iter().enumerate() {
        if !used_config_allows.contains(&i) {
            report
                .unused_allows
                .push(format!("allow {:?} [{}]", a.path, a.specs.join(", ")));
        }
    }
    for node in scene.walk() {
        for spec in &node.own_allows {
            if !used_inline.contains(&(node.path.clone(), spec.clone())) {
                report
                    .unused_allows
                    .push(format!("inline @lint:{spec} on {}", node.path));
            }
        }
    }

    report.findings.sort_by(|a, b| {
        (std::cmp::Reverse(a.severity), a.rule, &a.path).cmp(&(
            std::cmp::Reverse(b.severity),
            b.rule,
            &b.path,
        ))
    });
    report
}

/// The doc reference for a rule — `{docs_base}/rules/{id}.md`.
/// URL bases pass through verbatim; filesystem bases resolve to an
/// absolute path when the file exists so terminals and editors
/// linkify it, and stay relative otherwise.
fn doc_ref(docs_base: &str, rule_id: &str) -> String {
    let base = docs_base.trim_end_matches('/');
    if base.starts_with("http://") || base.starts_with("https://") {
        return format!("{base}/rules/{rule_id}.md");
    }
    let rel = format!("{base}/rules/{rule_id}.md");
    let path = std::path::Path::new(&rel);
    if path.is_file() {
        return path
            .canonicalize()
            .map(|p| p.display().to_string())
            .unwrap_or(rel);
    }
    rel
}

/// Does an ancestor-or-self inline `@lint:` marker suppress this
/// finding? Returns the declaring node's path when so.
fn inline_allow(
    finding: &Finding,
    rule: &dyn LintRule,
    by_path: &HashMap<&str, &LintNode>,
) -> Option<String> {
    let mut path = finding.path.as_str();
    loop {
        let node = by_path.get(path)?;
        for spec in &node.own_allows {
            if spec_matches(spec, rule) {
                return Some(node.path.clone());
            }
        }
        let i = path.rfind('/')?;
        path = &path[..i];
    }
}

/// The specs on the declaring node that match this rule — for
/// used-marker bookkeeping.
fn allow_specs(
    finding: &Finding,
    rule: &dyn LintRule,
    by_path: &HashMap<&str, &LintNode>,
) -> Vec<String> {
    let mut path = finding.path.as_str();
    loop {
        let Some(node) = by_path.get(path) else {
            return Vec::new();
        };
        let hits: Vec<String> = node
            .own_allows
            .iter()
            .filter(|s| spec_matches(s, rule))
            .cloned()
            .collect();
        if !hits.is_empty() {
            return hits;
        }
        match path.rfind('/') {
            Some(i) => path = &path[..i],
            None => return Vec::new(),
        }
    }
}

/// Does a configured [`PathAllow`] suppress this finding? Returns the
/// allow's index for used-bookkeeping.
fn matching_config_allow(
    finding: &Finding,
    rule: &dyn LintRule,
    config: &LintConfig,
) -> Option<usize> {
    let mut path = finding.path.as_str();
    loop {
        for (i, a) in config.allows.iter().enumerate() {
            if a.matches_path(path) && a.suppresses(rule.id(), rule.standards()) {
                return Some(i);
            }
        }
        let i = path.rfind('/')?;
        path = &path[..i];
    }
}

/// Does an allow spec name this rule — directly, by `all`, or by
/// `standard:<key>` membership?
fn spec_matches(spec: &str, rule: &dyn LintRule) -> bool {
    spec == "all"
        || spec == rule.id()
        || spec
            .strip_prefix("standard:")
            .and_then(Standard::from_key)
            .is_some_and(|s| rule.standards().contains(&s))
}

#[cfg(test)]
mod tests {
    use super::*;
    use kurbo::Rect;
    use martensite_core::PaintCommand;

    fn scope_list() -> PaintList {
        let mut list = PaintList::new();
        list.push_scope(None, "App", Rect::new(0.0, 0.0, 800.0, 600.0));
        list.commands.push(PaintCommand::FillRect(
            Rect::new(0.0, 0.0, 800.0, 600.0),
            [20, 20, 24, 255],
        ));
        list
    }

    fn end_scope(list: &mut PaintList) {
        list.pop_scope();
    }

    /// A surface with `n` buttons — enough area to count as a surface.
    fn strip_scene(n: usize) -> LintScene {
        let mut list = scope_list();
        list.push_scope(None, "Toolbar", Rect::new(0.0, 0.0, 800.0, 60.0));
        for i in 0..n {
            list.push_scope(
                None,
                "Button",
                Rect::new(10.0 + i as f64 * 70.0, 10.0, 70.0 + i as f64 * 70.0, 50.0),
            );
            end_scope(&mut list);
        }
        end_scope(&mut list);
        end_scope(&mut list);
        LintScene::from_paint_list(&list)
    }

    #[test]
    fn scene_builds_tree_with_stats() {
        let scene = strip_scene(3);
        assert_eq!(scene.roots.len(), 1);
        let app = &scene.roots[0];
        assert_eq!(app.name, "App");
        assert_eq!(app.children.len(), 1);
        let bar = &app.children[0];
        assert_eq!(bar.name, "Toolbar");
        assert_eq!(bar.children.len(), 3);
        assert_eq!(bar.children[0].kind, NodeKind::Interactive);
        assert!(bar.children[0].path.ends_with("Toolbar/Button"));
    }

    #[test]
    fn choice_count_fires_over_max() {
        let scene = strip_scene(9);
        let report = lint(&scene, &LintConfig::new());
        let cc: Vec<_> = report
            .findings
            .iter()
            .filter(|f| f.rule == "choice-count")
            .collect();
        assert_eq!(cc.len(), 1, "expected one finding, got {report:?}");
        assert!(cc[0].message.contains("9"));
    }

    #[test]
    fn choice_count_under_max_is_quiet() {
        let scene = strip_scene(4);
        let cfg = LintConfig::new().only_standards(&[Standard::HciLaws]);
        let report = lint(&scene, &cfg);
        assert!(report.findings.iter().all(|f| f.rule != "choice-count"));
    }

    #[test]
    fn standards_selection_gates_rules() {
        let scene = strip_scene(9);
        // Only WCAG — choice-count (HciLaws) must not fire.
        let cfg = LintConfig::new().only_standards(&[Standard::Wcag]);
        let report = lint(&scene, &cfg);
        assert!(report.findings.iter().all(|f| f.rule != "choice-count"));
        assert!(report.findings.iter().all(|f| f.rule != "nav-depth"));
    }

    #[test]
    fn per_rule_off_disables() {
        let scene = strip_scene(9);
        let cfg = LintConfig::new().with_rule("choice-count", Severity::Off);
        let report = lint(&scene, &cfg);
        assert!(report.findings.iter().all(|f| f.rule != "choice-count"));
    }

    #[test]
    fn config_path_allow_suppresses_and_reports() {
        let scene = strip_scene(9);
        let cfg = LintConfig::new().with_allow("App/Toolbar/**", &["choice-count"]);
        let report = lint(&scene, &cfg);
        assert!(report.findings.iter().all(|f| f.rule != "choice-count"));
        assert_eq!(
            report
                .suppressed
                .iter()
                .filter(|f| f.rule == "choice-count")
                .count(),
            1
        );
        assert!(report.unused_allows.is_empty());
    }

    #[test]
    fn unused_allow_is_reported() {
        let scene = strip_scene(3);
        let cfg = LintConfig::new().with_allow("App/Nowhere/**", &["all"]);
        let report = lint(&scene, &cfg);
        assert_eq!(report.unused_allows.len(), 1);
    }

    #[test]
    fn inline_lint_marker_suppresses_subtree() {
        let mut list = scope_list();
        list.push_scope(
            None,
            "Toolbar@lint:choice-count",
            Rect::new(0.0, 0.0, 800.0, 60.0),
        );
        for i in 0..9 {
            list.push_scope(
                None,
                "Button",
                Rect::new(10.0 + i as f64 * 70.0, 10.0, 70.0 + i as f64 * 70.0, 50.0),
            );
            end_scope(&mut list);
        }
        end_scope(&mut list);
        end_scope(&mut list);
        let scene = LintScene::from_paint_list(&list);
        let report = lint(&scene, &LintConfig::new());
        assert!(report.findings.iter().all(|f| f.rule != "choice-count"));
        assert_eq!(
            report
                .suppressed
                .iter()
                .filter(|f| f.rule == "choice-count")
                .count(),
            1
        );
        // The marker suppressed something — not reported as unused.
        assert!(report
            .unused_allows
            .iter()
            .all(|u| !u.contains("choice-count")));
    }

    #[test]
    fn forbid_cannot_be_suppressed() {
        let scene = strip_scene(9);
        let cfg = LintConfig::new()
            .with_rule("choice-count", Severity::Forbid)
            .with_allow("**", &["all"]);
        let report = lint(&scene, &cfg);
        assert!(report
            .findings
            .iter()
            .any(|f| f.rule == "choice-count" && f.severity == Severity::Forbid));
    }

    #[test]
    fn nav_depth_counts_runs_not_widgets() {
        // Tabs > TabBar > Tab — one nav run, not three layers.
        let mut list = scope_list();
        list.push_scope(None, "Tabs", Rect::new(0.0, 60.0, 800.0, 90.0));
        list.push_scope(None, "TabBar", Rect::new(0.0, 60.0, 800.0, 84.0));
        list.push_scope(None, "Tab", Rect::new(0.0, 60.0, 100.0, 84.0));
        end_scope(&mut list);
        end_scope(&mut list);
        end_scope(&mut list);
        end_scope(&mut list);
        let scene = LintScene::from_paint_list(&list);
        let report = lint(&scene, &LintConfig::new());
        assert!(report.findings.iter().all(|f| f.rule != "nav-depth"));
    }

    #[test]
    fn nested_nav_layers_flag() {
        // Strip of controls (named like a toolbar → Navigation) nested
        // inside Tabs inside a MenuBar = 3 layers > max 2.
        let mut list = scope_list();
        list.push_scope(None, "MenuBar", Rect::new(0.0, 0.0, 800.0, 30.0));
        list.push_scope(None, "Tabs", Rect::new(0.0, 30.0, 800.0, 60.0));
        list.push_scope(None, "Toolbar", Rect::new(0.0, 60.0, 800.0, 90.0));
        end_scope(&mut list);
        end_scope(&mut list);
        end_scope(&mut list);
        end_scope(&mut list);
        let scene = LintScene::from_paint_list(&list);
        let report = lint(&scene, &LintConfig::new());
        assert_eq!(
            report
                .findings
                .iter()
                .filter(|f| f.rule == "nav-depth")
                .count(),
            1
        );
    }

    #[test]
    fn target_size_flags_small_controls() {
        let mut list = scope_list();
        list.push_scope(None, "Button", Rect::new(10.0, 10.0, 30.0, 26.0)); // 20×16px
        end_scope(&mut list);
        end_scope(&mut list);
        let mut scene = LintScene::from_paint_list(&list);
        scene.scale_factor = 1.0;
        let cfg = LintConfig::new().only_standards(&[Standard::Wcag]);
        let report = lint(&scene, &cfg);
        assert!(report.findings.iter().any(|f| f.rule == "target-size"));
    }

    #[test]
    fn toml_config_roundtrip() {
        let text = r#"
scale_factor = 2.0
standards = ["wcag", "hci-laws"]

[rules.choice-count]
severity = "error"
max = 5

[classify]
"MyThing" = "navigation"

[[allow]]
path = "App/Media/**"
rules = ["color-budget"]
"#;
        let cfg = LintConfig::from_toml(text).unwrap();
        assert_eq!(cfg.scale_factor, 2.0);
        assert_eq!(cfg.standards.len(), 2);
        assert_eq!(cfg.rule_severity("choice-count"), Some(Severity::Error));
        assert_eq!(cfg.rule_param("choice-count", "max", 7.0), 5.0);
        assert_eq!(cfg.allows.len(), 1);
        assert_eq!(cfg.classified("MyThing"), Some(crate::NodeKind::Navigation));
    }

    #[test]
    fn level_marker_parses() {
        let mut list = PaintList::new();
        list.push_scope(
            None,
            "OverviewPanel@level:1@lint:all",
            Rect::new(0.0, 0.0, 100.0, 100.0),
        );
        list.pop_scope();
        let scene = LintScene::from_paint_list(&list);
        let node = &scene.roots[0];
        assert_eq!(node.display_level, Some(1));
        assert!(node.allows.contains(&"all".to_string()));
        assert_eq!(node.name, "OverviewPanel");
    }
}
