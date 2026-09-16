//! Industrial Workstation — the v0.18.0 dogfooding example.
//!
//! A headless composition of the Martensite application stack the way a
//! real consumer assembles it: reactive signals driving state, theme
//! tokens, an arena of real widgets resolved by the Taffy layout engine,
//! a BSP docking tree, a virtualized 1,000,000-row `DataTable`, chart
//! series, a code editor model, a `MediaView` display surface (stub
//! surface — no video file required), keyboard focus navigation, shell
//! chrome material resolution, and a feature-gated devtools HUD.
//!
//! The example intentionally runs without a display server: every
//! subsystem is exercised through its headless model APIs so
//! `cargo run -p industrial_dashboard` works in CI. `FRICTION` comments
//! mark every API awkwardness found while wiring the demo — they are
//! pre-freeze fix candidates, see docs/milestones/v0.18.0 §4.6.
//!
//! Run with the devtools HUD enabled:
//! `cargo run -p industrial_dashboard --features devtools`

use martensite::blessed::{
    data_table::{ColumnConfig, ColumnSort, KeyAction, RowFilter},
    Chart, CodeEditor, DataTable, DockPanel, DockTree, LineSeries, Point, SplitDirection,
};
use martensite::prelude::*;

#[cfg(feature = "devtools")]
use martensite::devtools::hud::{DiagnosticHud, FrameTiming};

