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

#[cfg(test)]
mod tests {
    use super::{AvailableSpace, LayoutEngine, Size, Style};

    #[test]
    fn new_creates_empty_tree() {
        let mut engine = LayoutEngine::new();
        // A fresh engine has a tree that can accept new leaf nodes.
        let _ = engine
            .tree
            .new_leaf(Style::default())
            .expect("create leaf in empty tree");
    }

    #[test]
    fn default_matches_new() {
        let mut new_engine = LayoutEngine::new();
        let mut default_engine = LayoutEngine::default();
        // Both start empty; the first node created in each should have the same id.
        let new_node = new_engine
            .tree
            .new_leaf(Style::default())
            .expect("create leaf");
        let default_node = default_engine
            .tree
            .new_leaf(Style::default())
            .expect("create leaf");
        assert_eq!(new_node, default_node);
    }

    #[test]
    fn can_add_nodes_and_compute_layout() {
        let mut engine = LayoutEngine::new();
        let child = engine.tree.new_leaf(Style::default()).expect("create leaf");
        let root = engine
            .tree
            .new_with_children(Style::default(), &[child])
            .expect("create root");
        engine.compute_layout(
            root,
            Size {
                width: AvailableSpace::MaxContent,
                height: AvailableSpace::MaxContent,
            },
        );
        let layout = engine.tree.layout(root).expect("get layout");
        assert_eq!(layout.order, 0);
    }
}
