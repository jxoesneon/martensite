//! Taffy layout engine bridge for Martensite.
#![forbid(unsafe_code)]

/// Re-exports the Taffy prelude, providing common layout types and helpers.
pub use taffy::prelude::*;
/// Re-exports the [`TaffyTree`] layout tree implementation from Taffy.
pub use taffy::TaffyTree;

/// A wrapper around the Taffy layout engine that manages a layout tree.
///
/// This struct owns a [`TaffyTree`] and exposes convenience methods for
/// building and computing layouts within the Martensite framework.
#[derive(Default)]
pub struct LayoutEngine {
    /// The underlying Taffy layout tree that stores nodes and their styles.
    pub tree: TaffyTree,
}

impl LayoutEngine {
    /// Creates a new [`LayoutEngine`] with an empty layout tree.
    pub fn new() -> Self {
        Self {
            tree: TaffyTree::new(),
        }
    }

    /// Computes the layout for the subtree rooted at `root` within the given
    /// `available_space`.
    ///
    /// Any layout errors returned by the underlying Taffy engine are silently
    /// discarded.
    pub fn compute_layout(
        &mut self,
        root: taffy::NodeId,
        available_space: taffy::Size<taffy::AvailableSpace>,
    ) {
        self.tree.compute_layout(root, available_space).ok();
    }
}
