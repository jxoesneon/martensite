//! Findings and the lint report.
//!
//! The report has three buckets, following axe-core's model: active
//! `findings`, `suppressed` findings (allowed but **shown**, never
//! silently dropped — suppressed rules that quietly swallow real
//! problems are how lint systems lose trust), and `unused_allows`
//! (allow entries that suppressed nothing — rustc `#[expect]` /
//! ESLint `reportUnusedDisableDirectives` semantics, so stale ignores
//! self-report instead of rotting in place).

use std::fmt::Write as _;

use kurbo::Rect;

use crate::rule::Confidence;
use crate::severity::Severity;
use crate::standard::Standard;

/// One rule violation (or suppressed violation) anchored to a scope.
///
/// # Examples
///
/// Findings are produced by [`crate::lint`]; severity, standards, and
/// citation come from the rule and its configuration.
///
/// ```
/// use martensite_design_lint::{lint_paint_list, LintConfig};
/// use martensite_core::PaintList;
///
/// let report = lint_paint_list(&PaintList::new(), &LintConfig::new());
/// assert!(report.findings.is_empty()); // empty scene: nothing to flag
/// ```
#[derive(Debug, Clone)]
pub struct Finding {
    /// The firing rule's id.
    pub rule: &'static str,
    /// The rule's intrinsic severity — the evidence-backed default,
    /// before config overrides.
    pub intrinsic: Severity,
    /// The effective severity after config overrides.
    pub severity: Severity,
    /// The rule's confidence tier — heuristic findings are review
    /// prompts, not violations.
    pub confidence: Confidence,
    /// Standards the rule cites.
    pub standards: &'static [Standard],
    /// The citation text — the standard and rationale.
    pub citation: &'static str,
    /// Scope path where the finding anchors (`"App/ZonePanel/Tabs"`).
    pub path: String,
    /// Bounds in device px, when a region is implicated.
    pub bounds: Option<Rect>,
    /// What was measured and what the threshold is — written for
    /// someone who has never read the standard.
    pub message: String,
    /// Reference to the rule's full documentation —
    /// `{docs_base}/rules/{rule-id}.md`, resolved to an absolute path
    /// when it exists on disk so terminals linkify it; verbatim for
    /// URL bases or missing files.
    pub doc: String,
    /// For findings in [`LintReport::suppressed`]: which allow
    /// suppressed them (path glob or inline marker location).
    pub suppressed_by: Option<String>,
}

impl Finding {
    /// Construct a finding at the rule's intrinsic severity —
    /// `lint_scene` rewrites `severity` when config overrides it.
    pub(crate) fn new(rule: &'static str, path: &str, message: impl Into<String>) -> Self {
        Finding {
            rule,
            intrinsic: Severity::Warn,
            severity: Severity::Warn,
            confidence: Confidence::Deterministic,
            standards: &[],
            citation: "",
            path: path.to_string(),
            bounds: None,
            message: message.into(),
            doc: String::new(),
            suppressed_by: None,
        }
    }

    /// Builder — set bounds.
    pub(crate) fn at(mut self, bounds: Rect) -> Self {
        self.bounds = Some(bounds);
        self
    }

    /// A stable fingerprint for baseline/suppression ledgers —
    /// rule + path + message digest, robust to unrelated layout drift.
    pub fn fingerprint(&self) -> String {
        let mut h: u64 = 0xcbf2_9ce4_8422_2325;
        for b in format!("{}|{}|{}", self.rule, self.path, self.message).bytes() {
            h ^= u64::from(b);
            h = h.wrapping_mul(0x0000_0100_0000_01b3);
        }
        format!("{h:016x}")
    }
}

/// The engine's output — active findings, suppressed findings, and
/// stale allows.
///
/// Deliberately has **no aggregate score**: single-number scores get
/// gamed and overclaimed ("Lighthouse 100"). Per-rule and
/// per-severity counts are honest; a score is not.
///
/// # Examples
///
/// ```
/// use martensite_design_lint::{LintReport, LintScene, LintConfig};
///
/// let report = martensite_design_lint::lint(&LintScene::default(), &LintConfig::new());
/// assert!(report.is_clean());
/// ```
#[derive(Debug, Default)]
pub struct LintReport {
    /// Active findings after suppression.
    pub findings: Vec<Finding>,
    /// Findings suppressed by allows — reported, not dropped.
    /// Each finding's message is prefixed by which allow suppressed it.
    pub suppressed: Vec<Finding>,
    /// Allow specifiers (path allows and inline `@lint:` markers)
    /// that suppressed nothing — stale ignores to clean up.
    pub unused_allows: Vec<String>,
}

impl LintReport {
    /// True when no active findings at `Warn` or above exist.
    /// Info findings never dirty a report.
    pub fn is_clean(&self) -> bool {
        !self.findings.iter().any(|f| f.severity >= Severity::Warn)
    }

    /// Findings at `Error`/`Forbid`.
    pub fn errors(&self) -> impl Iterator<Item = &Finding> {
        self.findings
            .iter()
            .filter(|f| f.severity >= Severity::Error)
    }

    /// Findings at `Warn`.
    pub fn warnings(&self) -> impl Iterator<Item = &Finding> {
        self.findings
            .iter()
            .filter(|f| f.severity == Severity::Warn)
    }

    /// Count of active findings at or above a severity.
    pub fn count_at(&self, severity: Severity) -> usize {
        self.findings
            .iter()
            .filter(|f| f.severity >= severity)
            .count()
    }

    /// Human-readable report: findings grouped by severity, each with
    /// its scope path, citation, and heuristic phrasing where
    /// applicable. Suppressed findings and stale allows are listed
    /// under their own headers so nothing is hidden.
    pub fn to_text(&self) -> String {
        let mut out = String::new();
        let mut sorted: Vec<&Finding> = self.findings.iter().collect();
        sorted.sort_by_key(|f| (std::cmp::Reverse(f.severity), f.rule, f.path.clone()));
        for f in sorted {
            let _ = writeln!(out, "[{}] {} @ {}", f.severity, f.rule, f.path);
            let _ = writeln!(out, "    {}", f.message);
            let prefix = match f.confidence {
                Confidence::Heuristic => "consider",
                Confidence::Deterministic => "standard",
            };
            let standards = f
                .standards
                .iter()
                .map(|s| s.config_key())
                .collect::<Vec<_>>()
                .join(", ");
            let _ = writeln!(out, "    {prefix}: {} [{}]", f.citation, standards);
            if !f.doc.is_empty() {
                let _ = writeln!(out, "    see: {}", f.doc);
            }
            if f.severity != f.intrinsic {
                let _ = writeln!(
                    out,
                    "    note: severity overridden {} -> {}",
                    f.intrinsic, f.severity
                );
            }
        }
        if !self.suppressed.is_empty() {
            let _ = writeln!(
                out,
                "\n{} suppressed finding(s) — shown for auditability:",
                self.suppressed.len()
            );
            for f in &self.suppressed {
                let _ = writeln!(out, "[suppressed] {} @ {}", f.rule, f.path);
                let _ = writeln!(out, "    {}", f.message);
                if let Some(by) = &f.suppressed_by {
                    let _ = writeln!(out, "    allowed by: {by}");
                }
            }
        }
        if !self.unused_allows.is_empty() {
            let _ = writeln!(
                out,
                "\n{} unused allow(s) — suppressed nothing, safe to remove:",
                self.unused_allows.len()
            );
            for a in &self.unused_allows {
                let _ = writeln!(out, "    {a}");
            }
        }
        out
    }
}
