//! Temporal rules — states that are only wrong because they persist.
//!
//! `loading-stuck` is the design-lint half of ADR-0040's "stuck
//! loading is lint policy, not runtime timers": a widget whose
//! skeleton placeholder survives N consecutive sampled frames is the
//! signature of a forgotten `set_loading(false)` or a fetch that
//! never resolves — a panel that reports "pending" forever is
//! indistinguishable from a broken one.
//!
//! A single `LintScene` carries one frame, so persistence arrives as
//! data, not state: scope names carrying the `@loading:N` marker (N
//! consecutive loading sightings, stamped by
//! [`LoadingTracker`](crate::LoadingTracker) sampling the arena's
//! `node_loading` resolution, or embedded by a producer directly) or
//! a bare `@loading` (one sighting). A first sighting is never a
//! finding — loading itself is legitimate; *stuck* is the smell.

use crate::config::LintConfig;
use crate::report::Finding;
use crate::rule::{param, Confidence, LintRule};
use crate::rules::marker_in_lineage;
use crate::scene::LintScene;
use crate::severity::Severity;
use crate::standard::Standard;

/// This module's rules, appended to the registry by `all_rules()`.
pub(crate) fn rules() -> Vec<&'static dyn LintRule> {
    vec![&LoadingStuck]
}

