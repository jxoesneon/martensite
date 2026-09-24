//! Gestalt & layout-geometry rules — the wave-2 expansion covering
//! perceptual-balance metrics, structural regularity, and
//! interaction-geometry hazards.
//!
//! Everything here is a *heuristic* by design: the metrics
//! (Miniukovich's balance/symmetry/regularity, Tufte's data-ink,
//! Fitts-adjacency) measure real geometry, but "too much" is a design
//! judgment, so findings are review prompts at `info`/`warn`, never
//! build-blocking facts.

use std::collections::{BTreeMap, BTreeSet, HashSet};

use kurbo::Rect;

use crate::config::LintConfig;
use crate::report::Finding;
use crate::rule::{param, Confidence, LintRule};
use crate::rules::{interactive_leaves, is_alarm_red, kind_of, marker_in_lineage, surface_nodes};
use crate::scene::{LintNode, LintScene, NodeKind};
use crate::severity::Severity;
use crate::standard::Standard;

/// — `balance`: painted-mass centroid vs geometric center.
///
/// Miniukovich & De Angeli's balance metric: the area-weighted center
/// of everything painted in a surface should sit near the surface's
/// own center. A strong horizontal or vertical pull reads as
/// lopsided even when every element is individually aligned —
/// balance is a global property the eye computes before content.
struct Balance;
impl LintRule for Balance {
    fn id(&self) -> &'static str {
        "balance"
    }
    fn standards(&self) -> &'static [Standard] {
        &[Standard::InfoDesign, Standard::Perception]
    }
    fn default_severity(&self) -> Severity {
        Severity::Info
    }
    fn confidence(&self) -> Confidence {
        Confidence::Heuristic
    }
    fn title(&self) -> &'static str {
        "painted-mass centroid offset from the surface center"
    }
    fn citation(&self) -> &'static str {
        "Miniukovich & De Angeli, CHI 2015 — balance is a validated computable aesthetic metric"
    }
    fn check(&self, scene: &LintScene, cfg: &LintConfig) -> Vec<Finding> {
        let max_frac = param(cfg, self.id(), "max_offset_frac", 0.25);
        let mut out = Vec::new();
        for n in surface_nodes(scene, cfg, self.id()) {
            let (mut mass, mut cx, mut cy) = (0.0, 0.0, 0.0);
            for d in n.walk() {
                for f in &d.fills {
                    let r = f.rect.intersect(n.bounds);
                    // Clamp per dimension — a disjoint intersection has
                    // two negative extents whose product is positive.
                    let a = r.width().max(0.0) * r.height().max(0.0);
                    if a <= 0.0 {
                        continue;
                    }
                    mass += a;
                    cx += a * (r.x0 + r.x1) / 2.0;
                    cy += a * (r.y0 + r.y1) / 2.0;
                }
            }
            if mass <= 0.0 {
                continue;
            }
            let bcx = (n.bounds.x0 + n.bounds.x1) / 2.0;
            let bcy = (n.bounds.y0 + n.bounds.y1) / 2.0;
            let (off_x, off_y) = ((cx / mass - bcx).abs(), (cy / mass - bcy).abs());
            let (lim_x, lim_y) = (max_frac * n.bounds.width(), max_frac * n.bounds.height());
            if off_x > lim_x || off_y > lim_y {
                out.push(
                    Finding::new(
                        "balance",
                        &n.path,
                        format!(
                            "painted-mass centroid sits {off_x:.0}px off-center horizontally, \
                             {off_y:.0}px vertically (limit {:.0}% of extent) — the surface \
                             reads lopsided even if every element is aligned",
                            max_frac * 100.0
                        ),
                    )
                    .at(n.bounds),
                );
            }
        }
        out
    }
}

/// — `symmetry`: left/right painted-mass split.
///
/// Reflectional symmetry across the vertical midline — the strongest
/// single cue for "calm, ordered surface". Rails, sidebars, and nav
/// columns are *legitimately* asymmetric (the whole point of a rail
/// is an off-center weight), so nodes whose names say so are skipped.
struct Symmetry;
impl LintRule for Symmetry {
    fn id(&self) -> &'static str {
        "symmetry"
    }
    fn standards(&self) -> &'static [Standard] {
        &[Standard::InfoDesign, Standard::Perception]
    }
    fn default_severity(&self) -> Severity {
        Severity::Info
    }
    fn confidence(&self) -> Confidence {
        Confidence::Heuristic
    }
    fn title(&self) -> &'static str {
        "left/right painted-mass asymmetry per surface"
    }
    fn citation(&self) -> &'static str {
        "Miniukovich & De Angeli, CHI 2015 — symmetry/regularity aesthetic metrics; \
         Gestalt symmetry & order (Prägnanz)"
    }
    fn check(&self, scene: &LintScene, cfg: &LintConfig) -> Vec<Finding> {
        let max_asym = param(cfg, self.id(), "max_asymmetry", 0.5);
        let mut out = Vec::new();
        for n in surface_nodes(scene, cfg, self.id()) {
            let lower = n.name.to_ascii_lowercase();
            if lower.contains("rail") || lower.contains("sidebar") || lower.contains("nav") {
                continue; // asymmetric chrome is a deliberate layout choice
            }
            let mid = (n.bounds.x0 + n.bounds.x1) / 2.0;
            let (mut left, mut right) = (0.0, 0.0);
            for d in n.walk() {
                for f in &d.fills {
                    let r = f.rect.intersect(n.bounds);
                    let h = r.height().max(0.0);
                    left += (r.x1.min(mid) - r.x0).max(0.0) * h;
                    right += (r.x1 - r.x0.max(mid)).max(0.0) * h;
                }
            }
            let total = left + right;
            if total <= 0.0 {
                continue;
            }
            let asym = (left - right).abs() / total;
            if asym > max_asym {
                out.push(
                    Finding::new(
                        "symmetry",
                        &n.path,
                        format!(
                            "painted mass is {:.0}% asymmetric left/right (max {:.0}%) — \
                             visual weight piles onto one side of the midline",
                            asym * 100.0,
                            max_asym * 100.0
                        ),
                    )
                    .at(n.bounds),
                );
            }
        }
        out
    }
}

/// — `regularity`: same-name siblings should share geometry.
///
/// Miniukovich's regularity metric: a group of same-kind siblings (a
/// card list, a chip row, a toolbar strip) is perceived as *one
/// pattern* only when the members agree in size. High coefficient of
/// variation in either dimension means the "group" is really a
/// collection of one-offs.
struct Regularity;
impl LintRule for Regularity {
    fn id(&self) -> &'static str {
        "regularity"
    }
    fn standards(&self) -> &'static [Standard] {
        &[Standard::InfoDesign, Standard::Perception]
    }
    fn default_severity(&self) -> Severity {
        Severity::Info
    }
    fn confidence(&self) -> Confidence {
        Confidence::Heuristic
    }
    fn title(&self) -> &'static str {
        "same-kind siblings with irregular dimensions"
    }
    fn citation(&self) -> &'static str {
        "Miniukovich & De Angeli regularity metric — uniform repetition reads as one pattern"
    }
    fn check(&self, scene: &LintScene, cfg: &LintConfig) -> Vec<Finding> {
        let min_group = param(cfg, self.id(), "min_group", 4.0) as usize;
        let max_cv = param(cfg, self.id(), "max_cv", 0.15);
        let mut out = Vec::new();
        for p in scene.walk() {
            let mut groups: BTreeMap<&str, Vec<&LintNode>> = BTreeMap::new();
            for c in &p.children {
                if c.area() > 1.0 {
                    groups.entry(c.name.as_str()).or_default().push(c);
                }
            }
            for (name, group) in groups {
                if group.len() < min_group {
                    continue;
                }
                let cv = |f: fn(&LintNode) -> f64| -> f64 {
                    let vals: Vec<f64> = group.iter().map(|c| f(c)).collect();
                    let mean = vals.iter().sum::<f64>() / vals.len() as f64;
                    if mean <= 0.0 {
                        return 0.0;
                    }
                    let var =
                        vals.iter().map(|v| (v - mean).powi(2)).sum::<f64>() / vals.len() as f64;
                    var.sqrt() / mean
                };
                let (cv_w, cv_h) = (cv(|c| c.bounds.width()), cv(|c| c.bounds.height()));
                if cv_w > max_cv || cv_h > max_cv {
                    out.push(
                        Finding::new(
                            "regularity",
                            &p.path,
                            format!(
                                "{} '{name}' siblings vary irregularly — width CV {cv_w:.2}, \
                                 height CV {cv_h:.2} (max {max_cv}) — same-kind siblings \
                                 should be uniform",
                                group.len()
                            ),
                        )
                        .at(p.bounds),
                    );
                }
            }
        }
        out
    }
}

