//! Binary space partitioning (BSP) docking tree.
//!
//! The docking tree stores an arrangement of docked panels as a binary tree of
//! splits and leaves inside a pre-allocated [`slab::Slab`] arena. Splits and
//! merges that stay within the arena capacity perform no heap allocation, which
//! makes the structure suitable for interactive drag-and-drop docking where the
//! layout changes every frame the user drags a panel.
//!
//! Rectangle computation ([`DockTree::panel_rects`]) uses an explicit
//! stack-based iterative traversal so that deeply nested trees never overflow
//! the call stack.

use slab::Slab;

// ---------------------------------------------------------------------------
// Rect
// ---------------------------------------------------------------------------

/// An axis-aligned rectangle in surface-local coordinates.
///
/// All values are in device-independent pixels. The rectangle is stored as a
/// top-left corner plus a width and height; `x`/`y` are inclusive while the
/// bottom and right edges are exclusive.
///
/// # Examples
///
/// ```
/// use martensite_blessed::Rect;
/// let r = Rect::new(0.0, 0.0, 100.0, 50.0);
/// assert_eq!(r.width, 100.0);
/// assert!(r.contains(50.0, 25.0));
/// assert!(!r.contains(100.0, 25.0));
/// ```
#[derive(Clone, Copy, Debug, PartialEq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct Rect {
    /// Left edge x coordinate.
    pub x: f64,
    /// Top edge y coordinate.
    pub y: f64,
    /// Width of the rectangle.
    pub width: f64,
    /// Height of the rectangle.
    pub height: f64,
}

impl Rect {
    /// Creates a rectangle from a top-left corner and a size.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite_blessed::Rect;
    /// let r = Rect::new(1.0, 2.0, 3.0, 4.0);
    /// assert_eq!((r.x, r.y, r.width, r.height), (1.0, 2.0, 3.0, 4.0));
    /// ```
    pub fn new(x: f64, y: f64, width: f64, height: f64) -> Self {
        Self {
            x,
            y,
            width,
            height,
        }
    }

    /// Splits this rectangle with a horizontal divider into a top and bottom
    /// pair.
    ///
    /// The first returned rectangle is the top region with height
    /// `self.height * ratio`; the second is the remaining bottom region. A
    /// horizontal divider therefore stacks the two halves vertically.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite_blessed::Rect;
    /// let r = Rect::new(0.0, 0.0, 100.0, 100.0);
    /// let (top, bottom) = r.split_horizontal(0.25);
    /// assert_eq!(top.height, 25.0);
    /// assert_eq!(bottom.y, 25.0);
    /// assert_eq!(bottom.height, 75.0);
    /// ```
    pub fn split_horizontal(self, ratio: f64) -> (Rect, Rect) {
        let top_height = self.height * ratio;
        let bottom_height = self.height - top_height;
        let top = Rect::new(self.x, self.y, self.width, top_height);
        let bottom = Rect::new(self.x, self.y + top_height, self.width, bottom_height);
        (top, bottom)
    }

    /// Splits this rectangle with a vertical divider into a left and right
    /// pair.
    ///
    /// The first returned rectangle is the left region with width
    /// `self.width * ratio`; the second is the remaining right region. A
    /// vertical divider therefore places the two halves side by side.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite_blessed::Rect;
    /// let r = Rect::new(0.0, 0.0, 100.0, 100.0);
    /// let (left, right) = r.split_vertical(0.3);
    /// assert_eq!(left.width, 30.0);
    /// assert_eq!(right.x, 30.0);
    /// assert_eq!(right.width, 70.0);
    /// ```
    pub fn split_vertical(self, ratio: f64) -> (Rect, Rect) {
        let left_width = self.width * ratio;
        let right_width = self.width - left_width;
        let left = Rect::new(self.x, self.y, left_width, self.height);
        let right = Rect::new(self.x + left_width, self.y, right_width, self.height);
        (left, right)
    }

    /// Returns `true` when the point lies inside this rectangle.
    ///
    /// The left and top edges are inclusive; the right and bottom edges are
    /// exclusive. Points containing a NaN coordinate never lie inside.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite_blessed::Rect;
    /// let r = Rect::new(10.0, 10.0, 20.0, 20.0);
    /// assert!(r.contains(10.0, 10.0));
    /// assert!(r.contains(29.9, 29.9));
    /// assert!(!r.contains(30.0, 10.0));
    /// assert!(!r.contains(10.0, 30.0));
    /// ```
    pub fn contains(self, x: f64, y: f64) -> bool {
        x >= self.x && y >= self.y && x < self.x + self.width && y < self.y + self.height
    }
}

// ---------------------------------------------------------------------------
// NodeId
// ---------------------------------------------------------------------------

/// A handle addressing a single node inside the [`DockTree`] arena.
///
/// `NodeId` is a stable index into the slab as long as the node it addresses
/// has not been removed. Removing a node frees its slot; a subsequently inserted
/// node may reuse the same index.
///
/// # Examples
///
/// ```
/// use martensite_blessed::{DockPanel, DockTree, NodeId};
/// let mut tree = DockTree::new();
/// let id: NodeId = tree.insert_root(DockPanel::new(1, "Editor"));
/// assert!(tree.node(id).is_some());
/// ```
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct NodeId(pub u32);

// ---------------------------------------------------------------------------
// SplitDirection
// ---------------------------------------------------------------------------

/// The orientation of a split divider between two docking regions.
///
/// `Horizontal` describes a horizontal divider line that stacks the two
/// children vertically (top/bottom). `Vertical` describes a vertical divider
/// line that places the two children side by side (left/right).
///
/// # Examples
///
/// ```
/// use martensite_blessed::{DockPanel, DockTree, SplitDirection};
/// let mut tree = DockTree::new();
/// let root = tree.insert_root(DockPanel::new(0, "A"));
/// let (left, right) = tree
///     .split_leaf(root, SplitDirection::Vertical, 0.5, DockPanel::new(1, "B"))
///     .unwrap();
/// assert_ne!(left, right);
/// ```
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub enum SplitDirection {
    /// A horizontal divider producing top/bottom children.
    Horizontal,
    /// A vertical divider producing left/right children.
    Vertical,
}

// ---------------------------------------------------------------------------
// DockPanel
// ---------------------------------------------------------------------------

