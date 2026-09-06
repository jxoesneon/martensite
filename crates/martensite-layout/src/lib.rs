//! Taffy layout engine bridge for Martensite.
#![forbid(unsafe_code)]

pub use taffy::prelude::*;
pub use taffy::TaffyTree;

#[derive(Default)]
pub struct LayoutEngine {
    pub tree: TaffyTree,
}

impl LayoutEngine {
    pub fn new() -> Self {
        Self {
            tree: TaffyTree::new(),
        }
    }

    pub fn compute_layout(
        &mut self,
        root: taffy::NodeId,
        available_space: taffy::Size<taffy::AvailableSpace>,
    ) {
        self.tree.compute_layout(root, available_space).ok();
    }
}
