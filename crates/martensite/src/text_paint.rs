//! Shared shaped-text emission for widgets whose labels change at
//! paint time.
//!
//! [`PaintList::push_text`](martensite_core::paint::PaintList::push_text)
//! emits `DrawText`, which the render backends
//! approximate as opaque bounding-box rectangles — real glyph outlines
//! go through `DrawGlyphRun` + `FontResource`. `Widget::paint` takes
//! `&self`, so widgets cannot own a `FontManager` and shape lazily the
//! way `Text` does; `TextPainter` is the interior-mutable escape hatch:
//! wrap it in a [`SharedTextPainter`](crate::text_paint::SharedTextPainter)
//! (`Arc<Mutex<…>>`), hand clones to
//! every widget that should paint real text, and each `paint` locks it
//! briefly to shape.
//!
//! Without a painter widgets keep emitting `DrawText` placeholder
//! boxes — honest degraded output, documented in each widget's `paint`
//! rather than silently changed.

use std::sync::Arc;

use parking_lot::Mutex;

use kurbo::Point;
use martensite_core::paint::{FontResource, GlyphInstance, GlyphRun, PaintList, TextStyle};
use martensite_text::{shape_text, shape_text_with_attrs, Attrs, FontManager, Style, Weight};

/// Builds cosmic-text [`Attrs`] for a [`TextStyle`] — the single place
/// the core weight/slant/tracking axis maps onto `fontdb` values.
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

/// A shared, lockable [`TextPainter`].
///
/// Cheap to clone; every clone shares one `FontManager` so font
/// discovery happens once per app rather than once per widget. The
/// mutex is `parking_lot` — poison-free, so `paint` never unwraps.
/// Newtype (rather than a bare `Arc<Mutex>`) so it can implement
/// [`martensite_core::paint::TextShaper`], which lets it serve both as
/// a widget's explicit painter and as the arena's ambient one.
#[derive(Clone)]
pub struct SharedTextPainter(Arc<Mutex<TextPainter>>);

/// Creates a [`SharedTextPainter`] — convenience for the `Arc<Mutex>`
/// ceremony.
///
/// # Examples
///
/// ```
/// use martensite::text_paint::shared_painter;
///
/// let painter = shared_painter();
/// ```
pub fn shared_painter() -> SharedTextPainter {
    SharedTextPainter(Arc::new(Mutex::new(TextPainter::new())))
}

/// WCAG relative luminance of an sRGB channel.
fn channel_lum(c: u8) -> f32 {
    let v = c as f32 / 255.0;
    if v <= 0.04045 {
        v / 12.92
    } else {
        ((v + 0.055) / 1.055).powf(2.4)
    }
}

/// WCAG relative luminance of an opaque sRGB color (`[r, g, b, a]`).
///
/// Widgets that paint text or edges over arbitrary data colors (tile
/// grids, charts, calendars) use this to choose readable ink.
pub(crate) fn relative_luminance(c: [u8; 4]) -> f32 {
    0.2126 * channel_lum(c[0]) + 0.7152 * channel_lum(c[1]) + 0.0722 * channel_lum(c[2])
}

/// Whichever of `a`/`b` contrasts better against `bg` (WCAG ratio).
/// Both candidates are treated as opaque over opaque `bg`.
pub(crate) fn better_ink(bg: [u8; 4], a: [u8; 4], b: [u8; 4]) -> [u8; 4] {
    let l_bg = relative_luminance(bg);
    let ra = (relative_luminance(a).max(l_bg) + 0.05) / (relative_luminance(a).min(l_bg) + 0.05);
    let rb = (relative_luminance(b).max(l_bg) + 0.05) / (relative_luminance(b).min(l_bg) + 0.05);
    if ra >= rb {
        a
    } else {
        b
    }
}

/// Owns font discovery + shaping for widget text. `FontManager`
/// creation scans the system font set, so construction defers it until
/// the first emission (the same lazy pattern `Text` uses).
pub struct TextPainter {
    fonts: Option<FontManager>,
}

impl Default for TextPainter {
    fn default() -> Self {
        Self::new()
    }
}

