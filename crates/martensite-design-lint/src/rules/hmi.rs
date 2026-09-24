//! ISA-101 / ISA-18.2 / NUREG-0700 / FAA HFDS high-performance HMI
//! rules — the industrial display discipline the dashboard domain
//! actually runs on.
//!
//! Most of these consume the semantic `@`-markers (`@level:N`,
//! `@alarm`, `@priority:N`, `@kpi`) — they only fire where a project
//! declares the domain meaning, so unmarked projects see nothing.

use std::collections::{BTreeMap, BTreeSet};

use crate::config::LintConfig;
use crate::fix::{FixOp, FixSafety, LintFix};
use crate::report::Finding;
use crate::rule::{param, Confidence, LintRule};
use crate::rules::{
    data_display_leaves, desaturate, interactive_leaves, is_alarm_red, marker_in_lineage,
    rgb_saturation, text_cell_area, text_leaf_share, union_area,
};
use crate::scene::{LintNode, LintScene};
use crate::severity::Severity;
use crate::standard::Standard;

/// This module's rules, appended to the registry by `all_rules()`.
pub(crate) fn rules() -> Vec<&'static dyn LintRule> {
    vec![
        &LevelPurity,
        &LevelSkip,
        &ReservedHue,
        &SaturatedAreaCap,
        &FloodCap,
        &PriorityMix,
        &KpiContext,
        &PackingDensity,
        &TextDensity,
        &SimultaneousChannels,
    ]
}

/// — `level-purity`: overview displays carrying detail controls.
///
/// ISA-101's L1 overview exists for situation awareness — KPIs,
/// trends, alarm access — and explicitly excludes equipment-level
/// controls, which live at L2/L3. Detail controls on an L1 display
/// re-create the dense-console problem the standard was written to
/// end. Requires `@level:1` on the surface.
struct LevelPurity;
impl LintRule for LevelPurity {
    fn id(&self) -> &'static str {
        "level-purity"
    }
    fn standards(&self) -> &'static [Standard] {
        &[Standard::Isa101, Standard::Nureg0700]
    }
    fn default_severity(&self) -> Severity {
        Severity::Warn
    }
    fn confidence(&self) -> Confidence {
        Confidence::Heuristic
    }
    fn title(&self) -> &'static str {
        "detail controls on an L1 overview display"
    }
    fn citation(&self) -> &'static str {
        "ISA-101.01 §display hierarchy: L1 = situation awareness only, \
         no equipment control; NUREG-0700 — density minimized for \
         critical-information displays"
    }
    fn check(&self, scene: &LintScene, cfg: &LintConfig) -> Vec<Finding> {
        let max = param(cfg, self.id(), "max_interactive", 0.0) as usize;
        let by_path = scene.by_path();
        let mut out = Vec::new();
        for n in scene.walk() {
            if n.display_level != Some(1) {
                continue;
            }
            // Outermost L1 only — a nested @level:1 inside @level:1
            // would double-report the same controls.
            let mut path = n.path.as_str();
            let nested_l1 = loop {
                match path.rfind('/') {
                    Some(i) => {
                        path = &path[..i];
                        if by_path.get(path).and_then(|a| a.display_level) == Some(1) {
                            break true;
                        }
                    }
                    None => break false,
                }
            };
            if nested_l1 {
                continue;
            }
            let leaves = interactive_leaves(n, cfg);
            if leaves.len() > max {
                out.push(
                    Finding::new(
                        "level-purity",
                        &n.path,
                        format!(
                            "L1 overview carries {} interactive control(s) — the \
                             overview level is for situation awareness; detail control \
                             belongs on L2/L3 displays",
                            leaves.len()
                        ),
                    )
                    .at(n.bounds),
                );
            }
        }
        out
    }
}

