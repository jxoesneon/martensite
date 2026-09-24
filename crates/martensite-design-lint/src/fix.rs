//! Best-effort autofix — `rustc --fix` semantics over the lint scene.
//!
//! Findings may carry a [`LintFix`] describing scene mutations that
//! resolve them. [`autofix`] applies `Safe` fixes immediately and
//! `Risky` ones only under `force` (the `--force` flag), then — like
//! `cargo fix` — **re-lints and keeps fixing**: fixes can expose new
//! findings (widening a button reveals a cramped sibling row), so the
//! loop recurses until the scene converges or `max_depth` passes run
//! (`--max-recursiveness`; `--no-recursive` = a single pass).
//!
//! Fixes mutate the [`LintScene`] — the normalized model rules
//! measure. That makes autofix a *convergence proof and preview*: it
//! demonstrates that the suggested remediation resolves the finding
//! without introducing new ones, and reports exactly which edits are
//! needed. Applying the equivalent change to widget source remains a
//! developer action, guided by each finding's `doc` reference — the
//! scene is derived from the paint list, not the other way around.
//!
//! # Examples
//!
//! ```
//! use martensite_design_lint::{autofix, FixOptions, LintConfig, LintScene};
//!
//! let mut scene = LintScene::default();
//! let report = autofix(&mut scene, &LintConfig::new(), &FixOptions::default());
//! assert!(report.converged);
//! ```

use std::fmt::Write as _;

use kurbo::Vec2;

use crate::config::LintConfig;
use crate::report::LintReport;
use crate::scene::{LintNode, LintScene};

/// How dangerous a fix is — the `--force` gate.
///
/// `Safe` fixes are spacing/alignment nudges that cannot regress
/// meaning: evening out gaps, snapping to a grid, aligning edges.
/// `Risky` fixes change what the user *sees* (colors, font sizes) or
/// grow geometry into space that may be occupied — they wait for
/// `--force`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FixSafety {
    /// Innocuous — apply without `--force`.
    Safe,
    /// Can cascade or alter visual identity — require `--force`.
    Risky,
}

/// Which sibling edge [`FixOp::AlignSiblings`] aligns.
///
/// Each variant translates children onto the extreme of that edge —
/// `Left` pulls everything to the leftmost `x0`, `Right` to the
/// rightmost `x1`, and so on.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AlignEdge {
    /// Align all children to the leftmost `x0` (column alignment).
    Left,
    /// Align all children to the topmost `y0` (row alignment).
    Top,
    /// Align all children to the rightmost `x1`.
    Right,
    /// Align all children to the bottommost `y1`.
    Bottom,
}

/// One scene mutation. Fixes are *data*, not closures — serializable,
/// testable, and inspectable before application.
#[derive(Debug, Clone, PartialEq)]
pub enum FixOp {
    /// Enforce a minimum `gap` px between `parent`'s consecutive
    /// children along the dominant layout axis — wider-spaced
    /// siblings keep their positions; the fix only ever spreads.
    SetGap {
        /// Path of the parent whose children are re-spaced.
        parent: String,
        /// Minimum inter-sibling gap in device px.
        gap: f64,
    },
    /// Round `parent`'s sibling gaps to the nearest multiple of
    /// `grid` px (minimum `grid` — never collapses to zero).
    SnapGapsToGrid {
        /// Path of the parent whose children are re-spaced.
        parent: String,
        /// The grid step in device px (typically 4 × scale).
        grid: f64,
    },
    /// Translate `parent`'s children so the given `edge` lines up.
    AlignSiblings {
        /// Path of the parent whose children are aligned.
        parent: String,
        /// Which edge aligns.
        edge: AlignEdge,
    },
    /// Recolor fills at `path` whose color equals `from`.
    RecolorFill {
        /// Path of the node owning the fills.
        path: String,
        /// The offending color to replace.
        from: [u8; 4],
        /// The compliant replacement.
        to: [u8; 4],
    },
    /// Recolor text runs at `path` whose color equals `from`.
    RecolorText {
        /// Path of the node owning the text runs.
        path: String,
        /// The offending color to replace.
        from: [u8; 4],
        /// The compliant replacement.
        to: [u8; 4],
    },
    /// Retarget text runs at `path` sized `from` (±0.5px) to `to`.
    SetFontSize {
        /// Path of the node owning the text runs.
        path: String,
        /// The undersized font size in device px.
        from: f32,
        /// The compliant size in device px.
        to: f32,
    },
    /// Grow `path`'s bounds to at least `min_w`×`min_h` (expands
    /// right/down; overlaps are the caller's `--force` problem).
    GrowBounds {
        /// Path of the node to grow.
        path: String,
        /// Minimum width in device px.
        min_w: f64,
        /// Minimum height in device px.
        min_h: f64,
    },
}

