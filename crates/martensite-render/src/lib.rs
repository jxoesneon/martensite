//! Intermediate PaintList command stream and rendering backends.
//!
//! This crate provides the hardware-agnostic [`PaintList`] command stream
//! produced by layout, plus two concrete implementations of the
//! [`RenderBackend`] trait:
//!
//! - [`tinyskia_backend::TinySkiaBackend`] — a pure-CPU rasterizer built on
//!   `tiny_skia`, suitable for headless CI and software fallback.
//! - [`vello_backend::VelloRenderer`] — a GPU renderer that translates a
//!   `PaintList` into a Vello scene (gated behind the `vello` feature).
//!
//! A perceptual diffing engine ([`diff`]) is provided for reftest-style
//! verification of rendered output.

#![forbid(unsafe_code)]

pub mod diff;
pub mod paint;
pub mod presentation;
pub mod tinyskia_backend;
pub mod vello_backend;

pub use paint::{
    FontResource, GlyphInstance, GlyphRun, GradientStop, GradientStops, PaintCommand, PaintList,
    PathBuilder,
};
pub use presentation::{
    nonzero as presentation_nonzero, present_rgba_to_softbuffer, rgba_to_softbuffer,
    PresentationError, SoftbufferPresenter,
};
pub use tinyskia_backend::TinySkiaBackend;
pub use vello_backend::VelloRenderer;

// Re-export the core rendering trait and supporting geometry types for
// downstream convenience.
pub use kurbo::{BezPath, Point, Rect};

/// Controls how the render target is cleared at the start of a frame.
///
/// `ClearMode::Opaque` clears with a solid color (preserving the historical
/// opaque-black behavior), while `ClearMode::Transparent` clears with fully
/// transparent black so the system compositor's backdrop shows through — this
/// is required for CSD window shadows and the Liquid Glass backdrop blur on
/// Linux where the surface itself must be transparent.
///
/// # Examples
///
/// ```
/// use martensite_render::ClearMode;
///
/// let opaque = ClearMode::Opaque([0.0, 0.0, 0.0, 1.0]);
/// let transparent = ClearMode::Transparent;
/// assert_ne!(opaque, transparent);
/// ```
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum ClearMode {
    /// Clear with an opaque solid color (R, G, B, A=1.0).
    Opaque([f32; 4]),
    /// Clear with transparent black (0, 0, 0, 0) for system backdrop compositing.
    Transparent,
}

impl Default for ClearMode {
    /// The default clear mode is transparent black, preserving the
    /// historical behavior of both backends (TinySkia's `clear()` and
    /// Vello's `base_color` both cleared to fully transparent) so that
    /// existing callers of [`RenderBackend::render`] see no behavior change.
    /// Use [`ClearMode::Opaque`] explicitly via
    /// [`RenderBackend::render_with_clear`] when an opaque backdrop is
    /// desired.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite_render::ClearMode;
    ///
    /// let mode = ClearMode::default();
    /// assert_eq!(mode, ClearMode::Transparent);
    /// ```
    fn default() -> Self {
        Self::Transparent
    }
}

