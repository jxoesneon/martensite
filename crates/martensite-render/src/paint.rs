//! The [`PaintList`] command stream and supporting types.
//!
//! Layout produces an ordered list of [`PaintCommand`]s which are then consumed
//! by a [`crate::RenderBackend`]. The representation is intentionally
//! allocation-light: a single [`PaintList`] can be cleared and reused across
//! frames so that no per-frame heap traffic is required for steady-state
//! rendering.

use std::sync::Arc;

use kurbo::{BezPath, Point, Rect};

/// A single color stop within a gradient, defined by a normalized position in
/// `[0.0, 1.0]` and an RGBA color.
///
/// # Examples
///
/// ```
/// use martensite_render::GradientStop;
///
/// let stop = GradientStop::new(0.5, [255, 128, 0, 255]);
/// assert_eq!(stop.position, 0.5);
/// assert_eq!(stop.color, [255, 128, 0, 255]);
/// ```
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct GradientStop {
    /// Normalized stop position in the range `[0.0, 1.0]`.
    pub position: f32,
    /// RGBA color at this stop (non-premultiplied).
    pub color: [u8; 4],
}

impl GradientStop {
    /// Creates a new gradient stop.
    pub const fn new(position: f32, color: [u8; 4]) -> Self {
        Self { position, color }
    }
}

/// An ordered collection of [`GradientStop`]s describing a color ramp.
///
/// # Examples
///
/// ```
/// use martensite_render::{GradientStop, GradientStops};
///
/// // Build a black-to-white ramp.
/// let mut stops = GradientStops::new();
/// assert!(stops.is_empty());
/// stops.push(GradientStop::new(0.0, [0, 0, 0, 255]));
/// stops.push(GradientStop::new(1.0, [255, 255, 255, 255]));
/// assert_eq!(stops.len(), 2);
/// assert!(!stops.is_empty());
///
/// // `from_slice` is convenient for static ramps.
/// let ramp = GradientStops::from_slice(&[
///     GradientStop::new(0.0, [255, 0, 0, 255]),
///     GradientStop::new(1.0, [0, 0, 255, 255]),
/// ]);
/// assert_eq!(ramp.len(), 2);
/// ```
#[derive(Clone, Debug, Default)]
pub struct GradientStops {
    /// The ordered stop list.
    pub stops: Vec<GradientStop>,
}

impl GradientStops {
    /// Creates an empty stop list.
    pub fn new() -> Self {
        Self::default()
    }

    /// Creates a stop list from the given slice.
    pub fn from_slice(stops: &[GradientStop]) -> Self {
        Self {
            stops: stops.to_vec(),
        }
    }

    /// Appends a single stop.
    pub fn push(&mut self, stop: GradientStop) {
        self.stops.push(stop);
    }

    /// Returns `true` when the stop list contains no entries.
    pub fn is_empty(&self) -> bool {
        self.stops.is_empty()
    }

    /// Returns the number of stops.
    pub fn len(&self) -> usize {
        self.stops.len()
    }
}

/// A backend-agnostic handle to a raw font file and its collection index.
///
/// This is the font data carrier that lets a `RenderBackend` rasterize
/// real glyph outlines from a [`GlyphRun`] without depending on any specific
/// text-shaping stack. The bytes are shared via an [`Arc`] so cloning a
/// `FontResource` is cheap and a single loaded font can be referenced by many
/// glyph runs.
///
/// The `index` field selects a face within a TrueType/OpenType collection
/// (`.ttc`); for a standalone `.ttf`/`.otf` file it is `0`.
///
/// Backends consume this as follows:
/// - The Vello backend builds a `peniko::FontData` from the bytes and calls
///   `vello::Scene::draw_glyphs`.
/// - The TinySkia backend parses the bytes with `swash` and rasterizes the
///   resulting Bézier outlines into its `Pixmap`.
///
/// # Examples
///
/// ```
/// use martensite_render::FontResource;
///
/// // `index` is 0 for a standalone font file.
/// let font = FontResource::new(b"raw font bytes".to_vec(), 0);
/// assert_eq!(font.index(), 0);
/// assert!(!font.data().is_empty());
///
/// // Cloning shares the underlying bytes (no copy).
/// let cloned = font.clone();
/// assert_eq!(font.data().as_ptr(), cloned.data().as_ptr());
/// ```
#[derive(Clone, Debug)]
pub struct FontResource {
    /// The raw font file bytes, shared via [`Arc`].
    data: Arc<[u8]>,
    /// The face index within a font collection (0 for standalone files).
    index: u32,
}

