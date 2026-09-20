//! On-screen compliance audit for [`PaintList`](martensite_core::PaintList)
//! command streams.
//!
//! Widgets declare intent; the paint list is what is actually drawn. This
//! pass inspects the recorded command stream and reports WCAG 2.2
//! violations that a widget tree cannot see on its own:
//!
//! - **Undersized text** — `DrawText` and
//!   `DrawGlyphRun` commands whose font size, converted to
//!   logical points through the display scale factor, falls below
//!   [`PaintAuditConfig::min_text_size_pt`](crate::paint_audit::PaintAuditConfig::min_text_size_pt).
//! - **Insufficient text contrast** — text whose color fails the Criterion
//!   1.4.3 ratio against the backdrop actually painted beneath it. The
//!   backdrop is resolved by walking the command stream backwards from the
//!   text command and compositing every containing solid fill until an
//!   opaque surface is found. Gradient backdrops cannot be reduced to a
//!   single color and are reported as [`PaintLintKind::UnknownBackdrop`]
//!   when [`PaintAuditConfig::report_unknown_backdrop`] is enabled.
//! - **Non-text contrast (Criterion 1.4.11)** — stroked outlines
//!   (`StrokeRect`, `StrokePath`), which typically carry control borders
//!   and focus indicators, must reach 3:1 against their backdrop.
//! - **Bounds** — text extending horizontally past its clip container
//!   reports [`PaintLintKind::ClipOverflow`] (advisory — clipping is
//!   often intentional); text whose *visible* region extends past the
//!   configured [`PaintAuditConfig::frame`] reports
//!   [`PaintLintKind::OutOfFrame`]. Visibility and bounds checks test
//!   `bounds ∩ clip`, so pixels clipped away by design never count.
//! - **Positions** — every lint carries an `anchor` point and the
//!   detail string ends with `@ (x, y)` in device px so findings can be
//!   mapped back to the offending element without a debugger.
//! - **Invisible text** — text painted fully outside the active clip
//!   region, or covered by a later opaque fill. Both are usually bugs:
//!   z-order mistakes or positioning errors that produce no output.
//! - **Overlapping text** — two text runs whose ink boxes intersect,
//!   the signature of a layout/spacing collision.
//! - **Provenance** — [`PaintCommand::PushScope`]/`PopScope` markers
//!   emitted by the arena's paint walker let every lint name the widget
//!   that produced it (`PaintLint::scope`, `in <name>` in the detail).
//!   The scope bounds also power the **container-overflow** check:
//!   text whose visible region escapes its own widget's scope rect
//!   reports [`PaintLintKind::WidgetOverflow`] — a real paint leak,
//!   distinct from intentional clipping.
//! - **Focus indicator (Criterion 2.4.7)** — when the app reports the
//!   focused widget's rect via [`PaintAuditConfig::focus_rect`], the
//!   audit requires at least one painted stroke to intersect it and
//!   reports [`PaintLintKind::MissingFocusIndicator`] otherwise.
//! - **Target size (Criterion 2.5.8)** — [`audit_target_sizes`] walks
//!   the arena directly (the paint stream cannot see hit regions) and
//!   reports interactive nodes under 24×24pt as
//!   [`PaintLintKind::UndersizedTarget`].
//! - **Locale coverage (opt-in)** — when the app installs
//!   [`PaintAuditConfig::locale_probe`], every visible
//!   [`PaintCommand::DrawText`] string the probe doesn't recognize as
//!   translated reports [`PaintLintKind::MissingLocale`] (advisory).
//!   `DrawGlyphRun` output carries no source text and is skipped; the
//!   probe itself exempts intentionally unlocalized values (dynamic
//!   data, endonyms).
//! - **Machine-readable output** — [`PaintLint::to_json`] serializes a
//!   finding for CI tooling and baseline files.
//!
//! [`PaintLintKind::WidgetOverflow`]: crate::paint_audit::PaintLintKind::WidgetOverflow
//! [`PaintLintKind::MissingFocusIndicator`]: crate::paint_audit::PaintLintKind::MissingFocusIndicator
//! [`PaintLintKind::UndersizedTarget`]: crate::paint_audit::PaintLintKind::UndersizedTarget
//! [`PaintAuditConfig::focus_rect`]: crate::paint_audit::PaintAuditConfig::focus_rect
//! [`PaintLint::to_json`]: crate::paint_audit::PaintLint::to_json
//! [`PaintLint::scope`]: crate::paint_audit::PaintLint::scope
//! [`PaintLint::widget`]: crate::paint_audit::PaintLint::widget
//! [`audit_target_sizes`]: crate::paint_audit::audit_target_sizes
//! [`PaintCommand::PushScope`]: martensite_core::PaintCommand::PushScope
//! [`PaintLintKind::UnknownBackdrop`]: crate::paint_audit::PaintLintKind::UnknownBackdrop
//! [`PaintAuditConfig::report_unknown_backdrop`]: crate::paint_audit::PaintAuditConfig::report_unknown_backdrop
//! [`PaintLintKind::ClipOverflow`]: crate::paint_audit::PaintLintKind::ClipOverflow
//! [`PaintLintKind::OutOfFrame`]: crate::paint_audit::PaintLintKind::OutOfFrame
//! [`PaintAuditConfig::frame`]: crate::paint_audit::PaintAuditConfig::frame
//! [`PaintAuditConfig::locale_probe`]: crate::paint_audit::PaintAuditConfig::locale_probe
//! [`PaintCommand::DrawText`]: martensite_core::PaintCommand::DrawText
//! [`PaintLintKind::MissingLocale`]: crate::paint_audit::PaintLintKind::MissingLocale
//!
//! The audit is advisory — it reports, it never blocks rendering. Font
//! sizes in a paint list are device pixels; 1pt is approximated as one
//! logical pixel (`device_px / scale_factor`), matching CSS px semantics
//! used by the WCAG thresholds. Bold-face detection is not available at
//! the paint level, so the large-text classification uses the 18pt
//! regular-weight threshold only.
//!
//! # Examples
//!
//! ```
//! use martensite_access::paint_audit::{audit_paint_list, PaintAuditConfig, PaintLintKind};
//! use kurbo::{Point, Rect};
//! use martensite_core::PaintList;
//!
//! let mut list = PaintList::new();
//! list.push_fill_rect(Rect::new(0.0, 0.0, 200.0, 100.0), [255, 255, 255, 255]);
//! list.push_text(
//!     Point::new(10.0, 10.0),
//!     "tiny".to_string(),
//!     8.0,
//!     [0, 0, 0, 255],
//! );
//!
//! let lints = audit_paint_list(&list, &PaintAuditConfig::default());
//! assert!(lints.iter().any(|l| l.kind == PaintLintKind::UndersizedText));
//! ```

use std::collections::HashSet;

use kurbo::{Point, Rect, Shape};
use martensite_core::paint::{GlyphRun, PaintCommand, PaintList};
use martensite_core::NodeFlags;
use tracing::{info, warn};

use crate::compliance::{contrast_ratio, ColorRgba, TextSize, WcagLevel};

/// Default minimum text size in logical points. WCAG 2.2 defines no hard
/// minimum font size; 12pt matches the lowest comfortable-reading floor
/// common to Apple HIG (~11pt) and Material (12sp) guidance.
pub const DEFAULT_MIN_TEXT_SIZE_PT: f32 = 12.0;

/// WCAG 2.2 large-text threshold in points (regular weight; the bold
/// 14pt alternative cannot be detected at the paint level).
pub const LARGE_TEXT_PT: f32 = 18.0;

/// Configuration for [`audit_paint_list`].
///
/// # Examples
///
/// ```
/// use martensite_access::paint_audit::PaintAuditConfig;
/// use martensite_access::compliance::WcagLevel;
///
/// let cfg = PaintAuditConfig::default().with_scale_factor(2.0);
/// assert_eq!(cfg.scale_factor, 2.0);
/// assert_eq!(cfg.level, WcagLevel::Aa);
/// ```
#[derive(Debug, Clone)]
pub struct PaintAuditConfig {
    /// Display scale factor converting device pixels to logical points.
    pub scale_factor: f32,
    /// WCAG conformance level used for contrast thresholds.
    pub level: WcagLevel,
    /// Minimum acceptable text size in logical points.
    pub min_text_size_pt: f32,
    /// Report [`PaintLintKind::UnknownBackdrop`] informational lints for
    /// text whose backdrop cannot be resolved to a solid color (gradient,
    /// external surface, or no containing fill at all).
    pub report_unknown_backdrop: bool,
    /// Check stroked shapes — typically control borders and focus
    /// indicators — against the WCAG 1.4.11 non-text contrast
    /// requirement (3:1 at AA/AAA alike).
    pub check_ui_component_contrast: bool,
    /// Flag text that paints nothing the user can read: fully outside
    /// the active clip region, or covered by a later opaque fill.
    pub check_text_visibility: bool,
    /// Flag text runs whose ink boxes overlap — usually a layout
    /// collision where two strings render on top of each other.
    pub check_text_overlap: bool,
    /// Flag elements whose ink boxes exceed their container: text
    /// extending horizontally past its active clip region, or any text
    /// painted partially outside [`frame`](Self::frame). Vertical clip
    /// overflow is deliberately not flagged — scroll containers clip
    /// mid-row at top and bottom as part of normal operation.
    pub check_bounds: bool,
    /// The drawable frame in device pixels (usually the surface size).
    /// When set, text whose bounds extend beyond it is reported as
    /// [`PaintLintKind::OutOfFrame`]. `None` disables that check.
    pub frame: Option<Rect>,
    /// Flag text whose *visible* region (bounds ∩ clip) extends past
    /// its own widget's `PushScope` bounds — content escaping its
    /// container even after clipping is accounted for. Zero-area
    /// scope bounds (widget not yet laid out) are skipped.
    pub check_container_overflow: bool,
    /// When [`focus_rect`](Self::focus_rect) is set, require at least
    /// one stroked shape intersecting it — the focused widget should
    /// show a visible indicator (WCAG 2.4.7). `None` disables.
    ///
    /// Limitation: this verifies *presence* of emphasis, not intent —
    /// any stroke crossing the rect satisfies it, including a panel
    /// border that happens to overlap the focused widget's bounds.
    /// The 1.4.11 contrast check still verifies the stroke is visible.
    pub check_focus_indicator: bool,
    /// Bounds of the currently-focused widget in device pixels, fed by
    /// the app each frame (e.g. via
    /// `RenderOrchestrator::set_audit_focus_rect`). In practice the
    /// focused widget's own border usually serves as the indicator, so
    /// false passes are rare — but a widget with no emphasis at all is
    /// caught reliably, which is the failure this check exists for.
    pub focus_rect: Option<Rect>,
    /// Optional predicate answering "does this painted string have a
    /// locale translation". When set, every [`PaintCommand::DrawText`]
    /// string the probe rejects is reported as
    /// [`PaintLintKind::MissingLocale`]; `None` (default) disables the
    /// check entirely.
    ///
    /// Only `DrawText` carries source text — shaped
    /// [`PaintCommand::DrawGlyphRun`] output is not probeable and is
    /// skipped. The probe is also the opt-out seam for intentionally
    /// non-localized strings: return `true` for dynamic data (numbers,
    /// units, IDs), endonyms, brand names, and other non-user-facing
    /// values. Strings that paint nothing (empty, whitespace-only, or
    /// fully outside their clip) are never probed.
    pub locale_probe: Option<LocaleProbe>,
}

impl Default for PaintAuditConfig {
    fn default() -> Self {
        Self {
            scale_factor: 1.0,
            level: WcagLevel::Aa,
            min_text_size_pt: DEFAULT_MIN_TEXT_SIZE_PT,
            report_unknown_backdrop: false,
            check_ui_component_contrast: true,
            check_text_visibility: true,
            check_text_overlap: true,
            check_bounds: true,
            frame: None,
            check_container_overflow: true,
            check_focus_indicator: true,
            focus_rect: None,
            locale_probe: None,
        }
    }
}

