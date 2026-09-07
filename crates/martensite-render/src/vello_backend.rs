//! Vello GPU rendering backend.
//!
//! [`VelloRenderer`] translates a [`PaintList`] into a Vello
//! [`Scene`] by mapping each [`PaintCommand`] to the corresponding
//! Vello scene draw call. The actual GPU compute dispatch is performed
//! by the `martensite-wgpu` orchestrator, which owns the Vello render
//! pipeline and target texture. This module is responsible only for
//! scene composition.
//!
//! The Vello integration is gated behind the `vello` feature flag; without
//! it the struct is still defined and implements [`RenderBackend`]
//! but performs no GPU work, allowing downstream crates to compile against
//! the type unconditionally.

#[cfg(feature = "vello")]
use crate::paint::{GlyphRun, PaintCommand};
use crate::paint::PaintList;
use crate::RenderBackend;

#[cfg(feature = "vello")]
use {
    kurbo::{Affine, Point as KurboPoint, Rect as KurboRect, RoundedRect, Stroke as KurboStroke},
    peniko::{
        color::{AlphaColor, DynamicColor, Srgb},
        BlendMode, Color, ColorStop, ColorStops, Compose, Fill, Gradient, Mix,
    },
    vello::Scene,
};

/// Converts a `[u8; 4]` RGBA color tuple into a peniko `Color` (sRGB AlphaColor).
#[cfg(feature = "vello")]
fn rgba_to_color(rgba: [u8; 4]) -> Color {
    AlphaColor::<Srgb>::from_rgba8(rgba[0], rgba[1], rgba[2], rgba[3])
}

/// Converts a peniko `Color` (AlphaColor<Srgb>) into a `DynamicColor`
/// suitable for `ColorStop`.
#[cfg(feature = "vello")]
fn to_dynamic(color: Color) -> DynamicColor {
    DynamicColor::from_alpha_color(color)
}

/// Converts [`crate::paint::GradientStops`] into peniko `ColorStops`.
#[cfg(feature = "vello")]
fn gradient_stops_to_peniko(stops: &crate::paint::GradientStops) -> ColorStops {
    let mut peniko_stops = ColorStops::new();
    for stop in &stops.stops {
        peniko_stops.push(ColorStop {
            offset: stop.position,
            color: to_dynamic(rgba_to_color(stop.color)),
        });
    }
    peniko_stops
}

/// A GPU renderer that converts a [`PaintList`] into a Vello scene.
///
/// When the `vello` feature is enabled, [`VelloRenderer::render`] builds a
/// [`Scene`] from the supplied commands by translating each [`PaintCommand`]
/// into the corresponding Vello draw call (fill, stroke, gradient, clip
/// layer, or glyph run). The scene is then available via [`VelloRenderer::scene`]
/// for the `martensite-wgpu` orchestrator to dispatch to the GPU compute
/// pipeline.
///
/// When the feature is disabled the renderer is a no-op stub that simply
/// records the number of commands it received, which is useful for
/// type-level compatibility in crates that depend on `martensite-render`
/// without GPU support.
pub struct VelloRenderer {
    #[cfg(feature = "vello")]
    scene: Scene,
    /// The number of clip layers pushed during the current frame.
    /// These are all popped at the end of `render()` to keep the scene
    /// balanced, matching the TinySkia clip_stack design where clips
    /// accumulate for the remainder of the frame and reset each frame.
    #[cfg(feature = "vello")]
    clip_depth: u32,
    last_command_count: usize,
}

impl VelloRenderer {
    /// Creates a new renderer with an empty scene.
    #[must_use]
    pub fn new() -> Self {
        Self {
            #[cfg(feature = "vello")]
            scene: Scene::new(),
            #[cfg(feature = "vello")]
            clip_depth: 0,
            last_command_count: 0,
        }
    }

    /// Returns the number of commands processed by the most recent
    /// [`RenderBackend::render`] call.
    #[must_use]
    pub fn last_command_count(&self) -> usize {
        self.last_command_count
    }

    /// Returns a reference to the most recently built Vello scene.
    ///
    /// Only available when the `vello` feature is enabled.
    #[cfg(feature = "vello")]
    #[must_use]
    pub fn scene(&self) -> &Scene {
        &self.scene
    }

