//! CPU software rasterization backend built on top of [`tiny_skia`].
//!
//! [`TinySkiaBackend`] renders a [`PaintList`] into an
//! in-memory RGBA pixel buffer using pure-CPU rasterization. The resulting
//! buffer can be handed to the `softbuffer` crate for presentation on a window surface,
//! or inspected directly by tests. Because no GPU is required, this backend is
//! the primary target for headless CI rendering tests.

use crate::paint::{FontResource, GlyphRun, GradientStops, PaintCommand, PaintList};
use crate::RenderBackend;
use kurbo::{BezPath, PathEl, Point, Rect};
use swash::scale::ScaleContext;
use swash::zeno::Verb;
use swash::FontRef as SwashFontRef;
use tiny_skia::{
    Color, FillRule, LinearGradient, Mask, Paint, PathBuilder as TsPathBuilder, Pixmap,
    RadialGradient, Rect as TsRect, SpreadMode, Stroke, Transform,
};

/// A CPU software rasterizer that renders a [`PaintList`] into an RGBA
/// [`Pixmap`].
///
/// The backend owns a fixed-size pixel buffer. Each call to
/// [`TinySkiaBackend::render`] clears the buffer to fully transparent,
/// resets the active clip stack, and replays the supplied [`PaintList`] in
/// order. The rendered pixels are available via [`TinySkiaBackend::pixels`].
pub struct TinySkiaBackend {
    width: u32,
    height: u32,
    pixmap: Pixmap,
    /// Stack of cumulative clip masks, one entry per nested
    /// [`PaintCommand::ClipRect`] / [`PaintCommand::ClipRoundedRect`]. Each
    /// entry is a full-pixmap [`Mask`] representing the intersection of all
    /// active clips at that nesting level. The top of the stack is the active
    /// clip applied to every subsequent draw.
    clip_stack: Vec<Mask>,
    /// Reusable buffer for building [`tiny_skia::GradientStop`] vectors without
    /// reallocating on every gradient command. Cleared and refilled per use.
    gradient_stops_buf: Vec<tiny_skia::GradientStop>,
    /// Reusable [`tiny_skia::PathBuilder`] for converting [`BezPath`]s and
    /// building rounded-rect clip paths. Cleared before each use.
    ///
    /// Note: [`TsPathBuilder::finish`] consumes the builder's internal buffers
    /// into the returned [`tiny_skia::Path`], so the builder is recreated
    /// (via [`core::mem::take`]) after each build. The field is retained so the
    /// build step itself can reuse a single owned builder across calls.
    path_builder: TsPathBuilder,
}

impl TinySkiaBackend {
    /// Creates a new backend with the given dimensions in device pixels.
    ///
    /// Returns `None` when the requested dimensions are zero or exceed the
    /// internal allocation limits of [`tiny_skia`].
    pub fn new(width: u32, height: u32) -> Option<Self> {
        let pixmap = Pixmap::new(width, height)?;
        Some(Self {
            width,
            height,
            pixmap,
            clip_stack: Vec::new(),
            gradient_stops_buf: Vec::new(),
            path_builder: TsPathBuilder::new(),
        })
    }

    /// Returns the width of the backing pixel buffer.
    pub fn width(&self) -> u32 {
        self.width
    }

    /// Returns the height of the backing pixel buffer.
    pub fn height(&self) -> u32 {
        self.height
    }

    /// Returns the rendered pixels as a premultiplied RGBA byte slice.
    pub fn pixels(&self) -> &[u8] {
        self.pixmap.data()
    }

    /// Returns a reference to the underlying [`Pixmap`].
    pub fn pixmap(&self) -> &Pixmap {
        &self.pixmap
    }

    /// Clears the backing buffer to fully transparent black.
    pub fn clear(&mut self) {
        self.pixmap.fill(Color::TRANSPARENT);
    }

    /// Converts a `[u8; 4]` RGBA color (non-premultiplied) into a tiny-skia
    /// [`Color`].
    fn to_color(rgba: [u8; 4]) -> Color {
        Color::from_rgba8(rgba[0], rgba[1], rgba[2], rgba[3])
    }

    /// Converts a [`kurbo::Rect`] into a [`tiny_skia::Rect`], returning
    /// `None` for degenerate (zero or negative area) rectangles.
    fn to_ts_rect(rect: Rect) -> Option<TsRect> {
        TsRect::from_xywh(
            rect.x0 as f32,
            rect.y0 as f32,
            rect.width() as f32,
            rect.height() as f32,
        )
    }

