//! The built-in rule set — everything computable from the scope
//! tree, plus the `frame` module's optional rendered-pixel pass.
//!
//! Threshold defaults are evidence-backed where a standard gives one
//! (WCAG 24pt, Hick ~7 choices) and conservative where the research
//! gives a direction but not a number (chrome ratio, alignment). All
//! thresholds are tunable via [`LintConfig::with_rule_param`] — the
//! defaults encode the standard, not the app.

mod consistency_ext;
mod frame;
mod gestalt;
mod hmi;
mod wcag;

use crate::config::LintConfig;
use crate::fix::{AlignEdge, FixOp, FixSafety, LintFix};
use crate::report::Finding;
use crate::rule::{param, Confidence, LintRule};
use crate::scene::{LintNode, LintScene, NodeKind};
use crate::severity::Severity;
use crate::standard::Standard;

/// A node's effective kind — config `classify` overrides beat the
/// name heuristic.
pub(crate) fn kind_of(n: &LintNode, cfg: &LintConfig) -> NodeKind {
    cfg.classified(&n.name).unwrap_or(n.kind)
}

/// Interactive leaves honoring `classify` overrides — controls not
/// nested inside another control.
pub(crate) fn interactive_leaves<'a>(node: &'a LintNode, cfg: &LintConfig) -> Vec<&'a LintNode> {
    fn collect<'a>(n: &'a LintNode, inside: bool, cfg: &LintConfig, out: &mut Vec<&'a LintNode>) {
        let interactive = kind_of(n, cfg) == NodeKind::Interactive;
        if interactive && !inside {
            out.push(n);
            return; // don't double-count controls nested in controls
        }
        for c in &n.children {
            collect(c, inside || interactive, cfg, out);
        }
    }
    let mut out = Vec::new();
    for c in &node.children {
        collect(c, false, cfg, &mut out);
    }
    out
}

/// Union of rect areas clipped to `clip` — the "used area" share
/// NUREG-0700's packing density measures. X-slab sweep: at each
/// interval between consecutive x-edges, every rect covering the
/// slab's midpoint spans the whole slab (slab boundaries are rect
/// edges), so merging their y-intervals gives the covered column.
/// O(n²) worst case — trivial at scene sizes (~100 leaves).
pub(crate) fn union_area(rects: impl IntoIterator<Item = kurbo::Rect>, clip: kurbo::Rect) -> f64 {
    let rects: Vec<kurbo::Rect> = rects
        .into_iter()
        .map(|r| r.intersect(clip))
        .filter(|r| r.width() > 0.0 && r.height() > 0.0)
        .collect();
    if rects.is_empty() {
        return 0.0;
    }
    let mut xs: Vec<f64> = rects.iter().flat_map(|r| [r.x0, r.x1]).collect();
    xs.sort_by(f64::total_cmp);
    xs.dedup();
    let mut area = 0.0;
    for slab in xs.windows(2) {
        let (x0, x1) = (slab[0], slab[1]);
        let mut covered = 0.0;
        let mut open: Option<(f64, f64)> = None;
        // Slab boundaries are exactly the rect edges, so a rect spans
        // the slab iff it covers both endpoints.
        let mut ys: Vec<(f64, f64)> = rects
            .iter()
            .filter(|r| r.x0 <= x0 && r.x1 >= x1)
            .map(|r| (r.y0, r.y1))
            .collect();
        ys.sort_by(|a, b| f64::total_cmp(&a.0, &b.0));
        for (y0, y1) in ys {
            match open {
                Some((cy0, cy1)) if y0 <= cy1 => open = Some((cy0, cy1.max(y1))),
                Some((cy0, cy1)) => {
                    covered += cy1 - cy0;
                    open = Some((y0, y1));
                }
                None => open = Some((y0, y1)),
            }
        }
        if let Some((y0, y1)) = open {
            covered += y1 - y0;
        }
        area += covered * (x1 - x0);
    }
    area
}

/// Estimated character-cell area of a node's text runs — advance
/// width × em box per run. `width` carries real advances when the
/// paint list recorded them; otherwise ~0.5em/char.
pub(crate) fn text_cell_area(node: &LintNode) -> f64 {
    node.texts
        .iter()
        .map(|t| {
            let w = t
                .width
                .unwrap_or_else(|| f64::from(t.size) * 0.5 * t.text.chars().count() as f64);
            w * f64::from(t.size)
        })
        .sum()
}

/// Estimated character count of a text run — the real string for
/// `DrawText` stats, or advance-width inference (~0.5em per char) for
/// `DrawGlyphRun` stats whose text isn't recorded. A one-glyph icon
/// run estimates ~2 chars, below the alphanumeric threshold.
fn estimated_chars(t: &crate::scene::TextStat) -> usize {
    if !t.text.is_empty() {
        return t.text.chars().count();
    }
    match t.width {
        Some(w) if w > 0.0 => (w / (f64::from(t.size).max(1.0) * 0.5)).round() as usize,
        _ => 0,
    }
}

/// True when any single run on the node looks like real text — ≥3
/// estimated characters in *one* run. Per-run, not summed: a leaf of
/// three icon glyphs is a control cluster, not an alphanumeric
/// element.
fn has_text_run(node: &LintNode) -> bool {
    node.texts.iter().any(|t| estimated_chars(t) >= 3)
}

/// Fraction of leaf-bounds area belonging to *alphanumeric elements*
/// — the "largely alphanumeric" test NUREG-0700 and FAA HFDS use to
/// select the stricter density cap. A leaf counts when its text cells
/// cover ≥5% of its own bounds *and* it carries ≥3 estimated
/// characters: a Chart with a caption is a display element, an icon
/// button is a control element, and a Label full of log lines is an
/// alphanumeric element. Leaf bounds are clipped to the surface, the
/// same convention `union_area` uses for the density numerator.
pub(crate) fn text_leaf_share(node: &LintNode) -> f64 {
    let mut text_area = 0.0;
    let mut total = 0.0;
    for d in node.walk().skip(1) {
        if !d.children.is_empty() {
            continue;
        }
        let a = {
            let c = d.bounds.intersect(node.bounds);
            (c.width() * c.height()).max(0.0)
        };
        total += a;
        if a > 0.0 && has_text_run(d) && text_cell_area(d) >= 0.05 * a {
            text_area += a;
        }
    }
    if total <= 0.0 {
        0.0
    } else {
        text_area / total
    }
}

/// Word segments of a widget name — PascalCase/camelCase humps,
/// snake/kebab separators, and acronym tails (`LEDMatrix` →
/// `["led", "matrix"]`) all become boundaries.
fn name_segments(name: &str) -> Vec<String> {
    let chars: Vec<char> = name.chars().collect();
    let mut segs = Vec::new();
    let mut cur = String::new();
    for (i, ch) in chars.iter().enumerate() {
        if !ch.is_alphanumeric() {
            if !cur.is_empty() {
                segs.push(std::mem::take(&mut cur));
            }
            continue;
        }
        let prev = chars.get(i.wrapping_sub(1)).copied();
        let boundary = !cur.is_empty()
            && prev.is_some_and(|p| {
                // alpha↔digit transitions always split ("Chart2D" →
                // ["chart","2","d"], "ISO8601Chart" → ["iso","8601","chart"])
                (p.is_alphabetic() && ch.is_ascii_digit())
                    || (p.is_ascii_digit() && ch.is_alphabetic())
                    || (ch.is_uppercase()
                        && (p.is_lowercase()
                            || (p.is_uppercase()
                                && chars.get(i + 1).is_some_and(|n| n.is_lowercase()))))
            });
        if boundary {
            segs.push(std::mem::take(&mut cur));
        }
        cur.push(ch.to_ascii_lowercase());
    }
    if !cur.is_empty() {
        segs.push(cur);
    }
    segs
}

