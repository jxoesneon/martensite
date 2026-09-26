//! Configuration — the developer-control layer.
//!
//! The model is deliberately rustc/ESLint-shaped: **all standards on
//! by default**, with four independent override mechanisms that
//! compose:
//!
//! 1. **Standard selection** — enable the whole catalog (default),
//!    a subset via [`only_standards`](LintConfig::only_standards), or
//!    all-but via [`without_standard`](LintConfig::without_standard).
//! 2. **Per-rule severity** — [`with_rule`](LintConfig::with_rule),
//!    including [`Severity::Off`].
//! 3. **Per-rule parameters** — every threshold is tunable via
//!    [`with_rule_param`](LintConfig::with_rule_param) (or TOML).
//! 4. **Path allows** — [`with_allow`](LintConfig::with_allow) suppresses
//!    findings for a scope subtree by path glob, for named rules or
//!    whole standards. Inline per-element ignores use the
//!    `@lint:rule` suffix on `Widget::debug_name` — see
//!    [`LintScene::from_paint_list`](crate::LintScene::from_paint_list).
//!
//! # Examples
//!
//! ```
//! use martensite_design_lint::{LintConfig, Severity, Standard};
//!
//! let cfg = LintConfig::new()
//!     .without_standard(Standard::Perception)
//!     .with_rule("nav-depth", Severity::Error)
//!     .with_rule_param("choice-count", "max", 5.0)
//!     .with_allow("**/MediaZone/**", &["color-budget"]);
//!
//! assert_eq!(cfg.rule_severity("nav-depth"), Some(Severity::Error));
//! ```

use std::collections::{BTreeSet, HashMap};

use crate::scene::NodeKind;
use crate::severity::Severity;
use crate::standard::Standard;

/// A path-scoped suppression — "under `path`, these rules/standards
/// don't apply." `path` is a `/`-separated glob over
/// [`LintNode::path`](crate::LintNode::path): `*` matches inside a
/// segment, `**` crosses depth. `specs` entries are rule ids,
/// `"standard:<key>"`, or `"all"`.
///
/// # Examples
///
/// ```
/// use martensite_design_lint::PathAllow;
///
/// let a = PathAllow::new("App/Media/**", &["all"]);
/// assert!(a.matches_path("App/Media/TabStrip/Button"));
/// assert!(!a.matches_path("App/Grid/Button"));
///
/// // `**` also matches zero segments — the allow covers the
/// // declaring node itself, not just its descendants.
/// assert!(a.matches_path("App/Media"));
/// // `*` never crosses `/`.
/// let b = PathAllow::new("App/*Zone", &["all"]);
/// assert!(b.matches_path("App/MediaZone"));
/// assert!(!b.matches_path("App/Media/Zone"));
/// ```
#[derive(Debug, Clone)]
pub struct PathAllow {
    /// Glob pattern matched against [`LintNode::path`](crate::LintNode::path).
    pub path: String,
    /// Suppressed specifiers: rule ids, `standard:<key>`, or `all`.
    pub specs: Vec<String>,
}

impl PathAllow {
    /// Construct from a path glob and specifiers.
    pub fn new(path: &str, specs: &[&str]) -> Self {
        PathAllow {
            path: path.to_string(),
            specs: specs.iter().map(|s| s.to_ascii_lowercase()).collect(),
        }
    }

    /// Glob-match the pattern against a node path.
    pub fn matches_path(&self, path: &str) -> bool {
        glob_match(&self.path, path)
    }

    /// Does this allow suppress `rule_id` (a member of `standards`)?
    pub fn suppresses(&self, rule_id: &str, standards: &[Standard]) -> bool {
        self.specs.iter().any(|spec| {
            spec == "all"
                || spec == rule_id
                || spec
                    .strip_prefix("standard:")
                    .and_then(Standard::from_key)
                    .is_some_and(|s| standards.contains(&s))
        })
    }
}

/// Per-rule override: an optional severity plus free-form numeric
/// parameters the rule documents (thresholds like `max`, `min_pt`).
#[derive(Debug, Clone, Default)]
pub struct RuleSetting {
    /// Resolved severity override; `None` = rule default.
    pub severity: Option<Severity>,
    /// Named numeric parameters, e.g. `{"max": 5.0}`.
    pub params: HashMap<String, f64>,
}

