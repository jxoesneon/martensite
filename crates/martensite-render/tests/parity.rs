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
    // The opaque red rect interior must be exactly the requested color.
    let red = backend.pixmap().pixel(20, 20).expect("pixel in range");
    assert_eq!(red.red(), 220);
    assert_eq!(red.green(), 40);
    assert_eq!(red.blue(), 40);
    assert_eq!(red.alpha(), 255);
    // A pixel outside all drawn shapes must remain transparent (background).
    let bg = backend.pixmap().pixel(90, 5).expect("pixel in range");
    assert_eq!(
        bg.alpha(),
        0,
        "pixel outside drawn area should be transparent"
    );
    // Secondary check: the shared list should produce many visible pixels.
    assert!(
        non_zero_pixels(&backend) > 100,
        "shared list should produce many visible pixels"
    );
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
        // The glyph is anti-aliased, so we check for significant black ink
        // near the glyph baseline rather than an exact alpha value.
        let ink = ts.pixmap().pixel(8, 14).expect("pixel in range");
        assert!(
            ink.alpha() > 64,
            "font-backed glyph should have ink near (8, 14), got alpha={}",
            ink.alpha()
        );
        assert_eq!(ink.red(), 0);
        assert_eq!(ink.green(), 0);
        assert_eq!(ink.blue(), 0);
        // A pixel far from the glyph must remain transparent.
        let bg = ts.pixmap().pixel(80, 5).expect("pixel in range");
        assert_eq!(bg.alpha(), 0, "pixel outside glyph should be transparent");
        // Secondary check.
        assert!(
            non_zero_pixels(&ts) > 10,
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
        // The fontless fallback draws a bounding-box rectangle for the glyph.
        // The box spans x in [4, 16], y in [6, 20] (baseline 20 minus height 14).
        // A pixel near the center must be exactly the fill color.
        let inside = ts.pixmap().pixel(10, 13).expect("pixel in range");
        assert_eq!(inside.red(), 255);
        assert_eq!(inside.green(), 0);
        assert_eq!(inside.blue(), 0);
        assert_eq!(inside.alpha(), 255);
        // A pixel outside the box must remain transparent.
        let outside = ts.pixmap().pixel(50, 50).expect("pixel in range");
        assert_eq!(
            outside.alpha(),
            0,
            "pixel outside glyph box should be transparent"
        );
        // Secondary check.
        assert!(
            non_zero_pixels(&ts) > 10,
            "fontless run should produce pixels"
        );
    }
}

// ---------------------------------------------------------------------------
// GPU/CPU pixel-level DSSIM parity (gated behind the `vello` feature).
// ---------------------------------------------------------------------------

#[cfg(feature = "vello")]
mod gpu_cpu_parity {
    use super::*;
    use kurbo::BezPath;
    use martensite_test::dssim::{dssim, ImageBuffer};
    use martensite_wgpu::{GpuContext, RecoveryMachine, RenderMode, RenderOrchestrator};

    /// Demultiplies a premultiplied RGBA8 buffer in place: each color channel
    /// is divided by alpha so the result is non-premultiplied ("straight")
    /// alpha. Fully transparent pixels collapse to black. This normalizes the
    /// premultiplied output of both TinySkia and Vello before grayscale
    /// conversion so that anti-aliased edges compare on the same straight-alpha
    /// color basis rather than a darkened premultiplied one.
    fn demultiply(rgba: &mut [u8]) {
        for px in rgba.as_chunks_mut::<4>().0 {
            let a = px[3] as u32;
            if a == 0 {
                px[0] = 0;
                px[1] = 0;
                px[2] = 0;
            } else if a < 255 {
                // Un-premultiply with rounding: c = round(c_premul * 255 / a).
                px[0] = ((px[0] as u32 * 255 + a / 2) / a).min(255) as u8;
                px[1] = ((px[1] as u32 * 255 + a / 2) / a).min(255) as u8;
                px[2] = ((px[2] as u32 * 255 + a / 2) / a).min(255) as u8;
            }
            // a == 255: premultiplied == straight, no change needed.
        }
    }

