//! Comprehensive integration tests for the dev-mode error surface (WP-09 / W7).
//!
//! Verifies:
//! 1. Tier 1 inline annotations (diagonal hatch tape, clipped text underline, corner ticks).
//! 2. Tier 2 diagnostics overlay (HUD/inspector tab listing, deduplication, frame-age badges, path reveal).
//! 3. Tier 3 structured panic (crash bundle, copy report formatting, continue vs restart policy, error card).
//! 4. Anti-noise rules: zero-cost when clean, severity floor, 500-violation collapsed summary.

use glam::Vec2;
use kurbo::Rect;
use martensite_core::{PaintCommand, PaintList, WidgetId};
use martensite_devtools::error_surface::{
    clear_in_flight, current_in_flight, scope_in_flight, set_dev_panic_enabled, set_in_flight,
    ClippedTextDiagnostic, DiagnosticSeverity, ErrorSurface, LayoutDiagnostic, LintDiagnostic,
    PaintErrorDiagnostic, PanicPhase, RecoveryPolicy, DEFAULT_ERROR_RED, DEFAULT_STRIPE_BLACK,
    DEFAULT_STRIPE_YELLOW,
};
use martensite_devtools::event_ledger::{Disposition, EventKind, EventLedger, EventRecord, Point};
use martensite_devtools::Axis;

#[test]
fn test_layout_diagnostic_creation_and_overflow_calculations() {
    let node = WidgetId::from_parts(1, 1);

    // 1. Horizontal overflow.
    let diag_h = LayoutDiagnostic::new(
        node,
        "Root/Flex/Row",
        Rect::new(0.0, 0.0, 200.0, 100.0),
        Vec2::new(200.0, 100.0),
        Vec2::new(245.5, 100.0),
    );
    assert!(diag_h.has_overflow());
    assert_eq!(diag_h.axis, Axis::Horizontal);
    assert_eq!(diag_h.overflow_delta.x, 45.5);
    assert_eq!(diag_h.overflow_delta.y, 0.0);
    assert_eq!(diag_h.max_overflow(), 45.5);
    assert!(diag_h.one_line_cause().contains("45.5px along Horizontal"));

    // 2. Vertical overflow.
    let diag_v = LayoutDiagnostic::new(
        node,
        "Root/Column",
        Rect::new(0.0, 0.0, 100.0, 300.0),
        Vec2::new(100.0, 300.0),
        Vec2::new(100.0, 360.0),
    );
    assert!(diag_v.has_overflow());
    assert_eq!(diag_v.axis, Axis::Vertical);
    assert_eq!(diag_v.overflow_delta.x, 0.0);
    assert_eq!(diag_v.overflow_delta.y, 60.0);
    assert_eq!(diag_v.max_overflow(), 60.0);
    assert!(diag_v.one_line_cause().contains("60.0px along Vertical"));

    // 3. Both axes overflow.
    let diag_both = LayoutDiagnostic::new(
        node,
        "Root/Grid/Cell",
        Rect::new(0.0, 0.0, 50.0, 50.0),
        Vec2::new(50.0, 50.0),
        Vec2::new(75.0, 80.0),
    );
    assert!(diag_both.has_overflow());
    assert_eq!(diag_both.axis, Axis::Both);
    assert_eq!(diag_both.overflow_delta.x, 25.0);
    assert_eq!(diag_both.overflow_delta.y, 30.0);
    assert_eq!(diag_both.max_overflow(), 30.0);

    // 4. Clean layout (no overflow).
    let diag_clean = LayoutDiagnostic::new(
        node,
        "Root/Card",
        Rect::new(0.0, 0.0, 100.0, 100.0),
        Vec2::new(100.0, 100.0),
        Vec2::new(80.0, 90.0),
    );
    assert!(!diag_clean.has_overflow());
    assert_eq!(diag_clean.overflow_delta, Vec2::ZERO);
}

