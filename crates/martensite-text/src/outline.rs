//! Glyph outline extraction — `swash` outlines as kurbo [`BezPath`](kurbo::BezPath)s.
//!
//! The paint command set has no rotation/transform op, so labels that
//! must render rotated (axis captions, edge tabs) emit their shaped
//! outlines under `FillPath` instead — every backend fills a path the
//! same way, so the rotation renders identically everywhere.
//!
//! Returned paths are in *run-local* device coordinates: `x` advances
//! along the text direction and `y = 0` is the baseline, with
//! ascenders extending into negative y — the same convention
//! `DrawGlyphRun` uses (swash outlines are y-up; they are flipped
//! here).

use kurbo::BezPath;
use swash::scale::ScaleContext;
use swash::zeno::Verb;
use swash::FontRef;

/// Outline of `glyph_id` in font `data` (face `index`) scaled to
/// `size_px`, as a [`BezPath`] in run-local device coordinates — see
/// the module docs for the axis convention.
///
/// `None` when the font fails to parse, the glyph has no outline
/// (spaces, missing glyphs), or the outline is empty. Swash parsing is
/// wrapped in `catch_unwind` — malformed font data is a known panic
/// source (swash #123–#126).
///
/// # Examples
///
/// ```
/// // `None` for unparseable data — callers degrade to plain text.
/// assert!(martensite_text::outline::glyph_outline(&[], 0, 3, 14.0).is_none());
/// ```
pub fn glyph_outline(data: &[u8], index: u32, glyph_id: u16, size_px: f32) -> Option<BezPath> {
    std::panic::catch_unwind(|| outline_inner(data, index, glyph_id, size_px))
        .ok()
        .flatten()
}

fn outline_inner(data: &[u8], index: u32, glyph_id: u16, size_px: f32) -> Option<BezPath> {
    let font = FontRef::from_index(data, usize::try_from(index).ok()?)?;
    let mut ctx = ScaleContext::new();
    let mut scaler = ctx.builder(font).size(size_px).build();
    let outline = scaler.scale_outline(glyph_id)?;
    // A non-color font has a single outline layer; color fonts (COLR)
    // are out of scope — the first layer is enough for label ink.
    let layer = outline.get(0)?;
    let points = layer.points();
    let verbs = layer.verbs();
    let mut path = BezPath::new();
    let mut i = 0usize;
    // Font space is y-up; run-local device space is y-down — flip.
    let pt = |p: swash::zeno::Point| (f64::from(p.x), f64::from(-p.y));
    for verb in verbs {
        match verb {
            Verb::MoveTo => {
                path.move_to(pt(*points.get(i)?));
                i += 1;
            }
            Verb::LineTo => {
                path.line_to(pt(*points.get(i)?));
                i += 1;
            }
            Verb::QuadTo => {
                let c = pt(*points.get(i)?);
                let p = pt(*points.get(i + 1)?);
                i += 2;
                path.quad_to(c, p);
            }
            Verb::CurveTo => {
                let c1 = pt(*points.get(i)?);
                let c2 = pt(*points.get(i + 1)?);
                let p = pt(*points.get(i + 2)?);
                i += 3;
                path.curve_to(c1, c2, p);
            }
            Verb::Close => path.close_path(),
        }
    }
    Some(path)
}