/// The resolved lint configuration — what runs, how loudly, and
/// where it doesn't apply.
///
/// # Examples
///
/// ```
/// use martensite_design_lint::{LintConfig, Standard};
///
/// // Default: every standard enabled.
/// let cfg = LintConfig::new();
/// assert_eq!(cfg.standards.len(), Standard::ALL.len());
///
/// // Cherry-pick for a consumer app — no industrial standards.
/// let cfg = LintConfig::new().only_standards(&[
///     Standard::Wcag,
///     Standard::HciLaws,
///     Standard::Consistency,
/// ]);
/// assert_eq!(cfg.standards.len(), 3);
/// ```
#[derive(Debug, Clone)]
pub struct LintConfig {
    /// Enabled standards — rules fire only when at least one of their
    /// cited standards is enabled. Default: all.
    pub standards: BTreeSet<Standard>,
    /// Display scale factor (device px → logical pt).
    pub scale_factor: f32,
    /// Per-rule overrides keyed by rule id.
    pub rules: HashMap<String, RuleSetting>,
    /// Path-scoped suppressions, applied in order — later entries win
    /// over earlier ones for the same rule.
    pub allows: Vec<PathAllow>,
    /// `debug_name` last-segment → kind reclassification, for widgets
    /// the name heuristics get wrong (e.g. a `Dashboard` that is
    /// content, not a container).
    pub classify: HashMap<String, NodeKind>,
    /// Root the per-rule doc references resolve against — a
    /// repo-relative path (`"docs/design-standards"`) or a URL
    /// (`"https://docs.example.com/design"`). Findings carry
    /// `{docs_base}/rules/{rule-id}.md` so reports link straight to
    /// the full explanation.
    pub docs_base: String,
}

impl Default for LintConfig {
    fn default() -> Self {
        LintConfig::new()
    }
}

impl LintConfig {
    /// All standards enabled, no overrides.
    pub fn new() -> Self {
        LintConfig {
            standards: Standard::ALL.iter().copied().collect(),
            scale_factor: 1.0,
            rules: HashMap::new(),
            allows: Vec::new(),
            classify: HashMap::new(),
            docs_base: "docs/design-standards".to_string(),
        }
    }

    /// Enable exactly the given standards — the "cherry-pick" entry
    /// point for projects that want a subset of the catalog.
    pub fn only_standards(mut self, standards: &[Standard]) -> Self {
        self.standards = standards.iter().copied().collect();
        self
    }

    /// Enable one additional standard.
    pub fn with_standard(mut self, standard: Standard) -> Self {
        self.standards.insert(standard);
        self
    }

    /// Disable one standard — its rules produce no findings.
    pub fn without_standard(mut self, standard: Standard) -> Self {
        self.standards.remove(&standard);
        self
    }

    /// Override a rule's severity (`Off` disables it).
    pub fn with_rule(mut self, rule_id: &str, severity: Severity) -> Self {
        self.rules.entry(rule_id.to_string()).or_default().severity = Some(severity);
        self
    }

    /// Tune a rule's numeric parameter (see each rule's docs for keys).
    pub fn with_rule_param(mut self, rule_id: &str, key: &str, value: f64) -> Self {
        self.rules
            .entry(rule_id.to_string())
            .or_default()
            .params
            .insert(key.to_string(), value);
        self
    }

    /// Suppress findings under a scope-path glob for the given
    /// specifiers (rule ids, `"standard:<key>"`, or `"all"`).
    pub fn with_allow(mut self, path_glob: &str, specs: &[&str]) -> Self {
        self.allows.push(PathAllow::new(path_glob, specs));
        self
    }

    /// Reclassify a widget type name for the structural rules.
    pub fn with_classify(mut self, name: &str, kind: NodeKind) -> Self {
        self.classify.insert(name.to_ascii_lowercase(), kind);
        self
    }

    /// Point finding doc references at a different docs root — a
    /// project-local path or a hosted URL.
    pub fn with_docs_base(mut self, base: &str) -> Self {
        self.docs_base = base.trim_end_matches('/').to_string();
        self
    }

    /// The configured severity for a rule, if overridden.
    pub fn rule_severity(&self, rule_id: &str) -> Option<Severity> {
        self.rules.get(rule_id).and_then(|r| r.severity)
    }

    /// A rule's numeric parameter, falling back to its documented
    /// default.
    pub fn rule_param(&self, rule_id: &str, key: &str, default: f64) -> f64 {
        self.rules
            .get(rule_id)
            .and_then(|r| r.params.get(key))
            .copied()
            .unwrap_or(default)
    }