/// Name-table test for live data displays — the monitoring
/// "channels" `simultaneous-channels` counts. Name-based rather than
/// kind-based: a `Label` is Content too, but it isn't a channel.
/// Matching is word-segment exact — `Dialog` contains `dial` and
/// `Paragraph` contains `graph`, but neither is a channel.
pub(crate) fn is_data_display(name: &str) -> bool {
    let short = name.rsplit("::").next().unwrap_or(name);
    let (short, _) = short.split_once('@').unwrap_or((short, ""));
    let segs = name_segments(short.trim());
    const SEGMENT_DISPLAYS: &[&str] = &[
        "chart",
        "graph",
        "plot",
        "sparkline",
        "gauge",
        "dial",
        "meter",
        "indicator",
        "progress",
        "table",
        "led",
        "ticker",
        "media",
        "map",
        "kpi",
        "gantt",
        "treemap",
        "timeline",
        "fishbone",
        "terminal",
        "heatmap",
        "histogram",
        "diagram",
        "canvas",
        "spectrum",
        "waterfall",
        "marquee",
    ];
    // Compounds that don't reduce to a single segment keyword —
    // `StackLight`, `SplitFlap`, `MapView`, `DataGrid`, `LedMatrix`.
    const COMPOUND_DISPLAYS: &[&str] = &[
        "stacklight",
        "splitflap",
        "mapview",
        "minimap",
        "datatable",
        "datagrid",
        "ledmatrix",
        "videogrid",
        "mindmap",
        "flowgraph",
        "nodegraph",
    ];
    if segs.iter().any(|s| SEGMENT_DISPLAYS.contains(&s.as_str())) {
        return true;
    }
    // Compound names match at segment boundaries, so a prefixed
    // `MyStackLight`/`ZoneDataGrid` still counts.
    (0..segs.len()).any(|i| COMPOUND_DISPLAYS.contains(&segs[i..].concat().as_str()))
}

/// Topmost data-display descendants — a Chart's internal parts don't
/// each count as a channel; the chart is one display.
///
/// `inside` starts from the anchor's own display-ness: when the scope
/// under test IS a display (e.g. a `Table` node), its rows and headers
/// are internals, not channels — without this a `TableRowChild`/`TableHeaderChild`
/// name segment matches `table` and one widget self-reports as N displays.
pub(crate) fn data_display_leaves(node: &LintNode) -> Vec<&LintNode> {
    fn collect<'a>(n: &'a LintNode, inside: bool, out: &mut Vec<&'a LintNode>) {
        let display = is_data_display(&n.name);
        if display && !inside {
            out.push(n);
            return;
        }
        for c in &n.children {
            collect(c, inside || display, out);
        }
    }
    let mut out = Vec::new();
    let inside = is_data_display(&node.name);
    for c in &node.children {
        collect(c, inside, &mut out);
    }
    out
}

/// Every built-in rule, in stable report order.
pub fn all_rules() -> Vec<&'static dyn LintRule> {
    let mut rules: Vec<&'static dyn LintRule> = vec![
        &NavDepth,
        &ChoiceCount,
        &ChromeRatio,
        &Alignment,
        &TargetSize,
        &TypeScale,
        &InteractiveDensity,
        &ColorBudget,
        &AlertSaturation,
        &Whitespace,
        &TokenDrift,
        &EdgeDensity,
        &ColorOnlyInfo,
        &ProgressiveDisclosure,
    ];
    rules.extend(wcag::rules());
    rules.extend(hmi::rules());
    rules.extend(gestalt::rules());
    rules.extend(consistency_ext::rules());
    rules.extend(frame::rules());
    rules
}

/// Surfaces are evaluated per subtree; tiny subtrees are noise.
/// `min_surface_pt` gates area-sensitive rules — below it a node is a
/// control cluster, not a surface.
pub(crate) fn surface_nodes<'a>(
    scene: &'a LintScene,
    cfg: &LintConfig,
    rule: &str,
) -> Vec<&'a LintNode> {
    let min_area_pt = param(cfg, rule, "min_surface_pt", 20_000.0);
    let min_px = min_area_pt * f64::from(scene.scale_factor).powi(2);
    scene
        .walk()
        .filter(|n| n.children.len() >= 2 && n.area() >= min_px)
        .collect()
}

/// — `nav-depth`: stacked orientation layers per surface.
///
/// Counts navigation *layers* — a Navigation node whose parent is not
/// Navigation — on each root→node path. A Tabs containing a TabBar is
/// one layer; a strip of controls inside a tabbed page is a second.
/// ISA-101's four-level display hierarchy exists because operators
/// lose the thread when orientation devices stack; Nielsen #8 says
/// the same for everyone else.
struct NavDepth;
impl LintRule for NavDepth {
    fn id(&self) -> &'static str {
        "nav-depth"
    }
    fn standards(&self) -> &'static [Standard] {
        &[Standard::Isa101, Standard::InfoDesign]
    }
    fn default_severity(&self) -> Severity {
        Severity::Warn
    }
    fn title(&self) -> &'static str {
        "stacked navigation/orientation layers per surface"
    }
    fn citation(&self) -> &'static str {
        "ISA-101 progressive display hierarchy; Nielsen heuristic #8"
    }
    fn check(&self, scene: &LintScene, cfg: &LintConfig) -> Vec<Finding> {
        let max = param(cfg, self.id(), "max", 2.0) as usize;
        let mut out = Vec::new();
        fn walk(
            node: &LintNode,
            nav_layers: usize,
            max: usize,
            cfg: &LintConfig,
            out: &mut Vec<Finding>,
        ) {
            let layers = nav_layers + usize::from(kind_of(node, cfg) == NodeKind::Navigation);
            if layers > max {
                // Report once, at the node where the depth is exceeded.
                out.push(
                    Finding::new(
                        "nav-depth",
                        &node.path,
                        format!(
                            "{} stacked navigation layers — the user re-orients at each \
                             (title → tabs → strip → rail reads as four separate systems)",
                            layers
                        ),
                    )
                    .at(node.bounds),
                );
                return; // don't cascade findings down the subtree
            }
            for c in &node.children {
                walk(c, layers, max, cfg, out);
            }
        }
        for r in &scene.roots {
            walk(r, 0, max, cfg, &mut out);
        }
        out
    }
}

/// — `choice-count`: simultaneous actions in one decision surface.
///
/// Counts interactive leaves under each node; flags the *innermost*
/// containers that still exceed the budget — the strip itself, not
/// the window that happens to contain it. Hick's law puts decision
/// cost at log₂(n+1); Cowan's revision of Miller puts working-memory
/// chunks at ~4. Default `max = 7` (the generous classic bound);
/// `max = 4` is the defensible strict bound.
struct ChoiceCount;
impl LintRule for ChoiceCount {
    fn id(&self) -> &'static str {
        "choice-count"
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
        "simultaneous choices in one decision surface"
    }
    fn citation(&self) -> &'static str {
        "Hick's law: decision time ∝ log2(n+1); Cowan 4±1 chunks"
    }
    fn check(&self, scene: &LintScene, cfg: &LintConfig) -> Vec<Finding> {
        let max = param(cfg, self.id(), "max", 7.0) as usize;
        let mut out = Vec::new();
        fn visit(node: &LintNode, max: usize, cfg: &LintConfig, out: &mut Vec<Finding>) {
            let count = interactive_leaves(node, cfg).len();
            // Defer to children that are themselves over budget —
            // report the deepest overloaded group, not the window.
            let deeper_over = node
                .children
                .iter()
                .any(|c| interactive_leaves(c, cfg).len() > max);
            if count > max && !deeper_over {
                out.push(
                    Finding::new(
                        "choice-count",
                        &node.path,
                        format!(
                            "{count} simultaneous actions in one surface (max {max}) — \
                             consider grouping into categories or an overflow menu"
                        ),
                    )
                    .at(node.bounds),
                );
            }
            for c in &node.children {
                visit(c, max, cfg, out);
            }
        }
        for r in &scene.roots {
            visit(r, max, cfg, &mut out);
        }
        out
    }
}

/// — `chrome-ratio`: non-content area share per surface.
///
/// Sum of Navigation + Chrome child bounds over the surface's area.
/// Tufte's data-ink ratio as a structural proxy: pixels spent on
/// orientation furniture are pixels not spent on data.
struct ChromeRatio;
impl LintRule for ChromeRatio {
    fn id(&self) -> &'static str {
        "chrome-ratio"
    }
    fn standards(&self) -> &'static [Standard] {
        &[Standard::InfoDesign]
    }
    fn default_severity(&self) -> Severity {
        Severity::Warn
    }
    fn confidence(&self) -> Confidence {
        Confidence::Heuristic
    }
    fn title(&self) -> &'static str {
        "share of surface area spent on chrome, not content"
    }
    fn citation(&self) -> &'static str {
        "Tufte data-ink ratio; Few, Information Dashboard Design ch.3"
    }
    fn check(&self, scene: &LintScene, cfg: &LintConfig) -> Vec<Finding> {
        let max = param(cfg, self.id(), "max", 0.45);
        let mut out = Vec::new();
        for n in surface_nodes(scene, cfg, self.id()) {
            let chrome: f64 = n
                .children
                .iter()
                .filter(|c| matches!(kind_of(c, cfg), NodeKind::Navigation | NodeKind::Chrome))
                .map(|c| c.bounds.intersect(n.bounds))
                .map(|r| (r.width() * r.height()).max(0.0))
                .sum();
            let ratio = chrome / n.area().max(1.0);
            if ratio > max {
                out.push(
                    Finding::new(
                        "chrome-ratio",
                        &n.path,
                        format!(
                            "chrome occupies {:.0}% of this surface (guideline: ≤{:.0}%) — \
                             the content is fighting its own frame",
                            ratio * 100.0,
                            max * 100.0
                        ),
                    )
                    .at(n.bounds),
                );
            }
        }
        out
    }
}