/// A single docked panel: a widget plus optional metadata for presentation.
///
/// A panel is the payload stored in every leaf of the docking tree. The
/// `surface_handle` optionally identifies a swapchain surface that the panel
/// should be composited from, allowing video or 3D content to be docked
/// alongside regular widgets.
///
/// # Examples
///
/// ```
/// use martensite_blessed::DockPanel;
/// let panel = DockPanel::new(42, "Console").with_surface(7);
/// assert_eq!(panel.widget_id(), 42);
/// assert_eq!(panel.title(), "Console");
/// assert_eq!(panel.surface_handle(), Some(7));
/// ```
#[derive(Clone, Debug, PartialEq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct DockPanel {
    /// Widget id rendered inside this panel.
    pub widget_id: u64,
    /// Title shown in the panel's tab or title bar.
    pub title: String,
    /// Optional swapchain surface handle to composite, if any.
    pub surface_handle: Option<u64>,
}

impl DockPanel {
    /// Creates a new panel with no attached surface.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite_blessed::DockPanel;
    /// let panel = DockPanel::new(7, "Outline");
    /// assert_eq!(panel.widget_id(), 7);
    /// assert_eq!(panel.surface_handle(), None);
    /// ```
    pub fn new(widget_id: u64, title: impl Into<String>) -> Self {
        Self {
            widget_id,
            title: title.into(),
            surface_handle: None,
        }
    }

    /// Attaches a swapchain surface handle and returns the updated panel.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite_blessed::DockPanel;
    /// let panel = DockPanel::new(3, "Preview").with_surface(99);
    /// assert_eq!(panel.surface_handle(), Some(99));
    /// ```
    pub fn with_surface(mut self, surface_handle: u64) -> Self {
        self.surface_handle = Some(surface_handle);
        self
    }

    /// Returns the widget id rendered inside this panel.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite_blessed::DockPanel;
    /// assert_eq!(DockPanel::new(11, "x").widget_id(), 11);
    /// ```
    pub fn widget_id(&self) -> u64 {
        self.widget_id
    }

    /// Returns the panel title.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite_blessed::DockPanel;
    /// assert_eq!(DockPanel::new(1, "Files").title(), "Files");
    /// ```
    pub fn title(&self) -> &str {
        &self.title
    }

    /// Returns the optional swapchain surface handle.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite_blessed::DockPanel;
    /// assert_eq!(DockPanel::new(1, "x").surface_handle(), None);
    /// assert_eq!(DockPanel::new(1, "x").with_surface(5).surface_handle(), Some(5));
    /// ```
    pub fn surface_handle(&self) -> Option<u64> {
        self.surface_handle
    }
}

// ---------------------------------------------------------------------------
// DockNode
// ---------------------------------------------------------------------------

/// A node in the docking tree.
///
/// Every node is either an interior [`Split`](DockNode::Split) with two
/// children or a [`Leaf`](DockNode::Leaf) holding a single [`DockPanel`].
///
/// # Examples
///
/// ```
/// use martensite_blessed::{DockNode, DockPanel};
/// let leaf = DockNode::Leaf { panel: DockPanel::new(1, "A") };
/// assert!(matches!(leaf, DockNode::Leaf { .. }));
/// ```
#[derive(Clone, Debug, PartialEq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub enum DockNode {
    /// An interior node splitting its rectangle between two children.
    Split {
        /// Orientation of the divider.
        direction: SplitDirection,
        /// Fraction of the rectangle assigned to the first (`left`) child.
        ratio: f64,
        /// First child (top for `Horizontal`, left for `Vertical`).
        left: NodeId,
        /// Second child (bottom for `Horizontal`, right for `Vertical`).
        right: NodeId,
    },
    /// A terminal node holding one docked panel.
    Leaf {
        /// The panel docked into this leaf.
        panel: DockPanel,
    },
}

impl DockNode {
    /// Returns `true` when this node is a [`DockNode::Leaf`].
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite_blessed::{DockNode, DockPanel};
    /// let leaf = DockNode::Leaf { panel: DockPanel::new(1, "A") };
    /// assert!(leaf.is_leaf());
    /// ```
    pub fn is_leaf(&self) -> bool {
        matches!(self, DockNode::Leaf { .. })
    }
}

// ---------------------------------------------------------------------------
// DockError
// ---------------------------------------------------------------------------

/// Errors returned by docking tree operations.
///
/// # Examples
///
/// ```
/// use martensite_blessed::{DockError, DockPanel, DockTree, SplitDirection};
/// let mut tree = DockTree::new();
/// let root = tree.insert_root(DockPanel::new(0, "A"));
/// // Cannot split with an out-of-range ratio.
/// assert_eq!(
///     tree.split_leaf(root, SplitDirection::Vertical, 1.5, DockPanel::new(1, "B")),
///     Err(DockError::InvalidRatio(1.5))
/// );
/// ```
#[derive(Clone, Debug, PartialEq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub enum DockError {
    /// The referenced node id is not present in the tree.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite_blessed::{DockError, DockTree, NodeId};
    /// let tree = DockTree::new();
    /// assert_eq!(tree.node(NodeId(999)), None);
    /// assert_eq!(DockError::NodeNotFound, DockError::NodeNotFound);
    /// ```
    NodeNotFound,
    /// A merge was attempted on a leaf node, which has no children to collapse.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite_blessed::{DockError, DockPanel, DockTree};
    /// let mut tree = DockTree::new();
    /// let root = tree.insert_root(DockPanel::new(0, "A"));
    /// assert_eq!(tree.merge(root), Err(DockError::CannotMergeLeaf));
    /// ```
    CannotMergeLeaf,
    /// A split was attempted on a node that is already an interior split.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite_blessed::{
    ///     DockError, DockPanel, DockTree, NodeId, SplitDirection,
    /// };
    /// let mut tree = DockTree::new();
    /// let root = tree.insert_root(DockPanel::new(0, "A"));
    /// let (left, _) = tree
    ///     .split_leaf(root, SplitDirection::Vertical, 0.5, DockPanel::new(1, "B"))
    ///     .unwrap();
    /// // Splitting the root again fails because it is now a split, not a leaf.
    /// assert_eq!(
    ///     tree.split_leaf(root, SplitDirection::Vertical, 0.5, DockPanel::new(2, "C")),
    ///     Err(DockError::CannotSplitNonLeaf)
    /// );
    /// let _ = left;
    /// ```
    CannotSplitNonLeaf,
    /// A split ratio outside the open interval `(0.0, 1.0)` was supplied.
    ///
    /// The wrapped value is the offending ratio.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite_blessed::DockError;
    /// assert_eq!(DockError::InvalidRatio(0.0), DockError::InvalidRatio(0.0));
    /// ```
    InvalidRatio(f64),
    /// The tree has no nodes and therefore no root to operate on.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite_blessed::{DockError, DockTree};
    /// let tree = DockTree::new();
    /// assert_eq!(tree.root(), None);
    /// assert_eq!(DockError::TreeEmpty, DockError::TreeEmpty);
    /// ```
    TreeEmpty,
}

