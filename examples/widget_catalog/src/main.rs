//! Martensite Widget Catalog executable.

use widget_catalog::{all_pages, CatalogView};

/// Render knobs beyond size — staged page, prop overrides, zoom,
/// direction, and locale selection.
struct RenderOpts<'a> {
    page: Option<usize>,
    props: &'a [(String, String)],
    zoom: f64,
    rtl: bool,
    locale: Option<usize>,
}

/// Rasterizes the catalog view to a PNG — the same
/// `WidgetArena` → `PaintList` → `TinySkiaBackend` path the dev
/// channel's `capture_node` uses, without needing a window or GPU.
fn render_png(path: &str, w: f32, h: f32, scale: f32, opts: &RenderOpts) {
    use glam::Vec2;
    use martensite::core::{LayoutConstraints, LayoutContext, PaintList};
    use martensite::prelude::*;
    use martensite_render::RenderBackend;

    let mut arena = WidgetArena::new();
    arena.set_theme(martensite::theme::tokens::default_dark());
    arena.set_scale_factor(scale);
    arena.set_text_painter(martensite::text_paint::shared_painter());
    let _measurer = arena
        .text_painter_shared()
        .map(martensite::core::paint::install_ambient_measurer);
    let mut hot = HotNode::default();
    hot.flags |= NodeFlags::VISIBLE | NodeFlags::HIT_TEST_ENABLED;
    let mut view = CatalogView::new(all_pages());
    if let Some(page) = opts.page {
        view.select_page(page);
    }
    for (k, v) in opts.props {
        view.apply_prop_text(k, v);
    }
    if opts.zoom != 1.0 {
        view.sig_zoom.set(opts.zoom);
    }
    if opts.rtl {
        view.sig_rtl.set(true);
    }
    if let Some(l) = opts.locale {
        view.sig_locale.set(l);
    }
    // Rebuild the staged widget with the overridden props.
    view.reconcile();
    let root = arena.insert_with_widget(hot, Box::new(view));

    // Same contract as the live path: layout works in surface
    // (device) pixels; widgets convert pt→px via `cx.pt(scale)`.
    let bounds = Rect::new(0.0, 0.0, w, h);
    if let Some((hot, cold)) = arena.get_both_mut(root) {
        cold.widget.measure(
            &mut LayoutContext { hot, scale },
            LayoutConstraints {
                min_size: Vec2::ZERO,
                max_size: Vec2::new(w, h),
            },
        );
        hot.bounds = bounds;
        cold.widget
            .layout(&mut LayoutContext { hot, scale }, bounds);
    }
    let mut list = PaintList::new();
    arena.build_paint_list(root, &mut list);
    paint_overlays(&mut arena, bounds, &mut list);
    let mut backend =
        martensite_render::TinySkiaBackend::new(w as u32, h as u32).expect("pixmap alloc");
    RenderBackend::render(&mut backend, &list);
    let png = backend.pixmap().encode_png().expect("png encode");
    std::fs::write(path, png).expect("png write");
    eprintln!("catalog → {path} ({w}x{h} @ {scale}x)");
}

/// Syncs the overlay layer and appends its paint commands — the same
/// two steps the windowed host runs each frame. Without this, staged
/// popups (popovers, sheets, hover cards, tooltips) render nothing.
fn paint_overlays(
    arena: &mut martensite::prelude::WidgetArena,
    bounds: martensite::prelude::Rect,
    list: &mut martensite::core::PaintList,
) {
    arena.overlay_mut().set_viewport(bounds);
    arena.sync_overlays();
    arena.sync_overlays(); // second pass resolves entries opened above
    let painter = arena.text_painter_shared();
    arena
        .overlay()
        .paint(list, arena.theme(), painter.as_deref().map(|p| p as _));
}

