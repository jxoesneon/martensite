//! Shared text emission for the workstation panels.
//!
//! `PaintList::push_text` emits `DrawText`, which the render backends
//! approximate as bounding-box rectangles — real glyph outlines go
//! through `DrawGlyphRun` + `FontResource`. `TextPainter` wraps the
//! system-font discovery and shaping the same way `Text::paint` does:
//! `shape_text` for layout, `FontManager::font_data` to attach resolved
//! font bytes, one `GlyphRun` per (line, font) segment.

use martensite::core::{FontResource, GlyphInstance, GlyphRun};
use martensite::render::{PaintList, Point};
use martensite::text::{shape_text, FontManager};

/// A colored byte-range override for [`TextPainter::push_colored`]: every
/// glyph whose `start` byte offset falls inside `[start, end)` is drawn in
/// `color` instead of the base color. Used by the editor panel for syntax
/// highlighting straight off `CodeEditor::highlight_line`.
pub struct SpanColor {
    pub start: usize,
    pub end: usize,
    pub color: [u8; 4],
}

/// Owns font discovery + shaping for one widget. `FontManager` creation
/// scans the system font set, so panels construct it lazily on first use
/// (the same pattern `Text` uses).
pub struct TextPainter {
    fonts: FontManager,
}

impl TextPainter {
    pub fn new() -> Self {
        Self {
            fonts: FontManager::new(),
        }
    }

    /// Shapes `text` and emits `DrawGlyphRun` commands; returns the y
    /// coordinate of the rendered block's bottom so callers can stack
    /// text without colliding. `origin` is the top-left of the block;
    /// `line_y` carries each line's baseline offset.
    pub fn push(
        &mut self,
        list: &mut PaintList,
        origin: Point,
        text: &str,
        size: f32,
        color: [u8; 4],
        max_width: Option<f32>,
    ) -> f64 {
        self.emit(list, origin, text, size, |_| color, max_width)
    }

    /// Like [`push`](Self::push), but each glyph's color comes from the
    /// span in `spans` containing its byte offset (base color when no
    /// span matches). Byte offsets come from `ShapedGlyph::start`, which
    /// is exactly the coordinate space `HighlightedSpan` uses.
    #[allow(clippy::too_many_arguments)]
    pub fn push_colored(
        &mut self,
        list: &mut PaintList,
        origin: Point,
        text: &str,
        size: f32,
        base: [u8; 4],
        spans: &[SpanColor],
        max_width: Option<f32>,
    ) -> f64 {
        self.emit(
            list,
            origin,
            text,
            size,
            |byte| {
                spans
                    .iter()
                    .find(|s| byte >= s.start && byte < s.end)
                    .map(|s| s.color)
                    .unwrap_or(base)
            },
            max_width,
        )
    }

    /// Truncates `text` to `max` device px with an ellipsis. For
    /// single-line slots (title bars, cells, hints) where a wrapped
    /// second line would overflow its allotted height and collide with
    /// the content below — the audit's `TextOverlap` check catches the
    /// wrap, but fitting first keeps the layout deterministic.
    pub fn fit(&mut self, text: &str, size: f32, max: f32) -> String {
        let ell = "…";
        let ew = self.measure(ell, size);
        // Single shape pass — cut at the first glyph whose right edge
        // would leave no room for the ellipsis (avoids the O(n²)
        // measure-per-pop loop).
        let mut cut = None;
        for line in shape_text(&mut self.fonts, text, size, size * 1.25, None) {
            for g in &line.glyphs {
                if g.x + g.w > max - ew {
                    cut = Some(g.start);
                    break;
                }
            }
            if cut.is_some() {
                break;
            }
        }
        match cut {
            None => text.to_string(),
            Some(byte) => format!("{}{ell}", &text[..byte]),
        }
    }

    /// Advance width of `text` at `size` — used for right-aligned columns
    /// and caret positioning. Shapes a single line with no wrap.
    pub fn measure(&mut self, text: &str, size: f32) -> f32 {
        let mut end = 0.0f32;
        for line in shape_text(&mut self.fonts, text, size, size * 1.25, None) {
            for g in &line.glyphs {
                end = end.max(g.x + g.w);
            }
        }
        end
    }

    /// Column (char index) in `text` nearest to `x` pixels — caret
    /// placement for the editor panel. Walks the shaped glyphs once and
    /// returns the char index of the glyph whose left edge is closest.
    pub fn column_at(&mut self, text: &str, size: f32, x: f32) -> usize {
        let mut best_col = 0usize;
        let mut best_dist = f32::MAX;
        for line in shape_text(&mut self.fonts, text, size, size * 1.25, None) {
            for g in &line.glyphs {
                for (dist, byte) in [(g.x, g.start), (g.x + g.w, g.end)] {
                    let d = (dist - x).abs();
                    if d < best_dist {
                        best_dist = d;
                        best_col = text[..byte.min(text.len())].chars().count();
                    }
                }
            }
        }
        best_col
    }

    fn emit(
        &mut self,
        list: &mut PaintList,
        origin: Point,
        text: &str,
        size: f32,
        color_for: impl Fn(usize) -> [u8; 4],
        max_width: Option<f32>,
    ) -> f64 {
        let mut block_bottom = origin.y;
        for line in shape_text(&mut self.fonts, text, size, size * 1.25, max_width) {
            block_bottom = f64::from(origin.y as f32 + line.line_y + size * 1.25);
            let baseline_y = origin.y as f32 + line.line_y;
            let mut run = GlyphRun::new(size, color_for(0));
            let mut run_font = None;
            let mut run_color = run.color;
            for g in &line.glyphs {
                let color = color_for(g.start);
                let boundary = run_font.is_some_and(|prev| prev != g.font_id) || color != run_color;
                if boundary {
                    if let Some(font_id) = run_font {
                        if let Some((data, index)) = self.fonts.font_data(font_id) {
                            run.set_font(FontResource::new(data, index));
                        }
                        list.push_glyph_run(run);
                    }
                    run = GlyphRun::new(g.font_size, color);
                    run_color = color;
                }
                run.font_size = g.font_size;
                run_font = Some(g.font_id);
                run.push(GlyphInstance::new(
                    origin.x as f32 + g.x,
                    baseline_y + g.y,
                    u32::from(g.glyph_id),
                    g.w,
                    line.line_height,
                ));
            }
            if let Some(font_id) = run_font.filter(|_| !run.is_empty()) {
                if let Some((data, index)) = self.fonts.font_data(font_id) {
                    run.set_font(FontResource::new(data, index));
                }
                list.push_glyph_run(run);
            }
        }
        block_bottom
    }
}