    /// Converts a [`kurbo::BezPath`] into a [`tiny_skia::Path`].
    ///
    /// Returns `None` when the path is empty or degenerate. The conversion
    /// reuses the backend's owned [`TsPathBuilder`] buffer, clearing it before
    /// building and transferring its contents into the returned path via
    /// [`TsPathBuilder::finish`].
    fn build_ts_path(&mut self, path: &BezPath) -> Option<tiny_skia::Path> {
        self.path_builder.clear();
        for el in path.elements() {
            match *el {
                PathEl::MoveTo(p) => self.path_builder.move_to(p.x as f32, p.y as f32),
                PathEl::LineTo(p) => self.path_builder.line_to(p.x as f32, p.y as f32),
                PathEl::QuadTo(p1, p2) => {
                    self.path_builder
                        .quad_to(p1.x as f32, p1.y as f32, p2.x as f32, p2.y as f32);
                }
                PathEl::CurveTo(p1, p2, p3) => {
                    self.path_builder.cubic_to(
                        p1.x as f32,
                        p1.y as f32,
                        p2.x as f32,
                        p2.y as f32,
                        p3.x as f32,
                        p3.y as f32,
                    );
                }
                PathEl::ClosePath => self.path_builder.close(),
            }
        }
        // `finish` consumes the builder's internal vectors into the returned
        // `Path`; `take` swaps in a fresh default builder so the field remains
        // valid for the next conversion.
        let builder = core::mem::take(&mut self.path_builder);
        builder.finish()
    }

    /// Builds a [`tiny_skia::Path`] approximating a rounded rectangle, reusing
    /// the backend's owned [`TsPathBuilder`]. The corner radius is clamped to
    /// half the shorter side so it never exceeds the rectangle. Returns `None`
    /// for degenerate rectangles.
    fn build_rounded_rect_path(&mut self, rect: Rect, radius: f32) -> Option<tiny_skia::Path> {
        let left = rect.x0 as f32;
        let right = rect.x1 as f32;
        let top = rect.y0 as f32;
        let bottom = rect.y1 as f32;
        let half_w = (right - left) / 2.0;
        let half_h = (bottom - top) / 2.0;
        if half_w <= 0.0 || half_h <= 0.0 {
            return None;
        }
        let r = radius.max(0.0).min(half_w).min(half_h);
        // Cubic Bézier approximation of a quarter circle (kappa factor).
        let k = r * 0.5523_f32;

        self.path_builder.clear();
        self.path_builder.move_to(left + r, top);
        self.path_builder.line_to(right - r, top);
        self.path_builder
            .cubic_to(right - r + k, top, right, top + r - k, right, top + r);
        self.path_builder.line_to(right, bottom - r);
        self.path_builder.cubic_to(
            right,
            bottom - r + k,
            right - r + k,
            bottom,
            right - r,
            bottom,
        );
        self.path_builder.line_to(left + r, bottom);
        self.path_builder
            .cubic_to(left + r - k, bottom, left, bottom - r + k, left, bottom - r);
        self.path_builder.line_to(left, top + r);
        self.path_builder
            .cubic_to(left, top + r - k, left + r - k, top, left + r, top);
        self.path_builder.close();

        let builder = core::mem::take(&mut self.path_builder);
        builder.finish()
    }

    /// Fills the reusable gradient-stops buffer from [`GradientStops`] without
    /// reallocating in the steady state. The buffer is cleared and refilled on
    /// every call; callers should clone it when an owned [`Vec`] is required by
    /// the tiny-skia shader constructors.
    fn fill_gradient_stops(&mut self, stops: &GradientStops) {
        self.gradient_stops_buf.clear();
        self.gradient_stops_buf.extend(
            stops
                .stops
                .iter()
                .map(|s| tiny_skia::GradientStop::new(s.position, Self::to_color(s.color))),
        );
    }

    /// Pushes a new clip onto the clip stack by intersecting the supplied path
    /// with the current cumulative clip. When the stack is empty the new mask
    /// is seeded directly from the path; otherwise the previous top mask is
    /// cloned and intersected with the path so nested clips compose correctly.
    ///
    /// Each entry allocates a full-pixmap [`Mask`] (O(width * height)); this is
    /// acceptable for v0.2.0 software rendering but will be revisited when a
    /// cheaper region-based clip is available.
    fn push_clip_path(&mut self, path: tiny_skia::Path) {
        let mask = match self.clip_stack.last().cloned() {
            Some(mut m) => {
                m.intersect_path(&path, FillRule::Winding, true, Transform::identity());
                m
            }
            None => {
                let Some(mut m) = Mask::new(self.width, self.height) else {
                    return;
                };
                m.fill_path(&path, FillRule::Winding, true, Transform::identity());
                m
            }
        };
        self.clip_stack.push(mask);
    }

