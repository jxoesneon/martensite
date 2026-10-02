//! Configurable input sanitization for editable text surfaces.
//!
//! Every Martensite text-entry widget runs committed and pasted text
//! through a [`Sanitize`] pipeline before it lands in the model. The
//! widget-facing configuration is [`SanitizerConfig`]:
//!
//! * [`SanitizerConfig::Aggressive`] — the default, the most
//!   comprehensive built-in profile: NFKC normalization, stripping of
//!   C0/C1 controls, bidirectional overrides and isolates (the
//!   Trojan-Source vector), noncharacters, invisible math operators,
//!   interlinear annotation controls, tag-block characters, and the
//!   BOM; folding of ASCII-adjacent confusables (primes, modifier
//!   apostrophes); mapping of exotic Unicode spaces to `U+0020`.
//! * [`SanitizerConfig::Baseline`] — structural hygiene only: the
//!   control-character floor every field has always enforced.
//!   `widget.sanitize(false)` selects this.
//! * [`SanitizerConfig::Raw`] — fully verbatim: nothing is removed or
//!   rewritten, including newlines in single-line fields.
//!   `widget.raw()` selects this.
//! * [`SanitizerConfig::Custom`] — a caller-supplied [`Sanitize`]
//!   trait object replaces the built-in profile entirely
//!   (`widget.with_sanitizer(..)`).
//!
//! Profiles are built with [`Profile`], a builder over the aggressive
//! defaults — individual stages can be tightened or relaxed, and
//! pipeline order follows CERT IDS11 (normalization first, so a
//! folded character cannot hide inside a later strip set).
//!
//! # Deliberate keeps
//!
//! The aggressive profile preserves the *functional* invisible
//! characters `U+200D` ZWJ, `U+200C` ZWNJ, and `U+200B` ZWSP —
//! removing them destroys emoji sequences and breaks Indic, Arabic,
//! Persian, Thai, and Khmer shaping. [`Profile::strip_zw_chars`]
//! tightens this for hostile-input contexts. Variation selectors are
//! likewise kept (they select emoji/text presentation); see
//! [`Profile::strip_variation_selectors`]. Directional marks
//! `U+200E`/`U+200F` are kept because legitimate RTL text needs them;
//! the stateful override/isolate characters are always stripped.
//!
//! # Examples
//!
//! ```
//! use martensite_sanitize::{Phase, SanitizeContext, SanitizerConfig};
//!
//! let cfg = SanitizerConfig::Aggressive;
//! let ctx = SanitizeContext::single_line(Phase::Insert);
//! assert_eq!(cfg.sanitize("  hi\u{202e}txt  ", &ctx), "  hitxt  ");
//! let raw = SanitizerConfig::Raw;
//! assert_eq!(raw.sanitize("a\nb\x00", &ctx), "a\nb\x00");
//! ```

mod profile;
mod tables;

pub use profile::{Profile, ProfileBuilder};
pub use tables::ConfusableFold;

use std::sync::Arc;

/// Which stage of editing is being sanitized.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum Phase {
    /// Text arriving from the user — IME commits, pastes, drags.
    /// Insert-time sanitization handles structure: control
    /// characters, invisibles, and the newline policy.
    #[default]
    Insert,
    /// The widget's commit seam — Enter, blur, send. The commit pass
    /// applies policy-level finishing (a final sanitize plus
    /// [`ProfileBuilder::trim_on_commit`] when enabled).
    Commit,
}

/// Per-call context handed to a [`Sanitize`] implementation.
#[derive(Clone, Copy, Debug, Default)]
pub struct SanitizeContext {
    /// The field holds a single line — line and paragraph separators
    /// (`\n`, `\r`, `U+2028`, `U+2029`, `U+0085`) are removed rather
    /// than preserved.
    pub single_line: bool,
    /// The pipeline stage.
    pub phase: Phase,
}

impl SanitizeContext {
    /// Context for a single-line field (`TextInput` and friends).
    pub fn single_line(phase: Phase) -> Self {
        Self {
            single_line: true,
            phase,
        }
    }

    /// Context for a multiline field (`TextArea` and friends).
    pub fn multi_line(phase: Phase) -> Self {
        Self {
            single_line: false,
            phase,
        }
    }
}

/// A sanitization rule — pluggable per input surface.
///
/// Implementations are shared behind `Arc` so widgets stay `Clone`.
/// `sanitize` receives the candidate text and returns the stored
/// text; it is invoked at insert time (IME commit, paste) and again
/// at the widget's commit seam with [`Phase::Commit`].
pub trait Sanitize: Send + Sync {
    /// Stable rule name for diagnostics and introspection.
    fn name(&self) -> &'static str;

    /// Transforms `input` under `ctx`. Always returns valid UTF-8
    /// (guaranteed by `&str`/`String` in Rust).
    fn sanitize(&self, input: &str, ctx: &SanitizeContext) -> String;
}

/// How a widget sanitizes its text.
#[derive(Clone, Default)]
pub enum SanitizerConfig {
    /// The default — the most comprehensive built-in profile
    /// (see the crate docs).
    #[default]
    Aggressive,
    /// Structural hygiene only — `sanitize(false)`: the
    /// control-character floor every field has always enforced. Text
    /// is not normalized and invisibles are kept.
    Baseline,
    /// Fully verbatim — `raw()`: nothing is removed, normalized, or
    /// rewritten, including newlines in single-line fields.
    Raw,
    /// A caller-supplied rule replaces the built-in profile.
    Custom(Arc<dyn Sanitize>),
}

impl SanitizerConfig {
    /// A custom rule — `widget.with_sanitizer(..)` wraps it here.
    pub fn custom(rule: Arc<dyn Sanitize>) -> Self {
        Self::Custom(rule)
    }

    /// Runs the configured pipeline over `input`.
    pub fn sanitize(&self, input: &str, ctx: &SanitizeContext) -> String {
        match self {
            Self::Aggressive => Profile::AGGRESSIVE.sanitize(input, ctx),
            Self::Baseline => Profile::BASELINE.sanitize(input, ctx),
            Self::Raw => input.to_string(),
            Self::Custom(rule) => rule.sanitize(input, ctx),
        }
    }

    /// `true` when the raw passthrough is selected.
    pub fn is_raw(&self) -> bool {
        matches!(self, Self::Raw)
    }

    /// `true` when a caller-supplied rule is selected.
    pub fn is_custom(&self) -> bool {
        matches!(self, Self::Custom(_))
    }
}

impl std::fmt::Debug for SanitizerConfig {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Aggressive => f.write_str("SanitizerConfig::Aggressive"),
            Self::Baseline => f.write_str("SanitizerConfig::Baseline"),
            Self::Raw => f.write_str("SanitizerConfig::Raw"),
            Self::Custom(rule) => write!(f, "SanitizerConfig::Custom({})", rule.name()),
        }
    }
}