impl PaintAuditConfig {
    /// Sets the display scale factor (physical pixels per logical point).
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite_access::paint_audit::PaintAuditConfig;
    ///
    /// let cfg = PaintAuditConfig::default().with_scale_factor(1.5);
    /// assert_eq!(cfg.scale_factor, 1.5);
    /// ```
    #[must_use]
    pub fn with_scale_factor(mut self, scale_factor: f64) -> Self {
        self.scale_factor = scale_factor as f32;
        self
    }
}

/// The predicate a [`LocaleProbe`] wraps: `(text, scope) ->
/// has_translation`.
pub type LocaleProbeFn = dyn Fn(&str, Option<&'static str>) -> bool + Send + Sync;

/// Predicate deciding whether a painted string has a locale translation
/// — the opt-in seam behind [`PaintAuditConfig::locale_probe`] and
/// [`PaintLintKind::MissingLocale`].
///
/// The probe receives the full source text of a
/// [`PaintCommand::DrawText`] command plus the innermost widget-paint
/// scope name (`Widget::debug_name`, `None` outside any scope). Return
/// `true` when the string is covered by the localization system — or
/// intentionally exempt (dynamic data, endonyms, brand names); `false`
/// reports the lint.
///
/// # Examples
///
/// ```
/// use martensite_access::paint_audit::LocaleProbe;
///
/// let probe = LocaleProbe::new(|text, _scope| text == "OK");
/// assert!(probe.is_translated("OK", None));
/// assert!(!probe.is_translated("Cancel", None));
/// ```
#[derive(Clone)]
pub struct LocaleProbe(std::sync::Arc<LocaleProbeFn>);

impl LocaleProbe {
    /// Wraps a `(text, scope) -> has_translation` predicate.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite_access::paint_audit::LocaleProbe;
    ///
    /// let probe = LocaleProbe::new(|text, scope| scope == Some("Data"));
    /// assert!(probe.is_translated("anything", Some("Data")));
    /// assert!(!probe.is_translated("anything", Some("Chrome")));
    /// ```
    #[must_use]
    pub fn new(f: impl Fn(&str, Option<&'static str>) -> bool + Send + Sync + 'static) -> Self {
        Self(std::sync::Arc::new(f))
    }

    /// Whether `text` painted inside `scope` has a locale translation
    /// (or is intentionally exempt).
    #[must_use]
    pub fn is_translated(&self, text: &str, scope: Option<&'static str>) -> bool {
        (self.0)(text, scope)
    }
}

impl std::fmt::Debug for LocaleProbe {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("LocaleProbe(..)")
    }
}

/// Severity of a [`PaintLint`].
///
/// # Examples
///
/// ```
/// use martensite_access::paint_audit::LintSeverity;
///
/// assert!(LintSeverity::Warning > LintSeverity::Info);
/// ```
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum LintSeverity {
    /// Informational — a check could not be performed (e.g. the backdrop
    /// under a text run is not a resolvable solid color).
    Info,
    /// A measurable standard is violated.
    Warning,
}

/// The kind of non-compliance a [`PaintLint`] reports.
///
/// # Examples
///
/// ```
/// use martensite_access::paint_audit::PaintLintKind;
///
/// assert_ne!(PaintLintKind::UndersizedText, PaintLintKind::InsufficientContrast);
/// ```
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum PaintLintKind {
    /// Text smaller than [`PaintAuditConfig::min_text_size_pt`] once the
    /// scale factor is applied.
    UndersizedText,
    /// Text/background contrast below the WCAG threshold for its size
    /// class and configured [`WcagLevel`].
    InsufficientContrast,
    /// The backdrop beneath the text could not be resolved to a solid
    /// color, so contrast could not be verified.
    UnknownBackdrop,
    /// A stroked outline (control border, focus ring) fails the WCAG
    /// 1.4.11 non-text contrast requirement of 3:1 against its backdrop.
    NonTextContrast,
    /// Text was painted but a later opaque fill covers it — invisible
    /// output and usually a z-order bug.
    OccludedText,
    /// Text lies fully outside the active clip region and paints
    /// nothing — wasted work or a positioning bug.
    ClippedText,
    /// Two text runs' ink boxes intersect — strings render on top of
    /// each other, typically a spacing/layout collision.
    TextOverlap,
    /// Text extends horizontally past its active clip region — its
    /// width exceeds the container it was laid out for. Advisory
    /// ([`LintSeverity::Info`]): scroll containers clip deliberately,
    /// and the paint stream cannot tell intent.
    ClipOverflow,
    /// Text extends partially outside the configured frame — at best
    /// wasted work, usually a layout that does not fit the window.
    OutOfFrame,
    /// Text's visible region (bounds ∩ clip) extends past its own
    /// widget's scope bounds — content painting outside its container
    /// with no clip to constrain it. The semantic form of
    /// [`PaintLintKind::ClipOverflow`]: clipping inside a container is
    /// intentional; leaking past the container's bounds is a bug.
    WidgetOverflow,
    /// An interactive node's bounds are smaller than the WCAG 2.5.8
    /// target-size minimum (24×24 CSS px). Emitted by
    /// [`audit_target_sizes`], an arena-level pass — the paint stream
    /// cannot see hit regions.
    UndersizedTarget,
    /// `config.focus_rect` was set but no stroke or edge emphasis was
    /// painted inside it — the focused widget shows no visible
    /// indicator (WCAG 2.4.7 focus visible).
    MissingFocusIndicator,
    /// A user-visible string has no locale translation, per the
    /// configured [`PaintAuditConfig::locale_probe`]. Advisory
    /// ([`LintSeverity::Info`]) — localization coverage is opt-in and
    /// the probe decides which strings are intentionally unlocalized.
    MissingLocale,
    /// A node's allocated bounds are smaller than its declared
    /// [`RenderMinimum`](martensite_core::RenderMinimum) and no engaged
    /// [`UnderflowPolicy`](martensite_core::UnderflowPolicy) is handling
    /// the shortfall. Emitted by [`audit_underflow`], an arena-level
    /// pass — the paint stream cannot see declared minimums.
    Underflow,
}

/// A single compliance finding against a painted element.
#[derive(Debug, Clone)]
pub struct PaintLint {
    /// What was violated.
    pub kind: PaintLintKind,
    /// Report severity.
    pub severity: LintSeverity,
    /// Approximate center of the offending element, in device pixels —
    /// usable for devtools overlays.
    pub anchor: (f64, f64),
    /// Measured value: logical font size in points for
    /// [`PaintLintKind::UndersizedText`], contrast ratio for
    /// [`PaintLintKind::InsufficientContrast`] and
    /// [`PaintLintKind::NonTextContrast`].
    pub measured: Option<f32>,
    /// Required threshold for `measured`.
    pub required: Option<f32>,
    /// Human-readable detail line, suitable for `tracing` output.
    pub detail: String,
    /// The innermost widget-paint scope the offending command was
    /// emitted in (`Widget::debug_name`) — which component to look at.
    /// `None` for commands outside any scope (app-level chrome) and for
    /// lints that name a widget directly in `detail` instead.
    pub scope: Option<&'static str>,
    /// The arena handle of the emitting widget when the offending
    /// command ran inside an arena-emitted scope — for tooling that
    /// wants to correlate a lint back to the live tree (devtools
    /// "select the offending widget"). `None` for manual scopes and
    /// non-widget findings.
    pub widget: Option<martensite_core::WidgetId>,
}

impl PaintLint {
    /// One JSON object describing the lint — for CI tooling and
    /// baseline files. Keys: `kind`, `severity`, `scope`, `widget`,
    /// `anchor`, `measured`, `required`, `detail`. Non-finite floats
    /// serialize as `null` (JSON has no NaN/Infinity).
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite_access::paint_audit::{LintSeverity, PaintLint, PaintLintKind};
    ///
    /// let lint = PaintLint {
    ///     kind: PaintLintKind::UndersizedText,
    ///     severity: LintSeverity::Warning,
    ///     anchor: (10.0, 20.0),
    ///     measured: Some(9.0),
    ///     required: Some(12.0),
    ///     detail: "tiny text".to_string(),
    ///     scope: Some("Panel"),
    ///     widget: None,
    /// };
    /// assert!(lint.to_json().contains("\"kind\":\"UndersizedText\""));
    /// ```
    pub fn to_json(&self) -> String {
        // Escape for a JSON string literal: backslash and quote first,
        // then the control characters JSON forbids raw (< 0x20).
        fn esc(s: &str) -> String {
            let mut out = String::with_capacity(s.len() + 8);
            for c in s.chars() {
                match c {
                    '\\' => out.push_str("\\\\"),
                    '"' => out.push_str("\\\""),
                    '\n' => out.push_str("\\n"),
                    '\r' => out.push_str("\\r"),
                    '\t' => out.push_str("\\t"),
                    c if (c as u32) < 0x20 => out.push_str(&format!("\\u{:04x}", c as u32)),
                    c => out.push(c),
                }
            }
            out
        }
        let scope = self
            .scope
            .map_or("null".to_string(), |s| format!("\"{}\"", esc(s)));
        // JSON has no NaN/Infinity — non-finite measurements serialize
        // as null rather than producing unparseable output.
        let num = |v: Option<f32>| {
            v.filter(|v| v.is_finite())
                .map_or("null".to_string(), |v| format!("{v:.3}"))
        };
        let coord = |v: f64| {
            if v.is_finite() {
                format!("{v:.1}")
            } else {
                "null".to_string()
            }
        };
        let widget = self.widget.map_or("null".to_string(), |w| {
            format!(
                "{{\"slot\":{},\"generation\":{}}}",
                w.slot_idx(),
                w.generation()
            )
        });
        format!(
            "{{\"kind\":\"{:?}\",\"severity\":\"{:?}\",\"scope\":{scope},\"widget\":{widget},\"anchor\":[{},{}],\"measured\":{},\"required\":{},\"detail\":\"{}\"}}",
            self.kind,
            self.severity,
            coord(self.anchor.0),
            coord(self.anchor.1),
            num(self.measured),
            num(self.required),
            esc(&self.detail)
        )
    }
}

impl PaintLint {
    /// Stable identity for deduplication: same kind at the same anchor
    /// with the same detail hashes equal. Digits are excluded from the
    /// detail hash — measured values (px overflow, contrast ratios)
    /// jitter frame to frame and would mint a fresh fingerprint for
    /// what is conceptually the same finding.
    fn fingerprint(&self) -> u64 {
        use std::hash::{Hash, Hasher};
        let mut h = std::collections::hash_map::DefaultHasher::new();
        self.kind.hash(&mut h);
        // Scope matters: the same finding in two different widgets is
        // two findings, not one.
        self.scope.hash(&mut h);
        self.widget.hash(&mut h);
        ((self.anchor.0 * 4.0) as i64).hash(&mut h);
        ((self.anchor.1 * 4.0) as i64).hash(&mut h);
        for c in self.detail.chars().filter(|c| !c.is_ascii_digit()) {
            c.hash(&mut h);
        }
        h.finish()
    }
}

/// The resolved solid backdrop under a point, or the reason it could not
/// be resolved.
enum Backdrop {
    /// A composited color with effective alpha ≥ ~1.0.
    Resolved(ColorRgba),
    /// No resolvable opaque surface — gradient fill, or the fill stack
    /// never reached full coverage.
    Unknown,
}

/// Converts an `[u8; 4]` paint color to [`ColorRgba`].
fn rgba_u8(c: [u8; 4]) -> ColorRgba {
    ColorRgba::new(
        c[0] as f32 / 255.0,
        c[1] as f32 / 255.0,
        c[2] as f32 / 255.0,
        c[3] as f32 / 255.0,
    )
}

/// Converts a `[f32; 4]` paint color to [`ColorRgba`].
fn rgba_f32(c: [f32; 4]) -> ColorRgba {
    ColorRgba::new(c[0], c[1], c[2], c[3])
}

/// Point-in-rect for kurbo rects.
fn contains(rect: &Rect, p: (f64, f64)) -> bool {
    rect.contains(p)
}

/// Point-in-rect for the `[x, y, w, h]` array form used by
/// [`PaintCommand::BlurredRect`].
fn contains4(rect: [f32; 4], p: (f64, f64)) -> bool {
    let (x, y) = (p.0 as f32, p.1 as f32);
    x >= rect[0] && y >= rect[1] && x < rect[0] + rect[2] && y < rect[1] + rect[3]
}

/// One fill command's audit record — its coverage shape, color, and
/// the clip that was active when it was emitted. A fill clipped away
/// from a point never rendered there, so both checks must pass.
struct FillRec {
    /// Command index in the paint list (ordering).
    idx: usize,
    shape: FillShape,
    color: ColorRgba,
    clip: Option<Rect>,
    /// Emitted inside an `"Overlay"` provenance scope — popup fills
    /// *intentionally* occlude content beneath them, so they don't
    /// count as accidental coverers of non-overlay text.
    in_overlay: bool,
}

/// One text command's audit record — command index (ordering), the
/// probe geometry, the clip and innermost scope active at emit time,
/// and whether it sits inside an `"Overlay"` provenance scope.
struct TextRec {
    idx: usize,
    probe: TextProbe,
    clip: Option<Rect>,
    scope: Option<ScopeRec>,
    /// Emitted inside an `"Overlay"` scope — popup text intentionally
    /// sits above page content, so cross-layer overlap is not a lint.
    in_overlay: bool,
}

/// One active widget-paint scope: name plus the widget's layout bounds.
#[derive(Clone, Copy)]
struct ScopeRec {
    id: Option<martensite_core::WidgetId>,
    name: &'static str,
    bounds: Rect,
}

/// The coverage geometry of a recorded fill.
enum FillShape {
    /// `FillRect` — kurbo rect.
    Rect(Rect),
    /// `BlurredRect` — `[x, y, w, h]` array.
    Quad([f32; 4]),
    /// Linear/radial gradient — no single luminance, so a covering one
    /// makes the backdrop unresolvable rather than contributing color.
    Gradient(Rect),
}

impl FillRec {
    /// True when `p` lies inside both the fill's shape and the clip
    /// that was active when it emitted — a fill clipped away from `p`
    /// produced no output there.
    fn covers(&self, p: (f64, f64)) -> bool {
        if self.clip.is_some_and(|c| !contains(&c, p)) {
            return false;
        }
        match self.shape {
            FillShape::Rect(r) | FillShape::Gradient(r) => contains(&r, p),
            FillShape::Quad(a) => contains4(a, p),
        }
    }
}

/// Resolves the painted backdrop under `anchor` from the fills emitted
/// before command `before`, compositing containing solids until
/// opacity is reached. A containing gradient encountered before opacity
/// makes the backdrop unresolvable — a gradient has no single
/// luminance. Fills whose clip excluded `anchor` are skipped: they
/// never painted there.
fn resolve_backdrop(fills: &[FillRec], before: usize, anchor: (f64, f64)) -> Backdrop {
    let mut acc: Option<ColorRgba> = None;
    for rec in fills.iter().rev() {
        if rec.idx >= before || !rec.covers(anchor) {
            continue;
        }
        match rec.shape {
            FillShape::Gradient(_) => {
                if acc.is_none_or(|c| c.a < 0.999) {
                    return Backdrop::Unknown;
                }
            }
            _ => {
                acc = Some(match acc {
                    None => rec.color,
                    Some(top) => top.composite_over(rec.color),
                });
            }
        }
        if acc.is_some_and(|c| c.a >= 0.999) {
            return Backdrop::Resolved(acc.unwrap());
        }
    }
    match acc {
        Some(c) if c.a >= 0.98 => Backdrop::Resolved(c),
        _ => Backdrop::Unknown,
    }
}

/// Required contrast ratio for a text size class at a WCAG level —
/// mirrors the threshold table inside
/// [`check_text_contrast`](crate::compliance::check_text_contrast).
fn required_contrast(size: TextSize, level: WcagLevel) -> f32 {
    match (size, level) {
        (TextSize::Normal, WcagLevel::Aa) => 4.5,
        (TextSize::Normal, WcagLevel::Aaa) => 7.0,
        (TextSize::Large, WcagLevel::Aa) => 3.0,
        (TextSize::Large, WcagLevel::Aaa) => 4.5,
    }
}

/// Text extracted from one command: ink bounds, anchor point, font size
/// in device pixels, and foreground color.
struct TextProbe {
    /// Approximate ink box in device pixels.
    bounds: Rect,
    anchor: (f64, f64),
    font_px: f32,
    color: ColorRgba,
    /// Short excerpt for the detail line.
    excerpt: String,
    /// Full source string for the locale probe — `DrawText` only;
    /// shaped [`GlyphRun`] output carries no recoverable text.
    text: Option<String>,
}

/// Extracts the audit inputs from a `DrawText` command. `DrawText`'s
/// point is the top-left of the text block (per the backend's
/// per-character rectangle approximation), so the probe anchor sits at
/// the center of the first em and the bounds approximate each glyph as
/// `0.6em` wide.
fn probe_text(point: &Point, text: &str, size: f32, color: [u8; 4]) -> TextProbe {
    let width = f64::from(size) * 0.6 * text.chars().count() as f64;
    let bounds = Rect::new(
        point.x,
        point.y,
        point.x + width.max(f64::from(size)),
        point.y + f64::from(size),
    );
    TextProbe {
        bounds,
        anchor: ((bounds.x0 + bounds.x1) * 0.5, (bounds.y0 + bounds.y1) * 0.5),
        font_px: size,
        color: rgba_u8(color),
        excerpt: text.chars().take(24).collect(),
        // Bounded: strings past 512 chars are bulk data, not chrome —
        // truncating keeps probe calls cheap and lints readable.
        text: Some(text.chars().take(512).collect()),
    }
}

/// Extracts the audit inputs from a [`GlyphRun`]. Glyph `x`/`y` are
/// baseline origins; the probe anchor is the center of the run's ink
/// box (top approximated at `0.8 * font_size` above the baseline,
/// descent at `0.25 * font_size` below).
fn probe_glyph_run(run: &GlyphRun) -> Option<TextProbe> {
    if run.glyphs.is_empty() {
        return None;
    }
    let x0 = run.glyphs.iter().map(|g| g.x).fold(f32::INFINITY, f32::min);
    let x1 = run
        .glyphs
        .iter()
        .map(|g| g.x + g.width)
        .fold(f32::NEG_INFINITY, f32::max);
    let baseline = run
        .glyphs
        .iter()
        .map(|g| g.y)
        .fold(f32::NEG_INFINITY, f32::max);
    let top = baseline - run.font_size * 0.8;
    let bottom = baseline + run.font_size * 0.25;
    Some(TextProbe {
        bounds: Rect::new(
            f64::from(x0),
            f64::from(top),
            f64::from(x1),
            f64::from(bottom),
        ),
        anchor: (f64::from((x0 + x1) * 0.5), f64::from((top + bottom) * 0.5)),
        font_px: run.font_size,
        color: rgba_u8(run.color),
        excerpt: format!("{} glyphs", run.glyphs.len()),
        text: None,
    })
}

/// Audits a recorded [`PaintList`] for on-screen WCAG 2.2 violations.
///
/// Returns one [`PaintLint`] per finding. The pass is read-only;
/// allocations scale with the command stream (text probes, fill
/// records, scope and stroke tracking). It is intended to run per
/// frame in debug builds via `RenderOrchestrator`'s audit hook, or
/// standalone in tests and tooling.
///
/// Backdrop resolution is a single-point approximation: the text's
/// center is tested against every fill painted before it. Text spanning
/// two different backgrounds resolves against whichever contains the
/// center point.
///
/// # Examples
///
/// ```
/// use martensite_access::paint_audit::{audit_paint_list, PaintAuditConfig, PaintLintKind};
/// use kurbo::{Point, Rect};
/// use martensite_core::PaintList;
///
/// let mut list = PaintList::new();
/// list.push_fill_rect(Rect::new(0.0, 0.0, 400.0, 200.0), [255, 255, 255, 255]);
/// // Black on white at 14pt-equivalent — compliant.
/// list.push_text(Point::new(8.0, 8.0), "readable".to_string(), 14.0, [0, 0, 0, 255]);
///
/// assert!(audit_paint_list(&list, &PaintAuditConfig::default()).is_empty());
/// ```
pub fn audit_paint_list(list: &PaintList, config: &PaintAuditConfig) -> Vec<PaintLint> {
    let scale = if config.scale_factor > 0.0 {
        config.scale_factor
    } else {
        1.0
    };
    let mut lints = Vec::new();
    let mut clip_stack: Vec<Rect> = Vec::new();
    // Every text command in order — reused by the occlusion, overlap
    // and bounds passes below. `in_overlay` marks content inside an
    // `"Overlay"` provenance scope: popups intentionally occlude page
    // content, so overlay↔page collisions are not findings.
    let mut texts: Vec<TextRec> = Vec::new();
    // Every fill command in order — backdrop resolution and occlusion
    // testing are clip-aware, so each records the clip active when it
    // emitted.
    let mut fills: Vec<FillRec> = Vec::new();
    // Widget-paint scopes (PushScope/PopScope) — the innermost entry is
    // the widget whose paint produced a command.
    let mut scope_stack: Vec<ScopeRec> = Vec::new();
    // Every scope ever pushed, with its depth — lets the focus check
    // attribute a finding to the deepest scope containing `focus_rect`.
    let mut scopes_seen: Vec<(usize, ScopeRec)> = Vec::new();
    // Bounding rect of every stroked shape with its active clip — the
    // focus-indicator check looks for at least one *visible* stroke
    // intersecting `config.focus_rect`.
    let mut strokes: Vec<(Rect, Option<Rect>, Option<ScopeRec>)> = Vec::new();
    for (i, cmd) in list.commands.iter().enumerate() {
        match cmd {
            PaintCommand::ClipRect(r) | PaintCommand::ClipRoundedRect(r, _) => {
                clip_stack.push(*r);
            }
            PaintCommand::ClipPath(p) => {
                // The path's bounding box conservatively approximates
                // the clip region — it can only *over*-include, so a
                // finding never escapes by claiming to be clipped away.
                clip_stack.push(p.bounding_box());
            }
            PaintCommand::PopClip => {
                clip_stack.pop();
            }
            PaintCommand::PushScope { id, name, bounds } => {
                let rec = ScopeRec {
                    id: *id,
                    name,
                    bounds: *bounds,
                };
                scope_stack.push(rec);
                scopes_seen.push((scope_stack.len() - 1, rec));
            }
            PaintCommand::PopScope => {
                scope_stack.pop();
            }
            _ => {}
        }
        // Effective clip is the intersection of every active region.
        let clip = clip_stack.iter().copied().reduce(|a, b| a.intersect(b));
        let scope = scope_stack.last().copied();
        let in_overlay = scope_stack.iter().any(|s| s.name == "Overlay");
        match cmd {
            PaintCommand::FillRect(rect, color) => fills.push(FillRec {
                idx: i,
                shape: FillShape::Rect(*rect),
                color: rgba_u8(*color),
                clip,
                in_overlay,
            }),
            PaintCommand::BlurredRect { rect, color, .. } => fills.push(FillRec {
                idx: i,
                shape: FillShape::Quad(*rect),
                color: rgba_f32(*color),
                clip,
                in_overlay,
            }),
            PaintCommand::FillPath(path, color) => fills.push(FillRec {
                idx: i,
                // Bounding-box coverage — convex silhouettes (rounded,
                // squircle, pill) are nearly exact; concave outlines can
                // over-claim coverage slightly, which errs toward
                // *reporting* a backdrop rather than inventing one.
                shape: FillShape::Rect(path.bounding_box()),
                color: rgba_u8(*color),
                clip,
                in_overlay,
            }),
            PaintCommand::FillLinearGradient(rect, ..)
            | PaintCommand::FillRadialGradient(rect, ..) => fills.push(FillRec {
                idx: i,
                shape: FillShape::Gradient(*rect),
                color: ColorRgba::new(0.0, 0.0, 0.0, 0.0),
                clip,
                in_overlay,
            }),
            PaintCommand::FillLinearGradientPath(path, ..)
            | PaintCommand::FillRadialGradientPath(path, ..) => fills.push(FillRec {
                idx: i,
                shape: FillShape::Gradient(path.bounding_box()),
                color: ColorRgba::new(0.0, 0.0, 0.0, 0.0),
                clip,
                in_overlay,
            }),
            PaintCommand::DrawText(point, text, size, color) => {
                texts.push(TextRec {
                    idx: i,
                    probe: probe_text(point, text, *size, *color),
                    clip,
                    scope,
                    in_overlay,
                });
            }
            PaintCommand::DrawGlyphRun(run) => {
                if let Some(p) = probe_glyph_run(run) {
                    texts.push(TextRec {
                        idx: i,
                        probe: p,
                        clip,
                        scope,
                        in_overlay,
                    });
                }
            }
            PaintCommand::StrokeRect(rect, _, color) => {
                if config.check_focus_indicator {
                    strokes.push((*rect, clip, scope));
                }
                if config.check_ui_component_contrast {
                    let anchor = ((rect.x0 + rect.x1) * 0.5, rect.y0);
                    check_stroke(&mut lints, &fills, i, anchor, *color, scope, "rect");
                }
            }
            PaintCommand::StrokePath(path, _, color) => {
                let bb = path.bounding_box();
                if config.check_focus_indicator {
                    strokes.push((bb, clip, scope));
                }
                if config.check_ui_component_contrast {
                    let anchor = ((bb.x0 + bb.x1) * 0.5, bb.y0);
                    check_stroke(&mut lints, &fills, i, anchor, *color, scope, "path");
                }
            }
            _ => {}
        }
        if config.check_text_visibility {
            if let Some(TextRec {
                probe, scope: sc, ..
            }) = texts.last().filter(|t| t.idx == i)
            {
                if clip.is_some_and(|c| fully_outside(&probe.bounds, &c)) {
                    // Scroll-clipped text *inside* its own widget
                    // bounds is expected virtualization, not a
                    // defect — the paint walk already culls whole
                    // off-view subtrees. The defect class worth
                    // reporting is text escaping its own
                    // allocation: it can never be shown.
                    let inside_own_bounds =
                        sc.is_some_and(|s| !fully_outside(&probe.bounds, &s.bounds));
                    if inside_own_bounds {
                        continue;
                    }
                    lints.push(PaintLint {
                        kind: PaintLintKind::ClippedText,
                        severity: LintSeverity::Warning,
                        anchor: probe.anchor,
                        measured: None,
                        required: None,
                        detail: format!(
                            "text \"{}\" lies fully outside the active clip region — nothing is painted{} @ ({:.0}, {:.0})",
                            probe.excerpt,
                            sc.map_or(String::new(), |s| format!(" in {}", s.name)),
                            probe.anchor.0, probe.anchor.1
                        ),
                        scope: sc.map(|s| s.name),
                        widget: sc.and_then(|s| s.id),
                    });
                }
            }
        }
    }
    for &TextRec {
        idx: i,
        ref probe,
        clip,
        scope: sc,
        in_overlay: text_in_overlay,
    } in &texts
    {
        let pos = format!("@ ({:.0}, {:.0})", probe.anchor.0, probe.anchor.1);
        // Which widget emitted this text — names the component to look
        // at instead of making the developer hunt coordinates.
        let in_scope = sc.map_or(String::new(), |s| format!(" in {}", s.name));
        let scope_name = sc.map(|s| s.name);
        // Opt-in locale coverage: flag user-visible `DrawText` strings
        // the app's probe doesn't recognize as translated. Text that
        // paints nothing (empty, whitespace, or clipped away entirely)
        // is skipped — there is nothing to localize.
        if let (Some(probe_fn), Some(text)) = (&config.locale_probe, &probe.text) {
            let clipped_away = clip.is_some_and(|c| fully_outside(&probe.bounds, &c));
            if !clipped_away && !text.trim().is_empty() && !probe_fn.is_translated(text, scope_name)
            {
                lints.push(PaintLint {
                    kind: PaintLintKind::MissingLocale,
                    severity: LintSeverity::Info,
                    anchor: probe.anchor,
                    measured: None,
                    required: None,
                    detail: format!(
                        "text \"{}\" has no locale translation{in_scope} {pos}",
                        probe.excerpt
                    ),
                    scope: scope_name,
                    widget: sc.and_then(|s| s.id),
                });
            }
        }
        let size_pt = probe.font_px / scale;
        if size_pt < config.min_text_size_pt {
            lints.push(PaintLint {
                kind: PaintLintKind::UndersizedText,
                severity: LintSeverity::Warning,
                anchor: probe.anchor,
                measured: Some(size_pt),
                required: Some(config.min_text_size_pt),
                detail: format!(
                    "text \"{}\" at {:.0}pt is below the {:.0}pt minimum ({:.1}px device @ {:.2}x){in_scope} {pos}",
                    probe.excerpt, size_pt, config.min_text_size_pt, probe.font_px, scale
                ),
                scope: scope_name,
                widget: sc.and_then(|s| s.id),
            });
        }
        match resolve_backdrop(&fills, i, probe.anchor) {
            Backdrop::Resolved(bg) => {
                let size_class = if size_pt >= LARGE_TEXT_PT {
                    TextSize::Large
                } else {
                    TextSize::Normal
                };
                let required = required_contrast(size_class, config.level);
                let fg = probe.color.composite_over(bg);
                let ratio = contrast_ratio(fg, bg);
                if ratio < required {
                    lints.push(PaintLint {
                        kind: PaintLintKind::InsufficientContrast,
                        severity: LintSeverity::Warning,
                        anchor: probe.anchor,
                        measured: Some(ratio),
                        required: Some(required),
                        detail: format!(
                            "text \"{}\" contrast {:.2}:1 below {:.1}:1 ({:?} {:?} @ {:.0}pt){in_scope} {pos}",
                            probe.excerpt, ratio, required, config.level, size_class, size_pt
                        ),
                        scope: scope_name,
                        widget: sc.and_then(|s| s.id),
                    });
                }
            }
            Backdrop::Unknown if config.report_unknown_backdrop => {
                lints.push(PaintLint {
                    kind: PaintLintKind::UnknownBackdrop,
                    severity: LintSeverity::Info,
                    anchor: probe.anchor,
                    measured: None,
                    required: None,
                    detail: format!(
                        "text \"{}\" backdrop is not a resolvable solid fill — contrast unverified{in_scope} {pos}",
                        probe.excerpt
                    ),
                    scope: scope_name,
                    widget: sc.and_then(|s| s.id),
                });
            }
            Backdrop::Unknown => {}
        }
        // The region the text actually renders in — commands inside a
        // clip only produce output where bounds and clip intersect. All
        // visibility/bounds checks below test `visible`, so a fill that
        // merely covers clipped-away pixels does not read as occlusion.
        let visible = clip.map_or(probe.bounds, |c| probe.bounds.intersect(c));
        let renders = visible.width() > 0.0 && visible.height() > 0.0;
        // Five-sample probe: center plus the four quarter-points. A
        // fill covering only part of the text leaves a visible sample,
        // so partial cover no longer reads as full occlusion.
        let (w, h) = (visible.width(), visible.height());
        let samples = [
            (visible.x0 + w * 0.5, visible.y0 + h * 0.5),
            (visible.x0 + w * 0.25, visible.y0 + h * 0.25),
            (visible.x1 - w * 0.25, visible.y0 + h * 0.25),
            (visible.x0 + w * 0.25, visible.y1 - h * 0.25),
            (visible.x1 - w * 0.25, visible.y1 - h * 0.25),
        ];
        if config.check_text_visibility
            && renders
            && occluded_by_later_fill(&fills, i, &samples, text_in_overlay)
        {
            lints.push(PaintLint {
                kind: PaintLintKind::OccludedText,
                severity: LintSeverity::Warning,
                anchor: probe.anchor,
                measured: None,
                required: None,
                detail: format!(
                    "text \"{}\" is covered by a later opaque fill — invisible output{in_scope} {pos}",
                    probe.excerpt
                ),
                scope: scope_name,
                widget: sc.and_then(|s| s.id),
            });
        }
        if config.check_bounds && renders {
            // Width overflow: text extending past its clip's left/right
            // edge is wider than its container. Advisory only — scroll
            // containers clip overflow deliberately, and the paint
            // stream cannot tell intentional clipping from a layout
            // bug. Vertical overflow is not flagged at all for the same
            // reason (mid-row clipping is normal scrolling).
            if let Some(c) = clip {
                let over_r = probe.bounds.x1 - c.x1;
                let over_l = c.x0 - probe.bounds.x0;
                let (over, edge) = if over_r > 1.0 {
                    (over_r, "right")
                } else if over_l > 1.0 {
                    (over_l, "left")
                } else {
                    (0.0, "")
                };
                if over > 0.0 {
                    lints.push(PaintLint {
                        kind: PaintLintKind::ClipOverflow,
                        severity: LintSeverity::Info,
                        anchor: probe.anchor,
                        measured: Some(over as f32),
                        required: None,
                        detail: format!(
                            "text \"{}\" extends {over:.0}px past its clip's {edge} edge — wider than container (may be intentional){in_scope} {pos}",
                            probe.excerpt
                        ),
                        scope: scope_name,
                        widget: sc.and_then(|s| s.id),
                    });
                }
            }
            if let Some(f) = config.frame {
                let b = &visible;
                let (over, edge) = if b.x1 > f.x1 + 1.0 {
                    (b.x1 - f.x1, "right")
                } else if b.x0 < f.x0 - 1.0 {
                    (f.x0 - b.x0, "left")
                } else if b.y1 > f.y1 + 1.0 {
                    (b.y1 - f.y1, "bottom")
                } else if b.y0 < f.y0 - 1.0 {
                    (f.y0 - b.y0, "top")
                } else {
                    (0.0, "")
                };
                if over > 0.0 {
                    lints.push(PaintLint {
                        kind: PaintLintKind::OutOfFrame,
                        severity: LintSeverity::Warning,
                        anchor: probe.anchor,
                        measured: Some(over as f32),
                        required: None,
                        detail: format!(
                            "text \"{}\" extends {over:.0}px beyond the frame's {edge} edge{in_scope} {pos}",
                            probe.excerpt
                        ),
                        scope: scope_name,
                        widget: sc.and_then(|s| s.id),
                    });
                }
            }
        }
        // Container overflow: the *visible* region exceeding the
        // widget's own scope bounds means content paints outside its
        // container with no clip constraining it — a real leak, unlike
        // the advisory ClipOverflow which fires inside intentional clips.
        if config.check_container_overflow && renders {
            if let Some(sc) = sc {
                let b = sc.bounds;
                // A zero-area scope means the widget hasn't been laid
                // out — its bounds can't contain anything, so flagging
                // every text run would be noise. The 2px tolerance
                // absorbs probe-estimation error (text bounds are
                // approximated from glyph metrics, not measured ink).
                if b.width() > 0.0 && b.height() > 0.0 {
                    let (over, edge) = if visible.x1 > b.x1 + 2.0 {
                        (visible.x1 - b.x1, "right")
                    } else if visible.x0 < b.x0 - 2.0 {
                        (b.x0 - visible.x0, "left")
                    } else if visible.y1 > b.y1 + 2.0 {
                        (visible.y1 - b.y1, "bottom")
                    } else if visible.y0 < b.y0 - 2.0 {
                        (b.y0 - visible.y0, "top")
                    } else {
                        (0.0, "")
                    };
                    if over > 0.0 {
                        lints.push(PaintLint {
                            kind: PaintLintKind::WidgetOverflow,
                            severity: LintSeverity::Warning,
                            anchor: probe.anchor,
                            measured: Some(over as f32),
                            required: None,
                            detail: format!(
                                "text \"{}\" paints {over:.0}px past its widget's {edge} edge — outside container bounds{in_scope} {pos}",
                                probe.excerpt
                            ),
                            scope: scope_name,
                            widget: sc.id,
                        });
                    }
                }
            }
        }
    }
    if config.check_text_overlap {
        for a in 0..texts.len() {
            for b in (a + 1)..texts.len() {
                let (pa, clip_a, ov_a) = (&texts[a].probe, texts[a].clip, texts[a].in_overlay);
                let (pb, clip_b, ov_b) = (&texts[b].probe, texts[b].clip, texts[b].in_overlay);
                // Popup text intentionally renders above page content —
                // only report collisions within the same layer.
                if ov_a != ov_b {
                    continue;
                }
                // Compare the *visible* regions — a run clipped inside its
                // container can't actually render on top of a neighbour.
                let va = clip_a.map_or(pa.bounds, |c| pa.bounds.intersect(c));
                let vb = clip_b.map_or(pb.bounds, |c| pb.bounds.intersect(c));
                if va.intersect(vb).area() > 1.0 {
                    lints.push(PaintLint {
                        kind: PaintLintKind::TextOverlap,
                        severity: LintSeverity::Warning,
                        anchor: pa.anchor,
                        measured: None,
                        required: None,
                        detail: format!(
                            "text \"{}\" overlaps \"{}\" — runs render on top of each other{} @ ({:.0}, {:.0})",
                            pa.excerpt,
                            pb.excerpt,
                            texts[a]
                                .scope
                                .map_or(String::new(), |s| format!(" in {}", s.name)),
                            pa.anchor.0,
                            pa.anchor.1
                        ),
                        scope: texts[a].scope.map(|s| s.name),
                        widget: texts[a].scope.and_then(|s| s.id),
                    });
                }
            }
        }
    }
    // Focus indicator (WCAG 2.4.7): when the app reports the focused
    // widget's bounds, at least one *visible* stroked shape should
    // intersect them — a stroke clipped away entirely doesn't count.
    if config.check_focus_indicator {
        if let Some(fr) = config.focus_rect {
            let has_indicator = strokes.iter().any(|(sr, clip, _)| {
                let visible = clip.map_or(*sr, |c| sr.intersect(c));
                visible.intersect(fr).area() > 0.5
            });
            if !has_indicator {
                let center = ((fr.x0 + fr.x1) * 0.5, (fr.y0 + fr.y1) * 0.5);
                // Attribute to the deepest scope containing the focus
                // rect's center — that's the widget missing its ring.
                let owner = scopes_seen
                    .iter()
                    .filter(|(_, s)| s.bounds.contains(kurbo::Point::new(center.0, center.1)))
                    .max_by_key(|(depth, _)| *depth)
                    .map(|(_, s)| *s);
                let zero_area = fr.width() <= 0.0 || fr.height() <= 0.0;
                lints.push(PaintLint {
                    kind: PaintLintKind::MissingFocusIndicator,
                    severity: LintSeverity::Warning,
                    anchor: center,
                    measured: None,
                    required: None,
                    detail: format!(
                        "focused widget{} at ({:.0}, {:.0})–({:.0}, {:.0}) shows no painted focus indicator (WCAG 2.4.7)",
                        if zero_area { " (zero-area bounds)" } else { "" },
                        fr.x0, fr.y0, fr.x1, fr.y1
                    ),
                    scope: owner.map(|s| s.name),
                    widget: owner.and_then(|s| s.id),
                });
            }
        }
    }
    lints
}

/// Returns true when `inner` has no intersection with `clip`.
fn fully_outside(inner: &Rect, clip: &Rect) -> bool {
    inner.x1 <= clip.x0 || inner.x0 >= clip.x1 || inner.y1 <= clip.y0 || inner.y0 >= clip.y1
}

/// WCAG 1.4.11 check for one stroked shape: resolves the backdrop at
/// `anchor` (a point on the stroke's edge) and requires 3:1 against
/// the stroke's *composited* color — a translucent stroke is judged by
/// what it actually looks like, not its nominal paint color.
fn check_stroke(
    lints: &mut Vec<PaintLint>,
    fills: &[FillRec],
    before: usize,
    anchor: (f64, f64),
    color: [u8; 4],
    scope: Option<ScopeRec>,
    shape: &str,
) {
    let fg = rgba_u8(color);
    let Backdrop::Resolved(bg) = resolve_backdrop(fills, before, anchor) else {
        return;
    };
    let ratio = contrast_ratio(fg.composite_over(bg), bg);
    if ratio < 3.0 {
        lints.push(PaintLint {
            kind: PaintLintKind::NonTextContrast,
            severity: LintSeverity::Warning,
            anchor,
            measured: Some(ratio),
            required: Some(3.0),
            detail: format!(
                "stroked {shape} contrast {ratio:.2}:1 below 3:1 (WCAG 1.4.11 non-text){} @ ({:.0}, {:.0})",
                scope.map_or(String::new(), |s| format!(" in {}", s.name)),
                anchor.0, anchor.1
            ),
            scope: scope.map(|s| s.name),
            widget: scope.and_then(|s| s.id),
        });
    }
}

/// WCAG 2.5.8 target-size audit over the arena: interactive nodes —
/// `FOCUSABLE` or `HIT_TEST_ENABLED` — must be at least 24×24 logical
/// points. The paint stream cannot see hit regions, so this is an
/// arena-level sibling to [`audit_paint_list`].
///
/// Nodes with zero-area bounds (not yet laid out) are skipped, as are
/// nodes inside invisible subtrees and `INERT` nodes — a node that
/// can't be seen or reached can't be missed. Call once per frame
/// alongside the paint audit (e.g.
/// `RenderOrchestrator::audit_target_sizes`).
///
/// Limitations: widget-*internal* children (e.g. a `Button` inside a
/// `Flex`) are not arena nodes and are invisible to this pass — only
/// arena-registered widgets are checked. WCAG 2.5.8's spacing/inline/
/// essential exceptions can't be detected from geometry alone, so
/// exception-conforming targets are still reported.
///
/// # Examples
///
/// ```
/// use martensite_access::paint_audit::audit_target_sizes;
/// use martensite_core::{DummyWidget, HotNode, NodeFlags, Rect, WidgetArena};
///
/// let mut arena = WidgetArena::new();
/// let mut hot = HotNode::default();
/// hot.flags |= NodeFlags::VISIBLE | NodeFlags::FOCUSABLE;
/// hot.bounds = Rect::new(0.0, 0.0, 8.0, 8.0); // 8pt button — too small
/// arena.insert_with_widget(hot, Box::new(DummyWidget));
///
/// let lints = audit_target_sizes(&arena, 1.0);
/// assert_eq!(lints.len(), 1);
/// ```
pub fn audit_target_sizes(
    arena: &martensite_core::WidgetArena,
    scale_factor: f64,
) -> Vec<PaintLint> {
    const MIN_TARGET_PT: f64 = 24.0;
    let scale = if scale_factor > 0.0 {
        scale_factor
    } else {
        1.0
    };
    let mut lints = Vec::new();
    for id in arena.iter_depth_first() {
        let Some(hot) = arena.get_hot(id) else {
            continue;
        };
        let interactive = hot
            .flags
            .intersects(NodeFlags::FOCUSABLE | NodeFlags::HIT_TEST_ENABLED);
        if !interactive
            || !hot.flags.contains(NodeFlags::VISIBLE)
            || hot.flags.contains(NodeFlags::INERT)
        {
            continue;
        }
        // An invisible ancestor prunes the whole subtree from the paint
        // walk — the node can't be seen or hit, so don't flag it.
        let mut ancestor = arena.parent(id);
        let mut hidden = false;
        while let Some(a) = ancestor {
            match arena.get_hot(a) {
                Some(h) if !h.flags.contains(NodeFlags::VISIBLE) => {
                    hidden = true;
                    break;
                }
                _ => ancestor = arena.parent(a),
            }
        }
        if hidden {
            continue;
        }
        let (w, h) = (
            f64::from(hot.bounds.width()),
            f64::from(hot.bounds.height()),
        );
        if w <= 0.0 || h <= 0.0 {
            continue; // not laid out yet
        }
        if !crate::compliance::check_target_size((w / scale) as f32, (h / scale) as f32) {
            // Same name source as the paint scopes: instance name wins.
            let name = arena
                .get_cold(id)
                .and_then(|c| c.debug_name)
                .unwrap_or_else(|| {
                    arena
                        .get_cold(id)
                        .map(|c| c.widget.debug_name())
                        .unwrap_or("<widget>")
                });
            lints.push(PaintLint {
                kind: PaintLintKind::UndersizedTarget,
                severity: LintSeverity::Warning,
                anchor: (
                    f64::from(hot.bounds.min_x() + hot.bounds.max_x()) * 0.5,
                    f64::from(hot.bounds.min_y() + hot.bounds.max_y()) * 0.5,
                ),
                measured: Some((w.min(h) / scale) as f32),
                required: Some(MIN_TARGET_PT as f32),
                detail: format!(
                    "{name} is {:.0}×{:.0}pt — below the 24×24pt WCAG 2.5.8 target minimum (slot {}, generation {}) @ ({:.0}, {:.0})",
                    w / scale,
                    h / scale,
                    id.slot_idx(),
                    id.generation(),
                    f64::from(hot.bounds.min_x()),
                    f64::from(hot.bounds.min_y())
                ),
                scope: Some(name),
                widget: Some(id),
            });
        }
    }
    lints
}

/// Arena-level audit pass reporting [`PaintLintKind::Underflow`] for
/// nodes whose allocated bounds underflow their declared
/// [`RenderMinimum`](martensite_core::RenderMinimum).
///
/// A node is reported when all of these hold:
///
/// - It declares a non-zero render minimum (via `Widget::min_render`
///   or a `ColdNode::with_render_minimum` override).
/// - Its allocated bounds are below that minimum on either axis.
/// - No *enforcing* policy is currently engaged for it — an engaged
///   `Hide`/`Clip`/`Scrim`/`Fallback`/`Collapse` is handling the
///   shortfall by design and is not a finding.
///
/// `Allow` and `Lint` are advisory and always report (that is `Lint`'s
/// whole purpose). An enforcing policy that declares a minimum but is
/// not engaged also reports — the detail names the policy, which is the
/// tell that `WidgetArena::update_underflow` was never wired into that
/// layout path.
///
/// `scale_factor` is physical pixels per logical point (e.g. `2.0` on
/// Retina); `<= 0.0` falls back to `1.0`. Bounds are stored in device
/// pixels, declared minimums in logical points.
///
/// # Examples
///
/// ```
/// use martensite_access::paint_audit::{audit_underflow, PaintLintKind};
/// use martensite_core::{ColdNode, DummyWidget, HotNode, NodeFlags, Rect, WidgetArena};
/// use martensite_core::RenderMinimum;
/// use glam::Vec2;
///
/// let mut arena = WidgetArena::new();
/// let mut hot = HotNode::default();
/// hot.flags |= NodeFlags::VISIBLE;
/// hot.bounds = Rect::new(0.0, 0.0, 40.0, 10.0); // below the floor
/// let cold = ColdNode::new(Box::new(DummyWidget))
///     .with_render_minimum(RenderMinimum::new(Vec2::new(80.0, 24.0)));
/// arena.insert(hot, cold);
///
/// let lints = audit_underflow(&arena, 1.0);
/// assert_eq!(lints.len(), 1);
/// assert_eq!(lints[0].kind, PaintLintKind::Underflow);
/// ```
pub fn audit_underflow(arena: &martensite_core::WidgetArena, scale_factor: f64) -> Vec<PaintLint> {
    let scale = if scale_factor > 0.0 {
        scale_factor as f32
    } else {
        1.0
    };
    let mut lints = Vec::new();
    for id in arena.iter_depth_first() {
        let (Some(hot), Some(cold)) = (arena.get_hot(id), arena.get_cold(id)) else {
            continue;
        };
        let minimum = cold.effective_render_minimum();
        // Engaged enforcing policies handle the shortfall — not a finding.
        let underflowed = hot.bounds.size.x < minimum.size.x * scale
            || hot.bounds.size.y < minimum.size.y * scale;
        if cold.underflow_policy().is_some() || !underflowed {
            continue;
        }
        let name = cold.debug_name.unwrap_or_else(|| cold.widget.debug_name());
        let (w, h) = (hot.bounds.width() / scale, hot.bounds.height() / scale);
        let (mw, mh) = (minimum.size.x, minimum.size.y);
        let detail = if minimum.policy.enforces() {
            format!(
                "{name} is {w:.0}×{h:.0}pt — below declared minimum {mw:.0}×{mh:.0}pt; policy {:?} is declared but not engaged — is `update_underflow` wired into this layout path? (slot {}, generation {}) @ ({:.0}, {:.0})",
                minimum.policy,
                id.slot_idx(),
                id.generation(),
                f64::from(hot.bounds.min_x()),
                f64::from(hot.bounds.min_y())
            )
        } else {
            format!(
                "{name} is {w:.0}×{h:.0}pt — below declared minimum {mw:.0}×{mh:.0}pt (policy {:?}, slot {}, generation {}) @ ({:.0}, {:.0})",
                minimum.policy,
                id.slot_idx(),
                id.generation(),
                f64::from(hot.bounds.min_x()),
                f64::from(hot.bounds.min_y())
            )
        };
        lints.push(PaintLint {
            kind: PaintLintKind::Underflow,
            severity: LintSeverity::Warning,
            anchor: (
                f64::from(hot.bounds.min_x() + hot.bounds.max_x()) * 0.5,
                f64::from(hot.bounds.min_y() + hot.bounds.max_y()) * 0.5,
            ),
            measured: Some(w.min(h)),
            required: Some(mw.min(mh)),
            detail,
            scope: Some(name),
            widget: Some(id),
        });
    }
    lints
}

/// True when every point in `points` is covered by a later opaque
/// non-gradient fill. `FillRec::covers` already respects the covering
/// fill's own clip, so a fill clipped away from a sample can't count.
/// Probing several samples (not just the center) keeps a fill that
/// covers only part of the text from reading as full occlusion.
/// `text_in_overlay` marks text inside an `"Overlay"` provenance
/// scope: popup fills intentionally occlude page content, so a
/// cross-layer cover (overlay fill over page text, or vice versa)
/// does not count — only same-layer occlusion is a defect.
fn occluded_by_later_fill(
    fills: &[FillRec],
    after: usize,
    points: &[(f64, f64)],
    text_in_overlay: bool,
) -> bool {
    !points.is_empty()
        && points.iter().all(|p| {
            fills.iter().any(|rec| {
                rec.idx > after
                    && rec.in_overlay == text_in_overlay
                    && rec.color.a >= 0.999
                    && !matches!(rec.shape, FillShape::Gradient(_))
                    && rec.covers(*p)
            })
        })
}

/// Deduplicating reporter for [`PaintLint`]s.
///
/// Paint audits typically run every frame; without dedup each finding
/// would re-log at frame rate. `report` emits each unique lint exactly
/// once via `tracing` — `warn!` for [`LintSeverity::Warning`], `info!`
/// for [`LintSeverity::Info`]. A lint that disappears and later
/// reappears with an identical fingerprint is not re-reported; call
/// [`LintReporter::reset`] to re-arm all findings (e.g. after a theme
/// switch changes every color at once).
///
/// # Examples
///
/// ```
/// use martensite_access::paint_audit::LintReporter;
///
/// let reporter = LintReporter::new();
/// assert_eq!(reporter.seen_count(), 0);
/// ```
#[derive(Debug, Default)]
pub struct LintReporter {
    seen: HashSet<u64>,
}

impl LintReporter {
    /// Creates an empty reporter.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite_access::paint_audit::LintReporter;
    ///
    /// let _ = LintReporter::new();
    /// ```
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Emits each lint whose fingerprint has not been seen before.
    /// Returns the number of newly reported lints.
    pub fn report(&mut self, lints: &[PaintLint]) -> usize {
        let mut fresh = 0;
        for lint in lints {
            if self.seen.insert(lint.fingerprint()) {
                fresh += 1;
                match lint.severity {
                    LintSeverity::Warning => warn!(
                        target: "martensite::paint_audit",
                        "paint lint: {}", lint.detail
                    ),
                    LintSeverity::Info => info!(
                        target: "martensite::paint_audit",
                        "paint lint: {}", lint.detail
                    ),
                }
            }
        }
        fresh
    }

    /// Number of unique lints reported so far.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite_access::paint_audit::LintReporter;
    ///
    /// let reporter = LintReporter::new();
    /// assert_eq!(reporter.seen_count(), 0);
    /// ```
    pub fn seen_count(&self) -> usize {
        self.seen.len()
    }

    /// Clears the dedup set so every finding reports again.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite_access::paint_audit::LintReporter;
    ///
    /// let mut reporter = LintReporter::new();
    /// reporter.reset();
    /// assert_eq!(reporter.seen_count(), 0);
    /// ```
    pub fn reset(&mut self) {
        self.seen.clear();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn list_with(bg: [u8; 4]) -> PaintList {
        let mut list = PaintList::new();
        list.push_fill_rect(Rect::new(0.0, 0.0, 800.0, 600.0), bg);
        list
    }

    #[test]
    fn compliant_text_produces_no_lints() {
        let mut list = list_with([255, 255, 255, 255]);
        list.push_text(
            Point::new(10.0, 10.0),
            "hello".to_string(),
            14.0,
            [0, 0, 0, 255],
        );
        assert!(audit_paint_list(&list, &PaintAuditConfig::default()).is_empty());
    }

    #[test]
    fn undersized_text_is_flagged() {
        let mut list = list_with([0, 0, 0, 255]);
        list.push_text(
            Point::new(10.0, 10.0),
            "tiny".to_string(),
            20.0, // 20px device @2x = 10pt < 12pt
            [255, 255, 255, 255],
        );
        let cfg = PaintAuditConfig::default().with_scale_factor(2.0);
        let lints = audit_paint_list(&list, &cfg);
        assert!(lints
            .iter()
            .any(|l| l.kind == PaintLintKind::UndersizedText));
        let lint = lints
            .iter()
            .find(|l| l.kind == PaintLintKind::UndersizedText)
            .unwrap();
        assert_eq!(lint.measured, Some(10.0));
    }

    #[test]
    fn low_contrast_text_is_flagged() {
        let mut list = list_with([30, 30, 30, 255]);
        list.push_text(
            Point::new(10.0, 10.0),
            "dim".to_string(),
            14.0,
            [60, 60, 60, 255], // ~1.4:1
        );
        let lints = audit_paint_list(&list, &PaintAuditConfig::default());
        assert!(lints
            .iter()
            .any(|l| l.kind == PaintLintKind::InsufficientContrast));
    }

    #[test]
    fn large_text_uses_lower_threshold() {
        let mut list = list_with([255, 255, 255, 255]);
        // ~3.5:1 — fails normal (4.5) but passes large AA (3.0).
        list.push_text(
            Point::new(10.0, 10.0),
            "headline".to_string(),
            24.0,
            [119, 119, 119, 255],
        );
        let lints = audit_paint_list(&list, &PaintAuditConfig::default());
        assert!(!lints
            .iter()
            .any(|l| l.kind == PaintLintKind::InsufficientContrast));
    }

    #[test]
    fn topmost_fill_wins_backdrop_resolution() {
        let mut list = PaintList::new();
        list.push_fill_rect(Rect::new(0.0, 0.0, 800.0, 600.0), [255, 255, 255, 255]);
        // A dark panel painted over the white canvas.
        list.push_fill_rect(Rect::new(0.0, 0.0, 400.0, 300.0), [20, 20, 20, 255]);
        list.push_text(
            Point::new(10.0, 10.0),
            "light on dark".to_string(),
            14.0,
            [240, 240, 240, 255],
        );
        assert!(audit_paint_list(&list, &PaintAuditConfig::default()).is_empty());
    }

    #[test]
    fn semitransparent_fills_composite_downward() {
        let mut list = PaintList::new();
        list.push_fill_rect(Rect::new(0.0, 0.0, 800.0, 600.0), [0, 0, 0, 255]);
        // Half-transparent white over black → mid-grey backdrop.
        list.push_fill_rect(Rect::new(0.0, 0.0, 800.0, 600.0), [255, 255, 255, 128]);
        list.push_text(
            Point::new(10.0, 10.0),
            "on grey".to_string(),
            14.0,
            [255, 255, 255, 255],
        );
        // White on ~50% grey ≈ 3.9:1 — flagged.
        let lints = audit_paint_list(&list, &PaintAuditConfig::default());
        assert!(lints
            .iter()
            .any(|l| l.kind == PaintLintKind::InsufficientContrast));
    }

    #[test]
    fn unknown_backdrop_is_opt_in() {
        let mut list = PaintList::new();
        list.push_text(
            Point::new(10.0, 10.0),
            "floating".to_string(),
            14.0,
            [255, 255, 255, 255],
        );
        assert!(audit_paint_list(&list, &PaintAuditConfig::default()).is_empty());

        let cfg = PaintAuditConfig {
            report_unknown_backdrop: true,
            ..Default::default()
        };
        let lints = audit_paint_list(&list, &cfg);
        assert!(lints
            .iter()
            .any(|l| l.kind == PaintLintKind::UnknownBackdrop));
        assert_eq!(lints[0].severity, LintSeverity::Info);
    }

    #[test]
    fn glyph_runs_are_audited() {
        use martensite_core::{GlyphInstance, GlyphRun};
        let mut list = list_with([255, 255, 255, 255]);
        let mut run = GlyphRun::new(8.0, [0, 0, 0, 255]);
        run.push(GlyphInstance::new(10.0, 20.0, 36, 6.0, 8.0));
        list.push_glyph_run(run);
        let lints = audit_paint_list(&list, &PaintAuditConfig::default());
        assert!(lints
            .iter()
            .any(|l| l.kind == PaintLintKind::UndersizedText));
    }

    #[test]
    fn low_contrast_stroke_is_flagged() {
        let mut list = list_with([30, 30, 30, 255]);
        // Border barely distinguishable from its backdrop: ~1.4:1 < 3:1.
        list.push_stroke_rect(Rect::new(10.0, 10.0, 110.0, 50.0), 2.0, [45, 45, 45, 255]);
        let lints = audit_paint_list(&list, &PaintAuditConfig::default());
        assert!(lints
            .iter()
            .any(|l| l.kind == PaintLintKind::NonTextContrast));
    }

    #[test]
    fn high_contrast_stroke_passes() {
        let mut list = list_with([20, 20, 20, 255]);
        list.push_stroke_rect(
            Rect::new(10.0, 10.0, 110.0, 50.0),
            2.0,
            [200, 200, 200, 255],
        );
        assert!(!audit_paint_list(&list, &PaintAuditConfig::default())
            .iter()
            .any(|l| l.kind == PaintLintKind::NonTextContrast));
    }

    #[test]
    fn text_covered_by_later_fill_is_flagged() {
        let mut list = list_with([255, 255, 255, 255]);
        list.push_text(
            Point::new(10.0, 10.0),
            "hidden".to_string(),
            14.0,
            [0, 0, 0, 255],
        );
        // An opaque panel painted over the text.
        list.push_fill_rect(Rect::new(0.0, 0.0, 400.0, 300.0), [0, 0, 0, 255]);
        let lints = audit_paint_list(&list, &PaintAuditConfig::default());
        assert!(lints.iter().any(|l| l.kind == PaintLintKind::OccludedText));
    }

    #[test]
    fn overlay_scope_content_does_not_flag_occlusion() {
        // A dropdown popup legitimately covers page content: the page
        // text beneath it and the popup's own panel/text must not read
        // as occluded/overlapping defects. Same-layer defects still
        // must — a fill covering text *inside* the overlay is a bug.
        let mut list = list_with([255, 255, 255, 255]);
        list.push_text(
            Point::new(10.0, 10.0),
            "page".to_string(),
            14.0,
            [0, 0, 0, 255],
        );
        list.push_scope(None, "Overlay", Rect::new(0.0, 0.0, 400.0, 300.0));
        // Popup panel fill covers the page text — intentional.
        list.push_fill_rect(Rect::new(0.0, 0.0, 400.0, 300.0), [40, 40, 48, 255]);
        // Popup option text overlaps the page text — intentional.
        list.push_text(
            Point::new(10.0, 10.0),
            "popup".to_string(),
            14.0,
            [240, 240, 240, 255],
        );
        list.commands.push(PaintCommand::PopScope);
        let lints = audit_paint_list(&list, &PaintAuditConfig::default());
        assert!(
            !lints
                .iter()
                .any(|l| l.kind == PaintLintKind::OccludedText
                    || l.kind == PaintLintKind::TextOverlap),
            "cross-layer popup occlusion must not lint: {lints:?}"
        );

        // Same-layer defect inside the overlay still flags.
        let mut list = list_with([255, 255, 255, 255]);
        list.push_scope(None, "Overlay", Rect::new(0.0, 0.0, 400.0, 300.0));
        list.push_fill_rect(Rect::new(0.0, 0.0, 400.0, 300.0), [40, 40, 48, 255]);
        list.push_text(
            Point::new(10.0, 10.0),
            "popup".to_string(),
            14.0,
            [240, 240, 240, 255],
        );
        list.push_fill_rect(Rect::new(0.0, 0.0, 400.0, 300.0), [40, 40, 48, 255]);
        list.commands.push(PaintCommand::PopScope);
        let lints = audit_paint_list(&list, &PaintAuditConfig::default());
        assert!(lints.iter().any(|l| l.kind == PaintLintKind::OccludedText));
    }

    #[test]
    fn text_fully_outside_clip_is_flagged() {
        let mut list = list_with([255, 255, 255, 255]);
        list.push_clip(Rect::new(0.0, 0.0, 100.0, 100.0));
        list.push_text(
            Point::new(200.0, 200.0),
            "invisible".to_string(),
            14.0,
            [0, 0, 0, 255],
        );
        list.commands.push(PaintCommand::PopClip);
        let lints = audit_paint_list(&list, &PaintAuditConfig::default());
        assert!(lints.iter().any(|l| l.kind == PaintLintKind::ClippedText));
    }

    #[test]
    fn partially_clipped_text_is_allowed() {
        let mut list = list_with([255, 255, 255, 255]);
        list.push_clip(Rect::new(0.0, 0.0, 100.0, 100.0));
        // Half inside the clip — scroll views do this legitimately.
        list.push_text(
            Point::new(80.0, 50.0),
            "edge".to_string(),
            14.0,
            [0, 0, 0, 255],
        );
        list.commands.push(PaintCommand::PopClip);
        assert!(!audit_paint_list(&list, &PaintAuditConfig::default())
            .iter()
            .any(|l| l.kind == PaintLintKind::ClippedText));
    }

    #[test]
    fn overlapping_text_runs_are_flagged() {
        let mut list = list_with([255, 255, 255, 255]);
        list.push_text(
            Point::new(10.0, 10.0),
            "first".to_string(),
            14.0,
            [0, 0, 0, 255],
        );
        list.push_text(
            Point::new(12.0, 12.0),
            "second".to_string(),
            14.0,
            [0, 0, 0, 255],
        );
        let lints = audit_paint_list(&list, &PaintAuditConfig::default());
        assert!(lints.iter().any(|l| l.kind == PaintLintKind::TextOverlap));
    }

    #[test]
    fn text_past_clip_right_edge_is_flagged() {
        let mut list = list_with([255, 255, 255, 255]);
        list.push_clip(Rect::new(0.0, 0.0, 100.0, 600.0));
        // 14px * 0.6 ≈ 8.4px/glyph * 20 chars ≈ 168px wide at x=10 →
        // extends ~78px past the clip's right edge.
        list.push_text(
            Point::new(10.0, 10.0),
            "a string far too wide".to_string(),
            14.0,
            [0, 0, 0, 255],
        );
        list.pop_clip();
        let lints = audit_paint_list(&list, &PaintAuditConfig::default());
        assert!(lints.iter().any(|l| l.kind == PaintLintKind::ClipOverflow));
    }

    #[test]
    fn text_inside_clip_produces_no_overflow() {
        let mut list = list_with([255, 255, 255, 255]);
        list.push_clip(Rect::new(0.0, 0.0, 400.0, 600.0));
        list.push_text(
            Point::new(10.0, 10.0),
            "fits".to_string(),
            14.0,
            [0, 0, 0, 255],
        );
        list.pop_clip();
        let lints = audit_paint_list(&list, &PaintAuditConfig::default());
        assert!(!lints.iter().any(|l| l.kind == PaintLintKind::ClipOverflow));
    }

    #[test]
    fn text_outside_frame_is_flagged() {
        let mut list = list_with([255, 255, 255, 255]);
        list.push_text(
            Point::new(700.0, 10.0),
            "off the right edge".to_string(),
            14.0,
            [0, 0, 0, 255],
        );
        let cfg = PaintAuditConfig {
            frame: Some(Rect::new(0.0, 0.0, 800.0, 600.0)),
            ..PaintAuditConfig::default()
        };
        let lints = audit_paint_list(&list, &cfg);
        assert!(lints.iter().any(|l| l.kind == PaintLintKind::OutOfFrame));
    }

    #[test]
    fn no_frame_means_no_outoframe() {
        let mut list = list_with([255, 255, 255, 255]);
        list.push_text(
            Point::new(700.0, 10.0),
            "off the right edge".to_string(),
            14.0,
            [0, 0, 0, 255],
        );
        let lints = audit_paint_list(&list, &PaintAuditConfig::default());
        assert!(!lints.iter().any(|l| l.kind == PaintLintKind::OutOfFrame));
    }

    #[test]
    fn reporter_dedupes() {
        let mut list = PaintList::new();
        list.push_text(
            Point::new(10.0, 10.0),
            "tiny".to_string(),
            8.0,
            [255, 255, 255, 255],
        );
        let lints = audit_paint_list(&list, &PaintAuditConfig::default());
        assert!(!lints.is_empty());
        let mut reporter = LintReporter::new();
        assert_eq!(reporter.report(&lints), lints.len());
        assert_eq!(reporter.report(&lints), 0);
        reporter.reset();
        assert_eq!(reporter.report(&lints), lints.len());
    }

    #[test]
    fn scope_marks_emitting_widget() {
        let mut list = list_with([30, 30, 30, 255]);
        list.push_scope(None, "Process Grid", Rect::new(0.0, 0.0, 400.0, 300.0));
        list.push_text(
            Point::new(10.0, 20.0),
            "dim".to_string(),
            14.0,
            [60, 60, 60, 255],
        );
        list.pop_scope();
        let lints = audit_paint_list(&list, &PaintAuditConfig::default());
        let lint = lints
            .iter()
            .find(|l| l.kind == PaintLintKind::InsufficientContrast)
            .expect("contrast lint");
        assert_eq!(lint.scope, Some("Process Grid"));
        assert!(lint.detail.contains("in Process Grid"));
    }

    #[test]
    fn innermost_scope_wins() {
        let mut list = list_with([30, 30, 30, 255]);
        list.push_scope(None, "Outer", Rect::new(0.0, 0.0, 400.0, 300.0));
        list.push_scope(None, "Inner", Rect::new(0.0, 0.0, 200.0, 150.0));
        list.push_text(
            Point::new(10.0, 20.0),
            "dim".to_string(),
            14.0,
            [60, 60, 60, 255],
        );
        list.pop_scope();
        list.pop_scope();
        let lints = audit_paint_list(&list, &PaintAuditConfig::default());
        let lint = lints
            .iter()
            .find(|l| l.kind == PaintLintKind::InsufficientContrast)
            .expect("contrast lint");
        assert_eq!(lint.scope, Some("Inner"));
    }

    #[test]
    fn unbalanced_scopes_are_tolerated() {
        let mut list = list_with([30, 30, 30, 255]);
        // Pop on an empty stack, and a scope left open at list end.
        list.pop_scope();
        list.push_scope(None, "Leaky", Rect::new(0.0, 0.0, 400.0, 300.0));
        list.push_text(
            Point::new(10.0, 20.0),
            "dim".to_string(),
            14.0,
            [60, 60, 60, 255],
        );
        let lints = audit_paint_list(&list, &PaintAuditConfig::default());
        let lint = lints
            .iter()
            .find(|l| l.kind == PaintLintKind::InsufficientContrast)
            .expect("contrast lint");
        assert_eq!(lint.scope, Some("Leaky"));
    }

    #[test]
    fn text_past_scope_bounds_is_widget_overflow() {
        let mut list = list_with([255, 255, 255, 255]);
        // Widget occupies x 0..100 but its text extends to ~x=200.
        list.push_scope(None, "Narrow", Rect::new(0.0, 0.0, 100.0, 50.0));
        list.push_text(
            Point::new(10.0, 20.0),
            "this string is far wider than its widget".to_string(),
            14.0,
            [0, 0, 0, 255],
        );
        list.pop_scope();
        let cfg = PaintAuditConfig {
            check_container_overflow: true,
            ..Default::default()
        };
        let lints = audit_paint_list(&list, &cfg);
        let lint = lints
            .iter()
            .find(|l| l.kind == PaintLintKind::WidgetOverflow)
            .expect("widget overflow lint");
        assert_eq!(lint.scope, Some("Narrow"));
    }

    #[test]
    fn clipped_text_inside_scope_is_not_widget_overflow() {
        let mut list = list_with([255, 255, 255, 255]);
        // Same overflowing text, but a clip constrains the visible
        // region inside the widget — intentional clipping, no leak.
        list.push_scope(None, "Scroll", Rect::new(0.0, 0.0, 100.0, 50.0));
        list.push_clip(Rect::new(0.0, 0.0, 100.0, 50.0));
        list.push_text(
            Point::new(10.0, 20.0),
            "this string is far wider than its widget".to_string(),
            14.0,
            [0, 0, 0, 255],
        );
        list.pop_clip();
        list.pop_scope();
        let cfg = PaintAuditConfig {
            check_container_overflow: true,
            ..Default::default()
        };
        let lints = audit_paint_list(&list, &cfg);
        assert!(!lints
            .iter()
            .any(|l| l.kind == PaintLintKind::WidgetOverflow));
    }

    #[test]
    fn missing_focus_indicator_names_containing_scope() {
        let mut list = list_with([255, 255, 255, 255]);
        list.push_scope(None, "Panel", Rect::new(0.0, 0.0, 400.0, 300.0));
        list.push_scope(None, "Button", Rect::new(10.0, 10.0, 100.0, 40.0));
        list.pop_scope();
        list.pop_scope();
        let cfg = PaintAuditConfig {
            focus_rect: Some(Rect::new(10.0, 10.0, 100.0, 40.0)),
            ..Default::default()
        };
        let lint = audit_paint_list(&list, &cfg)
            .into_iter()
            .find(|l| l.kind == PaintLintKind::MissingFocusIndicator)
            .expect("focus lint");
        assert_eq!(lint.scope, Some("Button"));
    }

    #[test]
    fn missing_focus_indicator_is_flagged() {
        let list = list_with([255, 255, 255, 255]);
        let cfg = PaintAuditConfig {
            focus_rect: Some(Rect::new(10.0, 10.0, 100.0, 40.0)),
            ..Default::default()
        };
        let lints = audit_paint_list(&list, &cfg);
        assert!(lints
            .iter()
            .any(|l| l.kind == PaintLintKind::MissingFocusIndicator));
    }

    #[test]
    fn painted_focus_ring_passes() {
        let mut list = list_with([255, 255, 255, 255]);
        // A visible ring intersecting the focus rect.
        list.push_stroke_rect(Rect::new(10.0, 10.0, 100.0, 40.0), 2.0, [0, 0, 0, 255]);
        let cfg = PaintAuditConfig {
            focus_rect: Some(Rect::new(10.0, 10.0, 100.0, 40.0)),
            ..Default::default()
        };
        let lints = audit_paint_list(&list, &cfg);
        assert!(!lints
            .iter()
            .any(|l| l.kind == PaintLintKind::MissingFocusIndicator));
    }

    #[test]
    fn target_size_audit_flags_small_nodes() {
        use martensite_core::{DummyWidget, HotNode, NodeFlags, WidgetArena};
        let mut arena = WidgetArena::new();
        let mut hot = HotNode::default();
        hot.flags |= NodeFlags::VISIBLE | NodeFlags::FOCUSABLE;
        hot.bounds = martensite_core::Rect::new(0.0, 0.0, 8.0, 8.0);
        arena.insert_with_widget(hot, Box::new(DummyWidget));
        let mut big = HotNode::default();
        big.flags |= NodeFlags::VISIBLE | NodeFlags::HIT_TEST_ENABLED;
        big.bounds = martensite_core::Rect::new(0.0, 0.0, 100.0, 100.0);
        arena.insert_with_widget(big, Box::new(DummyWidget));

        let lints = audit_target_sizes(&arena, 1.0);
        assert_eq!(lints.len(), 1);
        assert_eq!(lints[0].kind, PaintLintKind::UndersizedTarget);
    }

    #[test]
    fn target_size_audit_skips_non_interactive() {
        use martensite_core::{DummyWidget, HotNode, NodeFlags, WidgetArena};
        let mut arena = WidgetArena::new();
        let mut hot = HotNode::default();
        hot.flags |= NodeFlags::VISIBLE; // small but not interactive
        hot.bounds = martensite_core::Rect::new(0.0, 0.0, 4.0, 4.0);
        arena.insert_with_widget(hot, Box::new(DummyWidget));
        assert!(audit_target_sizes(&arena, 1.0).is_empty());
    }

    #[test]
    fn target_size_audit_skips_hidden_subtrees() {
        use martensite_core::{DummyWidget, HotNode, NodeFlags, WidgetArena};
        let mut arena = WidgetArena::new();
        // Parent is invisible — its interactive child can't be hit.
        let parent = arena.insert_with_widget(HotNode::default(), Box::new(DummyWidget));
        let mut hot = HotNode::default();
        hot.flags |= NodeFlags::VISIBLE | NodeFlags::FOCUSABLE;
        hot.bounds = martensite_core::Rect::new(0.0, 0.0, 4.0, 4.0);
        let child = arena.insert_with_widget(hot, Box::new(DummyWidget));
        arena.append_child(parent, child).unwrap();
        assert!(audit_target_sizes(&arena, 1.0).is_empty());
    }

    #[test]
    fn target_size_audit_respects_scale_factor() {
        use martensite_core::{DummyWidget, HotNode, NodeFlags, WidgetArena};
        let mut arena = WidgetArena::new();
        let mut hot = HotNode::default();
        hot.flags |= NodeFlags::VISIBLE | NodeFlags::FOCUSABLE;
        // 40 device px = 20pt at 2x — undersized; at 1x it's 40pt — fine.
        hot.bounds = martensite_core::Rect::new(0.0, 0.0, 40.0, 40.0);
        arena.insert_with_widget(hot, Box::new(DummyWidget));
        assert!(audit_target_sizes(&arena, 1.0).is_empty());
        assert_eq!(audit_target_sizes(&arena, 2.0).len(), 1);
    }

    #[test]
    fn stroke_path_satisfies_focus_indicator() {
        use kurbo::BezPath;
        let mut list = list_with([255, 255, 255, 255]);
        let mut path = BezPath::new();
        path.move_to((10.0, 10.0));
        path.line_to((100.0, 10.0));
        path.line_to((100.0, 40.0));
        path.line_to((10.0, 40.0));
        path.close_path();
        list.push_stroke_path(path, 2.0, [0, 0, 0, 255]);
        let cfg = PaintAuditConfig {
            focus_rect: Some(Rect::new(10.0, 10.0, 100.0, 40.0)),
            ..Default::default()
        };
        assert!(!audit_paint_list(&list, &cfg)
            .iter()
            .any(|l| l.kind == PaintLintKind::MissingFocusIndicator));
    }

    #[test]
    fn clipped_stroke_does_not_satisfy_focus() {
        let mut list = list_with([255, 255, 255, 255]);
        // The ring is painted but clipped away entirely — no indicator.
        list.push_clip(Rect::new(0.0, 0.0, 5.0, 5.0));
        list.push_stroke_rect(Rect::new(10.0, 10.0, 100.0, 40.0), 2.0, [0, 0, 0, 255]);
        list.pop_clip();
        let cfg = PaintAuditConfig {
            focus_rect: Some(Rect::new(10.0, 10.0, 100.0, 40.0)),
            ..Default::default()
        };
        assert!(audit_paint_list(&list, &cfg)
            .iter()
            .any(|l| l.kind == PaintLintKind::MissingFocusIndicator));
    }

    #[test]
    fn json_serializes_non_finite_as_null() {
        let lint = PaintLint {
            kind: PaintLintKind::UndersizedText,
            severity: LintSeverity::Warning,
            anchor: (f64::NAN, 2.0),
            measured: Some(f32::INFINITY),
            required: None,
            detail: "x".to_string(),
            scope: None,
            widget: None,
        };
        let json = lint.to_json();
        assert!(json.contains("\"measured\":null"));
        assert!(json.contains("\"anchor\":[null,2.0]"));
        assert!(!json.contains("NaN"));
        assert!(!json.contains("inf"));
    }

    #[test]
    fn lint_json_escapes_strings() {
        let lint = PaintLint {
            kind: PaintLintKind::UndersizedText,
            severity: LintSeverity::Warning,
            anchor: (1.0, 2.0),
            measured: None,
            required: None,
            detail: "text \"a\\b\"\nline2".to_string(),
            scope: Some("Pane\"l"),
            widget: None,
        };
        let json = lint.to_json();
        assert!(json.contains("\\\"a\\\\b\\\"\\nline2"));
        assert!(json.contains("Pane\\\"l"));
    }
}

#[cfg(test)]
mod locale_tests {
    use super::*;
    use kurbo::Point;

    fn list_with_text(text: &str) -> PaintList {
        let mut list = PaintList::new();
        list.push_fill_rect(Rect::new(0.0, 0.0, 400.0, 200.0), [255, 255, 255, 255]);
        list.push_text(Point::new(8.0, 8.0), text.to_string(), 14.0, [0, 0, 0, 255]);
        list
    }

    fn locale_config() -> PaintAuditConfig {
        PaintAuditConfig {
            locale_probe: Some(LocaleProbe::new(|text, _| {
                matches!(text, "Cancelar" | "Aceptar")
            })),
            ..PaintAuditConfig::default()
        }
    }

    #[test]
    fn missing_locale_flags_untranslated_text() {
        let lints = audit_paint_list(&list_with_text("Unlocalized chrome"), &locale_config());
        assert!(lints.iter().any(|l| l.kind == PaintLintKind::MissingLocale));
    }

    #[test]
    fn translated_text_and_off_by_default() {
        // A probe-approved string produces no lint.
        let lints = audit_paint_list(&list_with_text("Cancelar"), &locale_config());
        assert!(!lints.iter().any(|l| l.kind == PaintLintKind::MissingLocale));
        // Default config — no probe — never reports MissingLocale.
        let lints = audit_paint_list(
            &list_with_text("Unlocalized chrome"),
            &PaintAuditConfig::default(),
        );
        assert!(!lints.iter().any(|l| l.kind == PaintLintKind::MissingLocale));
    }

    #[test]
    fn whitespace_and_clipped_text_are_not_probed() {
        // Whitespace-only strings paint nothing — nothing to localize.
        let lints = audit_paint_list(&list_with_text("   "), &locale_config());
        assert!(!lints.iter().any(|l| l.kind == PaintLintKind::MissingLocale));
        // Text clipped fully away is not user-visible — skip it.
        let mut list = PaintList::new();
        list.push_fill_rect(Rect::new(0.0, 0.0, 400.0, 200.0), [255, 255, 255, 255]);
        list.push_clip(Rect::new(0.0, 0.0, 10.0, 10.0));
        list.push_text(
            Point::new(300.0, 100.0),
            "Unlocalized chrome".to_string(),
            14.0,
            [0, 0, 0, 255],
        );
        let lints = audit_paint_list(&list, &locale_config());
        assert!(!lints.iter().any(|l| l.kind == PaintLintKind::MissingLocale));
    }

    #[test]
    fn underflow_audit_reports_violated_declared_minimum() {
        use martensite_core::{
            ColdNode, DummyWidget, HotNode, NodeFlags, RenderMinimum, UnderflowPolicy, WidgetArena,
        };
        let mut arena = WidgetArena::new();
        let mut hot = HotNode::default();
        hot.flags |= NodeFlags::VISIBLE;
        hot.bounds = martensite_core::Rect::new(0.0, 0.0, 40.0, 10.0);
        let cold = ColdNode::new(Box::new(DummyWidget)).with_render_minimum(
            RenderMinimum::new(glam::Vec2::new(80.0, 24.0)).with_policy(UnderflowPolicy::Lint),
        );
        arena.insert(hot, cold);
        let lints = audit_underflow(&arena, 1.0);
        assert_eq!(lints.len(), 1);
        assert_eq!(lints[0].kind, PaintLintKind::Underflow);
        assert!(lints[0].detail.contains("Lint"));
    }

    #[test]
    fn underflow_audit_skips_engaged_policies_and_undeclared() {
        use martensite_core::{
            ColdNode, DummyWidget, HotNode, NodeFlags, RenderMinimum, UnderflowPolicy, WidgetArena,
        };
        let mut arena = WidgetArena::new();
        // Hide-engaged node — the policy handles the shortfall.
        let mut hot = HotNode::default();
        hot.flags |= NodeFlags::VISIBLE;
        hot.bounds = martensite_core::Rect::new(0.0, 0.0, 40.0, 10.0);
        let cold = ColdNode::new(Box::new(DummyWidget)).with_render_minimum(
            RenderMinimum::new(glam::Vec2::new(80.0, 24.0)).with_policy(UnderflowPolicy::Hide),
        );
        let hidden = arena.insert(hot, cold);
        // No minimum declared — nothing to report.
        let mut hot2 = HotNode::default();
        hot2.flags |= NodeFlags::VISIBLE;
        hot2.bounds = martensite_core::Rect::new(0.0, 0.0, 4.0, 4.0);
        arena.insert_with_widget(hot2, Box::new(DummyWidget));

        arena.update_underflow(hidden);
        assert!(audit_underflow(&arena, 1.0).is_empty());
    }
}