/// A remediation attached to a finding — a human-readable `summary`
/// plus the [`FixOp`]s that implement it.
#[derive(Debug, Clone)]
pub struct LintFix {
    /// What the fix does, e.g. `"space the 9 siblings 8px apart"`.
    pub summary: String,
    /// `Safe` applies immediately; `Risky` requires `--force`.
    pub safety: FixSafety,
    /// Scene mutations, applied in order.
    pub ops: Vec<FixOp>,
}

impl LintFix {
    /// A one-op fix.
    pub fn new(summary: impl Into<String>, safety: FixSafety, op: FixOp) -> Self {
        LintFix {
            summary: summary.into(),
            safety,
            ops: vec![op],
        }
    }
}

/// Autofix controls — mirrors the CLI surface:
/// `--force` → [`force`](Self::force),
/// `--no-recursive` → `recursive: false`,
/// `--max-recursiveness N` → [`max_depth`](Self::max_depth).
#[derive(Debug, Clone)]
pub struct FixOptions {
    /// Apply `Risky` fixes too. Default `false`.
    pub force: bool,
    /// Re-lint after each pass and keep fixing newly-exposed
    /// findings. Default `true` (`--no-recursive` clears it).
    pub recursive: bool,
    /// Maximum fix passes — the recursion ceiling. Default `8`.
    pub max_depth: usize,
}

impl Default for FixOptions {
    fn default() -> Self {
        FixOptions {
            force: false,
            recursive: true,
            max_depth: 8,
        }
    }
}

/// One fix the engine applied.
#[derive(Debug, Clone)]
pub struct AppliedFix {
    /// The rule whose finding carried the fix.
    pub rule: &'static str,
    /// The scope the fix anchored to.
    pub path: String,
    /// The fix's human summary.
    pub summary: String,
    /// Safe or forced.
    pub safety: FixSafety,
}

/// One lint→fix pass of the [`autofix`] loop.
#[derive(Debug)]
pub struct FixIteration {
    /// Zero-based pass number.
    pub depth: usize,
    /// Fixes applied this pass.
    pub applied: Vec<AppliedFix>,
    /// `Risky` fixes skipped for lack of `force`.
    pub skipped_risky: usize,
}

/// [`autofix`] output — per-pass history plus the final lint state.
#[derive(Debug)]
pub struct FixReport {
    /// Pass history, one entry per lint→fix round.
    pub iterations: Vec<FixIteration>,
    /// The lint report on the *final* scene.
    pub report: LintReport,
    /// `true` when a pass applied nothing — the scene is at a fix
    /// fixpoint. `false` means `max_depth` ran out with fixes still
    /// applicable (or `recursive` was off).
    pub converged: bool,
    /// `true` when the loop stopped because a scene state repeated —
    /// fixes were oscillating (A→B→A); further passes would cycle
    /// forever.
    pub cycle_detected: bool,
}

impl FixReport {
    /// Total fixes applied across all passes.
    pub fn applied_count(&self) -> usize {
        self.iterations.iter().map(|i| i.applied.len()).sum()
    }

    /// `Risky` fixes still gated at the end of the run.
    pub fn gated_count(&self) -> usize {
        self.iterations.last().map_or(0, |i| i.skipped_risky)
    }

