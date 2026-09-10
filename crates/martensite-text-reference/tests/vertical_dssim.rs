//! DSSIM comparison tests for the Pango/Cairo vertical-CJK reference
//! renderer.
//!
//! ## Self-consistency and non-triviality
//!
//! The first three tests validate that the external reference renderer is
//! deterministic and produces non-trivial (non-blank) output.
//!
//! ## Martensite-vs-Pango DSSIM comparison
//!
//! The `vertical_cjk_martensite_vs_pango_dssim` test renders the same CJK
//! string through both Pango (external reference) and Martensite's own
//! TinySkia-based vertical rasterizer, then compares the two pixel buffers
//! via DSSIM.
//!
//! The Martensite rasterizer shapes text with cosmic-text, stacks glyphs
//! vertically (UAX #50 upright orientation), and rasterizes each glyph
//! outline via swash into a TinySkia pixmap. This is a genuine
//! cross-engine comparison: Pango uses HarfBuzz + FreeType, Martensite uses
//! cosmic-text + swash. The two will differ on anti-aliased edge coverage,
//! hinting, subpixel positioning, and glyph metrics, so the threshold is
//! DSSIM < 0.10 (similarity > 0.90), not near-exact parity.

#![forbid(unsafe_code)]

use martensite_render::paint::{FontResource, GlyphInstance, GlyphRun, PaintList};
use martensite_render::RenderBackend;
use martensite_render::TinySkiaBackend;
use martensite_test::dssim::{dssim, ImageBuffer};
use martensite_text_reference::{render_horizontal_cjk_reference, render_vertical_cjk_reference};

/// Font family used for the reference renders. `Noto Sans CJK JP` is
/// installed via `fonts-noto-cjk` on the CI runner.
const FONT_FAMILY: &str = "Noto Sans CJK JP";
/// Font size in points.
const FONT_SIZE: f64 = 24.0;
/// Surface width in pixels.
const WIDTH: u32 = 128;
/// Surface height in pixels.
const HEIGHT: u32 = 256;
/// Sample CJK text exercising upright glyphs in vertical mode.
const SAMPLE_TEXT: &str = "日本語の縦書き";

// ---------------------------------------------------------------------------
// Pango reference: self-consistency and non-triviality
// ---------------------------------------------------------------------------

/// Renders the same CJK string twice with the vertical reference renderer
/// and asserts the two outputs are perceptually identical.
#[test]
fn vertical_cjk_dssim_self_consistency() {
    let rgba_a = render_vertical_cjk_reference(SAMPLE_TEXT, FONT_FAMILY, FONT_SIZE, WIDTH, HEIGHT);
    let rgba_b = render_vertical_cjk_reference(SAMPLE_TEXT, FONT_FAMILY, FONT_SIZE, WIDTH, HEIGHT);

    let img_a = ImageBuffer::from_rgba(WIDTH, HEIGHT, &rgba_a);
    let img_b = ImageBuffer::from_rgba(WIDTH, HEIGHT, &rgba_b);

    let score = dssim(&img_a, &img_b);
    assert!(
        score < 0.001,
        "vertical reference renderer should be deterministic, got DSSIM = {score}"
    );
}

/// Renders a CJK string vertically and asserts the output is not all-white.
#[test]
fn vertical_cjk_reference_produces_non_trivial_output() {
    let rgba = render_vertical_cjk_reference(SAMPLE_TEXT, FONT_FAMILY, FONT_SIZE, WIDTH, HEIGHT);
    let img = ImageBuffer::from_rgba(WIDTH, HEIGHT, &rgba);

    let white_count = img.pixels.iter().filter(|&&p| p == 255).count();
    let total = img.pixels.len();
    assert!(
        white_count < total,
        "vertical reference output is all-white ({white_count}/{total} pixels); \
         no glyphs were drawn"
    );
}

/// Renders a CJK string horizontally and asserts the output is not all-white.
#[test]
fn horizontal_cjk_reference_produces_non_trivial_output() {
    let rgba = render_horizontal_cjk_reference(SAMPLE_TEXT, FONT_FAMILY, FONT_SIZE, WIDTH, HEIGHT);
    let img = ImageBuffer::from_rgba(WIDTH, HEIGHT, &rgba);

    let white_count = img.pixels.iter().filter(|&&p| p == 255).count();
    let total = img.pixels.len();
    assert!(
        white_count < total,
        "horizontal reference output is all-white ({white_count}/{total} pixels); \
         no glyphs were drawn"
    );
}

// ---------------------------------------------------------------------------
// Martensite TinySkia vertical rasterizer
// ---------------------------------------------------------------------------

