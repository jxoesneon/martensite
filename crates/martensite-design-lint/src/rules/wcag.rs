//! WCAG 2.2 rules — the accessibility floors measurable from the
//! scope tree: contrast, target spacing, text clipping, unlabeled
//! controls, reading order, and paint escaping its bounds.
//!
//! Contrast rules measure painted colors against painted
//! backgrounds — the same WCAG 2.x luminance math a renderer would
//! apply, computed from `FillStat`/`TextStat` rather than pixels.

use std::collections::BTreeSet;

use crate::config::LintConfig;
use crate::fix::{FixOp, FixSafety, LintFix};
use crate::report::Finding;
use crate::rule::{param, Confidence, LintRule};
use crate::rules::{
    background_at, contrast_ratio, interactive_leaves, kind_of, luminance, surface_nodes,
};
use crate::scene::{LintNode, LintScene, NodeKind};
use crate::severity::Severity;
use crate::standard::Standard;

/// This module's rules, appended to the registry by `all_rules()`.
pub(crate) fn rules() -> Vec<&'static dyn LintRule> {
    vec![
        &TextContrast,
        &NontextContrast,
        &TextMinSize,
        &TextTruncation,
        &TargetSpacing,
        &IconOnlyControl,
        &ReadingOrder,
        &OverflowClip,
    ]
}

/// Blend `fg` toward whichever of black/white reaches `ratio` against
/// `bg` with less movement — the direction isn't derivable from bg
/// luminance alone (mid-tone backgrounds can satisfy either), so both
/// are solved and the smaller blend wins.
fn adjust_to_contrast(fg: [u8; 4], bg: [u8; 4], ratio: f64) -> [u8; 4] {
    let blend = |t: f64, dst: f64| -> [u8; 4] {
        let b = |c: u8| {
            (f64::from(c) + (dst - f64::from(c)) * t)
                .round()
                .clamp(0.0, 255.0) as u8
        };
        [b(fg[0]), b(fg[1]), b(fg[2]), fg[3]]
    };
    // Minimal blend toward `dst` reaching `ratio` (t=1 if unreachable).
    let solve = |dst: f64| -> f64 {
        let (mut lo, mut hi) = (0.0f64, 1.0f64);
        if contrast_ratio(blend(1.0, dst), bg) < ratio {
            return f64::INFINITY;
        }
        for _ in 0..24 {
            let mid = (lo + hi) / 2.0;
            if contrast_ratio(blend(mid, dst), bg) >= ratio {
                hi = mid;
            } else {
                lo = mid;
            }
        }
        hi
    };
    let (t_dark, t_light) = (solve(0.0), solve(255.0));
    if t_dark <= t_light {
        blend(t_dark.min(1.0), 0.0)
    } else {
        blend(t_light.min(1.0), 255.0)
    }
}

/// — `text-contrast`: painted text vs its painted background.
///
/// WCAG 2.2 SC 1.4.3 requires 4.5:1 for normal text and 3:1 for large
/// text (≥18pt, or ≥14pt bold — we can't see weight, so the 18pt
/// branch alone applies). The background is the smallest opaque fill
/// containing the text origin — the same thing a reader's eye sits
/// on.
struct TextContrast;
impl LintRule for TextContrast {
    fn id(&self) -> &'static str {
        "text-contrast"
    }
    fn standards(&self) -> &'static [Standard] {
        &[Standard::Wcag]
    }
    fn default_severity(&self) -> Severity {
        Severity::Warn
    }
    fn title(&self) -> &'static str {
        "text painted below the WCAG contrast minimum"
    }
    fn citation(&self) -> &'static str {
        "WCAG 2.2 SC 1.4.3 Contrast (Minimum): 4.5:1 normal, 3:1 large (≥18pt)"
    }
    fn check(&self, scene: &LintScene, cfg: &LintConfig) -> Vec<Finding> {
        let min = param(cfg, self.id(), "min_ratio", 4.5);
        let min_large = param(cfg, self.id(), "min_ratio_large", 3.0);
        let sf = f64::from(scene.scale_factor);
        let mut out = Vec::new();
        let mut reported: BTreeSet<(String, String)> = BTreeSet::new();
        for n in scene.walk() {
            for t in &n.texts {
                let Some(bg) = background_at(scene, n, t.origin) else {
                    continue;
                };
                // WCAG "large" = 18pt = 24 CSS px (our pt unit is the
                // logical px — `min_pt * sf` convention).
                let large = f64::from(t.size) / sf >= 24.0;
                let need = if large { min_large } else { min };
                let ratio = contrast_ratio(t.color, bg);
                let key = (
                    n.path.clone(),
                    format!("{ratio:.2}@{},{}", t.origin.x, t.origin.y),
                );
                if ratio < need && reported.insert(key) {
                    let want = adjust_to_contrast(t.color, bg, need);
                    out.push(
                        Finding::new(
                            "text-contrast",
                            &n.path,
                            format!(
                                "text {:.2}:1 against its background — {} text needs \
                                 {:.1}:1 minimum (low-vision and glare conditions multiply \
                                 the deficit)",
                                ratio,
                                if large { "large" } else { "normal" },
                                need
                            ),
                        )
                        .at(n.bounds)
                        .with_fix(LintFix::new(
                            format!(
                                "recolor text to #{:02X}{:02X}{:02X} ({need:.1}:1)",
                                want[0], want[1], want[2]
                            ),
                            FixSafety::Risky,
                            FixOp::RecolorText {
                                path: n.path.clone(),
                                from: t.color,
                                to: want,
                            },
                        )),
                    );
                }
            }
        }
        out
    }
}