impl TextPainter {
    /// Creates a painter whose `FontManager` initializes on first use.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite::text_paint::TextPainter;
    ///
    /// let _painter = TextPainter::new();
    /// ```
    pub fn new() -> Self {
        Self { fonts: None }
    }

    fn fonts(&mut self) -> &mut FontManager {
        self.fonts.get_or_insert_with(FontManager::new)
    }

    /// Advance width of `text` at `size` — used for right-aligned cells
    /// and caret positioning. Shapes a single line with no wrap.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite::text_paint::TextPainter;
    ///
    /// let mut p = TextPainter::new();
    /// let w = p.measure("hello", 14.0);
    /// assert!(w >= 0.0);
    /// ```
    pub fn measure(&mut self, text: &str, size: f32) -> f32 {
        self.measure_styled(text, size, TextStyle::REGULAR)
    }

    /// [`measure`](Self::measure) with an explicit style axis — a
    /// semibold string is wider than the same string at regular weight,
    /// so styled callers must measure through this channel for caret
    /// placement and right-aligned columns to agree with painted output.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite::text_paint::TextPainter;
    /// use martensite_core::paint::{FontWeight, TextStyle};
    ///
    /// let mut p = TextPainter::new();
    /// let w = p.measure_styled("hi", 14.0, TextStyle::default().weight(FontWeight::BOLD));
    /// assert!(w >= 0.0);
    /// ```
    pub fn measure_styled(&mut self, text: &str, size: f32, style: TextStyle) -> f32 {
        let mut end = 0.0f32;
        for line in shape_text_with_attrs(
            self.fonts(),
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

    /// Device-pixel x of the insertion boundary before `byte_idx`,
    /// derived from one shape pass over the whole text so kerning and
    /// ligature context agree with `push` — this is the caret position
    /// for that byte offset. `byte_idx` is clamped to `text.len()`;
    /// boundaries inside a multi-byte cluster interpolate within the
    /// cluster's glyph. Assumes a single LTR line (text inputs, labels).
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite::text_paint::TextPainter;
    ///
    /// let mut p = TextPainter::new();
    /// let w = p.measure("hi", 14.0);
    /// assert!((p.caret_x("hi", 14.0, 2) - w).abs() < 1.0);
    /// ```
    pub fn caret_x(&mut self, text: &str, size: f32, byte_idx: usize) -> f32 {
        let byte_idx = byte_idx.min(text.len());
        let Some(line) = shape_text(self.fonts(), text, size, size * 1.25, None)
            .into_iter()
            .next()
        else {
            return 0.0;
        };
        let mut right_edge = 0.0f32;
        for g in &line.glyphs {
            if g.start >= byte_idx {
                // First glyph at or after the boundary — its left edge
                // is where the next character would begin.
                return g.x;
            }
            if g.end > byte_idx {
                // Boundary inside a cluster (e.g. a ligature) —
                // interpolate so round-tripping `byte_at` stays stable.
                let span = (g.end - g.start).max(1) as f32;
                return g.x + g.w * (byte_idx - g.start) as f32 / span;
            }
            right_edge = right_edge.max(g.x + g.w);
        }
        right_edge
    }

    /// Byte offset of the insertion boundary nearest device-pixel `x`,
    /// the inverse of [`caret_x`](Self::caret_x) — click and drag
    /// hit-testing. Always returns a boundary between clusters.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite::text_paint::TextPainter;
    ///
    /// let mut p = TextPainter::new();
    /// assert_eq!(p.byte_at("hi", 14.0, -10.0), 0);
    /// assert_eq!(p.byte_at("hi", 14.0, 10_000.0), 2);
    /// ```
    pub fn byte_at(&mut self, text: &str, size: f32, x: f32) -> usize {
        let Some(line) = shape_text(self.fonts(), text, size, size * 1.25, None)
            .into_iter()
            .next()
        else {
            return 0;
        };
        for g in &line.glyphs {
            if x < g.x + g.w * 0.5 {
                return g.start.min(text.len());
            }
            if x < g.x + g.w {
                return g.end.min(text.len());
            }
        }
        text.len()
    }

    /// Shapes `text` and emits `DrawGlyphRun` commands; returns the y
    /// coordinate of the rendered block's bottom so callers can stack
    /// text without colliding. `origin` is the top-left of the block.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite::text_paint::TextPainter;
    /// use kurbo::Point;
    /// use martensite_core::PaintList;
    ///
    /// let mut p = TextPainter::new();
    /// let mut list = PaintList::new();
    /// p.push(&mut list, Point::new(10.0, 10.0), "PAUSE", 14.0, [255; 4], None);
    /// ```
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

    /// [`push`](Self::push) with an explicit style axis — semibold
    /// titles, true italics, tracked caps. The style is carried on each
    /// emitted [`GlyphRun`] as metadata so paint-level audits can see
    /// the weight axis (WCAG large text is ≥18pt *or* ≥14pt bold).
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite::text_paint::TextPainter;
    /// use kurbo::Point;
    /// use martensite_core::PaintList;
    /// use martensite_core::paint::{FontWeight, TextStyle};
    ///
    /// let mut p = TextPainter::new();
    /// let mut list = PaintList::new();
    /// let title = TextStyle::default().weight(FontWeight::SEMIBOLD);
    /// p.push_styled(&mut list, Point::new(10.0, 10.0), "ALARMS", 15.0, [255; 4], None, title);
    /// ```
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
        if text.is_empty() || size <= 0.0 {
            return origin.y;
        }
        let mut block_bottom = origin.y;
        let fonts = self.fonts();
        for line in
            shape_text_with_attrs(fonts, text, &attrs_for(style), size, size * 1.25, max_width)
        {
            block_bottom = origin.y + f64::from(line.line_y + size * 1.25);
            let baseline_y = origin.y as f32 + line.line_y;
            let mut run = GlyphRun::new(size, color).with_style(style);
            let mut run_font = None;
            for g in &line.glyphs {
                if let Some(prev) = run_font {
                    if prev != g.font_id {
                        if let Some((data, index)) = fonts.font_data(prev) {
                            run.set_font(FontResource::new(data, index));
                        }
                        list.push_glyph_run(run);
                        run = GlyphRun::new(g.font_size, color).with_style(style);
                    }
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
                if let Some((data, index)) = fonts.font_data(font_id) {
                    run.set_font(FontResource::new(data, index));
                }
                list.push_glyph_run(run);
            }
        }
        block_bottom
    }

    /// Union ink box of every glyph run [`push`](Self::push) would
    /// emit for `text` at `origin`, in device pixels — glyph x/width
    /// horizontally and `baseline − 0.8·size … baseline + 0.25·size`
    /// vertically, matching what the paint audit probes. `None` when
    /// nothing would be emitted (empty text, no glyphs).
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite::text_paint::TextPainter;
    /// use kurbo::Point;
    ///
    /// let mut p = TextPainter::new();
    /// assert!(p.ink_bounds(Point::ZERO, "", 14.0, None).is_none());
    /// ```
    pub fn ink_bounds(
        &mut self,
        origin: Point,
        text: &str,
        size: f32,
        max_width: Option<f32>,
    ) -> Option<kurbo::Rect> {
        self.ink_bounds_styled(origin, text, size, max_width, TextStyle::REGULAR)
    }

    /// [`ink_bounds`](Self::ink_bounds) with an explicit style axis —
    /// the ink box a styled [`push_styled`](Self::push_styled) emission
    /// would mark.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite::text_paint::TextPainter;
    /// use kurbo::Point;
    /// use martensite_core::paint::{FontWeight, TextStyle};
    ///
    /// let mut p = TextPainter::new();
    /// let b = p.ink_bounds_styled(
    ///     Point::ZERO, "", 14.0, None,
    ///     TextStyle::default().weight(FontWeight::BOLD),
    /// );
    /// assert!(b.is_none());
    /// ```
    pub fn ink_bounds_styled(
        &mut self,
        origin: Point,
        text: &str,
        size: f32,
        max_width: Option<f32>,
        style: TextStyle,
    ) -> Option<kurbo::Rect> {
        if text.is_empty() || size <= 0.0 {
            return None;
        }
        let mut box_: Option<kurbo::Rect> = None;
        for line in shape_text_with_attrs(
            self.fonts(),
            text,
            &attrs_for(style),
            size,
            size * 1.25,
            max_width,
        ) {
            let baseline_y = origin.y as f32 + line.line_y;
            for g in &line.glyphs {
                let glyph = kurbo::Rect::new(
                    f64::from(origin.x as f32 + g.x),
                    f64::from(baseline_y + g.y - g.font_size * 0.8),
                    f64::from(origin.x as f32 + g.x + g.w),
                    f64::from(baseline_y + g.y + g.font_size * 0.25),
                );
                box_ = Some(box_.map_or(glyph, |b: kurbo::Rect| b.union(glyph)));
            }
        }
        box_
    }
}

impl SharedTextPainter {
    /// Advance width of `text` at `size` device pixels — see
    /// [`TextPainter::measure`].
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite::text_paint::shared_painter;
    ///
    /// let p = shared_painter();
    /// assert!(p.measure("hi", 14.0) >= 0.0);
    /// ```
    pub fn measure(&self, text: &str, size: f32) -> f32 {
        self.0.lock().measure(text, size)
    }

    /// Device-pixel x of the insertion boundary before `byte_idx` —
    /// see [`TextPainter::caret_x`].
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite::text_paint::shared_painter;
    ///
    /// let p = shared_painter();
    /// assert_eq!(p.caret_x("", 14.0, 0), 0.0);
    /// ```
    pub fn caret_x(&self, text: &str, size: f32, byte_idx: usize) -> f32 {
        self.0.lock().caret_x(text, size, byte_idx)
    }

    /// Byte offset of the insertion boundary nearest device-pixel `x`
    /// — see [`TextPainter::byte_at`].
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite::text_paint::shared_painter;
    ///
    /// let p = shared_painter();
    /// assert_eq!(p.byte_at("hi", 14.0, 10_000.0), 2);
    /// ```
    pub fn byte_at(&self, text: &str, size: f32, x: f32) -> usize {
        self.0.lock().byte_at(text, size, x)
    }
}

/// `SharedTextPainter` satisfies the core
/// [`TextShaper`](martensite_core::paint::TextShaper) seam —
/// widgets use it as their explicit painter, and
/// [`martensite_core::WidgetArena::set_text_painter`] stores one as the
/// arena's ambient painter.
impl martensite_core::paint::TextShaper for SharedTextPainter {
    fn paint_shaped_text(
        &self,
        list: &mut PaintList,
        origin: Point,
        text: &str,
        size_px: f32,
        color: [u8; 4],
    ) {
        self.0.lock().push(list, origin, text, size_px, color, None);
    }

    fn paint_shaped_text_styled(
        &self,
        list: &mut PaintList,
        origin: Point,
        text: &str,
        size_px: f32,
        color: [u8; 4],
        style: TextStyle,
    ) {
        self.0
            .lock()
            .push_styled(list, origin, text, size_px, color, None, style);
    }

    fn measure_text(&self, text: &str, size_px: f32) -> Option<f32> {
        Some(self.0.lock().measure(text, size_px))
    }

    fn measure_text_styled(&self, text: &str, size_px: f32, style: TextStyle) -> Option<f32> {
        Some(self.0.lock().measure_styled(text, size_px, style))
    }

    fn ink_bounds(&self, origin: Point, text: &str, size_px: f32) -> Option<kurbo::Rect> {
        // `TextPainter::ink_bounds` is `None` when `push` would emit no
        // glyphs at all — report a degenerate box so callers cull the
        // emission rather than producing a dead command.
        Some(
            self.0
                .lock()
                .ink_bounds(origin, text, size_px, None)
                .unwrap_or(kurbo::Rect::ZERO),
        )
    }

    fn ink_bounds_styled(
        &self,
        origin: Point,
        text: &str,
        size_px: f32,
        style: TextStyle,
    ) -> Option<kurbo::Rect> {
        Some(
            self.0
                .lock()
                .ink_bounds_styled(origin, text, size_px, None, style)
                .unwrap_or(kurbo::Rect::ZERO),
        )
    }
}

/// Resolves the painter a widget should use for this paint pass: its
/// own explicitly-injected one first, then the ambient
/// [`PaintContext::text_painter`](martensite_core::PaintContext::text_painter).
pub(crate) fn resolve_painter<'a>(
    explicit: &'a Option<SharedTextPainter>,
    ambient: Option<&'a (dyn martensite_core::paint::TextShaper + Send + Sync)>,
) -> Option<&'a (dyn martensite_core::paint::TextShaper + Send + Sync)> {
    explicit
        .as_ref()
        .map(|p| p as &(dyn martensite_core::paint::TextShaper + Send + Sync))
        .or(ambient)
}