/// — `redundant-border`: divider lines between already-separated siblings.
///
/// Tufte's data-ink audit: when whitespace alone already encodes a
/// separation, a drawn divider *double-encodes* it — pure non-data
/// ink. Detects thin fills (one dimension under `divider_max_px`,
/// the other spanning >`min_span_frac` of the parent's extent)
/// sitting inside a sibling gap that already exceeds `min_gap_pt`.
struct RedundantBorder;
impl LintRule for RedundantBorder {
    fn id(&self) -> &'static str {
        "redundant-border"
    }
    fn standards(&self) -> &'static [Standard] {
        &[Standard::InfoDesign, Standard::Perception]
    }
    fn default_severity(&self) -> Severity {
        Severity::Info
    }
    fn confidence(&self) -> Confidence {
        Confidence::Heuristic
    }
    fn title(&self) -> &'static str {
        "divider lines where whitespace already encodes the separation"
    }
    fn citation(&self) -> &'static str {
        "Tufte, data-ink ratio — erase non-data ink; Gestalt proximity groups without borders"
    }
    fn check(&self, scene: &LintScene, cfg: &LintConfig) -> Vec<Finding> {
        let thin = param(cfg, self.id(), "divider_max_px", 3.0);
        let min_gap = param(cfg, self.id(), "min_gap_pt", 8.0) * f64::from(scene.scale_factor);
        let min_span = param(cfg, self.id(), "min_span_frac", 0.4);
        const EPS: f64 = 0.5;
        let mut out = Vec::new();
        for p in scene.walk() {
            let children: Vec<&LintNode> = p.children.iter().filter(|c| c.area() > 1.0).collect();
            if children.len() < 2 {
                continue;
            }
            // Candidate dividers: fills the parent paints itself, plus
            // fills of direct children that *are* divider widgets
            // (thin bounds or a divider-ish debug name).
            let mut candidates: Vec<(Option<&LintNode>, Rect)> =
                p.fills.iter().map(|f| (None, f.rect)).collect();
            for c in &p.children {
                let cname = c.name.to_ascii_lowercase();
                let divider_named = cname.contains("divider")
                    || cname.contains("separator")
                    || cname.contains("rule")
                    || cname == "hr";
                let divider_shaped = c.bounds.height() <= thin || c.bounds.width() <= thin;
                if !divider_named && !divider_shaped {
                    continue;
                }
                candidates.extend(c.fills.iter().map(|f| (Some(c), f.rect)));
            }
            for (owner, r) in candidates {
                let siblings: Vec<&LintNode> = children
                    .iter()
                    .copied()
                    .filter(|c| !owner.is_some_and(|o| std::ptr::eq(*c, o)))
                    .collect();
                if siblings.len() < 2 {
                    continue;
                }
                let (w, h) = (r.width(), r.height());
                // Horizontal rule: thin in Y, spanning in X, between
                // vertically-adjacent siblings.
                if h > 0.0 && h <= thin && w > min_span * p.bounds.width() {
                    let x_overlaps = |c: &LintNode| c.bounds.x1 > r.x0 && c.bounds.x0 < r.x1;
                    // No x-overlapping sibling may share the rule's
                    // y-band — it must live inside the gap.
                    let in_gap = siblings
                        .iter()
                        .filter(|c| x_overlaps(c))
                        .all(|c| c.bounds.y1 <= r.y0 + EPS || c.bounds.y0 >= r.y1 - EPS);
                    let above = siblings
                        .iter()
                        .filter(|c| x_overlaps(c) && c.bounds.y1 <= r.y0 + EPS)
                        .map(|c| c.bounds.y1)
                        .fold(f64::NEG_INFINITY, f64::max);
                    let below = siblings
                        .iter()
                        .filter(|c| x_overlaps(c) && c.bounds.y0 >= r.y1 - EPS)
                        .map(|c| c.bounds.y0)
                        .fold(f64::INFINITY, f64::min);
                    if in_gap && above.is_finite() && below.is_finite() && below - above >= min_gap
                    {
                        out.push(
                            Finding::new(
                                "redundant-border",
                                &p.path,
                                format!(
                                    "{h:.0}px divider inside a {:.0}px sibling gap (≥ \
                                     {:.0}pt already separates them) — border + spacing \
                                     double-encodes the separation; erase the non-data ink",
                                    below - above,
                                    min_gap / f64::from(scene.scale_factor)
                                ),
                            )
                            .at(r),
                        );
                    }
                    continue;
                }
                // Vertical rule: thin in X, spanning in Y, between
                // horizontally-adjacent siblings.
                if w > 0.0 && w <= thin && h > min_span * p.bounds.height() {
                    let y_overlaps = |c: &LintNode| c.bounds.y1 > r.y0 && c.bounds.y0 < r.y1;
                    let in_gap = siblings
                        .iter()
                        .filter(|c| y_overlaps(c))
                        .all(|c| c.bounds.x1 <= r.x0 + EPS || c.bounds.x0 >= r.x1 - EPS);
                    let left = siblings
                        .iter()
                        .filter(|c| y_overlaps(c) && c.bounds.x1 <= r.x0 + EPS)
                        .map(|c| c.bounds.x1)
                        .fold(f64::NEG_INFINITY, f64::max);
                    let right = siblings
                        .iter()
                        .filter(|c| y_overlaps(c) && c.bounds.x0 >= r.x1 - EPS)
                        .map(|c| c.bounds.x0)
                        .fold(f64::INFINITY, f64::min);
                    if in_gap && left.is_finite() && right.is_finite() && right - left >= min_gap {
                        out.push(
                            Finding::new(
                                "redundant-border",
                                &p.path,
                                format!(
                                    "{w:.0}px divider inside a {:.0}px sibling gap (≥ \
                                     {:.0}pt already separates them) — border + spacing \
                                     double-encodes the separation; erase the non-data ink",
                                    right - left,
                                    min_gap / f64::from(scene.scale_factor)
                                ),
                            )
                            .at(r),
                        );
                    }
                }
            }
        }
        out
    }
}

/// — `empty-surface`: allocated area carrying almost no paint.
///
/// The inverse of `density`: a surface-sized scope whose whole
/// subtree paints a negligible fraction of its bounds is dead space
/// — an empty panel, a placeholder never filled, or padding that ate
/// the content. Chrome kinds (status/title bars) are legitimately
/// sparse and skipped.
struct EmptySurface;
impl LintRule for EmptySurface {
    fn id(&self) -> &'static str {
        "empty-surface"
    }
    fn standards(&self) -> &'static [Standard] {
        &[Standard::InfoDesign, Standard::Perception]
    }
    fn default_severity(&self) -> Severity {
        Severity::Info
    }
    fn confidence(&self) -> Confidence {
        Confidence::Heuristic
    }
    fn title(&self) -> &'static str {
        "surface-scale scope carrying almost no painted content"
    }
    fn citation(&self) -> &'static str {
        "Tufte data-ink / Few dashboard canon — space is the scarcest resource; \
         inverse of the density metric"
    }
    fn check(&self, scene: &LintScene, cfg: &LintConfig) -> Vec<Finding> {
        let min_cov = param(cfg, self.id(), "min_coverage", 0.03);
        let mut out = Vec::new();
        for n in surface_nodes(scene, cfg, self.id()) {
            if kind_of(n, cfg) == NodeKind::Chrome {
                continue;
            }
            // What the surface *carries*: painted coverage across the
            // whole subtree, capped at the surface's own area.
            let carried: f64 = n.walk().map(|d| d.painted_area).sum();
            let cov = (carried / n.area().max(1.0)).min(1.0);
            if cov < min_cov {
                out.push(
                    Finding::new(
                        "empty-surface",
                        &n.path,
                        format!(
                            "subtree paints {:.1}% of this surface's area (min {:.0}%) — \
                             allocated space carrying almost nothing",
                            cov * 100.0,
                            min_cov * 100.0
                        ),
                    )
                    .at(n.bounds),
                );
            }
        }
        out
    }
}