/// — `alignment`: sibling edge regularity.
///
/// Miniukovich & De Angeli's alignment metric: every distinct sibling
/// edge is a line the eye must trace. A clean column has 1 distinct
/// left edge; a clean row has 1 distinct top edge. Siblings that
/// align in *neither* axis read as scattered.
struct Alignment;
impl LintRule for Alignment {
    fn id(&self) -> &'static str {
        "alignment"
    }
    fn standards(&self) -> &'static [Standard] {
        &[Standard::Perception, Standard::Consistency]
    }
    fn default_severity(&self) -> Severity {
        Severity::Info
    }
    fn confidence(&self) -> Confidence {
        Confidence::Heuristic
    }
    fn title(&self) -> &'static str {
        "sibling edge alignment regularity"
    }
    fn citation(&self) -> &'static str {
        "Miniukovich & De Angeli, CHI 2015 — alignment explains ~49% of aesthetic judgement"
    }
    fn check(&self, scene: &LintScene, cfg: &LintConfig) -> Vec<Finding> {
        let max_edges = param(cfg, self.id(), "max_edges", 4.0) as usize;
        let min_children = param(cfg, self.id(), "min_children", 4.0) as usize;
        let tol = param(cfg, self.id(), "tolerance_px", 2.0);
        let mut out = Vec::new();
        for n in scene.walk() {
            let children: Vec<&LintNode> = n.children.iter().filter(|c| c.area() > 1.0).collect();
            if children.len() < min_children {
                continue;
            }
            let distinct = |f: fn(&LintNode) -> f64| -> usize {
                let mut edges: Vec<i64> = children
                    .iter()
                    .map(|c| (f(c) / tol).round() as i64)
                    .collect();
                edges.sort_unstable();
                edges.dedup();
                edges.len()
            };
            let lefts = distinct(|c| c.bounds.x0);
            let tops = distinct(|c| c.bounds.y0);
            // Scattered = aligned in neither axis. A column has few
            // lefts; a row has few tops; a grid has few of both.
            if lefts > max_edges && tops > max_edges {
                // Fix to the axis that's *closer* to already-aligned.
                let (edge, label) = if lefts <= tops {
                    (AlignEdge::Left, "left edges")
                } else {
                    (AlignEdge::Top, "top edges")
                };
                out.push(
                    Finding::new(
                        "alignment",
                        &n.path,
                        format!(
                            "{} children share neither axis — {lefts} distinct left edges, \
                             {tops} distinct top edges (grids align on at least one)",
                            children.len()
                        ),
                    )
                    .at(n.bounds)
                    .with_fix(LintFix::new(
                        format!("align the children's {label}"),
                        FixSafety::Safe,
                        FixOp::AlignSiblings {
                            parent: n.path.clone(),
                            edge,
                        },
                    )),
                );
            }
        }
        out
    }
}

/// — `target-size`: interactive element floor.
///
/// WCAG 2.2 SC 2.5.8 Target Size (Minimum): 24×24 CSS px — the *floor*.
/// Apple HIG says 44pt, Material says 48dp; tune `min_pt` up for
/// touch-first products. The standard's own exceptions (inline text,
/// spacing-equivalent, essential size) are documented limits — allow
/// those cases explicitly rather than weakening the default.
struct TargetSize;
impl LintRule for TargetSize {
    fn id(&self) -> &'static str {
        "target-size"
    }
    fn standards(&self) -> &'static [Standard] {
        &[Standard::Wcag]
    }
    fn default_severity(&self) -> Severity {
        Severity::Warn
    }
    fn title(&self) -> &'static str {
        "interactive elements below the minimum target size"
    }
    fn citation(&self) -> &'static str {
        "WCAG 2.2 SC 2.5.8 Target Size (Minimum): 24×24pt; HIG 44pt / Material 48dp for touch"
    }
    fn check(&self, scene: &LintScene, cfg: &LintConfig) -> Vec<Finding> {
        let min_pt = param(cfg, self.id(), "min_pt", 24.0);
        let min_px = min_pt * f64::from(scene.scale_factor);
        let mut out = Vec::new();
        for n in scene.walk() {
            if kind_of(n, cfg) != NodeKind::Interactive {
                continue;
            }
            let (w, h) = (n.bounds.width(), n.bounds.height());
            if w < 1.0 || h < 1.0 {
                continue; // unlaid-out or zero-area — not this rule's problem
            }
            if w < min_px || h < min_px {
                out.push(
                    Finding::new(
                        "target-size",
                        &n.path,
                        format!(
                            "interactive target {:.0}×{:.0}pt — under the {:.0}pt minimum \
                             (motor-impaired users miss small targets disproportionately)",
                            w / f64::from(scene.scale_factor),
                            h / f64::from(scene.scale_factor),
                            min_pt
                        ),
                    )
                    .at(n.bounds)
                    .with_fix(LintFix::new(
                        format!("grow the target to ≥{min_pt:.0}pt"),
                        FixSafety::Risky,
                        FixOp::GrowBounds {
                            path: n.path.clone(),
                            min_w: min_px,
                            min_h: min_px,
                        },
                    )),
                );
            }
        }
        out
    }
}

/// — `type-scale`: distinct text sizes per surface.
///
/// Typographic canon: a view that needs more than ~4–5 text roles is
/// usually a hierarchy problem, not a font problem. Sizes are
/// quantized to 0.5pt before counting.
struct TypeScale;
impl LintRule for TypeScale {
    fn id(&self) -> &'static str {
        "type-scale"
    }
    fn standards(&self) -> &'static [Standard] {
        &[Standard::Consistency]
    }
    fn default_severity(&self) -> Severity {
        Severity::Info
    }
    fn confidence(&self) -> Confidence {
        Confidence::Heuristic
    }
    fn title(&self) -> &'static str {
        "distinct font sizes per surface"
    }
    fn citation(&self) -> &'static str {
        "typographic scale discipline — a surface should need ≤4–5 text roles"
    }
    fn check(&self, scene: &LintScene, cfg: &LintConfig) -> Vec<Finding> {
        let max = param(cfg, self.id(), "max", 5.0) as usize;
        let mut out = Vec::new();
        for n in surface_nodes(scene, cfg, self.id()) {
            let sizes = n.subtree_font_sizes();
            if sizes.len() > max {
                out.push(
                    Finding::new(
                        "type-scale",
                        &n.path,
                        format!(
                            "{} distinct text sizes (max {max}) — a modular scale usually \
                             needs 4–5 roles; more is usually inconsistent hierarchy",
                            sizes.len()
                        ),
                    )
                    .at(n.bounds),
                );
            }
        }
        out
    }
}

/// — `interactive-density`: controls per unit area.
///
/// Fitts + crowding research: control density, not just count, drives
/// mis-clicks and search time. Density is controls per 10,000pt²
/// (a 100×100pt square). Dense data grids legitimately exceed the
/// default — that's what `with_rule_param` is for.
struct InteractiveDensity;
impl LintRule for InteractiveDensity {
    fn id(&self) -> &'static str {
        "density"
    }
    fn standards(&self) -> &'static [Standard] {
        &[Standard::HciLaws, Standard::Perception]
    }
    fn default_severity(&self) -> Severity {
        Severity::Info
    }
    fn confidence(&self) -> Confidence {
        Confidence::Heuristic
    }
    fn title(&self) -> &'static str {
        "interactive controls per unit surface area"
    }
    fn citation(&self) -> &'static str {
        "Fitts's law mis-click risk scales with crowding; Rosenholtz clutter research"
    }
    fn check(&self, scene: &LintScene, cfg: &LintConfig) -> Vec<Finding> {
        let max = param(cfg, self.id(), "max", 0.6);
        let sf2 = f64::from(scene.scale_factor).powi(2);
        let mut out = Vec::new();
        for n in surface_nodes(scene, cfg, self.id()) {
            let leaves = interactive_leaves(n, cfg).len();
            if leaves < 4 {
                continue;
            }
            let area_pt2 = n.area() / sf2;
            let density = leaves as f64 / (area_pt2 / 10_000.0).max(1.0);
            if density > max {
                out.push(
                    Finding::new(
                        "density",
                        &n.path,
                        format!(
                            "{leaves} controls in {:.0}k pt² ({density:.2}/10k pt², max {max}) — \
                             dense control fields raise mis-click and search cost",
                            area_pt2 / 1000.0
                        ),
                    )
                    .at(n.bounds),
                );
            }
        }
        out
    }
}

