//! Severity levels for lint findings.

use std::fmt;

/// How loudly a rule reports a violation.
///
/// `Off` disables the rule entirely — this is the per-rule override
/// mechanism alongside whole-standard selection and path allows.
/// `Forbid` (rustc's model) is a severity no `allow` can suppress —
/// for non-negotiable rules like an unreachable primary action.
///
/// # Examples
///
/// ```
/// use martensite_design_lint::Severity;
///
/// assert!(Severity::Error > Severity::Warn);
/// assert_eq!(Severity::from_key("off"), Some(Severity::Off));
/// assert_eq!(Severity::from_key("forbid"), Some(Severity::Forbid));
/// ```
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Severity {
    /// Rule produces no findings.
    Off,
    /// Reported for awareness only — never fails a build.
    Info,
    /// Reported as a warning — the default for guidance rules.
    Warn,
    /// Reported as an error — intended for CI gating.
    Error,
    /// An error that path allows and inline `@lint:` markers cannot
    /// suppress. Use sparingly — for rules where a "legitimate
    /// exception" does not exist.
    Forbid,
}

impl Severity {
    /// Parse a config-file severity key (`"off"`, `"info"`, `"warn"`,
    /// `"error"`, `"forbid"` — `"warning"`/`"deny"`/`"allow"`
    /// accepted as aliases).
    pub fn from_key(key: &str) -> Option<Self> {
        match key.to_ascii_lowercase().as_str() {
            "off" | "allow" => Some(Severity::Off),
            "info" | "note" => Some(Severity::Info),
            "warn" | "warning" => Some(Severity::Warn),
            "error" | "deny" => Some(Severity::Error),
            "forbid" | "fatal" => Some(Severity::Forbid),
            _ => None,
        }
    }

    /// The config-file key for this level.
    pub fn config_key(self) -> &'static str {
        match self {
            Severity::Off => "off",
            Severity::Info => "info",
            Severity::Warn => "warn",
            Severity::Error => "error",
            Severity::Forbid => "forbid",
        }
    }
}

impl fmt::Display for Severity {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.config_key())
    }
}