// ---------------------------------------------------------------------------
// DockDropZone
// ---------------------------------------------------------------------------

/// A drop target zone used during drag-and-drop docking.
///
/// While a panel is dragged, the hit-tested node and one of these zones
/// determine where the panel will be inserted when the drag is released.
///
/// # Examples
///
/// ```
/// use martensite_blessed::DockDropZone;
/// let zones = [
///     DockDropZone::Center,
///     DockDropZone::Left,
///     DockDropZone::Right,
///     DockDropZone::Top,
///     DockDropZone::Bottom,
///     DockDropZone::Tab,
/// ];
/// assert_eq!(zones.len(), 6);
/// ```
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub enum DockDropZone {
    /// Drop into the center, replacing or tabbing with the target panel.
    Center,
    /// Drop to the left of the target, creating a vertical split.
    Left,
    /// Drop to the right of the target, creating a vertical split.
    Right,
    /// Drop above the target, creating a horizontal split.
    Top,
    /// Drop below the target, creating a horizontal split.
    Bottom,
    /// Drop as a new tab in the target panel.
    Tab,
}

// ---------------------------------------------------------------------------
// DockTree
// ---------------------------------------------------------------------------

/// Default pre-allocated arena capacity for a new [`DockTree`].
const DEFAULT_CAPACITY: usize = 64;

/// Maximum traversal depth supported by the stack-based rectangle iterator.
///
/// A binary docking tree of depth 64 can address up to `2^64` leaves, which is
/// far beyond any practical layout; the fixed stack therefore never overflows
/// for real trees while keeping [`DockTree::panel_rects`] allocation-free.
const RECT_STACK_DEPTH: usize = 64;

/// A binary space partitioning docking tree backed by a slab arena.
///
/// The tree owns all of its nodes in a single pre-allocated [`slab::Slab`].
/// Splits and merges that keep the live node count within the slab capacity do
/// not allocate, making interactive docking cheap.
///
/// # Examples
///
/// ```
/// use martensite_blessed::{DockPanel, DockTree, Rect, SplitDirection};
/// let mut tree = DockTree::new();
/// let root = tree.insert_root(DockPanel::new(0, "Editor"));
/// let (left, right) = tree
///     .split_leaf(root, SplitDirection::Vertical, 0.5, DockPanel::new(1, "Outline"))
///     .unwrap();
/// assert_eq!(tree.panel_count(), 2);
/// let rects: Vec<_> = tree.panel_rects(Rect::new(0.0, 0.0, 100.0, 100.0)).collect();
/// assert_eq!(rects.len(), 2);
/// let _ = (left, right);
/// ```
#[derive(Clone, Debug)]
pub struct DockTree {
    /// Arena storing every node.
    nodes: Slab<DockNode>,
    /// Root node id, or `None` when the tree is empty.
    root: Option<NodeId>,
    /// Pre-allocated capacity of the arena.
    capacity: usize,
}

impl Default for DockTree {
    fn default() -> Self {
        Self::new()
    }
}

impl DockTree {
    /// Creates an empty docking tree with the default capacity.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite_blessed::DockTree;
    /// let tree = DockTree::new();
    /// assert_eq!(tree.node_count(), 0);
    /// assert_eq!(tree.root(), None);
    /// ```
    pub fn new() -> Self {
        Self::with_capacity(DEFAULT_CAPACITY)
    }

    /// Creates an empty docking tree whose arena is pre-allocated to `capacity`.
    ///
    /// Operations that keep the live node count at or below `capacity` perform
    /// no heap allocation.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite_blessed::DockTree;
    /// let tree = DockTree::with_capacity(128);
    /// assert_eq!(tree.node_count(), 0);
    /// ```
    pub fn with_capacity(capacity: usize) -> Self {
        Self {
            nodes: Slab::with_capacity(capacity),
            root: None,
            capacity,
        }
    }

    /// Returns the pre-allocated arena capacity.
    ///
    /// Operations that keep the live node count at or below this value perform
    /// no heap allocation.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite_blessed::DockTree;
    /// let tree = DockTree::with_capacity(128);
    /// assert_eq!(tree.capacity(), 128);
    /// ```
    pub fn capacity(&self) -> usize {
        self.capacity
    }

    /// Returns the root node id, or `None` when the tree is empty.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite_blessed::{DockPanel, DockTree};
    /// let mut tree = DockTree::new();
    /// assert_eq!(tree.root(), None);
    /// let root = tree.insert_root(DockPanel::new(0, "A"));
    /// assert_eq!(tree.root(), Some(root));
    /// ```
    pub fn root(&self) -> Option<NodeId> {
        self.root
    }

    /// Inserts the first panel as the root leaf of an empty tree.
    ///
    /// # Panics
    ///
    /// Panics if the tree already has a root. Callers are expected to check
    /// [`DockTree::root`] first or build a fresh tree.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite_blessed::{DockPanel, DockTree};
    /// let mut tree = DockTree::new();
    /// let root = tree.insert_root(DockPanel::new(1, "Editor"));
    /// assert_eq!(tree.panel_count(), 1);
    /// assert_eq!(tree.root(), Some(root));
    /// ```
    pub fn insert_root(&mut self, panel: DockPanel) -> NodeId {
        assert!(
            self.root.is_none(),
            "DockTree::insert_root: tree already has a root"
        );
        let id = self.nodes.insert(DockNode::Leaf { panel });
        let id = NodeId(id as u32);
        self.root = Some(id);
        id
    }

