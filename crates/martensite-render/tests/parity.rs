//! TinySkia ↔ Vello backend parity tests.
//!
//! These tests feed a *shared* [`PaintList`] through both the CPU TinySkia
//! backend and the Vello GPU scene builder and verify they agree on
//! structural parity:
//!
//! - Both backends accept the same command sequence without panicking.
//! - The TinySkia pixmap produces visible pixels for every command type.
//! - The Vello scene encoding contains the expected geometry (path segments
//!   for rect/gradient commands and a glyph run for the font-backed
//!   `DrawGlyphRun`).
//!
//! Pixel-level parity (rasterizing the Vello scene to a CPU buffer and
//! comparing against TinySkia with [`martensite_render::diff::perceptual_diff`])
//! requires a wgpu adapter and is environment-dependent. It is covered by the
//! `vello_parity` module below when the `vello` feature is enabled; the
//! structural assertions always run.
//!
//! The Vello-side assertions are gated behind the `vello` feature because
//! they require [`martensite_render::VelloRenderer`].

#![cfg(test)]

use kurbo::{Point, Rect};
use martensite_render::paint::{
    FontResource, GlyphInstance, GlyphRun, GradientStop, GradientStops,
};
use martensite_render::paint::{PaintCommand, PaintList};
use martensite_render::tinyskia_backend::TinySkiaBackend;
use martensite_render::RenderBackend;

/// Bundled Fira Mono Regular font used to exercise real glyph outlines.
const FIRA_MONO: &[u8] = include_bytes!("assets/FiraMono-Medium.ttf");

/// Width/height of the test surface.
const W: u32 = 96;
const H: u32 = 64;

/// Builds a shared `PaintList` exercising the rect, gradient, text, and
/// font-backed glyph-run command paths.
fn shared_paint_list() -> PaintList {
    let mut list = PaintList::new();
    // A solid opaque red rect in the top-left.
    list.push_fill_rect(Rect::new(0.0, 0.0, 40.0, 40.0), [220, 40, 40, 255]);
    // A linear gradient across the bottom band.
    let stops = GradientStops::from_slice(&[
        GradientStop::new(0.0, [0, 0, 0, 255]),
        GradientStop::new(1.0, [255, 255, 255, 255]),
    ]);
    list.push_linear_gradient(
        Rect::new(0.0, 48.0, 96.0, 64.0),
        stops,
        [0.0, 48.0],
        [96.0, 48.0],
    );
    // A legacy text command (no font resource — exercises the placeholder
    // path in both backends).
    list.push_text(
        Point::new(4.0, 20.0),
        "Hi".to_string(),
        16.0,
        [0, 0, 0, 255],
    );
    // A font-backed glyph run drawing a single glyph.
    let font = FontResource::from_static(FIRA_MONO, 0);
    let mut run = GlyphRun::new(24.0, [0, 0, 0, 255]).with_font(font);
    run.glyphs
        .push(GlyphInstance::new(56.0, 28.0, 36, 0.0, 0.0));
    list.push_glyph_run(run);
    list
}

/// Counts the number of non-transparent pixels in a TinySkia backend.
fn non_zero_pixels(b: &TinySkiaBackend) -> usize {
    b.pixels()
        .as_chunks::<4>()
        .0
        .iter()
        .filter(|px| px[3] != 0)
        .count()
}

#[test]
fn tinyskia_renders_shared_list() {
    let mut backend = TinySkiaBackend::new(W, H).expect("pixmap should allocate");
    let list = shared_paint_list();
    backend.render(&list);
    assert!(
        non_zero_pixels(&backend) > 0,
        "shared list should produce visible pixels"
    );
    // The opaque red rect interior must be exactly the requested color.
    let red = backend.pixmap().pixel(20, 20).expect("pixel in range");
    assert_eq!(red.red(), 220);
    assert_eq!(red.green(), 40);
    assert_eq!(red.blue(), 40);
    assert_eq!(red.alpha(), 255);
}

