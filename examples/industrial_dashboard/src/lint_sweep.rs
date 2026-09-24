//! Shared design-lint sweep — the machinery behind the
//! `dump_design_lints` test and the `design-lint` CLI bin.
//!
//! Sweeps every zone page inside a `ScrollView` across widths and
//! scroll offsets plus one full-dock pass, running
//! `martensite-design-lint` over each frame's `PaintList`. With
//! [`SweepOptions::fix`] set, each frame's scene is run through
//! `autofix` first and the reported findings describe the *post-fix*
//! scene — the lint→fix→re-lint loop.

use std::collections::{BTreeSet, HashSet};
use std::fmt::Write as _;
use std::time::Duration;

use glam::Vec2;
use martensite::core::{LayoutContext, PaintList, SemanticAction, WidgetEvent};
use martensite::prelude::*;
use martensite::widgets::container::Container;
use martensite::widgets::scrollview::ScrollView;
use martensite_design_lint::{autofix, lint_paint_list, FixOptions, LintConfig, LintScene};

use crate::app::{App, ThemeChoice};

/// What to sweep and how.
pub struct SweepOptions {
    /// Zone widths to render each page at.
    pub widths: Vec<f32>,
    /// Substring filter over `{zone}/{page}@{width}` tags.
    pub page_filter: String,
    /// Echo the scope tree of every frame.
    pub dump_scopes: bool,
    /// When `Some`, run `autofix` on each frame's scene before
    /// collecting findings (post-fix lint output).
    pub fix: Option<FixOptions>,
    /// Suppress per-finding lines — summary only.
    pub quiet: bool,
}

impl Default for SweepOptions {
    fn default() -> Self {
        SweepOptions {
            widths: vec![700.0, 900.0, 1100.0, 1324.0, 1500.0, 1828.0, 2100.0, 2400.0],
            page_filter: String::new(),
            dump_scopes: false,
            fix: None,
            quiet: false,
        }
    }
}

/// Aggregate sweep results — `log` holds the full human-readable
/// dump; the counters are for assertions and exit codes.
#[derive(Default)]
pub struct SweepReport {
    /// Frames audited.
    pub frames: usize,
    /// Unique `(rule, message)` active findings across all frames.
    pub unique_findings: usize,
    /// Unique active findings at `Warn` or above — the CI-gating
    /// subset (Info findings never fail a run).
    pub gating_findings: usize,
    /// Unique `(rule, message)` suppressed findings.
    pub unique_suppressed: usize,
    /// Allows that suppressed nothing in ANY frame.
    pub stale_allows: Vec<String>,
    /// Fixes applied across all frames (deduplicated by rule+summary).
    pub fixes_applied: usize,
    /// Risky fixes skipped for lack of force (deduplicated).
    pub fixes_skipped_risky: usize,
    /// The full human-readable dump (findings, fixes, summary).
    pub log: String,
}

fn tick_all(w: &mut dyn martensite::core::Widget, dt: Duration) {
    for i in 0..w.child_count() {
        if let Some(c) = w.child_mut(i) {
            tick_all(c, dt);
        }
    }
    let _ = w.tick(dt);
}