    /// Transforms a leaf node into a split with two leaf children.
    ///
    /// The original leaf's panel becomes the first (`left`) child and
    /// `new_panel` becomes the second (`right`) child. When the slab has spare
    /// capacity this operation performs no heap allocation.
    ///
    /// Returns the `(left_child_id, right_child_id)` pair of the newly created
    /// leaves.
    ///
    /// # Errors
    ///
    /// Returns [`DockError::NodeNotFound`] if `leaf_id` is not present,
    /// [`DockError::CannotSplitNonLeaf`] if the node is already a split, or
    /// [`DockError::InvalidRatio`] if `ratio` is not in the open interval
    /// `(0.0, 1.0)`.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite_blessed::{DockPanel, DockTree, SplitDirection};
    /// let mut tree = DockTree::new();
    /// let root = tree.insert_root(DockPanel::new(0, "A"));
    /// let (left, right) = tree
    ///     .split_leaf(root, SplitDirection::Horizontal, 0.5, DockPanel::new(1, "B"))
    ///     .unwrap();
    /// assert_eq!(tree.panel_count(), 2);
    /// assert!(tree.node(left).unwrap().is_leaf());
    /// assert!(tree.node(right).unwrap().is_leaf());
    /// ```
    pub fn split_leaf(
        &mut self,
        leaf_id: NodeId,
        direction: SplitDirection,
        ratio: f64,
        new_panel: DockPanel,
    ) -> Result<(NodeId, NodeId), DockError> {
        if !ratio.is_finite() || ratio <= 0.0 || ratio >= 1.0 {
            return Err(DockError::InvalidRatio(ratio));
        }
        let idx = leaf_id.0 as usize;
        if !self.nodes.contains(idx) {
            return Err(DockError::NodeNotFound);
        }

        // Extract the existing panel by replacing the leaf with a placeholder
        // split. The placeholder ids are overwritten immediately after the two
        // children are inserted, so the tree is never observed in this state.
        let original_panel = match std::mem::replace(
            &mut self.nodes[idx],
            DockNode::Split {
                direction,
                ratio,
                left: NodeId(u32::MAX),
                right: NodeId(u32::MAX),
            },
        ) {
            DockNode::Leaf { panel } => panel,
            DockNode::Split { .. } => return Err(DockError::CannotSplitNonLeaf),
        };

        let left_idx = self.nodes.insert(DockNode::Leaf {
            panel: original_panel,
        });
        let right_idx = self.nodes.insert(DockNode::Leaf { panel: new_panel });

        self.nodes[idx] = DockNode::Split {
            direction,
            ratio,
            left: NodeId(left_idx as u32),
            right: NodeId(right_idx as u32),
        };

        Ok((NodeId(left_idx as u32), NodeId(right_idx as u32)))
    }

    /// Collapses a split node back into a single node.
    ///
    /// The left child is promoted into the split node's slot and the right
    /// subtree is freed. When the left child is a leaf the node becomes a leaf
    /// again. Both child slots are freed in the slab. This operation performs
    /// no heap allocation.
    ///
    /// # Errors
    ///
    /// Returns [`DockError::NodeNotFound`] if `node_id` is not present, or
    /// [`DockError::CannotMergeLeaf`] if the node is already a leaf.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite_blessed::{DockPanel, DockTree, SplitDirection};
    /// let mut tree = DockTree::new();
    /// let root = tree.insert_root(DockPanel::new(0, "A"));
    /// tree.split_leaf(root, SplitDirection::Vertical, 0.5, DockPanel::new(1, "B"))
    ///     .unwrap();
    /// assert_eq!(tree.panel_count(), 2);
    /// tree.merge(root).unwrap();
    /// assert_eq!(tree.panel_count(), 1);
    /// assert!(tree.node(root).unwrap().is_leaf());
    /// ```
    pub fn merge(&mut self, node_id: NodeId) -> Result<(), DockError> {
        let idx = node_id.0 as usize;
        let (left, right) = match self.nodes.get(idx) {
            Some(DockNode::Split { left, right, .. }) => (*left, *right),
            Some(DockNode::Leaf { .. }) => return Err(DockError::CannotMergeLeaf),
            None => return Err(DockError::NodeNotFound),
        };

        // Move the left child's node out of its slot (freeing the slot) and
        // free the entire right subtree. The left node is then written into the
        // merged slot, preserving any subtree it may own.
        let left_node = self.nodes.remove(left.0 as usize);
        self.remove_subtree(right);

        self.nodes[idx] = left_node;
        Ok(())
    }

    /// Removes a node and rebalances the tree.
    ///
    /// If the node is a leaf child of a split, the split is collapsed: the
    /// surviving sibling is promoted into the parent's slot and both the
    /// removed node and the sibling's old slot are freed. Removing the root
    /// clears the entire tree. Removing a node that is not present does
    /// nothing.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite_blessed::{DockPanel, DockTree, SplitDirection};
    /// let mut tree = DockTree::new();
    /// let root = tree.insert_root(DockPanel::new(0, "A"));
    /// let (left, right) = tree
    ///     .split_leaf(root, SplitDirection::Vertical, 0.5, DockPanel::new(1, "B"))
    ///     .unwrap();
    /// tree.remove(right);
    /// // The split collapsed; the root is a leaf again holding panel A.
    /// assert_eq!(tree.panel_count(), 1);
    /// assert!(tree.node(root).unwrap().is_leaf());
    /// let _ = left;
    /// ```
    pub fn remove(&mut self, node_id: NodeId) {
        if let Some(parent_id) = self.find_parent(node_id) {
            let parent_idx = parent_id.0 as usize;
            let (left, right) = match self.nodes[parent_idx] {
                DockNode::Split { left, right, .. } => (left, right),
                DockNode::Leaf { .. } => return,
            };
            let sibling = if left == node_id { right } else { left };
            // Move the sibling up into the parent slot and free the removed
            // node's subtree plus the sibling's now-vacant slot.
            let sibling_node = self.nodes.remove(sibling.0 as usize);
            self.remove_subtree(node_id);
            self.nodes[parent_idx] = sibling_node;
            return;
        }

        // No parent: the node is the root (or absent).
        if self.root == Some(node_id) {
            self.remove_subtree(node_id);
            self.root = None;
        }
    }

    /// Recursively frees the subtree rooted at `id` from the slab.
    fn remove_subtree(&mut self, id: NodeId) {
        let children = match self.nodes.get(id.0 as usize) {
            Some(DockNode::Split { left, right, .. }) => Some((*left, *right)),
            _ => None,
        };
        if let Some((left, right)) = children {
            self.remove_subtree(left);
            self.remove_subtree(right);
        }
        if self.nodes.contains(id.0 as usize) {
            self.nodes.remove(id.0 as usize);
        }
    }

    /// Returns the id of the parent of `target`, or `None` if it is the root or
    /// absent.
    fn find_parent(&self, target: NodeId) -> Option<NodeId> {
        let root = self.root?;
        self.find_parent_in(root, target)
    }

