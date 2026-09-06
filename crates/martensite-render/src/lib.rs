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