/// — `nontext-contrast`: adjacent UI boundaries below 3:1.
///
/// WCAG 2.2 SC 1.4.11: visual information identifying a component's
/// boundary needs 3:1 against adjacent colors. Edge-sharing sibling
/// fills whose contrast is below the floor make controls invisible
/// without any text failing at all.
struct NontextContrast;
impl LintRule for NontextContrast {
    fn id(&self) -> &'static str {
        "nontext-contrast"
    }
    fn standards(&self) -> &'static [Standard] {
        &[Standard::Wcag]
    }
    fn default_severity(&self) -> Severity {
        Severity::Warn
    }
    fn title(&self) -> &'static str {
        "adjacent component boundaries below 3:1"
    }
    fn citation(&self) -> &'static str {
        "WCAG 2.2 SC 1.4.11 Non-text Contrast: 3:1 for component boundaries"
    }
    fn check(&self, scene: &LintScene, cfg: &LintConfig) -> Vec<Finding> {
        let min = param(cfg, self.id(), "min_ratio", 3.0);
        let max_fills = param(cfg, self.id(), "max_fills", 50.0) as usize;
        let mut out = Vec::new();
        let mut reported: BTreeSet<String> = BTreeSet::new();
        for n in scene.walk() {
            // Compare each child's fills against its edge-sharing
            // siblings' fills (the boundary contrast a user sees).
            for (i, a) in n.children.iter().enumerate() {
                if a.fills.len() > max_fills {
                    continue;
                }
                for b in n.children.iter().skip(i + 1) {
                    if b.fills.len() > max_fills {
                        continue;
                    }
                    // Edge-sharing: touching on one axis AND
                    // overlapping on the perpendicular one — otherwise
                    // coincidental edge values pair across the canvas.
                    let share_x = (a.bounds.x1 - b.bounds.x0).abs() < 1.0
                        || (b.bounds.x1 - a.bounds.x0).abs() < 1.0;
                    let share_y = (a.bounds.y1 - b.bounds.y0).abs() < 1.0
                        || (b.bounds.y1 - a.bounds.y0).abs() < 1.0;
                    let ov_x = a.bounds.x1.min(b.bounds.x1) - a.bounds.x0.max(b.bounds.x0);
                    let ov_y = a.bounds.y1.min(b.bounds.y1) - a.bounds.y0.max(b.bounds.y0);
                    if !((share_x && ov_y > 0.0) || (share_y && ov_x > 0.0)) {
                        continue;
                    }
                    for fa in &a.fills {
                        for fb in &b.fills {
                            let ratio = contrast_ratio(fa.color, fb.color);
                            if ratio < min
                                && reported
                                    .insert(format!("{}/{:?}/{:?}", n.path, fa.color, fb.color))
                            {
                                let (from, to) = if luminance(fa.color) > luminance(fb.color) {
                                    (fa.color, adjust_to_contrast(fa.color, fb.color, min))
                                } else {
                                    (fb.color, adjust_to_contrast(fb.color, fa.color, min))
                                };
                                let path = if luminance(fa.color) > luminance(fb.color) {
                                    a.path.clone()
                                } else {
                                    b.path.clone()
                                };
                                out.push(
                                    Finding::new(
                                        "nontext-contrast",
                                        &n.path,
                                        format!(
                                            "adjacent fills #{:02X}{:02X}{:02X}/#{:02X}{:02X}{:02X} \
                                             share an edge at {:.2}:1 — component boundaries need \
                                             3:1 to stay identifiable",
                                            fa.color[0], fa.color[1], fa.color[2],
                                            fb.color[0], fb.color[1], fb.color[2],
                                            ratio
                                        ),
                                    )
                                    .at(n.bounds)
                                    .with_fix(LintFix::new(
                                        "raise the boundary fill to 3:1",
                                        FixSafety::Risky,
                                        FixOp::RecolorFill { path, from, to },
                                    )),
                                );
                            }
                        }
                    }
                }
            }
        }
        out
    }
}

