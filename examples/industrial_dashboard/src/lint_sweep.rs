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
    /// When `Some`, every swept frame is also rasterized through
    /// `martensite_render::TinySkiaBackend` and written to PNG under
    /// the configured dir (plus Machado-matrix deutan/protan sims).
    /// The ambient text painter is swapped for the bundled-font
    /// [`crate::frames::FixtureTextShaper`] so ambient text stays
    /// machine-independent — see `frames` for the residual `Text`
    /// widget limitation. Always dumps the RAW paint list — the
    /// `fix` option's post-fix scene is a lint-side model, not a
    /// repainted frame.
    pub dump_frames: Option<crate::frames::FrameDump>,
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
            dump_frames: None,
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
    /// Sorted `"rule :: path"` lines for every gating finding — the
    /// comparable form the checked-in baseline asserts against.
    /// Keyed on path, not message: messages embed font-derived
    /// metrics (target sizes, areas), and although the sweep installs
    /// the bundled-font fixture on both text paths (ambient painter +
    /// `FontManager::new` thread-local override), path keys keep the
    /// baseline robust against any residual metric drift. Granularity:
    /// a second finding under an already-flagged path doesn't extend
    /// the baseline (the backlog is per-location). Sorted for
    /// stability.
    pub gating_details: Vec<String>,
    /// Unique `(rule, message)` suppressed findings.
    pub unique_suppressed: usize,
    /// Allows that suppressed nothing in ANY frame.
    pub stale_allows: Vec<String>,
    /// Fixes applied across all frames (deduplicated by rule+summary).
    pub fixes_applied: usize,
    /// Risky fixes skipped for lack of force (deduplicated).
    pub fixes_skipped_risky: usize,
    /// PNGs written by `dump_frames` — zero when the option is off.
    pub frames_written: usize,
    /// Frames compared against a baseline dir (`--check`).
    pub frames_checked: usize,
    /// Frames whose perceptual diff vs the baseline failed (or whose
    /// baseline was missing/undecodable) — CI-gating like
    /// `gating_findings` for golden checks.
    pub frames_drifted: usize,
    /// The full human-readable dump (findings, fixes, summary).
    pub log: String,
}