/// Rasterizes each page's staged widget inside a [`StageHost`] — one
/// PNG per element, named `NNN_Family_Name.png`, for per-element
/// review. `page` limits to a single index.
fn render_stage_pngs(dir: &str, w: f32, h: f32, scale: f32, page: Option<usize>) {
    let pages = all_pages();
    std::fs::create_dir_all(dir).expect("mkdir stage shots");
    for (i, p) in pages.iter().enumerate() {
        if let Some(sel) = page {
            if sel != i {
                continue;
            }
        }
        let name = p.meta().name;
        let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            render_one_stage(dir, w, h, scale, i, p.as_ref());
        }));
        if result.is_err() {
            eprintln!("stage shots: page {i} ({name}) panicked — skipped");
        }
    }
    eprintln!("stage shots → {dir}");
}

/// Builds one page's staged widget and returns its arena-scoped
/// [`PaintList`] — the shared body for `stage_raster` and `lint_sweep`.
/// The scope walk is what lets the design-lint attribute findings to
/// widget paths; a bare `paint()` produces an empty scene.
fn stage_paint_list(
    p: &dyn widget_catalog::Page,
    props: &widget_catalog::PropValues,
    w: f32,
    h: f32,
    scale: f32,
) -> martensite::core::PaintList {
    use glam::Vec2;
    use martensite::core::{LayoutConstraints, LayoutContext, PaintList};
    use martensite::prelude::*;
    use widget_catalog::StageHost;

    let host = StageHost::new(p.build(props));
    let mut arena = WidgetArena::new();
    arena.set_theme(martensite::theme::tokens::default_dark());
    arena.set_scale_factor(scale);
    arena.set_text_painter(martensite::text_paint::shared_painter());
    let _measurer = arena
        .text_painter_shared()
        .map(martensite::core::paint::install_ambient_measurer);
    let mut hot = HotNode::default();
    hot.flags |= NodeFlags::VISIBLE | NodeFlags::HIT_TEST_ENABLED;
    let root = arena.insert_with_widget(hot, Box::new(host));
    let bounds = Rect::new(0.0, 0.0, w, h);
    if let Some((hot, cold)) = arena.get_both_mut(root) {
        cold.widget.measure(
            &mut LayoutContext { hot, scale },
            LayoutConstraints {
                min_size: Vec2::ZERO,
                max_size: Vec2::new(w, h),
            },
        );
        hot.bounds = bounds;
        cold.widget
            .layout(&mut LayoutContext { hot, scale }, bounds);
    }
    let mut list = PaintList::new();
    arena.build_paint_list(root, &mut list);
    paint_overlays(&mut arena, bounds, &mut list);
    list
}

/// Rasterizes one page's staged widget and returns the backend holding
/// the rendered frame — the shared body for `render_one_stage` and
/// `audit_props`. `.pixels()` is the RGBA8 buffer; `.pixmap()` encodes.
fn stage_raster(
    p: &dyn widget_catalog::Page,
    props: &widget_catalog::PropValues,
    w: f32,
    h: f32,
    scale: f32,
) -> martensite_render::TinySkiaBackend {
    use martensite_render::RenderBackend;

    let list = stage_paint_list(p, props, w, h, scale);
    let mut backend =
        martensite_render::TinySkiaBackend::new(w as u32, h as u32).expect("pixmap alloc");
    RenderBackend::render(&mut backend, &list);
    backend
}