// ---------------------------------------------------------------------------
// FRICTION LOG (pre-freeze fix candidates)
// ---------------------------------------------------------------------------
//
// F1. `ReactiveRuntime` was not in `martensite::prelude` — only
//     `Signal`/`Memo` were. `Signal::new()` silently binds to a global
//     default runtime while `runtime.create_signal()` binds to an explicit
//     one; signals on different runtimes cannot form dependencies.
//     → RESOLVED v0.18.0: prelude now re-exports `ReactiveRuntime`,
//       `ReactiveError`, `Effect`, and the free `create_signal`/
//       `create_memo`/`create_effect`/`batch`/`flush` fns.
//
// F2. `martensite::prelude` exported `Oklab` but not `Theme`/`TokenKey`/
//     `ThemeToken` — the two things a consumer actually sets tokens with.
//     → RESOLVED v0.18.0: `Theme`/`ThemeToken`/`TokenKey` are in the prelude
//       (and `martensite::theme::TokenKey` resolves via the theme root).
//
// F3. `martensite-blessed`, `martensite-shell`, `martensite-devtools` were
//     not re-exported from the `martensite` facade at all (unlike the other
//     subsystems). → RESOLVED v0.18.0: `martensite::blessed`,
//     `martensite::shell`, `martensite::devtools` aliases added; this demo
//     now imports through them.
//
// F4. `DockPanel::new(widget_id: u64, ...)` takes a raw `u64` while every
//     other subsystem passes `WidgetId` around. The bridge is
//     `WidgetId::to_u64()`/`from_u64()` — it works, but the type-safe
//     boundary is on the consumer. → DEFERRED to pre-RC: accepting
//     `WidgetId` is a signature change; tracked in
//     docs/API_FREEZE_AUDIT.md "Known API friction".
//
// F5. Name/unit collision: `martensite_core::Rect{origin:Vec2,size:Vec2}`
//     (f32) vs `martensite_blessed::Rect{x:f64,y:f64,width,height}` — two
//     `Rect` types with different fields AND different float widths.
//     Importing both in one file forces an alias. → DEFERRED to pre-RC:
//     unifying or renaming is breaking; tracked in the audit doc.
//
// F6. `ColumnConfig` is the odd one out API-wise: `Text`/`Flex`/`Container`/
//     `MediaView` all have builder methods (`fn font_size(self)->Self`), but
//     `ColumnConfig::set_width/set_sortable` take `&mut self` and return `()`,
//     forcing `let mut col` ceremony. → DEFERRED to pre-RC: `with_*`
//     builders are additive but were kept out of the freeze batch; tracked
//     in the audit doc.
//
// F7. `SplitDirection::Horizontal` means "horizontal divider → children
//     stacked top/bottom" — correct once read, but every first-time reader
//     guesses "horizontal split = side by side". → RESOLVED v0.18.0 (docs):
//     the enum's rustdoc now calls out that the variant names the divider
//     line's orientation, not the child arrangement.
//
// F8. `LayoutEngine::compute` takes a taffy `NodeId` while `register_node`
//     is keyed by `WidgetId` — two id spaces in adjacent calls; you must
//     `lookup_node(widget_id)` before `compute`. → DEFERRED to pre-RC: a
//     `compute_for_widget(WidgetId, ...)` convenience is tracked in the
//     audit doc. (`compute_with_widgets` already covers the full-pipeline
//     case used below.)
//
// F9. `FocusManager::set_focus` silently does nothing if the HotNode lacks
//     `NodeFlags::FOCUSABLE` — no error, no log. → RESOLVED v0.18.0:
//     `try_set_focus(arena, id) -> bool` reports whether focus moved, and
//     `set_focus`'s rustdoc now flags the silent no-op.
//
// F10. `DataTable::handle_key` takes a framework `KeyAction` enum, not a
//     winit `KeyEvent` — every consumer writes the same keymap.
//     → RESOLVED v0.18.0: `KeyAction::from_key_name(key, shift, ctrl)`
//      maps the framework's logical key names (winit `NamedKey` strings)
//      to actions without a winit dependency edge.
//
// F11. Re-export shadowing: `martensite::layout::Size` is the crate's own
//     non-generic geometry Size, which shadows `taffy::Size<T>` from the
//     `pub use taffy::prelude::*` glob. There is NO path through the facade
//     to name `taffy::Size<Dimension>`/`taffy::Size<AvailableSpace>` —
//     `Style.size` and `compute(available)` become unnameable without a
//     direct taffy dep. Workaround used below: `constraints_to_available`
//     + `Style::default()`. → DEFERRED to pre-RC: renaming geometry `Size`
//     is breaking; tracked in the audit doc.
//
// F12. `resolve_backdrop_material`/`resolve_vibrancy_material` were `pub`
//     but NOT re-exported at the `martensite_shell` root — only reachable
//     as `martensite_shell::backdrop::resolve_*`. → RESOLVED v0.18.0: both
//     are re-exported at the shell root (and reachable via
//     `martensite::shell::resolve_*` through the facade).
//
// F13. `StubBackdropController::mode()` is a trait method
//     (`BackdropController`), so `ctrl.mode()` fails to resolve until the
//     trait is imported — fine Rust, but the stub's public surface has no
//     inherent methods at all, which trips consumers copying doctest
//     patterns. → PARTIALLY ADDRESSED: `BackdropController` is already at
//     the shell root (now also `martensite::shell::BackdropController`);
//     inherent-method ergonomics deferred to pre-RC.
//
// F14. `DataTable::sort_by(col, direction, cmp)` inverts the comparator
//     for `Descending` — so the comparator must be written ASCENDING
//     (`a.cmp(b)`), and a "natural" `b.cmp(a)` + `Descending` silently
//     produces ascending output. This demo's first version shipped exactly
//     that bug. → RESOLVED v0.18.0 (docs): `sort_by` rustdoc now states
//     the comparator always encodes ascending order.
//
// F15. (Remark on F9 — docs-visibility note, not a doc gap.)
//     `is_focusable_target` requires FOCUSABLE **and** VISIBLE flags —
//     `FOCUSABLE` alone silently fails `set_focus`. → RESOLVED v0.18.0
//     with F9: `try_set_focus` makes the rejection observable.
// ---------------------------------------------------------------------------

/// One row of the process-metrics table. Kept POD so 1M rows stay cheap.
#[derive(Clone, Debug)]
struct MetricRow {
    pid: u32,
    cpu_milli: u32,
    mem_kib: u32,
    alert: bool,
}