/// Mirrors `WidgetArena::tick_recursive`: parent first, then children,
/// skipping children with no allocated bounds (a closed `Disclosure`
/// reports zero children, matching production suspend semantics).
fn tick_all(w: &mut dyn martensite::core::Widget, dt: Duration) {
    let _ = w.tick(dt);
    for i in 0..w.child_count() {
        if w.child_bounds(i).is_none() {
            continue;
        }
        if let Some(c) = w.child_mut(i) {
            tick_all(c, dt);
        }
    }
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
        // (rule, path) pairs for the baseline — metric-free, so host
        // font differences can't false-drift the gate.
        gating_paths: HashSet<(&'static str, String)>,
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
                acc.gating_paths.insert((f.rule, f.path.clone()));
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

    // Deterministic fonts on BOTH paths: the ambient painter gets the
    // bundled-font FixtureTextShaper (built once, cloned into every
    // arena so its FontSystem + shaping caches are shared), and the
    // thread-local `FontManager::new` override routes the per-widget
    // managers inside `Text` through the same bundled face — lint
    // metrics and finding membership are identical between the dump
    // and plain-sweep paths and across machines (system fonts drift),
    // so `LINT_GATING_BASELINE.txt` stays honest.
    let _font_guard = crate::frames::install_test_fonts();
    let fixture = Some(crate::frames::FixtureTextShaper::new());

    let mut app = App::new(Some(ThemeChoice::Dark), false);
    // The sweep never runs the frame loop that feeds the sim
    // (`push_history`, `tick_acoustic`, `tick_minute`) — warm the
    // model to its operating state before pages bind to it or every
    // chart/spectrum/gauge paints the flat seed line.
    app.model.warm_demo_state();
    type ZonePages = Vec<(&'static str, crate::zone::Page)>;
    let mut pages_by_zone: Vec<(&str, usize, f32, f32, ZonePages)> = vec![];
    for zw in &opts.widths {
        let zw = *zw;
        pages_by_zone.push(("grid", 0, zw, 480.0, crate::zones::grid::pages(&app.model)));
        pages_by_zone.push((
            "telemetry",
            1,
            zw,
            480.0,
            crate::zones::telemetry::pages(&app.model),
        ));
        pages_by_zone.push((
            "editor",
            2,
            zw,
            350.0,
            crate::zones::editor::pages(&app.model),
        ));
        pages_by_zone.push((
            "media",
            3,
            zw,
            350.0,
            crate::zones::media::pages(&app.model),
        ));
    }
    for (zname, zindex, zw, zh, pages) in pages_by_zone {
        // Pages mount into bare ScrollViews here — no ZonePanel ever
        // publishes its width, so seed the zone's slot directly or the
        // pages see the 960 default and the responsive breakpoints
        // (stack at <560, disclosure collapse at <380) go unexercised.
        // The 2.0 scale factor below divides physical px into pt.
        app.model.zone_width[zindex].set(zw / 2.0);
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
            if let Some(p) = &fixture {
                arena.set_text_painter(p.clone());
            } else {
                arena.set_text_painter(martensite::text_paint::shared_painter());
            }
            // Manual layout bypasses `LayoutEngine` — install the
            // ambient measurer so widget `measure` calls see the same
            // glyph metrics the paint pass will use.
            let _measurer = arena
                .text_painter_shared()
                .map(martensite::core::paint::install_ambient_measurer);
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
                if let Some(d) = &opts.dump_frames {
                    let d_out = crate::frames::dump_frame(
                        d,
                        &tag,
                        y,
                        &list,
                        zw.round() as u32,
                        zh.round() as u32,
                        &mut log,
                    );
                    out.frames_written += d_out.written;
                    out.frames_checked += d_out.checked;
                    out.frames_drifted += d_out.drifted;
                }
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
        // Same fixture swap as the per-zone arenas — overwrites the
        // `shared_painter` `build_arena` installed.
        if let Some(p) = &fixture {
            arena.set_text_painter(p.clone());
        }
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
        if let Some(d) = &opts.dump_frames {
            let d_out =
                crate::frames::dump_frame(d, "app@1600x1000", 0.0, &list, 1600, 1000, &mut log);
            out.frames_written += d_out.written;
            out.frames_checked += d_out.checked;
            out.frames_drifted += d_out.drifted;
        }
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
    out.gating_details = acc
        .gating_paths
        .iter()
        .map(|(rule, path)| format!("{rule} :: {path}"))
        .collect();
    out.gating_details.sort();
    out.unique_suppressed = acc.suppressed.len();
    out.fixes_applied = acc.applied_fixes.len();
    out.fixes_skipped_risky = acc.skipped_risky.len();

    let _ = writeln!(
        log,
        "=== design-lint summary: {} unique findings, {} unique suppressed, {} stale allows{}{} ===",
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
        },
        if opts.dump_frames.is_some() {
            format!(
                ", {} frame PNGs written, {} checked, {} drifted",
                out.frames_written, out.frames_checked, out.frames_drifted
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

#[cfg(test)]
mod tests {
    use super::*;

    /// `cargo test -p industrial_dashboard dump_frames -- --nocapture`
    /// renders every page at the spec widths (700/1200/1600) through
    /// `TinySkiaBackend` and writes PNGs + Machado deutan/protan sims
    /// under `target/dashboard-frames/`. `PAGE_FILTER` narrows the
    /// zone loop as usual; `FRAME_BASELINE=<dir>` switches the run
    /// into golden-diff mode (drift fails the assertion).
    #[test]
    fn dump_frames() {
        let cfg = LintConfig::from_toml(include_str!("../design-lint.toml"))
            .expect("design-lint.toml parses");
        let opts = SweepOptions {
            widths: crate::frames::FRAME_WIDTHS.to_vec(),
            page_filter: std::env::var("PAGE_FILTER").unwrap_or_default(),
            dump_frames: Some(crate::frames::FrameDump {
                dir: crate::frames::default_dir(),
                baseline: std::env::var("FRAME_BASELINE").ok().map(Into::into),
            }),
            quiet: true,
            ..Default::default()
        };
        let report = run(&cfg, &opts);
        eprint!("{}", report.log);
        assert!(report.frames_written > 0, "dump_frames wrote no PNGs");
        assert_eq!(
            report.frames_drifted, 0,
            "golden drift vs FRAME_BASELINE — inspect target/dashboard-frames"
        );
    }
}