/// Advance width of `text` at `size_pt` logical pt, in device pixels,
/// measured through `explicit` (the widget's injected painter) or the
/// ambient measurer — `None` when neither can measure so callers keep
/// their estimate fallback. Layout-side sibling of
/// [`resolve_painter`]: measure must see the same shaping pipeline the
/// paint pass will use or sized-to-fit controls clip mid-glyph.
pub(crate) fn measure_label(
    explicit: &Option<SharedTextPainter>,
    scale: f32,
    text: &str,
    size_pt: f32,
) -> Option<f32> {
    let size_px = size_pt * scale;
    if let Some(p) = explicit {
        return Some(p.measure(text, size_px));
    }
    martensite_core::paint::ambient_measure_text(text, size_px)
}

/// Case-aware label width estimate (logical pt at the 14 pt UI font):
/// uppercase letters and digits run ~9.6 pt, other chars ~7.6 pt.
/// Layout-side sibling of [`paint_label_clipped`] — widgets whose
/// measure is an estimate (tabs, buttons) share it so the estimate
/// stays consistent everywhere. Callers add their own padding.
pub(crate) fn estimate_label_width(label: &str) -> f32 {
    label
        .chars()
        .map(|c| {
            if c.is_ascii_uppercase() || c.is_ascii_digit() {
                9.6
            } else {
                7.6
            }
        })
        .sum()
}