    fn find_parent_in(&self, current: NodeId, target: NodeId) -> Option<NodeId> {
        match self.nodes.get(current.0 as usize) {
            Some(DockNode::Split { left, right, .. }) => {
                if *left == target || *right == target {
                    Some(current)
                } else {
                    self.find_parent_in(*left, target)
                        .or_else(|| self.find_parent_in(*right, target))
                }
            }
            Some(DockNode::Leaf { .. }) => None,
            None => None,
        }
    }

    /// Returns a shared reference to the node at `id`, or `None` if absent.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite_blessed::{DockPanel, DockTree};
    /// let mut tree = DockTree::new();
    /// let root = tree.insert_root(DockPanel::new(0, "A"));
    /// assert!(tree.node(root).is_some());
    /// assert!(tree.node(martensite_blessed::NodeId(123)).is_none());
    /// ```
    pub fn node(&self, id: NodeId) -> Option<&DockNode> {
        self.nodes.get(id.0 as usize)
    }

    /// Returns a mutable reference to the node at `id`, or `None` if absent.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite_blessed::{DockNode, DockPanel, DockTree};
    /// let mut tree = DockTree::new();
    /// let root = tree.insert_root(DockPanel::new(0, "A"));
    /// if let Some(DockNode::Leaf { panel }) = tree.node_mut(root) {
    ///     panel.title = "Changed".to_string();
    /// }
    /// match tree.node(root).unwrap() {
    ///     DockNode::Leaf { panel } => assert_eq!(panel.title(), "Changed"),
    ///     _ => panic!("expected leaf"),
    /// }
    /// ```
    pub fn node_mut(&mut self, id: NodeId) -> Option<&mut DockNode> {
        self.nodes.get_mut(id.0 as usize)
    }

    /// Iterates over every leaf panel together with its node id.
    ///
    /// The order is slab insertion order, not tree order. The iterator
    /// allocates no intermediate collections.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite_blessed::{DockPanel, DockTree, SplitDirection};
    /// let mut tree = DockTree::new();
    /// let root = tree.insert_root(DockPanel::new(0, "A"));
    /// tree.split_leaf(root, SplitDirection::Vertical, 0.5, DockPanel::new(1, "B"))
    ///     .unwrap();
    /// let panels: Vec<_> = tree.panels().map(|(_, p)| p.widget_id()).collect();
    /// assert_eq!(panels, vec![0, 1]);
    /// ```
    pub fn panels(&self) -> impl Iterator<Item = (NodeId, &DockPanel)> {
        self.nodes.iter().filter_map(|(idx, node)| match node {
            DockNode::Leaf { panel } => Some((NodeId(idx as u32), panel)),
            DockNode::Split { .. } => None,
        })
    }

    /// Iterates over every leaf panel together with its computed rectangle.
    ///
    /// The traversal is stack-based and iterative: it never recurses and never
    /// allocates. Each leaf's rectangle is obtained by recursively subdividing
    /// `root_rect` according to the splits along the path from the root.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite_blessed::{DockPanel, DockTree, Rect, SplitDirection};
    /// let mut tree = DockTree::new();
    /// let root = tree.insert_root(DockPanel::new(0, "A"));
    /// tree.split_leaf(root, SplitDirection::Vertical, 0.5, DockPanel::new(1, "B"))
    ///     .unwrap();
    /// let rects: Vec<_> = tree.panel_rects(Rect::new(0.0, 0.0, 100.0, 100.0)).collect();
    /// assert_eq!(rects.len(), 2);
    /// let total: f64 = rects.iter().map(|(_, r)| r.width).sum();
    /// assert_eq!(total, 100.0);
    /// ```
    pub fn panel_rects(&self, root_rect: Rect) -> impl Iterator<Item = (NodeId, Rect)> + '_ {
        PanelRectsIter {
            tree: self,
            root_rect,
            stack: [(NodeId(0), Rect::new(0.0, 0.0, 0.0, 0.0)); RECT_STACK_DEPTH],
            len: 0,
            started: false,
        }
    }

    /// Returns the total number of nodes (splits and leaves) in the tree.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite_blessed::{DockPanel, DockTree, SplitDirection};
    /// let mut tree = DockTree::new();
    /// let root = tree.insert_root(DockPanel::new(0, "A"));
    /// assert_eq!(tree.node_count(), 1);
    /// tree.split_leaf(root, SplitDirection::Vertical, 0.5, DockPanel::new(1, "B"))
    ///     .unwrap();
    /// assert_eq!(tree.node_count(), 3);
    /// ```
    pub fn node_count(&self) -> usize {
        self.nodes.len()
    }

    /// Returns the number of leaf panels in the tree.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite_blessed::{DockPanel, DockTree, SplitDirection};
    /// let mut tree = DockTree::new();
    /// let root = tree.insert_root(DockPanel::new(0, "A"));
    /// assert_eq!(tree.panel_count(), 1);
    /// tree.split_leaf(root, SplitDirection::Vertical, 0.5, DockPanel::new(1, "B"))
    ///     .unwrap();
    /// assert_eq!(tree.panel_count(), 2);
    /// ```
    pub fn panel_count(&self) -> usize {
        self.nodes.iter().filter(|(_, n)| n.is_leaf()).count()
    }

    // -- serialization -----------------------------------------------------

    /// Serializes the tree into a flat vector of [`DockNodeLayout`] records.
    ///
    /// The root is the record whose id is never referenced as a child.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite_blessed::{DockPanel, DockTree, SplitDirection};
    /// let mut tree = DockTree::new();
    /// let root = tree.insert_root(DockPanel::new(0, "A"));
    /// tree.split_leaf(root, SplitDirection::Vertical, 0.5, DockPanel::new(1, "B"))
    ///     .unwrap();
    /// let layout = tree.to_layout();
    /// assert_eq!(layout.len(), 3);
    /// ```
    pub fn to_layout(&self) -> Vec<DockNodeLayout> {
        let mut out = Vec::with_capacity(self.nodes.len());
        for (idx, node) in self.nodes.iter() {
            let id = idx as u32;
            let kind = match node {
                DockNode::Split {
                    direction,
                    ratio,
                    left,
                    right,
                } => DockNodeLayoutKind::Split {
                    direction: *direction,
                    ratio: *ratio,
                    left: left.0,
                    right: right.0,
                },
                DockNode::Leaf { panel } => DockNodeLayoutKind::Leaf {
                    panel: panel.clone(),
                },
            };
            out.push(DockNodeLayout { id, kind });
        }
        out
    }

    /// Reconstructs a [`DockTree`] from a flat vector of [`DockNodeLayout`]
    /// records.
    ///
    /// The root is detected as the record whose id is never referenced as a
    /// child. Node ids are remapped to fresh slab indices in the new tree, so
    /// the resulting tree is structurally equivalent but not index-identical.
    ///
    /// # Errors
    ///
    /// Returns [`DockError::TreeEmpty`] if `layout` is empty, or
    /// [`DockError::NodeNotFound`] if a referenced child id is missing.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite_blessed::{DockPanel, DockTree, SplitDirection};
    /// let mut tree = DockTree::new();
    /// let root = tree.insert_root(DockPanel::new(0, "A"));
    /// tree.split_leaf(root, SplitDirection::Vertical, 0.5, DockPanel::new(1, "B"))
    ///     .unwrap();
    /// let layout = tree.to_layout();
    /// let rebuilt = DockTree::from_layout(layout).unwrap();
    /// assert_eq!(rebuilt.panel_count(), 2);
    /// ```
    pub fn from_layout(layout: Vec<DockNodeLayout>) -> Result<Self, DockError> {
        if layout.is_empty() {
            return Err(DockError::TreeEmpty);
        }

        let root_id = layout
            .iter()
            .map(|n| n.id)
            .find(|id| {
                !layout.iter().any(|m| match &m.kind {
                    DockNodeLayoutKind::Split { left, right, .. } => left == id || right == id,
                    DockNodeLayoutKind::Leaf { .. } => false,
                })
            })
            .ok_or(DockError::TreeEmpty)?;

        let capacity = layout.len().max(DEFAULT_CAPACITY);
        let mut tree = DockTree::with_capacity(capacity);
        let root = tree.build_from_layout(&layout, root_id)?;
        tree.root = Some(root);
        Ok(tree)
    }

    /// Recursively inserts the subtree rooted at the layout record `id`.
    fn build_from_layout(
        &mut self,
        layout: &[DockNodeLayout],
        id: u32,
    ) -> Result<NodeId, DockError> {
        let entry = layout
            .iter()
            .find(|n| n.id == id)
            .ok_or(DockError::NodeNotFound)?;
        let new_id = match &entry.kind {
            DockNodeLayoutKind::Leaf { panel } => self.nodes.insert(DockNode::Leaf {
                panel: panel.clone(),
            }),
            DockNodeLayoutKind::Split {
                direction,
                ratio,
                left,
                right,
            } => {
                let left_id = self.build_from_layout(layout, *left)?;
                let right_id = self.build_from_layout(layout, *right)?;
                self.nodes.insert(DockNode::Split {
                    direction: *direction,
                    ratio: *ratio,
                    left: left_id,
                    right: right_id,
                })
            }
        };
        Ok(NodeId(new_id as u32))
    }
}