/// — `text-min-size`: text painted below a readability floor.
///
/// WCAG doesn't fix a minimum size (1.4.4 requires *resize* to
/// 200%), but below ~9pt text stops being text for many users — the
/// floor catches decorative microtext that no zoom setting was meant
/// to rescue.
struct TextMinSize;
impl LintRule for TextMinSize {
    fn id(&self) -> &'static str {
        "text-min-size"
    }
    fn standards(&self) -> &'static [Standard] {
        &[Standard::Wcag, Standard::Consistency]
    }
    fn default_severity(&self) -> Severity {
        Severity::Info
    }
    fn confidence(&self) -> Confidence {
        Confidence::Heuristic
    }
    fn title(&self) -> &'static str {
        "text below the readability floor"
    }
    fn citation(&self) -> &'static str {
        "legibility research — sub-9pt text is decorative, not readable; \
         WCAG 1.4.4 presumes a usable base size"
    }
    fn check(&self, scene: &LintScene, cfg: &LintConfig) -> Vec<Finding> {
        let min_pt = param(cfg, self.id(), "min_pt", 9.0);
        let min_px = min_pt * f64::from(scene.scale_factor);
        let mut out = Vec::new();
        let mut reported: BTreeSet<String> = BTreeSet::new();
        for n in scene.walk() {
            for t in &n.texts {
                let px = f64::from(t.size);
                // Key on path+origin — same-name siblings share a path
                // and must each count.
                let key = format!("{}@{},{}", n.path, t.origin.x, t.origin.y);
                if px > 0.5 && px < min_px && reported.insert(key) {
                    out.push(
                        Finding::new(
                            "text-min-size",
                            &n.path,
                            format!(
                                "text at {:.0}px ({:.1}pt) — below the {:.0}pt floor \
                                 where body text stops being readable",
                                px,
                                px / f64::from(scene.scale_factor),
                                min_pt
                            ),
                        )
                        .at(n.bounds)
                        .with_fix(LintFix::new(
                            format!("raise the text size to {min_pt:.0}pt"),
                            FixSafety::Risky,
                            FixOp::SetFontSize {
                                path: n.path.clone(),
                                from: t.size,
                                to: min_px as f32,
                            },
                        )),
                    );
                }
            }
        }
        out
    }
}

/// — `text-truncation`: text runs extending past their scope.
///
/// A run whose measured or estimated advance crosses its node's edge
/// is either clipped mid-glyph or painted over a sibling — both read
/// as bugs to the user even when intentional.
struct TextTruncation;
impl LintRule for TextTruncation {
    fn id(&self) -> &'static str {
        "text-truncation"
    }
    fn standards(&self) -> &'static [Standard] {
        &[Standard::Wcag, Standard::Consistency]
    }
    fn default_severity(&self) -> Severity {
        Severity::Info
    }
    fn confidence(&self) -> Confidence {
        Confidence::Heuristic
    }
    fn title(&self) -> &'static str {
        "text runs extending beyond their scope"
    }
    fn citation(&self) -> &'static str {
        "clipped text fails WCAG 1.4.4 resize expectations and reads as a defect"
    }
    fn check(&self, scene: &LintScene, cfg: &LintConfig) -> Vec<Finding> {
        let tol = param(cfg, self.id(), "tolerance_px", 2.0);
        let char_w = param(cfg, self.id(), "char_width_ratio", 0.55);
        let mut out = Vec::new();
        for n in scene.walk() {
            let mut worst = 0.0f64;
            for t in &n.texts {
                let w = t
                    .width
                    .unwrap_or_else(|| t.text.chars().count() as f64 * f64::from(t.size) * char_w);
                // All four edges — right-overflow is the common case,
                // but left/top/descent overflow clip too (ink extends
                // ~0.2× size below the baseline origin).
                worst = worst
                    .max((t.origin.x + w) - n.bounds.x1)
                    .max(n.bounds.x0 - t.origin.x)
                    .max(n.bounds.y0 - (t.origin.y - f64::from(t.size)))
                    .max((t.origin.y + f64::from(t.size) * 0.2) - n.bounds.y1);
            }
            if worst > tol {
                out.push(
                    Finding::new(
                        "text-truncation",
                        &n.path,
                        format!(
                            "text extends {:.0}px past the scope edge — likely clipped \
                             mid-glyph or painted over a sibling",
                            worst
                        ),
                    )
                    .at(n.bounds),
                );
            }
        }
        out
    }
}