/// — `aspect-extreme`: unusable ribbon-strip geometry.
///
/// A node whose aspect ratio exceeds `max_aspect` (default 12:1)
/// while covering real area is a sliver — too thin to host legible
/// content along its short axis. Almost always a layout accident:
/// a stretch that ate a sibling's cross-axis size.
struct AspectExtreme;
impl LintRule for AspectExtreme {
    fn id(&self) -> &'static str {
        "aspect-extreme"
    }
    fn standards(&self) -> &'static [Standard] {
        &[Standard::InfoDesign, Standard::Perception]
    }
    fn default_severity(&self) -> Severity {
        Severity::Info
    }
    fn confidence(&self) -> Confidence {
        Confidence::Heuristic
    }
    fn title(&self) -> &'static str {
        "nodes with extreme (ribbon-strip) aspect ratios"
    }
    fn citation(&self) -> &'static str {
        "layout sanity bound — beyond ~12:1 a region can't host legible content on its short axis"
    }
    fn check(&self, scene: &LintScene, cfg: &LintConfig) -> Vec<Finding> {
        let sf = f64::from(scene.scale_factor);
        let min_area = param(cfg, self.id(), "min_area_pt", 4_000.0) * sf * sf;
        let max_aspect = param(cfg, self.id(), "max_aspect", 12.0);
        let mut out = Vec::new();
        for n in scene.walk() {
            let (w, h) = (n.bounds.width(), n.bounds.height());
            if w <= 0.0 || h <= 0.0 || w * h <= min_area {
                continue;
            }
            let aspect = w / h;
            if aspect > max_aspect || aspect < 1.0 / max_aspect {
                out.push(
                    Finding::new(
                        "aspect-extreme",
                        &n.path,
                        format!(
                            "{w:.0}×{h:.0}px — {aspect:.1}:1 aspect ratio (limit \
                             {max_aspect:.0}:1) — a ribbon strip this thin can't host \
                             usable content"
                        ),
                    )
                    .at(n.bounds),
                );
            }
        }
        out
    }
}

/// — `line-length`: text runs outside the readability band.
///
/// The 45–75 character line-length canon (Bringhurst, WCAG 1.4.8's
/// 80-char ceiling for body text). When a run's measured advance
/// width is known (glyph runs), characters are estimated at ~0.5em
/// each; `DrawText` runs count characters directly. Reports the
/// worst run per node.
struct LineLength;
impl LintRule for LineLength {
    fn id(&self) -> &'static str {
        "line-length"
    }
    fn standards(&self) -> &'static [Standard] {
        &[Standard::InfoDesign]
    }
    fn default_severity(&self) -> Severity {
        Severity::Info
    }
    fn confidence(&self) -> Confidence {
        Confidence::Heuristic
    }
    fn title(&self) -> &'static str {
        "text lines exceeding the 45–75ch readability band"
    }
    fn citation(&self) -> &'static str {
        "Bringhurst ~66ch optimum; WCAG 1.4.8 ≤80ch — long lines force return-sweep re-finding"
    }
    fn check(&self, scene: &LintScene, cfg: &LintConfig) -> Vec<Finding> {
        let max_chars = param(cfg, self.id(), "max_chars", 75.0);
        let mut out = Vec::new();
        for n in scene.walk() {
            // Worst run in this node, measured in characters.
            let worst = n
                .texts
                .iter()
                .filter_map(|t| {
                    if let Some(w) = t.width {
                        // ~0.5em average advance per character.
                        let est = w / (f64::from(t.size) * 0.5).max(1.0);
                        Some((est, w, None))
                    } else if !t.text.is_empty() {
                        let count = t.text.chars().count() as f64;
                        Some((count, 0.0, Some(t.text.clone())))
                    } else {
                        None
                    }
                })
                .max_by(|a, b| a.0.total_cmp(&b.0));
            let Some((chars, width_px, text)) = worst else {
                continue;
            };
            if chars > max_chars {
                let detail = match &text {
                    Some(s) => {
                        let preview: String = s.chars().take(40).collect();
                        format!("{chars:.0} characters ({preview:?}…)")
                    }
                    None => format!("≈{chars:.0} characters ({width_px:.0}px wide)"),
                };
                out.push(
                    Finding::new(
                        "line-length",
                        &n.path,
                        format!(
                            "text run is {detail} — past the {max_chars:.0}ch ceiling of the \
                             45–75ch readability band; the eye loses the next line on the \
                             return sweep"
                        ),
                    )
                    .at(n.bounds),
                );
            }
        }
        out
    }
}

/// — `grid-drift`: column edges that don't line up across rows.
///
/// For a parent whose children each hold ≥2 children (a grid or
/// table: rows × cells), the i-th cell of every row should share one
/// left edge and one right edge. When a column's edges drift more
/// than `tolerance_px` across rows the table stops reading as a
/// table — the eye loses the column track. Reported without a fix:
/// the cells live under different parents, so no sibling-align op
/// applies.
struct GridDrift;
impl LintRule for GridDrift {
    fn id(&self) -> &'static str {
        "grid-drift"
    }
    fn standards(&self) -> &'static [Standard] {
        &[Standard::InfoDesign, Standard::Perception]
    }
    fn default_severity(&self) -> Severity {
        Severity::Info
    }
    fn confidence(&self) -> Confidence {
        Confidence::Heuristic
    }
    fn title(&self) -> &'static str {
        "grid column edges drifting across rows"
    }
    fn citation(&self) -> &'static str {
        "tabular alignment — shared column edges are what makes a grid read as a grid; \
         Miniukovich alignment metric applied across rows"
    }
    fn check(&self, scene: &LintScene, cfg: &LintConfig) -> Vec<Finding> {
        let tol = param(cfg, self.id(), "tolerance_px", 2.0);
        let mut out = Vec::new();
        for p in scene.walk() {
            let rows: Vec<Vec<&LintNode>> = p
                .children
                .iter()
                .filter(|r| r.children.len() >= 2)
                .map(|r| {
                    let mut cells: Vec<&LintNode> =
                        r.children.iter().filter(|c| c.area() > 1.0).collect();
                    cells.sort_by(|a, b| {
                        a.bounds
                            .x0
                            .partial_cmp(&b.bounds.x0)
                            .unwrap_or(std::cmp::Ordering::Equal)
                    });
                    cells
                })
                .filter(|cells| cells.len() >= 2)
                .collect();
            if rows.len() < 2 {
                continue;
            }
            let cols = rows.iter().map(|r| r.len()).min().unwrap_or(0);
            // Worst (column, spread, which edge) across the grid.
            let mut worst: Option<(usize, f64, &'static str)> = None;
            for i in 0..cols {
                let spread = |pick: fn(&LintNode) -> f64| -> f64 {
                    let (mut lo, mut hi) = (f64::INFINITY, f64::NEG_INFINITY);
                    for row in &rows {
                        let v = pick(row[i]);
                        lo = lo.min(v);
                        hi = hi.max(v);
                    }
                    hi - lo
                };
                for (s, edge) in [
                    (spread(|c| c.bounds.x0), "left"),
                    (spread(|c| c.bounds.x1), "right"),
                ] {
                    if s > tol && worst.is_none_or(|(_, ws, _)| s > ws) {
                        worst = Some((i, s, edge));
                    }
                }
            }
            if let Some((col, spread, edge)) = worst {
                out.push(
                    Finding::new(
                        "grid-drift",
                        &p.path,
                        format!(
                            "column {} {edge} edges drift {spread:.1}px across {} rows \
                             (tolerance {tol:.0}px) — align the column edges; a table \
                             reads as a table only when columns share tracks",
                            col + 1,
                            rows.len()
                        ),
                    )
                    .at(p.bounds),
                );
            }
        }
        out
    }
}