impl FontResource {
    /// Creates a new font resource from the given bytes and collection index.
    ///
    /// For a standalone `.ttf`/`.otf` file, pass `index = 0`.
    #[must_use]
    pub fn new(data: Vec<u8>, index: u32) -> Self {
        Self {
            data: Arc::from(data),
            index,
        }
    }

    /// Creates a new font resource from a static byte slice and collection
    /// index.
    ///
    /// This avoids the allocation that [`FontResource::new`] performs when the
    /// bytes already live in a `&'static [u8]` (e.g. from `include_bytes!`).
    #[must_use]
    pub fn from_static(data: &'static [u8], index: u32) -> Self {
        Self {
            data: Arc::from(data),
            index,
        }
    }

    /// Returns the raw font file bytes.
    #[must_use]
    pub fn data(&self) -> &[u8] {
        &self.data
    }

    /// Returns a reference to the shared byte buffer backing this resource.
    ///
    /// This is intended for backends (such as the Vello backend) that need to
    /// wrap the bytes in their own `Arc`-backed shared handle without copying.
    #[must_use]
    pub fn data_arc(&self) -> &Arc<[u8]> {
        &self.data
    }

    /// Returns the face index within a font collection.
    #[must_use]
    pub fn index(&self) -> u32 {
        self.index
    }
}

impl PartialEq for FontResource {
    fn eq(&self, other: &Self) -> bool {
        // Compare by pointer identity of the shared slice first (cheap), then
        // fall back to a byte-wise comparison. Two resources built from the
        // same `Arc` are equal without scanning the bytes.
        (Arc::ptr_eq(&self.data, &other.data) || self.data.as_ref() == other.data.as_ref())
            && self.index == other.index
    }
}

/// A single pre-resolved glyph instance ready for rasterization.
///
/// Coordinates are in device pixels and the `glyph_id` is an index into the
/// font's glyph table. This type is backend-agnostic: the concrete renderer is
/// responsible for mapping the id to the appropriate atlas or outline.
///
/// The `width` and `height` fields carry the glyph's pre-measured bounding box
/// in device pixels so that a backend can rasterize the glyph's footprint
/// without consulting a font atlas. Full glyph-outline rasterization is
/// deferred to the text pipeline (planned for v0.3.0).
///
/// # Examples
///
/// ```
/// use martensite_render::GlyphInstance;
///
/// // Place glyph id 36 at the baseline (0, 16) with a 9x16 px box.
/// let glyph = GlyphInstance::new(0.0, 16.0, 36, 9.0, 16.0);
/// assert_eq!(glyph.x, 0.0);
/// assert_eq!(glyph.y, 16.0);
/// assert_eq!(glyph.glyph_id, 36);
/// assert_eq!(glyph.width, 9.0);
/// assert_eq!(glyph.height, 16.0);
/// ```
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct GlyphInstance {
    /// The X coordinate of the glyph origin, in device pixels.
    pub x: f32,
    /// The Y coordinate of the glyph origin (baseline), in device pixels.
    pub y: f32,
    /// The font-specific glyph identifier.
    pub glyph_id: u32,
    /// The pre-measured glyph advance width, in device pixels.
    pub width: f32,
    /// The pre-measured glyph bounding-box height, in device pixels.
    pub height: f32,
}