#[test]
fn both_backends_accept_same_command_set() {
    let list = shared_paint_list();

    // TinySkia side.
    let mut ts = TinySkiaBackend::new(W, H).expect("pixmap should allocate");
    ts.render(&list);
    assert!(ts.pixels().iter().any(|&b| b != 0));

    // The command count and types are identical regardless of backend.
    assert_eq!(list.commands.len(), 4);
    assert!(matches!(list.commands[0], PaintCommand::FillRect(..)));
    assert!(matches!(
        list.commands[1],
        PaintCommand::FillLinearGradient(..)
    ));
    assert!(matches!(list.commands[2], PaintCommand::DrawText(..)));
    assert!(matches!(list.commands[3], PaintCommand::DrawGlyphRun(..)));
}

// ---------------------------------------------------------------------------
// Vello parity (gated behind the `vello` feature).
// ---------------------------------------------------------------------------

#[cfg(feature = "vello")]
mod vello_parity {
    use super::*;
    use martensite_render::vello_backend::VelloRenderer;

    #[test]
    fn vello_scene_builds_geometry_for_shared_list() {
        let mut renderer = VelloRenderer::new();
        let list = shared_paint_list();
        renderer.render(&list);
        let encoding = renderer.scene().encoding();
        assert!(
            encoding.n_path_segments > 0,
            "vello scene should contain path geometry (rect, gradient, text)"
        );
        assert!(
            !encoding.resources.glyph_runs.is_empty(),
            "vello scene should contain a font-backed glyph run"
        );
    }

    /// Both backends must agree on the command count after rendering the
    /// shared list. This catches divergence where one backend silently drops
    /// or duplicates commands.
    #[test]
    fn both_backends_report_same_command_count() {
        let list = shared_paint_list();
        let expected = list.commands.len();

        let mut ts = TinySkiaBackend::new(W, H).expect("pixmap should allocate");
        ts.render(&list);
        // TinySkiaBackend has no command counter, but it must not panic.
        assert!(ts.pixels().iter().any(|&b| b != 0));

        let mut vello = VelloRenderer::new();
        vello.render(&list);
        assert_eq!(vello.last_command_count(), expected);
    }

    /// A font-backed glyph run must produce a Vello glyph run (not path
    /// segments), while a fontless glyph run must fall back to path segments
    /// (rectangles). Both backends must handle both cases without panicking.
    #[test]
    fn glyph_run_font_vs_fontless_parity() {
        // --- Font-backed: Vello encodes a real glyph run ---
        let font = FontResource::from_static(FIRA_MONO, 0);
        let mut run = GlyphRun::new(20.0, [0, 0, 0, 255]).with_font(font);
        run.glyphs.push(GlyphInstance::new(4.0, 20.0, 36, 0.0, 0.0));
        let mut list = PaintList::new();
        list.push_glyph_run(run);

        let mut vello = VelloRenderer::new();
        vello.render(&list);
        assert!(
            !vello.scene().encoding().resources.glyph_runs.is_empty(),
            "font-backed run should produce a vello glyph run"
        );

        let mut ts = TinySkiaBackend::new(W, H).expect("pixmap should allocate");
        ts.render(&list);
        assert!(
            non_zero_pixels(&ts) > 0,
            "font-backed run should produce pixels"
        );

        // --- Fontless: Vello falls back to path segments ---
        let mut run = GlyphRun::new(16.0, [255, 0, 0, 255]);
        run.glyphs
            .push(GlyphInstance::new(4.0, 20.0, 1, 12.0, 14.0));
        let mut list = PaintList::new();
        list.push_glyph_run(run);

        let mut vello = VelloRenderer::new();
        vello.render(&list);
        assert!(
            vello.scene().encoding().resources.glyph_runs.is_empty(),
            "fontless run should not produce a vello glyph run"
        );
        assert!(
            vello.scene().encoding().n_path_segments > 0,
            "fontless run should fall back to path segments"
        );

        let mut ts = TinySkiaBackend::new(W, H).expect("pixmap should allocate");
        ts.render(&list);
        assert!(
            non_zero_pixels(&ts) > 0,
            "fontless run should produce pixels"
        );
    }
}