fn gen_rows(n: usize) -> Vec<MetricRow> {
    (0..n)
        .map(|i| MetricRow {
            pid: 1000 + (i as u32) * 7 % 60000,
            cpu_milli: ((i * 37) % 100_000) as u32,
            mem_kib: 4096 + ((i * 53) % 2_000_000) as u32,
            alert: i % 977 == 0,
        })
        .collect()
}

/// Builds the widget arena: a column flex holding header + content widgets.
/// Returns the arena plus the ids of focusable widgets for the focus pass.
fn build_widget_tree() -> (WidgetArena, WidgetId, Vec<WidgetId>) {
    let mut arena = WidgetArena::new();

    // F2 resolved: TokenKey is in the prelude; `default_dark` still lives
    // under theme::tokens.
    let theme = martensite::theme::tokens::default_dark();
    let bg = theme
        .color(TokenKey::BackgroundColor)
        .unwrap_or(Oklab::from_srgb(0.08, 0.09, 0.11));

    let root = arena.insert_with_widget(
        HotNode::default(),
        Box::new(
            Flex::column().gap(8.0).child(
                Container::new()
                    .padding_uniform(12.0)
                    .background(bg)
                    .child(Text::new("INDUSTRIAL WORKSTATION").font_size(18.0)),
            ),
        ),
    );

    // Focusable leaf widgets for the keyboard-navigation pass (F9/F15:
    // the FOCUSABLE flag must be set on the HotNode before insert, and
    // try_set_focus below now reports rejection).
    let mut focusable = Vec::new();
    for (name, widget) in [
        (
            "grid_focus",
            Box::new(Text::new("[grid]")) as Box<dyn Widget>,
        ),
        ("media_focus", Box::new(Text::new("[media]"))),
        ("editor_focus", Box::new(Text::new("[editor]"))),
    ] {
        let mut hot = HotNode::default();
        // F15: FOCUSABLE alone is not enough — VISIBLE is also required.
        hot.flags |= NodeFlags::FOCUSABLE | NodeFlags::VISIBLE;
        let mut cold = ColdNode::new(widget);
        cold.debug_name = Some(name);
        let id = arena.insert(hot, cold);
        arena
            .append_child(root, id)
            .expect("append focusable child");
        focusable.push(id);
    }
    (arena, root, focusable)
}

/// Exercises the widget-aware two-pass Taffy layout engine over the arena
/// tree: syncs the Taffy topology from the arena, measures leaf widgets
/// through `Widget::measure`, computes, then applies bounds back via
/// `Widget::layout` — the full sync→measure→apply round-trip.
fn run_layout_pass(arena: &mut WidgetArena, root: WidgetId) {
    // F11: `martensite::layout::Size` (geometry) shadows `taffy::Size<T>` —
    // `Style.size` and `compute(available)` cannot name their Size types via
    // the facade. `constraints_to_available` is the facade-side escape hatch.
    use martensite::layout::{constraints_to_available, Constraints, Display, LayoutEngine, Style};
    let mut engine = LayoutEngine::new();
    // F8 note: `compute` takes a taffy NodeId while `register_node` is keyed
    // by WidgetId — but `compute_with_widgets` is the WidgetId-keyed entry
    // point that already exists for the whole pipeline, so no id-space
    // bridging is needed here. Pre-registering the root with an explicit
    // style is fine: `sync_from_arena` preserves already-registered styles
    // and defaults only the new ones.
    engine
        .register_node(
            root,
            Style {
                display: Display::Flex,
                ..Default::default()
            },
        )
        .expect("register root");
    engine
        .compute_with_widgets(
            arena,
            root,
            constraints_to_available(Constraints::tight(1600.0, 900.0)),
        )
        .expect("compute layout");
    println!(
        "  layout: {} nodes synced+measured, bounds applied to arena",
        engine.node_count()
    );
}