impl GlyphInstance {
    /// Creates a new glyph instance with the given origin, identifier, and
    /// pre-measured bounding-box dimensions (in device pixels).
    pub fn new(x: f32, y: f32, glyph_id: u32, width: f32, height: f32) -> Self {
        Self {
            x,
            y,
            glyph_id,
            width,
            height,
        }
    }
}

/// A run of glyphs sharing a font, size, and color.
///
/// All glyph positions are pre-resolved so the backend can blit them without
/// performing any further shaping or layout work. When the optional
/// [`FontResource`] is set via [`GlyphRun::with_font`], a backend that supports
/// outline rasterization (Vello via `Scene::draw_glyphs`, TinySkia via
/// `swash`) will render real glyph outlines; otherwise it falls back to
/// drawing each glyph's pre-measured bounding box as a filled rectangle.
///
/// # Examples
///
/// ```
/// use martensite_render::{GlyphInstance, GlyphRun};
///
/// let mut run = GlyphRun::new(16.0, [0, 0, 0, 255]);
/// assert!(run.is_empty());
///
/// // Append pre-resolved glyph instances (positions in device pixels).
/// run.push(GlyphInstance::new(0.0, 16.0, 36, 9.0, 16.0));
/// run.push(GlyphInstance::new(9.0, 16.0, 68, 9.0, 16.0));
/// assert_eq!(run.glyphs.len(), 2);
/// assert!(!run.is_empty());
/// ```
#[derive(Clone, Debug, Default)]
pub struct GlyphRun {
    /// The font size in device pixels.
    pub font_size: f32,
    /// The RGBA color (non-premultiplied) of all glyphs in the run.
    pub color: [u8; 4],
    /// The pre-resolved glyph instances.
    pub glyphs: Vec<GlyphInstance>,
    /// The font data backing this run. When present, backends render real
    /// glyph outlines; when absent, they fall back to bounding-box
    /// rectangles.
    pub font: Option<FontResource>,
}

impl GlyphRun {
    /// Creates an empty glyph run.
    pub fn new(font_size: f32, color: [u8; 4]) -> Self {
        Self {
            font_size,
            color,
            glyphs: Vec::new(),
            font: None,
        }
    }

    /// Attaches a [`FontResource`] to this run, enabling real glyph-outline
    /// rasterization in backends that support it.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite_render::{FontResource, GlyphRun};
    ///
    /// let font = FontResource::new(b"font bytes".to_vec(), 0);
    /// let run = GlyphRun::new(16.0, [0, 0, 0, 255]).with_font(font);
    /// assert!(run.font.is_some());
    /// ```
    #[must_use]
    pub fn with_font(mut self, font: FontResource) -> Self {
        self.font = Some(font);
        self
    }

    /// Attaches a [`FontResource`] to this run in place.
    pub fn set_font(&mut self, font: FontResource) {
        self.font = Some(font);
    }

    /// Appends a single glyph instance.
    pub fn push(&mut self, glyph: GlyphInstance) {
        self.glyphs.push(glyph);
    }

    /// Returns `true` when the run contains no glyphs.
    pub fn is_empty(&self) -> bool {
        self.glyphs.is_empty()
    }
}

