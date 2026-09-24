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
    /// NUREG-0700 — the NRC's Human-System Interface Design Review
    /// Guidelines for nuclear control rooms. The most quantified HSI
    /// standard in print: packing density ≤50% (≤25% for
    /// alphanumeric-dominant displays), minimized for critical
    /// information, and split/multi-page guidance when a display
    /// cannot be refined to fit.
    Nureg0700,
    /// FAA HFDS / FAA-CT-96-1 — Human Factors Design Standard and
    /// Design Guide for acquisition. Quantified screen economics:
    /// text-display character:blank ratio ≤60%, and the simultaneity
    /// norm — only information essential *at a given time* should be
    /// presented; related data belongs on one integrated display.
    FaaHfds,
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
        Standard::Nureg0700,
        Standard::FaaHfds,
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
            Standard::Nureg0700 => "nureg-0700",
            Standard::FaaHfds => "faa-hfds",
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
            "nureg-0700" | "nureg-700" | "nureg0700" | "nureg" => Some(Standard::Nureg0700),
            "faa-hfds" | "hfds" | "faa" | "faa-ct-96-1" => Some(Standard::FaaHfds),
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
            Standard::Nureg0700 => "NUREG-0700 HSI review guidelines (nuclear)",
            Standard::FaaHfds => "FAA HFDS/CT-96-1 display design economics",
        }
    }
}

impl fmt::Display for Standard {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.config_key())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn config_keys_round_trip() {
        for s in Standard::ALL {
            assert_eq!(
                Standard::from_key(s.config_key()),
                Some(*s),
                "config_key {} did not round-trip",
                s.config_key()
            );
        }
    }

    #[test]
    fn aliases_resolve() {
        assert_eq!(Standard::from_key("NUREG"), Some(Standard::Nureg0700));
        assert_eq!(Standard::from_key("nureg-700"), Some(Standard::Nureg0700));
        assert_eq!(Standard::from_key("hfds"), Some(Standard::FaaHfds));
        assert_eq!(Standard::from_key("faa-ct-96-1"), Some(Standard::FaaHfds));
        assert_eq!(Standard::from_key("nonsense"), None);
    }

    #[test]
    fn all_is_unique_and_described() {
        let mut seen = std::collections::HashSet::new();
        for s in Standard::ALL {
            assert!(seen.insert(s), "duplicate standard in ALL: {s}");
            assert!(!s.describe().is_empty());
            assert!(!s.config_key().is_empty());
        }
        assert_eq!(Standard::ALL.len(), 9);
    }
}