/// Builds the BSP docking tree mirroring a real workstation layout:
/// left = metrics grid, right column = media surface over code editor.
fn build_dock_tree(widget_ids: &[u64]) -> DockTree {
    let mut tree = DockTree::with_capacity(32);
    let root = tree.insert_root(DockPanel::new(widget_ids[0], "Process Grid"));
    // F7: Vertical divider → left/right children.
    let (_left, right) = tree
        .split_leaf(
            root,
            SplitDirection::Vertical,
            0.62,
            DockPanel::new(widget_ids[1], "Telemetry"),
        )
        .expect("split vertical");
    // F7 again: Horizontal divider → top/bottom children.
    tree.split_leaf(
        right,
        SplitDirection::Horizontal,
        0.55,
        DockPanel::new(widget_ids[2], "Editor"),
    )
    .expect("split horizontal");
    tree
}

/// Exercises the virtualized table: columns, sort, filter, selection, keys.
fn run_grid_pass(rows: Vec<MetricRow>) {
    let mut table = DataTable::new(rows, 22.0);
    table.set_viewport_height(880.0);

    // F6: ColumnConfig has no builder API — &mut ceremony per column.
    let mut c_pid = ColumnConfig::new(90.0);
    c_pid.set_sortable(true);
    let mut c_cpu = ColumnConfig::new(120.0);
    c_cpu.set_sortable(true);
    c_cpu.set_resizable(true);
    let mut c_mem = ColumnConfig::new(140.0);
    c_mem.set_resizable(true);
    let mut c_alert = ColumnConfig::new(80.0);
    c_alert.set_sortable(false);
    table.set_columns(vec![c_pid, c_cpu, c_mem, c_alert]);

    println!(
        "  grid: {} rows stored, {} visible @22px/880px",
        table.row_count(),
        table.visible_range().len()
    );

    // Sort by CPU descending — index-vector sort, rows never move.
    // F14: comparator is always the ASCENDING order; Descending flips it.
    table.sort_by(1, ColumnSort::Descending, |a, b| {
        a.cpu_milli.cmp(&b.cpu_milli)
    });
    // Render the top visible row the way a cell painter would: every
    // column config maps to a row field.
    if let Some((_, r)) = table.visible_rows().next() {
        println!(
            "  grid: top row after sort → pid={} cpu={:.1}% mem={}KiB alert={}",
            r.pid,
            r.cpu_milli as f64 / 1000.0,
            r.mem_kib,
            r.alert
        );
    }

    // Filter to alerting rows only, then keyboard-navigate the result.
    table.set_filter(RowFilter::new(|r: &MetricRow| r.alert));
    println!(
        "  grid: {} alert rows after filter",
        table.display_row_count()
    );
    // F10 resolved: KeyAction::from_key_name maps framework key names.
    // F16 documented: SelectionModel::extend_to builds a Range over raw
    // *storage* row indices — after sort+filter reorder the display, a
    // shift-range selects mostly filtered-out, never-displayed rows.
    assert_eq!(
        KeyAction::from_key_name("PageDown", false, false),
        Some(KeyAction::PageDown)
    );
    table.handle_key(KeyAction::PageDown);
    table.handle_key(KeyAction::ShiftDown);
    table.handle_key(KeyAction::ShiftDown);
    println!(
        "  grid: focused_row={:?}, selected={} after PgDn+Shift↓×2 (storage-index range — F16)",
        table.focused_row(),
        table.selection().selected_count()
    );
    table.clear_filter();
    table.clear_sort();
}

/// Exercises chart series + auto-bounds + projection.
fn run_chart_pass() {
    let mut chart = Chart::new();
    chart.add_line(LineSeries::new(
        (0..120)
            .map(|i| Point {
                x: f64::from(i),
                y: 50.0 + 40.0 * (f64::from(i) * 0.11).sin(),
            })
            .collect(),
    ));
    if let Some(bounds) = chart.bounds() {
        let p = Chart::project(Point { x: 60.0, y: 50.0 }, bounds, 800.0, 400.0);
        println!(
            "  chart: bounds x[{:.0},{:.0}] y[{:.1},{:.1}], mid-point→({:.0},{:.0})px",
            bounds.x_min, bounds.x_max, bounds.y_min, bounds.y_max, p.x, p.y
        );
    }
}