/// Emits `text` through `painter` when present, else falls back to
/// [`PaintList::push_text`]'s placeholder boxes. `pub(crate)` — the
/// facade widgets share this so the opt-in is one line in each `paint`.
///
/// Runs whose ink cannot intersect the list's active clip are not
/// emitted — a fully clipped run is dead paint work, the invisible
/// output the paint audit flags.
pub(crate) fn paint_label(
    painter: Option<&(dyn martensite_core::paint::TextShaper + Send + Sync)>,
    list: &mut PaintList,
    origin: Point,
    text: &str,
    size_px: f32,
    color: [u8; 4],
) {
    paint_label_styled(
        painter,
        list,
        origin,
        text,
        size_px,
        color,
        TextStyle::REGULAR,
    );
}

/// [`paint_label`] with an explicit style axis — semibold titles, true
/// italics, tracked caps. Painters that don't override the styled
/// channel degrade to regular weight, same as before.
pub(crate) fn paint_label_styled(
    painter: Option<&(dyn martensite_core::paint::TextShaper + Send + Sync)>,
    list: &mut PaintList,
    origin: Point,
    text: &str,
    size_px: f32,
    color: [u8; 4],
    style: TextStyle,
) {
    if let Some(ink) = label_ink_bounds_styled(painter, origin, text, size_px, style) {
        if !visible_ink(list, ink) {
            return;
        }
    }
    match painter {
        Some(tp) => {
            tp.paint_shaped_text_styled(list, origin, text, size_px, color, style);
        }
        None => {
            list.push_text(origin, text.to_string(), size_px, color);
        }
    }
}