#[test]
fn test_tier1_layout_overflow_tape_metadata_and_rendering() {
    let diag = LayoutDiagnostic::new(
        None,
        "App/Sidebar/Panel",
        Rect::new(10.0, 20.0, 210.0, 120.0),
        Vec2::new(200.0, 100.0),
        Vec2::new(242.0, 100.0),
    );

    let tape = diag.to_tape_metadata(12.0);
    assert_eq!(tape.axis, Axis::Horizontal);
    assert_eq!(tape.overflow_px, 42.0);
    assert_eq!(tape.label_text, "+42.0px");
    assert_eq!(tape.stripe_color_1, DEFAULT_STRIPE_YELLOW);
    assert_eq!(tape.stripe_color_2, DEFAULT_STRIPE_BLACK);

    // The edge rect for horizontal overflow is placed along the right edge.
    assert_eq!(tape.edge_rect.x1, 210.0);
    assert_eq!(tape.edge_rect.x0, 210.0 - 12.0);
    assert_eq!(tape.edge_rect.y0, 20.0);
    assert_eq!(tape.edge_rect.y1, 120.0);

    // Render tape into PaintList.
    let mut paint = PaintList::new();
    tape.render(&mut paint);

    assert!(!paint.is_empty());
    // Verify tape emitted background fill, clipping, paths, and badge text.
    let has_clip = paint
        .commands
        .iter()
        .any(|c| matches!(c, PaintCommand::ClipRect(..)));
    let has_pop = paint
        .commands
        .iter()
        .any(|c| matches!(c, PaintCommand::PopClip));
    let has_path = paint
        .commands
        .iter()
        .any(|c| matches!(c, PaintCommand::FillPath(..)));
    let has_badge_text = paint.commands.iter().any(|c| {
        if let PaintCommand::DrawText(_, text, _, _) = c {
            text.contains("+42.0px")
        } else {
            false
        }
    });

    assert!(has_clip, "tape must push clip");
    assert!(has_pop, "tape must pop clip");
    assert!(has_path, "tape must draw diagonal stripe paths");
    assert!(has_badge_text, "tape must draw amount badge text");
}

#[test]
fn test_tier1_clipped_text_indicator_and_rendering() {
    let diag = ClippedTextDiagnostic::new(
        Some(WidgetId::from_parts(3, 1)),
        "App/TitleBar/Label",
        "Dashboard Overview - Industrial Metrics",
        Rect::new(10.0, 10.0, 110.0, 30.0),
        48.0,
    );

    assert_eq!(diag.deficit_px, 48.0);
    assert!(diag.one_line_cause().contains("truncated by 48.0px"));

    let entry = diag.to_entry(1);
    assert_eq!(entry.node_path, "App/TitleBar/Label");
    assert_eq!(entry.severity, DiagnosticSeverity::Error);

    let mut paint = PaintList::new();
    diag.render_marker(&mut paint);

    // Should emit underline rect and deficit label.
    let has_underline = paint.commands.iter().any(|c| {
        if let PaintCommand::FillRect(_, color) = c {
            *color == DEFAULT_ERROR_RED
        } else {
            false
        }
    });
    let has_deficit_text = paint.commands.iter().any(|c| {
        if let PaintCommand::DrawText(_, text, _, _) = c {
            text.contains("clip -48px")
        } else {
            false
        }
    });

    assert!(has_underline);
    assert!(has_deficit_text);
}