/// A single drawing operation emitted into a [`PaintList`].
///
/// # Examples
///
/// ```
/// use martensite_render::{GlyphRun, PaintCommand, PaintList};
/// use kurbo::Rect;
///
/// let mut list = PaintList::new();
///
/// // Fills and strokes are the most common commands.
/// list.commands.push(PaintCommand::FillRect(
///     Rect::new(0.0, 0.0, 50.0, 50.0),
///     [255, 0, 0, 255],
/// ));
/// list.commands.push(PaintCommand::StrokeRect(
///     Rect::new(0.0, 0.0, 50.0, 50.0),
///     1.0,
///     [0, 0, 0, 255],
/// ));
///
/// // Pre-resolved glyph runs carry their own font and positions.
/// list.commands.push(PaintCommand::DrawGlyphRun(GlyphRun::new(
///     16.0,
///     [0, 0, 0, 255],
/// )));
///
/// assert_eq!(list.len(), 3);
/// assert!(matches!(list.commands[0], PaintCommand::FillRect(..)));
/// assert!(matches!(list.commands[2], PaintCommand::DrawGlyphRun(..)));
/// ```
#[derive(Clone, Debug)]
pub enum PaintCommand {
    /// Fill a rectangle with a solid RGBA color.
    FillRect(Rect, [u8; 4]),
    /// Stroke the outline of a rectangle with the given line width and RGBA color.
    StrokeRect(Rect, f32, [u8; 4]),
    /// Fill a Bézier path with a solid RGBA color.
    FillPath(BezPath, [u8; 4]),
    /// Stroke a Bézier path with the given line width and RGBA color.
    StrokePath(BezPath, f32, [u8; 4]),
    /// Fill a rectangle with a linear gradient between two points.
    FillLinearGradient(Rect, GradientStops, [f64; 2], [f64; 2]),
    /// Fill a rectangle with a radial gradient centered at a point with a radius.
    FillRadialGradient(Rect, GradientStops, [f64; 2], f64),
    /// Push a rectangular clip onto the active clip stack.
    ClipRect(Rect),
    /// Push a rounded-rectangular clip onto the active clip stack.
    ClipRoundedRect(Rect, f32),
    /// Draw a text string at the given position, font size, and RGBA color.
    DrawText(Point, String, f32, [u8; 4]),
    /// Draw a pre-resolved [`GlyphRun`].
    DrawGlyphRun(GlyphRun),
}

/// A helper for incrementally constructing a [`kurbo::BezPath`].
///
/// This is a thin convenience wrapper around [`BezPath`] that records the
/// current subpath state and provides ergonomic builder methods. It is purely
/// advisory — the underlying [`BezPath`] can always be extracted with
/// [`PathBuilder::build`].
///
/// # Examples
///
/// ```
/// use martensite_render::PathBuilder;
/// use kurbo::Point;
///
/// let mut builder = PathBuilder::new();
/// builder.move_to(Point::new(0.0, 0.0));
/// builder.line_to(Point::new(10.0, 0.0));
/// builder.quad_to(Point::new(15.0, 5.0), Point::new(10.0, 10.0));
/// builder.line_to(Point::new(0.0, 10.0));
/// builder.close_path();
///
/// let path = builder.build();
/// // move + 2 lines + quad + close = 5 elements.
/// assert_eq!(path.elements().len(), 5);
/// ```
#[derive(Clone, Debug, Default)]
pub struct PathBuilder {
    path: BezPath,
}

impl PathBuilder {
    /// Creates a new, empty path builder.
    pub fn new() -> Self {
        Self::default()
    }

    /// Begins a new subpath at `p`.
    pub fn move_to(&mut self, p: Point) {
        self.path.move_to(p);
    }

    /// Adds a line segment to `p`.
    pub fn line_to(&mut self, p: Point) {
        self.path.line_to(p);
    }

    /// Adds a quadratic Bézier segment with control point `p1` and end `p2`.
    pub fn quad_to(&mut self, p1: Point, p2: Point) {
        self.path.quad_to(p1, p2);
    }

    /// Adds a cubic Bézier segment with control points `p1`, `p2` and end `p3`.
    pub fn curve_to(&mut self, p1: Point, p2: Point, p3: Point) {
        self.path.curve_to(p1, p2, p3);
    }

    /// Closes the current subpath.
    pub fn close_path(&mut self) {
        self.path.close_path();
    }

    /// Returns a reference to the underlying path elements.
    pub fn elements(&self) -> &[kurbo::PathEl] {
        self.path.elements()
    }

