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
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite_render::GradientStop;
    ///
    /// let stop = GradientStop::new(0.25, [10, 20, 30, 255]);
    /// assert_eq!(stop.position, 0.25);
    /// assert_eq!(stop.color, [10, 20, 30, 255]);
    /// ```
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
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite_render::GradientStops;
    ///
    /// let stops = GradientStops::new();
    /// assert!(stops.is_empty());
    /// assert_eq!(stops.len(), 0);
    /// ```
    pub fn new() -> Self {
        Self::default()
    }

    /// Creates a stop list from the given slice.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite_render::{GradientStop, GradientStops};
    ///
    /// let stops = GradientStops::from_slice(&[
    ///     GradientStop::new(0.0, [0, 0, 0, 255]),
    ///     GradientStop::new(1.0, [255, 255, 255, 255]),
    /// ]);
    /// assert_eq!(stops.len(), 2);
    /// ```
    pub fn from_slice(stops: &[GradientStop]) -> Self {
        Self {
            stops: stops.to_vec(),
        }
    }

    /// Appends a single stop.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite_render::{GradientStop, GradientStops};
    ///
    /// let mut stops = GradientStops::new();
    /// stops.push(GradientStop::new(0.0, [255, 0, 0, 255]));
    /// stops.push(GradientStop::new(1.0, [0, 0, 255, 255]));
    /// assert_eq!(stops.len(), 2);
    /// ```
    pub fn push(&mut self, stop: GradientStop) {
        self.stops.push(stop);
    }

    /// Returns `true` when the stop list contains no entries.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite_render::{GradientStop, GradientStops};
    ///
    /// let mut stops = GradientStops::new();
    /// assert!(stops.is_empty());
    /// stops.push(GradientStop::new(0.0, [0, 0, 0, 255]));
    /// assert!(!stops.is_empty());
    /// ```
    pub fn is_empty(&self) -> bool {
        self.stops.is_empty()
    }

    /// Returns the number of stops.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite_render::{GradientStop, GradientStops};
    ///
    /// let stops = GradientStops::from_slice(&[
    ///     GradientStop::new(0.0, [0, 0, 0, 255]),
    ///     GradientStop::new(1.0, [255, 255, 255, 255]),
    /// ]);
    /// assert_eq!(stops.len(), 2);
    /// ```
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
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite_render::FontResource;
    ///
    /// let font = FontResource::new(b"font bytes".to_vec(), 0);
    /// assert_eq!(font.index(), 0);
    /// assert!(!font.data().is_empty());
    /// ```
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
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite_render::FontResource;
    ///
    /// static BYTES: &[u8] = b"static font bytes";
    /// let font = FontResource::from_static(BYTES, 0);
    /// assert_eq!(font.index(), 0);
    /// assert_eq!(font.data(), BYTES);
    /// ```
    #[must_use]
    pub fn from_static(data: &'static [u8], index: u32) -> Self {
        Self {
            data: Arc::from(data),
            index,
        }
    }

    /// Returns the raw font file bytes.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite_render::FontResource;
    ///
    /// let font = FontResource::new(b"hello".to_vec(), 0);
    /// assert_eq!(font.data(), b"hello");
    /// ```
    #[must_use]
    pub fn data(&self) -> &[u8] {
        &self.data
    }

    /// Returns a reference to the shared byte buffer backing this resource.
    ///
    /// This is intended for backends (such as the Vello backend) that need to
    /// wrap the bytes in their own `Arc`-backed shared handle without copying.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite_render::FontResource;
    ///
    /// let font = FontResource::new(b"hello".to_vec(), 0);
    /// let arc = font.data_arc();
    /// assert_eq!(arc.as_ref(), b"hello");
    /// ```
    #[must_use]
    pub fn data_arc(&self) -> &Arc<[u8]> {
        &self.data
    }

    /// Returns the face index within a font collection.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite_render::FontResource;
    ///
    /// let font = FontResource::new(b"font".to_vec(), 2);
    /// assert_eq!(font.index(), 2);
    /// ```
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
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite_render::GlyphInstance;
    ///
    /// let glyph = GlyphInstance::new(10.0, 20.0, 42, 12.0, 16.0);
    /// assert_eq!(glyph.x, 10.0);
    /// assert_eq!(glyph.y, 20.0);
    /// assert_eq!(glyph.glyph_id, 42);
    /// assert_eq!(glyph.width, 12.0);
    /// assert_eq!(glyph.height, 16.0);
    /// ```
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
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite_render::GlyphRun;
    ///
    /// let run = GlyphRun::new(16.0, [0, 0, 0, 255]);
    /// assert!(run.is_empty());
    /// assert_eq!(run.font_size, 16.0);
    /// assert_eq!(run.color, [0, 0, 0, 255]);
    /// ```
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
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite_render::{FontResource, GlyphRun};
    ///
    /// let mut run = GlyphRun::new(16.0, [0, 0, 0, 255]);
    /// assert!(run.font.is_none());
    /// run.set_font(FontResource::new(b"font".to_vec(), 0));
    /// assert!(run.font.is_some());
    /// ```
    pub fn set_font(&mut self, font: FontResource) {
        self.font = Some(font);
    }

    /// Appends a single glyph instance.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite_render::{GlyphInstance, GlyphRun};
    ///
    /// let mut run = GlyphRun::new(16.0, [0, 0, 0, 255]);
    /// run.push(GlyphInstance::new(0.0, 16.0, 36, 9.0, 16.0));
    /// assert_eq!(run.glyphs.len(), 1);
    /// ```
    pub fn push(&mut self, glyph: GlyphInstance) {
        self.glyphs.push(glyph);
    }

    /// Returns `true` when the run contains no glyphs.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite_render::{GlyphInstance, GlyphRun};
    ///
    /// let mut run = GlyphRun::new(16.0, [0, 0, 0, 255]);
    /// assert!(run.is_empty());
    /// run.push(GlyphInstance::new(0.0, 0.0, 1, 8.0, 16.0));
    /// assert!(!run.is_empty());
    /// ```
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
    /// A blurred filled rectangle, used for CSD shadows and backdrop blur effects.
    ///
    /// The blur is a two-pass Gaussian (horizontal + vertical) applied to a
    /// solid-color rect. The blur radius is in physical pixels.
    BlurredRect {
        /// The rectangle bounds (`x`, `y`, `w`, `h`).
        rect: [f32; 4],
        /// The blur radius in physical pixels.
        blur_radius: f32,
        /// The fill color (R, G, B, A), each channel in `0.0..=1.0`.
        color: [f32; 4],
    },
    /// Composite an externally-produced GPU surface into the scene.
    ///
    /// `surface_id` refers to a surface registered with the
    /// `martensite-engine-bridge` registry / `martensite-wgpu` `WgpuHost`.
    /// The Vello scene builder emits nothing for this command — the
    /// orchestrator composites the external texture into the target at
    /// this exact position in the paint order (see `PaintList::segments`).
    /// The TinySkia backend draws a documented checkerboard placeholder.
    External {
        /// The external surface identifier (`SurfaceId` raw value).
        surface_id: u64,
        /// The destination rectangle in physical pixels (`x`, `y`, `w`, `h`).
        rect: [f32; 4],
        /// The clip rectangle in physical pixels (`x`, `y`, `w`, `h`).
        clip: [f32; 4],
    },
}