    /// Human-readable summary for CLI output.
    pub fn to_text(&self) -> String {
        let mut out = String::new();
        for it in &self.iterations {
            let _ = writeln!(
                out,
                "pass {}: {} fix(es) applied{}",
                it.depth + 1,
                it.applied.len(),
                if it.skipped_risky > 0 {
                    format!(", {} risky skipped (use --force)", it.skipped_risky)
                } else {
                    String::new()
                }
            );
            for a in &it.applied {
                let tag = if a.safety == FixSafety::Risky {
                    "risky"
                } else {
                    "safe"
                };
                let _ = writeln!(out, "  [{tag}] {} @ {} — {}", a.rule, a.path, a.summary);
            }
        }
        let gated = self.gated_count();
        let status = if self.cycle_detected {
            "stopped (fixes oscillating — repeated scene state)"
        } else if self.converged && gated > 0 {
            // Not misleading: converged on *eligible* fixes only.
            "converged — but risky fixes remain gated"
        } else if self.converged {
            "converged"
        } else {
            "stopped (recursion limit or --no-recursive)"
        };
        let _ = writeln!(
            out,
            "{status} — {} fix(es), {} active finding(s) remain{}",
            self.applied_count(),
            self.report.findings.len(),
            if gated > 0 {
                format!("; {gated} risky fix(es) need --force")
            } else {
                String::new()
            }
        );
        out
    }
}

/// The lint→fix loop: lint, apply eligible fixes, re-lint, repeat
/// until convergence, `max_depth`, or a single pass when
/// `recursive` is off.
///
/// Suppressed findings are never fixed — an allowed violation is a
/// declared exception, not a defect to repair. `Forbid` findings
/// *are* fixable (they can't be suppressed, only fixed).
///
/// # Examples
///
/// ```
/// use kurbo::Rect;
/// use martensite_core::{PaintCommand, PaintList};
/// use martensite_design_lint::{autofix, FixOptions, LintConfig, LintScene};
///
/// // Four siblings crammed 2px apart — whitespace flags it, and the
/// // Safe SetGap fix evens them out. Re-lint converges.
/// let mut list = PaintList::new();
/// list.push_scope(None, "App", Rect::new(0.0, 0.0, 800.0, 600.0));
/// list.push_scope(None, "Panel", Rect::new(0.0, 0.0, 800.0, 100.0));
/// for i in 0..4 {
///     let x = i as f64 * 52.0;
///     list.push_scope(None, "Label", Rect::new(x, 10.0, x + 50.0, 40.0));
///     list.pop_scope();
/// }
/// list.pop_scope();
/// list.pop_scope();
/// let mut scene = LintScene::from_paint_list(&list);
/// let report = autofix(&mut scene, &LintConfig::new(), &FixOptions::default());
/// assert!(report.applied_count() > 0);
/// ```
pub fn autofix(scene: &mut LintScene, config: &LintConfig, opts: &FixOptions) -> FixReport {
    let max_passes = if opts.recursive {
        opts.max_depth.max(1)
    } else {
        1
    };
    let mut iterations = Vec::new();
    // Scene-state signatures — a fix pair that undoes each other
    // (A→B→A) would loop to `max_depth` doing nothing useful; a
    // repeated state means stop.
    let mut seen_states = std::collections::HashSet::new();
    seen_states.insert(scene_signature(scene));
    loop {
        let report = crate::lint(scene, config);
        let mut applied = Vec::new();
        let mut skipped_risky = 0usize;
        for f in &report.findings {
            let Some(fix) = &f.fix else { continue };
            if fix.safety == FixSafety::Risky && !opts.force {
                skipped_risky += 1;
                continue;
            }
            let mut ops_applied = 0usize;
            for op in &fix.ops {
                if apply_op(scene, op) {
                    ops_applied += 1;
                }
            }
            // One entry per *finding* — a multi-op fix is one logical
            // remediation, not N.
            if ops_applied > 0 {
                applied.push(AppliedFix {
                    rule: f.rule,
                    path: f.path.clone(),
                    summary: fix.summary.clone(),
                    safety: fix.safety,
                });
            }
        }
        let converged = applied.is_empty();
        let depth = iterations.len();
        iterations.push(FixIteration {
            depth,
            applied,
            skipped_risky,
        });
        if converged || iterations.len() >= max_passes {
            return FixReport {
                iterations,
                // Re-lint only when the last pass changed the scene —
                // otherwise `report` already describes it.
                report: if converged {
                    report
                } else {
                    crate::lint(scene, config)
                },
                converged,
                cycle_detected: false,
            };
        }
        if !seen_states.insert(scene_signature(scene)) {
            return FixReport {
                iterations,
                report: crate::lint(scene, config),
                converged: false,
                cycle_detected: true,
            };
        }
    }
}