    /// Renders a single [`PaintCommand`] into the backing pixmap.
    fn render_command(&mut self, command: &PaintCommand) {
        match command {
            PaintCommand::FillRect(rect, color) => {
                let mut paint = Paint::default();
                paint.set_color(Self::to_color(*color));
                paint.anti_alias = true;
                if let Some(ts_rect) = Self::to_ts_rect(*rect) {
                    self.pixmap.fill_rect(
                        ts_rect,
                        &paint,
                        Transform::identity(),
                        self.clip_stack.last(),
                    );
                }
            }
            PaintCommand::StrokeRect(rect, width, color) => {
                let mut paint = Paint::default();
                paint.set_color(Self::to_color(*color));
                paint.anti_alias = true;
                let stroke = Stroke {
                    width: *width,
                    line_cap: tiny_skia::LineCap::Butt,
                    line_join: tiny_skia::LineJoin::Miter,
                    miter_limit: 4.0,
                    dash: None,
                };
                if let Some(ts_rect) = Self::to_ts_rect(*rect) {
                    let path = TsPathBuilder::from_rect(ts_rect);
                    self.pixmap.stroke_path(
                        &path,
                        &paint,
                        &stroke,
                        Transform::identity(),
                        self.clip_stack.last(),
                    );
                }
            }
            PaintCommand::FillPath(path, color) => {
                let mut paint = Paint::default();
                paint.set_color(Self::to_color(*color));
                paint.anti_alias = true;
                if let Some(ts_path) = self.build_ts_path(path) {
                    self.pixmap.fill_path(
                        &ts_path,
                        &paint,
                        FillRule::Winding,
                        Transform::identity(),
                        self.clip_stack.last(),
                    );
                }
            }
            PaintCommand::StrokePath(path, width, color) => {
                let mut paint = Paint::default();
                paint.set_color(Self::to_color(*color));
                paint.anti_alias = true;
                let stroke = Stroke {
                    width: *width,
                    line_cap: tiny_skia::LineCap::Butt,
                    line_join: tiny_skia::LineJoin::Miter,
                    miter_limit: 4.0,
                    dash: None,
                };
                if let Some(ts_path) = self.build_ts_path(path) {
                    self.pixmap.stroke_path(
                        &ts_path,
                        &paint,
                        &stroke,
                        Transform::identity(),
                        self.clip_stack.last(),
                    );
                }
            }
            PaintCommand::FillLinearGradient(rect, stops, start, end) => {
                self.fill_gradient_stops(stops);
                let ts_stops = self.gradient_stops_buf.clone();
                if let Some(shader) = LinearGradient::new(
                    tiny_skia::Point {
                        x: start[0] as f32,
                        y: start[1] as f32,
                    },
                    tiny_skia::Point {
                        x: end[0] as f32,
                        y: end[1] as f32,
                    },
                    ts_stops,
                    SpreadMode::Pad,
                    Transform::identity(),
                ) {
                    let paint = Paint {
                        shader,
                        anti_alias: true,
                        ..Paint::default()
                    };
                    if let Some(ts_rect) = Self::to_ts_rect(*rect) {
                        self.pixmap.fill_rect(
                            ts_rect,
                            &paint,
                            Transform::identity(),
                            self.clip_stack.last(),
                        );
                    }
                }
            }
            PaintCommand::FillRadialGradient(rect, stops, center, radius) => {
                self.fill_gradient_stops(stops);
                let ts_stops = self.gradient_stops_buf.clone();
                let r = *radius as f32;
                if let Some(shader) = RadialGradient::new(
                    tiny_skia::Point {
                        x: center[0] as f32,
                        y: center[1] as f32,
                    },
                    r,
                    tiny_skia::Point {
                        x: center[0] as f32,
                        y: center[1] as f32,
                    },
                    r,
                    ts_stops,
                    SpreadMode::Pad,
                    Transform::identity(),
                ) {
                    let paint = Paint {
                        shader,
                        anti_alias: true,
                        ..Paint::default()
                    };
                    if let Some(ts_rect) = Self::to_ts_rect(*rect) {
                        self.pixmap.fill_rect(
                            ts_rect,
                            &paint,
                            Transform::identity(),
                            self.clip_stack.last(),
                        );
                    }
                }
            }
            PaintCommand::ClipRect(rect) => {
                if let Some(ts_rect) = Self::to_ts_rect(*rect) {
                    let path = TsPathBuilder::from_rect(ts_rect);
                    self.push_clip_path(path);
                }
            }
            PaintCommand::ClipRoundedRect(rect, radius) => {
                if let Some(path) = self.build_rounded_rect_path(*rect, *radius) {
                    self.push_clip_path(path);
                }
            }
            PaintCommand::DrawText(origin, text, size, color) => {
                self.render_text(*origin, text, *size, *color);
            }
            PaintCommand::DrawGlyphRun(run) => {
                self.render_glyph_run(run);
            }
        }
    }

