//! Intermediate PaintList stream and software rasterization fallback.
#![forbid(unsafe_code)]

use kurbo::{Point, Rect};

/// A single drawing operation emitted into a [`PaintList`].
pub enum PaintCommand {
    /// Fill a rectangle with a solid RGBA color.
    FillRect(Rect, [u8; 4]),
    /// Stroke the outline of a rectangle with the given line width and RGBA color.
    StrokeRect(Rect, f32, [u8; 4]),
    /// Draw a text string at the given position, font size, and RGBA color.
    DrawText(Point, String, f32, [u8; 4]),
}

/// An ordered list of [`PaintCommand`]s produced by the layout phase and
/// consumed by a [`RenderBackend`].
#[derive(Default)]
pub struct PaintList {
    /// The ordered sequence of paint commands to render.
    pub commands: Vec<PaintCommand>,
}

impl PaintList {
    /// Creates a new, empty `PaintList`.
    pub fn new() -> Self {
        Self {
            commands: Vec::new(),
        }
    }

    /// Removes all commands from the list, leaving it empty.
    pub fn clear(&mut self) {
        self.commands.clear();
    }
}

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