/// A cheap fingerprint of everything fixes can change — bounds,
/// colors, font sizes, fill/text geometry — used to detect
/// oscillating fixes (A→B→A) before `max_depth` burns out.
fn scene_signature(scene: &LintScene) -> u64 {
    use std::hash::{Hash, Hasher};
    let mut h = std::collections::hash_map::DefaultHasher::new();
    fn hash_node(n: &LintNode, h: &mut std::collections::hash_map::DefaultHasher) {
        for v in [n.bounds.x0, n.bounds.y0, n.bounds.x1, n.bounds.y1] {
            v.to_bits().hash(h);
        }
        for c in &n.colors {
            c.hash(h);
        }
        for s in &n.font_sizes {
            s.to_bits().hash(h);
        }
        for f in &n.fills {
            f.color.hash(h);
            for v in [f.rect.x0, f.rect.y0, f.rect.x1, f.rect.y1] {
                v.to_bits().hash(h);
            }
        }
        for t in &n.texts {
            t.color.hash(h);
            t.size.to_bits().hash(h);
            t.origin.x.to_bits().hash(h);
            t.origin.y.to_bits().hash(h);
        }
        for c in &n.children {
            hash_node(c, h);
        }
    }
    for r in &scene.roots {
        hash_node(r, &mut h);
    }
    h.finish()
}

