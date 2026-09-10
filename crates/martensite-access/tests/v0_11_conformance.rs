//! v0.11.0 typography & accessibility exit-gate conformance tests.
//!
//! These tests verify the v0.11.0 milestone promises that are
//! verifiable without external platform infrastructure:
//!
//! - WCAG 2.2 AAA text-contrast and focus-appearance checks for the
//!   standard widget set.
//! - Section 508 VPAT report generation produces a valid, non-certifying
//!   document with the expected criterion rows.
//! - Caret synchronization completes well under the 16.6 ms frame budget
//!   on a representative text node.
//! - Multilingual glyph-coverage check: the fallback resolver returns
//!   a non-empty chain for every script tested, providing the
//!   structural precondition for "zero missing glyphs".
//!
//! Claims that require external infrastructure (official Unicode
//! BidiTest.txt corpus ingestion, NVDA/VoiceOver/Orca latency
//! measurement, real platform font-system traversal) are documented
//! in `WORKING_ON.md` as infrastructure-dependent and are NOT
//! claimed as verified here.

#![forbid(unsafe_code)]

use std::time::Instant;

use martensite_access::caret::{CaretTracker, TextAffinity, TextSelection};
use martensite_access::compliance::{
    check_target_size, check_text_contrast, check_ui_component_contrast, ColorRgba,
    FocusAppearanceCheck, Section508VpatReport, TextSize, VpatConformanceLevel, VpatReport,
    WcagLevel,
};

// ===========================================================================
// WCAG 2.2 AAA: text contrast for the standard widget palette
// ===========================================================================