/// — `level-skip`: display hierarchy jumping a level.
///
/// An `@level:N` node whose nearest `@level` ancestor is more than
/// one level up strands the intermediate context — the user reaches
/// detail without the unit-level picture that explains it.
struct LevelSkip;
impl LintRule for LevelSkip {
    fn id(&self) -> &'static str {
        "level-skip"
    }
    fn standards(&self) -> &'static [Standard] {
        &[Standard::Isa101]
    }
    fn default_severity(&self) -> Severity {
        Severity::Warn
    }
    fn title(&self) -> &'static str {
        "display hierarchy skips an intermediate level"
    }
    fn citation(&self) -> &'static str {
        "ISA-101.01 — each level provides context for the next; skipping \
         strands the operator without the unit picture"
    }
    fn check(&self, scene: &LintScene, _cfg: &LintConfig) -> Vec<Finding> {
        let by_path = scene.by_path();
        let mut out = Vec::new();
        for n in scene.walk() {
            let Some(level) = n.display_level else {
                continue;
            };
            // Nearest ancestor with a declared level.
            let mut path = n.path.as_str();
            let mut ancestor_level = None;
            while let Some(i) = path.rfind('/') {
                path = &path[..i];
                if let Some(l) = by_path.get(path).and_then(|a| a.display_level) {
                    ancestor_level = Some(l);
                    break;
                }
            }
            if let Some(parent) = ancestor_level {
                if level > parent.saturating_add(1) {
                    out.push(
                        Finding::new(
                            "level-skip",
                            &n.path,
                            format!(
                                "L{level} display sits directly under L{parent} — the \
                                 L{} context the detail assumes is never shown",
                                parent + 1
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

/// — `reserved-hue`: the alarm channel used for non-alarm paint.
///
/// ISA-101's core contract: saturated red means *abnormal*. Every
/// decorative red erodes the channel — when a real alarm paints the
/// same hue, the operator has been trained to ignore it. `@alarm` in
/// the node's lineage marks legitimate use; a `[[allow]]` on the path
/// suppresses without recoloring.
struct ReservedHue;
impl LintRule for ReservedHue {
    fn id(&self) -> &'static str {
        "reserved-hue"
    }
    fn standards(&self) -> &'static [Standard] {
        &[Standard::Isa101]
    }
    fn default_severity(&self) -> Severity {
        Severity::Error
    }
    fn title(&self) -> &'static str {
        "alarm-red family used outside an @alarm context"
    }
    fn citation(&self) -> &'static str {
        "ISA-101/ASM Consortium — saturated red is the abnormal-state \
         channel; decorative use trains operators to ignore alarms"
    }
    fn check(&self, scene: &LintScene, _cfg: &LintConfig) -> Vec<Finding> {
        let mut out = Vec::new();
        for n in scene.walk() {
            if marker_in_lineage(scene, n, "alarm") {
                continue;
            }
            let mut ops = Vec::new();
            let mut worst: Option<[u8; 4]> = None;
            for f in &n.fills {
                if is_alarm_red(f.color) {
                    worst = Some(f.color);
                    ops.push(FixOp::RecolorFill {
                        path: n.path.clone(),
                        from: f.color,
                        to: desaturate(f.color, 0.3),
                    });
                }
            }
            for t in &n.texts {
                if is_alarm_red(t.color) {
                    worst = Some(t.color);
                    ops.push(FixOp::RecolorText {
                        path: n.path.clone(),
                        from: t.color,
                        to: desaturate(t.color, 0.3),
                    });
                }
            }
            if let Some(c) = worst {
                let muted = desaturate(c, 0.3);
                out.push(
                    Finding::new(
                        "reserved-hue",
                        &n.path,
                        format!(
                            "#{:02X}{:02X}{:02X} is in the alarm-red family but nothing \
                             in this lineage is marked @alarm — the channel means \
                             abnormal-or-nothing",
                            c[0], c[1], c[2]
                        ),
                    )
                    .at(n.bounds)
                    .with_fix(LintFix {
                        summary: format!(
                            "desaturate to #{:02X}{:02X}{:02X} — off the alarm channel",
                            muted[0], muted[1], muted[2]
                        ),
                        safety: FixSafety::Risky,
                        ops,
                    }),
                );
            }
        }
        out
    }
}

/// — `saturated-area-cap`: attention color covering too much canvas.
///
/// `color-budget` counts hues; this measures *coverage* — a single
/// saturated hue painting 40% of the screen is still a saturated
/// screen. ASM guidance keeps the canvas gray so abnormal color
/// pops; area is what the operator's retina actually sees.
struct SaturatedAreaCap;
impl LintRule for SaturatedAreaCap {
    fn id(&self) -> &'static str {
        "saturated-area-cap"
    }
    fn standards(&self) -> &'static [Standard] {
        &[Standard::Isa101]
    }
    fn default_severity(&self) -> Severity {
        Severity::Warn
    }
    fn confidence(&self) -> Confidence {
        Confidence::Heuristic
    }
    fn title(&self) -> &'static str {
        "saturated color covering too much of the display"
    }
    fn citation(&self) -> &'static str {
        "ISA-101/ASM — gray is the canvas; saturation reserved for \
         abnormal reads as a share of painted area, not a count"
    }
    fn check(&self, scene: &LintScene, cfg: &LintConfig) -> Vec<Finding> {
        let max_pct = param(cfg, self.id(), "max_pct", 15.0);
        let min_sat = param(cfg, self.id(), "min_sat", 0.5);
        let min_area = param(cfg, self.id(), "min_surface_pt", 20_000.0)
            * f64::from(scene.scale_factor).powi(2);
        let mut out = Vec::new();
        for n in scene.walk().filter(|n| n.area() >= min_area) {
            let area = n.area();
            // Subtree coverage — a canvas painted saturated by ten
            // child scopes is a saturated canvas.
            let sat_area: f64 = n
                .walk()
                .flat_map(|d| d.fills.iter())
                .filter(|f| rgb_saturation(f.color) >= min_sat && f.color[3] > 200)
                .map(|f| {
                    let r = f.rect.intersect(n.bounds);
                    r.width().max(0.0) * r.height().max(0.0)
                })
                .sum();
            let pct = sat_area / area * 100.0;
            if pct > max_pct {
                out.push(
                    Finding::new(
                        "saturated-area-cap",
                        &n.path,
                        format!(
                            "{pct:.0}% of this surface is painted saturated — above the \
                             {max_pct:.0}% cap where abnormal-state color still pops \
                             against a calm canvas"
                        ),
                    )
                    .at(n.bounds),
                );
            }
        }
        out
    }
}

/// — `flood-cap`: simultaneous alert-colored elements on one display.
///
/// ISA-18.2's flood threshold is ~10 alarms in 10 minutes; the
/// design-time analog is simultaneous alert-colored elements — each
/// one competes for the same attentional channel, and past a handful
/// they all lose.
struct FloodCap;
impl LintRule for FloodCap {
    fn id(&self) -> &'static str {
        "flood-cap"
    }
    fn standards(&self) -> &'static [Standard] {
        &[Standard::Isa182]
    }
    fn default_severity(&self) -> Severity {
        Severity::Warn
    }
    fn confidence(&self) -> Confidence {
        Confidence::Heuristic
    }
    fn title(&self) -> &'static str {
        "simultaneous alert-colored elements exceed the flood cap"
    }
    fn citation(&self) -> &'static str {
        "ISA-18.2 alarm-flood analog — attention is a single channel; \
         N alerts at once is zero alerts noticed"
    }
    fn check(&self, scene: &LintScene, cfg: &LintConfig) -> Vec<Finding> {
        let max = param(cfg, self.id(), "max_simultaneous", 5.0) as usize;
        // Count alert-painted descendants per node; report only the
        // innermost node over the cap — the same alert set flagged at
        // every ancestor is one defect, not N.
        let mut flagged: Vec<&LintNode> = Vec::new();
        for n in scene.walk() {
            let alerts = n
                .walk()
                .filter(|d| {
                    d.fills.iter().any(|f| is_alarm_red(f.color))
                        || d.texts.iter().any(|t| is_alarm_red(t.color))
                })
                .count();
            if alerts > max {
                flagged.push(n);
            }
        }
        let mut out = Vec::new();
        for (i, n) in flagged.iter().enumerate() {
            // Skip n when a flagged descendant already reports the set.
            let covered = flagged
                .iter()
                .enumerate()
                .any(|(j, m)| j != i && m.path.starts_with(&format!("{}/", n.path)));
            if covered {
                continue;
            }
            out.push(
                Finding::new(
                    "flood-cap",
                    &n.path,
                    format!(
                        "elements painted in the alert channel at once exceed the \
                         ~{max}-simultaneous cap — operators stop distinguishing them"
                    ),
                )
                .at(n.bounds),
            );
        }
        out
    }
}