// ---------------------------------------------------------------------------
// PanelRectsIter
// ---------------------------------------------------------------------------

/// Allocation-free stack-based iterator yielding each leaf's rectangle.
struct PanelRectsIter<'a> {
    tree: &'a DockTree,
    root_rect: Rect,
    /// Fixed-capacity explicit traversal stack of `(node id, rectangle)`.
    stack: [(NodeId, Rect); RECT_STACK_DEPTH],
    /// Number of valid entries currently on the stack.
    len: usize,
    /// Whether the root has been seeded onto the stack yet.
    started: bool,
}

impl<'a> Iterator for PanelRectsIter<'a> {
    type Item = (NodeId, Rect);

    fn next(&mut self) -> Option<Self::Item> {
        if !self.started {
            self.started = true;
            if let Some(root) = self.tree.root {
                // Seed the stack with the root. A `None` root means the tree is
                // empty and the iterator yields nothing.
                self.push(root, self.root_rect);
            }
        }

        while let Some((id, rect)) = self.pop() {
            match self.tree.nodes.get(id.0 as usize) {
                Some(DockNode::Leaf { .. }) => return Some((id, rect)),
                Some(DockNode::Split {
                    direction,
                    ratio,
                    left,
                    right,
                }) => {
                    let (first, second) = match direction {
                        SplitDirection::Horizontal => rect.split_horizontal(*ratio),
                        SplitDirection::Vertical => rect.split_vertical(*ratio),
                    };
                    // Push right first so left is processed first (LIFO).
                    self.push(*right, second);
                    self.push(*left, first);
                }
                None => {
                    // Stale id; skip it.
                    continue;
                }
            }
        }
        None
    }
}

impl<'a> PanelRectsIter<'a> {
    /// Pushes an entry onto the fixed-capacity stack.
    ///
    /// If the stack is full the entry is dropped, which only happens for trees
    /// deeper than [`RECT_STACK_DEPTH`] — far beyond any practical layout.
    #[inline]
    fn push(&mut self, id: NodeId, rect: Rect) {
        if self.len < RECT_STACK_DEPTH {
            self.stack[self.len] = (id, rect);
            self.len += 1;
        }
    }

    /// Pops the top entry from the stack.
    #[inline]
    fn pop(&mut self) -> Option<(NodeId, Rect)> {
        if self.len == 0 {
            None
        } else {
            self.len -= 1;
            Some(self.stack[self.len])
        }
    }
}

// ---------------------------------------------------------------------------
// DockDragSession
// ---------------------------------------------------------------------------

/// Tracks a floating panel being dragged over the docking tree.
///
/// The session holds the panel that would be inserted on drop and the
/// currently hit-tested `(target node, drop zone)` pair, if any. It is purely
/// presentational state: it never mutates the [`DockTree`] itself.
///
/// # Examples
///
/// ```
/// use martensite_blessed::{
///     DockDragSession, DockDropZone, DockPanel, DockTree, NodeId, Rect,
/// };
/// let mut tree = DockTree::new();
/// let root = tree.insert_root(DockPanel::new(0, "A"));
/// let mut session = DockDragSession::new(DockPanel::new(1, "B"));
/// session.set_target(root, DockDropZone::Left);
/// let rect = Rect::new(0.0, 0.0, 100.0, 100.0);
/// let preview = session.preview_rect(rect, DockDropZone::Left);
/// assert!(preview.width < 100.0);
/// ```
#[derive(Clone, Debug)]
pub struct DockDragSession {
    /// The floating panel awaiting a drop target.
    pub floating_panel: DockPanel,
    /// The currently hit-tested target node and drop zone, if any.
    pub target_zone: Option<(NodeId, DockDropZone)>,
}

impl DockDragSession {
    /// Creates a new drag session for the given floating panel with no target.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite_blessed::{DockDragSession, DockPanel};
    /// let session = DockDragSession::new(DockPanel::new(1, "Floating"));
    /// assert!(session.target_zone.is_none());
    /// ```
    pub fn new(floating_panel: DockPanel) -> Self {
        Self {
            floating_panel,
            target_zone: None,
        }
    }