    /// Renders a text string as a sequence of filled per-character rectangles
    /// using a monospace approximation of the glyph advance.
    ///
    /// This is a real, visible rasterization suitable for v0.2.0: each
    /// character produces an opaque rectangle whose width is ~60% of the font
    /// size and whose height is the full font size, laid out left-to-right
    /// from `origin` with the baseline at `origin.y`. Full text shaping,
    /// kerning, and font-atlas rasterization are deferred to the text pipeline
    /// (planned for v0.3.0); until then [`PaintCommand::DrawText`] is a
    /// high-level convenience that produces placeholder glyph boxes.
    fn render_text(&mut self, origin: Point, text: &str, size: f32, color: [u8; 4]) {
        if text.is_empty() || size <= 0.0 {
            return;
        }
        let mut paint = Paint::default();
        paint.set_color(Self::to_color(color));
        paint.anti_alias = false;
        // Monospace advance approximation: ~60% of the font size.
        let char_width = size * 0.6;
        let clip = self.clip_stack.last();
        for (i, _ch) in text.chars().enumerate() {
            let x = origin.x as f32 + i as f32 * char_width;
            // Place the glyph box above the baseline at `origin.y`.
            let y = origin.y as f32 - size;
            if let Some(rect) = TsRect::from_xywh(x, y, char_width, size) {
                self.pixmap
                    .fill_rect(rect, &paint, Transform::identity(), clip);
            }
        }
    }

    /// Renders a [`GlyphRun`].
    ///
    /// When the run carries a [`FontResource`] (see [`GlyphRun::with_font`]),
    /// each glyph is rasterized from its real Bézier outline using `swash`:
    /// the font bytes are parsed, the glyph is scaled to `run.font_size`
    /// pixels-per-em, and the resulting outline is converted to a
    /// [`tiny_skia::Path`] and filled with the run color. The outline's
    /// font-space y-up coordinate system is flipped to the backend's y-down
    /// device space and translated to the glyph's pre-resolved origin
    /// (`glyph.x`, `glyph.y`).
    ///
    /// When no font is attached, the run falls back to drawing each glyph's
    /// pre-measured bounding box as a filled rectangle — the v0.2.0 placeholder
    /// behavior — so existing callers without a font continue to render.
    fn render_glyph_run(&mut self, run: &GlyphRun) {
        if run.glyphs.is_empty() {
            return;
        }
        if let Some(font) = run.font.as_ref() {
            self.render_glyph_run_outlines(run, font);
            return;
        }
        let mut paint = Paint::default();
        paint.set_color(Self::to_color(run.color));
        paint.anti_alias = false;
        let clip = self.clip_stack.last();
        for glyph in &run.glyphs {
            let w = glyph.width.max(1.0);
            let h = glyph.height.max(1.0);
            // The glyph origin sits on the baseline; the bounding box extends
            // upward by `height` and rightward by `width`.
            if let Some(rect) = TsRect::from_xywh(glyph.x, glyph.y - h, w, h) {
                self.pixmap
                    .fill_rect(rect, &paint, Transform::identity(), clip);
            }
        }
    }