/// Rasterizes CJK text in vertical writing mode using Martensite's own
/// text pipeline (cosmic-text shaping + swash glyph outlines + TinySkia
/// rasterization).
///
/// Glyphs are stacked top-to-bottom in a single vertical column, centered
/// horizontally. Upright CJK glyphs (per UAX #50) are placed without
/// rotation; rotated glyphs (Latin, digits) are not specially handled in
/// this minimal rasterizer — they are placed upright, which is acceptable
/// for a DSSIM comparison against Pango's vertical output since the test
/// text is pure CJK.
///
/// Returns straight RGBA8 (unpremultiplied) to match the Pango reference
/// format.
fn rasterize_vertical_text_martensite(
    text: &str,
    font_family: &str,
    font_size: f32,
    width: u32,
    height: u32,
) -> Vec<u8> {
    use cosmic_text::{Attrs, Family, Metrics, Shaping};
    use martensite_text::FontManager;

    // Create a font system that discovers system fonts (including Noto CJK
    // on CI runners with `fonts-noto-cjk` installed).
    let mut manager = FontManager::new();
    let font_system = manager.system_mut();

    // Shape the text with cosmic-text to get glyph IDs, positions, and
    // font face IDs. We shape horizontally — cosmic-text does not support
    // vertical writing mode natively — then manually stack the glyphs
    // vertically for rendering.
    let mut buffer =
        cosmic_text::Buffer::new(font_system, Metrics::new(font_size, font_size * 1.4));
    buffer.set_size(Some(width as f32), Some(height as f32));
    let attrs = Attrs::new().family(Family::Name(font_family));
    buffer.set_text(text, &attrs, Shaping::Advanced, None);
    buffer.shape_until_scroll(font_system, false);

    // Collect font data: for each unique font_id encountered, extract the
    // raw font bytes from fontdb so we can build FontResource instances for
    // the TinySkia backend.
    let db = manager.system().db();
    let mut font_cache: std::collections::HashMap<cosmic_text::fontdb::ID, FontResource> =
        std::collections::HashMap::new();

    for run in buffer.layout_runs() {
        for glyph in run.glyphs {
            let face_id = glyph.font_id;
            if font_cache.contains_key(&face_id) {
                continue;
            }
            // Extract font bytes from fontdb.
            let mut font_data: Option<(Vec<u8>, u32)> = None;
            db.with_face_data(face_id, |data, index| {
                font_data = Some((data.to_vec(), index));
            });
            if let Some((bytes, index)) = font_data {
                font_cache.insert(face_id, FontResource::new(bytes, index));
            }
        }
    }

    // Build a TinySkia backend and render the glyphs vertically.
    let mut backend = TinySkiaBackend::new(width, height)
        .expect("pixmap allocation should succeed for reasonable dimensions");

    // Note: TinySkiaBackend::render() calls self.clear() at the start,
    // so we must put the white background and the glyphs in the SAME
    // PaintList. Using two separate render() calls would wipe the
    // background on the second call.

    let column_x = width as f32 / 2.0;
    let mut cursor_y = font_size; // Start below the top edge with some padding.

    let mut paint_list = PaintList::new();
    let black = [0, 0, 0, 255];

    // White background (first command, so it's drawn before the glyphs).
    paint_list.push_fill_rect(
        kurbo::Rect::new(0.0, 0.0, width as f64, height as f64),
        [255, 255, 255, 255],
    );

    for run in buffer.layout_runs() {
        // Group glyphs by font_id so each GlyphRun uses one FontResource.
        let mut current_font_id: Option<cosmic_text::fontdb::ID> = None;
        let mut current_run: Option<GlyphRun> = None;

        for glyph in run.glyphs {
            let face_id = glyph.font_id;

            // If the font changed, flush the current run.
            if current_font_id.is_some() && current_font_id != Some(face_id) {
                if let Some(r) = current_run.take() {
                    paint_list.push_glyph_run(r);
                }
                current_font_id = Some(face_id);
                current_run = None;
            }

            if current_font_id.is_none() {
                current_font_id = Some(face_id);
            }

            // Get the font resource for this glyph.
            let font = match font_cache.get(&face_id) {
                Some(f) => f.clone(),
                None => continue,
            };

            // Start a new run if needed.
            if current_run.is_none() {
                let mut r = GlyphRun::new(font_size, black).with_font(font);
                // Place the glyph: x = column center (approximate), y = cursor.
                // The glyph's baseline position in vertical mode is (column_x, cursor_y).
                r.push(GlyphInstance::new(
                    column_x,
                    cursor_y,
                    u32::from(glyph.glyph_id),
                    glyph.w,
                    font_size,
                ));
                current_run = Some(r);
            } else if let Some(r) = current_run.as_mut() {
                r.push(GlyphInstance::new(
                    column_x,
                    cursor_y,
                    u32::from(glyph.glyph_id),
                    glyph.w,
                    font_size,
                ));
            }

            // Advance the cursor vertically by the glyph's horizontal width.
            cursor_y += glyph.w.max(font_size * 0.5);
        }

        // Flush the last run.
        if let Some(r) = current_run.take() {
            paint_list.push_glyph_run(r);
        }

        // Add line spacing between layout runs.
        cursor_y += font_size * 0.4;
    }

    backend.render(&paint_list);

    // Convert TinySkia's premultiplied RGBA8 to straight RGBA8.
    let premul = backend.pixels();
    let pixel_count = (width as usize) * (height as usize);
    let mut rgba = vec![0u8; pixel_count * 4];
    for i in 0..pixel_count {
        let r = premul[i * 4];
        let g = premul[i * 4 + 1];
        let b = premul[i * 4 + 2];
        let a = premul[i * 4 + 3];
        let a_safe = a.max(1);
        rgba[i * 4] = ((r as u16 * 255 + a_safe as u16 / 2) / a_safe as u16) as u8;
        rgba[i * 4 + 1] = ((g as u16 * 255 + a_safe as u16 / 2) / a_safe as u16) as u8;
        rgba[i * 4 + 2] = ((b as u16 * 255 + a_safe as u16 / 2) / a_safe as u16) as u8;
        rgba[i * 4 + 3] = a;
    }

    rgba
}