    /// Would `path` suppress `rule_id` — via path allows or the
    /// node's own inline `@lint:` marker? `allows` is the node's
    /// inline spec list.
    pub fn is_suppressed(&self, path: &str, rule_id: &str, standards: &[Standard]) -> bool {
        // Path allows match any ancestor prefix — suppressing
        // "A/B/**" covers findings anchored at "A/B/C/D".
        let mut p = path;
        loop {
            if self
                .allows
                .iter()
                .any(|a| a.matches_path(p) && a.suppresses(rule_id, standards))
            {
                return true;
            }
            match p.rfind('/') {
                Some(i) => p = &p[..i],
                None => return false,
            }
        }
    }

    /// Reclassification for a name, if configured.
    pub fn classified(&self, short_name: &str) -> Option<NodeKind> {
        self.classify.get(&short_name.to_ascii_lowercase()).copied()
    }

    /// Parse a TOML config document. Schema:
    ///
    /// ```toml
    /// scale_factor = 2.0
    /// standards = ["wcag", "hci-laws"]      # subset; omit = all
    /// disabled_standards = ["perception"]   # subtract from the set
    ///
    /// [rules.choice-count]
    /// severity = "warn"                     # off|info|warn|error|forbid
    /// max = 7
    ///
    /// [classify]
    /// "MyWidget" = "navigation"
    ///
    /// [[allow]]
    /// path = "App/Media/**"
    /// rules = ["color-budget", "standard:isa-101"]
    /// ```
    pub fn from_toml(text: &str) -> Result<Self, LintConfigError> {
        let doc: toml::Value =
            toml::from_str(text).map_err(|e| LintConfigError::Parse(e.to_string()))?;
        let mut cfg = LintConfig::new();

        if let Some(sf) = doc.get("scale_factor").and_then(|v| v.as_float()) {
            cfg.scale_factor = sf as f32;
        }
        if let Some(base) = doc.get("docs_base").and_then(|v| v.as_str()) {
            cfg.docs_base = base.trim_end_matches('/').to_string();
        }
        if let Some(stds) = doc.get("standards") {
            cfg.standards = parse_standards(stds)?;
        }
        if let Some(stds) = doc.get("disabled_standards") {
            for s in parse_standards(stds)? {
                cfg.standards.remove(&s);
            }
        }
        if let Some(rules) = doc.get("rules").and_then(|v| v.as_table()) {
            for (id, val) in rules {
                let table = val
                    .as_table()
                    .ok_or_else(|| LintConfigError::Parse(format!("rules.{id} must be a table")))?;
                let setting = cfg.rules.entry(id.clone()).or_default();
                for (k, v) in table {
                    match k.as_str() {
                        "severity" => {
                            let s = v.as_str().ok_or_else(|| {
                                LintConfigError::Parse(format!(
                                    "rules.{id}.severity must be a string"
                                ))
                            })?;
                            setting.severity = Some(Severity::from_key(s).ok_or_else(|| {
                                LintConfigError::Parse(format!(
                                    "rules.{id}: unknown severity {s:?}"
                                ))
                            })?);
                        }
                        _ => {
                            let num = v.as_float().or_else(|| v.as_integer().map(|i| i as f64));
                            setting.params.insert(
                                k.clone(),
                                num.ok_or_else(|| {
                                    LintConfigError::Parse(format!(
                                        "rules.{id}.{k} must be a number"
                                    ))
                                })?,
                            );
                        }
                    }
                }
            }
        }
        if let Some(classify) = doc.get("classify").and_then(|v| v.as_table()) {
            for (name, val) in classify {
                let kind_str = val.as_str().ok_or_else(|| {
                    LintConfigError::Parse(format!("classify.{name} must be a string"))
                })?;
                let kind = parse_kind(kind_str).ok_or_else(|| {
                    LintConfigError::Parse(format!("classify.{name}: unknown kind {kind_str:?}"))
                })?;
                cfg.classify.insert(name.to_ascii_lowercase(), kind);
            }
        }
        if let Some(allows) = doc.get("allow").and_then(|v| v.as_array()) {
            for entry in allows {
                let table = entry.as_table().ok_or_else(|| {
                    LintConfigError::Parse("allow entries must be tables".to_string())
                })?;
                let path = table.get("path").and_then(|v| v.as_str()).ok_or_else(|| {
                    LintConfigError::Parse("allow entry missing `path`".to_string())
                })?;
                let rules = table
                    .get("rules")
                    .and_then(|v| v.as_array())
                    .ok_or_else(|| {
                        LintConfigError::Parse("allow entry missing `rules`".to_string())
                    })?
                    .iter()
                    .map(|v| {
                        v.as_str().map(|s| s.to_ascii_lowercase()).ok_or_else(|| {
                            LintConfigError::Parse(
                                "allow.rules entries must be strings".to_string(),
                            )
                        })
                    })
                    .collect::<Result<Vec<_>, _>>()?;
                cfg.allows.push(PathAllow {
                    path: path.to_string(),
                    specs: rules,
                });
            }
        }
        Ok(cfg)
    }