/// — `heading-rhythm`: deeper levels shouldn't set larger type.
///
/// Typographic hierarchy is the reading order of a surface: the
/// surface's own title sets the ceiling, and everything below it
/// should set *smaller*. A descendant subtree that out-sizes its
/// parent's own text inverts the pyramid — a footnote shouting over
/// the headline.
struct HeadingRhythm;
impl LintRule for HeadingRhythm {
    fn id(&self) -> &'static str {
        "heading-rhythm"
    }
    fn standards(&self) -> &'static [Standard] {
        &[Standard::InfoDesign, Standard::Perception]
    }
    fn default_severity(&self) -> Severity {
        Severity::Info
    }
    fn confidence(&self) -> Confidence {
        Confidence::Heuristic
    }
    fn title(&self) -> &'static str {
        "descendant text larger than its surface's own text"
    }
    fn citation(&self) -> &'static str {
        "typographic hierarchy — size encodes rank; deeper levels set smaller, \
         never larger (Bringhurst; every design-system type ramp)"
    }
    fn check(&self, scene: &LintScene, cfg: &LintConfig) -> Vec<Finding> {
        let tol = param(cfg, self.id(), "tolerance_pt", 0.5) * f64::from(scene.scale_factor);
        let mut out = Vec::new();
        for n in surface_nodes(scene, cfg, self.id()) {
            let Some(parent_max) = n.font_sizes.iter().copied().reduce(f32::max) else {
                continue; // no own text — no ceiling to break
            };
            for c in &n.children {
                let Some(child_max) = c
                    .walk()
                    .flat_map(|d| d.font_sizes.iter().copied())
                    .reduce(f32::max)
                else {
                    continue;
                };
                if f64::from(child_max - parent_max) > tol {
                    out.push(
                        Finding::new(
                            "heading-rhythm",
                            &c.path,
                            format!(
                                "subtree text peaks at {child_max:.0}px but the surface's \
                                 own text peaks at {parent_max:.0}px — deeper levels \
                                 shouldn't set larger type than their parent"
                            ),
                        )
                        .at(c.bounds),
                    );
                }
            }
        }
        out
    }
}

/// — `baseline-drift`: sibling text that shares a row but not a baseline.
///
/// Side-by-side siblings carrying same-size text should share one
/// baseline. When the closest matching baselines still differ by
/// more than `tolerance_px` the row reads ragged — the typographic
/// equivalent of the `alignment` rule's edge scatter.
struct BaselineDrift;
impl LintRule for BaselineDrift {
    fn id(&self) -> &'static str {
        "baseline-drift"
    }
    fn standards(&self) -> &'static [Standard] {
        &[Standard::InfoDesign, Standard::Perception]
    }
    fn default_severity(&self) -> Severity {
        Severity::Info
    }
    fn confidence(&self) -> Confidence {
        Confidence::Heuristic
    }
    fn title(&self) -> &'static str {
        "same-size sibling text on different baselines"
    }
    fn citation(&self) -> &'static str {
        "baseline-grid discipline — a shared baseline is what makes a row read as one line"
    }
    fn check(&self, scene: &LintScene, cfg: &LintConfig) -> Vec<Finding> {
        let tol = param(cfg, self.id(), "tolerance_px", 1.0);
        let mut out = Vec::new();
        for p in scene.walk() {
            let children: Vec<&LintNode> = p.children.iter().filter(|c| c.area() > 1.0).collect();
            if children.len() < 2 {
                continue;
            }
            // Worst drift among y-band-sharing sibling pairs.
            let mut worst: Option<(f64, String, String)> = None;
            for i in 0..children.len() {
                for j in (i + 1)..children.len() {
                    let (a, b) = (children[i], children[j]);
                    // Same y-band: the siblings overlap vertically by
                    // more than half the shorter one — they're a row.
                    let overlap = a.bounds.y1.min(b.bounds.y1) - a.bounds.y0.max(b.bounds.y0);
                    let shorter = a.bounds.height().min(b.bounds.height());
                    if overlap <= 0.5 * shorter {
                        continue;
                    }
                    let ta: Vec<_> = a.walk().flat_map(|d| d.texts.iter()).collect();
                    let tb: Vec<_> = b.walk().flat_map(|d| d.texts.iter()).collect();
                    // Closest baseline among same-size runs: if even
                    // the best pair misses, the row shares no line.
                    let mut closest = f64::INFINITY;
                    for t in &ta {
                        for u in &tb {
                            if (t.size - u.size).abs() <= 0.5 {
                                closest = closest.min((t.origin.y - u.origin.y).abs());
                            }
                        }
                    }
                    if closest.is_finite() && closest > tol {
                        let entry = (closest, a.name.clone(), b.name.clone());
                        if worst.as_ref().is_none_or(|(w, _, _)| closest > *w) {
                            worst = Some(entry);
                        }
                    }
                }
            }
            if let Some((drift, na, nb)) = worst {
                out.push(
                    Finding::new(
                        "baseline-drift",
                        &p.path,
                        format!(
                            "'{na}' and '{nb}' carry same-size text {drift:.1}px off a \
                             shared baseline (tolerance {tol:.0}px) — the row reads ragged"
                        ),
                    )
                    .at(p.bounds),
                );
            }
        }
        out
    }
}

/// — `duplicate-action`: identical labels on distinct controls.
///
/// Two interactive controls under one surface with the same label
/// ("Save" next to "Save") force the user to disambiguate by position
/// or context — a comprehension tax on every use, and an automation/
/// accessibility hazard (screen readers announce identical names).
struct DuplicateAction;
impl LintRule for DuplicateAction {
    fn id(&self) -> &'static str {
        "duplicate-action"
    }
    fn standards(&self) -> &'static [Standard] {
        &[Standard::Consistency, Standard::InfoDesign]
    }
    fn default_severity(&self) -> Severity {
        Severity::Warn
    }
    fn confidence(&self) -> Confidence {
        Confidence::Heuristic
    }
    fn title(&self) -> &'static str {
        "same label on multiple controls in one surface"
    }
    fn citation(&self) -> &'static str {
        "ambiguous labeling — identical action names defeat scan-and-act; \
         WCAG-adjacent unique-name guidance for controls"
    }
    fn check(&self, scene: &LintScene, cfg: &LintConfig) -> Vec<Finding> {
        let mut out = Vec::new();
        // (normalized label, sorted leaf paths) — so nested surfaces
        // containing the same duplicated pair report only once.
        let mut reported: HashSet<(String, String)> = HashSet::new();
        for n in surface_nodes(scene, cfg, self.id()) {
            // Vec, not a set — same-name siblings share a path
            // (`App/Panel/Button`) and must each count.
            let mut labels: BTreeMap<String, Vec<&str>> = BTreeMap::new();
            for leaf in interactive_leaves(n, cfg) {
                let mut own = BTreeSet::new();
                for d in leaf.walk() {
                    for t in &d.texts {
                        let s = t.text.trim().to_ascii_lowercase();
                        if !s.is_empty() {
                            own.insert(s);
                        }
                    }
                }
                for s in own {
                    labels.entry(s).or_default().push(leaf.path.as_str());
                }
            }
            for (label, paths) in labels {
                if paths.len() < 2 {
                    continue;
                }
                let key = (label.clone(), paths.join("|"));
                if !reported.insert(key) {
                    continue;
                }
                out.push(
                    Finding::new(
                        "duplicate-action",
                        &n.path,
                        format!(
                            "the label \"{label}\" appears on {} separate controls under \
                             this surface — ambiguous actions make the user disambiguate \
                             by position every time",
                            paths.len()
                        ),
                    )
                    .at(n.bounds),
                );
            }
        }
        out
    }
}

