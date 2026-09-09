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

/// Abstraction over the concrete rendering target that consumes a [`PaintList`].
pub trait RenderBackend: Send + 'static {
    /// Renders the given [`PaintList`] to this backend's output surface.
    fn render(&mut self, paint_list: &PaintList);
}

#[cfg(test)]
mod tests {
    use super::{PaintCommand, PaintList, RenderBackend};
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
        fn render(&mut self, paint_list: &PaintList) {
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