/// — `color-budget`: saturated-hue count per surface.
///
/// ISA-101/ASM color discipline: saturated color is the alarm channel.
/// A surface where five saturated hues compete has spent its alarm
/// budget on decoration — when something goes wrong, nothing stands
/// out. Counts distinct hue buckets among opaque, saturated, mid-
/// lightness colors in the subtree.
struct ColorBudget;
impl LintRule for ColorBudget {
    fn id(&self) -> &'static str {
        "color-budget"
    }
    fn standards(&self) -> &'static [Standard] {
        &[Standard::Isa101, Standard::Consistency]
    }
    fn default_severity(&self) -> Severity {
        Severity::Warn
    }
    fn confidence(&self) -> Confidence {
        Confidence::Heuristic
    }
    fn title(&self) -> &'static str {
        "distinct saturated hues competing for attention"
    }
    fn citation(&self) -> &'static str {
        "ISA-101/ASM: saturated color is the abnormal-state channel — spend it only there"
    }
    fn check(&self, scene: &LintScene, cfg: &LintConfig) -> Vec<Finding> {
        let max = param(cfg, self.id(), "max", 3.0) as usize;
        let min_sat = param(cfg, self.id(), "min_saturation", 0.4);
        let mut out = Vec::new();
        for n in surface_nodes(scene, cfg, self.id()) {
            let mut hues = std::collections::BTreeSet::new();
            for c in n.subtree_colors() {
                if let Some(h) = saturated_hue_bucket(c, min_sat) {
                    hues.insert(h);
                }
            }
            if hues.len() > max {
                // Fix: desaturate the offending fills toward their own
                // luminance — the ISA-101 "gray canvas" correction.
                // One op per (node, saturated fill color); Risky
                // because recoloring is a design decision.
                let mut ops = Vec::new();
                for d in n.walk() {
                    for f in &d.fills {
                        if saturated_hue_bucket(f.color, min_sat).is_some() {
                            ops.push(FixOp::RecolorFill {
                                path: d.path.clone(),
                                from: f.color,
                                to: desaturate(f.color, 0.2),
                            });
                        }
                    }
                }
                out.push(
                    Finding::new(
                        "color-budget",
                        &n.path,
                        format!(
                            "{} distinct saturated hues (max {max}) — saturated color is \
                             attention; spending it on decoration empties the alarm budget",
                            hues.len()
                        ),
                    )
                    .at(n.bounds)
                    .with_fix(LintFix {
                        summary: format!("desaturate {} non-alarm fill(s) toward gray", ops.len()),
                        safety: FixSafety::Risky,
                        ops,
                    }),
                );
            }
        }
        out
    }
}

/// — `alert-saturation`: simultaneous alert-level signals.
///
/// ISA-18.2's design-time analog: alarm floods destroy response. If
/// more than a couple of surfaces are screaming simultaneously, none
/// of them are. Counts alert-named scopes plus subtrees dominated by
/// alarm-red fills.
struct AlertSaturation;
impl LintRule for AlertSaturation {
    fn id(&self) -> &'static str {
        "alert-saturation"
    }
    fn standards(&self) -> &'static [Standard] {
        &[Standard::Isa182, Standard::Isa101]
    }
    fn default_severity(&self) -> Severity {
        Severity::Warn
    }
    fn confidence(&self) -> Confidence {
        Confidence::Heuristic
    }
    fn title(&self) -> &'static str {
        "simultaneous alert-level signals in one view"
    }
    fn citation(&self) -> &'static str {
        "ISA-18.2 alarm-flood doctrine applied at design time: >10/10min is unmanageable — the screen analog is simultaneous alert surfaces"
    }
    fn check(&self, scene: &LintScene, cfg: &LintConfig) -> Vec<Finding> {
        let max = param(cfg, self.id(), "max", 3.0) as usize;
        let mut out = Vec::new();
        for n in surface_nodes(scene, cfg, self.id()) {
            let mut alerts = 0usize;
            for d in n.walk() {
                let lower = d.name.to_ascii_lowercase();
                let named = lower.contains("alert")
                    || lower.contains("alarm")
                    || lower.contains("critical");
                // A leaf whose own colors include a saturated alarm-red
                // fill counts as an alert signal.
                let red_filled =
                    !named && d.children.is_empty() && d.colors.iter().any(|c| is_alarm_red(*c));
                if named || red_filled {
                    alerts += 1;
                }
            }
            if alerts > max {
                out.push(
                    Finding::new(
                        "alert-saturation",
                        &n.path,
                        format!(
                            "{alerts} simultaneous alert signals (max {max}) — operators \
                             triage ~1–2 alarms/10min; beyond that, alarms become noise"
                        ),
                    )
                    .at(n.bounds),
                );
            }
        }
        out
    }
}

