//! The rule contract and rule metadata.
//!
//! Rules are stateless — the registry hands out `&'static dyn
//! LintRule` references, so a rule can never smuggle state between
//! scenes. Each rule declares the [`Standard`]s it belongs to
//! (multi-membership is normal — `color-budget` is both ISA-101 color
//! discipline and design-system consistency), its *intrinsic* default
//! severity, a [`Confidence`] tier, and a citation the report prints
//! so findings teach the standard, not just the symptom.

use crate::config::LintConfig;
use crate::report::Finding;
use crate::scene::LintScene;
use crate::severity::Severity;
use crate::standard::Standard;

/// How certain a rule's finding is — the axe violations/needs-review
/// split. Heuristic findings must never be dressed up as
/// deterministic violations: a crowded-layout heuristic is a *review
/// prompt*, not a fact.
///
/// # Examples
///
/// ```
/// use martensite_design_lint::Confidence;
///
/// assert!(Confidence::Deterministic > Confidence::Heuristic);
/// ```
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Confidence {
    /// The rule makes a subjective call — flag for human review.
    /// Reported with "consider:" phrasing regardless of severity.
    Heuristic,
    /// The rule measures something concrete against a stated
    /// threshold — 24pt targets, declared level, element counts.
    Deterministic,
}

impl Confidence {
    /// Config key for this tier.
    pub fn config_key(self) -> &'static str {
        match self {
            Confidence::Heuristic => "heuristic",
            Confidence::Deterministic => "deterministic",
        }
    }
}

/// A design-standard rule evaluated over a [`LintScene`].
///
/// Implementations should be deterministic and side-effect free —
/// findings are sorted and deduplicated by the engine.
pub trait LintRule: Send + Sync {
    /// Stable kebab-case identifier — used in config files, allow
    /// specifiers, and reports (`"nav-depth"`).
    fn id(&self) -> &'static str;

    /// The standards this rule enforces. A rule is active when at
    /// least one of these is enabled in [`LintConfig::standards`].
    fn standards(&self) -> &'static [Standard];

    /// The evidence-backed severity — what the rule fires at with no
    /// configuration. Config can raise or lower it; the finding keeps
    /// this as its intrinsic level so overrides stay visible.
    fn default_severity(&self) -> Severity;

    /// Deterministic or heuristic — see [`Confidence`].
    fn confidence(&self) -> Confidence {
        Confidence::Deterministic
    }

    /// One-line rule summary for `--list-rules` style output.
    fn title(&self) -> &'static str;

    /// The standard/rationale citation printed with findings — e.g.
    /// `"ISA-101 §5 display hierarchy; Nielsen #8 aesthetic &
    /// minimalist design"`. This is what makes a finding teach.
    fn citation(&self) -> &'static str;

    /// Evaluate the rule over the whole scene. Findings are produced
    /// *before* suppression — the engine applies allows afterward so
    /// suppressed findings remain visible in the report's
    /// `suppressed` bucket.
    fn check(&self, scene: &LintScene, cfg: &LintConfig) -> Vec<Finding>;
}

/// Helper for rule bodies — read a tunable with its documented
/// default.
pub(crate) fn param(cfg: &LintConfig, rule: &str, key: &str, default: f64) -> f64 {
    cfg.rule_param(rule, key, default)
}