    /// Load a config file — `None` when the file doesn't exist (an
    /// absent config means defaults, not an error).
    pub fn from_file(path: &std::path::Path) -> Result<Option<Self>, LintConfigError> {
        match std::fs::read_to_string(path) {
            Ok(text) => Self::from_toml(&text).map(Some),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(None),
            Err(e) => Err(LintConfigError::Io(e)),
        }
    }
}

/// Errors from [`LintConfig::from_toml`]/[`LintConfig::from_file`].
///
/// # Examples
///
/// ```
/// use martensite_design_lint::LintConfig;
///
/// assert!(LintConfig::from_toml("standards = [\"bogus\"]").is_err());
/// assert!(LintConfig::from_toml("scale_factor = 2.0").is_ok());
/// ```
#[derive(Debug)]
pub enum LintConfigError {
    /// The TOML document or a value within it is malformed.
    Parse(String),
    /// The config file could not be read.
    Io(std::io::Error),
}

impl std::fmt::Display for LintConfigError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            LintConfigError::Parse(m) => write!(f, "design-lint config: {m}"),
            LintConfigError::Io(e) => write!(f, "design-lint config: {e}"),
        }
    }
}

impl std::error::Error for LintConfigError {}

fn parse_standards(v: &toml::Value) -> Result<BTreeSet<Standard>, LintConfigError> {
    let arr = v
        .as_array()
        .ok_or_else(|| LintConfigError::Parse("standards must be an array".to_string()))?;
    arr.iter()
        .map(|item| {
            item.as_str()
                .and_then(Standard::from_key)
                .ok_or_else(|| LintConfigError::Parse(format!("unknown standard {item}")))
        })
        .collect()
}

fn parse_kind(s: &str) -> Option<NodeKind> {
    match s.to_ascii_lowercase().as_str() {
        "navigation" | "nav" => Some(NodeKind::Navigation),
        "interactive" | "control" => Some(NodeKind::Interactive),
        "content" => Some(NodeKind::Content),
        "container" => Some(NodeKind::Container),
        "chrome" => Some(NodeKind::Chrome),
        "unknown" => Some(NodeKind::Unknown),
        _ => None,
    }
}

/// `/`-separated glob match: `*` matches within a segment, `**`
/// matches zero or more whole segments.
fn glob_match(pattern: &str, path: &str) -> bool {
    let pat: Vec<&str> = pattern.split('/').collect();
    let segs: Vec<&str> = path.split('/').collect();
    glob_rec(&pat, &segs)
}

fn glob_rec(pat: &[&str], segs: &[&str]) -> bool {
    match (pat.first(), segs.first()) {
        (None, None) => true,
        (Some(&"**"), _) => {
            // `**` consumes zero or more segments.
            (0..=segs.len()).any(|skip| glob_rec(&pat[1..], &segs[skip..]))
        }
        (Some(p), Some(s)) => segment_match(p, s) && glob_rec(&pat[1..], &segs[1..]),
        _ => false,
    }
}

/// Within-segment match: `*` matches any substring (including empty).
fn segment_match(pat: &str, seg: &str) -> bool {
    if pat == "*" {
        return true;
    }
    if !pat.contains('*') {
        return pat.eq_ignore_ascii_case(seg);
    }
    let parts: Vec<&str> = pat.split('*').collect();
    let mut rest = seg;
    for (i, part) in parts.iter().enumerate() {
        if part.is_empty() {
            continue;
        }
        match rest.to_ascii_lowercase().find(&part.to_ascii_lowercase()) {
            Some(idx) if i == 0 && !pat.starts_with('*') && idx != 0 => return false,
            Some(idx) => rest = &rest[idx + part.len()..],
            None => return false,
        }
    }
    // Trailing literal must reach the end unless pattern ends in `*`.
    if !pat.ends_with('*') && !parts.last().is_none_or(|p| p.is_empty()) {
        if let Some(last) = parts.last() {
            return rest.is_empty() || rest.eq_ignore_ascii_case(last);
        }
    }
    true
}