/// — `priority-mix`: alarm priority distribution off the 80/15/5 norm.
///
/// ISA-18.2's rationalized distribution puts most alarms at low
/// priority; a display where everything is high-priority has
/// re-created alarm inflation at the design level. Consumes
/// `@priority:N` markers (1=low … 3+=high).
struct PriorityMix;
impl LintRule for PriorityMix {
    fn id(&self) -> &'static str {
        "priority-mix"
    }
    fn standards(&self) -> &'static [Standard] {
        &[Standard::Isa182]
    }
    fn default_severity(&self) -> Severity {
        Severity::Info
    }
    fn confidence(&self) -> Confidence {
        Confidence::Heuristic
    }
    fn title(&self) -> &'static str {
        "alarm priority distribution off the 80/15/5 norm"
    }
    fn citation(&self) -> &'static str {
        "ISA-18.2 rationalization — ~80% low / 15% medium / 5% high; \
         uniform priority is no priority"
    }
    fn check(&self, scene: &LintScene, cfg: &LintConfig) -> Vec<Finding> {
        let max_high_pct = param(cfg, self.id(), "max_high_pct", 10.0);
        let min_alarms = param(cfg, self.id(), "min_alarms", 4.0) as usize;
        let mut counts: BTreeMap<u8, Vec<&LintNode>> = BTreeMap::new();
        for n in scene.walk() {
            if let Some(v) = n
                .marker_value("priority")
                .and_then(|v| v.parse::<u8>().ok())
            {
                counts.entry(v.clamp(1, 3)).or_default().push(n);
            }
        }
        let total: usize = counts.values().map(Vec::len).sum();
        let mut out = Vec::new();
        if total >= min_alarms {
            if let Some(highs) = counts.get(&3) {
                let pct = highs.len() as f64 / total as f64 * 100.0;
                if pct > max_high_pct {
                    out.push(
                        Finding::new(
                            "priority-mix",
                            &highs[0].path,
                            format!(
                                "{}/{total} alarms declared high priority ({pct:.0}%) — \
                                 ISA-18.2's norm keeps ~5% high; when everything screams, \
                                 nothing is urgent",
                                highs.len()
                            ),
                        )
                        .at(highs[0].bounds),
                    );
                }
            }
        }
        out
    }
}

/// — `kpi-context`: a headline number without its comparison context.
///
/// A bare KPI value ("72.3") answers a question nobody asked — Few's
/// dashboard canon requires context: trend, target, or comparison.
/// Consumes `@kpi` markers; a sibling whose name matches
/// trend|spark|target|compare|chart counts as context.
struct KpiContext;
impl LintRule for KpiContext {
    fn id(&self) -> &'static str {
        "kpi-context"
    }
    fn standards(&self) -> &'static [Standard] {
        &[Standard::Isa101, Standard::InfoDesign]
    }
    fn default_severity(&self) -> Severity {
        Severity::Warn
    }
    fn confidence(&self) -> Confidence {
        Confidence::Heuristic
    }
    fn title(&self) -> &'static str {
        "KPI value without trend or comparison context"
    }
    fn citation(&self) -> &'static str {
        "Few, Information Dashboard Design — a number without context \
         is data, not information"
    }
    fn check(&self, scene: &LintScene, _cfg: &LintConfig) -> Vec<Finding> {
        // "vs" excluded — substring matches like `PrevSibling` fire it.
        const CONTEXT: &[&str] = &["trend", "spark", "target", "compare", "chart"];
        let mut out = Vec::new();
        // Dedupe by path+bounds — same-name siblings share a path.
        let mut reported: BTreeSet<(String, i64, i64)> = BTreeSet::new();
        for n in scene.walk() {
            if !n.has_marker("kpi") {
                continue;
            }
            // Context = a sibling (or self/child) carrying a
            // context-bearing name.
            let mut has_context = n.walk().any(|d| {
                !std::ptr::eq(d, n)
                    && CONTEXT
                        .iter()
                        .any(|m| d.name.to_ascii_lowercase().contains(m))
            });
            if !has_context {
                if let Some(i) = n.path.rfind('/') {
                    if let Some(parent) = scene.by_path().get(&n.path[..i]) {
                        has_context = parent.children.iter().any(|s| {
                            !std::ptr::eq(s, n)
                                && s.walk().any(|d| {
                                    CONTEXT
                                        .iter()
                                        .any(|m| d.name.to_ascii_lowercase().contains(m))
                                })
                        });
                    }
                }
            }
            if !has_context
                && reported.insert((n.path.clone(), n.bounds.x0 as i64, n.bounds.y0 as i64))
            {
                out.push(
                    Finding::new(
                        "kpi-context",
                        &n.path,
                        "this KPI paints a value with no trend, target, or comparison \
                         sibling — operators need to know if the number is *good*"
                            .to_string(),
                    )
                    .at(n.bounds),
                );
            }
        }
        out
    }
}