/// Design-lints each page's staged frame via the same
/// `lint_paint_list` entry point the dev channel's `LintBridge` uses —
/// findings here match `lintpull` in a live app. Prints one
/// `LINT\t{i}\t{name}\t{severity}\t{rule}\t{path}\t{message}` row per
/// finding. Exit status is non-zero only on render panics.
fn lint_sweep(from: usize, to: usize, w: f32, h: f32, scale: f32) -> i32 {
    use std::panic::{catch_unwind, AssertUnwindSafe};
    use widget_catalog::PropValues;

    let pages = all_pages();
    let mut config = martensite::design_lint::LintConfig {
        scale_factor: scale,
        ..Default::default()
    };
    // Catalog chrome is a demo surface: the stage's dot-grid backdrop
    // trips edge-density and staged payload widgets saturate the
    // viewport surface. Path allows cover the chrome nodes only —
    // widget-scoped findings still report.
    for path in [
        "StageHost",
        "StageHost/Viewport",
        "StageHost/Viewport/Flex",
        "StageHost/Viewport/Flex/Flex",
    ] {
        config = config.with_allow(path, &["edge-density", "saturated-area-cap"]);
    }
    let mut findings = 0usize;
    let mut panics = 0usize;
    for (i, p) in pages.iter().enumerate() {
        if i < from || i > to {
            continue;
        }
        let name = p.meta().name;
        let props = PropValues::from_specs(p.props());
        let list = catch_unwind(AssertUnwindSafe(|| {
            stage_paint_list(p.as_ref(), &props, w, h, scale)
        }));
        let Ok(list) = list else {
            println!("LINT\t{i}\t{name}\tPANIC");
            panics += 1;
            continue;
        };
        let report = martensite::design_lint::lint_paint_list(&list, &config);
        for f in &report.findings {
            println!(
                "LINT\t{i}\t{name}\t{:?}\t{}\t{}\t{}",
                f.severity, f.rule, f.path, f.message
            );
            findings += 1;
        }
    }
    println!("LINT-SWEEP: {findings} findings, {panics} panics");
    i32::from(panics > 0)
}

/// Renders one page's staged widget to `dir/NNN_Family_Name.png`.
fn render_one_stage(dir: &str, w: f32, h: f32, scale: f32, i: usize, p: &dyn widget_catalog::Page) {
    use widget_catalog::PropValues;

    let props = PropValues::from_specs(p.props());
    let backend = stage_raster(p, &props, w, h, scale);
    let png = backend.pixmap().encode_png().expect("png encode");
    let m = p.meta();
    let file = format!(
        "{dir}/{i:03}_{}_{}.png",
        m.family.replace(' ', "_"),
        m.name.replace(' ', "_")
    );
    std::fs::write(&file, png).expect("png write");
}

/// Non-default probe values for a prop spec — each should change the
/// rendered frame if the page actually wires the prop.
fn probe_values(spec: &widget_catalog::PropSpec) -> Vec<widget_catalog::PropValue> {
    use widget_catalog::{PropSpec, PropValue};
    match *spec {
        PropSpec::Header { .. } | PropSpec::Probe { .. } => vec![],
        PropSpec::Bool { default, .. } => vec![PropValue::Bool(!default)],
        PropSpec::Int {
            min, max, default, ..
        } => [min, max, (min + max) / 2]
            .into_iter()
            .filter(|v| *v != default)
            .map(PropValue::Int)
            .collect(),
        PropSpec::Float {
            min, max, default, ..
        } => {
            let mut out: Vec<PropValue> = Vec::new();
            for v in [min, max, min + (max - min) * 0.37] {
                if (v - default).abs() > f64::EPSILON && !out.contains(&PropValue::Float(v)) {
                    out.push(PropValue::Float(v));
                }
            }
            out
        }
        PropSpec::Text { default, .. } => {
            let mut out = vec![PropValue::Text(
                if default == "AuditText" {
                    "Other"
                } else {
                    "AuditText"
                }
                .into(),
            )];
            if !default.is_empty() {
                out.push(PropValue::Text(String::new()));
            }
            out
        }
        PropSpec::Choice {
            options, default, ..
        } => (0..options.len())
            .filter(|i| *i != default)
            .map(PropValue::Choice)
            .collect(),
    }
}