#[test]
fn test_tier1_lint_error_corner_ticks_and_severity_floor() {
    let mut surface = ErrorSurface::new();

    // 1. Error severity: SHOULD produce corner tick.
    surface.record_lint_diagnostic(LintDiagnostic::new(
        None,
        "App/Button[1]",
        "wcag-contrast",
        "Contrast fails 4.5:1",
        DiagnosticSeverity::Error,
        Rect::new(0.0, 0.0, 100.0, 40.0),
    ));

    // 2. Warn severity: should NOT produce inline corner tick (anti-noise rule).
    surface.record_lint_diagnostic(LintDiagnostic::new(
        None,
        "App/Button[2]",
        "touch-target",
        "Target size slightly small",
        DiagnosticSeverity::Warn,
        Rect::new(0.0, 50.0, 100.0, 90.0),
    ));

    // 3. Info severity: should NOT produce inline corner tick.
    surface.record_lint_diagnostic(LintDiagnostic::new(
        None,
        "App/Button[3]",
        "typography",
        "Font scale info",
        DiagnosticSeverity::Info,
        Rect::new(0.0, 100.0, 100.0, 140.0),
    ));

    let annotations = surface.collect_tier1_annotations();
    // Only the Error severity diagnostic should appear inline!
    assert_eq!(annotations.len(), 1);
    assert_eq!(annotations[0].kind_name(), "corner_tick");

    // In Tier 2 overlay, all three should appear.
    assert_eq!(surface.tier2_entries().len(), 3);
    assert_eq!(
        surface
            .tier2_entries_by_severity(DiagnosticSeverity::Warn)
            .len(),
        2
    );
    assert_eq!(
        surface
            .tier2_entries_by_severity(DiagnosticSeverity::Error)
            .len(),
        1
    );
}

#[test]
fn test_tier2_diagnostics_overlay_and_deduplication() {
    let mut surface = ErrorSurface::new();

    let diag = LayoutDiagnostic::new(
        Some(WidgetId::from_parts(4, 1)),
        "App/Content/Table",
        Rect::new(0.0, 0.0, 500.0, 300.0),
        Vec2::new(500.0, 300.0),
        Vec2::new(550.0, 300.0),
    );

    // Frame 1: first observation.
    surface.set_current_frame(1);
    surface.record_layout_diagnostic(diag.clone());

    let entries = surface.tier2_entries();
    assert_eq!(entries.len(), 1);
    assert_eq!(entries[0].occurrences, 1);
    assert_eq!(entries[0].first_seen_frame, 1);
    assert_eq!(entries[0].last_seen_frame, 1);
    assert_eq!(entries[0].frame_age(1), 1);
    assert_eq!(entries[0].age_badge(1), "for 1f");

    // Simulate 240 frames of the same violation.
    surface.clear_frame();
    surface.set_current_frame(240);
    surface.record_layout_diagnostic(diag);

    // Still only 1 entry in the overlay (deduplicated across frames)!
    let entries = surface.tier2_entries();
    assert_eq!(entries.len(), 1);
    assert_eq!(entries[0].occurrences, 2);
    assert_eq!(entries[0].first_seen_frame, 1);
    assert_eq!(entries[0].last_seen_frame, 240);
    assert_eq!(entries[0].frame_age(240), 240);
    assert_eq!(entries[0].age_badge(240), "for 240f");
}

#[test]
fn test_tier2_round_trip_inspector_reveal() {
    // Acceptance gate 5: "Every diagnostic entry carries a linkable node path resolvable by the inspector's reveal (round-trip test)."
    let mut surface = ErrorSurface::new();

    let id_a = WidgetId::from_parts(10, 1);
    let id_b = WidgetId::from_parts(11, 1);
    let _id_c = WidgetId::from_parts(12, 1);

    let path_a = "Root/Sidebar/NavButton[1]";
    let path_b = "Root/Main/Header/UserBadge";
    let path_c = "Root/Footer/StatusText";

    surface.record_layout_diagnostic(LayoutDiagnostic::new(
        id_a,
        path_a,
        Rect::ZERO,
        Vec2::new(100.0, 30.0),
        Vec2::new(120.0, 30.0),
    ));

    surface.record_clipped_text(ClippedTextDiagnostic::new(
        id_b,
        path_b,
        "Truncated text",
        Rect::ZERO,
        15.0,
    ));

    surface.record_paint_error(PaintErrorDiagnostic::new(
        path_c,
        "Failed to render status icon",
        None,
    ));

    // Round-trip resolve paths.
    let entry_a = surface
        .find_by_node_path(path_a)
        .expect("path A must resolve");
    assert_eq!(entry_a.node_id, Some(id_a));
    assert!(entry_a.one_line_cause.contains("Layout overflow"));

    let entry_b = surface
        .find_by_node_path(path_b)
        .expect("path B must resolve");
    assert_eq!(entry_b.node_id, Some(id_b));
    assert!(entry_b.one_line_cause.contains("Clipped text"));

    let entry_c = surface
        .find_by_node_path(path_c)
        .expect("path C must resolve");
    assert_eq!(entry_c.node_path, path_c);
    assert!(entry_c.one_line_cause.contains("Paint error"));

    // Non-existent path returns None.
    assert!(surface.find_by_node_path("NonExistent/Node").is_none());
}