/// — `packing-density`: element footprint share of a display scope.
///
/// NUREG-0700 is the most quantified HSI standard in print — written
/// for nuclear control rooms, where a crowded display is a safety
/// finding, not a taste complaint. Its packing-density clause caps the
/// used fraction of a display at **50%**, tightens to **25%** for
/// displays consisting largely of alphanumeric information, and says
/// density should be *minimized* for displays of critical information
/// — here, any surface inside an `@level:1` lineage gets the
/// `max_critical` cap.
///
/// Measured as the union of leaf-scope bounds over the surface's own
/// bounds — element real estate, not painted pixels, so a full-bleed
/// background fill doesn't inflate the ratio. Reports the outermost
/// offender only.
struct PackingDensity;
impl LintRule for PackingDensity {
    fn id(&self) -> &'static str {
        "packing-density"
    }
    fn standards(&self) -> &'static [Standard] {
        &[Standard::Nureg0700]
    }
    fn default_severity(&self) -> Severity {
        Severity::Warn
    }
    fn confidence(&self) -> Confidence {
        Confidence::Heuristic
    }
    fn title(&self) -> &'static str {
        "element footprint share of a display (NUREG-0700 packing density)"
    }
    fn citation(&self) -> &'static str {
        "NUREG-0700 §1.1 information display density: ≤50% packing, \
         ≤25% alphanumeric-dominant, minimized for critical info; \
         ISO 9241-125 §5.1.4"
    }
    fn check(&self, scene: &LintScene, cfg: &LintConfig) -> Vec<Finding> {
        let max = param(cfg, self.id(), "max", 0.50);
        let max_alnum = param(cfg, self.id(), "max_alphanumeric", 0.25);
        let max_critical = param(cfg, self.id(), "max_critical", 0.35);
        let alnum_share = param(cfg, self.id(), "alnum_share", 0.5);
        let min_px = param(cfg, self.id(), "min_surface_pt", 20_000.0)
            * f64::from(scene.scale_factor).powi(2);
        let mut out = Vec::new();

        fn visit(
            node: &LintNode,
            caps: (f64, f64, f64, f64),
            min_px: f64,
            // `@level:1` inherited down the real ancestor chain — the
            // critical-overview lineage NUREG-0700 wants sparsest.
            in_l1: bool,
            out: &mut Vec<Finding>,
        ) {
            let (max, max_alnum, max_critical, alnum_share) = caps;
            let in_l1 = in_l1 || node.display_level == Some(1);
            let mut flagged = false;
            if node.children.len() >= 2 && node.area() >= min_px {
                let leaves = node
                    .walk()
                    .skip(1)
                    .filter(|d| d.children.is_empty())
                    .map(|d| d.bounds);
                let density = union_area(leaves, node.bounds) / node.area().max(1.0);
                // Tightest applicable cap wins — an L1 log display is
                // both critical-information AND alphanumeric.
                let mut cap = max;
                let mut tags = Vec::new();
                if text_leaf_share(node) >= alnum_share {
                    cap = cap.min(max_alnum);
                    tags.push("alphanumeric-dominant");
                }
                if in_l1 {
                    cap = cap.min(max_critical);
                    tags.push("critical-information");
                }
                let why = if tags.is_empty() {
                    "display".to_string()
                } else {
                    format!("{} display", tags.join(", "))
                };
                if density > cap {
                    out.push(
                        Finding::new(
                            "packing-density",
                            &node.path,
                            format!(
                                "elements occupy {:.0}% of this {why} (max {:.0}%) — \
                                 split it into separate displays, move detail to \
                                 on-demand surfaces, or consolidate into fewer \
                                 integrated elements",
                                density * 100.0,
                                cap * 100.0,
                            ),
                        )
                        .at(node.bounds),
                    );
                    flagged = true;
                }
            }
            if flagged {
                return; // outermost offender — don't cascade into it
            }
            for c in &node.children {
                visit(c, caps, min_px, in_l1, out);
            }
        }
        for r in &scene.roots {
            visit(
                r,
                (max, max_alnum, max_critical, alnum_share),
                min_px,
                false,
                &mut out,
            );
        }
        out
    }
}

/// — `text-density`: character-cell coverage of a text display.
///
/// FAA-CT-96-1 / HFDS screen-design guidance: on a text display, the
/// character-to-blank ratio should not exceed **60%** — beyond that
/// the eye can't take a line's worth of structure per fixation and
/// scan cost climbs steeply. Measured as the sum of text-run cells
/// (advance width × font size) over the surface's area, on surfaces
/// where alphanumeric content is dominant (leaf-area share ≥ 0.5 — a
/// captioned chart page isn't a text display). Reports the outermost
/// offender only.
struct TextDensity;
impl LintRule for TextDensity {
    fn id(&self) -> &'static str {
        "text-density"
    }
    fn standards(&self) -> &'static [Standard] {
        &[Standard::FaaHfds]
    }
    fn default_severity(&self) -> Severity {
        Severity::Warn
    }
    fn confidence(&self) -> Confidence {
        Confidence::Heuristic
    }
    fn title(&self) -> &'static str {
        "character coverage of a text-dominant display"
    }
    fn citation(&self) -> &'static str {
        "FAA HFDS/FAA-CT-96-1 §8.1.1.3: text-display character:blank ratio ≤60%; \
         ISO 9241-125 §5.1.4"
    }
    fn check(&self, scene: &LintScene, cfg: &LintConfig) -> Vec<Finding> {
        let max = param(cfg, self.id(), "max", 0.60);
        let alnum_share = param(cfg, self.id(), "alnum_share", 0.5);
        let min_px = param(cfg, self.id(), "min_surface_pt", 20_000.0)
            * f64::from(scene.scale_factor).powi(2);
        let mut out = Vec::new();

        fn visit(node: &LintNode, max: f64, alnum_share: f64, min_px: f64, out: &mut Vec<Finding>) {
            let mut flagged = false;
            if node.children.len() >= 2
                && node.area() >= min_px
                && text_leaf_share(node) >= alnum_share
            {
                // Character-cell coverage over ALL descendants — the
                // numerator includes interior scopes' own text runs,
                // not just leaf text (a wall of text painted directly
                // on a container still counts).
                let text_area: f64 = node.walk().map(text_cell_area).sum();
                let coverage = text_area / node.area().max(1.0);
                if coverage > max {
                    out.push(
                        Finding::new(
                            "text-density",
                            &node.path,
                            format!(
                                "text fills {:.0}% of this display's area (max {:.0}%) — \
                                 past ~60% character coverage each fixation carries no \
                                 grouping cues; paginate, filter, or summarize",
                                coverage * 100.0,
                                max * 100.0,
                            ),
                        )
                        .at(node.bounds),
                    );
                    flagged = true;
                }
            }
            if flagged {
                return;
            }
            for c in &node.children {
                visit(c, max, alnum_share, min_px, out);
            }
        }
        for r in &scene.roots {
            visit(r, max, alnum_share, min_px, &mut out);
        }
        out
    }
}