    /// Sets the current drop target node and zone.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite_blessed::{
    ///     DockDragSession, DockDropZone, DockPanel, NodeId,
    /// };
    /// let mut session = DockDragSession::new(DockPanel::new(1, "B"));
    /// session.set_target(NodeId(0), DockDropZone::Right);
    /// assert_eq!(session.target_zone, Some((NodeId(0), DockDropZone::Right)));
    /// ```
    pub fn set_target(&mut self, node: NodeId, zone: DockDropZone) {
        self.target_zone = Some((node, zone));
    }

    /// Clears the current drop target.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite_blessed::{DockDragSession, DockDropZone, DockPanel, NodeId};
    /// let mut session = DockDragSession::new(DockPanel::new(1, "B"));
    /// session.set_target(NodeId(0), DockDropZone::Center);
    /// session.clear_target();
    /// assert!(session.target_zone.is_none());
    /// ```
    pub fn clear_target(&mut self) {
        self.target_zone = None;
    }

    /// Computes the rectangle where the floating panel would land if dropped
    /// onto `target_node_rect` using `zone`.
    ///
    /// For directional zones the panel occupies the corresponding half of the
    /// target rectangle (split at the conventional preview ratio of `0.5`).
    /// For [`DockDropZone::Center`] and [`DockDropZone::Tab`] the panel fills
    /// the entire target rectangle.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite_blessed::{DockDragSession, DockDropZone, DockPanel, Rect};
    /// let session = DockDragSession::new(DockPanel::new(1, "B"));
    /// let target = Rect::new(0.0, 0.0, 100.0, 100.0);
    /// let left = session.preview_rect(target, DockDropZone::Left);
    /// assert_eq!(left.width, 50.0);
    /// let center = session.preview_rect(target, DockDropZone::Center);
    /// assert_eq!(center, target);
    /// ```
    pub fn preview_rect(&self, target_node_rect: Rect, zone: DockDropZone) -> Rect {
        const PREVIEW_RATIO: f64 = 0.5;
        match zone {
            DockDropZone::Left => target_node_rect.split_vertical(PREVIEW_RATIO).0,
            DockDropZone::Right => target_node_rect.split_vertical(PREVIEW_RATIO).1,
            DockDropZone::Top => target_node_rect.split_horizontal(PREVIEW_RATIO).0,
            DockDropZone::Bottom => target_node_rect.split_horizontal(PREVIEW_RATIO).1,
            DockDropZone::Center | DockDropZone::Tab => target_node_rect,
        }
    }
}

// ---------------------------------------------------------------------------
// DockNodeLayout
// ---------------------------------------------------------------------------

/// Serializable snapshot of a single docking tree node.
///
/// A vector of these records fully describes a [`DockTree`] and can be
/// round-tripped through [`DockTree::to_layout`] / [`DockTree::from_layout`].
/// When the `serde` feature is enabled the record derives `serde::Serialize`
/// and `serde::Deserialize`.
///
/// # Examples
///
/// ```
/// use martensite_blessed::{DockNodeLayout, DockNodeLayoutKind, DockPanel};
/// let record = DockNodeLayout {
///     id: 0,
///     kind: DockNodeLayoutKind::Leaf { panel: DockPanel::new(1, "A") },
/// };
/// assert_eq!(record.id, 0);
/// ```
#[derive(Clone, Debug, PartialEq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct DockNodeLayout {
    /// Identifier of this node within the snapshot.
    pub id: u32,
    /// The node payload: a split or a leaf.
    pub kind: DockNodeLayoutKind,
}