/// Abstraction over the concrete rendering target that consumes a [`PaintList`].
///
/// The primary entry point is [`RenderBackend::render_with_clear`], which
/// accepts a [`ClearMode`] controlling how the target is cleared at the start
/// of a frame. The convenience method [`RenderBackend::render`] is a thin
/// wrapper that forwards to `render_with_clear` with [`ClearMode::default`]
/// (transparent black), preserving the historical behavior of the pipeline
/// before opaque backdrops were introduced.
///
/// # Examples
///
/// ```
/// use martensite_render::{ClearMode, PaintList, RenderBackend};
/// use kurbo::Rect;
///
/// // A mock backend that records how many commands it received.
/// struct CountingBackend {
///     received: usize,
/// }
///
/// impl RenderBackend for CountingBackend {
///     fn render_with_clear(&mut self, paint_list: &PaintList, _clear_mode: ClearMode) {
///         self.received = paint_list.len();
///     }
/// }
///
/// let mut list = PaintList::new();
/// list.push_fill_rect(Rect::ZERO, [255, 0, 0, 255]);
///
/// let mut backend = CountingBackend { received: 0 };
/// backend.render(&list);
/// assert_eq!(backend.received, 1);
/// ```
pub trait RenderBackend: Send + 'static {
    /// Renders the given [`PaintList`] to this backend's output surface,
    /// clearing the target according to `clear_mode` first.
    ///
    /// Implementors should perform the clear, reset any per-frame state
    /// (such as clip stacks), and replay the supplied commands in order.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite_render::{ClearMode, PaintList, RenderBackend};
    /// use kurbo::Rect;
    ///
    /// struct Recorder { count: usize }
    /// impl RenderBackend for Recorder {
    ///     fn render_with_clear(&mut self, paint_list: &PaintList, _clear_mode: ClearMode) {
    ///         self.count = paint_list.len();
    ///     }
    /// }
    ///
    /// let mut list = PaintList::new();
    /// list.push_fill_rect(Rect::ZERO, [255, 0, 0, 255]);
    ///
    /// let mut backend = Recorder { count: 0 };
    /// backend.render_with_clear(&list, ClearMode::Transparent);
    /// assert_eq!(backend.count, 1);
    /// ```
    fn render_with_clear(&mut self, paint_list: &PaintList, clear_mode: ClearMode);

    /// Renders the given [`PaintList`] using the default [`ClearMode`]
    /// (transparent black), preserving the historical behavior of the render
    /// pipeline where both backends cleared to fully transparent.
    ///
    /// This is a convenience wrapper around [`RenderBackend::render_with_clear`];
    /// callers that need a transparent backdrop (for CSD shadows or Liquid
    /// Glass compositing) should call `render_with_clear` directly with
    /// [`ClearMode::Transparent`].
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite_render::{PaintList, RenderBackend};
    /// use kurbo::Rect;
    ///
    /// struct Recorder { count: usize }
    /// impl RenderBackend for Recorder {
    ///     fn render_with_clear(&mut self, paint_list: &PaintList, _clear_mode: martensite_render::ClearMode) {
    ///         self.count = paint_list.len();
    ///     }
    /// }
    ///
    /// let mut list = PaintList::new();
    /// list.push_fill_rect(Rect::ZERO, [255, 0, 0, 255]);
    ///
    /// let mut backend = Recorder { count: 0 };
    /// backend.render(&list);
    /// assert_eq!(backend.count, 1);
    /// ```
    fn render(&mut self, paint_list: &PaintList) {
        self.render_with_clear(paint_list, ClearMode::default());
    }
}

#[cfg(test)]
mod tests {
    use super::{ClearMode, PaintCommand, PaintList, RenderBackend};
    use kurbo::{Point, Rect};

    /// A mock backend that records the number of commands it received.
    struct MockBackend {
        received_count: usize,
    }

    impl MockBackend {
        fn new() -> Self {
            Self { received_count: 0 }
        }
    }

    impl RenderBackend for MockBackend {
        fn render_with_clear(&mut self, paint_list: &PaintList, _clear_mode: ClearMode) {
            self.received_count = paint_list.commands.len();
        }
    }

    #[test]
    fn new_creates_empty_commands() {
        let list = PaintList::new();
        assert!(list.commands.is_empty());
    }

    #[test]
    fn default_creates_empty_commands() {
        let list = PaintList::default();
        assert!(list.commands.is_empty());
    }

    #[test]
    fn clear_empties_non_empty_list() {
        let mut list = PaintList::new();
        list.commands
            .push(PaintCommand::FillRect(Rect::ZERO, [255, 0, 0, 255]));
        assert_eq!(list.commands.len(), 1);
        list.clear();
        assert!(list.commands.is_empty());
    }

    #[test]
    fn commands_appear_in_order() {
        let mut list = PaintList::new();
        list.commands
            .push(PaintCommand::FillRect(Rect::ZERO, [255, 0, 0, 255]));
        list.commands
            .push(PaintCommand::StrokeRect(Rect::ZERO, 1.0, [0, 255, 0, 255]));
        list.commands.push(PaintCommand::DrawText(
            Point::ZERO,
            "hi".to_string(),
            12.0,
            [0, 0, 255, 255],
        ));
        assert_eq!(list.commands.len(), 3);
        assert!(matches!(list.commands[0], PaintCommand::FillRect(..)));
        assert!(matches!(list.commands[1], PaintCommand::StrokeRect(..)));
        assert!(matches!(list.commands[2], PaintCommand::DrawText(..)));
    }

    #[test]
    fn mock_backend_receives_commands() {
        let mut list = PaintList::new();
        list.commands
            .push(PaintCommand::FillRect(Rect::ZERO, [255, 0, 0, 255]));
        list.commands
            .push(PaintCommand::StrokeRect(Rect::ZERO, 1.0, [0, 255, 0, 255]));

        let mut backend = MockBackend::new();
        assert_eq!(backend.received_count, 0);
        backend.render(&list);
        assert_eq!(backend.received_count, 2);

        list.clear();
        backend.render(&list);
        assert_eq!(backend.received_count, 0);
    }
}