/// Exercises the code editor model: insert + syntax highlight spans.
fn run_editor_pass() {
    let mut editor = CodeEditor::new("fn main() {\n    let cpu = Signal::new(0.42);\n}");
    editor.insert("\n    // hot path");
    let spans = editor.highlight_line(1);
    println!(
        "  editor: {} lines, line-1 highlight spans = {}",
        editor.lines().len(),
        spans.len()
    );
}

/// Exercises MediaView as a display surface with a stub (mock) surface —
/// no video file and no decoder feature required.
fn run_media_pass() -> f32 {
    let surface = martensite::media::surface::VideoSurface::new_mock(
        1920,
        1080,
        martensite::media::surface::VideoPixelFormat::Nv12,
    );
    let view = MediaView::new()
        .with_fit(VideoFit::Contain)
        .with_surface(surface);
    let ratio = view.effective_aspect_ratio();
    let dest = view.compute_dest_rect(
        martensite::core::Rect::new(0.0, 0.0, 800.0, 500.0),
        VideoFit::Contain,
    );
    println!(
        "  media: mock 1920x1080 NV12 surface, ratio {ratio:.3}, contain dest {:.0}x{:.0}",
        dest.size.x, dest.size.y
    );
    ratio
}

/// Exercises keyboard focus traversal across the focusable widgets.
fn run_focus_pass(arena: &mut WidgetArena, root: WidgetId, focusable: &[WidgetId]) {
    use martensite::focus::{FocusManager, TabNavigation};
    let mut fm = FocusManager::new();
    fm.set_root(root);
    // F9 resolved: try_set_focus reports whether focus actually moved;
    // set_focus still exists for the fire-and-forget path.
    assert!(fm.try_set_focus(arena, focusable[0]));
    let mut order = vec![fm.current_focus()];
    for _ in 0..focusable.len() {
        order.push(fm.tab(arena, TabNavigation::Forward));
    }
    let hits = order.iter().filter(|o| o.is_some()).count();
    println!(
        "  focus: tab-chain walked {hits}/{} stops across {} focusable widgets",
        focusable.len() + 1,
        focusable.len()
    );
}

/// Exercises shell chrome material resolution (headless stub — no DWM/
/// NSVisualEffectView involved).
fn run_shell_pass() {
    use martensite::theme::tokens::default_dark;
    // F12 resolved: resolver fns are re-exported at the shell root.
    let material = martensite::shell::resolve_backdrop_material(&default_dark());
    // F13: `mode()` lives on the `BackdropController` trait — import required.
    use martensite::shell::BackdropController;
    let ctrl = martensite::shell::StubBackdropController::new();
    println!(
        "  shell: dark theme → backdrop material {material:?}, stub controller mode {:?}",
        ctrl.mode()
    );
}

/// Feature-gated devtools HUD: records simulated frame timings.
#[cfg(feature = "devtools")]
fn run_devtools_pass() {
    let mut hud = DiagnosticHud::new();
    hud.toggle(); // default off; flip on
    for i in 0..60u64 {
        hud.record_frame(FrameTiming::from_ms(
            0.8 + (i % 5) as f64 * 0.05,
            1.1,
            0.3,
            2.4 + (i % 7) as f64 * 0.1,
        ));
    }
    let avg = hud.histogram().average();
    println!(
        "  devtools: HUD enabled={}, {} frames, avg total {:.2}ms",
        hud.is_enabled(),
        hud.histogram().len(),
        avg.total_time_ns as f64 / 1_000_000.0
    );
}