/// The payload of a [`DockNodeLayout`] record.
///
/// # Examples
///
/// ```
/// use martensite_blessed::{DockNodeLayoutKind, DockPanel, SplitDirection};
/// let split = DockNodeLayoutKind::Split {
///     direction: SplitDirection::Vertical,
///     ratio: 0.5,
///     left: 1,
///     right: 2,
/// };
/// assert!(matches!(split, DockNodeLayoutKind::Split { .. }));
/// let leaf = DockNodeLayoutKind::Leaf { panel: DockPanel::new(0, "A") };
/// assert!(matches!(leaf, DockNodeLayoutKind::Leaf { .. }));
/// ```
#[derive(Clone, Debug, PartialEq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub enum DockNodeLayoutKind {
    /// An interior split node referencing two child records by id.
    Split {
        /// Orientation of the divider.
        direction: SplitDirection,
        /// Fraction of the rectangle assigned to the `left` child.
        ratio: f64,
        /// Id of the first child record.
        left: u32,
        /// Id of the second child record.
        right: u32,
    },
    /// A terminal leaf node holding a docked panel.
    Leaf {
        /// The panel docked into this leaf.
        panel: DockPanel,
    },
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    fn panel(id: u64, title: &str) -> DockPanel {
        DockPanel::new(id, title)
    }

    #[test]
    fn insert_root_sets_root() {
        let mut tree = DockTree::new();
        let root = tree.insert_root(panel(0, "a"));
        assert_eq!(tree.root(), Some(root));
        assert_eq!(tree.node_count(), 1);
        assert_eq!(tree.panel_count(), 1);
    }

    #[test]
    fn split_creates_two_leaves() {
        let mut tree = DockTree::new();
        let root = tree.insert_root(panel(0, "a"));
        let (left, right) = tree
            .split_leaf(root, SplitDirection::Vertical, 0.5, panel(1, "b"))
            .unwrap();
        assert!(tree.node(left).unwrap().is_leaf());
        assert!(tree.node(right).unwrap().is_leaf());
        assert_eq!(tree.panel_count(), 2);
        assert_eq!(tree.node_count(), 3);
    }

    #[test]
    fn split_rejects_non_leaf() {
        let mut tree = DockTree::new();
        let root = tree.insert_root(panel(0, "a"));
        tree.split_leaf(root, SplitDirection::Vertical, 0.5, panel(1, "b"))
            .unwrap();
        assert_eq!(
            tree.split_leaf(root, SplitDirection::Vertical, 0.5, panel(2, "c")),
            Err(DockError::CannotSplitNonLeaf)
        );
    }

    #[test]
    fn split_rejects_bad_ratio() {
        let mut tree = DockTree::new();
        let root = tree.insert_root(panel(0, "a"));
        assert_eq!(
            tree.split_leaf(root, SplitDirection::Vertical, 0.0, panel(1, "b")),
            Err(DockError::InvalidRatio(0.0))
        );
        assert_eq!(
            tree.split_leaf(root, SplitDirection::Vertical, 1.0, panel(1, "b")),
            Err(DockError::InvalidRatio(1.0))
        );
        assert!(matches!(
            tree.split_leaf(root, SplitDirection::Vertical, f64::NAN, panel(1, "b")),
            Err(DockError::InvalidRatio(_))
        ));
    }

    #[test]
    fn split_rejects_missing_node() {
        let mut tree = DockTree::new();
        assert_eq!(
            tree.split_leaf(NodeId(99), SplitDirection::Vertical, 0.5, panel(1, "b")),
            Err(DockError::NodeNotFound)
        );
    }

    #[test]
    fn merge_collapses_split() {
        let mut tree = DockTree::new();
        let root = tree.insert_root(panel(0, "a"));
        tree.split_leaf(root, SplitDirection::Vertical, 0.5, panel(1, "b"))
            .unwrap();
        tree.merge(root).unwrap();
        assert_eq!(tree.panel_count(), 1);
        assert!(tree.node(root).unwrap().is_leaf());
        // The original (left) panel survives.
        match tree.node(root).unwrap() {
            DockNode::Leaf { panel } => assert_eq!(panel.widget_id(), 0),
            _ => panic!("expected leaf"),
        }
    }

    #[test]
    fn merge_rejects_leaf() {
        let mut tree = DockTree::new();
        let root = tree.insert_root(panel(0, "a"));
        assert_eq!(tree.merge(root), Err(DockError::CannotMergeLeaf));
    }

    #[test]
    fn merge_rejects_missing() {
        let mut tree = DockTree::new();
        assert_eq!(tree.merge(NodeId(99)), Err(DockError::NodeNotFound));
    }

    #[test]
    fn remove_collapses_parent() {
        let mut tree = DockTree::new();
        let root = tree.insert_root(panel(0, "a"));
        let (left, right) = tree
            .split_leaf(root, SplitDirection::Vertical, 0.5, panel(1, "b"))
            .unwrap();
        tree.remove(right);
        assert_eq!(tree.panel_count(), 1);
        assert!(tree.node(root).unwrap().is_leaf());
        assert_eq!(tree.node(left), None);
    }

    #[test]
    fn remove_root_clears_tree() {
        let mut tree = DockTree::new();
        let root = tree.insert_root(panel(0, "a"));
        tree.remove(root);
        assert_eq!(tree.root(), None);
        assert_eq!(tree.node_count(), 0);
    }

    #[test]
    fn panel_rects_subdivide_vertical() {
        let mut tree = DockTree::new();
        let root = tree.insert_root(panel(0, "a"));
        tree.split_leaf(root, SplitDirection::Vertical, 0.3, panel(1, "b"))
            .unwrap();
        let rects: Vec<_> = tree
            .panel_rects(Rect::new(0.0, 0.0, 100.0, 100.0))
            .collect();
        assert_eq!(rects.len(), 2);
        let total_width: f64 = rects.iter().map(|(_, r)| r.width).sum();
        assert!((total_width - 100.0).abs() < 1e-9);
    }

    #[test]
    fn panel_rects_subdivide_horizontal() {
        let mut tree = DockTree::new();
        let root = tree.insert_root(panel(0, "a"));
        tree.split_leaf(root, SplitDirection::Horizontal, 0.25, panel(1, "b"))
            .unwrap();
        let rects: Vec<_> = tree
            .panel_rects(Rect::new(0.0, 0.0, 100.0, 100.0))
            .collect();
        assert_eq!(rects.len(), 2);
        let total_height: f64 = rects.iter().map(|(_, r)| r.height).sum();
        assert!((total_height - 100.0).abs() < 1e-9);
    }

    #[test]
    fn panel_rects_empty_tree() {
        let tree = DockTree::new();
        let rects: Vec<_> = tree
            .panel_rects(Rect::new(0.0, 0.0, 100.0, 100.0))
            .collect();
        assert!(rects.is_empty());
    }

    #[test]
    fn panels_iterates_leaves_only() {
        let mut tree = DockTree::new();
        let root = tree.insert_root(panel(0, "a"));
        tree.split_leaf(root, SplitDirection::Vertical, 0.5, panel(1, "b"))
            .unwrap();
        let ids: Vec<_> = tree.panels().map(|(_, n)| n.widget_id()).collect();
        assert_eq!(ids, vec![0, 1]);
    }

    #[test]
    fn layout_round_trip() {
        let mut tree = DockTree::new();
        let root = tree.insert_root(panel(0, "a"));
        tree.split_leaf(root, SplitDirection::Vertical, 0.4, panel(1, "b"))
            .unwrap();
        let layout = tree.to_layout();
        let rebuilt = DockTree::from_layout(layout).unwrap();
        assert_eq!(rebuilt.panel_count(), 2);
        assert_eq!(rebuilt.node_count(), 3);
        let ids: Vec<_> = rebuilt.panels().map(|(_, n)| n.widget_id()).collect();
        assert_eq!(ids, vec![0, 1]);
    }

    #[test]
    fn from_layout_empty_errors() {
        assert!(matches!(
            DockTree::from_layout(Vec::new()),
            Err(DockError::TreeEmpty)
        ));
    }

    #[test]
    fn drag_session_preview() {
        let session = DockDragSession::new(panel(1, "b"));
        let target = Rect::new(0.0, 0.0, 100.0, 100.0);
        assert_eq!(
            session.preview_rect(target, DockDropZone::Left),
            Rect::new(0.0, 0.0, 50.0, 100.0)
        );
        assert_eq!(
            session.preview_rect(target, DockDropZone::Right),
            Rect::new(50.0, 0.0, 50.0, 100.0)
        );
        assert_eq!(
            session.preview_rect(target, DockDropZone::Top),
            Rect::new(0.0, 0.0, 100.0, 50.0)
        );
        assert_eq!(
            session.preview_rect(target, DockDropZone::Bottom),
            Rect::new(0.0, 50.0, 100.0, 50.0)
        );
        assert_eq!(session.preview_rect(target, DockDropZone::Center), target);
    }

    #[test]
    fn rect_contains_edges() {
        let r = Rect::new(10.0, 10.0, 20.0, 20.0);
        assert!(r.contains(10.0, 10.0));
        assert!(!r.contains(30.0, 10.0));
        assert!(!r.contains(10.0, 30.0));
        assert!(!r.contains(f64::NAN, 15.0));
    }
}