/// Prop-effect audit: for each page, render the staged widget at
/// defaults, then re-render with each non-default probe value and count
/// differing pixels. A prop that changes zero pixels across all probes
/// is dead — the page ignores it or the widget doesn't honor it.
/// `page` limits to `Some(i)` single index or `Some(-1)` unused; range
/// slicing comes from `from..to` (inclusive).
/// Returns `(deads, panics)` so `--audit-gate` can compare against the
/// checked-in baseline; `deads` is a set of `WidgetName\tkey` pairs —
/// index-free so page reordering can't desync the baseline.
fn audit_props(
    from: usize,
    to: usize,
    w: f32,
    h: f32,
    scale: f32,
    probes: &[(String, String)],
) -> (std::collections::BTreeSet<String>, usize) {
    use std::panic::{catch_unwind, AssertUnwindSafe};
    use widget_catalog::{PropSpec, PropValues};

    let mut deads = std::collections::BTreeSet::new();
    let mut panics = 0usize;

    let pages = all_pages();
    // Per-page panic noise off unless AUDIT_VERBOSE=1 is set.
    let quiet = if std::env::var_os("AUDIT_VERBOSE").is_none() {
        let q = std::panic::take_hook();
        std::panic::set_hook(Box::new(|_| {}));
        Some(q)
    } else {
        None
    };
    for (i, p) in pages.iter().enumerate() {
        if i < from || i > to {
            continue;
        }
        let name = p.meta().name;
        let specs = p.props();
        // Spec-embedded probes: `PropSpec::Probe` annotations carry a
        // semantically-valid value per prop key.
        let embedded: Vec<(&'static str, &'static str)> = specs
            .iter()
            .filter_map(|s| match s {
                PropSpec::Probe { key, value } => Some((*key, *value)),
                _ => None,
            })
            .collect();
        let base_props = PropValues::from_specs(specs);
        let baseline = catch_unwind(AssertUnwindSafe(|| {
            stage_raster(p.as_ref(), &base_props, w, h, scale)
                .pixels()
                .to_vec()
        }));
        let Ok(baseline) = baseline else {
            println!("AUDIT\t{i}\t{name}\t(base)\tPANIC\t-1");
            panics += 1;
            continue;
        };
        // Noise floor: a second identical render should be byte-stable.
        let noise = catch_unwind(AssertUnwindSafe(|| {
            stage_raster(p.as_ref(), &base_props, w, h, scale)
                .pixels()
                .to_vec()
        }))
        .map(|again| {
            baseline
                .iter()
                .zip(&again)
                .zip(0..)
                .filter(|((a, b), _)| a != b)
                .map(|(_, i)| i / 4)
                .collect::<std::collections::BTreeSet<usize>>()
                .len()
        })
        .unwrap_or(0);
        println!("AUDIT\t{i}\t{name}\t(noise)\t{noise}");
        for spec in specs {
            if matches!(spec, PropSpec::Header { .. } | PropSpec::Probe { .. }) {
                continue;
            }
            let key = spec.key();
            let mut all_dead = true;
            let mut candidates = probe_values(spec);
            // Spec-embedded probes, then caller-supplied `--probe
            // key=value` — both feed values a generic probe can't
            // reach (dates, icon names, pattern strings).
            for (pk, pv) in embedded
                .iter()
                .map(|&(k, v)| (k, v))
                .chain(probes.iter().map(|(k, v)| (k.as_str(), v.as_str())))
            {
                if pk == key {
                    if let Some(parsed) = spec.parse_value(pv) {
                        candidates.push(parsed);
                    } else {
                        println!("AUDIT\t{i}\t{name}\t{key}\tprobe {pv:?}\tUNPARSEABLE");
                    }
                }
            }
            for v in candidates {
                let mut props = base_props.clone();
                props.set(key, v.clone());
                let rendered = catch_unwind(AssertUnwindSafe(|| {
                    stage_raster(p.as_ref(), &props, w, h, scale)
                        .pixels()
                        .to_vec()
                }));
                match rendered {
                    Err(_) => {
                        all_dead = false;
                        panics += 1;
                        println!("AUDIT\t{i}\t{name}\t{key}\t{v:?}\tPANIC");
                    }
                    Ok(buf) => {
                        let diff = baseline
                            .iter()
                            .zip(&buf)
                            .zip(0..)
                            .filter(|((a, b), _)| a != b)
                            .map(|(_, i)| i / 4)
                            .collect::<std::collections::BTreeSet<usize>>()
                            .len();
                        if diff > noise.max(4) {
                            all_dead = false;
                        }
                        println!("AUDIT\t{i}\t{name}\t{key}\t{v:?}\t{diff}");
                    }
                }
            }
            if all_dead {
                println!("DEAD\t{i}\t{name}\t{key}\t{label}", label = spec.label());
                deads.insert(format!("{name}\t{key}"));
            }
        }
    }
    if let Some(q) = quiet {
        std::panic::set_hook(q);
    }
    (deads, panics)
}