    /// Builds a deterministic paint list exercising a solid fill rect, a
    /// linear gradient, and a simple filled triangle path.
    ///
    /// Text and glyph runs are intentionally omitted: their placeholder
    /// rasterization (filled glyph boxes) differs between backends in ways
    /// unrelated to the rasterizer parity under test, and would only add noise
    /// to the DSSIM score. The three commands here exercise the core fill,
    /// gradient, and path pipelines that the parity check is meant to cover.
    fn parity_paint_list() -> PaintList {
        let mut list = PaintList::new();
        // Solid opaque red rect in the upper-left.
        list.push_fill_rect(Rect::new(8.0, 8.0, 56.0, 56.0), [220, 40, 40, 255]);
        // Linear black→white gradient across the bottom band. Both backends
        // interpolate in sRGB (gamma-encoded) space, so the ramp matches.
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
        // Simple filled triangle path (blue) in the upper-right.
        let mut path = BezPath::new();
        path.move_to((72.0, 8.0));
        path.line_to((90.0, 44.0));
        path.line_to((54.0, 44.0));
        path.close_path();
        list.push_path(path, [40, 80, 220, 255]);
        list
    }

    /// GPU/CPU pixel-level parity via DSSIM.
    ///
    /// Renders the same deterministic paint list with TinySkia (CPU) and Vello
    /// (GPU via the Lavapipe/llvmpipe software adapter selected by
    /// [`GpuContext::with_cpu_fallback`]), then compares the demultiplied
    /// grayscale images.
    ///
    /// The project's [`dssim`] function returns a *dissimilarity* score where
    /// `0.0` is identical and `1.0` is completely different. The parity
    /// assertion is expressed in terms of the equivalent SSIM *similarity*
    /// (`1.0 - dssim`): independent rasterizers differ on anti-aliased edges
    /// and gradient interpolation, so we require `SSIM > 0.98` (i.e.
    /// `DSSIM < 0.02`) rather than near-exact parity (`0.999`), which is not
    /// realistic between two independent rasterizers.
    #[test]
    #[ignore = "requires a software Vulkan adapter (Lavapipe/llvmpipe)"]
    fn gpu_cpu_dssim_parity() {
        // Acquire the CPU fallback adapter (llvmpipe). If no software Vulkan
        // driver is available, skip gracefully rather than failing.
        let ctx = match GpuContext::with_cpu_fallback() {
            Ok(ctx) => ctx,
            Err(_) => return,
        };

        let list = parity_paint_list();

        // --- CPU reference (TinySkia) ---
        let mut cpu = TinySkiaBackend::new(W, H).expect("pixmap should allocate");
        cpu.render(&list);
        let mut cpu_pixels = cpu.pixels().to_vec();

        // --- GPU render (Vello) via the headless readback path ---
        let mut orchestrator =
            RenderOrchestrator::with_default_config(W, H).expect("orchestrator init");
        let recovery = RecoveryMachine::new();
        orchestrator.render(&list, &recovery);
        assert_eq!(
            orchestrator.mode(),
            RenderMode::Gpu,
            "default config uses GPU"
        );
        let mut gpu_pixels = orchestrator
            .render_to_buffer(&ctx.device, &ctx.queue, W, H)
            .expect("GPU readback should succeed on the fallback adapter");

        let expected = (W as usize) * (H as usize) * 4;
        assert_eq!(cpu_pixels.len(), expected, "CPU pixel buffer size");
        assert_eq!(gpu_pixels.len(), expected, "GPU pixel buffer size");

        // Demultiply both premultiplied buffers to straight alpha before the
        // grayscale conversion used by the DSSIM metric.
        demultiply(&mut cpu_pixels);
        demultiply(&mut gpu_pixels);

        let cpu_img = ImageBuffer::from_rgba(W, H, &cpu_pixels);
        let gpu_img = ImageBuffer::from_rgba(W, H, &gpu_pixels);

        let dissim = dssim(&cpu_img, &gpu_img);
        let similarity = 1.0 - dissim;
        assert!(
            similarity > 0.98,
            "GPU/CPU parity too low: SSIM={similarity:.4} (DSSIM={dissim:.4}); \
             expected SSIM > 0.98 between TinySkia and Vello on the shared paint list"
        );
    }
}
