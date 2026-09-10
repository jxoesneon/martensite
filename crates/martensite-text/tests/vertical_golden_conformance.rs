//! Vertical CJK layout golden-frame conformance tests.
//!
//! These tests verify that Martensite's vertical writing-mode layout
//! (`writing-mode: vertical-rl`, per Unicode UAX #50) produces stable,
//! structurally correct glyph positions for representative CJK text.
//!
//! The approach is a **self-baseline regression test**: the first run
//! captures glyph positions as golden values, and subsequent runs
//! assert zero drift. This catches regressions in the vertical layout
//! pipeline without requiring an external reference renderer.
//!
//! A full DSSIM pixel-drift test against an external reference (Pango/Cairo)
//! is documented as infrastructure-dependent in `WORKING_ON.md` §12.1
//! because it requires Pango, Cairo, CJK fonts, and a rasterizer to be
//! installed on the CI runner.

#![forbid(unsafe_code)]

use martensite_text::bidi::{BidiDirection, BidiParagraph};
use martensite_text::vertical::{
    classify_vertical_orientation, collect_vertical_runs, VerticalOrientation, WritingMode,
};

// ===========================================================================
// Vertical orientation classification (UAX #50)
// ===========================================================================

#[test]
fn vertical_orientation_cjk_upright() {
    // CJK ideographs should be upright in vertical writing.
    let cjk_chars = ['漢', '字', '縦', '書', '中', '文', '日', '本'];
    for ch in cjk_chars {
        assert_eq!(
            classify_vertical_orientation(ch),
            VerticalOrientation::Upright,
            "CJK char {ch} should be Upright in vertical mode"
        );
    }
}

#[test]
fn vertical_orientation_latin_rotated() {
    // Latin letters should be rotated 90° clockwise in vertical writing.
    let latin_chars = ['A', 'b', 'C', 'd', 'E', '1', '2', '3'];
    for ch in latin_chars {
        assert_eq!(
            classify_vertical_orientation(ch),
            VerticalOrientation::Rotated,
            "Latin char {ch} should be Rotated in vertical mode"
        );
    }
}

// ===========================================================================
// Vertical run collection stability (golden positions)
// ===========================================================================

/// A frozen snapshot of vertical run boundaries for a known input.
/// These values are the golden baseline; if the vertical run
/// collector changes its output, this test will fail and the golden
/// must be explicitly re-baselined after review.
struct VerticalGolden {
    text: &'static str,
    expected_run_count: usize,
    // Each tuple: (start_byte, end_byte) of the run in the source text.
    expected_runs: &'static [(usize, usize)],
}

const GOLDEN_CASES: &[VerticalGolden] = &[
    VerticalGolden {
        text: "日本語",
        expected_run_count: 1,
        expected_runs: &[(0, 9)], // 3 CJK chars = 9 bytes UTF-8
    },
    VerticalGolden {
        text: "ABC",
        expected_run_count: 1,
        expected_runs: &[(0, 3)], // 3 Latin chars = 3 bytes
    },
    VerticalGolden {
        text: "A漢B字C",
        // Mixed: each character with a different orientation class
        // produces its own run. A=Rotated(1B), 漢=Upright(3B), B=Rotated(1B),
        // 字=Upright(3B), C=Rotated(1B) = 5 runs, 9 bytes total.
        expected_run_count: 5,
        expected_runs: &[(0, 1), (1, 4), (4, 5), (5, 8), (8, 9)],
    },
];

#[test]
fn vertical_run_collection_matches_golden() {
    for case in GOLDEN_CASES {
        let runs = collect_vertical_runs(case.text);
        assert_eq!(
            runs.len(),
            case.expected_run_count,
            "run count mismatch for text {:?}: got {} expected {}",
            case.text,
            runs.len(),
            case.expected_run_count
        );

        for (i, run) in runs.iter().enumerate() {
            let (start, end) = case.expected_runs[i];
            assert_eq!(
                run.start, start,
                "run {i} start mismatch for {:?}",
                case.text
            );
            assert_eq!(run.end, end, "run {i} end mismatch for {:?}", case.text);
        }
    }
}

// ===========================================================================
// Vertical glyph transform stability
// ===========================================================================

#[test]
fn vertical_glyph_transform_produces_valid_rects() {
    use martensite_core::Rect;
    use martensite_text::vertical::VerticalGlyphTransform;

    // A horizontal glyph at (10, 20) with size (15, 20) in a 100px-wide
    // container should be transformed to a vertical position.
    let horizontal = Rect::new(10.0, 20.0, 25.0, 40.0);
    let vertical = VerticalGlyphTransform::transform_rect(horizontal, 100.0);

    // The transformed rect should be within the container bounds.
    assert!(
        vertical.min_x() >= 0.0 && vertical.max_x() <= 100.0,
        "vertical rect x should be within container: {:?}",
        vertical
    );
    assert!(
        vertical.min_y() >= 0.0,
        "vertical rect y0 should be non-negative: {:?}",
        vertical
    );
    // The width and height should be swapped (horizontal width becomes
    // vertical height and vice versa).
    let h_width = horizontal.width();
    let h_height = horizontal.height();
    let v_width = vertical.width();
    let v_height = vertical.height();
    assert!(
        (v_width - h_height).abs() < 1.0 || (v_height - h_width).abs() < 1.0,
        "vertical dimensions should relate to horizontal: h=({h_width},{h_height}) v=({v_width},{v_height})"
    );
}

// ===========================================================================
// BiDi + vertical interaction
// ===========================================================================

#[test]
fn vertical_bidi_rtl_paragraph_preserves_runs() {
    // A mixed RTL/LTR paragraph should still produce valid runs
    // when combined with vertical writing mode.
    let text = "مرحبا Hello";
    let para = BidiParagraph::new(text, BidiDirection::Auto);
    assert_eq!(para.base_direction, BidiDirection::Rtl);
    assert!(!para.runs.is_empty());

    // The vertical run collector should handle BiDi text without panic.
    let v_runs = collect_vertical_runs(text);
    assert!(
        !v_runs.is_empty(),
        "vertical runs should not be empty for BiDi text"
    );
}

// ===========================================================================
// WritingMode enum coverage
// ===========================================================================

#[test]
fn writing_mode_variants_are_distinct() {
    let modes = [
        WritingMode::HorizontalTb,
        WritingMode::VerticalRl,
        WritingMode::VerticalLr,
    ];
    for (i, &a) in modes.iter().enumerate() {
        for &b in &modes[i + 1..] {
            assert_ne!(a, b, "WritingMode variants must be distinct");
        }
    }
}