/// — `target-spacing`: interactive siblings packed too tightly.
///
/// WCAG 2.5.8 lets undersized targets pass when spacing compensates —
/// the inverse is also true: even adequately-sized targets mis-click
/// when packed below the spacing clause's intent. Fitts's law puts
/// the same math on error rates.
struct TargetSpacing;
impl LintRule for TargetSpacing {
    fn id(&self) -> &'static str {
        "target-spacing"
    }
    fn standards(&self) -> &'static [Standard] {
        &[Standard::Wcag]
    }
    fn default_severity(&self) -> Severity {
        Severity::Info
    }
    fn confidence(&self) -> Confidence {
        Confidence::Heuristic
    }
    fn title(&self) -> &'static str {
        "interactive targets packed below the spacing floor"
    }
    fn citation(&self) -> &'static str {
        "WCAG 2.2 SC 2.5.8 spacing clause; Fitts's law — crowding multiplies mis-clicks"
    }
    fn check(&self, scene: &LintScene, cfg: &LintConfig) -> Vec<Finding> {
        let min_gap = param(cfg, self.id(), "min_gap_pt", 4.0) * f64::from(scene.scale_factor);
        let mut out = Vec::new();
        for n in scene.walk() {
            let kids: Vec<&LintNode> = n
                .children
                .iter()
                .filter(|c| kind_of(c, cfg) == NodeKind::Interactive && c.area() > 1.0)
                .collect();
            if kids.len() < 2 {
                continue;
            }
            // Adjacent-pair gaps along both axes (band-overlap aware);
            // a pair crowded on both axes counts once.
            let mut crowded: BTreeSet<(usize, usize)> = BTreeSet::new();
            for axis in 0..2 {
                let mut sorted: Vec<usize> = (0..kids.len()).collect();
                sorted.sort_by(|&a, &b| {
                    let (pa, pb) = if axis == 0 {
                        (kids[a].bounds.x0, kids[b].bounds.x0)
                    } else {
                        (kids[a].bounds.y0, kids[b].bounds.y0)
                    };
                    pa.partial_cmp(&pb).unwrap_or(std::cmp::Ordering::Equal)
                });
                for w in sorted.windows(2) {
                    let (ia, ib) = (w[0], w[1]);
                    let (a, b) = (kids[ia], kids[ib]);
                    let (gap, band_overlap, shorter) = if axis == 0 {
                        (
                            b.bounds.x0 - a.bounds.x1,
                            a.bounds.y1.min(b.bounds.y1) - a.bounds.y0.max(b.bounds.y0),
                            a.bounds.height().min(b.bounds.height()),
                        )
                    } else {
                        (
                            b.bounds.y0 - a.bounds.y1,
                            a.bounds.x1.min(b.bounds.x1) - a.bounds.x0.max(b.bounds.x0),
                            a.bounds.width().min(b.bounds.width()),
                        )
                    };
                    if band_overlap > 0.5 * shorter && gap >= 0.0 && gap < min_gap {
                        crowded.insert((ia.min(ib), ia.max(ib)));
                    }
                }
            }
            let crowded = crowded.len();
            if crowded > 0 {
                out.push(
                    Finding::new(
                        "target-spacing",
                        &n.path,
                        format!(
                            "{crowded} interactive pair(s) sit under {:.0}pt apart — \
                             packed targets trade size compliance for mis-clicks",
                            min_gap / f64::from(scene.scale_factor)
                        ),
                    )
                    .at(n.bounds)
                    .with_fix(LintFix::new(
                        format!("space the targets ≥{:.0}px apart", min_gap),
                        FixSafety::Safe,
                        FixOp::SetGap {
                            parent: n.path.clone(),
                            gap: min_gap,
                        },
                    )),
                );
            }
        }
        out
    }
}

/// — `icon-only-control`: interactive leaves with no text anywhere.
///
/// An unlabeled icon asks the user to *recall* what it does instead
/// of recognizing it — Nielsen #6 — and usually means a missing
/// accessible name too. Counts text in the control's whole subtree.
struct IconOnlyControl;
impl LintRule for IconOnlyControl {
    fn id(&self) -> &'static str {
        "icon-only-control"
    }
    fn standards(&self) -> &'static [Standard] {
        &[Standard::Wcag, Standard::InfoDesign]
    }
    fn default_severity(&self) -> Severity {
        Severity::Warn
    }
    fn confidence(&self) -> Confidence {
        Confidence::Heuristic
    }
    fn title(&self) -> &'static str {
        "interactive controls with no text label"
    }
    fn citation(&self) -> &'static str {
        "Nielsen #6 recognition over recall; WCAG 4.1.2 — a nameless control \
         is unnameable to assistive tech"
    }
    fn check(&self, scene: &LintScene, cfg: &LintConfig) -> Vec<Finding> {
        let mut out = Vec::new();
        // Leaves inside nested surfaces are visited from each ancestor
        // — dedupe by identity (path+bounds, since same-name siblings
        // share a path).
        let mut reported: BTreeSet<(String, i64, i64)> = BTreeSet::new();
        for n in surface_nodes(scene, cfg, self.id()) {
            for leaf in interactive_leaves(n, cfg) {
                let has_text = leaf.walk().any(|d| !d.texts.is_empty());
                let key = (
                    leaf.path.clone(),
                    leaf.bounds.x0 as i64,
                    leaf.bounds.y0 as i64,
                );
                if !has_text && reported.insert(key) {
                    out.push(
                        Finding::new(
                            "icon-only-control",
                            &leaf.path,
                            format!(
                                "`{}` is interactive but paints no text — icon-only \
                                 controls need an accessible name and a tooltip, or users \
                                 memorize instead of recognize",
                                leaf.name
                            ),
                        )
                        .at(leaf.bounds),
                    );
                }
            }
        }
        out
    }
}

