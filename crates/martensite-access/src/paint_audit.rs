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
//!
//! [`PaintLintKind::UnknownBackdrop`]: crate::paint_audit::PaintLintKind::UnknownBackdrop
//! [`PaintAuditConfig::report_unknown_backdrop`]: crate::paint_audit::PaintAuditConfig::report_unknown_backdrop
//! [`PaintLintKind::ClipOverflow`]: crate::paint_audit::PaintLintKind::ClipOverflow
//! [`PaintLintKind::OutOfFrame`]: crate::paint_audit::PaintLintKind::OutOfFrame
//! [`PaintAuditConfig::frame`]: crate::paint_audit::PaintAuditConfig::frame
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
    /// Text extends horizontally past its active clip region — its
    /// width exceeds the container it was laid out for. Advisory
    /// ([`LintSeverity::Info`]): scroll containers clip deliberately,
    /// and the paint stream cannot tell intent.
    ClipOverflow,
    /// Text extends partially outside the configured frame — at best
    /// wasted work, usually a layout that does not fit the window.
    OutOfFrame,
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
    /// with the same detail hashes equal. Digits are excluded from the
    /// detail hash — measured values (px overflow, contrast ratios)
    /// jitter frame to frame and would mint a fresh fingerprint for
    /// what is conceptually the same finding.
    fn fingerprint(&self) -> u64 {
        use std::hash::{Hash, Hasher};
        let mut h = std::collections::hash_map::DefaultHasher::new();
        self.kind.hash(&mut h);
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
    // (command index, probe, clip active at record time) for every text
    // command — reused by the occlusion, overlap and bounds passes below.
    let mut texts: Vec<(usize, TextProbe, Option<Rect>)> = Vec::new();
    // Every fill command in order — backdrop resolution and occlusion
    // testing are clip-aware, so each records the clip active when it
    // emitted.
    let mut fills: Vec<FillRec> = Vec::new();
    for (i, cmd) in list.commands.iter().enumerate() {
        match cmd {
            PaintCommand::ClipRect(r) | PaintCommand::ClipRoundedRect(r, _) => {
                clip_stack.push(*r);
            }
            PaintCommand::PopClip => {
                clip_stack.pop();
            }
            _ => {}
        }
        // Effective clip is the intersection of every active region.
        let clip = clip_stack.iter().copied().reduce(|a, b| a.intersect(b));
        match cmd {
            PaintCommand::FillRect(rect, color) => fills.push(FillRec {
                idx: i,
                shape: FillShape::Rect(*rect),
                color: rgba_u8(*color),
                clip,
            }),
            PaintCommand::BlurredRect { rect, color, .. } => fills.push(FillRec {
                idx: i,
                shape: FillShape::Quad(*rect),
                color: rgba_f32(*color),
                clip,
            }),
            PaintCommand::FillLinearGradient(rect, ..)
            | PaintCommand::FillRadialGradient(rect, ..) => fills.push(FillRec {
                idx: i,
                shape: FillShape::Gradient(*rect),
                color: ColorRgba::new(0.0, 0.0, 0.0, 0.0),
                clip,
            }),
            PaintCommand::DrawText(point, text, size, color) => {
                texts.push((i, probe_text(point, text, *size, *color), clip));
            }
            PaintCommand::DrawGlyphRun(run) => {
                if let Some(p) = probe_glyph_run(run) {
                    texts.push((i, p, clip));
                }
            }
            PaintCommand::StrokeRect(rect, _, color) if config.check_ui_component_contrast => {
                let anchor = ((rect.x0 + rect.x1) * 0.5, rect.y0);
                check_stroke(&mut lints, &fills, i, anchor, *color, "rect");
            }
            PaintCommand::StrokePath(path, _, color) if config.check_ui_component_contrast => {
                let bb = path.bounding_box();
                let anchor = ((bb.x0 + bb.x1) * 0.5, bb.y0);
                check_stroke(&mut lints, &fills, i, anchor, *color, "path");
            }
            _ => {}
        }
        if config.check_text_visibility {
            if let Some((_, probe, _)) = texts.last().filter(|(idx, _, _)| *idx == i) {
                if clip.is_some_and(|c| fully_outside(&probe.bounds, &c)) {
                    lints.push(PaintLint {
                        kind: PaintLintKind::ClippedText,
                        severity: LintSeverity::Warning,
                        anchor: probe.anchor,
                        measured: None,
                        required: None,
                        detail: format!(
                            "text \"{}\" lies fully outside the active clip region — nothing is painted @ ({:.0}, {:.0})",
                            probe.excerpt, probe.anchor.0, probe.anchor.1
                        ),
                    });
                }
            }
        }
    }
    for &(i, ref probe, clip) in &texts {
        let pos = format!("@ ({:.0}, {:.0})", probe.anchor.0, probe.anchor.1);
        let size_pt = probe.font_px / scale;
        if size_pt < config.min_text_size_pt {
            lints.push(PaintLint {
                kind: PaintLintKind::UndersizedText,
                severity: LintSeverity::Warning,
                anchor: probe.anchor,
                measured: Some(size_pt),
                required: Some(config.min_text_size_pt),
                detail: format!(
                    "text \"{}\" at {:.0}pt is below the {:.0}pt minimum ({:.1}px device @ {:.2}x) {pos}",
                    probe.excerpt, size_pt, config.min_text_size_pt, probe.font_px, scale
                ),
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
                            "text \"{}\" contrast {:.2}:1 below {:.1}:1 ({:?} {:?} @ {:.0}pt) {pos}",
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
                        "text \"{}\" backdrop is not a resolvable solid fill — contrast unverified {pos}",
                        probe.excerpt
                    ),
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
        if config.check_text_visibility && renders && occluded_by_later_fill(&fills, i, &samples) {
            lints.push(PaintLint {
                kind: PaintLintKind::OccludedText,
                severity: LintSeverity::Warning,
                anchor: probe.anchor,
                measured: None,
                required: None,
                detail: format!(
                    "text \"{}\" is covered by a later opaque fill — invisible output {pos}",
                    probe.excerpt
                ),
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
                            "text \"{}\" extends {over:.0}px past its clip's {edge} edge — wider than container (may be intentional) {pos}",
                            probe.excerpt
                        ),
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
                            "text \"{}\" extends {over:.0}px beyond the frame's {edge} edge {pos}",
                            probe.excerpt
                        ),
                    });
                }
            }
        }
    }
    if config.check_text_overlap {
        for a in 0..texts.len() {
            for b in (a + 1)..texts.len() {
                let (_, pa, clip_a) = &texts[a];
                let (_, pb, clip_b) = &texts[b];
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
                            "text \"{}\" overlaps \"{}\" — runs render on top of each other @ ({:.0}, {:.0})",
                            pa.excerpt, pb.excerpt, pa.anchor.0, pa.anchor.1
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
/// `anchor` (a point on the stroke's edge) and requires 3:1 against
/// the stroke's *composited* color — a translucent stroke is judged by
/// what it actually looks like, not its nominal paint color.
fn check_stroke(
    lints: &mut Vec<PaintLint>,
    fills: &[FillRec],
    before: usize,
    anchor: (f64, f64),
    color: [u8; 4],
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
                "stroked {shape} contrast {ratio:.2}:1 below 3:1 (WCAG 1.4.11 non-text) @ ({:.0}, {:.0})",
                anchor.0, anchor.1
            ),
        });
    }
}

/// True when every point in `points` is covered by a later opaque
/// non-gradient fill. `FillRec::covers` already respects the covering
/// fill's own clip, so a fill clipped away from a sample can't count.
/// Probing several samples (not just the center) keeps a fill that
/// covers only part of the text from reading as full occlusion.
fn occluded_by_later_fill(fills: &[FillRec], after: usize, points: &[(f64, f64)]) -> bool {
    !points.is_empty()
        && points.iter().all(|p| {
            fills.iter().any(|rec| {
                rec.idx > after
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
}