    /// Rasterizes a [`GlyphRun`] using real glyph outlines parsed from `font`
    /// via `swash`.
    ///
    /// A fresh [`ScaleContext`] is created per call (it is not `Send` and so
    /// cannot live in the backend struct, which must be `Send`). For each
    /// glyph, the scaled outline is converted to a [`tiny_skia::Path`] with the
    /// y-axis flipped from font-space (y-up) to device-space (y-down) and
    /// translated to the glyph origin. The path is then filled with the run
    /// color under the active clip.
    ///
    /// Malformed font data or missing glyphs are skipped silently so that a
    /// single bad glyph never aborts the whole run; this mirrors the
    /// `catch_unwind` robustness used by the text pipeline for font-data
    /// access.
    fn render_glyph_run_outlines(&mut self, run: &GlyphRun, font: &FontResource) {
        let Some(font_ref) = SwashFontRef::from_index(font.data(), usize::try_from(font.index()).unwrap_or(0)) else {
            // Not a valid font file / index: fall back to bounding boxes.
            self.render_glyph_run_bounding_boxes(run);
            return;
        };
        let mut ctx = ScaleContext::new();
        let mut scaler = ctx.builder(font_ref).size(run.font_size).build();
        let mut paint = Paint::default();
        paint.set_color(Self::to_color(run.color));
        paint.anti_alias = true;
        for glyph in &run.glyphs {
            let Some(outline) = scaler.scale_outline(u16::try_from(glyph.glyph_id).unwrap_or(0))
            else {
                continue;
            };
            if outline.is_empty() {
                continue;
            }
            // A non-color font has a single outline layer; color fonts (COLR)
            // are out of scope for v0.2.0 software rendering, so we rasterize
            // only the first layer with the run color.
            let Some(layer) = outline.get(0) else {
                continue;
            };
            if let Some(path) = self.build_outline_path(layer.points(), layer.verbs(), glyph.x, glyph.y) {
                // `self.clip_stack.last()` is evaluated as an argument to the
                // `fill_path` call on `self.pixmap`; this is a disjoint
                // two-field borrow and does not conflict with the mutable
                // borrow of `self.pixmap`.
                self.pixmap.fill_path(
                    &path,
                    &paint,
                    FillRule::Winding,
                    Transform::identity(),
                    self.clip_stack.last(),
                );
            }
        }
    }

    /// Fallback that draws each glyph's bounding box as a filled rectangle.
    /// Used when a [`FontResource`] is present but cannot be parsed.
    fn render_glyph_run_bounding_boxes(&mut self, run: &GlyphRun) {
        let mut paint = Paint::default();
        paint.set_color(Self::to_color(run.color));
        paint.anti_alias = false;
        let clip = self.clip_stack.last();
        for glyph in &run.glyphs {
            let w = glyph.width.max(1.0);
            let h = glyph.height.max(1.0);
            if let Some(rect) = TsRect::from_xywh(glyph.x, glyph.y - h, w, h) {
                self.pixmap
                    .fill_rect(rect, &paint, Transform::identity(), clip);
            }
        }
    }

    /// Converts a `swash` glyph outline (points + verbs) into a
    /// [`tiny_skia::Path`].
    ///
    /// The font-space y-up coordinate system is flipped to device-space y-down
    /// and the whole outline is translated to the glyph origin `(ox, oy)`,
    /// which is the baseline position in device pixels. The conversion reuses
    /// the backend's owned [`TsPathBuilder`].
    fn build_outline_path(
        &mut self,
        points: &[swash::zeno::Point],
        verbs: &[Verb],
        ox: f32,
        oy: f32,
    ) -> Option<tiny_skia::Path> {
        self.path_builder.clear();
        let mut idx = 0usize;
        let to_device = |p: swash::zeno::Point| -> (f32, f32) {
            // Flip y: device_y = baseline_y - font_y.
            (ox + p.x, oy - p.y)
        };
        for verb in verbs {
            match verb {
                Verb::MoveTo => {
                    let p = points.get(idx)?;
                    idx += 1;
                    let (x, y) = to_device(*p);
                    self.path_builder.move_to(x, y);
                }
                Verb::LineTo => {
                    let p = points.get(idx)?;
                    idx += 1;
                    let (x, y) = to_device(*p);
                    self.path_builder.line_to(x, y);
                }
                Verb::QuadTo => {
                    let c = points.get(idx)?;
                    let p = points.get(idx + 1)?;
                    idx += 2;
                    let (cx, cy) = to_device(*c);
                    let (x, y) = to_device(*p);
                    self.path_builder.quad_to(cx, cy, x, y);
                }
                Verb::CurveTo => {
                    let c1 = points.get(idx)?;
                    let c2 = points.get(idx + 1)?;
                    let p = points.get(idx + 2)?;
                    idx += 3;
                    let (c1x, c1y) = to_device(*c1);
                    let (c2x, c2y) = to_device(*c2);
                    let (x, y) = to_device(*p);
                    self.path_builder.cubic_to(c1x, c1y, c2x, c2y, x, y);
                }
                Verb::Close => {
                    self.path_builder.close();
                }
            }
        }
        let builder = core::mem::take(&mut self.path_builder);
        builder.finish()
    }
}