/// Gate mode: run the audit, then compare the dead set against
/// `audit_baseline.tsv` — the checked-in list of classified dead props
/// (`WidgetName<TAB>key<TAB>CLASS` per line, `#` comments). Exits
/// non-zero on any render panic, any dead prop that is NOT in the
/// baseline, or any baseline entry that no longer reproduces (a prop
/// that started working — stale baseline, must be pruned so the file
/// stays honest). `a11y_label` is exempt by convention: it is forwarded
/// on every page but only reaches AccessKit, never the raster.
fn audit_gate(from: usize, to: usize, w: f32, h: f32, scale: f32) -> i32 {
    let (deads, panics) = audit_props(from, to, w, h, scale, &[]);

    let path = concat!(env!("CARGO_MANIFEST_DIR"), "/audit_baseline.tsv");
    let raw = std::fs::read_to_string(path).unwrap_or_else(|e| {
        eprintln!("AUDIT-GATE: cannot read {path}: {e}");
        std::process::exit(2);
    });
    let baseline: std::collections::BTreeSet<String> = raw
        .lines()
        .map(str::trim)
        .filter(|l| !l.is_empty() && !l.starts_with('#'))
        .filter_map(|l| {
            let mut it = l.split('\t');
            let w = it.next()?;
            let k = it.next()?;
            Some(format!("{w}\t{k}"))
        })
        .collect();

    let exempt = |key: &str| key == "a11y_label";
    let unexplained: Vec<&String> = deads
        .iter()
        .filter(|d| !baseline.contains(*d) && !exempt(d.split('\t').nth(1).unwrap_or("")))
        .collect();
    // Staleness only means anything on a complete sweep — a scoped
    // run (`--page`/`--from`/`--to`) never renders the props other
    // baseline entries describe.
    let full_sweep = from == 0 && to == usize::MAX;
    let stale: Vec<&String> = baseline
        .iter()
        .filter(|b| full_sweep && !deads.contains(*b))
        .collect();

    println!();
    for s in &stale {
        println!("AUDIT-GATE\tSTALE\t{s}\tbaseline entry no longer dead — prune it");
    }
    for u in &unexplained {
        println!("AUDIT-GATE\tFAIL\t{u}\tdead prop with no baseline classification");
    }
    if panics > 0 {
        println!("AUDIT-GATE\tFAIL\t{panics} render panic(s)");
    }
    let failed = panics > 0 || !unexplained.is_empty() || !stale.is_empty();
    if failed {
        println!(
            "AUDIT-GATE\tFAIL\t{} unexplained dead(s), {panics} panic(s), {} stale baseline entr(ies)",
            unexplained.len(),
            stale.len()
        );
        1
    } else {
        println!(
            "AUDIT-GATE\tOK\t{} dead prop(s), all baseline-classified; 0 panics",
            deads.len()
        );
        0
    }
}
fn headless_size() -> (f32, f32) {
    std::env::var("MARTENSITE_HEADLESS_SIZE")
        .ok()
        .and_then(|s| {
            let (w, h) = s.split_once('x')?;
            Some((w.parse().ok()?, h.parse().ok()?))
        })
        .unwrap_or((1600.0, 1000.0))
}

fn headless_scale() -> f32 {
    std::env::var("MARTENSITE_HEADLESS_SCALE")
        .ok()
        .and_then(|s| s.parse().ok())
        .unwrap_or(1.0)
}

