//! WCAG 2.2 AAA conformance checks for blessed widget rendering parameters.
//!
//! The blessed widgets (`DataTable`, `Chart`, `CodeEditor`, `AudioWaveform`)
//! are data/logic models without intrinsic colors. This test verifies that
//! their recommended rendering parameters — the color palette, target sizes,
//! and focus appearance that a Martensite application would use when
//! painting them — satisfy WCAG 2.2 Level AAA rules.

#![forbid(unsafe_code)]

use martensite_access::compliance::{
    check_target_size, check_text_contrast, check_ui_component_contrast, ColorRgba,
    FocusAppearanceCheck, TextSize, WcagLevel,
};

/// The recommended rendering palette for blessed widgets.
///
/// These are the foreground/background color pairs and target sizes
/// that Martensite applications should use when rendering each blessed
/// widget to meet WCAG AAA.
struct BlessedWidgetParams {
    name: &'static str,
    fg: ColorRgba,
    bg: ColorRgba,
    text_size: TextSize,
    target_w: f32,
    target_h: f32,
    focus_contrast: f32,
    focus_thickness: f32,
}

fn blessed_widget_params() -> [BlessedWidgetParams; 4] {
    let white = ColorRgba::rgb(1.0, 1.0, 1.0);
    let black = ColorRgba::rgb(0.0, 0.0, 0.0);
    let dark_gray = ColorRgba::rgb(0.15, 0.15, 0.15);
    let grid_color = ColorRgba::rgb(0.3, 0.3, 0.3);

    [
        BlessedWidgetParams {
            name: "DataTable",
            fg: black,
            bg: white,
            text_size: TextSize::Normal,
            target_w: 24.0,
            target_h: 24.0,
            focus_contrast: 4.5,
            focus_thickness: 2.0,
        },
        BlessedWidgetParams {
            name: "Chart",
            // Chart axis labels are small text on white background.
            fg: dark_gray,
            bg: white,
            text_size: TextSize::Normal,
            target_w: 24.0,
            target_h: 24.0,
            focus_contrast: 4.5,
            focus_thickness: 2.0,
        },
        BlessedWidgetParams {
            name: "CodeEditor",
            fg: black,
            bg: white,
            text_size: TextSize::Normal,
            target_w: 24.0,
            target_h: 24.0,
            focus_contrast: 4.5,
            focus_thickness: 2.0,
        },
        BlessedWidgetParams {
            name: "AudioWaveform",
            // Waveform is drawn with grid lines (UI component, not text).
            fg: grid_color,
            bg: white,
            text_size: TextSize::Normal,
            target_w: 24.0,
            target_h: 24.0,
            focus_contrast: 4.5,
            focus_thickness: 2.0,
        },
    ]
}

#[test]
fn blessed_widgets_wcg_aaa_text_contrast() {
    for params in blessed_widget_params() {
        assert!(
            check_text_contrast(params.fg, params.bg, params.text_size, WcagLevel::Aaa),
            "WCAG AAA text contrast failed for {}: fg={:?} bg={:?}",
            params.name,
            params.fg,
            params.bg
        );
    }
}

#[test]
fn blessed_widgets_wcg_ui_component_contrast() {
    let white = ColorRgba::rgb(1.0, 1.0, 1.0);
    for params in blessed_widget_params() {
        // Grid lines, borders, and focus indicators must reach 3.0:1.
        assert!(
            check_ui_component_contrast(params.fg, white),
            "WCAG 1.4.11 UI component contrast failed for {}",
            params.name
        );
    }
}

#[test]
fn blessed_widgets_wcg_target_size() {
    for params in blessed_widget_params() {
        assert!(
            check_target_size(params.target_w, params.target_h),
            "WCAG 2.5.8 target size failed for {}: {}x{} px",
            params.name,
            params.target_w,
            params.target_h
        );
    }
}

#[test]
fn blessed_widgets_wcg_focus_appearance_aaa() {
    for params in blessed_widget_params() {
        let check = FocusAppearanceCheck::new(params.focus_contrast, params.focus_thickness);
        assert!(
            check.is_compliant(),
            "WCAG 2.4.13 focus appearance failed for {}: contrast={} thickness={}",
            params.name,
            params.focus_contrast,
            params.focus_thickness
        );
    }
}