    /// Returns a mutable reference to the most recently built Vello scene.
    ///
    /// Only available when the `vello` feature is enabled. This allows the
    /// `martensite-wgpu` orchestrator to take ownership of the scene for
    /// GPU dispatch.
    #[cfg(feature = "vello")]
    #[must_use]
    pub fn scene_mut(&mut self) -> &mut Scene {
        &mut self.scene
    }

    /// Resets the internal scene, discarding any previously recorded commands.
    #[cfg(feature = "vello")]
    pub fn reset(&mut self) {
        self.scene.reset();
        self.clip_depth = 0;
        self.last_command_count = 0;
    }

    /// Translates a single paint command into Vello scene draw calls.
    ///
    /// This implements the full `PaintCommand` → `Scene` mapping:
    /// - `FillRect` / `FillPath` → `Scene::fill` with a solid color brush
    /// - `StrokeRect` / `StrokePath` → `Scene::stroke` with a solid color brush
    /// - `FillLinearGradient` → `Scene::fill` with a linear `Gradient` brush
    /// - `FillRadialGradient` → `Scene::fill` with a radial `Gradient` brush
    /// - `ClipRect` / `ClipRoundedRect` → `Scene::push_layer` with a clip shape
    /// - `DrawText` / `DrawGlyphRun` → approximated as filled rectangles
    ///   (full glyph rasterization requires font atlas integration from v0.3.0)
    #[cfg(feature = "vello")]
    fn render_command(&mut self, command: &PaintCommand) {
        match command {
            PaintCommand::FillRect(rect, color) => {
                if rect.width() > 0.0 && rect.height() > 0.0 {
                    self.scene.fill(
                        Fill::EvenOdd,
                        Affine::IDENTITY,
                        rgba_to_color(*color),
                        None,
                        rect,
                    );
                }
            }
            PaintCommand::StrokeRect(rect, width, color) => {
                if rect.width() > 0.0 && rect.height() > 0.0 {
                    let stroke = KurboStroke::new(f64::from(*width));
                    self.scene
                        .stroke(&stroke, Affine::IDENTITY, rgba_to_color(*color), None, rect);
                }
            }
            PaintCommand::FillPath(path, color) => {
                self.scene.fill(
                    Fill::EvenOdd,
                    Affine::IDENTITY,
                    rgba_to_color(*color),
                    None,
                    path,
                );
            }
            PaintCommand::StrokePath(path, width, color) => {
                let stroke = KurboStroke::new(f64::from(*width));
                self.scene
                    .stroke(&stroke, Affine::IDENTITY, rgba_to_color(*color), None, path);
            }
            PaintCommand::FillLinearGradient(rect, stops, start, end) => {
                if rect.width() <= 0.0 || rect.height() <= 0.0 {
                    return;
                }
                let gradient = Gradient::new_linear(
                    KurboPoint::new(start[0], start[1]),
                    KurboPoint::new(end[0], end[1]),
                )
                .with_stops(gradient_stops_to_peniko(stops));
                self.scene
                    .fill(Fill::EvenOdd, Affine::IDENTITY, &gradient, None, rect);
            }
            PaintCommand::FillRadialGradient(rect, stops, center, radius) => {
                if rect.width() <= 0.0 || rect.height() <= 0.0 || *radius <= 0.0 {
                    return;
                }
                let gradient =
                    Gradient::new_radial(KurboPoint::new(center[0], center[1]), *radius as f32)
                        .with_stops(gradient_stops_to_peniko(stops));
                self.scene
                    .fill(Fill::EvenOdd, Affine::IDENTITY, &gradient, None, rect);
            }
            PaintCommand::ClipRect(rect) => {
                if rect.width() > 0.0 && rect.height() > 0.0 {
                    self.scene.push_layer(
                        Fill::EvenOdd,
                        BlendMode::new(Mix::Normal, Compose::SrcOver),
                        1.0,
                        Affine::IDENTITY,
                        rect,
                    );
                    self.clip_depth += 1;
                }
            }
            PaintCommand::ClipRoundedRect(rect, radius) => {
                let rr = RoundedRect::from_rect(*rect, f64::from(*radius));
                if rr.rect().width() > 0.0 && rr.rect().height() > 0.0 {
                    self.scene.push_layer(
                        Fill::EvenOdd,
                        BlendMode::new(Mix::Normal, Compose::SrcOver),
                        1.0,
                        Affine::IDENTITY,
                        &rr,
                    );
                    self.clip_depth += 1;
                }
            }
            PaintCommand::DrawText(point, text, size, color) => {
                // Full text shaping and glyph rasterization requires font
                // atlas integration deferred to v0.3.0. Here we approximate
                // each character as a filled rectangle so the scene is
                // non-empty and the text region is visible.
                let size_f64 = f64::from(*size);
                let char_width = size_f64 * 0.6;
                let mut x = point.x;
                for _ch in text.chars() {
                    let glyph_rect = KurboRect::new(x, point.y, x + char_width, point.y + size_f64);
                    self.scene.fill(
                        Fill::EvenOdd,
                        Affine::IDENTITY,
                        rgba_to_color(*color),
                        None,
                        &glyph_rect,
                    );
                    x += char_width;
                }
            }
            PaintCommand::DrawGlyphRun(run) => {
                self.render_glyph_run(run);
            }
        }
    }

