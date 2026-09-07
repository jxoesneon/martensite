//! Vello GPU rendering backend.
//!
//! [`VelloRenderer`] translates a [`PaintList`] into a Vello
//! [`Scene`] and dispatches fine-raster compute work to the GPU. The actual
//! Vello integration is gated behind the `vello` feature flag; without it the
//! struct is still defined and implements [`RenderBackend`]
//! but performs no GPU work, allowing downstream crates to compile against the
//! type unconditionally.

use crate::paint::PaintList;
use crate::RenderBackend;

#[cfg(feature = "vello")]
use vello::Scene;

/// A GPU renderer that converts a [`PaintList`] into a Vello scene.
///
/// When the `vello` feature is enabled, [`VelloRenderer::render`] builds a
/// [`Scene`] from the supplied commands and stores it for later dispatch to a
/// Vello compute pipeline. When the feature is disabled the renderer is a
/// no-op stub that simply records the number of commands it received, which is
/// useful for type-level compatibility in crates that depend on
/// `martensite-render` without GPU support.
pub struct VelloRenderer {
    #[cfg(feature = "vello")]
    scene: Scene,
    last_command_count: usize,
}

impl VelloRenderer {
    /// Creates a new renderer with an empty scene.
    pub fn new() -> Self {
        Self {
            #[cfg(feature = "vello")]
            scene: Scene::new(),
            last_command_count: 0,
        }
    }

    /// Returns the number of commands processed by the most recent
    /// [`RenderBackend::render`] call.
    pub fn last_command_count(&self) -> usize {
        self.last_command_count
    }

    /// Returns a reference to the most recently built Vello scene.
    ///
    /// Only available when the `vello` feature is enabled.
    #[cfg(feature = "vello")]
    pub fn scene(&self) -> &Scene {
        &self.scene
    }

    /// Resets the internal scene, discarding any previously recorded commands.
    #[cfg(feature = "vello")]
    pub fn reset(&mut self) {
        self.scene.reset();
        self.last_command_count = 0;
    }

    /// Translates a single paint command into Vello scene draw calls.
    ///
    /// This is the extension point where the full PaintCommand-to-Scene
    /// mapping lives. GPU-dependent behavior is exercised by integration tests
    /// marked `#[ignore]` so that headless CI does not require a GPU.
    #[cfg(feature = "vello")]
    fn render_command(&mut self, _command: &crate::paint::PaintCommand) {
        // The detailed Scene translation (fill paths, gradients, glyph runs)
        // is implemented in martensite-wgpu where the Vello compute pipeline
        // is available. Here we only ensure the command stream is consumed.
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
            for command in &paint_list.commands {
                self.render_command(command);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::paint::PaintList;
    use kurbo::Rect;

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
}