/// — `modal-depth`: stacked modal layers.
///
/// A modal dialog interrupts the user's flow and demands resolution
/// *now*; a second modal stacked on it strands the first unresolved —
/// attention fragments and the escape path (which dialog does Esc
/// close?) becomes ambiguous. Names matching
/// modal|dialog|overlay|sheet|popover count as modal layers.
struct ModalDepth;
impl LintRule for ModalDepth {
    fn id(&self) -> &'static str {
        "modal-depth"
    }
    fn standards(&self) -> &'static [Standard] {
        &[Standard::InfoDesign, Standard::HciLaws]
    }
    fn default_severity(&self) -> Severity {
        Severity::Warn
    }
    fn confidence(&self) -> Confidence {
        Confidence::Heuristic
    }
    fn title(&self) -> &'static str {
        "modal layered on top of another modal"
    }
    fn citation(&self) -> &'static str {
        "attention is single-threaded — stacked modals strand context; \
         Nielsen #3 user control (ambiguous escape path)"
    }
    fn check(&self, scene: &LintScene, _cfg: &LintConfig) -> Vec<Finding> {
        const MODAL: &[&str] = &["modal", "dialog", "overlay", "sheet", "popover"];
        // Non-modal names that contain a modal substring.
        const NOT_MODAL: &[&str] = &[
            "spritesheet",
            "stylesheet",
            "worksheet",
            "spreadsheet",
            "popovermenu", // a menu *inside* a popover, not a new layer
        ];
        let is_modal = |n: &LintNode| {
            let lower = n.name.to_ascii_lowercase();
            MODAL.iter().any(|m| lower.contains(m)) && !NOT_MODAL.iter().any(|m| lower.contains(m))
        };
        let mut out = Vec::new();
        fn visit(
            node: &LintNode,
            depth: usize,
            is_modal: &dyn Fn(&LintNode) -> bool,
            out: &mut Vec<Finding>,
        ) {
            let depth = depth + usize::from(is_modal(node));
            if depth >= 2 {
                out.push(
                    Finding::new(
                        "modal-depth",
                        &node.path,
                        format!(
                            "'{}' is a modal layer inside another modal — the interrupted \
                             task is now stranded behind two demanding surfaces; \
                             consolidate or demote to inline/panel presentation",
                            node.name
                        ),
                    )
                    .at(node.bounds),
                );
                return; // already inside a flagged stack — don't cascade
            }
            for c in &node.children {
                visit(c, depth, is_modal, out);
            }
        }
        for r in &scene.roots {
            visit(r, 0, &is_modal, &mut out);
        }
        out
    }
}

/// — `scroll-competition`: multiple scroll regions in one surface.
///
/// Two scrolling areas under one surface compete: which scrolls when
/// the wheel turns depends on where the cursor happens to rest — the
/// classic nested-scroll disorientation (Few). Reports once per
/// innermost surface holding ≥2 scroll-named regions, with the count.
struct ScrollCompetition;
impl LintRule for ScrollCompetition {
    fn id(&self) -> &'static str {
        "scroll-competition"
    }
    fn standards(&self) -> &'static [Standard] {
        &[Standard::InfoDesign, Standard::Perception]
    }
    fn default_severity(&self) -> Severity {
        Severity::Info
    }
    fn confidence(&self) -> Confidence {
        Confidence::Heuristic
    }
    fn title(&self) -> &'static str {
        "competing scroll regions in one surface"
    }
    fn citation(&self) -> &'static str {
        "Few, Information Dashboard Design — nested scrolling disorients; \
         scroll capture is ambiguous by construction"
    }
    fn check(&self, scene: &LintScene, cfg: &LintConfig) -> Vec<Finding> {
        // A scroll region = a scroll-named descendant — never the
        // surface itself (a ScrollView IS one region) and never a
        // scrollbar chrome piece.
        let scroll_regions = |n: &LintNode| {
            n.walk()
                .filter(|d| {
                    !std::ptr::eq(*d, n)
                        && d.name.to_ascii_lowercase().contains("scroll")
                        && !d.name.to_ascii_lowercase().contains("scrollbar")
                })
                .count()
        };
        let mut out = Vec::new();
        let qualifying: Vec<&LintNode> = surface_nodes(scene, cfg, self.id())
            .into_iter()
            .filter(|n| scroll_regions(n) >= 2)
            .collect();
        for n in &qualifying {
            // Report the innermost qualifying surface — a containing
            // surface counting the same regions adds no information.
            let inner = qualifying
                .iter()
                .any(|q| q.path != n.path && q.path.starts_with(&format!("{}/", n.path)));
            if inner {
                continue;
            }
            let count = scroll_regions(n);
            out.push(
                Finding::new(
                    "scroll-competition",
                    &n.path,
                    format!(
                        "{count} scroll regions under one surface — wheel/trackpad input \
                         routes to whichever the cursor happens to rest over; nested \
                         scrolling disorients"
                    ),
                )
                .at(n.bounds),
            );
        }
        out
    }
}

/// — `danger-adjacency`: a destructive-looking control beside a safe one.
///
/// Fitts's law says pointing error is a fact, not a possibility — a
/// control painted alarm-red (the destructive-action color) sitting
/// within a slip's distance of a routine control converts one
/// overshoot into data loss. The red node is exempt inside an
/// `@alarm` lineage, where red is the domain color, not a danger
/// signal.
struct DangerAdjacency;
impl LintRule for DangerAdjacency {
    fn id(&self) -> &'static str {
        "danger-adjacency"
    }
    fn standards(&self) -> &'static [Standard] {
        &[Standard::HciLaws, Standard::InfoDesign]
    }
    fn default_severity(&self) -> Severity {
        Severity::Warn
    }
    fn confidence(&self) -> Confidence {
        Confidence::Heuristic
    }
    fn title(&self) -> &'static str {
        "destructive-styled control adjacent to a routine one"
    }
    fn citation(&self) -> &'static str {
        "Fitts's law error cost — slips are inevitable; a destructive action within a \
         slip's distance of a routine one converts error into data loss"
    }
    fn check(&self, scene: &LintScene, cfg: &LintConfig) -> Vec<Finding> {
        // Logical points, scaled — a 4pt slip is 8px at 2×.
        let adj = param(cfg, self.id(), "adjacent_pt", 4.0) * f64::from(scene.scale_factor);
        let mut interactives = Vec::new();
        for r in &scene.roots {
            interactives.extend(interactive_leaves(r, cfg));
        }
        let red = |n: &LintNode| n.fills.iter().any(|f| is_alarm_red(f.color));
        let mut out = Vec::new();
        for a in &interactives {
            if !red(a) || marker_in_lineage(scene, a, "alarm") {
                continue;
            }
            // The closest routine (non-red) control within a slip.
            let neighbor = interactives.iter().filter(|b| !red(b)).find(|b| {
                let x_gap = (a.bounds.x0.max(b.bounds.x0) - a.bounds.x1.min(b.bounds.x1)).max(0.0);
                let y_gap = (a.bounds.y0.max(b.bounds.y0) - a.bounds.y1.min(b.bounds.y1)).max(0.0);
                let x_overlap = a.bounds.x1.min(b.bounds.x1) - a.bounds.x0.max(b.bounds.x0);
                let y_overlap = a.bounds.y1.min(b.bounds.y1) - a.bounds.y0.max(b.bounds.y0);
                // Side-by-side or stacked, within the slip distance.
                (x_gap < adj && y_overlap > 0.0) || (y_gap < adj && x_overlap > 0.0)
            });
            if let Some(b) = neighbor {
                out.push(
                    Finding::new(
                        "danger-adjacency",
                        &a.path,
                        format!(
                            "destructive-styled '{}' sits within {adj:.0}px of routine \
                             control '{}' — one Fitts slip turns a routine tap into data \
                             loss; separate them or demote the red",
                            a.name, b.name
                        ),
                    )
                    .at(a.bounds),
                );
            }
        }
        out
    }
}