    /// Renders a glyph run as filled rectangles using each glyph's dimensions.
    ///
    /// Full Vello glyph rasterization via `Scene::draw_glyphs` requires a
    /// `FontData` reference and resolved font atlas, which is deferred to
    /// v0.3.0. This approximation produces visible output proportional to
    /// each glyph's width and height.
    #[cfg(feature = "vello")]
    fn render_glyph_run(&mut self, run: &GlyphRun) {
        let color = rgba_to_color(run.color);
        for glyph in &run.glyphs {
            let rect = KurboRect::new(
                f64::from(glyph.x),
                f64::from(glyph.y),
                f64::from(glyph.x) + f64::from(glyph.width),
                f64::from(glyph.y) + f64::from(glyph.height),
            );
            self.scene
                .fill(Fill::EvenOdd, Affine::IDENTITY, color, None, &rect);
        }
    }
}

impl Default for VelloRenderer {
    fn default() -> Self {
        Self::new()
    }
}

impl RenderBackend for VelloRenderer {
    fn render(&mut self, paint_list: &PaintList) {
        self.last_command_count = paint_list.commands.len();
        #[cfg(feature = "vello")]
        {
            self.scene.reset();
            self.clip_depth = 0;
            for command in &paint_list.commands {
                self.render_command(command);
            }
            // Pop all pushed clip layers to keep the Vello scene balanced.
            // This matches the TinySkia clip_stack design where clips
            // accumulate for the remainder of the frame and reset each frame.
            for _ in 0..self.clip_depth {
                self.scene.pop_layer();
            }
            self.clip_depth = 0;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::paint::{GlyphInstance, GlyphRun, GradientStop, GradientStops, PaintList};
    use kurbo::{Point, Rect};

    #[test]
    fn new_starts_empty() {
        let renderer = VelloRenderer::new();
        assert_eq!(renderer.last_command_count(), 0);
    }

    #[test]
    fn render_records_command_count() {
        let mut renderer = VelloRenderer::new();
        let mut list = PaintList::new();
        list.push_fill_rect(Rect::ZERO, [255, 0, 0, 255]);
        list.push_fill_rect(Rect::ZERO, [0, 255, 0, 255]);
        renderer.render(&list);
        assert_eq!(renderer.last_command_count(), 2);
    }

    #[test]
    fn render_empty_list_records_zero() {
        let mut renderer = VelloRenderer::new();
        let list = PaintList::new();
        renderer.render(&list);
        assert_eq!(renderer.last_command_count(), 0);
    }

    #[cfg(feature = "vello")]
    #[test]
    fn reset_clears_scene() {
        let mut renderer = VelloRenderer::new();
        let mut list = PaintList::new();
        list.push_fill_rect(Rect::ZERO, [255, 0, 0, 255]);
        renderer.render(&list);
        renderer.reset();
        assert_eq!(renderer.last_command_count(), 0);
    }

    #[cfg(feature = "vello")]
    #[test]
    fn fill_rect_translates_to_scene() {
        let mut renderer = VelloRenderer::new();
        let mut list = PaintList::new();
        list.push_fill_rect(Rect::new(0.0, 0.0, 100.0, 100.0), [255, 0, 0, 255]);
        renderer.render(&list);
        assert!(renderer.scene().encoding().n_path_segments > 0);
    }

    #[cfg(feature = "vello")]
    #[test]
    fn stroke_rect_translates_to_scene() {
        let mut renderer = VelloRenderer::new();
        let mut list = PaintList::new();
        list.push_stroke_rect(Rect::new(0.0, 0.0, 50.0, 50.0), 2.0, [0, 0, 255, 255]);
        renderer.render(&list);
        assert!(renderer.scene().encoding().n_path_segments > 0);
    }

    #[cfg(feature = "vello")]
    #[test]
    fn fill_path_translates_to_scene() {
        let mut renderer = VelloRenderer::new();
        let mut list = PaintList::new();
        let mut builder = crate::paint::PathBuilder::new();
        builder.move_to(Point::new(0.0, 0.0));
        builder.line_to(Point::new(50.0, 0.0));
        builder.line_to(Point::new(25.0, 50.0));
        builder.close_path();
        list.push_path(builder.build(), [0, 255, 0, 255]);
        renderer.render(&list);
        assert!(renderer.scene().encoding().n_path_segments > 0);
    }

    #[cfg(feature = "vello")]
    #[test]
    fn linear_gradient_translates_to_scene() {
        let mut renderer = VelloRenderer::new();
        let mut list = PaintList::new();
        let stops = GradientStops {
            stops: vec![
                GradientStop::new(0.0, [255, 0, 0, 255]),
                GradientStop::new(1.0, [0, 0, 255, 255]),
            ],
        };
        list.push_linear_gradient(
            Rect::new(0.0, 0.0, 100.0, 100.0),
            stops,
            [0.0, 0.0],
            [100.0, 0.0],
        );
        renderer.render(&list);
        assert!(renderer.scene().encoding().n_path_segments > 0);
    }

    #[cfg(feature = "vello")]
    #[test]
    fn radial_gradient_translates_to_scene() {
        let mut renderer = VelloRenderer::new();
        let mut list = PaintList::new();
        let stops = GradientStops {
            stops: vec![
                GradientStop::new(0.0, [255, 255, 0, 255]),
                GradientStop::new(1.0, [255, 0, 0, 255]),
            ],
        };
        list.push_radial_gradient(Rect::new(0.0, 0.0, 100.0, 100.0), stops, [50.0, 50.0], 50.0);
        renderer.render(&list);
        assert!(renderer.scene().encoding().n_path_segments > 0);
    }

    #[cfg(feature = "vello")]
    #[test]
    fn clip_rect_pushes_layer() {
        let mut renderer = VelloRenderer::new();
        let mut list = PaintList::new();
        list.push_clip(Rect::new(0.0, 0.0, 100.0, 100.0));
        renderer.render(&list);
        assert!(renderer.scene().encoding().n_path_segments > 0);
    }

    #[cfg(feature = "vello")]
    #[test]
    fn draw_text_translates_to_scene() {
        let mut renderer = VelloRenderer::new();
        let mut list = PaintList::new();
        list.push_text(
            Point::new(10.0, 10.0),
            "Hi".to_string(),
            16.0,
            [0, 0, 0, 255],
        );
        renderer.render(&list);
        assert!(renderer.scene().encoding().n_path_segments > 0);
    }

    #[cfg(feature = "vello")]
    #[test]
    fn glyph_run_translates_to_scene() {
        let mut renderer = VelloRenderer::new();
        let mut list = PaintList::new();
        let mut run = GlyphRun::new(16.0, [0, 0, 0, 255]);
        run.glyphs.push(GlyphInstance::new(0.0, 0.0, 1, 10.0, 14.0));
        run.glyphs
            .push(GlyphInstance::new(12.0, 0.0, 2, 10.0, 14.0));
        list.push_glyph_run(run);
        renderer.render(&list);
        assert!(renderer.scene().encoding().n_path_segments > 0);
    }

    #[cfg(feature = "vello")]
    #[test]
    fn degenerate_rect_produces_no_path_segments() {
        let mut renderer = VelloRenderer::new();
        let mut list = PaintList::new();
        list.push_fill_rect(Rect::new(0.0, 0.0, 0.0, 0.0), [255, 0, 0, 255]);
        renderer.render(&list);
        assert_eq!(renderer.scene().encoding().n_path_segments, 0);
    }

    #[cfg(feature = "vello")]
    #[test]
    fn scene_mut_allows_mutable_access() {
        let mut renderer = VelloRenderer::new();
        let _scene: &mut Scene = renderer.scene_mut();
    }
}