/// — `reading-order`: document order disagrees with visual order.
///
/// Scope order is what assistive tech reads; screen position is what
/// sighted users read. When siblings' paint order and visual order
/// diverge, the two users get different interfaces (WCAG 1.3.2's
/// meaningful-sequence analog).
struct ReadingOrder;
impl LintRule for ReadingOrder {
    fn id(&self) -> &'static str {
        "reading-order"
    }
    fn standards(&self) -> &'static [Standard] {
        &[Standard::Wcag, Standard::InfoDesign]
    }
    fn default_severity(&self) -> Severity {
        Severity::Info
    }
    fn confidence(&self) -> Confidence {
        Confidence::Heuristic
    }
    fn title(&self) -> &'static str {
        "document order disagrees with visual order"
    }
    fn citation(&self) -> &'static str {
        "WCAG 2.2 SC 1.3.2 Meaningful Sequence — paint order is read order"
    }
    fn check(&self, scene: &LintScene, cfg: &LintConfig) -> Vec<Finding> {
        let min_inversions = param(cfg, self.id(), "min_inversions", 2.0) as usize;
        let band_frac = param(cfg, self.id(), "row_band_frac", 0.25);
        let mut out = Vec::new();
        for n in scene.walk() {
            let kids: Vec<&LintNode> = n.children.iter().filter(|c| c.area() > 1.0).collect();
            if kids.len() < 3 {
                continue;
            }
            // Visual rank: row-major. Bucket into rows first (y0 sort,
            // greedy grouping against the row's running band) then sort
            // each row by x0 — a comparator with an in-band tolerance
            // isn't transitive and can't produce a total order.
            let mut by_y: Vec<usize> = (0..kids.len()).collect();
            by_y.sort_by(|&a, &b| {
                kids[a]
                    .bounds
                    .y0
                    .partial_cmp(&kids[b].bounds.y0)
                    .unwrap_or(std::cmp::Ordering::Equal)
            });
            let mut visual: Vec<usize> = Vec::with_capacity(kids.len());
            let mut row: Vec<usize> = Vec::new();
            let mut row_top = f64::NEG_INFINITY;
            let mut row_h = 1.0f64;
            let flush = |row: &mut Vec<usize>, visual: &mut Vec<usize>| {
                row.sort_by(|&a, &b| {
                    kids[a]
                        .bounds
                        .x0
                        .partial_cmp(&kids[b].bounds.x0)
                        .unwrap_or(std::cmp::Ordering::Equal)
                });
                visual.append(row);
            };
            for &i in &by_y {
                let b = &kids[i].bounds;
                if !row.is_empty() && b.y0 - row_top > band_frac * row_h {
                    flush(&mut row, &mut visual);
                    row_top = f64::NEG_INFINITY;
                    row_h = 1.0;
                }
                if row.is_empty() {
                    row_top = b.y0;
                    row_h = b.height().max(1.0);
                } else {
                    row_top = row_top.min(b.y0);
                    row_h = row_h.min(b.height().max(1.0));
                }
                row.push(i);
            }
            if !row.is_empty() {
                flush(&mut row, &mut visual);
            }
            // Inversions: pairs where document order disagrees with
            // visual rank.
            let rank: Vec<usize> = {
                let mut r = vec![0usize; kids.len()];
                for (v, &i) in visual.iter().enumerate() {
                    r[i] = v;
                }
                r
            };
            let mut inversions = 0usize;
            for i in 0..kids.len() {
                for j in (i + 1)..kids.len() {
                    if rank[i] > rank[j] {
                        inversions += 1;
                    }
                }
            }
            if inversions >= min_inversions {
                out.push(
                    Finding::new(
                        "reading-order",
                        &n.path,
                        format!(
                            "{inversions} order inversions among {} children — paint order \
                             and screen position tell different stories to different users",
                            kids.len()
                        ),
                    )
                    .at(n.bounds),
                );
            }
        }
        out
    }
}