/// [`paint_label`] clipped to `clip` — for labels painted inside a
/// fixed-size container (toast cards, dialog cards, input faces, tab
/// slots), where an over-long string must not spill past the chrome.
///
/// Labels whose estimated ink box cannot intersect `clip` are not
/// emitted at all: a fully clipped run is dead paint work — the kind
/// of invisible output the paint audit flags — and the clip/pop pair
/// would only wrap nothing.
pub(crate) fn paint_label_clipped(
    painter: Option<&(dyn martensite_core::paint::TextShaper + Send + Sync)>,
    list: &mut PaintList,
    clip: kurbo::Rect,
    origin: Point,
    text: &str,
    size_px: f32,
    color: [u8; 4],
) {
    paint_label_clipped_styled(
        painter,
        list,
        clip,
        origin,
        text,
        size_px,
        color,
        TextStyle::REGULAR,
    );
}

/// [`paint_label_clipped`] with an explicit style axis — see
/// [`paint_label_styled`].
#[allow(clippy::too_many_arguments)]
pub(crate) fn paint_label_clipped_styled(
    painter: Option<&(dyn martensite_core::paint::TextShaper + Send + Sync)>,
    list: &mut PaintList,
    clip: kurbo::Rect,
    origin: Point,
    text: &str,
    size_px: f32,
    color: [u8; 4],
    style: TextStyle,
) {
    // The pushed clip stacks with whatever is already active, so the
    // effective clip is `clip ∩ active` — compute it explicitly so the
    // culling check matches what the backend and audit will see. Rows
    // scrolled outside a scrollport hit this: their local clip is fine
    // but the intersected clip is empty and the run is dead paint.
    let clip = list
        .active_clip()
        .map(|o| o.intersect(clip))
        .unwrap_or(clip);
    // A degenerate or inverted clip provably paints nothing — emitting
    // the clip+text anyway is dead work the paint audit flags as
    // clipped text. Narrow widgets (a face too small for its label)
    // hit this path legitimately.
    if clip.x1 <= clip.x0 || clip.y1 <= clip.y0 {
        return;
    }
    if let Some(ink) = label_ink_bounds_styled(painter, origin, text, size_px, style) {
        if ink.x1 <= clip.x0 || ink.x0 >= clip.x1 || ink.y1 <= clip.y0 || ink.y0 >= clip.y1 {
            return;
        }
    }
    list.push_clip(clip);
    paint_label_styled(painter, list, origin, text, size_px, color, style);
    list.pop_clip();
}

