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
//! - **Invisible text** — text painted fully outside the active clip
//!   region, or covered by a later opaque fill. Both are usually bugs:
//!   z-order mistakes or positioning errors that produce no output.
//! - **Overlapping text** — two text runs whose ink boxes intersect,
//!   the signature of a layout/spacing collision.
//!
//! [`PaintLintKind::UnknownBackdrop`]: crate::paint_audit::PaintLintKind::UnknownBackdrop
//! [`PaintAuditConfig::report_unknown_backdrop`]: crate::paint_audit::PaintAuditConfig::report_unknown_backdrop
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
}

/// A single compliance finding against a painted element.
#[derive(Debug, Clone)]
pub struct PaintLint {
    /// What was violated.
    pub kind: PaintLintKind,
    /// Report severity.
    pub severity: LintSeverity,
    /// Approximate center of the offending text, in device pixels —
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
}

impl PaintLint {
    /// Stable identity for deduplication: same kind at the same anchor
    /// with the same detail hashes equal.
    fn fingerprint(&self) -> u64 {
        use std::hash::{Hash, Hasher};
        let mut h = std::collections::hash_map::DefaultHasher::new();
        self.kind.hash(&mut h);
        ((self.anchor.0 * 4.0) as i64).hash(&mut h);
        ((self.anchor.1 * 4.0) as i64).hash(&mut h);
        self.detail.hash(&mut h);
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

/// Resolves the painted backdrop under `anchor` by walking the commands
/// emitted before the text top-down, compositing every containing solid
/// fill until opacity is reached. A containing gradient encountered
/// before opacity is reached makes the backdrop unresolvable — a
/// gradient has no single luminance.
fn resolve_backdrop(commands: &[PaintCommand], anchor: (f64, f64)) -> Backdrop {
    let mut acc: Option<ColorRgba> = None;
    for cmd in commands.iter().rev() {
        match cmd {
            PaintCommand::FillRect(rect, color) if contains(rect, anchor) => {
                let c = rgba_u8(*color);
                acc = Some(match acc {
                    None => c,
                    Some(top) => top.composite_over(c),
                });
            }
            PaintCommand::BlurredRect { rect, color, .. } if contains4(*rect, anchor) => {
                let c = rgba_f32(*color);
                acc = Some(match acc {
                    None => c,
                    Some(top) => top.composite_over(c),
                });
            }
            // A gradient nearer than the accumulated surface
            // contributes color the audit cannot resolve.
            PaintCommand::FillLinearGradient(rect, ..)
            | PaintCommand::FillRadialGradient(rect, ..)
                if contains(rect, anchor) && acc.is_none_or(|c| c.a < 0.999) =>
            {
                return Backdrop::Unknown;
            }
            _ => {}
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
        anchor: (
            point.x + f64::from(size) * 0.5,
            point.y + f64::from(size) * 0.5,
        ),
        font_px: size,
        color: rgba_u8(color),
        excerpt: text.chars().take(24).collect(),
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
        anchor: (
            f64::from((x0 + x1) * 0.5),
            f64::from((top + baseline) * 0.5),
        ),
        font_px: run.font_size,
        color: rgba_u8(run.color),
        excerpt: format!("{} glyphs", run.glyphs.len()),
    })
}

/// Audits a recorded [`PaintList`] for on-screen WCAG 2.2 violations.
///
/// Returns one [`PaintLint`] per finding. The pass is read-only and
/// allocation-bounded to the number of text commands; it is intended to
/// run per frame in debug builds via `RenderOrchestrator`'s audit hook,
/// or standalone in tests and tooling.
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
    // (command index, probe) for every text command — reused by the
    // occlusion and overlap passes below.
    let mut texts: Vec<(usize, TextProbe)> = Vec::new();
    for (i, cmd) in list.commands.iter().enumerate() {
        match cmd {
            PaintCommand::ClipRect(r) | PaintCommand::ClipRoundedRect(r, _) => {
                clip_stack.push(*r);
            }
            PaintCommand::PopClip => {
                clip_stack.pop();
            }
            PaintCommand::DrawText(point, text, size, color) => {
                texts.push((i, probe_text(point, text, *size, *color)));
            }
            PaintCommand::DrawGlyphRun(run) => {
                if let Some(p) = probe_glyph_run(run) {
                    texts.push((i, p));
                }
            }
            _ => {}
        }
        // Effective clip is the intersection of every active region.
        let clip = clip_stack.iter().copied().reduce(|a, b| a.intersect(b));
        match cmd {
            PaintCommand::StrokeRect(rect, _, color) if config.check_ui_component_contrast => {
                let anchor = ((rect.x0 + rect.x1) * 0.5, rect.y0);
                check_stroke(&mut lints, &list.commands[..i], anchor, *color, "rect");
            }
            PaintCommand::StrokePath(path, _, color) if config.check_ui_component_contrast => {
                let bb = path.bounding_box();
                let anchor = ((bb.x0 + bb.x1) * 0.5, bb.y0);
                check_stroke(&mut lints, &list.commands[..i], anchor, *color, "path");
            }
            _ => {}
        }
        if config.check_text_visibility {
            if let Some((_, probe)) = texts.last().filter(|(idx, _)| *idx == i) {
                if clip.is_some_and(|c| fully_outside(&probe.bounds, &c)) {
                    lints.push(PaintLint {
                        kind: PaintLintKind::ClippedText,
                        severity: LintSeverity::Warning,
                        anchor: probe.anchor,
                        measured: None,
                        required: None,
                        detail: format!(
                            "text \"{}\" lies fully outside the active clip region — nothing is painted",
                            probe.excerpt
                        ),
                    });
                }
            }
        }
    }
    for &(i, ref probe) in &texts {
        let size_pt = probe.font_px / scale;
        if size_pt < config.min_text_size_pt {
            lints.push(PaintLint {
                kind: PaintLintKind::UndersizedText,
                severity: LintSeverity::Warning,
                anchor: probe.anchor,
                measured: Some(size_pt),
                required: Some(config.min_text_size_pt),
                detail: format!(
                    "text \"{}\" at {:.0}pt is below the {:.0}pt minimum ({:.1}px device @ {:.2}x)",
                    probe.excerpt, size_pt, config.min_text_size_pt, probe.font_px, scale
                ),
            });
        }
        match resolve_backdrop(&list.commands[..i], probe.anchor) {
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
                            "text \"{}\" contrast {:.2}:1 below {:.1}:1 ({:?} {:?} @ {:.0}pt)",
                            probe.excerpt, ratio, required, config.level, size_class, size_pt
                        ),
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
                        "text \"{}\" backdrop is not a resolvable solid fill — contrast unverified",
                        probe.excerpt
                    ),
                });
            }
            Backdrop::Unknown => {}
        }
        if config.check_text_visibility
            && occluded_by_later_fill(&list.commands[i + 1..], probe.anchor)
        {
            lints.push(PaintLint {
                kind: PaintLintKind::OccludedText,
                severity: LintSeverity::Warning,
                anchor: probe.anchor,
                measured: None,
                required: None,
                detail: format!(
                    "text \"{}\" is covered by a later opaque fill — invisible output",
                    probe.excerpt
                ),
            });
        }
    }
    if config.check_text_overlap {
        for a in 0..texts.len() {
            for b in (a + 1)..texts.len() {
                let (_, pa) = &texts[a];
                let (_, pb) = &texts[b];
                if pa.bounds.intersect(pb.bounds).area() > 1.0 {
                    lints.push(PaintLint {
                        kind: PaintLintKind::TextOverlap,
                        severity: LintSeverity::Warning,
                        anchor: pa.anchor,
                        measured: None,
                        required: None,
                        detail: format!(
                            "text \"{}\" overlaps \"{}\" — runs render on top of each other",
                            pa.excerpt, pb.excerpt
                        ),
                    });
                }
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
/// `anchor` (a point on the stroke's edge) and requires 3:1.
fn check_stroke(
    lints: &mut Vec<PaintLint>,
    prior: &[PaintCommand],
    anchor: (f64, f64),
    color: [u8; 4],
    shape: &str,
) {
    let fg = rgba_u8(color);
    if fg.a < 0.999 {
        return;
    }
    let Backdrop::Resolved(bg) = resolve_backdrop(prior, anchor) else {
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
                "stroked {shape} contrast {ratio:.2}:1 below 3:1 (WCAG 1.4.11 non-text)"
            ),
        });
    }
}

/// True when an opaque fill painted after index `i` covers `anchor`.
/// Clip state of the covering fill is not modeled — a deliberately
/// clipped-away fill may be reported as occluding.
fn occluded_by_later_fill(later: &[PaintCommand], anchor: (f64, f64)) -> bool {
    later.iter().any(|cmd| match cmd {
        PaintCommand::FillRect(rect, color) => contains(rect, anchor) && color[3] == 255,
        PaintCommand::BlurredRect { rect, color, .. } => {
            contains4(*rect, anchor) && color[3] >= 0.999
        }
        _ => false,
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
}