/// — `loading-stuck`: a skeleton placeholder persisting across
/// sampled frames.
///
/// Reads the `loading` marker count on each scope: `@loading:N` is a
/// node observed loading for N consecutive sampled frames, bare
/// `@loading` is a single sighting. Fires when the count reaches
/// `min_frames` (default 3 — at the ~2 Hz audit cadence that is
/// ≈1.5 s of continuous shimmer, past one full sweep period; in a
/// lint sweep it means the node was still loading on the third
/// consecutive sample — no longer a transitional flash).
///
/// The `@skeleton` marker (node or ancestor — the `Name@marker`
/// lineage convention) is the exemption: a deliberately perpetual
/// placeholder or a certified "data unknown" treatment declares
/// intent and reports nothing. Path allows and `@lint:loading-stuck`
/// suppress normally.
struct LoadingStuck;
impl LintRule for LoadingStuck {
    fn id(&self) -> &'static str {
        "loading-stuck"
    }
    fn standards(&self) -> &'static [Standard] {
        &[Standard::InfoDesign]
    }
    fn default_severity(&self) -> Severity {
        Severity::Warn
    }
    fn confidence(&self) -> Confidence {
        // The count is measured, but "still loading" vs "legitimately
        // slow fetch" is a judgement call — a review prompt, not a
        // violation.
        Confidence::Heuristic
    }
    fn title(&self) -> &'static str {
        "skeleton placeholder persisting across sampled frames"
    }
    fn citation(&self) -> &'static str {
        "Nielsen heuristic #1 visibility of system status — a skeleton \
         that never resolves reports 'pending' forever; ADR-0040 §5"
    }
    fn check(&self, scene: &LintScene, cfg: &LintConfig) -> Vec<Finding> {
        let min_frames = param(cfg, self.id(), "min_frames", 3.0) as u32;
        let mut out = Vec::new();
        for n in scene.walk() {
            // `@loading:N` carries the consecutive-sample count; a
            // bare `@loading` (or a non-numeric stamp) is one
            // sighting — below every sane threshold.
            let Some(v) = n.marker_value("loading") else {
                continue;
            };
            let count = v.parse::<u32>().unwrap_or(1).max(1);
            if count < min_frames {
                continue;
            }
            // `@skeleton` in the lineage = deliberately perpetual
            // placeholder (ADR-0040's exemption marker) — declared
            // intent, not a stuck load.
            if marker_in_lineage(scene, n, "skeleton") {
                continue;
            }
            out.push(
                Finding::new(
                    "loading-stuck",
                    &n.path,
                    format!(
                        "{} still skeleton-painted after {count} consecutive sampled frames \
                         (threshold {min_frames}) — a placeholder that never resolves usually \
                         means a forgotten `set_loading(false)` or a fetch that never \
                         completes; declare `@skeleton` if this is intentional",
                        n.name
                    ),
                )
                .at(n.bounds),
            );
        }
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{lint, LoadingTracker};
    use kurbo::Rect;
    use martensite_core::{
        ColdNode, DummyWidget, HotNode, NodeFlags, PaintCommand, PaintList, WidgetArena,
    };

    fn findings_for<'a>(report: &'a crate::LintReport, rule: &str) -> Vec<&'a crate::Finding> {
        report.findings.iter().filter(|f| f.rule == rule).collect()
    }

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

    #[test]
    fn stuck_node_flags_past_min_frames() {
        let mut list = app_list();
        list.push_scope(None, "Feed@loading:5", Rect::new(10.0, 10.0, 400.0, 200.0));
        list.pop_scope();
        list.pop_scope();
        let scene = LintScene::from_paint_list(&list);
        let report = lint(&scene, &LintConfig::new());
        let hits = findings_for(&report, "loading-stuck");
        assert_eq!(hits.len(), 1, "expected one finding, got {hits:?}");
        assert!(hits[0].path.ends_with("Feed"));
        assert!(hits[0].message.contains('5'));
    }

    #[test]
    fn recent_sighting_is_quiet() {
        // Bare `@loading` = one sighting; `@loading:2` < min_frames 3.
        for name in ["Feed@loading", "Feed@loading:2"] {
            let mut list = app_list();
            list.push_scope(None, name, Rect::new(10.0, 10.0, 400.0, 200.0));
            list.pop_scope();
            list.pop_scope();
            let scene = LintScene::from_paint_list(&list);
            let report = lint(&scene, &LintConfig::new());
            assert!(
                findings_for(&report, "loading-stuck").is_empty(),
                "{name} should not fire at default min_frames"
            );
        }
    }

    #[test]
    fn min_frames_param_reconfigures() {
        let mut list = app_list();
        list.push_scope(None, "Feed@loading:2", Rect::new(10.0, 10.0, 400.0, 200.0));
        list.pop_scope();
        list.pop_scope();
        let scene = LintScene::from_paint_list(&list);
        let cfg = LintConfig::new().with_rule_param("loading-stuck", "min_frames", 2.0);
        let report = lint(&scene, &cfg);
        assert_eq!(findings_for(&report, "loading-stuck").len(), 1);
    }

    #[test]
    fn skeleton_marker_exempts() {
        // Direct exemption and lineage exemption — `@skeleton` on an
        // ancestor covers the loading node below it.
        let mut list = app_list();
        list.push_scope(
            None,
            "Feed@loading:9@skeleton",
            Rect::new(10.0, 10.0, 400.0, 200.0),
        );
        list.pop_scope();
        list.push_scope(
            None,
            "StatusPanel@skeleton",
            Rect::new(10.0, 220.0, 400.0, 300.0),
        );
        list.push_scope(
            None,
            "Channel@loading:9",
            Rect::new(20.0, 230.0, 200.0, 260.0),
        );
        list.pop_scope();
        list.pop_scope();
        list.pop_scope();
        let scene = LintScene::from_paint_list(&list);
        let report = lint(&scene, &LintConfig::new());
        assert!(
            findings_for(&report, "loading-stuck").is_empty(),
            "@skeleton lineage must exempt: {report:?}"
        );
    }

    #[test]
    fn stuck_loading_via_arena_tracker() {
        // The arena seam: `set_loading` flips `node_loading`, the
        // tracker counts consecutive samples, `annotate` stamps the
        // count the rule reads.
        let mut arena = WidgetArena::new();
        let mut hot = HotNode::default();
        hot.flags |= NodeFlags::VISIBLE;
        hot.bounds = martensite_core::Rect::new(0.0, 0.0, 200.0, 80.0);
        let root = arena.insert(hot, ColdNode::new(Box::new(DummyWidget)));
        arena.set_loading(root, true);

        let mut tracker = LoadingTracker::new();
        let mut scene = LintScene::default();
        for _ in 0..3 {
            tracker.sample_arena(&arena);
            let mut list = PaintList::new();
            arena.build_paint_list(root, &mut list);
            scene = LintScene::from_paint_list(&list);
            tracker.annotate(&mut scene);
        }
        let report = lint(&scene, &LintConfig::new());
        let hits = findings_for(&report, "loading-stuck");
        assert_eq!(hits.len(), 1, "stuck arena node must flag: {report:?}");
    }

    #[test]
    fn resolving_node_never_reaches_threshold() {
        // Loading for two samples, resolved on the third — the count
        // resets and no marker is stamped.
        let mut arena = WidgetArena::new();
        let mut hot = HotNode::default();
        hot.flags |= NodeFlags::VISIBLE;
        hot.bounds = martensite_core::Rect::new(0.0, 0.0, 200.0, 80.0);
        let root = arena.insert(hot, ColdNode::new(Box::new(DummyWidget)));

        let mut tracker = LoadingTracker::new();
        let mut scene = LintScene::default();
        for i in 0..3 {
            arena.set_loading(root, i < 2);
            tracker.sample_arena(&arena);
            let mut list = PaintList::new();
            arena.build_paint_list(root, &mut list);
            scene = LintScene::from_paint_list(&list);
            tracker.annotate(&mut scene);
        }
        let report = lint(&scene, &LintConfig::new());
        assert!(findings_for(&report, "loading-stuck").is_empty());
    }

    #[test]
    fn lint_allow_suppresses() {
        let mut list = app_list();
        list.push_scope(
            None,
            "Feed@loading:9@lint:loading-stuck",
            Rect::new(10.0, 10.0, 400.0, 200.0),
        );
        list.pop_scope();
        list.pop_scope();
        let scene = LintScene::from_paint_list(&list);
        let report = lint(&scene, &LintConfig::new());
        assert!(findings_for(&report, "loading-stuck").is_empty());
        assert_eq!(
            report
                .suppressed
                .iter()
                .filter(|f| f.rule == "loading-stuck")
                .count(),
            1
        );
    }
}