/// `true` when `ink` can intersect the list's active clip — widgets
/// that emit unclipped labels (node captions, annotations) use it to
/// cull runs the enclosing clips would discard anyway. `true` when no
/// clip is active, so callers outside clips are unaffected.
pub(crate) fn visible_ink(list: &mut PaintList, ink: kurbo::Rect) -> bool {
    match list.active_clip() {
        Some(c) => ink.x1 > c.x0 && ink.x0 < c.x1 && ink.y1 > c.y0 && ink.y0 < c.y1,
        None => true,
    }
}

/// `true` when `occluders` provably covers `rect` — used by layered
/// widgets (cover flow, week view) to skip labels that opaque sibling
/// fills painted *later* in the command stream will hide entirely.
/// Emitting such a label is dead work the paint audit reports as
/// invisible output.
///
/// Uses the audit's five-point probe (center + four interior quarter
/// points), point-wise against the occluder union — exact for the
/// single-occluder and same-axis slice cases that produce these
/// overlaps; conservative elsewhere (a missed union returns `false`,
/// i.e. "emit", which can only leave a stale report, never hide
/// visible text).
pub(crate) fn fully_occluded(rect: kurbo::Rect, occluders: &[kurbo::Rect]) -> bool {
    if rect.x1 <= rect.x0 || rect.y1 <= rect.y0 {
        return true;
    }
    if occluders.is_empty() {
        return false;
    }
    let (w, h) = (rect.width(), rect.height());
    let samples = [
        (rect.x0 + w * 0.5, rect.y0 + h * 0.5),
        (rect.x0 + w * 0.25, rect.y0 + h * 0.25),
        (rect.x1 - w * 0.25, rect.y0 + h * 0.25),
        (rect.x0 + w * 0.25, rect.y1 - h * 0.25),
        (rect.x1 - w * 0.25, rect.y1 - h * 0.25),
    ];
    samples.iter().all(|&(sx, sy)| {
        let p = kurbo::Point::new(sx, sy);
        occluders.iter().any(|o| o.contains(p))
    })
}