fn main() {
    println!("Industrial Workstation — Martensite dogfooding build-up\n");

    // -- 1. Signals-driven state ------------------------------------------
    // F1 resolved: Signal::new still binds to the implicit global runtime,
    // but ReactiveRuntime + the explicit create_* fns are now in the
    // prelude — an explicit runtime is one import away.
    let cpu_load = Signal::new(0.42_f64);
    let load_pct = Memo::new({
        let s = cpu_load.clone();
        move || format!("{:.0}%", s.get() * 100.0)
    });
    println!(
        "  signal: cpu_load={} → memo load_pct={}",
        cpu_load.get(),
        load_pct.get()
    );
    cpu_load.set(0.71);
    println!(
        "  signal: after set(0.71) memo re-evaluates → {}",
        load_pct.get()
    );

    // -- 2. Widget arena + real widgets ------------------------------------
    let (mut arena, root, focusable) = build_widget_tree();
    println!(
        "  arena: {} nodes (root + {} focusable)",
        arena.len(),
        focusable.len()
    );

    // -- 3. Taffy two-pass layout ------------------------------------------
    run_layout_pass(&mut arena, root);

    // -- 4. BSP docking -----------------------------------------------------
    // F4: DockPanel wants u64; WidgetId::to_u64() is the manual bridge.
    let panel_ids: Vec<u64> = focusable.iter().map(|id| id.to_u64()).collect();
    let dock = build_dock_tree(&panel_ids);
    let rects: Vec<_> = dock
        .panel_rects(martensite::blessed::Rect::new(0.0, 0.0, 1600.0, 900.0))
        .collect();
    println!(
        "  dock: {} panels across {} nodes",
        dock.panel_count(),
        dock.node_count()
    );
    for (nid, r) in &rects {
        let panel = dock.node(*nid).unwrap();
        if let martensite::blessed::DockNode::Leaf { panel } = panel {
            println!(
                "    - \"{}\" @ {:.0}x{:.0}+{:.0},{:.0}",
                panel.title(),
                r.width,
                r.height,
                r.x,
                r.y
            );
        }
    }

    // -- 5. Virtualized 1M-row grid ----------------------------------------
    run_grid_pass(gen_rows(1_000_000));

    // -- 6. Chart -----------------------------------------------------------
    run_chart_pass();

    // -- 7. Code editor ------------------------------------------------------
    run_editor_pass();

    // -- 8. Media display surface (stub) -------------------------------------
    run_media_pass();

    // -- 9. Keyboard focus navigation ----------------------------------------
    run_focus_pass(&mut arena, root, &focusable);

    // -- 10. Shell chrome -----------------------------------------------------
    run_shell_pass();

    // -- 11. Devtools HUD (feature-gated) ------------------------------------
    #[cfg(feature = "devtools")]
    run_devtools_pass();
    #[cfg(not(feature = "devtools"))]
    println!("  devtools: disabled (run with --features devtools to enable HUD)");

    println!("\nWorkstation composition complete — all subsystems exercised headless.");
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn signal_memo_chain() {
        let s = Signal::new(0.5_f64);
        let m = Memo::new({
            let s = s.clone();
            move || s.get() * 2.0
        });
        assert_eq!(m.get(), 1.0);
        s.set(0.8);
        assert_eq!(m.get(), 1.6);
    }

    #[test]
    fn dock_tree_rects_cover_surface() {
        let dock = build_dock_tree(&[1, 2, 3]);
        let total: f64 = dock
            .panel_rects(martensite::blessed::Rect::new(0.0, 0.0, 1600.0, 900.0))
            .map(|(_, r)| r.width * r.height)
            .sum();
        assert!((total - 1600.0 * 900.0).abs() < 1.0);
    }

    #[test]
    fn million_row_grid_virtualizes() {
        let mut t = DataTable::new(gen_rows(1_000_000), 22.0);
        t.set_viewport_height(880.0);
        assert_eq!(t.visible_range().len(), 40);
        t.set_scroll_offset(10_000_000.0);
        assert!(t.visible_range().start > 0);
    }

    #[test]
    fn focus_tab_chain_cycles() {
        let (mut arena, root, focusable) = build_widget_tree();
        use martensite::focus::{FocusManager, TabNavigation};
        let mut fm = FocusManager::new();
        fm.set_root(root);
        fm.set_focus(&mut arena, focusable[0]);
        assert_eq!(fm.current_focus(), Some(focusable[0]));
        let next = fm.tab(&arena, TabNavigation::Forward);
        assert!(next.is_some());
    }

    #[test]
    fn media_stub_surface_computes_dest() {
        assert!(run_media_pass() > 1.0);
    }
}