/// This module's rules, appended to the registry by `all_rules()`.
pub(crate) fn rules() -> Vec<&'static dyn LintRule> {
    vec![
        &Balance,
        &Symmetry,
        &Regularity,
        &RedundantBorder,
        &EmptySurface,
        &AspectExtreme,
        &LineLength,
        &GridDrift,
        &HeadingRhythm,
        &BaselineDrift,
        &DuplicateAction,
        &ModalDepth,
        &ScrollCompetition,
        &DangerAdjacency,
    ]
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::lint;
    use kurbo::{Point, Rect};
    use martensite_core::{PaintCommand, PaintList};

    /// `App` root with the standard dark backdrop fill.
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

    /// A surface-sized `Panel` with `n` empty `Label` children — the
    /// minimal scene qualifying for `surface_nodes` (≥2 children,
    /// ≥20,000px²).
    fn panel_scope(list: &mut PaintList, bounds: Rect, children: usize) {
        list.push_scope(None, "Panel", bounds);
        for i in 0..children {
            let x = bounds.x0 + i as f64 * 20.0;
            list.push_scope(
                None,
                "Label",
                Rect::new(x, bounds.y0, x + 10.0, bounds.y0 + 10.0),
            );
            list.pop_scope();
        }
    }

    // ---------- balance ----------

    #[test]
    fn balance_flags_lopsided_surface() {
        let mut list = app_list();
        panel_scope(&mut list, Rect::new(10.0, 10.0, 210.0, 210.0), 2);
        // All mass in the left third — centroid well past 25% off-center.
        list.commands.push(PaintCommand::FillRect(
            Rect::new(10.0, 10.0, 80.0, 210.0),
            [60, 60, 70, 255],
        ));
        list.pop_scope();
        list.pop_scope();
        let scene = LintScene::from_paint_list(&list);
        let report = lint(&scene, &LintConfig::new());
        let hits = findings_for(&report, "balance");
        assert_eq!(hits.len(), 1, "expected one finding, got {hits:?}");
        assert!(hits[0].path.ends_with("Panel"));
    }

    #[test]
    fn balance_quiet_on_centered_mass() {
        let mut list = app_list();
        panel_scope(&mut list, Rect::new(10.0, 10.0, 210.0, 210.0), 2);
        // Symmetric fills — centroid sits at the bounds center.
        list.commands.push(PaintCommand::FillRect(
            Rect::new(10.0, 10.0, 80.0, 210.0),
            [60, 60, 70, 255],
        ));
        list.commands.push(PaintCommand::FillRect(
            Rect::new(140.0, 10.0, 210.0, 210.0),
            [60, 60, 70, 255],
        ));
        list.pop_scope();
        list.pop_scope();
        let scene = LintScene::from_paint_list(&list);
        let report = lint(&scene, &LintConfig::new());
        assert!(findings_for(&report, "balance").is_empty());
    }

    // ---------- symmetry ----------

    #[test]
    fn symmetry_flags_one_sided_surface() {
        let mut list = app_list();
        panel_scope(&mut list, Rect::new(10.0, 10.0, 210.0, 210.0), 2);
        // Every fill on the left of the midline — 100% asymmetry.
        list.commands.push(PaintCommand::FillRect(
            Rect::new(10.0, 10.0, 110.0, 210.0),
            [60, 60, 70, 255],
        ));
        list.pop_scope();
        list.pop_scope();
        let scene = LintScene::from_paint_list(&list);
        let report = lint(&scene, &LintConfig::new());
        let hits = findings_for(&report, "symmetry");
        assert_eq!(hits.len(), 1, "expected one finding, got {hits:?}");
    }

    #[test]
    fn symmetry_quiet_on_mirror_or_sidebar() {
        // Mirrored fills — perfectly symmetric.
        let mut list = app_list();
        panel_scope(&mut list, Rect::new(10.0, 10.0, 210.0, 210.0), 2);
        list.commands.push(PaintCommand::FillRect(
            Rect::new(10.0, 10.0, 110.0, 210.0),
            [60, 60, 70, 255],
        ));
        list.commands.push(PaintCommand::FillRect(
            Rect::new(110.0, 10.0, 210.0, 210.0),
            [60, 60, 70, 255],
        ));
        list.pop_scope();
        list.pop_scope();
        let scene = LintScene::from_paint_list(&list);
        let report = lint(&scene, &LintConfig::new());
        assert!(findings_for(&report, "symmetry").is_empty());

        // Same lopsided mass inside a *Sidebar* — legitimately asymmetric.
        let mut list = app_list();
        list.push_scope(None, "Sidebar", Rect::new(10.0, 10.0, 210.0, 210.0));
        list.push_scope(None, "Label", Rect::new(10.0, 10.0, 20.0, 20.0));
        list.pop_scope();
        list.push_scope(None, "Label", Rect::new(30.0, 10.0, 40.0, 20.0));
        list.pop_scope();
        list.commands.push(PaintCommand::FillRect(
            Rect::new(10.0, 10.0, 110.0, 210.0),
            [60, 60, 70, 255],
        ));
        list.pop_scope();
        list.pop_scope();
        let scene = LintScene::from_paint_list(&list);
        let report = lint(&scene, &LintConfig::new());
        assert!(findings_for(&report, "symmetry").is_empty());
    }

    // ---------- regularity ----------

    #[test]
    fn regularity_flags_irregular_group() {
        let mut list = app_list();
        list.push_scope(None, "List", Rect::new(10.0, 10.0, 410.0, 110.0));
        for (i, w) in [40.0, 55.0, 90.0, 130.0].iter().enumerate() {
            let x = 10.0 + i as f64 * 100.0;
            list.push_scope(None, "Item", Rect::new(x, 10.0, x + w, 60.0));
            list.pop_scope();
        }
        list.pop_scope();
        list.pop_scope();
        let scene = LintScene::from_paint_list(&list);
        let report = lint(&scene, &LintConfig::new());
        let hits = findings_for(&report, "regularity");
        assert_eq!(hits.len(), 1, "expected one finding, got {hits:?}");
        assert!(hits[0].path.ends_with("List"));
    }

    #[test]
    fn regularity_quiet_on_uniform_group() {
        let mut list = app_list();
        list.push_scope(None, "List", Rect::new(10.0, 10.0, 410.0, 110.0));
        for i in 0..4 {
            let x = 10.0 + i as f64 * 100.0;
            list.push_scope(None, "Item", Rect::new(x, 10.0, x + 80.0, 60.0));
            list.pop_scope();
        }
        list.pop_scope();
        list.pop_scope();
        let scene = LintScene::from_paint_list(&list);
        let report = lint(&scene, &LintConfig::new());
        assert!(findings_for(&report, "regularity").is_empty());
    }

    // ---------- redundant-border ----------

    #[test]
    fn redundant_border_flags_divider_in_wide_gap() {
        let mut list = app_list();
        list.push_scope(None, "Panel", Rect::new(0.0, 0.0, 400.0, 300.0));
        list.push_scope(None, "Row", Rect::new(0.0, 0.0, 400.0, 140.0));
        list.pop_scope();
        list.push_scope(None, "Row", Rect::new(0.0, 160.0, 400.0, 300.0));
        list.pop_scope();
        // 1px rule inside the 20px gap — 380px > 40% of the 400px parent.
        list.commands.push(PaintCommand::FillRect(
            Rect::new(10.0, 148.0, 390.0, 149.0),
            [80, 80, 90, 255],
        ));
        list.pop_scope();
        list.pop_scope();
        let mut scene = LintScene::from_paint_list(&list);
        scene.scale_factor = 1.0;
        let report = lint(&scene, &LintConfig::new());
        let hits = findings_for(&report, "redundant-border");
        assert_eq!(hits.len(), 1, "expected one finding, got {hits:?}");
        assert!(hits[0].path.ends_with("Panel"));
    }

    #[test]
    fn redundant_border_quiet_when_gap_is_tight() {
        let mut list = app_list();
        list.push_scope(None, "Panel", Rect::new(0.0, 0.0, 400.0, 300.0));
        list.push_scope(None, "Row", Rect::new(0.0, 0.0, 400.0, 146.0));
        list.pop_scope();
        list.push_scope(None, "Row", Rect::new(0.0, 150.0, 400.0, 300.0));
        list.pop_scope();
        // Same rule, but the 4px gap doesn't already separate — the
        // border is doing the work.
        list.commands.push(PaintCommand::FillRect(
            Rect::new(10.0, 147.0, 390.0, 148.0),
            [80, 80, 90, 255],
        ));
        list.pop_scope();
        list.pop_scope();
        let mut scene = LintScene::from_paint_list(&list);
        scene.scale_factor = 1.0;
        let report = lint(&scene, &LintConfig::new());
        assert!(findings_for(&report, "redundant-border").is_empty());
    }

    // ---------- empty-surface ----------

    #[test]
    fn empty_surface_flags_dead_panel() {
        let mut list = app_list();
        // 200×100 = 20,000px² surface whose subtree paints nothing.
        panel_scope(&mut list, Rect::new(10.0, 10.0, 210.0, 110.0), 2);
        list.pop_scope();
        list.pop_scope();
        let scene = LintScene::from_paint_list(&list);
        let report = lint(&scene, &LintConfig::new());
        let hits = findings_for(&report, "empty-surface");
        assert_eq!(hits.len(), 1, "expected one finding, got {hits:?}");
        assert!(hits[0].path.ends_with("Panel"));
    }

    #[test]
    fn empty_surface_quiet_with_content_or_chrome() {
        // Panel carrying a real fill — not empty.
        let mut list = app_list();
        panel_scope(&mut list, Rect::new(10.0, 10.0, 210.0, 110.0), 2);
        list.commands.push(PaintCommand::FillRect(
            Rect::new(10.0, 10.0, 210.0, 110.0),
            [40, 40, 50, 255],
        ));
        list.pop_scope();
        list.pop_scope();
        let scene = LintScene::from_paint_list(&list);
        let report = lint(&scene, &LintConfig::new());
        assert!(findings_for(&report, "empty-surface").is_empty());

        // Chrome kinds are legitimately sparse — skipped.
        let mut list = app_list();
        list.push_scope(None, "StatusBar", Rect::new(10.0, 10.0, 210.0, 110.0));
        list.push_scope(None, "Label", Rect::new(10.0, 10.0, 20.0, 20.0));
        list.pop_scope();
        list.push_scope(None, "Label", Rect::new(30.0, 10.0, 40.0, 20.0));
        list.pop_scope();
        list.pop_scope();
        list.pop_scope();
        let scene = LintScene::from_paint_list(&list);
        let report = lint(&scene, &LintConfig::new());
        assert!(findings_for(&report, "empty-surface").is_empty());
    }

    // ---------- aspect-extreme ----------

    #[test]
    fn aspect_extreme_flags_ribbon() {
        let mut list = app_list();
        // 500×25 = 12,500px² at 20:1 — past the 12:1 limit.
        list.push_scope(None, "Banner", Rect::new(10.0, 10.0, 510.0, 35.0));
        list.pop_scope();
        list.pop_scope();
        let mut scene = LintScene::from_paint_list(&list);
        scene.scale_factor = 1.0;
        let report = lint(&scene, &LintConfig::new());
        let hits = findings_for(&report, "aspect-extreme");
        assert_eq!(hits.len(), 1, "expected one finding, got {hits:?}");
        assert!(hits[0].path.ends_with("Banner"));
    }

    #[test]
    fn aspect_extreme_quiet_on_normal_geometry() {
        let mut list = app_list();
        list.push_scope(None, "Card", Rect::new(10.0, 10.0, 210.0, 160.0));
        list.pop_scope();
        list.pop_scope();
        let mut scene = LintScene::from_paint_list(&list);
        scene.scale_factor = 1.0;
        let report = lint(&scene, &LintConfig::new());
        assert!(findings_for(&report, "aspect-extreme").is_empty());
    }

    // ---------- line-length ----------

    #[test]
    fn line_length_flags_overlong_run() {
        let mut list = app_list();
        list.push_scope(None, "Label", Rect::new(10.0, 10.0, 700.0, 30.0));
        list.commands.push(PaintCommand::DrawText(
            Point::new(10.0, 25.0),
            "x".repeat(90),
            12.0,
            [220, 220, 220, 255],
        ));
        list.pop_scope();
        list.pop_scope();
        let scene = LintScene::from_paint_list(&list);
        let report = lint(&scene, &LintConfig::new());
        let hits = findings_for(&report, "line-length");
        assert_eq!(hits.len(), 1, "expected one finding, got {hits:?}");
    }

    #[test]
    fn line_length_quiet_inside_band() {
        let mut list = app_list();
        list.push_scope(None, "Label", Rect::new(10.0, 10.0, 400.0, 30.0));
        list.commands.push(PaintCommand::DrawText(
            Point::new(10.0, 25.0),
            "x".repeat(60),
            12.0,
            [220, 220, 220, 255],
        ));
        list.pop_scope();
        list.pop_scope();
        let scene = LintScene::from_paint_list(&list);
        let report = lint(&scene, &LintConfig::new());
        assert!(findings_for(&report, "line-length").is_empty());
    }

    // ---------- grid-drift ----------

    /// `Grid` > two `Row`s of `Cell`s at the given x0 offsets.
    fn grid_scene(col_x: [[f64; 2]; 2]) -> LintScene {
        let mut list = app_list();
        list.push_scope(None, "Grid", Rect::new(10.0, 10.0, 410.0, 210.0));
        for (r, xs) in col_x.iter().enumerate() {
            let y = 10.0 + r as f64 * 100.0;
            list.push_scope(None, "Row", Rect::new(10.0, y, 410.0, y + 90.0));
            for (c, x) in xs.iter().enumerate() {
                list.push_scope(None, "Cell", Rect::new(*x, y + 5.0, x + 80.0, y + 80.0));
                let _ = c;
                list.pop_scope();
            }
            list.pop_scope();
        }
        list.pop_scope();
        list.pop_scope();
        LintScene::from_paint_list(&list)
    }

    #[test]
    fn grid_drift_flags_misaligned_column() {
        // Column 1 left edges at 20 vs 30 — 10px drift > 2px tolerance.
        let scene = grid_scene([[20.0, 210.0], [30.0, 210.0]]);
        let report = lint(&scene, &LintConfig::new());
        let hits = findings_for(&report, "grid-drift");
        assert_eq!(hits.len(), 1, "expected one finding, got {hits:?}");
        assert!(hits[0].path.ends_with("Grid"));
    }

    #[test]
    fn grid_drift_quiet_on_aligned_columns() {
        let scene = grid_scene([[20.0, 210.0], [20.0, 210.0]]);
        let report = lint(&scene, &LintConfig::new());
        assert!(findings_for(&report, "grid-drift").is_empty());
    }

    // ---------- heading-rhythm ----------

    #[test]
    fn heading_rhythm_flags_shouting_child() {
        let mut list = app_list();
        list.push_scope(None, "Section", Rect::new(10.0, 10.0, 210.0, 210.0));
        // The surface's own text sets a 12px ceiling.
        list.commands.push(PaintCommand::DrawText(
            Point::new(15.0, 25.0),
            "Section title".to_string(),
            12.0,
            [220, 220, 220, 255],
        ));
        list.push_scope(None, "Body", Rect::new(10.0, 40.0, 210.0, 210.0));
        list.commands.push(PaintCommand::DrawText(
            Point::new(15.0, 60.0),
            "shouting body".to_string(),
            20.0,
            [220, 220, 220, 255],
        ));
        list.pop_scope();
        list.push_scope(None, "Footnote", Rect::new(10.0, 210.0, 210.0, 230.0));
        list.pop_scope();
        list.pop_scope();
        list.pop_scope();
        let scene = LintScene::from_paint_list(&list);
        let report = lint(&scene, &LintConfig::new());
        let hits = findings_for(&report, "heading-rhythm");
        assert_eq!(hits.len(), 1, "expected one finding, got {hits:?}");
        assert!(hits[0].path.ends_with("Body"));
    }

    #[test]
    fn heading_rhythm_quiet_on_descending_scale() {
        let mut list = app_list();
        list.push_scope(None, "Section", Rect::new(10.0, 10.0, 210.0, 210.0));
        list.commands.push(PaintCommand::DrawText(
            Point::new(15.0, 30.0),
            "Section title".to_string(),
            20.0,
            [220, 220, 220, 255],
        ));
        list.push_scope(None, "Body", Rect::new(10.0, 40.0, 210.0, 210.0));
        list.commands.push(PaintCommand::DrawText(
            Point::new(15.0, 60.0),
            "quiet body".to_string(),
            12.0,
            [220, 220, 220, 255],
        ));
        list.pop_scope();
        list.push_scope(None, "Footnote", Rect::new(10.0, 210.0, 210.0, 230.0));
        list.pop_scope();
        list.pop_scope();
        list.pop_scope();
        let scene = LintScene::from_paint_list(&list);
        let report = lint(&scene, &LintConfig::new());
        assert!(findings_for(&report, "heading-rhythm").is_empty());
    }

    // ---------- baseline-drift ----------

    /// `Row` > two `Label` siblings side-by-side with a text run each.
    fn baseline_scene(y1: f64, y2: f64) -> LintScene {
        let mut list = app_list();
        list.push_scope(None, "Row", Rect::new(10.0, 10.0, 410.0, 60.0));
        list.push_scope(None, "Label", Rect::new(10.0, 10.0, 210.0, 60.0));
        list.commands.push(PaintCommand::DrawText(
            Point::new(15.0, y1),
            "left".to_string(),
            12.0,
            [220, 220, 220, 255],
        ));
        list.pop_scope();
        list.push_scope(None, "Label", Rect::new(210.0, 10.0, 410.0, 60.0));
        list.commands.push(PaintCommand::DrawText(
            Point::new(215.0, y2),
            "right".to_string(),
            12.0,
            [220, 220, 220, 255],
        ));
        list.pop_scope();
        list.pop_scope();
        list.pop_scope();
        LintScene::from_paint_list(&list)
    }

    #[test]
    fn baseline_drift_flags_ragged_row() {
        // Same size, baselines 2.5px apart — past the 1px tolerance.
        let scene = baseline_scene(30.0, 32.5);
        let report = lint(&scene, &LintConfig::new());
        let hits = findings_for(&report, "baseline-drift");
        assert_eq!(hits.len(), 1, "expected one finding, got {hits:?}");
        assert!(hits[0].path.ends_with("Row"));
    }

    #[test]
    fn baseline_drift_quiet_on_shared_baseline() {
        let scene = baseline_scene(30.0, 30.5);
        let report = lint(&scene, &LintConfig::new());
        assert!(findings_for(&report, "baseline-drift").is_empty());
    }

    // ---------- duplicate-action ----------

    /// `Panel` surface with two `Button`s carrying the given labels.
    fn buttons_scene(labels: [&str; 2]) -> LintScene {
        let mut list = app_list();
        list.push_scope(None, "Panel", Rect::new(10.0, 10.0, 410.0, 210.0));
        for (i, label) in labels.iter().enumerate() {
            let x = 10.0 + i as f64 * 200.0;
            list.push_scope(None, "Button", Rect::new(x, 10.0, x + 150.0, 60.0));
            list.commands.push(PaintCommand::DrawText(
                Point::new(x + 10.0, 40.0),
                label.to_string(),
                12.0,
                [220, 220, 220, 255],
            ));
            list.pop_scope();
        }
        list.pop_scope();
        list.pop_scope();
        LintScene::from_paint_list(&list)
    }

    #[test]
    fn duplicate_action_flags_same_label() {
        let scene = buttons_scene(["Save", "Save"]);
        let report = lint(&scene, &LintConfig::new());
        let hits = findings_for(&report, "duplicate-action");
        assert_eq!(hits.len(), 1, "expected one finding, got {hits:?}");
        assert!(hits[0].message.contains("save"));
    }

    #[test]
    fn duplicate_action_quiet_on_distinct_labels() {
        let scene = buttons_scene(["Save", "Cancel"]);
        let report = lint(&scene, &LintConfig::new());
        assert!(findings_for(&report, "duplicate-action").is_empty());
    }

    // ---------- modal-depth ----------

    #[test]
    fn modal_depth_flags_stacked_modal() {
        let mut list = app_list();
        list.push_scope(None, "Dialog", Rect::new(100.0, 100.0, 500.0, 400.0));
        list.push_scope(None, "ConfirmSheet", Rect::new(150.0, 150.0, 450.0, 350.0));
        list.pop_scope();
        list.pop_scope();
        list.pop_scope();
        let scene = LintScene::from_paint_list(&list);
        let report = lint(&scene, &LintConfig::new());
        let hits = findings_for(&report, "modal-depth");
        assert_eq!(hits.len(), 1, "expected one finding, got {hits:?}");
        assert!(hits[0].path.ends_with("ConfirmSheet"));
    }

    #[test]
    fn modal_depth_quiet_on_single_modal() {
        let mut list = app_list();
        list.push_scope(None, "Dialog", Rect::new(100.0, 100.0, 500.0, 400.0));
        list.push_scope(None, "Panel", Rect::new(150.0, 150.0, 450.0, 350.0));
        list.pop_scope();
        list.pop_scope();
        list.pop_scope();
        let scene = LintScene::from_paint_list(&list);
        let report = lint(&scene, &LintConfig::new());
        assert!(findings_for(&report, "modal-depth").is_empty());
    }

    // ---------- scroll-competition ----------

    #[test]
    fn scroll_competition_flags_two_regions() {
        let mut list = app_list();
        list.push_scope(None, "Panel", Rect::new(10.0, 10.0, 410.0, 410.0));
        list.push_scope(None, "ScrollView", Rect::new(10.0, 10.0, 210.0, 410.0));
        list.pop_scope();
        list.push_scope(None, "ScrollView", Rect::new(210.0, 10.0, 410.0, 410.0));
        list.pop_scope();
        list.pop_scope();
        list.pop_scope();
        let scene = LintScene::from_paint_list(&list);
        let report = lint(&scene, &LintConfig::new());
        let hits = findings_for(&report, "scroll-competition");
        assert_eq!(hits.len(), 1, "expected one finding, got {hits:?}");
        assert!(hits[0].message.contains("2"));
    }

    #[test]
    fn scroll_competition_quiet_with_one_region() {
        let mut list = app_list();
        list.push_scope(None, "Panel", Rect::new(10.0, 10.0, 410.0, 410.0));
        list.push_scope(None, "ScrollView", Rect::new(10.0, 10.0, 210.0, 410.0));
        list.pop_scope();
        list.push_scope(None, "DetailView", Rect::new(210.0, 10.0, 410.0, 410.0));
        list.pop_scope();
        list.pop_scope();
        list.pop_scope();
        let scene = LintScene::from_paint_list(&list);
        let report = lint(&scene, &LintConfig::new());
        assert!(findings_for(&report, "scroll-competition").is_empty());
    }

    // ---------- danger-adjacency ----------

    /// `Panel` > red `DeleteButton` + `SaveButton` with `gap` px between.
    fn danger_scene(gap: f64, alarm_marker: bool) -> LintScene {
        let mut list = app_list();
        list.push_scope(None, "Panel", Rect::new(10.0, 10.0, 410.0, 210.0));
        let name = if alarm_marker {
            "DeleteButton@alarm"
        } else {
            "DeleteButton"
        };
        list.push_scope(None, name, Rect::new(10.0, 10.0, 150.0, 60.0));
        list.commands.push(PaintCommand::FillRect(
            Rect::new(10.0, 10.0, 150.0, 60.0),
            [220, 30, 30, 255],
        ));
        list.pop_scope();
        let x = 150.0 + gap;
        list.push_scope(None, "SaveButton", Rect::new(x, 10.0, x + 140.0, 60.0));
        list.pop_scope();
        list.pop_scope();
        list.pop_scope();
        LintScene::from_paint_list(&list)
    }

    #[test]
    fn danger_adjacency_flags_slip_distance() {
        // 2px from a routine control — inside a Fitts slip.
        let scene = danger_scene(2.0, false);
        let report = lint(&scene, &LintConfig::new());
        let hits = findings_for(&report, "danger-adjacency");
        assert_eq!(hits.len(), 1, "expected one finding, got {hits:?}");
        assert!(hits[0].path.contains("DeleteButton"));
    }

    #[test]
    fn danger_adjacency_quiet_when_spaced_or_alarm_marked() {
        // 20px of separation — out of slip distance.
        let scene = danger_scene(20.0, false);
        let report = lint(&scene, &LintConfig::new());
        assert!(findings_for(&report, "danger-adjacency").is_empty());

        // Alarm lineage — red is the domain color, not a danger signal.
        let scene = danger_scene(2.0, true);
        let report = lint(&scene, &LintConfig::new());
        assert!(findings_for(&report, "danger-adjacency").is_empty());
    }
}
