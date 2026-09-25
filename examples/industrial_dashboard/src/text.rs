//! Shared text emission for the workstation panels.
//!
//! `PaintList::push_text` emits `DrawText`, which the render backends
//! approximate as bounding-box rectangles — real glyph outlines go
//! through `DrawGlyphRun` + `FontResource`. `TextPainter` wraps the
//! system-font discovery and shaping the same way `Text::paint` does:
//! `shape_text`/`shape_text_with_attrs` for layout,
//! `FontManager::font_data` to attach resolved font bytes, one
//! `GlyphRun` per (line, font) segment.
//!
//! The `*_styled` variants carry the B2 style axis — semibold titles,
//! tracking — through `martensite::core::TextStyle`. Every emitted
//! `GlyphRun` is tagged with its style so the paint audit's WCAG
//! large-text rule (≥18pt *or* ≥14pt bold) can see the weight.
//!
//! Residual divergence vs `martensite::text_paint::TextPainter`: this
//! fork adds `push_colored` (per-span syntax highlighting) and
//! `fit`/`column_at` (editor hit-testing). Porting those three up into
//! the crate would let the dashboard drop the fork entirely — tracked
//! as a follow-up.

use martensite::core::{FontResource, GlyphInstance, GlyphRun, TextStyle};
use martensite::render::{PaintList, Point};
use martensite::text::{shape_text_with_attrs, Attrs, FontManager, Style, Weight};

/// A colored byte-range override for [`TextPainter::push_colored`]: every
/// glyph whose `start` byte offset falls inside `[start, end)` is drawn in
/// `color` instead of the base color. Used by the editor panel for syntax
/// highlighting straight off `CodeEditor::highlight_line`.
pub struct SpanColor {
    pub start: usize,
    pub end: usize,
    pub color: [u8; 4],
}

/// Builds cosmic-text [`Attrs`] for a [`TextStyle`] — the same mapping
/// `martensite::text_paint` applies internally.
fn attrs_for(style: TextStyle) -> Attrs<'static> {
    let mut attrs = Attrs::new()
        .weight(Weight(style.weight.0))
        .style(if style.italic {
            Style::Italic
        } else {
            Style::Normal
        });
    if let Some(em) = style.letter_spacing {
        attrs = attrs.letter_spacing(em);
    }
    attrs
}

/// Owns font discovery + shaping for one widget. `FontManager` creation
/// scans the system font set, so panels construct it lazily on first use
/// (the same pattern `Text` uses).
pub struct TextPainter {
    fonts: FontManager,
}

impl Default for TextPainter {
    fn default() -> Self {
        Self::new()
    }
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
        self.push_styled(
            list,
            origin,
            text,
            size,
            color,
            max_width,
            TextStyle::REGULAR,
        )
    }

    /// [`push`](Self::push) with an explicit style axis — semibold panel
    /// titles (`FontWeight::SEMIBOLD`), tracked caps, italics.
    #[allow(clippy::too_many_arguments)]
    pub fn push_styled(
        &mut self,
        list: &mut PaintList,
        origin: Point,
        text: &str,
        size: f32,
        color: [u8; 4],
        max_width: Option<f32>,
        style: TextStyle,
    ) -> f64 {
        self.emit(list, origin, text, size, |_| color, max_width, style)
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
        self.push_colored_styled(
            list,
            origin,
            text,
            size,
            base,
            spans,
            max_width,
            TextStyle::REGULAR,
        )
    }

    /// [`push_colored`](Self::push_colored) with an explicit style axis.
    #[allow(clippy::too_many_arguments)]
    pub fn push_colored_styled(
        &mut self,
        list: &mut PaintList,
        origin: Point,
        text: &str,
        size: f32,
        base: [u8; 4],
        spans: &[SpanColor],
        max_width: Option<f32>,
        style: TextStyle,
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
            style,
        )
    }

    /// Truncates `text` to `max` device px with an ellipsis. For
    /// single-line slots (title bars, cells, hints) where a wrapped
    /// second line would overflow its allotted height and collide with
    /// the content below — the audit's `TextOverlap` check catches the
    /// wrap, but fitting first keeps the layout deterministic.
    pub fn fit(&mut self, text: &str, size: f32, max: f32) -> String {
        self.fit_styled(text, size, max, TextStyle::REGULAR)
    }

    /// [`fit`](Self::fit) with an explicit style axis — semibold titles
    /// measure wider, so fitting must shape with the same style that
    /// paints.
    pub fn fit_styled(&mut self, text: &str, size: f32, max: f32, style: TextStyle) -> String {
        let ell = "…";
        let ew = self.measure_styled(ell, size, style);
        let attrs = attrs_for(style);
        // Single shape pass — cut at the first glyph whose right edge
        // would leave no room for the ellipsis (avoids the O(n²)
        // measure-per-pop loop).
        let mut cut = None;
        for line in shape_text_with_attrs(&mut self.fonts, text, &attrs, size, size * 1.25, None) {
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
        self.measure_styled(text, size, TextStyle::REGULAR)
    }

    /// [`measure`](Self::measure) with an explicit style axis — a
    /// semibold string is wider than the same string at regular weight.
    pub fn measure_styled(&mut self, text: &str, size: f32, style: TextStyle) -> f32 {
        let mut end = 0.0f32;
        for line in shape_text_with_attrs(
            &mut self.fonts,
            text,
            &attrs_for(style),
            size,
            size * 1.25,
            None,
        ) {
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
        self.column_at_styled(text, size, x, TextStyle::REGULAR)
    }

    /// [`column_at`](Self::column_at) with an explicit style axis.
    pub fn column_at_styled(&mut self, text: &str, size: f32, x: f32, style: TextStyle) -> usize {
        let mut best_col = 0usize;
        let mut best_dist = f32::MAX;
        for line in shape_text_with_attrs(
            &mut self.fonts,
            text,
            &attrs_for(style),
            size,
            size * 1.25,
            None,
        ) {
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

    #[allow(clippy::too_many_arguments)]
    fn emit(
        &mut self,
        list: &mut PaintList,
        origin: Point,
        text: &str,
        size: f32,
        color_for: impl Fn(usize) -> [u8; 4],
        max_width: Option<f32>,
        style: TextStyle,
    ) -> f64 {
        let mut block_bottom = origin.y;
        for line in shape_text_with_attrs(
            &mut self.fonts,
            text,
            &attrs_for(style),
            size,
            size * 1.25,
            max_width,
        ) {
            block_bottom = f64::from(origin.y as f32 + line.line_y + size * 1.25);
            let baseline_y = origin.y as f32 + line.line_y;
            let mut run = GlyphRun::new(size, color_for(0)).with_style(style);
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
                    run = GlyphRun::new(g.font_size, color).with_style(style);
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

/// The semibold weight for primary/standard panel titles per the B1
/// type scale — `martensite::core::FontWeight::SEMIBOLD`.
pub const TITLE_WEIGHT: martensite::core::FontWeight = martensite::core::FontWeight::SEMIBOLD;