fn main() {
    let mut args: Vec<String> = std::env::args().skip(1).collect();

    // Windowed by default — the catalog is a GUI showcase.
    if args.is_empty() {
        widget_catalog::run().expect("windowed catalog");
        return;
    }
    if args.iter().any(|a| a == "--live-headless") {
        widget_catalog::run_live_headless().expect("live-headless catalog");
        return;
    }

    let mut page_sel: Option<usize> = None;
    let mut audit = false;
    let mut gate = false;
    let mut lint = false;
    let mut audit_from = 0usize;
    let mut audit_to = usize::MAX;
    let mut probes: Vec<(String, String)> = Vec::new();
    let mut props: Vec<(String, String)> = Vec::new();
    let mut zoom = 1.0f64;
    let mut rtl = false;
    let mut locale: Option<usize> = None;
    let mut args = args.drain(..);
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--filter" => {
                if let Some(query) = args.next() {
                    let q = query.to_lowercase();
                    let pages = all_pages();
                    println!("Widget Catalog: {} pages registered.", pages.len());
                    println!("Filter \"{query}\":");
                    for (i, p) in pages.iter().enumerate() {
                        let m = p.meta();
                        if m.name.to_lowercase().contains(&q)
                            || m.family.to_lowercase().contains(&q)
                        {
                            println!("  {i}\t{} [{}] (Role: {})", m.name, m.family, m.role);
                        }
                    }
                }
            }
            "--png" => {
                let path = args
                    .next()
                    .unwrap_or_else(|| "/tmp/widget_catalog.png".into());
                let (w, h) = headless_size();
                render_png(
                    &path,
                    w,
                    h,
                    headless_scale(),
                    &RenderOpts {
                        page: page_sel,
                        props: &props,
                        zoom,
                        rtl,
                        locale,
                    },
                );
            }
            "--stage-png" => {
                let dir = args.next().unwrap_or_else(|| "/tmp/stage_shots".into());
                render_stage_pngs(&dir, 720.0, 540.0, headless_scale(), page_sel);
            }
            "--audit-props" => audit = true,
            "--audit-gate" => gate = true,
            "--lint" => lint = true,
            "--from" => {
                if let Some(n) = args.next() {
                    audit_from = n.parse().unwrap_or(0);
                }
            }
            "--to" => {
                if let Some(n) = args.next() {
                    audit_to = n.parse().unwrap_or(usize::MAX);
                }
            }
            "--page" => {
                if let Some(n) = args.next() {
                    page_sel = n.parse().ok();
                }
            }
            "--prop" => {
                if let Some(kv) = args.next() {
                    if let Some((k, v)) = kv.split_once('=') {
                        props.push((k.to_string(), v.to_string()));
                    }
                }
            }
            "--probe" => {
                if let Some(kv) = args.next() {
                    if let Some((k, v)) = kv.split_once('=') {
                        probes.push((k.to_string(), v.to_string()));
                    }
                }
            }
            "--zoom" => {
                if let Some(z) = args.next() {
                    zoom = z.parse().unwrap_or(1.0);
                }
            }
            "--rtl" => rtl = true,
            "--locale" => {
                if let Some(n) = args.next() {
                    locale = n.parse().ok();
                }
            }
            _ => {}
        }
    }
    if lint {
        let (from, to) = match page_sel {
            Some(sel) => (sel, sel),
            None => (audit_from, audit_to),
        };
        std::process::exit(lint_sweep(from, to, 720.0, 540.0, headless_scale()));
    }
    if audit || gate {
        let (from, to) = match page_sel {
            Some(sel) => (sel, sel),
            None => (audit_from, audit_to),
        };
        let code = if gate {
            audit_gate(from, to, 720.0, 540.0, headless_scale())
        } else {
            audit_props(from, to, 720.0, 540.0, headless_scale(), &probes);
            0
        };
        std::process::exit(code);
    }
}