/// Apply one [`FixOp`] to the scene — `true` when it changed
/// something (idempotent ops report `false`, which is how the loop
/// knows it has converged).
pub(crate) fn apply_op(scene: &mut LintScene, op: &FixOp) -> bool {
    match op {
        FixOp::SetGap { parent, gap } => {
            let mut changed = false;
            for node in scene.nodes_mut(parent) {
                changed |= set_gap(node, *gap);
            }
            changed
        }
        FixOp::SnapGapsToGrid { parent, grid } => {
            if *grid <= 0.0 || !grid.is_finite() {
                return false;
            }
            let mut changed = false;
            for node in scene.nodes_mut(parent) {
                // Snap each band-adjacent gap to the nearest grid
                // multiple, min `grid`.
                let mut axis = Axis::X;
                let kids = sorted_children(node, &mut axis);
                if kids.len() < 2 {
                    continue;
                }
                let mut cursor: Option<(usize, f64)> = None;
                for &idx in &kids {
                    let (pos, _) = axis_pos(&node.children[idx].bounds, axis);
                    if let Some((prev_idx, prev_end)) = cursor {
                        if band_adjacent(&node.children[prev_idx], &node.children[idx], axis) {
                            let cur = pos - prev_end;
                            let snapped = (cur / grid).round().max(1.0) * grid;
                            if (snapped - cur).abs() > 0.01 {
                                shift_child(node, idx, axis, prev_end + snapped - pos);
                                changed = true;
                            }
                        }
                    }
                    let (_, end) = axis_pos(&node.children[idx].bounds, axis);
                    cursor = Some((idx, end));
                }
            }
            changed
        }
        FixOp::AlignSiblings { parent, edge } => {
            let mut changed = false;
            for node in scene.nodes_mut(parent) {
                changed |= align_siblings(node, *edge);
            }
            changed
        }
        FixOp::RecolorFill { path, from, to } => {
            let mut changed = false;
            for node in scene.nodes_mut(path) {
                let mut hit = false;
                for f in &mut node.fills {
                    if f.color == *from {
                        f.color = *to;
                        hit = true;
                    }
                }
                if hit {
                    changed = true;
                    sync_colors(node, from, to);
                }
            }
            changed
        }
        FixOp::RecolorText { path, from, to } => {
            let mut changed = false;
            for node in scene.nodes_mut(path) {
                let mut hit = false;
                for t in &mut node.texts {
                    if t.color == *from {
                        t.color = *to;
                        hit = true;
                    }
                }
                if hit {
                    changed = true;
                    sync_colors(node, from, to);
                }
            }
            changed
        }
        FixOp::SetFontSize { path, from, to } => {
            let mut changed = false;
            for node in scene.nodes_mut(path) {
                let mut hit = false;
                for t in &mut node.texts {
                    if (t.size - from).abs() < 0.5 {
                        t.size = *to;
                        hit = true;
                        changed = true;
                    }
                }
                if hit {
                    node.font_sizes.retain(|s| (*s - from).abs() >= 0.5);
                    if !node.font_sizes.iter().any(|s| (*s - to).abs() < 0.5) {
                        node.font_sizes.push(*to);
                    }
                }
            }
            changed
        }
        FixOp::GrowBounds { path, min_w, min_h } => {
            let mut changed = false;
            for node in scene.nodes_mut(path) {
                let mut r = node.bounds;
                let nx1 = r.x1.max(r.x0 + min_w);
                let ny1 = r.y1.max(r.y0 + min_h);
                if nx1 > r.x1 || ny1 > r.y1 {
                    r.x1 = nx1;
                    r.y1 = ny1;
                    node.bounds = r;
                    // Fills clipped against the old bounds may now
                    // legitimately cover more — refresh the stat so
                    // coverage rules see the grown surface.
                    node.painted_area = node
                        .fills
                        .iter()
                        .map(|f| {
                            let v = f.rect.intersect(node.bounds);
                            v.width().max(0.0) * v.height().max(0.0)
                        })
                        .sum::<f64>()
                        .min(node.area());
                    changed = true;
                }
            }
            changed
        }
    }
}

/// Horizontal or vertical — whichever axis the children spread along.
#[derive(Clone, Copy, PartialEq)]
enum Axis {
    X,
    Y,
}

fn axis_pos(r: &kurbo::Rect, axis: Axis) -> (f64, f64) {
    match axis {
        Axis::X => (r.x0, r.x1),
        Axis::Y => (r.y0, r.y1),
    }
}

/// Child indices sorted by dominant-axis position. `None` children
/// stay in place — zero-area or off-axis siblings aren't touched.
fn sorted_children(node: &LintNode, axis_out: &mut Axis) -> Vec<usize> {
    let xs: Vec<f64> = node.children.iter().map(|c| c.bounds.x0).collect();
    let ys: Vec<f64> = node.children.iter().map(|c| c.bounds.y0).collect();
    let spread = |v: &[f64]| {
        v.iter().copied().fold(f64::NEG_INFINITY, f64::max)
            - v.iter().copied().fold(f64::INFINITY, f64::min)
    };
    *axis_out = if spread(&xs) >= spread(&ys) {
        Axis::X
    } else {
        Axis::Y
    };
    let axis = *axis_out;
    let mut idx: Vec<usize> = (0..node.children.len()).collect();
    idx.sort_by(|&a, &b| {
        axis_pos(&node.children[a].bounds, axis)
            .0
            .partial_cmp(&axis_pos(&node.children[b].bounds, axis).0)
            .unwrap_or(std::cmp::Ordering::Equal)
    });
    idx
}