    /// Finalizes the builder and returns the constructed [`BezPath`].
    pub fn build(self) -> BezPath {
        self.path
    }
}

/// An ordered list of [`PaintCommand`]s produced by the layout phase and
/// consumed by a [`crate::RenderBackend`].
///
/// The list is designed for zero-allocation steady-state rendering: call
/// [`PaintList::clear`] between frames to retain the underlying capacity.
///
/// # Examples
///
/// ```
/// use martensite_render::PaintList;
/// use kurbo::Rect;
///
/// let mut list = PaintList::new();
/// assert!(list.is_empty());
///
/// // Record a frame's worth of commands.
/// list.push_fill_rect(Rect::new(0.0, 0.0, 100.0, 100.0), [255, 0, 0, 255]);
/// list.push_stroke_rect(Rect::new(0.0, 0.0, 100.0, 100.0), 2.0, [0, 0, 0, 255]);
/// assert_eq!(list.len(), 2);
/// assert!(!list.is_empty());
///
/// // Reuse the allocation for the next frame.
/// list.clear();
/// assert!(list.is_empty());
/// ```
#[derive(Default)]
pub struct PaintList {
    /// The ordered sequence of paint commands to render.
    pub commands: Vec<PaintCommand>,
}

impl PaintList {
    /// Creates a new, empty `PaintList`.
    pub fn new() -> Self {
        Self::default()
    }

    /// Removes all commands from the list, leaving it empty but preserving the
    /// allocated capacity for reuse on subsequent frames.
    pub fn clear(&mut self) {
        self.commands.clear();
    }

    /// Pushes a [`PaintCommand::FillRect`].
    pub fn push_fill_rect(&mut self, rect: Rect, color: [u8; 4]) {
        self.commands.push(PaintCommand::FillRect(rect, color));
    }

    /// Pushes a [`PaintCommand::StrokeRect`].
    pub fn push_stroke_rect(&mut self, rect: Rect, width: f32, color: [u8; 4]) {
        self.commands
            .push(PaintCommand::StrokeRect(rect, width, color));
    }

    /// Pushes a [`PaintCommand::FillPath`].
    pub fn push_path(&mut self, path: BezPath, color: [u8; 4]) {
        self.commands.push(PaintCommand::FillPath(path, color));
    }

    /// Pushes a [`PaintCommand::StrokePath`].
    pub fn push_stroke_path(&mut self, path: BezPath, width: f32, color: [u8; 4]) {
        self.commands
            .push(PaintCommand::StrokePath(path, width, color));
    }

    /// Pushes a [`PaintCommand::FillLinearGradient`].
    pub fn push_linear_gradient(
        &mut self,
        rect: Rect,
        stops: GradientStops,
        start: [f64; 2],
        end: [f64; 2],
    ) {
        self.commands
            .push(PaintCommand::FillLinearGradient(rect, stops, start, end));
    }

    /// Pushes a [`PaintCommand::FillRadialGradient`].
    pub fn push_radial_gradient(
        &mut self,
        rect: Rect,
        stops: GradientStops,
        center: [f64; 2],
        radius: f64,
    ) {
        self.commands.push(PaintCommand::FillRadialGradient(
            rect, stops, center, radius,
        ));
    }

    /// Pushes a [`PaintCommand::ClipRect`].
    pub fn push_clip(&mut self, rect: Rect) {
        self.commands.push(PaintCommand::ClipRect(rect));
    }

    /// Pushes a [`PaintCommand::ClipRoundedRect`].
    pub fn push_clip_rounded(&mut self, rect: Rect, radius: f32) {
        self.commands
            .push(PaintCommand::ClipRoundedRect(rect, radius));
    }

    /// Pushes a [`PaintCommand::DrawText`].
    pub fn push_text(&mut self, origin: Point, text: String, size: f32, color: [u8; 4]) {
        self.commands
            .push(PaintCommand::DrawText(origin, text, size, color));
    }