impl RenderBackend for TinySkiaBackend {
    fn render(&mut self, paint_list: &PaintList) {
        self.clear();
        // Reset the clip stack at the start of each frame. The current
        // `PaintCommand` set has no explicit "pop clip" variant, so clips do
        // not persist across frames and nesting resets per render pass.
        self.clip_stack.clear();
        for command in &paint_list.commands {
            self.render_command(command);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::paint::{GlyphInstance, GlyphRun, GradientStop, PaintList};
    use kurbo::Point;

    fn backend() -> TinySkiaBackend {
        TinySkiaBackend::new(64, 64).expect("64x64 pixmap should allocate")
    }

    fn non_zero_pixels(backend: &TinySkiaBackend) -> usize {
        backend
            .pixels()
            .as_chunks::<4>()
            .0
            .iter()
            .filter(|px| px[3] != 0)
            .count()
    }

    #[test]
    fn new_allocates_pixmap() {
        let b = backend();
        assert_eq!(b.width(), 64);
        assert_eq!(b.height(), 64);
        assert_eq!(b.pixels().len(), 64 * 64 * 4);
    }

    #[test]
    fn new_returns_none_for_zero_size() {
        assert!(TinySkiaBackend::new(0, 0).is_none());
    }

    #[test]
    fn clear_produces_transparent_buffer() {
        let mut b = backend();
        b.clear();
        assert_eq!(non_zero_pixels(&b), 0);
    }

    #[test]
    fn fill_rect_produces_non_zero_pixels() {
        let mut b = backend();
        let mut list = PaintList::new();
        list.push_fill_rect(Rect::new(10.0, 10.0, 40.0, 40.0), [255, 0, 0, 255]);
        b.render(&list);
        assert!(non_zero_pixels(&b) > 0, "filled rect should produce pixels");
    }

    #[test]
    fn fill_rect_covers_expected_region() {
        let mut b = backend();
        let mut list = PaintList::new();
        list.push_fill_rect(Rect::new(10.0, 10.0, 20.0, 20.0), [255, 0, 0, 255]);
        b.render(&list);
        // Interior pixel should be opaque red.
        let px = b.pixmap().pixel(15, 15).expect("pixel in range");
        assert_eq!(px.red(), 255);
        assert_eq!(px.alpha(), 255);
        // Pixel outside the rect should be transparent.
        let outside = b.pixmap().pixel(0, 0).expect("pixel in range");
        assert_eq!(outside.alpha(), 0);
    }

    #[test]
    fn stroke_rect_produces_non_zero_pixels() {
        let mut b = backend();
        let mut list = PaintList::new();
        list.push_stroke_rect(Rect::new(10.0, 10.0, 40.0, 40.0), 2.0, [0, 255, 0, 255]);
        b.render(&list);
        assert!(
            non_zero_pixels(&b) > 0,
            "stroked rect should produce pixels"
        );
    }

    #[test]
    fn fill_path_produces_non_zero_pixels() {
        let mut b = backend();
        let mut list = PaintList::new();
        let mut builder = crate::paint::PathBuilder::new();
        builder.move_to(Point::new(10.0, 10.0));
        builder.line_to(Point::new(50.0, 10.0));
        builder.line_to(Point::new(30.0, 50.0));
        builder.close_path();
        list.push_path(builder.build(), [0, 0, 255, 255]);
        b.render(&list);
        assert!(non_zero_pixels(&b) > 0, "filled path should produce pixels");
    }

    #[test]
    fn stroke_path_produces_non_zero_pixels() {
        let mut b = backend();
        let mut list = PaintList::new();
        let mut builder = crate::paint::PathBuilder::new();
        builder.move_to(Point::new(5.0, 5.0));
        builder.line_to(Point::new(59.0, 59.0));
        list.push_stroke_path(builder.build(), 3.0, [255, 255, 0, 255]);
        b.render(&list);
        assert!(
            non_zero_pixels(&b) > 0,
            "stroked path should produce pixels"
        );
    }

    #[test]
    fn linear_gradient_produces_non_zero_pixels() {
        let mut b = backend();
        let mut list = PaintList::new();
        let stops = GradientStops::from_slice(&[
            GradientStop::new(0.0, [0, 0, 0, 255]),
            GradientStop::new(1.0, [255, 255, 255, 255]),
        ]);
        list.push_linear_gradient(
            Rect::new(0.0, 0.0, 64.0, 64.0),
            stops,
            [0.0, 0.0],
            [64.0, 0.0],
        );
        b.render(&list);
        assert!(
            non_zero_pixels(&b) > 0,
            "linear gradient should produce pixels"
        );
    }

    #[test]
    fn radial_gradient_produces_non_zero_pixels() {
        let mut b = backend();
        let mut list = PaintList::new();
        let stops = GradientStops::from_slice(&[
            GradientStop::new(0.0, [255, 255, 255, 255]),
            GradientStop::new(1.0, [0, 0, 0, 255]),
        ]);
        list.push_radial_gradient(Rect::new(0.0, 0.0, 64.0, 64.0), stops, [32.0, 32.0], 32.0);
        b.render(&list);
        assert!(
            non_zero_pixels(&b) > 0,
            "radial gradient should produce pixels"
        );
    }

    #[test]
    fn glyph_run_produces_non_zero_pixels() {
        let mut b = backend();
        let mut list = PaintList::new();
        let mut run = GlyphRun::new(16.0, [255, 0, 0, 255]);
        run.push(GlyphInstance::new(32.0, 32.0, 0, 10.0, 16.0));
        list.push_glyph_run(run);
        b.render(&list);
        assert!(non_zero_pixels(&b) > 0, "glyph run should produce pixels");
    }

    #[test]
    fn empty_paint_list_produces_transparent_buffer() {
        let mut b = backend();
        let list = PaintList::new();
        b.render(&list);
        assert_eq!(non_zero_pixels(&b), 0);
    }

    #[test]
    fn render_replays_multiple_commands() {
        let mut b = backend();
        let mut list = PaintList::new();
        list.push_fill_rect(Rect::new(0.0, 0.0, 10.0, 10.0), [255, 0, 0, 255]);
        list.push_fill_rect(Rect::new(54.0, 54.0, 64.0, 64.0), [0, 255, 0, 255]);
        b.render(&list);
        let top_left = b.pixmap().pixel(5, 5).expect("pixel in range");
        assert_eq!(top_left.red(), 255);
        let bottom_right = b.pixmap().pixel(59, 59).expect("pixel in range");
        assert_eq!(bottom_right.green(), 255);
    }

    #[test]
    fn clip_rect_restricts_subsequent_draws() {
        let mut b = backend();
        let mut list = PaintList::new();
        // Clip to the top-left quadrant, then fill the whole pixmap.
        list.push_clip(Rect::new(0.0, 0.0, 32.0, 32.0));
        list.push_fill_rect(Rect::new(0.0, 0.0, 64.0, 64.0), [255, 0, 0, 255]);
        b.render(&list);

        // Inside the clip: opaque red.
        let inside = b.pixmap().pixel(10, 10).expect("pixel in range");
        assert_eq!(inside.alpha(), 255, "pixel inside clip should be drawn");
        assert_eq!(inside.red(), 255);

        // Outside the clip: unchanged (transparent).
        let outside = b.pixmap().pixel(40, 40).expect("pixel in range");
        assert_eq!(
            outside.alpha(),
            0,
            "pixel outside clip should remain transparent"
        );
        // Edge just outside the clip boundary on the other axis.
        let outside_y = b.pixmap().pixel(10, 40).expect("pixel in range");
        assert_eq!(outside_y.alpha(), 0, "pixel below clip should be clipped");
    }

    #[test]
    fn clip_rect_leaves_pixels_outside_clip_unchanged() {
        let mut b = backend();
        // Within a single frame: fill the whole pixmap green first (no clip
        // active), then push a clip in the top-left and fill red everywhere.
        // Pixels outside the clip must retain the green drawn earlier in the
        // same pass; only pixels inside the clip are overwritten with red.
        let mut list = PaintList::new();
        list.push_fill_rect(Rect::new(0.0, 0.0, 64.0, 64.0), [0, 255, 0, 255]);
        list.push_clip(Rect::new(0.0, 0.0, 16.0, 16.0));
        list.push_fill_rect(Rect::new(0.0, 0.0, 64.0, 64.0), [255, 0, 0, 255]);
        b.render(&list);

        let inside = b.pixmap().pixel(5, 5).expect("pixel in range");
        assert_eq!(inside.red(), 255);
        assert_eq!(inside.green(), 0, "pixel inside clip should be overwritten");
        let outside = b.pixmap().pixel(50, 50).expect("pixel in range");
        assert_eq!(
            outside.green(),
            255,
            "pixel outside clip should be unchanged"
        );
        assert_eq!(outside.red(), 0);
    }

    #[test]
    fn clip_rounded_rect_restricts_subsequent_draws() {
        let mut b = backend();
        let mut list = PaintList::new();
        // A rounded clip covering the center; the corner-center area is inside
        // while the pixmap corners are outside the rounded shape.
        list.push_clip_rounded(Rect::new(8.0, 8.0, 56.0, 56.0), 8.0);
        list.push_fill_rect(Rect::new(0.0, 0.0, 64.0, 64.0), [0, 0, 255, 255]);
        b.render(&list);

        // Center of the rounded rect is inside the clip.
        let center = b.pixmap().pixel(32, 32).expect("pixel in range");
        assert_eq!(center.alpha(), 255, "center should be inside rounded clip");
        assert_eq!(center.blue(), 255);

        // The very corner of the pixmap is outside the rounded clip.
        let corner = b.pixmap().pixel(0, 0).expect("pixel in range");
        assert_eq!(
            corner.alpha(),
            0,
            "pixmap corner should be outside rounded clip"
        );
    }

    #[test]
    fn nested_clip_rects_intersect() {
        let mut b = backend();
        let mut list = PaintList::new();
        // Outer clip: left half. Inner clip: top half. Intersection: top-left
        // quadrant only.
        list.push_clip(Rect::new(0.0, 0.0, 32.0, 64.0));
        list.push_clip(Rect::new(0.0, 0.0, 64.0, 32.0));
        list.push_fill_rect(Rect::new(0.0, 0.0, 64.0, 64.0), [255, 0, 0, 255]);
        b.render(&list);

        let inside = b.pixmap().pixel(10, 10).expect("pixel in range");
        assert_eq!(inside.alpha(), 255, "top-left quadrant should be drawn");
        let right = b.pixmap().pixel(40, 10).expect("pixel in range");
        assert_eq!(
            right.alpha(),
            0,
            "right half should be clipped by outer clip"
        );
        let bottom = b.pixmap().pixel(10, 40).expect("pixel in range");
        assert_eq!(
            bottom.alpha(),
            0,
            "bottom half should be clipped by inner clip"
        );
    }

    #[test]
    fn draw_text_produces_visible_per_character_rectangles() {
        let mut b = backend();
        let mut list = PaintList::new();
        // Three characters at font size 16 -> three rectangles.
        list.push_text(
            Point::new(8.0, 32.0),
            "abc".to_string(),
            16.0,
            [255, 0, 0, 255],
        );
        b.render(&list);
        assert!(
            non_zero_pixels(&b) > 0,
            "DrawText should produce visible output"
        );
        // The first character's rectangle starts at x=8 and spans ~9.6px wide;
        // a pixel a few px in should be opaque red.
        let px = b.pixmap().pixel(12, 24).expect("pixel in range");
        assert_eq!(px.alpha(), 255);
        assert_eq!(px.red(), 255);
    }

    #[test]
    fn draw_text_empty_string_is_a_noop() {
        let mut b = backend();
        let mut list = PaintList::new();
        list.push_text(Point::new(8.0, 32.0), String::new(), 16.0, [255, 0, 0, 255]);
        b.render(&list);
        assert_eq!(non_zero_pixels(&b), 0, "empty text should draw nothing");
    }

    #[test]
    fn glyph_run_uses_glyph_dimensions() {
        let mut b = backend();
        let mut list = PaintList::new();
        // A single wide glyph (width 20, height 8) at baseline (32, 32).
        let mut run = GlyphRun::new(16.0, [0, 255, 0, 255]);
        run.push(GlyphInstance::new(10.0, 32.0, 0, 20.0, 8.0));
        list.push_glyph_run(run);
        b.render(&list);

        // The glyph box spans x in [10, 30], y in [24, 32] (baseline minus
        // height). A pixel near the center should be opaque green.
        let inside = b.pixmap().pixel(20, 28).expect("pixel in range");
        assert_eq!(inside.alpha(), 255);
        assert_eq!(inside.green(), 255);
        // A pixel well outside the box should be untouched.
        let outside = b.pixmap().pixel(50, 50).expect("pixel in range");
        assert_eq!(outside.alpha(), 0);
    }
}