/// Run the sweep. Prints nothing itself — the caller decides where
/// `report.log` goes.
pub fn run(cfg: &LintConfig, opts: &SweepOptions) -> SweepReport {
    let mut out = SweepReport::default();
    let mut log = String::new();

    // (rule, message) dedupe — the same finding at twenty scroll
    // offsets and eight widths is one finding.
    #[derive(Default)]
    struct Acc {
        seen: HashSet<(&'static str, String)>,
        gating: HashSet<(&'static str, String)>,
        suppressed: HashSet<(&'static str, String)>,
        unused: BTreeSet<String>,
        // Allows that suppressed at least once across ALL frames —
        // `unused_allows` is per-frame, so an allow only counts as
        // stale when it never fired anywhere.
        used: HashSet<String>,
        applied_fixes: BTreeSet<(String, String)>,
        skipped_risky: BTreeSet<(String, String)>,
    }
    let mut acc = Acc::default();

    fn collect(
        tag: &str,
        report: &martensite_design_lint::LintReport,
        acc: &mut Acc,
        quiet: bool,
        log: &mut String,
    ) {
        for f in &report.findings {
            if f.severity >= martensite_design_lint::Severity::Warn {
                acc.gating.insert((f.rule, f.message.clone()));
            }
            if acc.seen.insert((f.rule, f.message.clone())) && !quiet {
                let _ = writeln!(
                    log,
                    "{tag}: [{}] {} {}: {}",
                    f.severity, f.rule, f.path, f.message
                );
                if !f.doc.is_empty() {
                    let _ = writeln!(log, "    see: {}", f.doc);
                }
                if let Some(fix) = &f.fix {
                    let tag2 = match fix.safety {
                        martensite_design_lint::FixSafety::Safe => "fix:",
                        martensite_design_lint::FixSafety::Risky => "fix (needs --force):",
                    };
                    let _ = writeln!(log, "    {tag2} {}", fix.summary);
                }
            }
        }
        for f in &report.suppressed {
            acc.suppressed.insert((f.rule, f.message.clone()));
        }
        // `used_allows`/`unused_allows` share one descriptor grammar —
        // subtracting the cross-frame used set leaves true stale.
        acc.used.extend(report.used_allows.iter().cloned());
        acc.unused.extend(report.unused_allows.iter().cloned());
    }

    fn audit_frame(
        tag: &str,
        list: &PaintList,
        cfg: &LintConfig,
        opts: &SweepOptions,
        acc: &mut Acc,
        log: &mut String,
        frames: &mut usize,
    ) {
        *frames += 1;
        if let Some(fix_opts) = &opts.fix {
            let mut scene = LintScene::from_paint_list(list);
            scene.scale_factor = cfg.scale_factor;
            let fr = autofix(&mut scene, cfg, fix_opts);
            for it in &fr.iterations {
                for a in &it.applied {
                    acc.applied_fixes
                        .insert((a.rule.to_string(), a.summary.clone()));
                }
            }
            // Post-fix lint — findings describe the repaired scene.
            let report = fr.report;
            collect(tag, &report, acc, opts.quiet, log);
            if fr.iterations.iter().any(|i| i.skipped_risky > 0) {
                // Record which risky fixes were gated (rule+summary
                // comes from the findings themselves).
                for f in &report.findings {
                    if let Some(fix) = &f.fix {
                        if fix.safety == martensite_design_lint::FixSafety::Risky {
                            acc.skipped_risky
                                .insert((f.rule.to_string(), fix.summary.clone()));
                        }
                    }
                }
            }
            return;
        }
        let report = lint_paint_list(list, cfg);
        collect(tag, &report, acc, opts.quiet, log);
    }

    let mut app = App::new(Some(ThemeChoice::Dark), false);
    type ZonePages = Vec<(&'static str, crate::zone::Page)>;
    let mut pages_by_zone: Vec<(&str, f32, f32, ZonePages)> = vec![];
    for zw in &opts.widths {
        let zw = *zw;
        pages_by_zone.push(("grid", zw, 480.0, crate::zones::grid::pages(&app.model)));
        pages_by_zone.push((
            "telemetry",
            zw,
            480.0,
            crate::zones::telemetry::pages(&app.model),
        ));
        pages_by_zone.push(("editor", zw, 350.0, crate::zones::editor::pages(&app.model)));
        pages_by_zone.push(("media", zw, 350.0, crate::zones::media::pages(&app.model)));
    }
    for (zname, zw, zh, pages) in pages_by_zone {
        for (label, page) in pages {
            let tag = format!("{zname}/{label}@{zw:.0}");
            if !opts.page_filter.is_empty() && !tag.contains(&opts.page_filter) {
                continue;
            }
            let view = ScrollView::new(
                Container::new()
                    .padding_uniform(crate::zone::ZONE_PAD)
                    .child(page),
            );
            let mut arena = WidgetArena::new();
            arena.set_theme(martensite::theme::tokens::default_dark());
            arena.set_scale_factor(2.0);
            arena.set_text_painter(martensite::text_paint::shared_painter());
            let mut hot = HotNode::default();
            hot.flags |= NodeFlags::VISIBLE;
            let root = arena.insert_with_widget(hot, Box::new(view));
            let bounds = Rect::new(0.0, 0.0, zw, zh);
            if let Some((hot, cold)) = arena.get_both_mut(root) {
                hot.bounds = bounds;
                cold.widget
                    .layout(&mut LayoutContext { hot, scale: 2.0 }, bounds);
            }
            // Run the binding cycle once so Bound::push populates the
            // widgets with model data.
            if let Some(cold) = arena.get_cold_mut(root) {
                tick_all(&mut *cold.widget, Duration::from_millis(16));
            }
            let content_h = arena
                .get_cold(root)
                .and_then(|c| c.widget.child_bounds(0))
                .map(|b| b.height())
                .unwrap_or(0.0);
            let mut y = 0.0f32;
            loop {
                arena.dispatch_event(
                    root,
                    &WidgetEvent::SemanticAction(SemanticAction::SetScrollOffset(Vec2::new(
                        0.0, y,
                    ))),
                );
                let mut list = PaintList::new();
                arena.build_paint_list(root, &mut list);
                if opts.dump_scopes {
                    for cmd in &list.commands {
                        if let martensite::core::PaintCommand::PushScope { name, bounds, .. } = cmd
                        {
                            let _ = writeln!(log, "  scope {name} {bounds:?}");
                        }
                    }
                }
                audit_frame(&tag, &list, cfg, opts, &mut acc, &mut log, &mut out.frames);
                if y >= content_h {
                    break;
                }
                y += zh * 0.5;
            }
        }
    }

    // Full-app pass — the real dock tree at a laptop-class surface,
    // painted at the same 2.0 scale the config declares (a
    // paint/audit scale mismatch halves every reported pt size).
    // The page filter gates this pass like any tagged pass.
    let app_tag = "app@1600x1000";
    if opts.page_filter.is_empty() || app_tag.contains(&opts.page_filter) {
        app.scale.set(2.0);
        app.build_arena();
        app.apply_dock_layout_at(1600, 1000);
        let root = app.root.expect("root");
        let arena = app.arena.as_mut().expect("arena");
        if let Some(cold) = arena.get_cold_mut(root) {
            tick_all(&mut *cold.widget, Duration::from_millis(16));
        }
        let mut list = PaintList::new();
        arena.build_paint_list(root, &mut list);
        audit_frame(
            "app@1600x1000",
            &list,
            cfg,
            opts,
            &mut acc,
            &mut log,
            &mut out.frames,
        );
    }

    // An allow is only stale when it never suppressed a finding in
    // ANY frame — `unused_allows` is per-frame, so subtract the
    // cross-frame used set (suppressed_by carries the same
    // `allow "path" [specs]` description).
    out.stale_allows = acc
        .unused
        .iter()
        .filter(|a| !acc.used.contains(*a))
        .cloned()
        .collect();
    out.unique_findings = acc.seen.len();
    out.gating_findings = acc.gating.len();
    out.unique_suppressed = acc.suppressed.len();
    out.fixes_applied = acc.applied_fixes.len();
    out.fixes_skipped_risky = acc.skipped_risky.len();

    let _ = writeln!(
        log,
        "=== design-lint summary: {} unique findings, {} unique suppressed, {} stale allows{} ===",
        out.unique_findings,
        out.unique_suppressed,
        out.stale_allows.len(),
        if opts.fix.is_some() {
            format!(
                ", {} fix(es) applied, {} risky gated",
                out.fixes_applied, out.fixes_skipped_risky
            )
        } else {
            String::new()
        }
    );
    for (rule, message) in &acc.suppressed {
        let _ = writeln!(log, "  suppressed: {rule}: {message}");
    }
    for a in &out.stale_allows {
        let _ = writeln!(log, "  unused allow: {a}");
    }
    for (rule, summary) in &acc.applied_fixes {
        let _ = writeln!(log, "  applied: {rule}: {summary}");
    }
    for (rule, summary) in &acc.skipped_risky {
        let _ = writeln!(log, "  gated (needs --force): {rule}: {summary}");
    }

    out.log = log;
    out
}