/// — `whitespace`: sibling gaps below the grouping threshold.
///
/// Gestalt proximity is the strongest grouping cue — stronger than
/// color or shape. When adjacent siblings sit closer than a few
/// points the eye cannot resolve "these belong together, those
/// don't"; the surface reads as one undifferentiated block no matter
/// how logical the hierarchy is. Measures the median gap between
/// adjacent siblings along each axis and flags containers whose
/// spacing falls under `min_gap_pt`.
struct Whitespace;
impl LintRule for Whitespace {
    fn id(&self) -> &'static str {
        "whitespace"
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
        "sibling spacing below the grouping threshold"
    }
    fn citation(&self) -> &'static str {
        "Gestalt proximity — spacing is the primary grouping signal; \
         Few, Information Dashboard Design (whitespace is parsing, not waste)"
    }
    fn check(&self, scene: &LintScene, cfg: &LintConfig) -> Vec<Finding> {
        let min_gap = param(cfg, self.id(), "min_gap_pt", 4.0) * f64::from(scene.scale_factor);
        let min_children = param(cfg, self.id(), "min_children", 3.0) as usize;
        let mut out = Vec::new();
        for n in scene.walk() {
            let children: Vec<&LintNode> = n.children.iter().filter(|c| c.area() > 1.0).collect();
            if children.len() < min_children {
                continue;
            }
            let mut gaps: Vec<f64> = Vec::new();
            // Horizontal gaps between siblings sharing a y-band;
            // vertical gaps between siblings sharing an x-band.
            let mut by_x = children.clone();
            by_x.sort_by(|a, b| {
                a.bounds
                    .x0
                    .partial_cmp(&b.bounds.x0)
                    .unwrap_or(std::cmp::Ordering::Equal)
            });
            for pair in by_x.windows(2) {
                let (a, b) = (pair[0], pair[1]);
                let overlap = a.bounds.y1.min(b.bounds.y1) - a.bounds.y0.max(b.bounds.y0);
                let shorter = (a.bounds.y1 - a.bounds.y0).min(b.bounds.y1 - b.bounds.y0);
                if overlap > 0.5 * shorter {
                    gaps.push((b.bounds.x0 - a.bounds.x1).max(0.0));
                }
            }
            let mut by_y = children;
            by_y.sort_by(|a, b| {
                a.bounds
                    .y0
                    .partial_cmp(&b.bounds.y0)
                    .unwrap_or(std::cmp::Ordering::Equal)
            });
            for pair in by_y.windows(2) {
                let (a, b) = (pair[0], pair[1]);
                let overlap = a.bounds.x1.min(b.bounds.x1) - a.bounds.x0.max(b.bounds.x0);
                let shorter = (a.bounds.x1 - a.bounds.x0).min(b.bounds.x1 - b.bounds.x0);
                if overlap > 0.5 * shorter {
                    gaps.push((b.bounds.y0 - a.bounds.y1).max(0.0));
                }
            }
            if gaps.len() < 2 {
                continue;
            }
            gaps.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
            let median = gaps[gaps.len() / 2];
            if median < min_gap {
                out.push(
                    Finding::new(
                        "whitespace",
                        &n.path,
                        format!(
                            "median sibling gap {:.1}px ({:.1}pt) — below {:.0}pt the gaps \
                             stop registering as separators, so grouping flattens into \
                             one block",
                            median,
                            median / f64::from(scene.scale_factor),
                            min_gap / f64::from(scene.scale_factor)
                        ),
                    )
                    .at(n.bounds)
                    .with_fix(LintFix::new(
                        format!(
                            "open sibling gaps to ≥{:.0}px ({:.0}pt)",
                            min_gap,
                            min_gap / f64::from(scene.scale_factor)
                        ),
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

/// — `token-drift`: painted colors that match no declared palette token.
///
/// Design-token discipline: a declared palette is the contract —
/// every off-token fill is a one-off color the system now carries
/// unowned. [`LintConfig`] rule params are `f64`-valued, so the
/// palette is declared numerically: `palette_entries = N` plus
/// `palette_0 ..= palette_{N-1}` as packed `0xRRGGBB` integers (TOML
/// hex literals work: `palette_0 = 0x1B2A3A`). A list/string param
/// mechanism is the pending better seam — until config grows one,
/// this encoding keeps the rule fully functional. With
/// `palette_entries` unset (0) the rule is registered-but-inert.
struct TokenDrift;
impl LintRule for TokenDrift {
    fn id(&self) -> &'static str {
        "token-drift"
    }
    fn standards(&self) -> &'static [Standard] {
        &[Standard::Consistency]
    }
    fn default_severity(&self) -> Severity {
        Severity::Info
    }
    fn confidence(&self) -> Confidence {
        Confidence::Heuristic
    }
    fn title(&self) -> &'static str {
        "painted colors matching no declared palette token"
    }
    fn citation(&self) -> &'static str {
        "design-token conformance — off-palette color is untracked one-off styling \
         (the stylelint color-no-hex / design-token lint model)"
    }
    fn check(&self, scene: &LintScene, cfg: &LintConfig) -> Vec<Finding> {
        let entries = param(cfg, self.id(), "palette_entries", 0.0) as usize;
        if entries == 0 {
            return Vec::new(); // no declared palette — nothing to drift from
        }
        let tolerance = param(cfg, self.id(), "tolerance", 24.0);
        // Packed 0xRRGGBB integers; missing/negative entries skipped.
        let palette: Vec<[u8; 3]> = (0..entries.min(64))
            .filter_map(|i| {
                let v = param(cfg, self.id(), &format!("palette_{i}"), -1.0) as i64;
                (0..=0xFF_FFFF).contains(&v).then_some([
                    ((v >> 16) & 0xFF) as u8,
                    ((v >> 8) & 0xFF) as u8,
                    (v & 0xFF) as u8,
                ])
            })
            .collect();
        if palette.is_empty() {
            return Vec::new();
        }
        let mut out = Vec::new();
        for n in scene.walk() {
            for c in &n.colors {
                if c[3] < 200 {
                    continue; // translucent paints blend — opaque-only check
                }
                let (nearest, dist) = palette
                    .iter()
                    .map(|p| {
                        let d = (i32::from(c[0]) - i32::from(p[0])).abs()
                            + (i32::from(c[1]) - i32::from(p[1])).abs()
                            + (i32::from(c[2]) - i32::from(p[2])).abs();
                        (p, d)
                    })
                    .min_by_key(|(_, d)| *d)
                    .expect("palette is non-empty");
                if f64::from(dist) > tolerance {
                    out.push(
                        Finding::new(
                            "token-drift",
                            &n.path,
                            format!(
                                "painted #{:02X}{:02X}{:02X} — {dist} from nearest palette \
                                 entry #{:02X}{:02X}{:02X} (tolerance {tolerance:.0}) — \
                                 consider mapping to the token or extending the palette",
                                c[0], c[1], c[2], nearest[0], nearest[1], nearest[2]
                            ),
                        )
                        .at(n.bounds),
                    );
                }
            }
        }
        out
    }
}

/// — `edge-density`: clutter proxy from paint coverage and subtree size.
///
/// Pending true raster metrics (edge maps, feature congestion), this
/// approximates Rosenholtz clutter two ways over the scope tree: a
/// scope whose own fills cover nearly all of its bounds has left no
/// whitespace to group by, and a subtree with a very large descendant
/// count is a deep busy surface the eye parses as one crowd. Reports
/// the outermost offender only — a flagged subtree's descendants are
/// not re-reported.
struct EdgeDensity;
impl LintRule for EdgeDensity {
    fn id(&self) -> &'static str {
        "edge-density"
    }
    fn standards(&self) -> &'static [Standard] {
        &[Standard::Perception]
    }
    fn default_severity(&self) -> Severity {
        Severity::Info
    }
    fn confidence(&self) -> Confidence {
        Confidence::Heuristic
    }
    fn title(&self) -> &'static str {
        "clutter proxy — paint coverage and subtree size"
    }
    fn citation(&self) -> &'static str {
        "Rosenholtz et al., feature congestion & clutter (J. Vision 2007); \
         Miniukovich interface density metrics"
    }
    fn check(&self, scene: &LintScene, cfg: &LintConfig) -> Vec<Finding> {
        let max_coverage = param(cfg, self.id(), "max_coverage", 0.85);
        let max_descendants = param(cfg, self.id(), "max_descendants", 40.0) as usize;
        let min_area = param(cfg, self.id(), "min_area", 20_000.0);
        let mut out = Vec::new();
        fn visit(
            node: &LintNode,
            max_coverage: f64,
            max_descendants: usize,
            min_area: f64,
            out: &mut Vec<Finding>,
        ) {
            let mut flagged = false;
            let area = node.area();
            if area > min_area {
                let coverage = (node.painted_area / area).min(1.0);
                if coverage > max_coverage {
                    out.push(
                        Finding::new(
                            "edge-density",
                            &node.path,
                            format!(
                                "fills cover {:.0}% of this scope (max {:.0}%) — near-total \
                                 paint coverage leaves no whitespace for the eye to group by",
                                coverage * 100.0,
                                max_coverage * 100.0
                            ),
                        )
                        .at(node.bounds),
                    );
                    flagged = true;
                }
            }
            let descendants = node.walk().count() - 1;
            if descendants > max_descendants {
                out.push(
                    Finding::new(
                        "edge-density",
                        &node.path,
                        format!(
                            "{descendants} descendant nodes (max {max_descendants}) — a \
                             subtree this deep reads as clutter before content is parsed"
                        ),
                    )
                    .at(node.bounds),
                );
                flagged = true;
            }
            if flagged {
                return; // outermost offender — don't cascade into it
            }
            for c in &node.children {
                visit(c, max_coverage, max_descendants, min_area, out);
            }
        }
        for r in &scene.roots {
            visit(r, max_coverage, max_descendants, min_area, &mut out);
        }
        out
    }
}

/// — `color-only-info`: saturated color patches with no redundant encoding.
///
/// WCAG 1.4.1: color must not be the only visual means of conveying
/// information — ~8% of males carry red-green color-vision deficiency.
/// A scope painting a saturated color over a meaningful area whose
/// whole subtree carries neither text nor a control is a state
/// indicator a CVD user may literally be unable to read. ISA-101
/// makes the same demand as redundant coding for abnormal states.
struct ColorOnlyInfo;
impl LintRule for ColorOnlyInfo {
    fn id(&self) -> &'static str {
        "color-only-info"
    }
    fn standards(&self) -> &'static [Standard] {
        &[Standard::Wcag, Standard::Isa101]
    }
    fn default_severity(&self) -> Severity {
        Severity::Warn
    }
    fn confidence(&self) -> Confidence {
        Confidence::Heuristic
    }
    fn title(&self) -> &'static str {
        "saturated color regions with no text or control backup"
    }
    fn citation(&self) -> &'static str {
        "WCAG 2.2 SC 1.4.1 Use of Color (~8% male red-green CVD); ISA-101 redundant state coding"
    }
    fn check(&self, scene: &LintScene, cfg: &LintConfig) -> Vec<Finding> {
        let min_sat = param(cfg, self.id(), "min_saturation", 0.5);
        let min_area = param(cfg, self.id(), "min_area", 100.0);
        let mut out = Vec::new();
        for n in scene.walk() {
            if n.painted_area <= min_area {
                continue;
            }
            // The strongest saturated opaque-ish paint this scope owns.
            let best = n
                .colors
                .iter()
                .filter(|c| c[3] > 0)
                .map(|c| (rgb_saturation(*c), c))
                .filter(|(s, _)| *s > min_sat)
                .max_by(|a, b| a.0.total_cmp(&b.0));
            let Some((_, c)) = best else {
                continue;
            };
            // Redundant-encoding check: no text anywhere in the
            // subtree, and no interactive descendant (a control
            // affords its own label/state channel).
            let has_text = n.walk().any(|d| !d.font_sizes.is_empty());
            let has_control = n
                .walk()
                .skip(1)
                .any(|d| kind_of(d, cfg) == NodeKind::Interactive);
            if has_text || has_control {
                continue;
            }
            out.push(
                Finding::new(
                    "color-only-info",
                    &n.path,
                    format!(
                        "saturated #{:02X}{:02X}{:02X} patch ({:.0}px²) with no text or \
                         control in its subtree — consider: pair the color with a label \
                         or icon; color alone can't carry meaning",
                        c[0], c[1], c[2], n.painted_area
                    ),
                )
                .at(n.bounds),
            );
        }
        out
    }
}