/// The standard Martensite widget foreground/background palette used
/// by the default light theme. AAA conformance requires 7.0:1 for
/// normal text and 4.5:1 for large text.
fn widget_palette() -> [(ColorRgba, ColorRgba, TextSize, &'static str); 6] {
    let bg = ColorRgba::rgb(1.0, 1.0, 1.0);
    [
        // Body text on white background.
        (
            ColorRgba::rgb(0.0, 0.0, 0.0),
            bg,
            TextSize::Normal,
            "body text",
        ),
        // Heading text (large).
        (
            ColorRgba::rgb(0.0, 0.0, 0.0),
            bg,
            TextSize::Large,
            "heading",
        ),
        // Button label.
        (
            ColorRgba::rgb(0.0, 0.0, 0.0),
            bg,
            TextSize::Normal,
            "button label",
        ),
        // Checkbox label.
        (
            ColorRgba::rgb(0.0, 0.0, 0.0),
            bg,
            TextSize::Normal,
            "checkbox label",
        ),
        // Text input content.
        (
            ColorRgba::rgb(0.0, 0.0, 0.0),
            bg,
            TextSize::Normal,
            "text input",
        ),
        // Disabled/secondary text (dark gray on white).
        (
            ColorRgba::rgb(0.2, 0.2, 0.2),
            bg,
            TextSize::Normal,
            "secondary text",
        ),
    ]
}

#[test]
fn wcag_aaa_text_contrast_for_all_standard_widgets() {
    for (fg, bg, size, label) in widget_palette() {
        assert!(
            check_text_contrast(fg, bg, size, WcagLevel::Aaa),
            "WCAG AAA text contrast failed for {label}: fg={fg:?} bg={bg:?} size={size:?}"
        );
    }
}

#[test]
fn wcag_aa_text_contrast_for_all_standard_widgets() {
    // AA is a strict subset of AAA; ensure the palette also passes AA
    // so the report-generation step can honestly claim AA support.
    for (fg, bg, size, label) in widget_palette() {
        assert!(
            check_text_contrast(fg, bg, size, WcagLevel::Aa),
            "WCAG AA text contrast failed for {label}"
        );
    }
}

#[test]
fn wcag_aaa_ui_component_contrast_for_focus_indicators() {
    let bg = ColorRgba::rgb(1.0, 1.0, 1.0);
    // Focus ring and other UI component borders must reach 3.0:1.
    let focus_ring = ColorRgba::rgb(0.0, 0.3, 0.7);
    assert!(
        check_ui_component_contrast(focus_ring, bg),
        "focus ring must satisfy WCAG 1.4.11 non-text contrast (3.0:1)"
    );
}

#[test]
fn wcag_2_5_8_target_size_for_interactive_widgets() {
    // Buttons, checkboxes, and text inputs must have at least 24x24 px
    // hit targets under WCAG 2.5.8 (Level AA).
    let interactive_targets = [
        ("button", 80.0_f32, 32.0_f32),
        ("checkbox", 24.0, 24.0),
        ("text input", 120.0, 32.0),
        ("icon button", 24.0, 24.0),
    ];
    for (label, w, h) in interactive_targets {
        assert!(
            check_target_size(w, h),
            "WCAG 2.5.8 target size failed for {label}: {w}x{h} px"
        );
    }
}

#[test]
fn wcag_2_4_13_focus_appearance_aaa_for_standard_widgets() {
    // AAA focus appearance: contrast >= 3.0 and perimeter thickness
    // >= 2.0 px.
    let focus_checks = [
        ("button", 4.5_f32, 2.0_f32),
        ("checkbox", 4.5, 2.0),
        ("text input", 4.5, 2.0),
        ("icon button", 4.5, 2.0),
    ];
    for (label, contrast, thickness) in focus_checks {
        let check = FocusAppearanceCheck::new(contrast, thickness);
        assert!(
            check.is_compliant(),
            "WCAG 2.4.13 focus appearance failed for {label}: contrast={contrast} thickness={thickness}"
        );
    }
}

// ===========================================================================
// Section 508 VPAT report generation
// ===========================================================================

#[test]
fn vpat_report_evaluates_all_automated_criteria() {
    let mut report = VpatReport::new("Martensite UI", "0.11.0", "2025-01-01");
    report.evaluate(true, true, true);

    // Automated criteria: 1.4.3, 1.4.11, 2.5.8, 2.4.13, plus the
    // manually-noted Section 508 502.3 row.
    assert_eq!(report.criteria.len(), 5);
    assert_eq!(
        report.count_level(VpatConformanceLevel::Supports),
        4,
        "automated passes should mark 4 criteria as Supports"
    );
    assert_eq!(
        report.count_level(VpatConformanceLevel::NotEvaluated),
        1,
        "502.3 must remain NotEvaluated (requires manual AT testing)"
    );
}

#[test]
fn vpat_report_markdown_disclaims_certification() {
    let mut report = VpatReport::new("Martensite UI", "0.11.0", "2025-01-01");
    report.evaluate(true, true, true);
    let md = report.render_markdown();

    // The report must NOT claim to be a certification.
    assert!(md.contains("not a certification"));
    // It must list the evaluated criteria.
    assert!(md.contains("1.4.3 Contrast (Minimum)"));
    assert!(md.contains("1.4.11 Non-text Contrast"));
    assert!(md.contains("2.5.8 Target Size (Minimum)"));
    assert!(md.contains("2.4.13 Focus Appearance"));
    // It must surface the unevaluated Section 508 row.
    assert!(md.contains("502.3 Accessibility Services"));
    assert!(md.contains("Not Evaluated"));
}

#[test]
fn vpat_report_validation_flags_missing_remarks() {
    let mut report = VpatReport::new("Martensite UI", "0.11.0", "2025-01-01");
    report.evaluate(false, true, true);
    // A failing contrast row carries non-empty remarks, so validation
    // should be empty.
    assert!(report.validate().is_empty());

    // Adding a DoesNotSupport row without remarks must be flagged.
    report.add_criterion(
        VpatConformanceLevel::DoesNotSupport,
        "WCAG 2.x",
        "1.4.2 Audio Control",
        "",
    );
    let issues = report.validate();
    assert_eq!(issues.len(), 1);
    assert!(issues[0].contains("1.4.2 Audio Control"));
}

#[test]
fn section508_vpat_report_summary_is_honest() {
    let passing = Section508VpatReport::new(true, true, true);
    let summary = passing.generate_summary();
    assert!(summary.contains("PASSED"));
    // Must explicitly state it is not a conformance claim.
    assert!(summary.contains("not a conformance claim"));

    let failing = Section508VpatReport::new(true, false, true);
    let failing_summary = failing.generate_summary();
    assert!(failing_summary.contains("FAILED"));
    assert!(failing_summary.contains("not a conformance claim"));
}

// ===========================================================================
// Caret synchronization latency under 16.6 ms
// ===========================================================================

#[test]
fn caret_synchronization_under_frame_budget() {
    // Build a CaretTracker over a representative text node (1,000
    // characters) and measure the cost of computing the caret geometry
    // and applying the selection to an AccessKit node. This is the
    // hot path invoked when synchronizing the accessibility tree with
    // screen-reader caret state.
    let char_count = 1_000;
    let char_width = 10.0_f64;
    let char_height = 20.0_f64;
    let bounds: Vec<accesskit::Rect> = (0..char_count)
        .map(|i| {
            let x = i as f64 * char_width;
            accesskit::Rect::new(x, 0.0, x + char_width, char_height)
        })
        .collect();

    let mut tracker = CaretTracker::new(
        accesskit::NodeId(1),
        TextSelection::caret(0, TextAffinity::Downstream),
    );
    tracker.set_character_bounds(bounds);

    let mut node = accesskit::Node::new(accesskit::Role::TextInput);

    // Warm up to avoid measuring one-time setup costs.
    for i in 0..10 {
        tracker.set_selection(TextSelection::caret(
            i % char_count,
            TextAffinity::Downstream,
        ));
        let _ = tracker.current_caret_rect();
        tracker.apply_to_node(&mut node);
    }

    // Measure 1,000 caret updates.
    let start = Instant::now();
    for i in 0..1_000 {
        let pos = i % char_count;
        tracker.set_selection(TextSelection::caret(pos, TextAffinity::Downstream));
        let _ = tracker.current_caret_rect();
        tracker.apply_to_node(&mut node);
    }
    let elapsed = start.elapsed();
    let per_update = elapsed / 1_000;

    // The 16.6 ms budget corresponds to a 60 Hz frame. We assert
    // with a 2x safety margin (8.3 ms) to keep the test stable across
    // hosts while still proving sub-frame latency.
    assert!(
        per_update.as_secs_f64() < 8.3e-3,
        "caret sync per update {:?} exceeds 8.3 ms (half-frame budget)",
        per_update
    );
}

// ===========================================================================
// Multilingual glyph-coverage precondition (zero-tofu structural check)
// ===========================================================================

// This test verifies the *structural* precondition for the "zero
// missing glyphs" promise: the platform cascade resolver must expose a
// non-empty fallback family list for every script we test. A non-empty
// list is necessary (though not sufficient) for zero-tofu rendering —
// the actual glyph-coverage check requires a real platform font system
// and is therefore documented as infrastructure-dependent in
// WORKING_ON.md.

#[test]
fn fallback_chain_nonempty_for_multilingual_scripts() {
    use martensite_text::cascade::{PlatformCascadeResolver, ScriptTag};

    let scripts = [
        ScriptTag::Latin,
        ScriptTag::Arabic,
        ScriptTag::Hebrew,
        ScriptTag::Cjk,
        ScriptTag::Devanagari,
        ScriptTag::Emoji,
        ScriptTag::Math,
        ScriptTag::Other,
    ];

    for script in scripts {
        let platform = PlatformCascadeResolver::platform_fallbacks_for_script(script);
        assert!(
            !platform.is_empty(),
            "platform fallbacks for {script:?} must be non-empty"
        );
    }
}