#[test]
fn test_anti_noise_zero_diagnostics_zero_cost() {
    // Acceptance gate 3: "A frame with zero diagnostics -> zero overlay cost and zero drawn decorations."
    let surface = ErrorSurface::new();

    assert!(!surface.has_diagnostics());
    assert_eq!(surface.diagnostic_count(), 0);
    assert!(!surface.is_collapsed());
    assert!(surface.collapsed_summary().is_none());
    assert!(surface.collect_tier1_annotations().is_empty());
    assert!(surface.tier2_entries().is_empty());

    let mut paint = PaintList::new();
    surface.render_tier1_annotations(&mut paint);

    // Emits zero commands. Zero allocation. Zero overhead.
    assert!(paint.is_empty());
    assert_eq!(paint.len(), 0);
}

#[test]
fn test_anti_noise_500_violations_collapse_summary() {
    // Acceptance gate 4: "500 simultaneous forced violations -> collapsed summary, no per-item paint."
    let mut surface = ErrorSurface::new().with_max_inline(50);

    for i in 0..500 {
        surface.record_layout_diagnostic(LayoutDiagnostic::new(
            Some(WidgetId::from_parts((i + 1) as u32, 1)),
            format!("Root/List/Item[{}]", i),
            Rect::new(0.0, i as f64 * 10.0, 200.0, i as f64 * 10.0 + 10.0),
            Vec2::new(200.0, 10.0),
            Vec2::new(250.0, 10.0),
        ));
    }

    assert_eq!(surface.diagnostic_count(), 500);
    assert!(surface.is_collapsed());
    assert_eq!(
        surface.collapsed_summary(),
        Some("500 diagnostics — open inspector".to_string())
    );

    let annotations = surface.collect_tier1_annotations();
    // Collapses to exactly one summary banner!
    assert_eq!(annotations.len(), 1);
    assert_eq!(annotations[0].kind_name(), "collapsed_summary");

    // Render into paint list.
    let mut paint = PaintList::new();
    surface.render_tier1_annotations(&mut paint);

    // Verify it rendered ONLY the single collapsed banner, not 500 individual tapes!
    assert!(paint.len() <= 5);
    let has_summary_text = paint.commands.iter().any(|c| {
        if let PaintCommand::DrawText(_, text, _, _) = c {
            text.contains("500 diagnostics — open inspector")
        } else {
            false
        }
    });
    assert!(
        has_summary_text,
        "paint must contain collapsed summary text"
    );
}