/// — `progressive-disclosure`: overloaded surfaces with no drill-down.
///
/// ISA-101's L1→L4 display hierarchy exists so operators see the
/// overview first and pull detail on demand; Nielsen #8 says the same
/// for everyone. A surface bearing more than `max` interactive leaves
/// with no disclosure affordance (expander, drawer, overflow menu,
/// accordion, …) presents everything at once. Reports the outermost
/// overloaded surface — flagged ancestors suppress descendants.
struct ProgressiveDisclosure;
impl LintRule for ProgressiveDisclosure {
    fn id(&self) -> &'static str {
        "progressive-disclosure"
    }
    fn standards(&self) -> &'static [Standard] {
        &[Standard::Isa101, Standard::InfoDesign]
    }
    fn default_severity(&self) -> Severity {
        Severity::Info
    }
    fn confidence(&self) -> Confidence {
        Confidence::Heuristic
    }
    fn title(&self) -> &'static str {
        "overloaded surfaces with no disclosure affordance"
    }
    fn citation(&self) -> &'static str {
        "ISA-101 L1–L4 progressive display hierarchy — overview first, detail on demand; \
         Nielsen heuristic #8"
    }
    fn check(&self, scene: &LintScene, cfg: &LintConfig) -> Vec<Finding> {
        /// Disclosure affordances matched case-insensitively against
        /// each descendant's final name segment.
        const AFFORDANCES: &[&str] = &[
            "expander",
            "collapse",
            "disclosure",
            "sheet",
            "drawer",
            "popover",
            "menu",
            "overflow",
            "accordion",
        ];
        let max = param(cfg, self.id(), "max", 10.0) as usize;
        let mut out = Vec::new();
        fn visit(
            node: &LintNode,
            affordances: &[&str],
            max: usize,
            cfg: &LintConfig,
            out: &mut Vec<Finding>,
        ) {
            // Surface kinds: containers and content surfaces — the
            // doc's "Surface" maps onto these (Unknown is treated as
            // Content per NodeKind's contract). Navigation, Chrome,
            // and controls themselves are not disclosure surfaces.
            let surface = matches!(
                kind_of(node, cfg),
                NodeKind::Container | NodeKind::Content | NodeKind::Unknown
            );
            let leaves = interactive_leaves(node, cfg).len();
            if leaves > max && surface {
                let has_affordance = node.walk().skip(1).any(|d| {
                    let lower = d.name.to_ascii_lowercase();
                    affordances.iter().any(|a| lower.contains(a))
                });
                if !has_affordance {
                    out.push(
                        Finding::new(
                            "progressive-disclosure",
                            &node.path,
                            format!(
                                "{leaves} controls on one surface (max {max}) with no \
                                 disclosure affordance — consider progressive disclosure: \
                                 overview first, detail behind an expander/drawer/overflow"
                            ),
                        )
                        .at(node.bounds),
                    );
                    return; // outermost offender — don't cascade into it
                }
            }
            // Children can only hold ≤ the parent's leaf count, but a
            // child may be a *surface* where the parent was not, so
            // only prune when the leaf budget itself can't be exceeded.
            if leaves > max {
                for c in &node.children {
                    visit(c, affordances, max, cfg, out);
                }
            }
        }
        for r in &scene.roots {
            visit(r, AFFORDANCES, max, cfg, &mut out);
        }
        out
    }
}

// ---------- color helpers ----------

/// Hue bucket (30° steps) for a saturated, opaque, mid-lightness
/// color — the "attention colors". Grays, near-blacks, near-whites,
/// and translucent colors return `None`.
pub(crate) fn saturated_hue_bucket(c: [u8; 4], min_sat: f64) -> Option<u32> {
    if c[3] < 200 {
        return None;
    }
    let (r, g, b) = (
        f64::from(c[0]) / 255.0,
        f64::from(c[1]) / 255.0,
        f64::from(c[2]) / 255.0,
    );
    let max = r.max(g).max(b);
    let min = r.min(g).min(b);
    let l = (max + min) / 2.0;
    if !(0.15..0.85).contains(&l) {
        return None;
    }
    let d = max - min;
    if d < 1e-6 {
        return None;
    }
    let s = d / (1.0 - (2.0 * l - 1.0).abs());
    if s < min_sat {
        return None;
    }
    let h = if (max - r).abs() < f64::EPSILON {
        60.0 * (((g - b) / d) % 6.0)
    } else if (max - g).abs() < f64::EPSILON {
        60.0 * ((b - r) / d + 2.0)
    } else {
        60.0 * ((r - g) / d + 4.0)
    };
    let h = h.rem_euclid(360.0);
    Some((h / 30.0) as u32)
}

/// Alarm-red heuristic: saturated red/orange-magenta with mid-to-high
/// lightness — the ISA-101 alarm channel.
pub(crate) fn is_alarm_red(c: [u8; 4]) -> bool {
    let Some(h) = saturated_hue_bucket(c, 0.5) else {
        return false;
    };
    // hue buckets: 0–30° or 330–360° → red family
    h == 0 || h == 11
}

/// HSV saturation in `0.0..=1.0` — chroma (max−min channel) over the
/// max channel. Black (max 0) reads as unsaturated.
pub(crate) fn rgb_saturation(c: [u8; 4]) -> f64 {
    let (r, g, b) = (f64::from(c[0]), f64::from(c[1]), f64::from(c[2]));
    let max = r.max(g).max(b);
    if max <= 0.0 {
        return 0.0;
    }
    (max - r.min(g).min(b)) / max
}

/// sRGB channel → linear-light (WCAG contrast math).
pub(crate) fn linear_channel(v: u8) -> f64 {
    let s = f64::from(v) / 255.0;
    if s <= 0.04045 {
        s / 12.92
    } else {
        ((s + 0.055) / 1.055).powf(2.4)
    }
}

/// WCAG relative luminance in `0.0..=1.0`.
pub(crate) fn luminance(c: [u8; 4]) -> f64 {
    0.2126 * linear_channel(c[0]) + 0.7152 * linear_channel(c[1]) + 0.0722 * linear_channel(c[2])
}

/// WCAG contrast ratio in `1.0..=21.0`.
pub(crate) fn contrast_ratio(a: [u8; 4], b: [u8; 4]) -> f64 {
    let (la, lb) = (luminance(a), luminance(b));
    (la.max(lb) + 0.05) / (la.min(lb) + 0.05)
}

/// Desaturate toward the color's own luminance — the ISA-101 "gray is
/// the canvas" correction. `keep` in `0..=1` blends toward gray
/// (`0` = full gray).
pub(crate) fn desaturate(c: [u8; 4], keep: f64) -> [u8; 4] {
    let l = luminance(c);
    // Back to sRGB from linear luminance.
    let srgb = if l <= 0.003_130_8 {
        12.92 * l
    } else {
        1.055 * l.powf(1.0 / 2.4) - 0.055
    };
    let gray = (srgb.clamp(0.0, 1.0) * 255.0).round() as u8;
    let blend = |ch: u8| {
        (f64::from(gray) + (f64::from(ch) - f64::from(gray)) * keep)
            .round()
            .clamp(0.0, 255.0) as u8
    };
    [blend(c[0]), blend(c[1]), blend(c[2]), c[3]]
}