/// Renders CJK text vertically through both Pango (reference) and
/// Martensite (TinySkia), then compares via DSSIM.
///
/// This is a genuine cross-engine comparison: Pango uses HarfBuzz +
/// FreeType for shaping and rasterization, while Martensite uses
/// cosmic-text + swash + TinySkia. The two engines differ on:
///
/// - Anti-aliased edge coverage (different AA algorithms)
/// - Hinting (FreeType hinting vs. swash outline scaling)
/// - Subpixel positioning (Pango uses fixed-point, cosmic-text uses f32)
/// - Glyph metrics (different font tables may be consulted)
/// - Vertical metrics (Pango uses proper vertical baseline; Martensite
///   stacks glyphs manually)
///
/// The threshold is DSSIM < 0.10 (similarity > 0.90), which catches gross
/// rendering failures (missing glyphs, wrong positions, wrong colors)
/// while tolerating the expected sub-pixel divergence between two
/// independent rasterizers.
///
/// This test is `#[ignore]`-gated because it requires:
/// - `fonts-noto-cjk` installed (for `Noto Sans CJK JP`)
/// - Pango's vertical gravity (`Gravity::East`) to work, which depends on
///   the platform's Pango build and font support
/// - The Martensite vertical rasterizer to produce positionally comparable
///   output, which is approximate since cosmic-text does not support
///   vertical writing mode natively
///
/// Run in CI with: `cargo test -p martensite-text-reference -- --ignored`
#[test]
#[ignore = "CI-only: requires fonts-noto-cjk and Pango vertical gravity support"]
fn vertical_cjk_martensite_vs_pango_dssim() {
    let pango_pixels =
        render_vertical_cjk_reference(SAMPLE_TEXT, FONT_FAMILY, FONT_SIZE, WIDTH, HEIGHT);
    let martensite_pixels = rasterize_vertical_text_martensite(
        SAMPLE_TEXT,
        FONT_FAMILY,
        FONT_SIZE as f32,
        WIDTH,
        HEIGHT,
    );

    let pango_img = ImageBuffer::from_rgba(WIDTH, HEIGHT, &pango_pixels);
    let martensite_img = ImageBuffer::from_rgba(WIDTH, HEIGHT, &martensite_pixels);

    let score = dssim(&pango_img, &martensite_img);
    eprintln!("vertical_cjk_martensite_vs_pango_dssim: DSSIM = {score:.4}");
    assert!(
        score < 0.10,
        "Martensite vertical CJK render diverges too far from Pango reference: \
         DSSIM = {score:.4} (threshold < 0.10)"
    );
}

/// Verifies that the Martensite vertical rasterizer produces non-trivial
/// output (not all-white), i.e. glyphs were actually drawn.
#[test]
fn martensite_vertical_rasterizer_produces_non_trivial_output() {
    let rgba = rasterize_vertical_text_martensite(
        SAMPLE_TEXT,
        FONT_FAMILY,
        FONT_SIZE as f32,
        WIDTH,
        HEIGHT,
    );
    let img = ImageBuffer::from_rgba(WIDTH, HEIGHT, &rgba);

    let white_count = img.pixels.iter().filter(|&&p| p == 255).count();
    let total = img.pixels.len();
    assert!(
        white_count < total,
        "Martensite vertical rasterizer output is all-white ({white_count}/{total} pixels); \
         no glyphs were drawn"
    );
}
