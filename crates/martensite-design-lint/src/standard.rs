//! The standards catalog — the named bodies of guidance rules cite.
//!
//! Each [`Standard`] is a selectable bundle: [`LintConfig`] includes
//! every standard by default and can narrow to any subset. Every
//! [`Finding`](crate::Finding) names the standard(s) its rule belongs
//! to so the report teaches the source, not just the symptom.

use std::fmt;

/// A named body of design guidance that rules cite and configs select.
///
/// Standards are deliberately coarse — rules map to the *authority*
/// behind their threshold, so a project can enable e.g. WCAG + ISA-101
/// for an industrial product while skipping perception research it
/// doesn't need.
///
/// # Examples
///
/// ```
/// use martensite_design_lint::Standard;
///
/// assert_eq!(Standard::Wcag.config_key(), "wcag");
/// assert_eq!(Standard::from_key("isa-101"), Some(Standard::Isa101));
/// ```
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Standard {
    /// W3C WCAG 2.2 — target size (2.5.8), non-text contrast (1.4.11),
    /// reflow (1.4.10), color-independence (1.4.1).
    Wcag,
    /// ANSI/ISA-101 — High-Performance HMI: display hierarchy (L1–L4
    /// progressive disclosure), color discipline (saturated color
    /// reserved for abnormal states), no gratuitous animation.
    Isa101,
    /// ANSI/ISA-18.2 — alarm management: simultaneous-alert budgets,
    /// alarm-rate and priority-distribution analogs applied at design
    /// time.
    Isa182,
    /// HCI laws — Hick/Hyman (choice count), Miller/Cowan (working
    /// memory chunks), Fitts (target geometry). Quantified decision
    /// and movement cost.
    HciLaws,
    /// Information design — Tufte's data-ink ratio, Few's dashboard
    /// canon, Gestalt grouping, Nielsen's heuristics.
    InfoDesign,
    /// Perceptual metrics — Miniukovich & De Angeli's interface
    /// aesthetics measures (alignment, density, balance) and
    /// Rosenholtz's clutter research (feature congestion, edge
    /// density).
    Perception,
    /// Internal consistency — token discipline, typographic scale,
    /// repeated-pattern regularity. The "design system conformance"
    /// layer every mature lint ecosystem converges on.
    Consistency,
}

impl Standard {
    /// Every standard in the catalog, in stable order.
    pub const ALL: &'static [Standard] = &[
        Standard::Wcag,
        Standard::Isa101,
        Standard::Isa182,
        Standard::HciLaws,
        Standard::InfoDesign,
        Standard::Perception,
        Standard::Consistency,
    ];

    /// The lowercase key used in config files and `standard:` allow
    /// specifiers — e.g. `"isa-101"`.
    pub fn config_key(self) -> &'static str {
        match self {
            Standard::Wcag => "wcag",
            Standard::Isa101 => "isa-101",
            Standard::Isa182 => "isa-18-2",
            Standard::HciLaws => "hci-laws",
            Standard::InfoDesign => "info-design",
            Standard::Perception => "perception",
            Standard::Consistency => "consistency",
        }
    }

    /// Parse a [`config_key`](Self::config_key) back into a standard.
    /// Accepts a few friendly aliases (`isa101`, `isa-18.2`, `hci`).
    pub fn from_key(key: &str) -> Option<Self> {
        match key.to_ascii_lowercase().as_str() {
            "wcag" | "wcag-2" | "wcag-2.2" => Some(Standard::Wcag),
            "isa-101" | "isa101" | "isa_101" => Some(Standard::Isa101),
            "isa-18-2" | "isa-18.2" | "isa182" | "isa_18_2" => Some(Standard::Isa182),
            "hci-laws" | "hci" | "laws" => Some(Standard::HciLaws),
            "info-design" | "tufte" | "few" | "infodesign" => Some(Standard::InfoDesign),
            "perception" | "aesthetics" | "clutter" => Some(Standard::Perception),
            "consistency" | "tokens" => Some(Standard::Consistency),
            _ => None,
        }
    }

    /// One-line description used in `--help`-style output and report
    /// headers.
    pub fn describe(self) -> &'static str {
        match self {
            Standard::Wcag => "WCAG 2.2 accessibility floors",
            Standard::Isa101 => "ANSI/ISA-101 High-Performance HMI",
            Standard::Isa182 => "ANSI/ISA-18.2 alarm-management analogs",
            Standard::HciLaws => "Hick/Miller/Fitts decision & movement cost",
            Standard::InfoDesign => "Tufte/Few/Gestalt/Nielsen information design",
            Standard::Perception => "Miniukovich/Rosenholtz perceptual metrics",
            Standard::Consistency => "design-system token & type discipline",
        }
    }
}

impl fmt::Display for Standard {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.config_key())
    }
}