/// — `simultaneous-channels`: concurrent live data displays.
///
/// The simultaneity norm: FAA HFDS says a screen should present only
/// the information essential *at a given time*; ISA-101's display
/// hierarchy exists to keep simultaneous information matched to the
/// operator's span. Counts topmost data-display descendants (charts,
/// gauges, sparklines, tables, media — one integrated display = one
/// channel, so a DataTable's columns don't inflate the count) and
/// reports the innermost scope still over budget — the crowded
/// surface, not the window containing it.
struct SimultaneousChannels;
impl LintRule for SimultaneousChannels {
    fn id(&self) -> &'static str {
        "simultaneous-channels"
    }
    fn standards(&self) -> &'static [Standard] {
        &[Standard::FaaHfds, Standard::Isa101]
    }
    fn default_severity(&self) -> Severity {
        Severity::Warn
    }
    fn confidence(&self) -> Confidence {
        Confidence::Heuristic
    }
    fn title(&self) -> &'static str {
        "concurrent live data displays competing for attention"
    }
    fn citation(&self) -> &'static str {
        "FAA HFDS/FAA-CT-96-1 §8.1.1.2 — only information essential at \
         a given time; ISA-101 display hierarchy; Miller 7±2 channels"
    }
    fn check(&self, scene: &LintScene, cfg: &LintConfig) -> Vec<Finding> {
        let max = param(cfg, self.id(), "max", 7.0) as usize;
        let min_px = param(cfg, self.id(), "min_surface_pt", 20_000.0)
            * f64::from(scene.scale_factor).powi(2);
        let mut out = Vec::new();
        fn visit(node: &LintNode, max: usize, min_px: f64, out: &mut Vec<Finding>) {
            let count = if node.area() >= min_px {
                data_display_leaves(node).len()
            } else {
                0
            };
            // Report the deepest overloaded group, not the window —
            // same anchor logic as choice-count. Walk all descendants:
            // an overflowing grandchild can exceed min_px even when
            // its parent doesn't.
            let deeper_over = count > max
                && node
                    .walk()
                    .skip(1)
                    .any(|d| d.area() >= min_px && data_display_leaves(d).len() > max);
            if count > max && !deeper_over {
                out.push(
                    Finding::new(
                        "simultaneous-channels",
                        &node.path,
                        format!(
                            "{count} live data displays compete in one surface (max {max}) — \
                             attention is a single channel; move secondary displays behind \
                             tabs, pages, or on-demand overlays",
                        ),
                    )
                    .at(node.bounds),
                );
            }
            for c in &node.children {
                visit(c, max, min_px, out);
            }
        }
        for r in &scene.roots {
            visit(r, max, min_px, &mut out);
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

    // ---------- level-purity / level-skip ----------

    #[test]
    fn level_purity_flags_controls_on_l1() {
        let mut list = app_list();
        list.push_scope(None, "Overview@level:1", Rect::new(0.0, 0.0, 800.0, 600.0));
        list.push_scope(None, "Button", Rect::new(10.0, 10.0, 100.0, 40.0));
        list.pop_scope();
        list.pop_scope();
        list.pop_scope();
        let scene = LintScene::from_paint_list(&list);
        let report = lint(&scene, &LintConfig::new());
        assert!(!findings_for(&report, "level-purity").is_empty());
    }

    #[test]
    fn level_purity_quiet_on_clean_l1() {
        let mut list = app_list();
        list.push_scope(None, "Overview@level:1", Rect::new(0.0, 0.0, 800.0, 600.0));
        list.push_scope(None, "Label", Rect::new(10.0, 10.0, 100.0, 40.0));
        list.pop_scope();
        list.pop_scope();
        list.pop_scope();
        let scene = LintScene::from_paint_list(&list);
        let report = lint(&scene, &LintConfig::new());
        assert!(findings_for(&report, "level-purity").is_empty());
    }

    #[test]
    fn level_skip_flags_l3_under_l1() {
        let mut list = app_list();
        list.push_scope(None, "Overview@level:1", Rect::new(0.0, 0.0, 800.0, 600.0));
        list.push_scope(None, "Detail@level:3", Rect::new(10.0, 10.0, 400.0, 300.0));
        list.pop_scope();
        list.pop_scope();
        list.pop_scope();
        let scene = LintScene::from_paint_list(&list);
        let report = lint(&scene, &LintConfig::new());
        assert!(!findings_for(&report, "level-skip").is_empty());
    }

    #[test]
    fn level_skip_quiet_on_l2_under_l1() {
        let mut list = app_list();
        list.push_scope(None, "Overview@level:1", Rect::new(0.0, 0.0, 800.0, 600.0));
        list.push_scope(None, "Unit@level:2", Rect::new(10.0, 10.0, 400.0, 300.0));
        list.pop_scope();
        list.pop_scope();
        list.pop_scope();
        let scene = LintScene::from_paint_list(&list);
        let report = lint(&scene, &LintConfig::new());
        assert!(findings_for(&report, "level-skip").is_empty());
    }

    #[test]
    fn level_purity_reports_outermost_l1_only() {
        // A nested L1 inside an L1 would double-report the same
        // control — only the outermost surface owns the finding.
        let mut list = app_list();
        list.push_scope(None, "Overview@level:1", Rect::new(0.0, 0.0, 800.0, 600.0));
        list.push_scope(None, "Sub@level:1", Rect::new(0.0, 0.0, 400.0, 300.0));
        list.push_scope(None, "Button", Rect::new(10.0, 10.0, 100.0, 40.0));
        list.pop_scope();
        list.pop_scope();
        list.pop_scope();
        list.pop_scope();
        let scene = LintScene::from_paint_list(&list);
        let report = lint(&scene, &LintConfig::new());
        let hits = findings_for(&report, "level-purity");
        assert_eq!(hits.len(), 1, "nested L1 double-reported: {hits:?}");
        assert!(hits[0].path.ends_with("Overview"));
    }

    // ---------- reserved-hue ----------

    const ALARM_RED: [u8; 4] = [220, 40, 40, 255];

    #[test]
    fn reserved_hue_flags_unmarked_red() {
        let mut list = app_list();
        list.push_scope(None, "Banner", Rect::new(10.0, 10.0, 400.0, 60.0));
        list.commands.push(PaintCommand::FillRect(
            Rect::new(10.0, 10.0, 400.0, 60.0),
            ALARM_RED,
        ));
        list.pop_scope();
        list.pop_scope();
        let scene = LintScene::from_paint_list(&list);
        let report = lint(&scene, &LintConfig::new());
        let hits = findings_for(&report, "reserved-hue");
        assert_eq!(hits.len(), 1, "expected one finding, got {hits:?}");
        assert!(hits[0].fix.is_some());
    }

    #[test]
    fn reserved_hue_quiet_inside_alarm_lineage() {
        let mut list = app_list();
        list.push_scope(None, "AlarmPanel@alarm", Rect::new(0.0, 0.0, 800.0, 300.0));
        list.push_scope(None, "Banner", Rect::new(10.0, 10.0, 400.0, 60.0));
        list.commands.push(PaintCommand::FillRect(
            Rect::new(10.0, 10.0, 400.0, 60.0),
            ALARM_RED,
        ));
        list.pop_scope();
        list.pop_scope();
        list.pop_scope();
        let scene = LintScene::from_paint_list(&list);
        let report = lint(&scene, &LintConfig::new());
        assert!(findings_for(&report, "reserved-hue").is_empty());
    }

    // ---------- saturated-area-cap ----------

    #[test]
    fn saturated_area_flags_flooded_surface() {
        let mut list = app_list();
        list.push_scope(None, "Panel", Rect::new(0.0, 0.0, 400.0, 200.0));
        // Saturated green covering ~87% of the panel.
        list.commands.push(PaintCommand::FillRect(
            Rect::new(0.0, 0.0, 400.0, 175.0),
            [30, 200, 80, 255],
        ));
        list.pop_scope();
        list.pop_scope();
        let scene = LintScene::from_paint_list(&list);
        let report = lint(&scene, &LintConfig::new());
        assert!(!findings_for(&report, "saturated-area-cap").is_empty());
    }

    #[test]
    fn saturated_area_quiet_on_calm_canvas() {
        let mut list = app_list();
        list.push_scope(None, "Panel", Rect::new(0.0, 0.0, 400.0, 200.0));
        list.commands.push(PaintCommand::FillRect(
            Rect::new(0.0, 0.0, 40.0, 20.0),
            [30, 200, 80, 255],
        ));
        list.pop_scope();
        list.pop_scope();
        let scene = LintScene::from_paint_list(&list);
        let report = lint(&scene, &LintConfig::new());
        assert!(findings_for(&report, "saturated-area-cap").is_empty());
    }

    #[test]
    fn saturated_area_aggregates_subtree_fills() {
        // The surface paints nothing itself; its children paint
        // saturated fills covering ~50% of the canvas — a saturated
        // canvas is saturated no matter which scope holds the paint.
        let mut list = app_list();
        list.push_scope(None, "Panel", Rect::new(0.0, 0.0, 400.0, 200.0));
        for i in 0..2 {
            let x = i as f64 * 200.0;
            list.push_scope(None, "Widget", Rect::new(x, 0.0, x + 200.0, 100.0));
            list.commands.push(PaintCommand::FillRect(
                Rect::new(x, 0.0, x + 200.0, 100.0),
                [30, 200, 80, 255],
            ));
            list.pop_scope();
        }
        list.pop_scope();
        list.pop_scope();
        let scene = LintScene::from_paint_list(&list);
        let report = lint(&scene, &LintConfig::new());
        assert!(
            !findings_for(&report, "saturated-area-cap").is_empty(),
            "subtree saturated coverage not measured"
        );
    }

    #[test]
    fn saturated_area_ignores_small_widgets() {
        // A single saturated badge is attention paint, not a flooded
        // canvas — below the surface-area gate it must not flag.
        let mut list = app_list();
        list.push_scope(None, "Badge", Rect::new(0.0, 0.0, 40.0, 20.0));
        list.commands.push(PaintCommand::FillRect(
            Rect::new(0.0, 0.0, 40.0, 20.0),
            [30, 200, 80, 255],
        ));
        list.pop_scope();
        list.pop_scope();
        let scene = LintScene::from_paint_list(&list);
        let report = lint(&scene, &LintConfig::new());
        assert!(findings_for(&report, "saturated-area-cap").is_empty());
    }

    // ---------- flood-cap ----------

    #[test]
    fn flood_cap_flags_simultaneous_alerts() {
        let mut list = app_list();
        list.push_scope(None, "Panel", Rect::new(0.0, 0.0, 800.0, 200.0));
        for i in 0..6 {
            let x = i as f64 * 120.0;
            list.push_scope(None, "Badge", Rect::new(x, 10.0, x + 100.0, 50.0));
            list.commands.push(PaintCommand::FillRect(
                Rect::new(x, 10.0, x + 100.0, 50.0),
                ALARM_RED,
            ));
            list.pop_scope();
        }
        list.pop_scope();
        list.pop_scope();
        let scene = LintScene::from_paint_list(&list);
        let report = lint(&scene, &LintConfig::new());
        assert!(!findings_for(&report, "flood-cap").is_empty());
    }

    #[test]
    fn flood_cap_quiet_under_cap() {
        let mut list = app_list();
        list.push_scope(None, "Panel", Rect::new(0.0, 0.0, 800.0, 200.0));
        for i in 0..3 {
            let x = i as f64 * 120.0;
            list.push_scope(None, "Badge", Rect::new(x, 10.0, x + 100.0, 50.0));
            list.commands.push(PaintCommand::FillRect(
                Rect::new(x, 10.0, x + 100.0, 50.0),
                ALARM_RED,
            ));
            list.pop_scope();
        }
        list.pop_scope();
        list.pop_scope();
        let scene = LintScene::from_paint_list(&list);
        let report = lint(&scene, &LintConfig::new());
        assert!(findings_for(&report, "flood-cap").is_empty());
    }

    #[test]
    fn flood_cap_reports_innermost_only() {
        // Section inside Panel, both over the cap on the same alert
        // set — one defect, reported at the innermost surface.
        let mut list = app_list();
        list.push_scope(None, "Panel", Rect::new(0.0, 0.0, 800.0, 200.0));
        list.push_scope(None, "Section", Rect::new(0.0, 0.0, 800.0, 150.0));
        for i in 0..6 {
            let x = i as f64 * 120.0;
            list.push_scope(None, "Badge", Rect::new(x, 10.0, x + 100.0, 50.0));
            list.commands.push(PaintCommand::FillRect(
                Rect::new(x, 10.0, x + 100.0, 50.0),
                ALARM_RED,
            ));
            list.pop_scope();
        }
        list.pop_scope();
        list.pop_scope();
        list.pop_scope();
        let scene = LintScene::from_paint_list(&list);
        let report = lint(&scene, &LintConfig::new());
        let hits = findings_for(&report, "flood-cap");
        assert_eq!(hits.len(), 1, "expected innermost-only: {hits:?}");
        assert!(hits[0].path.ends_with("Section"));
    }

    // ---------- priority-mix ----------

    #[test]
    fn priority_mix_flags_inflation() {
        let mut list = app_list();
        list.push_scope(None, "Panel", Rect::new(0.0, 0.0, 800.0, 200.0));
        for i in 0..4 {
            let x = i as f64 * 120.0;
            list.push_scope(
                None,
                "Alarm@priority:3",
                Rect::new(x, 10.0, x + 100.0, 50.0),
            );
            list.pop_scope();
        }
        list.pop_scope();
        list.pop_scope();
        let scene = LintScene::from_paint_list(&list);
        let report = lint(&scene, &LintConfig::new());
        assert!(!findings_for(&report, "priority-mix").is_empty());
    }

    #[test]
    fn priority_mix_quiet_on_rationalized_set() {
        let mut list = app_list();
        list.push_scope(None, "Panel", Rect::new(0.0, 0.0, 800.0, 200.0));
        for i in 0..8 {
            let x = i as f64 * 90.0;
            list.push_scope(None, "Alarm@priority:1", Rect::new(x, 10.0, x + 80.0, 50.0));
            list.pop_scope();
        }
        list.pop_scope();
        list.pop_scope();
        let scene = LintScene::from_paint_list(&list);
        let report = lint(&scene, &LintConfig::new());
        assert!(findings_for(&report, "priority-mix").is_empty());
    }

    // ---------- kpi-context ----------

    #[test]
    fn kpi_context_flags_bare_value() {
        let mut list = app_list();
        list.push_scope(None, "Panel", Rect::new(0.0, 0.0, 800.0, 200.0));
        list.push_scope(None, "KpiTile@kpi", Rect::new(10.0, 10.0, 200.0, 100.0));
        list.commands.push(PaintCommand::DrawText(
            Point::new(30.0, 60.0),
            "72.3".to_string(),
            28.0,
            [230, 230, 230, 255],
        ));
        list.pop_scope();
        list.pop_scope();
        list.pop_scope();
        let scene = LintScene::from_paint_list(&list);
        let report = lint(&scene, &LintConfig::new());
        assert!(!findings_for(&report, "kpi-context").is_empty());
    }

    #[test]
    fn kpi_context_quiet_with_trend_sibling() {
        let mut list = app_list();
        list.push_scope(None, "Panel", Rect::new(0.0, 0.0, 800.0, 200.0));
        list.push_scope(None, "KpiTile@kpi", Rect::new(10.0, 10.0, 200.0, 100.0));
        list.commands.push(PaintCommand::DrawText(
            Point::new(30.0, 60.0),
            "72.3".to_string(),
            28.0,
            [230, 230, 230, 255],
        ));
        list.pop_scope();
        list.push_scope(None, "TrendChart", Rect::new(220.0, 10.0, 400.0, 100.0));
        list.pop_scope();
        list.pop_scope();
        list.pop_scope();
        let scene = LintScene::from_paint_list(&list);
        let report = lint(&scene, &LintConfig::new());
        assert!(findings_for(&report, "kpi-context").is_empty());
    }

    // ---------- packing-density ----------

    /// A Panel (800×300 = 240k px² ≥ min_surface) holding `n` leaf
    /// widgets, each `w`×`h` on a grid starting at (0,0).
    fn packed_list(n: usize, w: f64, h: f64) -> martensite_core::PaintList {
        let mut list = app_list();
        list.push_scope(None, "Panel", Rect::new(0.0, 0.0, 800.0, 300.0));
        let cols = (800.0 / w).floor().max(1.0) as usize;
        for i in 0..n {
            let x = (i % cols) as f64 * w;
            let y = (i / cols) as f64 * h;
            list.push_scope(None, "Label", Rect::new(x, y, x + w, y + h));
            list.pop_scope();
        }
        list.pop_scope();
        list.pop_scope();
        list
    }

    #[test]
    fn packing_density_flags_full_display() {
        // 4×200×300 leaves tile 800×300 → 100% coverage > 50% cap.
        let scene = LintScene::from_paint_list(&packed_list(4, 200.0, 300.0));
        let report = lint(&scene, &LintConfig::new());
        assert!(!findings_for(&report, "packing-density").is_empty());
    }

    #[test]
    fn packing_density_quiet_on_sparse_display() {
        // 2×100×75 leaves in 800×300 → ~6% coverage, under every cap.
        let scene = LintScene::from_paint_list(&packed_list(2, 100.0, 75.0));
        let report = lint(&scene, &LintConfig::new());
        assert!(findings_for(&report, "packing-density").is_empty());
    }

    #[test]
    fn packing_density_uses_alphanumeric_cap() {
        // 35% coverage of a text-dominant display → over the 25%
        // alphanumeric cap even though it's under the 50% general cap.
        // Each Label carries enough runs that text covers ≥5% of its
        // bounds — the leaf counts as an alphanumeric element.
        let mut list = app_list();
        list.push_scope(None, "LogPanel", Rect::new(0.0, 0.0, 800.0, 300.0));
        for i in 0..4 {
            let x = i as f64 * 175.0;
            list.push_scope(None, "Label", Rect::new(x, 0.0, x + 175.0, 120.0));
            for row in 0..8 {
                list.commands.push(PaintCommand::DrawText(
                    Point::new(x + 4.0, 14.0 + row as f64 * 13.0),
                    "some log line".to_string(),
                    12.0,
                    [230, 230, 230, 255],
                ));
            }
            list.pop_scope();
        }
        list.pop_scope();
        list.pop_scope();
        let scene = LintScene::from_paint_list(&list);
        let report = lint(&scene, &LintConfig::new());
        let hits = findings_for(&report, "packing-density");
        assert!(!hits.is_empty(), "expected alphanumeric-cap finding");
        assert!(
            hits[0].message.contains("alphanumeric"),
            "{:?}",
            hits[0].message
        );
    }

    #[test]
    fn packing_density_tightens_inside_l1() {
        // 40% coverage under an @level:1 lineage → over the 35%
        // critical cap but under the 50% general cap.
        let mut list = app_list();
        list.push_scope(None, "Overview@level:1", Rect::new(0.0, 0.0, 800.0, 300.0));
        for i in 0..4 {
            let x = i as f64 * 200.0;
            list.push_scope(None, "Chart", Rect::new(x, 0.0, x + 200.0, 120.0));
            list.pop_scope();
        }
        list.pop_scope();
        list.pop_scope();
        let scene = LintScene::from_paint_list(&list);
        let report = lint(&scene, &LintConfig::new());
        let hits = findings_for(&report, "packing-density");
        assert!(!hits.is_empty(), "expected critical-cap finding");
        assert!(
            hits[0].message.contains("critical"),
            "{:?}",
            hits[0].message
        );
    }

    #[test]
    fn packing_density_reports_outermost_only() {
        // A dense child inside a dense display is the same problem —
        // the finding anchors at the outermost offender and doesn't
        // cascade into its subtree.
        let mut list = app_list();
        list.push_scope(None, "Panel", Rect::new(0.0, 0.0, 800.0, 300.0));
        list.push_scope(None, "SubPanel", Rect::new(0.0, 0.0, 800.0, 300.0));
        for i in 0..4 {
            let x = i as f64 * 200.0;
            list.push_scope(None, "Label", Rect::new(x, 0.0, x + 200.0, 300.0));
            list.pop_scope();
        }
        list.push_scope(None, "Label", Rect::new(0.0, 300.0, 200.0, 300.0));
        list.pop_scope();
        list.pop_scope();
        list.pop_scope();
        list.pop_scope();
        let scene = LintScene::from_paint_list(&list);
        let report = lint(&scene, &LintConfig::new());
        let hits = findings_for(&report, "packing-density");
        assert_eq!(hits.len(), 1, "expected outermost finding only: {hits:?}");
        assert!(hits[0].path.ends_with("Panel"), "{:?}", hits[0].path);
    }

    // ---------- text-density ----------

    #[test]
    fn text_density_flags_wall_of_text() {
        // A text-dominant surface packed with runs: 30 rows × ~700px
        // advance at 14px ≈ 88% cell coverage > 60% cap.
        let mut list = app_list();
        list.push_scope(None, "LogView", Rect::new(0.0, 0.0, 800.0, 500.0));
        for i in 0..2 {
            let x = i as f64 * 400.0;
            list.push_scope(None, "Label", Rect::new(x, 0.0, x + 400.0, 500.0));
            for row in 0..30 {
                list.commands.push(PaintCommand::DrawText(
                    Point::new(x + 2.0, row as f64 * 15.0),
                    "x".repeat(90),
                    14.0,
                    [230, 230, 230, 255],
                ));
            }
            list.pop_scope();
        }
        list.pop_scope();
        list.pop_scope();
        let scene = LintScene::from_paint_list(&list);
        let report = lint(&scene, &LintConfig::new());
        assert!(!findings_for(&report, "text-density").is_empty());
    }

    #[test]
    fn text_density_quiet_under_cap_on_text_dominant() {
        // Text-dominant surface at ~53% cell coverage — evaluated,
        // but under the 60% cap, so no finding.
        let mut list = app_list();
        list.push_scope(None, "LogView", Rect::new(0.0, 0.0, 800.0, 500.0));
        for i in 0..2 {
            let x = i as f64 * 400.0;
            list.push_scope(None, "Label", Rect::new(x, 0.0, x + 400.0, 500.0));
            for row in 0..12 {
                list.commands.push(PaintCommand::DrawText(
                    Point::new(x + 2.0, row as f64 * 15.0),
                    "x".repeat(90),
                    14.0,
                    [230, 230, 230, 255],
                ));
            }
            list.pop_scope();
        }
        list.pop_scope();
        list.pop_scope();
        let scene = LintScene::from_paint_list(&list);
        let report = lint(&scene, &LintConfig::new());
        assert!(findings_for(&report, "text-density").is_empty());
    }

    #[test]
    fn text_density_quiet_on_sparse_text() {
        let mut list = app_list();
        list.push_scope(None, "NotesPanel", Rect::new(0.0, 0.0, 800.0, 500.0));
        for i in 0..2 {
            let x = i as f64 * 400.0;
            list.push_scope(None, "Label", Rect::new(x, 0.0, x + 400.0, 500.0));
            list.commands.push(PaintCommand::DrawText(
                Point::new(x + 4.0, 20.0),
                "short note".to_string(),
                12.0,
                [230, 230, 230, 255],
            ));
            list.pop_scope();
        }
        list.pop_scope();
        list.pop_scope();
        let scene = LintScene::from_paint_list(&list);
        let report = lint(&scene, &LintConfig::new());
        assert!(findings_for(&report, "text-density").is_empty());
    }

    #[test]
    fn text_density_skips_non_text_displays() {
        // Charts dominate leaf area — not an alphanumeric display,
        // even with a text run inside each chart's caption zone.
        let mut list = app_list();
        list.push_scope(None, "ChartWall", Rect::new(0.0, 0.0, 800.0, 500.0));
        for i in 0..2 {
            let x = i as f64 * 400.0;
            list.push_scope(None, "Chart", Rect::new(x, 0.0, x + 400.0, 480.0));
            list.commands.push(PaintCommand::DrawText(
                Point::new(x + 4.0, 470.0),
                "x".repeat(90),
                14.0,
                [230, 230, 230, 255],
            ));
            list.pop_scope();
        }
        list.pop_scope();
        list.pop_scope();
        let scene = LintScene::from_paint_list(&list);
        let report = lint(&scene, &LintConfig::new());
        assert!(findings_for(&report, "text-density").is_empty());
    }

    // ---------- simultaneous-channels ----------

    #[test]
    fn simultaneous_channels_flags_display_wall() {
        let mut list = app_list();
        list.push_scope(None, "Panel", Rect::new(0.0, 0.0, 800.0, 300.0));
        for i in 0..8 {
            let x = i as f64 * 100.0;
            list.push_scope(None, "Gauge", Rect::new(x, 0.0, x + 100.0, 300.0));
            list.pop_scope();
        }
        list.pop_scope();
        list.pop_scope();
        let scene = LintScene::from_paint_list(&list);
        let report = lint(&scene, &LintConfig::new());
        let hits = findings_for(&report, "simultaneous-channels");
        assert_eq!(hits.len(), 1, "expected one innermost finding: {hits:?}");
    }

    #[test]
    fn simultaneous_channels_quiet_at_budget() {
        let mut list = app_list();
        list.push_scope(None, "Panel", Rect::new(0.0, 0.0, 800.0, 300.0));
        for i in 0..5 {
            let x = i as f64 * 160.0;
            list.push_scope(None, "Chart", Rect::new(x, 0.0, x + 160.0, 300.0));
            list.pop_scope();
        }
        list.pop_scope();
        list.pop_scope();
        let scene = LintScene::from_paint_list(&list);
        let report = lint(&scene, &LintConfig::new());
        assert!(findings_for(&report, "simultaneous-channels").is_empty());
    }

    #[test]
    fn simultaneous_channels_counts_topmost_only() {
        // A Chart containing an inner Sparkline is one channel — the
        // nested display doesn't inflate the count.
        let mut list = app_list();
        list.push_scope(None, "Panel", Rect::new(0.0, 0.0, 800.0, 300.0));
        for i in 0..6 {
            let x = i as f64 * 130.0;
            list.push_scope(None, "Chart", Rect::new(x, 0.0, x + 130.0, 300.0));
            list.push_scope(None, "Sparkline", Rect::new(x, 250.0, x + 130.0, 300.0));
            list.pop_scope();
            list.pop_scope();
        }
        list.pop_scope();
        list.pop_scope();
        let scene = LintScene::from_paint_list(&list);
        let report = lint(&scene, &LintConfig::new());
        assert!(findings_for(&report, "simultaneous-channels").is_empty());
    }
}
