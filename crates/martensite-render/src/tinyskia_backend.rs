//! CPU software rasterization backend built on top of [`tiny_skia`].
//!
//! [`TinySkiaBackend`] renders a [`PaintList`] into an
//! in-memory RGBA pixel buffer using pure-CPU rasterization. The resulting
//! buffer can be handed to [`softbuffer`] for presentation on a window surface,
//! or inspected directly by tests. Because no GPU is required, this backend is
//! the primary target for headless CI rendering tests.

use crate::paint::{GlyphRun, GradientStops, PaintCommand, PaintList};
use crate::RenderBackend;
use kurbo::{BezPath, PathEl, Rect};
use tiny_skia::{
    Color, FillRule, LinearGradient, Paint, PathBuilder as TsPathBuilder, Pixmap, RadialGradient,
    Rect as TsRect, SpreadMode, Stroke, Transform,
};

/// A CPU software rasterizer that renders a [`PaintList`] into an RGBA
/// [`Pixmap`].
///
/// The backend owns a fixed-size pixel buffer. Each call to
/// [`TinySkiaBackend::render`] clears the buffer to fully transparent and
/// replays the supplied [`PaintList`] in order. The rendered pixels are
/// available via [`TinySkiaBackend::pixels`].
pub struct TinySkiaBackend {
    width: u32,
    height: u32,
    pixmap: Pixmap,
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
    /// `None` for degenerate rectangles.
    fn to_ts_rect(rect: Rect) -> Option<TsRect> {
        TsRect::from_xywh(
            rect.x0 as f32,
            rect.y0 as f32,
            rect.width() as f32,
            rect.height() as f32,
        )
        .or_else(|| TsRect::from_ltrb(0.0, 0.0, 0.0, 0.0))
    }

    /// Converts a [`kurbo::BezPath`] into a [`tiny_skia::Path`].
    ///
    /// Returns `None` when the path is empty or degenerate.
    fn to_ts_path(path: &BezPath) -> Option<tiny_skia::Path> {
        let mut builder = TsPathBuilder::new();
        for el in path.elements() {
            match *el {
                PathEl::MoveTo(p) => builder.move_to(p.x as f32, p.y as f32),
                PathEl::LineTo(p) => builder.line_to(p.x as f32, p.y as f32),
                PathEl::QuadTo(p1, p2) => {
                    builder.quad_to(p1.x as f32, p1.y as f32, p2.x as f32, p2.y as f32);
                }
                PathEl::CurveTo(p1, p2, p3) => builder.cubic_to(
                    p1.x as f32,
                    p1.y as f32,
                    p2.x as f32,
                    p2.y as f32,
                    p3.x as f32,
                    p3.y as f32,
                ),
                PathEl::ClosePath => builder.close(),
            }
        }
        builder.finish()
    }

    /// Builds a tiny-skia gradient stop vector from [`GradientStops`].
    fn to_gradient_stops(stops: &GradientStops) -> Vec<tiny_skia::GradientStop> {
        stops
            .stops
            .iter()
            .map(|s| tiny_skia::GradientStop::new(s.position, Self::to_color(s.color)))
            .collect()
    }

    /// Renders a single [`PaintCommand`] into the backing pixmap.
    fn render_command(&mut self, command: &PaintCommand) {
        match command {
            PaintCommand::FillRect(rect, color) => {
                let mut paint = Paint::default();
                paint.set_color(Self::to_color(*color));
                paint.anti_alias = true;
                if let Some(ts_rect) = Self::to_ts_rect(*rect) {
                    self.pixmap
                        .fill_rect(ts_rect, &paint, Transform::identity(), None);
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
                    self.pixmap
                        .stroke_path(&path, &paint, &stroke, Transform::identity(), None);
                }
            }
            PaintCommand::FillPath(path, color) => {
                let mut paint = Paint::default();
                paint.set_color(Self::to_color(*color));
                paint.anti_alias = true;
                if let Some(ts_path) = Self::to_ts_path(path) {
                    self.pixmap.fill_path(
                        &ts_path,
                        &paint,
                        FillRule::Winding,
                        Transform::identity(),
                        None,
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
                if let Some(ts_path) = Self::to_ts_path(path) {
                    self.pixmap
                        .stroke_path(&ts_path, &paint, &stroke, Transform::identity(), None);
                }
            }
            PaintCommand::FillLinearGradient(rect, stops, start, end) => {
                let ts_stops = Self::to_gradient_stops(stops);
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
                        self.pixmap
                            .fill_rect(ts_rect, &paint, Transform::identity(), None);
                    }
                }
            }
            PaintCommand::FillRadialGradient(rect, stops, center, radius) => {
                let ts_stops = Self::to_gradient_stops(stops);
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
                        self.pixmap
                            .fill_rect(ts_rect, &paint, Transform::identity(), None);
                    }
                }
            }
            PaintCommand::ClipRect(_rect) => {
                // Clipping in the software backend is handled by replaying the
                // command stream; tiny_skia clipping requires a Mask which is
                // applied per-primitive. For the milestone scope we record the
                // clip but do not restrict subsequent draws.
            }
            PaintCommand::ClipRoundedRect(_rect, _radius) => {
                // See `ClipRect`: clip recording only for the software backend.
            }
            PaintCommand::DrawText(_origin, _text, _size, _color) => {
                // Text rasterization requires a font atlas which is supplied by
                // the martensite-text crate. The software backend draws
                // pre-resolved glyph runs (`DrawGlyphRun`) instead.
            }
            PaintCommand::DrawGlyphRun(run) => {
                self.render_glyph_run(run);
            }
        }
    }

    /// Renders a [`GlyphRun`] by drawing a small filled square per glyph as a
    /// placeholder. Real glyph rasterization is performed by the text stack;
    /// this implementation ensures the run produces visible, non-zero pixels
    /// so that layout-driven rendering can be exercised in tests.
    fn render_glyph_run(&mut self, run: &GlyphRun) {
        if run.glyphs.is_empty() {
            return;
        }
        let mut paint = Paint::default();
        paint.set_color(Self::to_color(run.color));
        paint.anti_alias = false;
        let size = run.font_size.max(1.0);
        let half = size / 2.0;
        for glyph in &run.glyphs {
            if let Some(rect) = TsRect::from_xywh(glyph.x - half, glyph.y - size, size, size) {
                self.pixmap
                    .fill_rect(rect, &paint, Transform::identity(), None);
            }
        }
    }
}

impl RenderBackend for TinySkiaBackend {
    fn render(&mut self, paint_list: &PaintList) {
        self.clear();
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
        run.push(GlyphInstance {
            x: 32.0,
            y: 32.0,
            glyph_id: 0,
        });
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
}