/// One element of a [`PaintList`] split at [`PaintCommand::External`]
/// boundaries.
///
/// The orchestrator renders each [`PaintSegment::Commands`] span with the
/// normal backend and composites each [`PaintSegment::External`] via the
/// `WgpuHost` pipeline, preserving exact paint ordering.
///
/// # Examples
///
/// ```
/// use martensite_render::{PaintList, PaintSegment};
/// use kurbo::Rect;
///
/// let mut list = PaintList::new();
/// list.push_fill_rect(Rect::new(0.0, 0.0, 10.0, 10.0), [255, 0, 0, 255]);
/// list.push_external(7, [0.0, 0.0, 100.0, 50.0], [0.0, 0.0, 100.0, 50.0]);
/// list.push_fill_rect(Rect::new(0.0, 0.0, 5.0, 5.0), [0, 255, 0, 255]);
///
/// let segments = list.segments();
/// assert_eq!(segments.len(), 3);
/// assert!(matches!(segments[1], PaintSegment::External { surface_id: 7, .. }));
/// ```
#[derive(Debug)]
pub enum PaintSegment<'a> {
    /// A contiguous span of ordinary paint commands.
    Commands(&'a [PaintCommand]),
    /// An external-surface composite point.
    External {
        /// The external surface identifier.
        surface_id: u64,
        /// Destination rectangle in physical pixels.
        rect: [f32; 4],
        /// Clip rectangle in physical pixels.
        clip: [f32; 4],
    },
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
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite_render::PathBuilder;
    ///
    /// let builder = PathBuilder::new();
    /// assert!(builder.elements().is_empty());
    /// ```
    pub fn new() -> Self {
        Self::default()
    }

    /// Begins a new subpath at `p`.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite_render::PathBuilder;
    /// use kurbo::Point;
    ///
    /// let mut builder = PathBuilder::new();
    /// builder.move_to(Point::new(5.0, 5.0));
    /// assert_eq!(builder.elements().len(), 1);
    /// ```
    pub fn move_to(&mut self, p: Point) {
        self.path.move_to(p);
    }

    /// Adds a line segment to `p`.
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
    /// assert_eq!(builder.elements().len(), 2);
    /// ```
    pub fn line_to(&mut self, p: Point) {
        self.path.line_to(p);
    }

    /// Adds a quadratic Bézier segment with control point `p1` and end `p2`.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite_render::PathBuilder;
    /// use kurbo::Point;
    ///
    /// let mut builder = PathBuilder::new();
    /// builder.move_to(Point::new(0.0, 0.0));
    /// builder.quad_to(Point::new(5.0, 5.0), Point::new(10.0, 0.0));
    /// assert_eq!(builder.elements().len(), 2);
    /// ```
    pub fn quad_to(&mut self, p1: Point, p2: Point) {
        self.path.quad_to(p1, p2);
    }

    /// Adds a cubic Bézier segment with control points `p1`, `p2` and end `p3`.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite_render::PathBuilder;
    /// use kurbo::Point;
    ///
    /// let mut builder = PathBuilder::new();
    /// builder.move_to(Point::new(0.0, 0.0));
    /// builder.curve_to(
    ///     Point::new(5.0, 0.0),
    ///     Point::new(10.0, 5.0),
    ///     Point::new(15.0, 0.0),
    /// );
    /// assert_eq!(builder.elements().len(), 2);
    /// ```
    pub fn curve_to(&mut self, p1: Point, p2: Point, p3: Point) {
        self.path.curve_to(p1, p2, p3);
    }

    /// Closes the current subpath.
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
    /// builder.close_path();
    /// // move + line + close = 3 elements
    /// assert_eq!(builder.elements().len(), 3);
    /// ```
    pub fn close_path(&mut self) {
        self.path.close_path();
    }

    /// Returns a reference to the underlying path elements.
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
    /// let elements = builder.elements();
    /// assert_eq!(elements.len(), 2);
    /// ```
    pub fn elements(&self) -> &[kurbo::PathEl] {
        self.path.elements()
    }

    /// Finalizes the builder and returns the constructed [`BezPath`].
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
    /// builder.close_path();
    /// let path = builder.build();
    /// // move + line + close = 3 elements
    /// assert_eq!(path.elements().len(), 3);
    /// ```
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
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite_render::PaintList;
    ///
    /// let list = PaintList::new();
    /// assert!(list.is_empty());
    /// assert_eq!(list.len(), 0);
    /// ```
    pub fn new() -> Self {
        Self::default()
    }

    /// Removes all commands from the list, leaving it empty but preserving the
    /// allocated capacity for reuse on subsequent frames.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite_render::PaintList;
    /// use kurbo::Rect;
    ///
    /// let mut list = PaintList::new();
    /// list.push_fill_rect(Rect::ZERO, [255, 0, 0, 255]);
    /// assert!(!list.is_empty());
    /// list.clear();
    /// assert!(list.is_empty());
    /// ```
    pub fn clear(&mut self) {
        self.commands.clear();
    }

    /// Pushes a [`PaintCommand::FillRect`].
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite_render::{PaintCommand, PaintList};
    /// use kurbo::Rect;
    ///
    /// let mut list = PaintList::new();
    /// list.push_fill_rect(Rect::new(0.0, 0.0, 50.0, 50.0), [255, 0, 0, 255]);
    /// assert_eq!(list.len(), 1);
    /// assert!(matches!(list.commands[0], PaintCommand::FillRect(..)));
    /// ```
    pub fn push_fill_rect(&mut self, rect: Rect, color: [u8; 4]) {
        self.commands.push(PaintCommand::FillRect(rect, color));
    }

    /// Pushes a [`PaintCommand::StrokeRect`].
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite_render::{PaintCommand, PaintList};
    /// use kurbo::Rect;
    ///
    /// let mut list = PaintList::new();
    /// list.push_stroke_rect(Rect::new(0.0, 0.0, 50.0, 50.0), 2.0, [0, 0, 0, 255]);
    /// assert_eq!(list.len(), 1);
    /// assert!(matches!(list.commands[0], PaintCommand::StrokeRect(..)));
    /// ```
    pub fn push_stroke_rect(&mut self, rect: Rect, width: f32, color: [u8; 4]) {
        self.commands
            .push(PaintCommand::StrokeRect(rect, width, color));
    }

    /// Pushes a [`PaintCommand::FillPath`].
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite_render::{PaintCommand, PaintList, PathBuilder};
    /// use kurbo::Point;
    ///
    /// let mut builder = PathBuilder::new();
    /// builder.move_to(Point::new(0.0, 0.0));
    /// builder.line_to(Point::new(10.0, 10.0));
    ///
    /// let mut list = PaintList::new();
    /// list.push_path(builder.build(), [0, 0, 255, 255]);
    /// assert_eq!(list.len(), 1);
    /// assert!(matches!(list.commands[0], PaintCommand::FillPath(..)));
    /// ```
    pub fn push_path(&mut self, path: BezPath, color: [u8; 4]) {
        self.commands.push(PaintCommand::FillPath(path, color));
    }

    /// Pushes a [`PaintCommand::StrokePath`].
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite_render::{PaintCommand, PaintList, PathBuilder};
    /// use kurbo::Point;
    ///
    /// let mut builder = PathBuilder::new();
    /// builder.move_to(Point::new(0.0, 0.0));
    /// builder.line_to(Point::new(10.0, 10.0));
    ///
    /// let mut list = PaintList::new();
    /// list.push_stroke_path(builder.build(), 1.5, [0, 0, 255, 255]);
    /// assert_eq!(list.len(), 1);
    /// assert!(matches!(list.commands[0], PaintCommand::StrokePath(..)));
    /// ```
    pub fn push_stroke_path(&mut self, path: BezPath, width: f32, color: [u8; 4]) {
        self.commands
            .push(PaintCommand::StrokePath(path, width, color));
    }

    /// Pushes a [`PaintCommand::FillLinearGradient`].
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite_render::{GradientStop, GradientStops, PaintCommand, PaintList};
    /// use kurbo::Rect;
    ///
    /// let stops = GradientStops::from_slice(&[
    ///     GradientStop::new(0.0, [0, 0, 0, 255]),
    ///     GradientStop::new(1.0, [255, 255, 255, 255]),
    /// ]);
    ///
    /// let mut list = PaintList::new();
    /// list.push_linear_gradient(Rect::new(0.0, 0.0, 100.0, 100.0), stops, [0.0, 0.0], [100.0, 0.0]);
    /// assert_eq!(list.len(), 1);
    /// assert!(matches!(list.commands[0], PaintCommand::FillLinearGradient(..)));
    /// ```
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
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite_render::{GradientStop, GradientStops, PaintCommand, PaintList};
    /// use kurbo::Rect;
    ///
    /// let stops = GradientStops::from_slice(&[
    ///     GradientStop::new(0.0, [255, 255, 255, 255]),
    ///     GradientStop::new(1.0, [0, 0, 0, 255]),
    /// ]);
    ///
    /// let mut list = PaintList::new();
    /// list.push_radial_gradient(Rect::new(0.0, 0.0, 100.0, 100.0), stops, [50.0, 50.0], 50.0);
    /// assert_eq!(list.len(), 1);
    /// assert!(matches!(list.commands[0], PaintCommand::FillRadialGradient(..)));
    /// ```
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
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite_render::{PaintCommand, PaintList};
    /// use kurbo::Rect;
    ///
    /// let mut list = PaintList::new();
    /// list.push_clip(Rect::new(0.0, 0.0, 100.0, 100.0));
    /// assert_eq!(list.len(), 1);
    /// assert!(matches!(list.commands[0], PaintCommand::ClipRect(..)));
    /// ```
    pub fn push_clip(&mut self, rect: Rect) {
        self.commands.push(PaintCommand::ClipRect(rect));
    }

    /// Pushes a [`PaintCommand::ClipRoundedRect`].
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite_render::{PaintCommand, PaintList};
    /// use kurbo::Rect;
    ///
    /// let mut list = PaintList::new();
    /// list.push_clip_rounded(Rect::new(0.0, 0.0, 100.0, 100.0), 8.0);
    /// assert_eq!(list.len(), 1);
    /// assert!(matches!(list.commands[0], PaintCommand::ClipRoundedRect(..)));
    /// ```
    pub fn push_clip_rounded(&mut self, rect: Rect, radius: f32) {
        self.commands
            .push(PaintCommand::ClipRoundedRect(rect, radius));
    }

    /// Pushes a [`PaintCommand::DrawText`].
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite_render::{PaintCommand, PaintList};
    /// use kurbo::Point;
    ///
    /// let mut list = PaintList::new();
    /// list.push_text(Point::new(10.0, 20.0), "hi".to_string(), 16.0, [0, 0, 0, 255]);
    /// assert_eq!(list.len(), 1);
    /// assert!(matches!(list.commands[0], PaintCommand::DrawText(..)));
    /// ```
    pub fn push_text(&mut self, origin: Point, text: String, size: f32, color: [u8; 4]) {
        self.commands
            .push(PaintCommand::DrawText(origin, text, size, color));
    }

    /// Pushes a [`PaintCommand::DrawGlyphRun`].
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite_render::{GlyphRun, PaintCommand, PaintList};
    ///
    /// let mut list = PaintList::new();
    /// let run = GlyphRun::new(16.0, [0, 0, 0, 255]);
    /// list.push_glyph_run(run);
    /// assert_eq!(list.len(), 1);
    /// assert!(matches!(list.commands[0], PaintCommand::DrawGlyphRun(..)));
    /// ```
    pub fn push_glyph_run(&mut self, run: GlyphRun) {
        self.commands.push(PaintCommand::DrawGlyphRun(run));
    }

    /// Pushes a [`PaintCommand::BlurredRect`].
    ///
    /// The blur is a two-pass Gaussian applied at physical resolution to
    /// avoid upscaling artifacts at fractional DPI. `rect` is `[x, y, w, h]`
    /// in physical pixels, `blur_radius` is the blur radius in physical
    /// pixels, and `color` is `[R, G, B, A]` with each channel in `0.0..=1.0`.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite_render::{PaintCommand, PaintList};
    ///
    /// let mut list = PaintList::new();
    /// list.push_blurred_rect([0.0, 0.0, 100.0, 100.0], 20.0, [0.0, 0.0, 0.0, 0.3]);
    /// assert_eq!(list.len(), 1);
    /// assert!(matches!(list.commands[0], PaintCommand::BlurredRect { .. }));
    /// ```
    pub fn push_blurred_rect(&mut self, rect: [f32; 4], blur_radius: f32, color: [f32; 4]) {
        self.commands.push(PaintCommand::BlurredRect {
            rect,
            blur_radius,
            color,
        });
    }

    /// Pushes a [`PaintCommand::External`] — a marker that an external GPU
    /// surface must be composited at this point in the paint order.
    ///
    /// `rect` is the widget's destination rectangle and `clip` the active
    /// clip region, both in physical pixels.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite_render::{PaintCommand, PaintList};
    ///
    /// let mut list = PaintList::new();
    /// list.push_external(42, [10.0, 10.0, 640.0, 360.0], [10.0, 10.0, 640.0, 360.0]);
    /// assert!(matches!(
    ///     list.commands[0],
    ///     PaintCommand::External { surface_id: 42, .. }
    /// ));
    /// ```
    pub fn push_external(&mut self, surface_id: u64, rect: [f32; 4], clip: [f32; 4]) {
        self.commands.push(PaintCommand::External {
            surface_id,
            rect,
            clip,
        });
    }

    /// Splits the command list at [`PaintCommand::External`] boundaries.
    ///
    /// The result alternates ordinary command spans and external markers
    /// in paint order. Rendering each `Commands` span with the normal
    /// backend and compositing each `External` via the `WgpuHost`
    /// pipeline reproduces the exact z-order — including UI elements
    /// drawn both below and above the external content.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite_render::{PaintList, PaintSegment};
    /// use kurbo::Rect;
    ///
    /// let mut list = PaintList::new();
    /// list.push_fill_rect(Rect::ZERO, [0, 0, 0, 255]);
    /// list.push_external(1, [0.0, 0.0, 8.0, 8.0], [0.0, 0.0, 8.0, 8.0]);
    /// list.push_fill_rect(Rect::ZERO, [9, 9, 9, 255]);
    /// list.push_external(2, [1.0, 1.0, 8.0, 8.0], [1.0, 1.0, 8.0, 8.0]);
    ///
    /// let segments = list.segments();
    /// // Commands → External → Commands → External (trailing empty span omitted).
    /// assert_eq!(segments.len(), 4);
    /// ```
    pub fn segments(&self) -> Vec<PaintSegment<'_>> {
        let mut out = Vec::new();
        let mut start = 0usize;
        for (i, cmd) in self.commands.iter().enumerate() {
            if let PaintCommand::External {
                surface_id,
                rect,
                clip,
            } = *cmd
            {
                if i > start {
                    out.push(PaintSegment::Commands(&self.commands[start..i]));
                }
                out.push(PaintSegment::External {
                    surface_id,
                    rect,
                    clip,
                });
                start = i + 1;
            }
        }
        if start < self.commands.len() {
            out.push(PaintSegment::Commands(&self.commands[start..]));
        }
        out
    }

    /// Returns `true` when the list contains at least one
    /// [`PaintCommand::External`] marker.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite_render::PaintList;
    ///
    /// let mut list = PaintList::new();
    /// assert!(!list.has_external());
    /// list.push_external(1, [0.0, 0.0, 4.0, 4.0], [0.0, 0.0, 4.0, 4.0]);
    /// assert!(list.has_external());
    /// ```
    pub fn has_external(&self) -> bool {
        self.commands
            .iter()
            .any(|c| matches!(c, PaintCommand::External { .. }))
    }

    /// Returns the number of commands currently in the list.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite_render::PaintList;
    /// use kurbo::Rect;
    ///
    /// let mut list = PaintList::new();
    /// assert_eq!(list.len(), 0);
    /// list.push_fill_rect(Rect::ZERO, [255, 0, 0, 255]);
    /// assert_eq!(list.len(), 1);
    /// ```
    pub fn len(&self) -> usize {
        self.commands.len()
    }

    /// Returns `true` when the list contains no commands.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite_render::PaintList;
    /// use kurbo::Rect;
    ///
    /// let mut list = PaintList::new();
    /// assert!(list.is_empty());
    /// list.push_fill_rect(Rect::ZERO, [255, 0, 0, 255]);
    /// assert!(!list.is_empty());
    /// ```
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

    #[test]
    fn segments_split_at_external_markers() {
        let mut list = PaintList::new();
        list.push_fill_rect(Rect::ZERO, [255, 0, 0, 255]);
        list.push_external(7, [0.0, 0.0, 100.0, 50.0], [0.0, 0.0, 100.0, 50.0]);
        list.push_fill_rect(Rect::ZERO, [0, 255, 0, 255]);
        list.push_external(9, [0.0, 0.0, 10.0, 10.0], [0.0, 0.0, 10.0, 10.0]);
        list.push_fill_rect(Rect::ZERO, [0, 0, 255, 255]);

        let segments = list.segments();
        assert_eq!(segments.len(), 5);
        assert!(matches!(segments[0], PaintSegment::Commands(c) if c.len() == 1));
        assert!(matches!(
            segments[1],
            PaintSegment::External { surface_id: 7, .. }
        ));
        assert!(matches!(segments[2], PaintSegment::Commands(c) if c.len() == 1));
        assert!(matches!(
            segments[3],
            PaintSegment::External { surface_id: 9, .. }
        ));
        assert!(matches!(segments[4], PaintSegment::Commands(c) if c.len() == 1));
    }

    #[test]
    fn segments_omit_empty_spans() {
        // External first and External last produce no empty command spans.
        let mut list = PaintList::new();
        list.push_external(1, [0.0; 4], [0.0; 4]);
        list.push_fill_rect(Rect::ZERO, [0, 0, 0, 255]);
        list.push_external(2, [0.0; 4], [0.0; 4]);
        let segments = list.segments();
        assert_eq!(segments.len(), 3);
        assert!(matches!(
            segments[0],
            PaintSegment::External { surface_id: 1, .. }
        ));
        assert!(matches!(segments[1], PaintSegment::Commands(_)));
        assert!(matches!(
            segments[2],
            PaintSegment::External { surface_id: 2, .. }
        ));
    }

    #[test]
    fn segments_no_externals_single_span() {
        let mut list = PaintList::new();
        list.push_fill_rect(Rect::ZERO, [1, 2, 3, 4]);
        let segments = list.segments();
        assert_eq!(segments.len(), 1);
        assert!(matches!(segments[0], PaintSegment::Commands(c) if c.len() == 1));
        assert!(!list.has_external());
    }
}