fn shift_child(node: &mut LintNode, idx: usize, axis: Axis, delta: f64) {
    let d = match axis {
        Axis::X => Vec2::new(delta, 0.0),
        Axis::Y => Vec2::new(0.0, delta),
    };
    node.children[idx].translate(d);
}

/// Keep `node.colors` honest after a recolor — drop `from` only when
/// no remaining fill/text still paints it (a color shared between a
/// fill and a text run must survive a fill-only recolor).
fn sync_colors(node: &mut LintNode, from: &[u8; 4], to: &[u8; 4]) {
    let still_used =
        node.fills.iter().any(|f| f.color == *from) || node.texts.iter().any(|t| t.color == *from);
    if !still_used {
        node.colors.retain(|c| c != from);
    }
    if !node.colors.contains(to) {
        node.colors.push(*to);
    }
}

/// Enforce a minimum inter-sibling gap — children spaced wider than
/// `gap` keep their positions (fixes spread siblings apart, never
/// pull them together). Only *band-adjacent* pairs are constrained —
/// children on different rows/columns don't share a gap.
fn set_gap(node: &mut LintNode, gap: f64) -> bool {
    let mut axis = Axis::X;
    let kids = sorted_children(node, &mut axis);
    if kids.len() < 2 {
        return false;
    }
    let mut changed = false;
    let mut cursor: Option<(usize, f64)> = None;
    for &idx in &kids {
        let (pos, _) = axis_pos(&node.children[idx].bounds, axis);
        if let Some((prev_idx, prev_end)) = cursor {
            if band_adjacent(&node.children[prev_idx], &node.children[idx], axis) {
                let want = prev_end + gap;
                if pos < want - 0.01 {
                    shift_child(node, idx, axis, want - pos);
                    changed = true;
                }
            }
        }
        let (_, end) = axis_pos(&node.children[idx].bounds, axis);
        cursor = Some((idx, end));
    }
    changed
}

/// Whether two siblings share the perpendicular band — sitting on
/// the same row (X-axis layout) or column (Y-axis layout), judged by
/// >50% overlap of the shorter extent.
fn band_adjacent(a: &LintNode, b: &LintNode, axis: Axis) -> bool {
    let (a0, a1, b0, b1) = match axis {
        Axis::X => (a.bounds.y0, a.bounds.y1, b.bounds.y0, b.bounds.y1),
        Axis::Y => (a.bounds.x0, a.bounds.x1, b.bounds.x0, b.bounds.x1),
    };
    let overlap = a1.min(b1) - a0.max(b0);
    overlap > 0.5 * (a1 - a0).min(b1 - b0).max(1.0)
}

/// Transitive band clusters along `axis` — children whose
/// perpendicular-band overlaps chain together form one group.
/// For a Left/Right alignment the band axis is X (columns); for
/// Top/Bottom it's Y (rows).
fn band_clusters(node: &LintNode, axis: Axis) -> Vec<Vec<usize>> {
    let mut idx: Vec<usize> = (0..node.children.len()).collect();
    idx.sort_by(|&a, &b| {
        axis_pos(&node.children[a].bounds, axis)
            .0
            .partial_cmp(&axis_pos(&node.children[b].bounds, axis).0)
            .unwrap_or(std::cmp::Ordering::Equal)
    });
    let mut clusters: Vec<Vec<usize>> = Vec::new();
    let mut cur_end = f64::NEG_INFINITY;
    for i in idx {
        let (s, e) = axis_pos(&node.children[i].bounds, axis);
        match clusters.last_mut() {
            Some(cluster) if s <= cur_end + 0.5 => {
                cluster.push(i);
                cur_end = cur_end.max(e);
            }
            _ => {
                clusters.push(vec![i]);
                cur_end = e;
            }
        }
    }
    clusters
}