/// Estimated ink box of `text` at `origin` — the shaped painter's real
/// glyph extents when it can report them, else the `0.6·size` per-char
/// block the `DrawText` fallback emits. `None` means "cannot estimate"
/// (a painter that reports no bounds); callers must not cull on it.
pub(crate) fn label_ink_bounds(
    painter: Option<&(dyn martensite_core::paint::TextShaper + Send + Sync)>,
    origin: Point,
    text: &str,
    size_px: f32,
) -> Option<kurbo::Rect> {
    label_ink_bounds_styled(painter, origin, text, size_px, TextStyle::REGULAR)
}

/// [`label_ink_bounds`] with an explicit style axis.
pub(crate) fn label_ink_bounds_styled(
    painter: Option<&(dyn martensite_core::paint::TextShaper + Send + Sync)>,
    origin: Point,
    text: &str,
    size_px: f32,
    style: TextStyle,
) -> Option<kurbo::Rect> {
    if text.is_empty() || size_px <= 0.0 {
        return Some(kurbo::Rect::ZERO);
    }
    match painter {
        Some(tp) => tp.ink_bounds_styled(origin, text, size_px, style),
        None => {
            // Mirrors `PaintList::push_text`'s box model and the
            // paint audit's `probe_text`: `0.6·size` per char, one
            // `size` tall starting at the origin.
            let w =
                (f64::from(size_px) * 0.6 * text.chars().count() as f64).max(f64::from(size_px));
            Some(kurbo::Rect::new(
                origin.x,
                origin.y,
                origin.x + w,
                origin.y + f64::from(size_px),
            ))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn empty_text_emits_nothing() {
        let mut p = TextPainter::new();
        let mut list = PaintList::new();
        let bottom = p.push(&mut list, Point::new(0.0, 5.0), "", 14.0, [255; 4], None);
        assert_eq!(bottom, 5.0);
        assert!(list.commands.is_empty());
    }

    #[test]
    fn zero_size_emits_nothing() {
        let mut p = TextPainter::new();
        let mut list = PaintList::new();
        p.push(&mut list, Point::new(0.0, 0.0), "x", 0.0, [255; 4], None);
        assert!(list.commands.is_empty());
    }

    #[test]
    fn caret_x_matches_measure_at_end() {
        let mut p = TextPainter::new();
        let text = "mixed Width iiWWwww";
        let w = p.measure(text, 14.0);
        let end = p.caret_x(text, 14.0, text.len());
        // The caret past the last glyph sits at the run's advance —
        // the property the old 7px/char estimate violated on
        // mixed-width text.
        assert!((end - w).abs() < 0.5, "caret {end} vs measure {w}");
    }

    #[test]
    fn caret_x_is_monotonic() {
        let mut p = TextPainter::new();
        let text = "iiiiWWWW";
        let mut prev = -1.0f32;
        for b in 0..=text.len() {
            let x = p.caret_x(text, 14.0, b);
            assert!(x >= prev, "caret moved backwards at byte {b}");
            prev = x;
        }
    }

    #[test]
    fn paint_label_clipped_balances_clip() {
        let mut list = PaintList::new();
        paint_label_clipped(
            None,
            &mut list,
            kurbo::Rect::new(10.0, 2.0, 50.0, 20.0),
            Point::new(12.0, 4.0),
            "label",
            14.0,
            [255; 4],
        );
        // ClipRect → text → PopClip, in order.
        assert!(matches!(
            list.commands[0],
            martensite_core::paint::PaintCommand::ClipRect(r)
                if r == kurbo::Rect::new(10.0, 2.0, 50.0, 20.0)
        ));
        assert!(matches!(
            list.commands.last(),
            Some(martensite_core::paint::PaintCommand::PopClip)
        ));
    }

    #[test]
    fn byte_at_roundtrips_caret_x() {
        let mut p = TextPainter::new();
        // No common ligatures here — every byte boundary is a real
        // cluster boundary, so caret_x inverts exactly.
        let text = "aXgTm";
        for b in 0..=text.len() {
            let x = p.caret_x(text, 14.0, b);
            assert_eq!(p.byte_at(text, 14.0, x), b, "round-trip at {b}");
        }
        // Clamping outside the run.
        assert_eq!(p.byte_at(text, 14.0, -100.0), 0);
        assert_eq!(p.byte_at(text, 14.0, 100_000.0), text.len());
    }
}
