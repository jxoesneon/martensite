//! External Pango/Cairo reference renderer for vertical-CJK DSSIM comparison.
//!
//! This crate provides a genuine external-reference rendering path using
//! Pango and Cairo to produce raster output for CJK text in both vertical
//! and horizontal writing modes. The output is intended to be compared
//! against Martensite's own rendering via the DSSIM perceptual metric in
//! `martensite-test`.
//!
//! `cosmic-text` does not support vertical writing mode natively, so it
//! cannot serve as the reference. Pango supports vertical text via
//! `gravity = EAST` and `gravity_hint = STRONG`.
//!
//! # System dependencies
//!
//! This crate requires the following system libraries to be installed:
//!
//! - `libcairo2-dev`
//! - `libpango1.0-dev`
//! - `libglib2.0-dev`
//! - `pkg-config`
//! - `fonts-noto-cjk` (for CJK glyph coverage)
//!
//! On Debian/Ubuntu:
//!
//! ```sh
//! sudo apt-get install -y libcairo2-dev libpango1.0-dev \
//!   libglib2.0-dev pkg-config fonts-noto-cjk
//! ```
//!
//! Because these are heavy native dependencies, this crate is isolated
//! from normal consumers and is opt-in via the workspace member list. It
//! is `publish = false`.

#![forbid(unsafe_code)]
#![deny(missing_docs)]

use cairo::{Context, Format, ImageSurface};
use pango::{FontDescription, Gravity, GravityHint};

/// Pango's fixed-point scale factor (1 point = 1024 Pango units).
const PANGO_SCALE: i32 = 1024;

/// Renders CJK text in vertical writing mode (`writing-mode: vertical-rl`)
/// using Pango with `gravity = EAST` and `gravity_hint = STRONG`, producing
/// an RGBA8 pixel buffer.
///
/// The surface is filled with an opaque white background before the text
/// is drawn in black, matching the convention used by Martensite's
/// golden-frame tests.
///
/// # Arguments
///
/// - `text`: The CJK (or mixed) string to render.
/// - `font_family`: The Pango font family (e.g. `"Noto Sans CJK JP"`).
/// - `font_size`: Font size in points.
/// - `width`: Surface width in pixels.
/// - `height`: Surface height in pixels.
///
/// # Returns
///
/// A `Vec<u8>` of length `width * height * 4` in RGBA8 byte order
/// (red, green, blue, alpha), with straight (non-premultiplied) alpha.
///
/// # Examples
///
/// ```
/// use martensite_text_reference::render_vertical_cjk_reference;
///
/// let rgba = render_vertical_cjk_reference("日本語", "Noto Sans CJK JP", 24.0, 128, 256);
/// assert_eq!(rgba.len(), 128 * 256 * 4);
/// ```
pub fn render_vertical_cjk_reference(
    text: &str,
    font_family: &str,
    font_size: f64,
    width: u32,
    height: u32,
) -> Vec<u8> {
    render_cjk_reference(text, font_family, font_size, width, height, true)
}

/// Renders CJK text in horizontal writing mode using Pango, producing an
/// RGBA8 pixel buffer. This serves as a comparison baseline for the
/// vertical renderer.
///
/// See [`render_vertical_cjk_reference`] for argument and return details.
///
/// # Examples
///
/// ```
/// use martensite_text_reference::render_horizontal_cjk_reference;
///
/// let rgba = render_horizontal_cjk_reference("日本語", "Noto Sans CJK JP", 24.0, 256, 128);
/// assert_eq!(rgba.len(), 256 * 128 * 4);
/// ```
pub fn render_horizontal_cjk_reference(
    text: &str,
    font_family: &str,
    font_size: f64,
    width: u32,
    height: u32,
) -> Vec<u8> {
    render_cjk_reference(text, font_family, font_size, width, height, false)
}

/// Internal helper that drives the Cairo/Pango rendering pipeline.
fn render_cjk_reference(
    text: &str,
    font_family: &str,
    font_size: f64,
    width: u32,
    height: u32,
    vertical: bool,
) -> Vec<u8> {
    let w = width as i32;
    let h = height as i32;
    let surface =
        ImageSurface::create(Format::ARgb32, w, h).expect("failed to create Cairo image surface");
    let cr = Context::new(&surface).expect("failed to create Cairo context");

    // White background.
    cr.set_source_rgba(1.0, 1.0, 1.0, 1.0);
    cr.paint().expect("failed to paint white background");

    // Black text (Pango uses the Cairo source as the default text color).
    cr.set_source_rgba(0.0, 0.0, 0.0, 1.0);

    let layout = if vertical {
        // Create a context with vertical gravity BEFORE creating the
        // layout, so the layout uses vertical mode from the start.
        // Using `layout.context()` after creation does not reliably
        // propagate gravity changes on all platforms.
        let context = pangocairo::create_context(&cr);
        context.set_base_gravity(Gravity::East);
        context.set_gravity_hint(GravityHint::Strong);
        pango::Layout::new(&context)
    } else {
        pangocairo::create_layout(&cr)
    };

    let mut font_desc = FontDescription::new();
    font_desc.set_family(font_family);
    font_desc.set_size((font_size * PANGO_SCALE as f64) as i32);
    layout.set_font_description(Some(&font_desc));

    // Constrain the layout so Pango wraps. In vertical mode the layout
    // "width" corresponds to the vertical extent (column height).
    layout.set_width(w * PANGO_SCALE);
    layout.set_text(text);

    // Draw at the origin; Pango handles vertical-rl column progression
    // internally via the context gravity.
    cr.move_to(0.0, 0.0);
    pangocairo::show_layout(&cr, &layout);

    let stride = surface.stride() as usize;

    // Drop the context and layout so all pending drawing is flushed
    // before we read back the pixel data.
    drop(layout);
    drop(cr);

    let pixel_count = (width as usize) * (height as usize);
    let mut rgba = vec![0u8; pixel_count * 4];
    surface
        .with_data(|data| {
            for y in 0..height as usize {
                for x in 0..width as usize {
                    let offset = y * stride + x * 4;
                    let pixel = u32::from_ne_bytes([
                        data[offset],
                        data[offset + 1],
                        data[offset + 2],
                        data[offset + 3],
                    ]);
                    let a = (pixel >> 24) & 0xff;
                    let r = (pixel >> 16) & 0xff;
                    let g = (pixel >> 8) & 0xff;
                    let b = pixel & 0xff;
                    let out = (y * width as usize + x) * 4;
                    // Unpremultiply (round-to-nearest). Guard against zero
                    // alpha with max(1); when alpha is zero the premultiplied
                    // channels are also zero, so the result is correct.
                    let a_safe = a.max(1);
                    rgba[out] = ((r * 255 + a_safe / 2) / a_safe) as u8;
                    rgba[out + 1] = ((g * 255 + a_safe / 2) / a_safe) as u8;
                    rgba[out + 2] = ((b * 255 + a_safe / 2) / a_safe) as u8;
                    rgba[out + 3] = a as u8;
                }
            }
        })
        .expect("failed to read surface data");

    rgba
}