    /// Pushes a [`PaintCommand::DrawGlyphRun`].
    pub fn push_glyph_run(&mut self, run: GlyphRun) {
        self.commands.push(PaintCommand::DrawGlyphRun(run));
    }

    /// Returns the number of commands currently in the list.
    pub fn len(&self) -> usize {
        self.commands.len()
    }

    /// Returns `true` when the list contains no commands.
    pub fn is_empty(&self) -> bool {
        self.commands.is_empty()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn gradient_stop_new() {
        let stop = GradientStop::new(0.5, [1, 2, 3, 4]);
        assert_eq!(stop.position, 0.5);
        assert_eq!(stop.color, [1, 2, 3, 4]);
    }

    #[test]
    fn gradient_stops_push_and_len() {
        let mut stops = GradientStops::new();
        assert!(stops.is_empty());
        stops.push(GradientStop::new(0.0, [0, 0, 0, 255]));
        stops.push(GradientStop::new(1.0, [255, 255, 255, 255]));
        assert_eq!(stops.len(), 2);
        assert!(!stops.is_empty());
    }

    #[test]
    fn gradient_stops_from_slice() {
        let stops = GradientStops::from_slice(&[
            GradientStop::new(0.0, [0, 0, 0, 255]),
            GradientStop::new(1.0, [255, 0, 0, 255]),
        ]);
        assert_eq!(stops.len(), 2);
    }

    #[test]
    fn glyph_run_push_and_empty() {
        let mut run = GlyphRun::new(16.0, [0, 0, 0, 255]);
        assert!(run.is_empty());
        run.push(GlyphInstance::new(0.0, 0.0, 1, 8.0, 16.0));
        assert!(!run.is_empty());
        assert_eq!(run.glyphs.len(), 1);
    }

    #[test]
    fn path_builder_builds_closed_square() {
        let mut builder = PathBuilder::new();
        builder.move_to(Point::new(0.0, 0.0));
        builder.line_to(Point::new(10.0, 0.0));
        builder.line_to(Point::new(10.0, 10.0));
        builder.line_to(Point::new(0.0, 10.0));
        builder.close_path();
        let path = builder.build();
        // move + 3 lines + close = 5 elements
        assert_eq!(path.elements().len(), 5);
    }

    #[test]
    fn path_builder_quad_and_curve() {
        let mut builder = PathBuilder::new();
        builder.move_to(Point::new(0.0, 0.0));
        builder.quad_to(Point::new(5.0, 5.0), Point::new(10.0, 0.0));
        builder.curve_to(
            Point::new(15.0, 0.0),
            Point::new(20.0, 5.0),
            Point::new(25.0, 0.0),
        );
        let path = builder.build();
        // move + quad + curve = 3 elements
        assert_eq!(path.elements().len(), 3);
    }

    #[test]
    fn paint_list_push_fill_rect() {
        let mut list = PaintList::new();
        list.push_fill_rect(Rect::ZERO, [255, 0, 0, 255]);
        assert_eq!(list.len(), 1);
        assert!(matches!(list.commands[0], PaintCommand::FillRect(..)));
    }

    #[test]
    fn paint_list_push_stroke_rect() {
        let mut list = PaintList::new();
        list.push_stroke_rect(Rect::ZERO, 2.0, [0, 255, 0, 255]);
        assert_eq!(list.len(), 1);
        assert!(matches!(list.commands[0], PaintCommand::StrokeRect(..)));
    }

    #[test]
    fn paint_list_push_path() {
        let mut list = PaintList::new();
        let mut builder = PathBuilder::new();
        builder.move_to(Point::ZERO);
        builder.line_to(Point::new(1.0, 1.0));
        list.push_path(builder.build(), [0, 0, 255, 255]);
        assert_eq!(list.len(), 1);
        assert!(matches!(list.commands[0], PaintCommand::FillPath(..)));
    }

    #[test]
    fn paint_list_push_stroke_path() {
        let mut list = PaintList::new();
        let mut builder = PathBuilder::new();
        builder.move_to(Point::ZERO);
        builder.line_to(Point::new(1.0, 1.0));
        list.push_stroke_path(builder.build(), 1.5, [0, 0, 255, 255]);
        assert_eq!(list.len(), 1);
        assert!(matches!(list.commands[0], PaintCommand::StrokePath(..)));
    }

    #[test]
    fn paint_list_push_linear_gradient() {
        let mut list = PaintList::new();
        let stops = GradientStops::from_slice(&[
            GradientStop::new(0.0, [0, 0, 0, 255]),
            GradientStop::new(1.0, [255, 255, 255, 255]),
        ]);
        list.push_linear_gradient(Rect::ZERO, stops, [0.0, 0.0], [10.0, 0.0]);
        assert_eq!(list.len(), 1);
        assert!(matches!(
            list.commands[0],
            PaintCommand::FillLinearGradient(..)
        ));
    }

    #[test]
    fn paint_list_push_radial_gradient() {
        let mut list = PaintList::new();
        let stops = GradientStops::from_slice(&[
            GradientStop::new(0.0, [0, 0, 0, 255]),
            GradientStop::new(1.0, [255, 255, 255, 255]),
        ]);
        list.push_radial_gradient(Rect::ZERO, stops, [5.0, 5.0], 10.0);
        assert_eq!(list.len(), 1);
        assert!(matches!(
            list.commands[0],
            PaintCommand::FillRadialGradient(..)
        ));
    }

    #[test]
    fn paint_list_push_clip() {
        let mut list = PaintList::new();
        list.push_clip(Rect::new(0.0, 0.0, 10.0, 10.0));
        assert_eq!(list.len(), 1);
        assert!(matches!(list.commands[0], PaintCommand::ClipRect(..)));
    }

    #[test]
    fn paint_list_push_clip_rounded() {
        let mut list = PaintList::new();
        list.push_clip_rounded(Rect::new(0.0, 0.0, 10.0, 10.0), 4.0);
        assert_eq!(list.len(), 1);
        assert!(matches!(
            list.commands[0],
            PaintCommand::ClipRoundedRect(..)
        ));
    }

    #[test]
    fn paint_list_push_text_and_glyph_run() {
        let mut list = PaintList::new();
        list.push_text(Point::ZERO, "hi".to_string(), 12.0, [0, 0, 0, 255]);
        list.push_glyph_run(GlyphRun::new(12.0, [0, 0, 0, 255]));
        assert_eq!(list.len(), 2);
        assert!(matches!(list.commands[0], PaintCommand::DrawText(..)));
        assert!(matches!(list.commands[1], PaintCommand::DrawGlyphRun(..)));
    }

    #[test]
    fn paint_list_clear_preserves_capacity() {
        let mut list = PaintList::new();
        for _ in 0..10 {
            list.push_fill_rect(Rect::ZERO, [255, 0, 0, 255]);
        }
        let capacity = list.commands.capacity();
        assert!(capacity >= 10);
        list.clear();
        assert!(list.is_empty());
        assert_eq!(list.commands.capacity(), capacity);
    }

    #[test]
    fn paint_list_commands_appear_in_order() {
        let mut list = PaintList::new();
        list.push_fill_rect(Rect::ZERO, [255, 0, 0, 255]);
        list.push_stroke_rect(Rect::ZERO, 1.0, [0, 255, 0, 255]);
        list.push_text(Point::ZERO, "hi".to_string(), 12.0, [0, 0, 255, 255]);
        assert_eq!(list.len(), 3);
        assert!(matches!(list.commands[0], PaintCommand::FillRect(..)));
        assert!(matches!(list.commands[1], PaintCommand::StrokeRect(..)));
        assert!(matches!(list.commands[2], PaintCommand::DrawText(..)));
    }
}