/// — `overflow-clip`: fill geometry escaping its scope bounds.
///
/// A fill extending past its owning scope's bounds either overflows
/// the layout contract or relies on an ancestor clip to hide the
/// mistake — both are the paint-audit class of defect, expressed as
/// a design rule.
struct OverflowClip;
impl LintRule for OverflowClip {
    fn id(&self) -> &'static str {
        "overflow-clip"
    }
    fn standards(&self) -> &'static [Standard] {
        &[Standard::Wcag, Standard::Consistency]
    }
    fn default_severity(&self) -> Severity {
        Severity::Warn
    }
    fn title(&self) -> &'static str {
        "paint geometry escaping its scope bounds"
    }
    fn citation(&self) -> &'static str {
        "layout-contract overflow — content outside its scope relies on clipping to hide"
    }
    fn check(&self, scene: &LintScene, cfg: &LintConfig) -> Vec<Finding> {
        let tol = param(cfg, self.id(), "tolerance_px", 2.0);
        let mut out = Vec::new();
        for n in scene.walk() {
            let mut worst = 0.0f64;
            for f in &n.fills {
                // Only the VISIBLE region can escape — a fill
                // oversized under a scope-tight clip (progress fills,
                // clipped decoration) shows nothing past the edge.
                let vis = match f.clip {
                    Some(c) => f.rect.intersect(c),
                    None => f.rect,
                };
                if vis.width() <= 0.0 || vis.height() <= 0.0 {
                    continue; // fully clipped away
                }
                let over = [
                    n.bounds.x0 - vis.x0,
                    vis.x1 - n.bounds.x1,
                    n.bounds.y0 - vis.y0,
                    vis.y1 - n.bounds.y1,
                ]
                .into_iter()
                .fold(0.0, f64::max);
                worst = worst.max(over);
            }
            if worst > tol {
                out.push(
                    Finding::new(
                        "overflow-clip",
                        &n.path,
                        format!(
                            "a fill extends {:.0}px outside this scope — overflow relies \
                             on an ancestor clip to stay invisible",
                            worst
                        ),
                    )
                    .at(n.bounds),
                );
            }
        }
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::lint;
    use kurbo::{Point, Rect};
    use martensite_core::{PaintCommand, PaintList};

    fn app_list() -> PaintList {
        let mut list = PaintList::new();
        list.push_scope(None, "App", Rect::new(0.0, 0.0, 800.0, 600.0));
        list.commands.push(PaintCommand::FillRect(
            Rect::new(0.0, 0.0, 800.0, 600.0),
            [0x14, 0x14, 0x18, 255],
        ));
        list
    }

    fn findings_for<'a>(report: &'a crate::LintReport, rule: &str) -> Vec<&'a crate::Finding> {
        report.findings.iter().filter(|f| f.rule == rule).collect()
    }

    // ---------- text-contrast ----------

    fn text_on_fill(fg: [u8; 4], bg: [u8; 4]) -> LintScene {
        let mut list = app_list();
        list.push_scope(None, "Panel", Rect::new(10.0, 10.0, 210.0, 60.0));
        list.commands.push(PaintCommand::FillRect(
            Rect::new(10.0, 10.0, 210.0, 60.0),
            bg,
        ));
        list.commands.push(PaintCommand::DrawText(
            Point::new(20.0, 40.0),
            "Status".to_string(),
            12.0,
            fg,
        ));
        list.pop_scope();
        list.pop_scope();
        LintScene::from_paint_list(&list)
    }

    #[test]
    fn text_contrast_flags_low_ratio() {
        // Mid-gray on dark — ~2:1.
        let scene = text_on_fill([90, 90, 90, 255], [20, 20, 24, 255]);
        let report = lint(&scene, &LintConfig::new());
        let hits = findings_for(&report, "text-contrast");
        assert_eq!(hits.len(), 1, "expected one finding, got {hits:?}");
        assert!(hits[0].fix.is_some());
    }

    #[test]
    fn text_contrast_quiet_when_compliant() {
        let scene = text_on_fill([230, 230, 230, 255], [20, 20, 24, 255]);
        let report = lint(&scene, &LintConfig::new());
        assert!(findings_for(&report, "text-contrast").is_empty());
    }

    // ---------- nontext-contrast ----------

    #[test]
    fn nontext_contrast_flags_indistinguishable_edges() {
        let mut list = app_list();
        list.push_scope(None, "Panel", Rect::new(0.0, 0.0, 400.0, 100.0));
        list.push_scope(None, "CardA", Rect::new(0.0, 0.0, 200.0, 100.0));
        list.commands.push(PaintCommand::FillRect(
            Rect::new(0.0, 0.0, 200.0, 100.0),
            [40, 40, 44, 255],
        ));
        list.pop_scope();
        list.push_scope(None, "CardB", Rect::new(200.0, 0.0, 400.0, 100.0));
        list.commands.push(PaintCommand::FillRect(
            Rect::new(200.0, 0.0, 400.0, 100.0),
            [44, 44, 48, 255],
        ));
        list.pop_scope();
        list.pop_scope();
        list.pop_scope();
        let scene = LintScene::from_paint_list(&list);
        let report = lint(&scene, &LintConfig::new());
        assert!(!findings_for(&report, "nontext-contrast").is_empty());
    }

    #[test]
    fn nontext_contrast_quiet_on_clear_boundary() {
        let mut list = app_list();
        list.push_scope(None, "Panel", Rect::new(0.0, 0.0, 400.0, 100.0));
        list.push_scope(None, "CardA", Rect::new(0.0, 0.0, 200.0, 100.0));
        list.commands.push(PaintCommand::FillRect(
            Rect::new(0.0, 0.0, 200.0, 100.0),
            [40, 40, 44, 255],
        ));
        list.pop_scope();
        list.push_scope(None, "CardB", Rect::new(200.0, 0.0, 400.0, 100.0));
        list.commands.push(PaintCommand::FillRect(
            Rect::new(200.0, 0.0, 400.0, 100.0),
            [200, 200, 205, 255],
        ));
        list.pop_scope();
        list.pop_scope();
        list.pop_scope();
        let scene = LintScene::from_paint_list(&list);
        let report = lint(&scene, &LintConfig::new());
        assert!(findings_for(&report, "nontext-contrast").is_empty());
    }

    // ---------- text-min-size ----------

    #[test]
    fn text_min_size_flags_microtext() {
        let mut list = app_list();
        list.push_scope(None, "Panel", Rect::new(10.0, 10.0, 210.0, 60.0));
        list.commands.push(PaintCommand::DrawText(
            Point::new(20.0, 40.0),
            "tiny".to_string(),
            6.0,
            [230, 230, 230, 255],
        ));
        list.pop_scope();
        list.pop_scope();
        let scene = LintScene::from_paint_list(&list);
        let report = lint(&scene, &LintConfig::new());
        assert!(!findings_for(&report, "text-min-size").is_empty());
    }

    #[test]
    fn text_min_size_quiet_at_body_size() {
        let scene = text_on_fill([230, 230, 230, 255], [20, 20, 24, 255]);
        let report = lint(&scene, &LintConfig::new());
        assert!(findings_for(&report, "text-min-size").is_empty());
    }

    // ---------- text-truncation ----------

    #[test]
    fn text_truncation_flags_overrun() {
        let mut list = app_list();
        list.push_scope(None, "Panel", Rect::new(10.0, 10.0, 60.0, 60.0));
        // 20 chars × 12px × 0.55 ≈ 132px from x=14 — way past x1=60.
        list.commands.push(PaintCommand::DrawText(
            Point::new(14.0, 40.0),
            "twenty-characters-long".to_string(),
            12.0,
            [230, 230, 230, 255],
        ));
        list.pop_scope();
        list.pop_scope();
        let scene = LintScene::from_paint_list(&list);
        let report = lint(&scene, &LintConfig::new());
        assert!(!findings_for(&report, "text-truncation").is_empty());
    }

    #[test]
    fn text_truncation_quiet_when_contained() {
        let scene = text_on_fill([230, 230, 230, 255], [20, 20, 24, 255]);
        let report = lint(&scene, &LintConfig::new());
        assert!(findings_for(&report, "text-truncation").is_empty());
    }

    // ---------- target-spacing ----------

    #[test]
    fn target_spacing_flags_packed_controls() {
        let mut list = app_list();
        list.push_scope(None, "Toolbar", Rect::new(0.0, 0.0, 300.0, 40.0));
        for i in 0..3 {
            let x = i as f64 * 32.0;
            list.push_scope(None, "Button", Rect::new(x, 4.0, x + 30.0, 36.0));
            list.pop_scope();
        }
        list.pop_scope();
        list.pop_scope();
        let mut scene = LintScene::from_paint_list(&list);
        scene.scale_factor = 1.0;
        let report = lint(&scene, &LintConfig::new());
        let hits = findings_for(&report, "target-spacing");
        assert_eq!(hits.len(), 1, "expected one finding, got {hits:?}");
        assert!(hits[0].fix.is_some());
    }

    #[test]
    fn target_spacing_quiet_with_room() {
        let mut list = app_list();
        list.push_scope(None, "Toolbar", Rect::new(0.0, 0.0, 300.0, 40.0));
        for i in 0..3 {
            let x = i as f64 * 60.0;
            list.push_scope(None, "Button", Rect::new(x, 4.0, x + 30.0, 36.0));
            list.pop_scope();
        }
        list.pop_scope();
        list.pop_scope();
        let mut scene = LintScene::from_paint_list(&list);
        scene.scale_factor = 1.0;
        let report = lint(&scene, &LintConfig::new());
        assert!(findings_for(&report, "target-spacing").is_empty());
    }

    // ---------- icon-only-control ----------

    #[test]
    fn icon_only_flags_textless_button() {
        let mut list = app_list();
        list.push_scope(None, "Panel", Rect::new(0.0, 0.0, 400.0, 200.0));
        list.push_scope(None, "IconButton", Rect::new(10.0, 10.0, 40.0, 40.0));
        list.pop_scope();
        list.push_scope(None, "LabeledButton", Rect::new(60.0, 10.0, 200.0, 40.0));
        list.commands.push(PaintCommand::DrawText(
            Point::new(70.0, 32.0),
            "Save".to_string(),
            12.0,
            [230, 230, 230, 255],
        ));
        list.pop_scope();
        list.pop_scope();
        list.pop_scope();
        let scene = LintScene::from_paint_list(&list);
        let report = lint(&scene, &LintConfig::new());
        let hits = findings_for(&report, "icon-only-control");
        assert_eq!(hits.len(), 1, "expected one finding, got {hits:?}");
        assert!(hits[0].path.contains("IconButton"));
    }

    #[test]
    fn icon_only_quiet_when_labeled() {
        let scene = text_on_fill([230, 230, 230, 255], [20, 20, 24, 255]);
        let report = lint(&scene, &LintConfig::new());
        assert!(findings_for(&report, "icon-only-control").is_empty());
    }

    // ---------- reading-order ----------

    #[test]
    fn reading_order_flags_inverted_siblings() {
        let mut list = app_list();
        list.push_scope(None, "Panel", Rect::new(0.0, 0.0, 400.0, 100.0));
        // Painted left-to-right order: C, A, B (visual positions
        // shuffled vs document order).
        list.push_scope(None, "C", Rect::new(200.0, 0.0, 300.0, 40.0));
        list.pop_scope();
        list.push_scope(None, "A", Rect::new(0.0, 0.0, 100.0, 40.0));
        list.pop_scope();
        list.push_scope(None, "B", Rect::new(100.0, 0.0, 200.0, 40.0));
        list.pop_scope();
        list.pop_scope();
        list.pop_scope();
        let scene = LintScene::from_paint_list(&list);
        let report = lint(&scene, &LintConfig::new());
        assert!(!findings_for(&report, "reading-order").is_empty());
    }

    #[test]
    fn reading_order_quiet_when_consistent() {
        let mut list = app_list();
        list.push_scope(None, "Panel", Rect::new(0.0, 0.0, 400.0, 100.0));
        list.push_scope(None, "A", Rect::new(0.0, 0.0, 100.0, 40.0));
        list.pop_scope();
        list.push_scope(None, "B", Rect::new(100.0, 0.0, 200.0, 40.0));
        list.pop_scope();
        list.push_scope(None, "C", Rect::new(200.0, 0.0, 300.0, 40.0));
        list.pop_scope();
        list.pop_scope();
        list.pop_scope();
        let scene = LintScene::from_paint_list(&list);
        let report = lint(&scene, &LintConfig::new());
        assert!(findings_for(&report, "reading-order").is_empty());
    }

    // ---------- overflow-clip ----------

    #[test]
    fn overflow_clip_flags_escaping_fill() {
        let mut list = app_list();
        list.push_scope(None, "Panel", Rect::new(10.0, 10.0, 100.0, 60.0));
        list.commands.push(PaintCommand::FillRect(
            Rect::new(10.0, 10.0, 130.0, 60.0), // 30px past x1
            [60, 60, 66, 255],
        ));
        list.pop_scope();
        list.pop_scope();
        let scene = LintScene::from_paint_list(&list);
        let report = lint(&scene, &LintConfig::new());
        assert!(!findings_for(&report, "overflow-clip").is_empty());
    }

    #[test]
    fn overflow_clip_quiet_when_contained() {
        let scene = text_on_fill([230, 230, 230, 255], [20, 20, 24, 255]);
        let report = lint(&scene, &LintConfig::new());
        assert!(findings_for(&report, "overflow-clip").is_empty());
    }

    #[test]
    fn overflow_clip_quiet_for_clipped_fill() {
        // A fill oversized under a scope-tight clip is intentional —
        // the clip hides the excess, so nothing visibly escapes.
        let mut list = app_list();
        list.push_scope(None, "Panel", Rect::new(10.0, 10.0, 100.0, 60.0));
        list.commands
            .push(PaintCommand::ClipRect(Rect::new(10.0, 10.0, 100.0, 60.0)));
        list.commands.push(PaintCommand::FillRect(
            Rect::new(10.0, 10.0, 230.0, 60.0), // 130px past x1, all clipped away
            [60, 60, 66, 255],
        ));
        list.pop_scope();
        list.pop_scope();
        let scene = LintScene::from_paint_list(&list);
        let report = lint(&scene, &LintConfig::new());
        assert!(
            findings_for(&report, "overflow-clip").is_empty(),
            "clipped fill flagged as overflow"
        );
    }

    #[test]
    fn overflow_clip_flags_when_clip_doesnt_hide_excess() {
        // A clip larger than the fill hides nothing — the escape is
        // still visible and must flag.
        let mut list = app_list();
        list.push_scope(None, "Panel", Rect::new(10.0, 10.0, 100.0, 60.0));
        list.commands
            .push(PaintCommand::ClipRect(Rect::new(0.0, 0.0, 400.0, 400.0)));
        list.commands.push(PaintCommand::FillRect(
            Rect::new(10.0, 10.0, 130.0, 60.0),
            [60, 60, 66, 255],
        ));
        list.pop_scope();
        list.pop_scope();
        let scene = LintScene::from_paint_list(&list);
        let report = lint(&scene, &LintConfig::new());
        assert!(!findings_for(&report, "overflow-clip").is_empty());
    }

    #[test]
    fn nontext_contrast_needs_perpendicular_overlap() {
        // CardA's right edge x=200 coincidentally equals CardB's left
        // edge — but they sit on different rows. Sharing one
        // coordinate is not a boundary.
        let mut list = app_list();
        list.push_scope(None, "Panel", Rect::new(0.0, 0.0, 400.0, 300.0));
        list.push_scope(None, "CardA", Rect::new(0.0, 0.0, 200.0, 100.0));
        list.commands.push(PaintCommand::FillRect(
            Rect::new(0.0, 0.0, 200.0, 100.0),
            [40, 40, 44, 255],
        ));
        list.pop_scope();
        list.push_scope(None, "CardB", Rect::new(200.0, 200.0, 400.0, 300.0));
        list.commands.push(PaintCommand::FillRect(
            Rect::new(200.0, 200.0, 400.0, 300.0),
            [44, 44, 48, 255],
        ));
        list.pop_scope();
        list.pop_scope();
        list.pop_scope();
        let scene = LintScene::from_paint_list(&list);
        let report = lint(&scene, &LintConfig::new());
        assert!(
            findings_for(&report, "nontext-contrast").is_empty(),
            "coincidental edge match flagged as invisible boundary"
        );
    }

    #[test]
    fn adjust_to_contrast_picks_the_shorter_direction() {
        // Mid-tone background: darkening a dark text is a short move,
        // lightening is a long one — the fix must pick the minimum
        // blend that reaches the ratio, not guess from bg luminance.
        let bg = [128, 128, 128, 255];
        let fg = [100, 100, 100, 255];
        let fixed = adjust_to_contrast(fg, bg, 4.5);
        assert!(contrast_ratio(fixed, bg) >= 4.5 - 0.05);
        // Darkening wins — the result should be darker than the input.
        assert!(fixed[0] < fg[0], "picked the long-way blend: {fixed:?}");
    }
}