/// The dominant background behind a point — the smallest opaque fill
/// containing `pt`, searching `node`'s subtree first (innermost paint
/// wins) then ancestors via lineage — text painted in a child scope
/// sits on a fill owned by its parent.
pub(crate) fn background_at(
    scene: &LintScene,
    node: &LintNode,
    pt: kurbo::Point,
) -> Option<[u8; 4]> {
    let containing = |n: &LintNode| {
        n.fills
            .iter()
            .filter(|f| f.rect.contains(pt) && f.color[3] > 200)
            .min_by(|a, b| {
                (a.rect.width() * a.rect.height())
                    .partial_cmp(&(b.rect.width() * b.rect.height()))
                    .unwrap_or(std::cmp::Ordering::Equal)
            })
            .map(|f| f.color)
    };
    if let Some(c) = node.walk().find_map(containing) {
        return Some(c);
    }
    let by_path = scene.by_path();
    let mut path = node.path.as_str();
    while let Some(i) = path.rfind('/') {
        path = &path[..i];
        if let Some(a) = by_path.get(path) {
            if let Some(c) = containing(a) {
                return Some(c);
            }
        }
    }
    None
}

/// Shared sibling-gap measurement — adjacent-pair gaps along the
/// dominant axis, honoring y/x-band overlap like `whitespace`.
/// Returns (gaps px, dominant axis is horizontal).
pub(crate) fn sibling_gaps(node: &LintNode) -> (Vec<f64>, bool) {
    let children: Vec<&LintNode> = node.children.iter().filter(|c| c.area() > 1.0).collect();
    let mut gaps = Vec::new();
    let mut by_x = children.clone();
    by_x.sort_by(|a, b| {
        a.bounds
            .x0
            .partial_cmp(&b.bounds.x0)
            .unwrap_or(std::cmp::Ordering::Equal)
    });
    for pair in by_x.windows(2) {
        let (a, b) = (pair[0], pair[1]);
        let overlap = a.bounds.y1.min(b.bounds.y1) - a.bounds.y0.max(b.bounds.y0);
        let shorter = (a.bounds.y1 - a.bounds.y0).min(b.bounds.y1 - b.bounds.y0);
        if overlap > 0.5 * shorter {
            gaps.push(b.bounds.x0 - a.bounds.x1);
        }
    }
    let x_gaps = gaps.len();
    let mut by_y = children;
    by_y.sort_by(|a, b| {
        a.bounds
            .y0
            .partial_cmp(&b.bounds.y0)
            .unwrap_or(std::cmp::Ordering::Equal)
    });
    for pair in by_y.windows(2) {
        let (a, b) = (pair[0], pair[1]);
        let overlap = a.bounds.x1.min(b.bounds.x1) - a.bounds.x0.max(b.bounds.x0);
        let shorter = (a.bounds.x1 - a.bounds.x0).min(b.bounds.x1 - b.bounds.x0);
        if overlap > 0.5 * shorter {
            gaps.push(b.bounds.y0 - a.bounds.y1);
        }
    }
    let horizontal = x_gaps >= gaps.len() - x_gaps;
    (gaps, horizontal)
}

/// True when `node` or any ancestor carries semantic marker `m` —
/// context rules (an alarm-red fill inside an `@alarm` panel is
/// legitimate).
pub(crate) fn marker_in_lineage(scene: &LintScene, node: &LintNode, m: &str) -> bool {
    let by_path = scene.by_path();
    let mut path = node.path.as_str();
    loop {
        let Some(n) = by_path.get(path) else {
            return false;
        };
        if n.has_marker(m) {
            return true;
        }
        match path.rfind('/') {
            Some(i) => path = &path[..i],
            None => return false,
        }
    }
}

#[cfg(test)]
mod rules_tests {
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

    // ---------- token-drift ----------

    #[test]
    fn token_drift_flags_off_palette_fill() {
        let mut list = app_list();
        list.push_scope(None, "Panel", Rect::new(10.0, 10.0, 110.0, 110.0));
        list.commands.push(PaintCommand::FillRect(
            Rect::new(10.0, 10.0, 110.0, 110.0),
            [255, 0, 0, 255],
        ));
        list.pop_scope();
        list.pop_scope();
        let scene = LintScene::from_paint_list(&list);
        let cfg = LintConfig::new()
            .with_rule_param("token-drift", "palette_entries", 2.0)
            .with_rule_param("token-drift", "palette_0", 0x141418 as f64)
            .with_rule_param("token-drift", "palette_1", 0xE0E0E0 as f64);
        let report = lint(&scene, &cfg);
        let hits = findings_for(&report, "token-drift");
        assert_eq!(hits.len(), 1, "expected one finding, got {hits:?}");
        assert!(hits[0].path.ends_with("Panel"));
        assert!(hits[0].message.contains("FF0000"));
    }

    #[test]
    fn token_drift_inert_or_on_palette_is_quiet() {
        let mut list = app_list();
        list.push_scope(None, "Panel", Rect::new(10.0, 10.0, 110.0, 110.0));
        list.commands.push(PaintCommand::FillRect(
            Rect::new(10.0, 10.0, 110.0, 110.0),
            [0x14, 0x14, 0x18, 255],
        ));
        list.pop_scope();
        list.pop_scope();
        let scene = LintScene::from_paint_list(&list);
        // No palette declared — the rule is registered but inert.
        let report = lint(&scene, &LintConfig::new());
        assert!(findings_for(&report, "token-drift").is_empty());
        // Palette declared, painted color is a token — still quiet.
        let cfg = LintConfig::new()
            .with_rule_param("token-drift", "palette_entries", 1.0)
            .with_rule_param("token-drift", "palette_0", 0x141418 as f64);
        let report = lint(&scene, &cfg);
        assert!(findings_for(&report, "token-drift").is_empty());
    }

    // ---------- edge-density ----------

    #[test]
    fn edge_density_flags_full_bleed_scope() {
        let mut list = PaintList::new();
        // No backdrop fill on App — its own coverage must stay low so
        // the inner Panel is the flagged node.
        list.push_scope(None, "App", Rect::new(0.0, 0.0, 800.0, 600.0));
        list.push_scope(None, "Panel", Rect::new(0.0, 0.0, 200.0, 200.0));
        list.commands.push(PaintCommand::FillRect(
            Rect::new(0.0, 0.0, 200.0, 200.0),
            [30, 30, 30, 255],
        ));
        list.pop_scope();
        list.pop_scope();
        let scene = LintScene::from_paint_list(&list);
        let report = lint(&scene, &LintConfig::new());
        let hits = findings_for(&report, "edge-density");
        assert_eq!(hits.len(), 1, "expected one finding, got {hits:?}");
        assert!(hits[0].path.ends_with("Panel"));
    }

    #[test]
    fn edge_density_flags_overstuffed_subtree() {
        let mut list = PaintList::new();
        list.push_scope(None, "App", Rect::new(0.0, 0.0, 800.0, 600.0));
        for i in 0..41 {
            list.push_scope(
                None,
                "Button",
                Rect::new(i as f64 * 10.0, 0.0, i as f64 * 10.0 + 8.0, 8.0),
            );
            list.pop_scope();
        }
        list.pop_scope();
        let scene = LintScene::from_paint_list(&list);
        let report = lint(&scene, &LintConfig::new());
        let hits = findings_for(&report, "edge-density");
        // Outermost offender only — App (42 descendants) reports once
        // and its children are not re-reported.
        assert_eq!(hits.len(), 1, "expected one finding, got {hits:?}");
        assert_eq!(hits[0].path, "App");
    }

    #[test]
    fn edge_density_quiet_on_normal_scope() {
        let mut list = PaintList::new();
        list.push_scope(None, "App", Rect::new(0.0, 0.0, 800.0, 600.0));
        list.push_scope(None, "Panel", Rect::new(0.0, 0.0, 200.0, 200.0));
        list.commands.push(PaintCommand::FillRect(
            Rect::new(0.0, 0.0, 50.0, 50.0),
            [30, 30, 30, 255],
        ));
        list.pop_scope();
        list.pop_scope();
        let scene = LintScene::from_paint_list(&list);
        let report = lint(&scene, &LintConfig::new());
        assert!(findings_for(&report, "edge-density").is_empty());
    }

    // ---------- color-only-info ----------