fn align_siblings(node: &mut LintNode, edge: AlignEdge) -> bool {
    if node.children.len() < 2 {
        return false;
    }
    // Align within perpendicular-band clusters only — pulling every
    // child to one global extreme would collapse a grid's columns or
    // rows onto each other.
    let axis = match edge {
        AlignEdge::Left | AlignEdge::Right => Axis::X,
        AlignEdge::Top | AlignEdge::Bottom => Axis::Y,
    };
    let edge_pos = |r: &kurbo::Rect| match edge {
        AlignEdge::Left => r.x0,
        AlignEdge::Top => r.y0,
        AlignEdge::Right => r.x1,
        AlignEdge::Bottom => r.y1,
    };
    let take_min = matches!(edge, AlignEdge::Left | AlignEdge::Top);
    let mut changed = false;
    for cluster in band_clusters(node, axis) {
        if cluster.len() < 2 {
            continue;
        }
        let target = cluster
            .iter()
            .map(|&i| edge_pos(&node.children[i].bounds))
            .reduce(|a, b| if take_min { a.min(b) } else { a.max(b) })
            .unwrap_or(0.0);
        for &i in &cluster {
            let delta = target - edge_pos(&node.children[i].bounds);
            if delta.abs() > 0.01 {
                shift_child(node, i, axis, delta);
                changed = true;
            }
        }
    }
    changed
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::LintConfig;
    use kurbo::Rect;
    use martensite_core::PaintList;

    fn cramped_scene(gap: f64) -> LintScene {
        let mut list = PaintList::new();
        list.push_scope(None, "App", Rect::new(0.0, 0.0, 800.0, 600.0));
        list.push_scope(None, "Panel", Rect::new(0.0, 0.0, 800.0, 100.0));
        for i in 0..4 {
            let x = i as f64 * (50.0 + gap);
            list.push_scope(None, "Label", Rect::new(x, 10.0, x + 50.0, 40.0));
            list.pop_scope();
        }
        list.pop_scope();
        list.pop_scope();
        LintScene::from_paint_list(&list)
    }

    #[test]
    fn autofix_spaces_cramped_siblings_and_converges() {
        let mut scene = cramped_scene(2.0);
        let report = autofix(&mut scene, &LintConfig::new(), &FixOptions::default());
        assert!(report.applied_count() > 0, "expected fixes: {report:?}");
        assert!(report.converged);
        let panel = scene.node("App/Panel").unwrap();
        for w in panel.children.windows(2) {
            let gap = w[1].bounds.x0 - w[0].bounds.x1;
            assert!(gap >= 3.9, "gap {gap} below the 4px floor");
        }
        // Whitespace no longer fires.
        assert!(report
            .report
            .findings
            .iter()
            .all(|f| f.rule != "whitespace"));
    }

    #[test]
    fn risky_fixes_wait_for_force() {
        // A tiny button gets a Risky GrowBounds fix from target-size.
        let mut list = PaintList::new();
        list.push_scope(None, "App", Rect::new(0.0, 0.0, 800.0, 600.0));
        list.push_scope(None, "Button", Rect::new(10.0, 10.0, 30.0, 26.0));
        list.pop_scope();
        list.pop_scope();
        let mut scene = LintScene::from_paint_list(&list);
        let cfg = LintConfig::new().only_standards(&[crate::Standard::Wcag]);
        let report = autofix(&mut scene, &cfg, &FixOptions::default());
        assert_eq!(report.applied_count(), 0, "risky fix ran without force");
        assert!(report.iterations[0].skipped_risky > 0);
        let opts = FixOptions {
            force: true,
            ..FixOptions::default()
        };
        let report = autofix(&mut scene, &cfg, &opts);
        assert!(report.applied_count() > 0, "force did not apply risky fix");
    }

    #[test]
    fn no_recursive_runs_single_pass() {
        let mut scene = cramped_scene(2.0);
        let opts = FixOptions {
            recursive: false,
            ..FixOptions::default()
        };
        let report = autofix(&mut scene, &LintConfig::new(), &opts);
        assert_eq!(report.iterations.len(), 1);
    }

    #[test]
    fn align_left_does_not_collapse_a_grid() {
        // 2×2 grid — aligning Left must not pull the right column to
        // x=0 (regression: global-extreme alignment collapsed grids).
        let mut list = PaintList::new();
        list.push_scope(None, "App", Rect::new(0.0, 0.0, 800.0, 600.0));
        list.push_scope(None, "Panel", Rect::new(0.0, 0.0, 800.0, 200.0));
        for (x, y) in [(10.0, 10.0), (200.0, 10.0), (12.0, 100.0), (202.0, 100.0)] {
            list.push_scope(None, "Cell", Rect::new(x, y, x + 50.0, y + 40.0));
            list.pop_scope();
        }
        list.pop_scope();
        list.pop_scope();
        let mut scene = LintScene::from_paint_list(&list);
        let changed = apply_op(
            &mut scene,
            &FixOp::AlignSiblings {
                parent: "App/Panel".into(),
                edge: AlignEdge::Left,
            },
        );
        assert!(changed, "the drifted left column should align");
        let panel = scene.node("App/Panel").unwrap();
        // Each column aligned to its OWN cluster min — the left to
        // 10, the right to 200; the global-extreme bug pulled the
        // right column to 10.
        assert_eq!(panel.children[2].bounds.x0, 10.0);
        assert_eq!(panel.children[3].bounds.x0, 200.0);
        assert!(panel.children[1].bounds.x0 > 100.0);
    }

    #[test]
    fn snap_gaps_rejects_nonpositive_grid() {
        let mut scene = cramped_scene(3.0);
        let changed = apply_op(
            &mut scene,
            &FixOp::SnapGapsToGrid {
                parent: "App/Panel".into(),
                grid: 0.0,
            },
        );
        assert!(!changed, "grid=0 must be a no-op, not a NaN shift");
    }

    #[test]
    fn set_gap_ignores_off_band_pairs() {
        // Two children on different rows — no shared gap to enforce.
        let mut list = PaintList::new();
        list.push_scope(None, "App", Rect::new(0.0, 0.0, 800.0, 600.0));
        list.push_scope(None, "Panel", Rect::new(0.0, 0.0, 800.0, 200.0));
        list.push_scope(None, "Label", Rect::new(10.0, 10.0, 60.0, 40.0));
        list.pop_scope();
        list.push_scope(None, "Label", Rect::new(70.0, 100.0, 120.0, 130.0));
        list.pop_scope();
        list.pop_scope();
        list.pop_scope();
        let mut scene = LintScene::from_paint_list(&list);
        let changed = apply_op(
            &mut scene,
            &FixOp::SetGap {
                parent: "App/Panel".into(),
                gap: 20.0,
            },
        );
        assert!(!changed, "off-band siblings share no gap");
    }

    #[test]
    fn recolor_keeps_color_still_used_by_text() {
        // Fill and text share a color; a fill-only recolor must not
        // drop it from `colors` while the text still paints it.
        let mut list = PaintList::new();
        list.push_scope(None, "App", Rect::new(0.0, 0.0, 800.0, 600.0));
        list.push_scope(None, "Badge", Rect::new(10.0, 10.0, 60.0, 40.0));
        list.commands.push(martensite_core::PaintCommand::FillRect(
            Rect::new(10.0, 10.0, 60.0, 40.0),
            [200, 30, 30, 255],
        ));
        list.commands.push(martensite_core::PaintCommand::DrawText(
            kurbo::Point::new(15.0, 30.0),
            "!".to_string(),
            12.0,
            [200, 30, 30, 255],
        ));
        list.pop_scope();
        list.pop_scope();
        let mut scene = LintScene::from_paint_list(&list);
        apply_op(
            &mut scene,
            &FixOp::RecolorFill {
                path: "App/Badge".into(),
                from: [200, 30, 30, 255],
                to: [120, 60, 60, 255],
            },
        );
        let badge = scene.node("App/Badge").unwrap();
        assert!(
            badge.colors.contains(&[200, 30, 30, 255]),
            "text's color was evicted by a fill-only recolor"
        );
        assert!(badge.colors.contains(&[120, 60, 60, 255]));
    }
}