#[test]
fn test_tier3_dev_panic_crash_bundle_and_copy_report() {
    // Acceptance gate 2: "A widget-paint panic in dev mode -> error card shows the debug_name path + crash bundle offers the last events; release build -> default panic behavior unchanged."
    let mut surface = ErrorSurface::new();

    // Populate event ledger with recent interactions.
    let mut ledger = EventLedger::new();
    let btn_id = WidgetId::from_parts(42, 1);

    ledger.push(
        EventRecord::new(1, 100, EventKind::Pointer, Disposition::Handled(btn_id))
            .with_position(Point::new(120.0, 45.0)),
    );
    ledger.push(
        EventRecord::new(2, 101, EventKind::Key, Disposition::Ignored)
            .with_position(Point::new(120.0, 45.0)),
    );

    // 1. Paint-phase panic -> ContinueWithPrunedNode policy.
    let bundle_paint = surface.capture_panic(
        "unexpected texture view format",
        Some(btn_id),
        "Root/Main/CanvasView",
        PanicPhase::Paint,
        Some(&ledger),
        5,
    );

    assert_eq!(bundle_paint.phase, PanicPhase::Paint);
    assert_eq!(bundle_paint.policy, RecoveryPolicy::ContinueWithPrunedNode);
    assert!(bundle_paint.policy.can_continue());
    assert_eq!(bundle_paint.recent_events.len(), 2);

    let report_paint = bundle_paint.format_copy_report();
    assert!(report_paint.contains("=== MARTENSITE DEV CRASH REPORT ==="));
    assert!(report_paint.contains("Phase: Paint"));
    assert!(report_paint.contains("Policy: Continue (prune node)"));
    assert!(report_paint.contains("Root/Main/CanvasView"));
    assert!(report_paint.contains("unexpected texture view format"));
    assert!(report_paint.contains("Recent Event Ledger"));

    // Render error card.
    let mut card_paint = PaintList::new();
    bundle_paint.render_card(&mut card_paint, Vec2::new(1920.0, 1080.0));
    assert!(!card_paint.is_empty());

    let has_header = card_paint.commands.iter().any(|c| {
        if let PaintCommand::DrawText(_, text, _, _) = c {
            text.contains("MARTENSITE DEV PANIC — PAINT PHASE")
        } else {
            false
        }
    });
    let has_continue_btn = card_paint.commands.iter().any(|c| {
        if let PaintCommand::DrawText(_, text, _, _) = c {
            text == "Continue"
        } else {
            false
        }
    });
    assert!(has_header);
    assert!(has_continue_btn);

    // 2. Layout-phase panic -> RestartOnly policy.
    let bundle_layout = surface.capture_panic(
        "circular flex constraint detected",
        Some(WidgetId::from_parts(15, 1)),
        "Root/FlexContainer",
        PanicPhase::Layout,
        None,
        0,
    );

    assert_eq!(bundle_layout.phase, PanicPhase::Layout);
    assert_eq!(bundle_layout.policy, RecoveryPolicy::RestartOnly);
    assert!(!bundle_layout.policy.can_continue());

    let report_layout = bundle_layout.format_copy_report();
    assert!(report_layout.contains("Phase: Layout"));
    assert!(report_layout.contains("Policy: Restart Required"));
    assert!(report_layout.contains("Restart is required"));
}

#[test]
fn test_in_flight_context_tracking() {
    clear_in_flight();
    assert!(current_in_flight().is_none());

    set_in_flight(
        Some(WidgetId::from_parts(1, 1)),
        "App/Root",
        PanicPhase::Layout,
    );
    let cur = current_in_flight().expect("in-flight must be set");
    assert_eq!(cur.path, "App/Root");
    assert_eq!(cur.phase, PanicPhase::Layout);

    // Scoped execution with restoration.
    let scoped_val = scope_in_flight(
        Some(WidgetId::from_parts(2, 1)),
        "App/Root/Child",
        PanicPhase::Paint,
        || {
            let inner = current_in_flight().expect("inner context");
            assert_eq!(inner.path, "App/Root/Child");
            assert_eq!(inner.phase, PanicPhase::Paint);
            99
        },
    );
    assert_eq!(scoped_val, 99);

    // Restores parent.
    let restored = current_in_flight().expect("restored context");
    assert_eq!(restored.path, "App/Root");
    assert_eq!(restored.phase, PanicPhase::Layout);

    clear_in_flight();
    assert!(current_in_flight().is_none());
}

#[test]
fn test_dev_panic_enabled_toggle() {
    set_dev_panic_enabled(false);
    assert!(!martensite_devtools::error_surface::is_dev_panic_enabled());

    set_dev_panic_enabled(true);
    assert!(martensite_devtools::error_surface::is_dev_panic_enabled());

    set_dev_panic_enabled(false);
    assert!(!martensite_devtools::error_surface::is_dev_panic_enabled());
}