    #[test]
    fn color_only_info_flags_saturated_unlabeled_patch() {
        let mut list = app_list();
        list.push_scope(None, "Panel", Rect::new(10.0, 10.0, 30.0, 30.0));
        list.commands.push(PaintCommand::FillRect(
            Rect::new(10.0, 10.0, 30.0, 30.0),
            [255, 0, 0, 255],
        ));
        list.pop_scope();
        list.pop_scope();
        let scene = LintScene::from_paint_list(&list);
        let report = lint(&scene, &LintConfig::new());
        let hits = findings_for(&report, "color-only-info");
        assert_eq!(hits.len(), 1, "expected one finding, got {hits:?}");
        assert!(hits[0].path.ends_with("Panel"));
        assert!(hits[0].message.contains("consider"));
    }

    #[test]
    fn color_only_info_quiet_when_text_present() {
        let mut list = app_list();
        list.push_scope(None, "Panel", Rect::new(10.0, 10.0, 60.0, 30.0));
        list.commands.push(PaintCommand::FillRect(
            Rect::new(10.0, 10.0, 60.0, 30.0),
            [255, 0, 0, 255],
        ));
        list.commands.push(PaintCommand::DrawText(
            Point::new(12.0, 24.0),
            "ALARM".to_string(),
            11.0,
            [255, 255, 255, 255],
        ));
        list.pop_scope();
        list.pop_scope();
        let scene = LintScene::from_paint_list(&list);
        let report = lint(&scene, &LintConfig::new());
        assert!(findings_for(&report, "color-only-info").is_empty());
    }

    // ---------- progressive-disclosure ----------

    /// `App` (classified Chrome so the *container* is the surface
    /// under test) > Section > `n` Buttons.
    fn disclosure_scene(n: usize, extra: Option<&'static str>) -> LintScene {
        let mut list = PaintList::new();
        list.push_scope(None, "App", Rect::new(0.0, 0.0, 800.0, 600.0));
        list.push_scope(None, "Section", Rect::new(0.0, 0.0, 800.0, 560.0));
        for i in 0..n {
            list.push_scope(
                None,
                "Button",
                Rect::new(i as f64 * 60.0, 10.0, i as f64 * 60.0 + 50.0, 40.0),
            );
            list.pop_scope();
        }
        if let Some(name) = extra {
            list.push_scope(None, name, Rect::new(700.0, 10.0, 780.0, 40.0));
            list.pop_scope();
        }
        list.pop_scope();
        list.pop_scope();
        LintScene::from_paint_list(&list)
    }

    #[test]
    fn progressive_disclosure_flags_flat_overloaded_surface() {
        let scene = disclosure_scene(11, None);
        let cfg = LintConfig::new().with_classify("App", NodeKind::Chrome);
        let report = lint(&scene, &cfg);
        let hits = findings_for(&report, "progressive-disclosure");
        assert_eq!(hits.len(), 1, "expected one finding, got {hits:?}");
        assert!(hits[0].path.ends_with("Section"));
    }

    #[test]
    fn progressive_disclosure_quiet_with_affordance_or_few_controls() {
        // An overflow menu descendant satisfies the affordance check.
        let scene = disclosure_scene(11, Some("OverflowMenu"));
        let cfg = LintConfig::new().with_classify("App", NodeKind::Chrome);
        let report = lint(&scene, &cfg);
        assert!(findings_for(&report, "progressive-disclosure").is_empty());
        // Under the leaf budget — nothing to disclose progressively.
        let scene = disclosure_scene(6, None);
        let report = lint(&scene, &cfg);
        assert!(findings_for(&report, "progressive-disclosure").is_empty());
    }

    // ---------- whitespace ----------

    /// `App` > `Panel` > `n` siblings with `gap` px between them.
    fn siblings_scene(n: usize, gap: f64) -> LintScene {
        let mut list = PaintList::new();
        list.push_scope(None, "App", Rect::new(0.0, 0.0, 800.0, 600.0));
        list.push_scope(None, "Panel", Rect::new(0.0, 0.0, 800.0, 100.0));
        for i in 0..n {
            let x = i as f64 * (50.0 + gap);
            list.push_scope(None, "Label", Rect::new(x, 10.0, x + 50.0, 40.0));
            list.pop_scope();
        }
        list.pop_scope();
        list.pop_scope();
        LintScene::from_paint_list(&list)
    }

    #[test]
    fn whitespace_flags_cramped_siblings() {
        // 2px gaps at scale 1.0 — below the 4pt grouping threshold.
        let mut scene = siblings_scene(4, 2.0);
        scene.scale_factor = 1.0;
        let report = lint(&scene, &LintConfig::new());
        let hits = findings_for(&report, "whitespace");
        assert_eq!(hits.len(), 1, "expected one finding, got {hits:?}");
        assert!(hits[0].path.ends_with("Panel"));
    }

    #[test]
    fn whitespace_quiet_with_breathing_room() {
        let mut scene = siblings_scene(4, 24.0);
        scene.scale_factor = 1.0;
        let report = lint(&scene, &LintConfig::new());
        assert!(findings_for(&report, "whitespace").is_empty());
    }

    // ---------- union_area ----------

    #[test]
    fn union_area_disjoint_rects_sum() {
        let clip = Rect::new(0.0, 0.0, 100.0, 100.0);
        let a = union_area(
            [
                Rect::new(0.0, 0.0, 40.0, 40.0),
                Rect::new(60.0, 60.0, 100.0, 100.0),
            ],
            clip,
        );
        assert_eq!(a, 1600.0 + 1600.0);
    }

    #[test]
    fn union_area_overlap_counts_once() {
        let clip = Rect::new(0.0, 0.0, 100.0, 100.0);
        // Two 50×50 rects overlapping in a 10×50 band → 2500+2500-500.
        let a = union_area(
            [
                Rect::new(0.0, 0.0, 50.0, 50.0),
                Rect::new(40.0, 0.0, 90.0, 50.0),
            ],
            clip,
        );
        assert_eq!(a, 4500.0);
    }

    #[test]
    fn union_area_touching_rects_do_not_merge() {
        let clip = Rect::new(0.0, 0.0, 100.0, 100.0);
        // Shared edge at x=50 — union is still the simple sum.
        let a = union_area(
            [
                Rect::new(0.0, 0.0, 50.0, 50.0),
                Rect::new(50.0, 0.0, 100.0, 50.0),
            ],
            clip,
        );
        assert_eq!(a, 5000.0);
    }

    #[test]
    fn union_area_clips_and_drops_outside() {
        let clip = Rect::new(0.0, 0.0, 100.0, 100.0);
        let a = union_area(
            [
                Rect::new(50.0, 50.0, 150.0, 150.0),   // half outside → 2500
                Rect::new(200.0, 200.0, 300.0, 300.0), // fully outside → 0
            ],
            clip,
        );
        assert_eq!(a, 2500.0);
    }

    #[test]
    fn union_area_empty_and_degenerate() {
        let clip = Rect::new(0.0, 0.0, 100.0, 100.0);
        assert_eq!(union_area([], clip), 0.0);
        assert_eq!(union_area([Rect::new(10.0, 10.0, 10.0, 50.0)], clip), 0.0);
    }

    // ---------- is_data_display ----------

    #[test]
    fn data_display_segment_matching_kills_collisions() {
        for name in [
            "Chart",
            "TrendChart",
            "Gauge",
            "RadialGauge",
            "Sparkline",
            "DataTable",
            "Table",
            "StackLight",
            "LedMatrix",
            "LEDMatrix",
            "SplitFlap",
            "MapView",
            "MiniMap",
            "Gantt",
            "Treemap",
            "Timeline",
            "Fishbone",
            "Terminal",
            "Heatmap",
            "ParameterChart",
            // digit boundaries + prefixed compounds
            "Chart2D",
            "Gauge3",
            "MyStackLight",
            "ZoneDataGrid",
        ] {
            assert!(is_data_display(name), "{name} should be a display");
        }
        for name in [
            "Dialog",
            "AlertDialog",
            "Paragraph",
            "Photograph",
            "Timetable",
            "Roundtable",
            "Sticker",
            "ImmediateMode",
            "Thermometer",
            "Flowchart",
            "Label",
            "Button",
            "Panel",
        ] {
            assert!(!is_data_display(name), "{name} is not a display");
        }
    }

    #[test]
    fn data_display_strips_markers_and_paths() {
        assert!(is_data_display("widgets::TrendChart@level:2"));
        assert!(!is_data_display("widgets::Dialog@modal"));
    }
}
