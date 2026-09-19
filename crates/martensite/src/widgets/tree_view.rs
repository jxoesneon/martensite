//! `TreeView` widget: a virtualized, hierarchical tree of labelled
//! nodes (QTreeView / GtkTreeExpander / NSOutlineView).
//!
//! Implements the core of the [APG tree view pattern](https://www.w3.org/WAI/ARIA/apg/patterns/treeview/):
//!
//! - `Role::Tree` on the widget; each *visible* row is emitted as an
//!   internal `Role::TreeItem` child carrying `level`, `expanded`,
//!   `selected`, and `posinset` metadata — the same roving-tabindex
//!   contract `RadioGroup` uses, so platform focus stays a single
//!   stop on the tree node.
//! - **Flattened projection**: the tree is walked into a visible-row
//!   list that skips collapsed subtrees; layout, hit-testing, paint,
//!   and AT all index into that projection, so expanding/collapsing
//!   is a cheap rebuild rather than a layout pass over the model.
//! - **Virtualized**: like `ListView`, only rows intersecting the
//!   viewport are pooled as child widgets — the same owned
//!   `scroll_y` + smart vertical scrollbar machinery
//!   (`ScrollView` cannot be reused; its content child paints whole).
//! - Keyboard: `ArrowUp`/`ArrowDown` move focus+selection over
//!   *visible* rows, `ArrowRight` expands or descends to the first
//!   child, `ArrowLeft` collapses or ascends to the parent,
//!   `Home`/`End`/`PageUp`/`PageDown` jump, `Enter` activates the
//!   focused path (drained via [`TreeView::take_activated`]).
//! - Pointer: click the disclosure triangle to toggle without
//!   selecting; click a row to select it; double-click toggles;
//!   hover highlights.
//!
//! # Documented limitations
//!
//! - **Single selection only** — tree multi-selection is out of
//!   scope (and `WidgetEvent::KeyPressed` carries no modifier field;
//!   see `list_view`'s note).
//! - **Items are labels**, not cell delegates — columns and per-node
//!   rich content are intentionally out of scope.
//! - **AT windowing**: rows outside the viewport are not emitted;
//!   `posinset`/`setsize` give AT the full coordinate space.
//!
//! # Examples
//!
//! ```
//! use martensite::widgets::{TreeNode, TreeView};
//!
//! let tree = TreeView::new().roots(vec![TreeNode::new("root")
//!     .with_children(vec![TreeNode::new("child")])]);
//! assert_eq!(tree.visible_row_count(), 2);
//! ```

use std::ops::Range;
use std::sync::{Arc, Mutex};

use accesskit::Node as AccessKitNode;
use glam::Vec2;
use martensite_core::widget::{
    EventContext, EventResponse, LayoutConstraints, LayoutContext, PaintContext, PointerButton,
    SemanticAction, Widget, WidgetEvent,
};
use martensite_core::{NodeFlags, Rect, RenderMinimum, TokenKey, UnderflowPolicy};

use crate::widgets::list_view::{BarRequest, VScrollBar};

/// Default row height in logical pixels.
const ROW_H: f32 = 24.0;
/// Maximum rows the widget asks for in `measure` before scrolling.
const MAX_VISIBLE_ROWS: f32 = 12.0;
/// Scrollbar thickness in logical pixels.
const BAR: f32 = 10.0;
/// Minimum scrollbar thumb length.
const MIN_THUMB: f32 = 24.0;
/// Per-depth indent in logical pixels.
const INDENT: f32 = 16.0;
/// Disclosure triangle box edge in logical pixels.
const TRI: f32 = 12.0;
/// Gap between the disclosure triangle and the label.
const TRI_GAP: f32 = 4.0;
/// List face background.
const SURFACE_BG: [u8; 4] = [250, 250, 252, 255];
/// List border.
const BORDER: [u8; 4] = [150, 155, 165, 255];
/// Label ink.
const INK: [u8; 4] = [30, 30, 36, 255];
/// Disabled label ink.
const INK_DISABLED: [u8; 4] = [150, 150, 158, 255];
/// Selected-row ink (on the accent fill).
const INK_SELECTED: [u8; 4] = [255, 255, 255, 255];
/// Selection accent.
const ACCENT: [u8; 4] = [60, 110, 220, 255];
/// Focus ring alpha (the accent colour at 50%).
const FOCUS_RING: [u8; 4] = [60, 110, 220, 128];
/// Hover wash alpha.
const HOVER_ALPHA: u8 = 32;
/// Disclosure triangle ink.
const TRI_INK: [u8; 4] = [90, 95, 105, 255];

/// One node in a [`TreeView`] model.
///
/// Nodes own their children recursively; `expanded` controls whether
/// the subtree is visible in the flattened projection. Construct with
/// [`TreeNode::new`] and compose with [`with_children`](Self::with_children).
///
/// # Examples
///
/// ```
/// use martensite::widgets::TreeNode;
///
/// let node = TreeNode::new("src")
///     .with_children(vec![TreeNode::new("main.rs")]);
/// assert_eq!(node.children.len(), 1);
/// ```
#[derive(Clone, Debug, PartialEq)]
pub struct TreeNode {
    /// The node's label.
    pub label: String,
    /// Child nodes, in display order.
    pub children: Vec<TreeNode>,
    /// Whether the subtree is expanded (children visible).
    pub expanded: bool,
}

impl TreeNode {
    /// Creates a node with `label`, no children, expanded by default
    /// so a freshly-built tree shows its content.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite::widgets::TreeNode;
    ///
    /// let n = TreeNode::new("root");
    /// assert_eq!(n.label, "root");
    /// assert!(n.expanded);
    /// ```
    pub fn new(label: impl Into<String>) -> Self {
        Self {
            label: label.into(),
            children: Vec::new(),
            expanded: true,
        }
    }

    /// Sets the node's children (builder form).
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite::widgets::TreeNode;
    ///
    /// let n = TreeNode::new("a").with_children(vec![TreeNode::new("b")]);
    /// assert!(!n.is_leaf());
    /// ```
    #[must_use]
    pub fn with_children(mut self, children: Vec<TreeNode>) -> Self {
        self.children = children;
        self
    }

    /// Sets the expanded flag (builder form).
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite::widgets::TreeNode;
    ///
    /// let n = TreeNode::new("a").expanded(false);
    /// assert!(!n.expanded);
    /// ```
    #[inline]
    #[must_use]
    pub fn expanded(mut self, expanded: bool) -> Self {
        self.expanded = expanded;
        self
    }

    /// Whether the node has no children (no disclosure triangle).
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite::widgets::TreeNode;
    ///
    /// assert!(TreeNode::new("leaf").is_leaf());
    /// ```
    #[inline]
    pub fn is_leaf(&self) -> bool {
        self.children.is_empty()
    }
}

/// A node in the flattened visible-row projection.
struct FlatRow {
    /// Path of `usize` child indices from the roots.
    path: Vec<usize>,
    /// Nesting depth (0 = root row).
    depth: usize,
    /// Whether the node has children (drives the triangle).
    has_children: bool,
    /// Whether the node is expanded.
    expanded: bool,
    /// The node's label (cloned so rows need no tree walk).
    label: String,
}

/// One visible row inside a [`TreeView`].
///
/// Rows are *pooled* exactly like `ListView`'s: enough `TreeItemRow`s
/// to cover the viewport, re-pointed at flattened rows as the view
/// scrolls. Each is emitted as an internal child with `Role::TreeItem`;
/// its fields mirror tree state via [`TreeView::sync_rows`]. AT actions
/// it receives are parked and applied by the owner via
/// [`TreeView::poll_pending`].
struct TreeItemRow {
    /// Flattened-row index this row presents.
    flat_index: usize,
    /// The node's accessible label.
    label: String,
    /// Nesting depth (drives indent and `level`).
    depth: usize,
    /// Whether the node has children.
    has_children: bool,
    /// Whether the node is expanded.
    expanded: bool,
    /// Whether the row is selected (mirrored from the owner).
    selected: bool,
    /// Whether this row carries the roving tabindex.
    focused: bool,
    /// Whether the owning tree holds keyboard focus.
    has_focus: bool,
    /// Whether the pointer is over this row.
    hovered: bool,
    /// Whether the owner is enabled.
    enabled: bool,
    /// Parked `SemanticAction::Click` / pointer press for the owner.
    press_pending: bool,
    /// Parked `SemanticAction::Focus` for the owner.
    focus_pending: bool,
    /// Parked `SemanticAction::Expand`/`Collapse` for the owner.
    expand_request: Option<bool>,
    /// Shared shaped-text painter from the owning `TreeView`.
    text_painter: Option<crate::text_paint::SharedTextPainter>,
}

impl TreeItemRow {
    fn new() -> Self {
        Self {
            flat_index: 0,
            label: String::new(),
            depth: 0,
            has_children: false,
            expanded: false,
            selected: false,
            focused: false,
            has_focus: false,
            hovered: false,
            enabled: true,
            press_pending: false,
            focus_pending: false,
            expand_request: None,
            text_painter: None,
        }
    }
}

impl Widget for TreeItemRow {
    fn measure(&mut self, cx: &mut LayoutContext, constraints: LayoutConstraints) -> Vec2 {
        Vec2::new(
            constraints.max_size.x.max(0.0),
            cx.pt(ROW_H).min(constraints.max_size.y.max(0.0)),
        )
    }

    fn layout(&mut self, _cx: &mut LayoutContext, _bounds: Rect) {}

    fn accessibility(&self, node: &mut AccessKitNode) {
        node.set_role(accesskit::Role::TreeItem);
        node.set_label(self.label.as_str());
        node.set_selected(self.selected);
        // `level` and `position_in_set` are zero-based (unlike ARIA's
        // one-based `aria-level`/`aria-posinset`); `size_of_set` lives
        // on the Tree container.
        node.set_level(self.depth);
        node.set_position_in_set(self.flat_index);
        if self.has_children {
            node.set_expanded(self.expanded);
            if self.expanded {
                node.add_action(accesskit::Action::Collapse);
            } else {
                node.add_action(accesskit::Action::Expand);
            }
        }
        node.add_action(accesskit::Action::Click);
        // Roving tabindex: only the row holding the tab stop
        // advertises Focus — the tree is a single tab stop.
        if self.focused {
            node.add_action(accesskit::Action::Focus);
        }
        if !self.enabled {
            node.set_disabled();
        }
    }

    fn event(&mut self, cx: &mut EventContext) -> EventResponse {
        if !self.enabled {
            return EventResponse::Ignored;
        }
        match cx.event {
            WidgetEvent::PointerPressed {
                button: PointerButton::Primary,
                ..
            }
            | WidgetEvent::SemanticAction(SemanticAction::Click) => {
                // Park the press; the owning tree applies it via
                // `poll_pending` so selection stays centralized.
                self.press_pending = true;
                EventResponse::CaptureFocus
            }
            WidgetEvent::SemanticAction(SemanticAction::Focus) => {
                self.focus_pending = true;
                EventResponse::CaptureFocus
            }
            WidgetEvent::SemanticAction(SemanticAction::Expand) => {
                self.expand_request = Some(true);
                EventResponse::Handled
            }
            WidgetEvent::SemanticAction(SemanticAction::Collapse) => {
                self.expand_request = Some(false);
                EventResponse::Handled
            }
            _ => EventResponse::Ignored,
        }
    }

    fn paint(&self, cx: &mut PaintContext) {
        let b = cx.bounds;
        let rect = kurbo::Rect::new(
            f64::from(b.min_x()),
            f64::from(b.min_y()),
            f64::from(b.max_x()),
            f64::from(b.max_y()),
        );
        let accent = cx.color(TokenKey::AccentColor, ACCENT);
        if self.selected {
            cx.list.push_fill_rect(rect, accent);
        } else if self.hovered {
            // Translucent wash of the accent colour.
            let wash = [accent[0], accent[1], accent[2], HOVER_ALPHA];
            cx.list.push_fill_rect(rect, wash);
        }
        if self.focused && self.has_focus {
            let wash = [accent[0], accent[1], accent[2], FOCUS_RING[3]];
            cx.list.push_stroke_rect(rect, cx.pt(2.0), wash);
        }
        let indent_px = cx.pt(INDENT) * self.depth as f32;
        let tri_px = cx.pt(TRI);
        // Disclosure triangle: a small filled BezPath — right-pointing
        // when collapsed, down-pointing when expanded. Leaves get an
        // empty triangle box so labels stay aligned.
        if self.has_children {
            let cxm = f64::from(b.min_x() + indent_px + tri_px / 2.0);
            let cy = f64::from(b.min_y() + b.height() / 2.0);
            let s = f64::from(tri_px * 0.32);
            let mut path = kurbo::BezPath::new();
            if self.expanded {
                path.move_to((cxm - s, cy - s * 0.6));
                path.line_to((cxm + s, cy - s * 0.6));
                path.line_to((cxm, cy + s * 0.8));
            } else {
                path.move_to((cxm - s * 0.6, cy - s));
                path.line_to((cxm + s * 0.8, cy));
                path.line_to((cxm - s * 0.6, cy + s));
            }
            path.close_path();
            cx.list.push_path(path, cx.color(TokenKey::TextMutedColor, TRI_INK));
        }
        // `DrawText` positions by the run's top edge — centre the font
        // box inside the row, clipped so a long label can't spill.
        let font_px = cx.pt(14.0);
        let ink = if self.selected {
            cx.color(TokenKey::TextInverseColor, INK_SELECTED)
        } else if self.enabled {
            cx.color(TokenKey::TextColor, INK)
        } else {
            cx.color(TokenKey::TextMutedColor, INK_DISABLED)
        };
        let text_x = b.min_x() + indent_px + tri_px + cx.pt(TRI_GAP);
        crate::text_paint::paint_label_clipped(
            crate::text_paint::resolve_painter(&self.text_painter, cx.text_painter),
            cx.list,
            kurbo::Rect::new(
                f64::from(text_x),
                f64::from(b.min_y()),
                f64::from(b.max_x() - cx.pt(8.0)),
                f64::from(b.max_y()),
            ),
            kurbo::Point::new(
                f64::from(text_x),
                f64::from(b.min_y() + (b.height() - font_px) / 2.0),
            ),
            &self.label,
            font_px,
            ink,
        );
    }
}

/// A virtualized, hierarchical tree of labelled nodes.
///
/// Selection is by *path* — a `Vec<usize>` of child indices from the
/// roots — so it stays meaningful across expand/collapse and content
/// edits above the node. The activation out-seam mirrors the
/// codebase's `Dialog::take_response`/`response_sink` pattern.
///
/// # Examples
///
/// ```
/// use martensite::widgets::{TreeNode, TreeView};
///
/// let mut tree = TreeView::new().roots(vec![TreeNode::new("root")
///     .with_children(vec![TreeNode::new("a"), TreeNode::new("b")])]);
/// tree.select_path(&[0, 1]);
/// assert_eq!(tree.selected_path(), Some(&[0, 1][..]));
/// ```
pub struct TreeView {
    /// Whether the tree accepts input.
    pub enabled: bool,
    /// Optional accessible label for the tree.
    pub label: Option<String>,
    /// Per-depth indent in logical points.
    pub indent: f32,
    /// The root nodes.
    roots: Vec<TreeNode>,
    /// Row height in logical points. Prefer
    /// [`set_row_height`](Self::set_row_height) — it rescales the
    /// scroll offset so the top row is preserved; direct assignment
    /// simply clamps the offset at the next layout.
    pub row_height: f32,
    /// The flattened visible-row projection (rebuilt on tree/expansion
    /// changes — never on scroll).
    flat: Vec<FlatRow>,
    /// Selected path, if any (survives collapse of its ancestors).
    selection: Option<Vec<usize>>,
    /// Visible-row index carrying the roving tabindex.
    focused: usize,
    /// Whether the widget holds keyboard focus (for the focus ring).
    has_focus: bool,
    /// Hovered visible-row index.
    hovered: Option<usize>,
    /// Vertical scroll offset in device pixels.
    scroll_y: f32,
    /// Pooled visible-window row children.
    rows: Vec<TreeItemRow>,
    /// The vertical scrollbar (last internal child).
    vbar: VScrollBar,
    /// Path activated since the last `take_activated`.
    activated: Option<Vec<usize>>,
    /// Shared cell also receiving activations — the observation seam.
    activated_sink: Option<Arc<Mutex<Option<Vec<usize>>>>>,
    /// Cached widget bounds.
    cached_bounds: Rect,
    /// Row viewport (widget bounds minus the shown bar).
    viewport: Rect,
    /// Vertical bar rect when shown.
    vbar_rect: Option<Rect>,
    /// Thumb-drag state: grab offset inside the thumb.
    thumb_drag: Option<f32>,
    /// Display scale from `layout` — row height, bar, thumbs are
    /// logical pt.
    scale: f32,
    /// Shared shaped-text painter — propagated to rows in `sync_rows`.
    text_painter: Option<crate::text_paint::SharedTextPainter>,
}

impl TreeView {
    /// Creates an empty tree; add roots with [`roots`](Self::roots) or
    /// [`set_root`](Self::set_root).
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite::widgets::TreeView;
    ///
    /// let tree = TreeView::new();
    /// assert_eq!(tree.visible_row_count(), 0);
    /// ```
    pub fn new() -> Self {
        Self {
            enabled: true,
            label: None,
            indent: INDENT,
            roots: Vec::new(),
            row_height: ROW_H,
            flat: Vec::new(),
            selection: None,
            focused: 0,
            has_focus: false,
            hovered: None,
            scroll_y: 0.0,
            rows: Vec::new(),
            vbar: VScrollBar::new(),
            activated: None,
            activated_sink: None,
            cached_bounds: Rect::default(),
            viewport: Rect::default(),
            vbar_rect: None,
            thumb_drag: None,
            scale: 1.0,
            text_painter: None,
        }
    }

    /// Sets the root nodes (builder form of
    /// [`set_roots`](Self::set_roots)).
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite::widgets::{TreeNode, TreeView};
    ///
    /// let tree = TreeView::new().roots(vec![TreeNode::new("root")]);
    /// assert_eq!(tree.visible_row_count(), 1);
    /// ```
    #[must_use]
    pub fn roots(mut self, roots: Vec<TreeNode>) -> Self {
        self.set_roots(roots);
        self
    }

    /// Replaces the root nodes and rebuilds the flattened projection.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite::widgets::{TreeNode, TreeView};
    ///
    /// let mut tree = TreeView::new();
    /// tree.set_roots(vec![TreeNode::new("a"), TreeNode::new("b")]);
    /// assert_eq!(tree.visible_row_count(), 2);
    /// ```
    pub fn set_roots(&mut self, roots: Vec<TreeNode>) {
        self.roots = roots;
        self.rebuild_flat();
        self.clamp_state();
    }

    /// Sets a single root node — convenience for the common
    /// one-invisible-root tree.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite::widgets::{TreeNode, TreeView};
    ///
    /// let mut tree = TreeView::new();
    /// tree.set_root(TreeNode::new("root"));
    /// assert_eq!(tree.visible_row_count(), 1);
    /// ```
    pub fn set_root(&mut self, root: TreeNode) {
        self.set_roots(vec![root]);
    }

    /// The node at `path` (a chain of child indices from the roots),
    /// if it exists.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite::widgets::{TreeNode, TreeView};
    ///
    /// let tree = TreeView::new().roots(vec![TreeNode::new("a")
    ///     .with_children(vec![TreeNode::new("b")])]);
    /// assert_eq!(tree.node(&[0, 0]).map(|n| n.label.as_str()), Some("b"));
    /// assert!(tree.node(&[9]).is_none());
    /// ```
    pub fn node(&self, path: &[usize]) -> Option<&TreeNode> {
        let (first, rest) = path.split_first()?;
        let mut node = self.roots.get(*first)?;
        for &i in rest {
            node = node.children.get(i)?;
        }
        Some(node)
    }

    /// Mutable access to the node at `path`. Mutating `children` or
    /// `expanded` through this does not rebuild the projection —
    /// prefer [`set_expanded`](Self::set_expanded)/
    /// [`toggle`](Self::toggle), or call [`rebuild`](Self::rebuild)
    /// after structural edits.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite::widgets::{TreeNode, TreeView};
    ///
    /// let mut tree = TreeView::new().roots(vec![TreeNode::new("a")]);
    /// tree.node_mut(&[0]).unwrap().label = "renamed".to_string();
    /// tree.rebuild();
    /// ```
    pub fn node_mut(&mut self, path: &[usize]) -> Option<&mut TreeNode> {
        let (first, rest) = path.split_first()?;
        let mut node = self.roots.get_mut(*first)?;
        for &i in rest {
            node = node.children.get_mut(i)?;
        }
        Some(node)
    }

    /// Rebuilds the flattened projection after direct model edits
    /// (through [`node_mut`](Self::node_mut) or a replaced root).
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite::widgets::{TreeNode, TreeView};
    ///
    /// let mut tree = TreeView::new().roots(vec![TreeNode::new("a")]);
    /// tree.node_mut(&[0]).unwrap().children.push(TreeNode::new("b"));
    /// tree.rebuild();
    /// assert_eq!(tree.visible_row_count(), 2);
    /// ```
    pub fn rebuild(&mut self) {
        self.rebuild_flat();
        self.clamp_state();
    }

    /// Sets the expanded flag of the node at `path` and rebuilds the
    /// projection. Unknown paths and leaf nodes are ignored.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite::widgets::{TreeNode, TreeView};
    ///
    /// let mut tree = TreeView::new().roots(vec![TreeNode::new("a")
    ///     .with_children(vec![TreeNode::new("b")])]);
    /// tree.set_expanded(&[0], false);
    /// assert_eq!(tree.visible_row_count(), 1);
    /// tree.set_expanded(&[0], true);
    /// assert_eq!(tree.visible_row_count(), 2);
    /// ```
    pub fn set_expanded(&mut self, path: &[usize], expanded: bool) {
        let node = match path.split_first() {
            Some((&first, rest)) => match self.roots.get_mut(first) {
                Some(node) => {
                    let mut n = node;
                    for &i in rest {
                        match n.children.get_mut(i) {
                            Some(child) => n = child,
                            None => return,
                        }
                    }
                    n
                }
                None => return,
            },
            None => return,
        };
        if node.children.is_empty() || node.expanded == expanded {
            return;
        }
        node.expanded = expanded;
        self.rebuild_flat();
        self.clamp_state();
    }

    /// Toggles the expanded flag of the node at `path`.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite::widgets::{TreeNode, TreeView};
    ///
    /// let mut tree = TreeView::new().roots(vec![TreeNode::new("a")
    ///     .with_children(vec![TreeNode::new("b")])]);
    /// tree.toggle(&[0]);
    /// assert_eq!(tree.visible_row_count(), 1);
    /// tree.toggle(&[0]);
    /// assert_eq!(tree.visible_row_count(), 2);
    /// ```
    pub fn toggle(&mut self, path: &[usize]) {
        let expanded = self.node(path).map(|n| n.expanded);
        if let Some(expanded) = expanded {
            self.set_expanded(path, !expanded);
        }
    }

    /// The expanded flag of the node at `path`, if the path exists.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite::widgets::{TreeNode, TreeView};
    ///
    /// let tree = TreeView::new().roots(vec![TreeNode::new("a")]);
    /// assert_eq!(tree.is_expanded(&[0]), Some(true));
    /// ```
    pub fn is_expanded(&self, path: &[usize]) -> Option<bool> {
        self.node(path).map(|n| n.expanded)
    }

    /// Expands every node — the whole tree becomes visible.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite::widgets::{TreeNode, TreeView};
    ///
    /// let mut tree = TreeView::new().roots(vec![TreeNode::new("a")
    ///     .expanded(false)
    ///     .with_children(vec![TreeNode::new("b")])]);
    /// tree.expand_all();
    /// assert_eq!(tree.visible_row_count(), 2);
    /// ```
    pub fn expand_all(&mut self) {
        fn walk(node: &mut TreeNode) {
            node.expanded = true;
            for child in &mut node.children {
                walk(child);
            }
        }
        for root in &mut self.roots {
            walk(root);
        }
        self.rebuild_flat();
        self.clamp_state();
    }

    /// Collapses every node — only the roots stay visible.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite::widgets::{TreeNode, TreeView};
    ///
    /// let mut tree = TreeView::new().roots(vec![TreeNode::new("a")
    ///     .with_children(vec![TreeNode::new("b")])]);
    /// tree.collapse_all();
    /// assert_eq!(tree.visible_row_count(), 1);
    /// ```
    pub fn collapse_all(&mut self) {
        fn walk(node: &mut TreeNode) {
            node.expanded = false;
            for child in &mut node.children {
                walk(child);
            }
        }
        for root in &mut self.roots {
            walk(root);
        }
        self.rebuild_flat();
        self.clamp_state();
    }

    /// The number of rows currently visible in the flattened
    /// projection.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite::widgets::{TreeNode, TreeView};
    ///
    /// let tree = TreeView::new().roots(vec![TreeNode::new("a")
    ///     .with_children(vec![TreeNode::new("b")])]);
    /// assert_eq!(tree.visible_row_count(), 2);
    /// ```
    #[inline]
    pub fn visible_row_count(&self) -> usize {
        self.flat.len()
    }

    /// Sets the row height in logical points.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite::widgets::TreeView;
    ///
    /// let t = TreeView::new().row_height(32.0);
    /// assert_eq!(t.row_height, 32.0);
    /// ```
    #[inline]
    #[must_use]
    pub fn row_height(mut self, height: f32) -> Self {
        self.set_row_height(height);
        self
    }

    /// Sets the row height in logical points (mutating form).
    /// Rescales the scroll offset so the top row is preserved across
    /// the change; assigning `row_height` directly skips the rescale
    /// (the offset simply clamps at the next layout).
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite::widgets::TreeView;
    ///
    /// let mut t = TreeView::new();
    /// t.set_row_height(18.0);
    /// assert_eq!(t.row_height, 18.0);
    /// ```
    pub fn set_row_height(&mut self, height: f32) {
        // Preserve the top row across the change (offsets are pixels).
        let new = if height.is_finite() { height.max(1.0) } else { ROW_H };
        let old_px = self.row_height * self.scale;
        let new_px = new * self.scale;
        if old_px > 0.0 && new_px > 0.0 && new_px != old_px {
            self.scroll_y *= new_px / old_px;
        }
        self.row_height = new;
        self.scroll_y = self.scroll_y.clamp(0.0, self.max_scroll());
        self.sync_rows();
        self.sync_bars();
    }

    /// Sets the per-depth indent in logical points (builder form).
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite::widgets::TreeView;
    ///
    /// let t = TreeView::new().indent(24.0);
    /// assert_eq!(t.indent, 24.0);
    /// ```
    #[inline]
    #[must_use]
    pub fn indent(mut self, indent: f32) -> Self {
        self.indent = indent;
        self
    }

    /// Sets the accessible label.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite::widgets::TreeView;
    ///
    /// let t = TreeView::new().label("Files");
    /// assert_eq!(t.label.as_deref(), Some("Files"));
    /// ```
    #[inline]
    #[must_use]
    pub fn label(mut self, label: impl Into<String>) -> Self {
        self.label = Some(label.into());
        self
    }

    /// Sets whether the tree accepts input.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite::widgets::TreeView;
    ///
    /// let t = TreeView::new().enabled(false);
    /// assert!(!t.enabled);
    /// ```
    #[inline]
    #[must_use]
    pub fn enabled(mut self, enabled: bool) -> Self {
        self.enabled = enabled;
        self.sync_rows();
        self
    }

    /// Shares a [`crate::text_paint::TextPainter`] so row labels emit
    /// real glyph runs instead of `DrawText` placeholder boxes.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite::widgets::TreeView;
    /// use martensite::text_paint::shared_painter;
    ///
    /// let t = TreeView::new().with_text_painter(shared_painter());
    /// ```
    #[must_use]
    pub fn with_text_painter(mut self, painter: crate::text_paint::SharedTextPainter) -> Self {
        self.text_painter = Some(painter);
        self.sync_rows();
        self
    }

    /// Wires a shared cell that receives each activated path — the
    /// observation seam for hosts that cannot poll
    /// [`take_activated`](Self::take_activated).
    ///
    /// # Examples
    ///
    /// ```
    /// use std::sync::{Arc, Mutex};
    /// use martensite::widgets::TreeView;
    ///
    /// let sink = Arc::new(Mutex::new(None));
    /// let t = TreeView::new().activated_sink(sink.clone());
    /// assert!(sink.lock().unwrap().is_none());
    /// ```
    #[must_use]
    pub fn activated_sink(mut self, sink: Arc<Mutex<Option<Vec<usize>>>>) -> Self {
        self.activated_sink = Some(sink);
        self
    }

    /// The selected path, if any.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite::widgets::{TreeNode, TreeView};
    ///
    /// let mut t = TreeView::new().roots(vec![TreeNode::new("a")]);
    /// assert_eq!(t.selected_path(), None);
    /// t.select_path(&[0]);
    /// assert_eq!(t.selected_path(), Some(&[0][..]));
    /// ```
    #[inline]
    pub fn selected_path(&self) -> Option<&[usize]> {
        self.selection.as_deref()
    }

    /// Selects the node at `path` and moves the roving tabindex to its
    /// visible row. Unknown or currently-hidden paths are ignored.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite::widgets::{TreeNode, TreeView};
    ///
    /// let mut t = TreeView::new().roots(vec![TreeNode::new("a")
    ///     .with_children(vec![TreeNode::new("b")])]);
    /// t.select_path(&[0, 0]);
    /// assert_eq!(t.selected_path(), Some(&[0, 0][..]));
    /// t.select_path(&[9]);
    /// assert_eq!(t.selected_path(), Some(&[0, 0][..])); // ignored
    /// ```
    pub fn select_path(&mut self, path: &[usize]) {
        if let Some(i) = self.flat_index_of(path) {
            self.focused = i;
            self.selection = Some(path.to_vec());
            self.ensure_visible(i);
            self.sync_rows();
        }
    }

    /// Selects the visible row `index` (an index into the flattened
    /// projection, not a path). Out-of-range indices are ignored.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite::widgets::{TreeNode, TreeView};
    ///
    /// let mut t = TreeView::new().roots(vec![TreeNode::new("a")
    ///     .with_children(vec![TreeNode::new("b")])]);
    /// t.select_row(1);
    /// assert_eq!(t.selected_path(), Some(&[0, 0][..]));
    /// ```
    pub fn select_row(&mut self, index: usize) {
        if index < self.flat.len() {
            self.focused = index;
            self.selection = Some(self.flat[index].path.clone());
            self.ensure_visible(index);
            self.sync_rows();
        }
    }

    /// Clears the selection.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite::widgets::{TreeNode, TreeView};
    ///
    /// let mut t = TreeView::new().roots(vec![TreeNode::new("a")]);
    /// t.select_path(&[0]);
    /// t.clear_selection();
    /// assert_eq!(t.selected_path(), None);
    /// ```
    pub fn clear_selection(&mut self) {
        self.selection = None;
        self.sync_rows();
    }

    /// The visible-row index carrying the roving tabindex.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite::widgets::{TreeNode, TreeView};
    ///
    /// assert_eq!(TreeView::new().roots(vec![TreeNode::new("a")]).focused_index(), 0);
    /// ```
    #[inline]
    pub fn focused_index(&self) -> usize {
        self.focused
    }

    /// The path of the focused row, if any.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite::widgets::{TreeNode, TreeView};
    ///
    /// let t = TreeView::new().roots(vec![TreeNode::new("a")
    ///     .with_children(vec![TreeNode::new("b")])]);
    /// assert_eq!(t.focused_path(), Some(&[0][..]));
    /// ```
    #[inline]
    pub fn focused_path(&self) -> Option<&[usize]> {
        self.flat.get(self.focused).map(|r| r.path.as_slice())
    }

    /// The current clamped vertical scroll offset in device pixels.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite::widgets::TreeView;
    ///
    /// assert_eq!(TreeView::new().scroll_offset(), 0.0);
    /// ```
    #[inline]
    pub fn scroll_offset(&self) -> f32 {
        self.scroll_y
    }

    /// Maximum scroll offset: `max(0, content - viewport)`.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite::widgets::TreeView;
    ///
    /// assert_eq!(TreeView::new().max_scroll_offset(), 0.0);
    /// ```
    #[inline]
    pub fn max_scroll_offset(&self) -> f32 {
        self.max_scroll()
    }

    /// Sets the scroll offset in device pixels, clamped.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite::widgets::TreeView;
    ///
    /// let mut t = TreeView::new();
    /// t.set_scroll_offset(-10.0);
    /// assert_eq!(t.scroll_offset(), 0.0); // clamped
    /// ```
    pub fn set_scroll_offset(&mut self, offset: f32) {
        self.set_scroll(offset);
    }

    /// Scrolls by `delta` device pixels, clamped. Returns the
    /// actually-applied delta — `0.0` means nothing was consumed
    /// (chaining boundary for an ancestor scroll region).
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite::widgets::TreeView;
    ///
    /// let mut t = TreeView::new();
    /// assert_eq!(t.scroll_by(10.0), 0.0); // nothing to scroll
    /// ```
    pub fn scroll_by(&mut self, delta: f32) -> f32 {
        let old = self.scroll_y;
        self.set_scroll(old + delta);
        self.scroll_y - old
    }

    /// Scrolls the minimum amount that makes visible row `index` fully
    /// visible.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite::widgets::{TreeNode, TreeView};
    ///
    /// let mut t = TreeView::new().roots(vec![TreeNode::new("a")]);
    /// t.scroll_row_into_view(0);
    /// ```
    pub fn scroll_row_into_view(&mut self, index: usize) {
        self.ensure_visible(index);
    }

    /// The range of flattened-row indices intersecting the viewport —
    /// the window the row pool materializes.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite::widgets::{TreeNode, TreeView};
    ///
    /// let t = TreeView::new().roots(vec![TreeNode::new("a")]);
    /// assert_eq!(t.visible_range(), 0..0); // not laid out yet
    /// ```
    pub fn visible_range(&self) -> Range<usize> {
        let row_px = self.row_px();
        if row_px <= 0.0 || self.flat.is_empty() || self.viewport.height() <= 0.0 {
            return 0..0;
        }
        let start = self.first_visible();
        let end = ((self.scroll_y + self.viewport.height()) / row_px).ceil() as usize;
        start..end.min(self.flat.len())
    }

    /// Returns the activated path once, if any — the out-seam for
    /// `Enter` activations (mirrors `ListView::take_activated`).
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite::widgets::{TreeNode, TreeView};
    ///
    /// let mut t = TreeView::new().roots(vec![TreeNode::new("a")]);
    /// assert_eq!(t.take_activated(), None);
    /// ```
    pub fn take_activated(&mut self) -> Option<Vec<usize>> {
        self.activated.take()
    }

    /// Applies pending presses/focus/expansion requests recorded by
    /// row children and scroll requests parked by the scrollbar (AT
    /// actions delivered through `WidgetArena::internal_widget_mut`).
    ///
    /// Called automatically from `event`, `layout`, and `a11y_prepare`.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite::widgets::{TreeNode, TreeView};
    /// use martensite_core::widget::Widget;
    ///
    /// let mut t = TreeView::new().roots(vec![TreeNode::new("a")]);
    /// t.poll_pending();
    /// ```
    pub fn poll_pending(&mut self) {
        let mut press = None;
        let mut focus = None;
        let mut expand = None;
        for row in &mut self.rows {
            if row.press_pending {
                row.press_pending = false;
                press = Some(row.flat_index);
            } else if row.focus_pending {
                row.focus_pending = false;
                focus = Some(row.flat_index);
            }
            if let Some(v) = row.expand_request.take() {
                expand = Some((row.flat_index, v));
            }
        }
        if let Some((i, v)) = expand {
            if let Some(path) = self.flat.get(i).map(|r| r.path.clone()) {
                self.set_expanded(&path, v);
            }
        }
        if let Some(i) = press {
            self.select_row(i);
        } else if let Some(i) = focus {
            // Focus without selection (a press also moves focus).
            self.focused = i.min(self.flat.len().saturating_sub(1));
            self.ensure_visible(self.focused);
            self.sync_rows();
        }
        if let Some(req) = self.vbar.pending.take() {
            match req {
                BarRequest::By(d) => {
                    self.scroll_by(d);
                }
                BarRequest::To(o) => {
                    self.set_scroll(o);
                }
            }
        }
    }

    /// Row height in device pixels at the cached display scale.
    fn row_px(&self) -> f32 {
        self.row_height * self.scale
    }

    /// Full content height in device pixels.
    fn content_height(&self) -> f32 {
        self.flat.len() as f32 * self.row_px()
    }

    /// Maximum scroll offset.
    fn max_scroll(&self) -> f32 {
        (self.content_height() - self.viewport.height()).max(0.0)
    }

    /// First fully-or-partially visible row index.
    fn first_visible(&self) -> usize {
        let row_px = self.row_px();
        if row_px <= 0.0 || self.flat.is_empty() {
            return 0;
        }
        ((self.scroll_y / row_px).floor() as usize).min(self.flat.len() - 1)
    }

    /// How many pooled rows cover the viewport (plus one partial row).
    fn visible_capacity(&self) -> usize {
        let row_px = self.row_px();
        if row_px <= 0.0 || self.viewport.height() <= 0.0 {
            return 0;
        }
        (self.viewport.height() / row_px).ceil() as usize + 1
    }

    /// The screen-space rect of visible row `index`.
    fn row_rect(&self, index: usize) -> Rect {
        let row_px = self.row_px();
        Rect::new(
            self.viewport.min_x(),
            self.viewport.min_y() + index as f32 * row_px - self.scroll_y,
            self.viewport.width(),
            row_px,
        )
    }

    /// Visible-row index under `position` (viewport-space hit test).
    fn row_at(&self, position: Vec2) -> Option<usize> {
        let row_px = self.row_px();
        if row_px <= 0.0 || !self.viewport.contains(position) {
            return None;
        }
        let i = ((position.y - self.viewport.min_y() + self.scroll_y) / row_px) as usize;
        (i < self.flat.len()).then_some(i)
    }

    /// The disclosure-triangle hit box of visible row `index` — only
    /// meaningful when the row has children.
    fn triangle_rect(&self, index: usize) -> Option<Rect> {
        let row = self.flat.get(index)?;
        if !row.has_children {
            return None;
        }
        let row_rect = self.row_rect(index);
        let indent_px = self.indent * self.scale * row.depth as f32;
        let tri_px = TRI * self.scale;
        Some(Rect::new(
            row_rect.min_x() + indent_px,
            row_rect.min_y() + (row_rect.height() - tri_px) / 2.0,
            tri_px,
            tri_px,
        ))
    }

    /// Flattened-row index of `path`, if it is currently visible.
    fn flat_index_of(&self, path: &[usize]) -> Option<usize> {
        self.flat.iter().position(|r| r.path == path)
    }

    /// Sets the scroll offset, clamped; mirrors state onto rows/bars.
    fn set_scroll(&mut self, offset: f32) {
        let clamped = if offset.is_finite() { offset } else { 0.0 };
        self.scroll_y = clamped.clamp(0.0, self.max_scroll());
        self.sync_rows();
        self.sync_bars();
    }

    /// Scrolls the minimum amount that makes visible row `index` fully
    /// visible.
    fn ensure_visible(&mut self, index: usize) {
        let row_px = self.row_px();
        if row_px <= 0.0 {
            return;
        }
        let top = index as f32 * row_px;
        let bottom = top + row_px;
        if top < self.scroll_y {
            self.set_scroll(top);
        } else if bottom > self.scroll_y + self.viewport.height() {
            self.set_scroll(bottom - self.viewport.height());
        }
    }

    /// Keyboard navigation: move the roving tabindex to visible row
    /// `index` (clamped), select it, and keep it visible.
    fn move_focus_to(&mut self, index: usize) {
        if self.flat.is_empty() {
            return;
        }
        self.select_row(index.min(self.flat.len() - 1));
    }

    /// Rows moved by `PageUp`/`PageDown`.
    fn page_size(&self) -> usize {
        let row_px = self.row_px();
        if row_px <= 0.0 {
            return 1;
        }
        ((self.viewport.height() / row_px).floor() as usize).max(1)
    }

    /// Records an activation for [`take_activated`](Self::take_activated)
    /// and the optional sink.
    fn activate(&mut self, index: usize) {
        if let Some(row) = self.flat.get(index) {
            let path = row.path.clone();
            self.activated = Some(path.clone());
            if let Some(ref sink) = self.activated_sink {
                *sink.lock().expect("activated sink poisoned") = Some(path);
            }
        }
    }

    /// The vertical scrollbar thumb rect, if the bar is shown.
    fn vbar_thumb(&self) -> Option<Rect> {
        let track = self.vbar_rect?;
        let content = self.content_height();
        if content <= 0.0 {
            return None;
        }
        let track_len = track.height();
        let frac = (self.viewport.height() / content).clamp(0.0, 1.0);
        let thumb_len = (track_len * frac)
            .max(MIN_THUMB * self.scale)
            .min(track_len);
        let max = self.max_scroll();
        let t = if max > 0.0 { self.scroll_y / max } else { 0.0 };
        let top = track.min_y() + t * (track_len - thumb_len);
        Some(Rect::new(track.min_x(), top, track.width(), thumb_len))
    }

    /// Maps a pointer position inside the bar track to a thumb grab or
    /// a page scroll. Returns `true` if the press was consumed.
    fn press_bar(&mut self, position: Vec2) -> bool {
        let Some(thumb) = self.vbar_thumb() else {
            return false;
        };
        if thumb.contains(position) {
            self.thumb_drag = Some(position.y - thumb.min_y());
        } else {
            // Track press: page toward the click.
            let sign = if position.y < thumb.min_y() {
                -1.0
            } else {
                1.0
            };
            self.scroll_by(sign * self.viewport.height() * 0.9);
        }
        true
    }

    /// Continues an active thumb drag.
    fn drag_thumb(&mut self, position: Vec2) {
        let Some(grab) = self.thumb_drag else {
            return;
        };
        let Some(track) = self.vbar_rect else {
            return;
        };
        let thumb_len = self.vbar_thumb().map(|t| t.height()).unwrap_or(0.0);
        let usable = (track.height() - thumb_len).max(f32::EPSILON);
        let frac = ((position.y - track.min_y() - grab) / usable).clamp(0.0, 1.0);
        self.set_scroll(frac * self.max_scroll());
    }

    /// Rebuilds the flattened visible-row projection from `roots`,
    /// skipping collapsed subtrees.
    fn rebuild_flat(&mut self) {
        self.flat.clear();
        flatten_into(&self.roots, &mut self.flat);
    }

    /// Clamps focus/scroll into the current projection and mirrors
    /// state onto children.
    fn clamp_state(&mut self) {
        self.focused = self.focused.min(self.flat.len().saturating_sub(1));
        self.hovered = self.hovered.filter(|&i| i < self.flat.len());
        self.scroll_y = self.scroll_y.clamp(0.0, self.max_scroll());
        self.sync_rows();
        self.sync_bars();
    }

    /// Resizes/re-points the pooled row children at the visible window
    /// and mirrors owner state onto them.
    fn sync_rows(&mut self) {
        let first = self.first_visible();
        let count = self
            .flat
            .len()
            .saturating_sub(first)
            .min(self.visible_capacity());
        self.rows.resize_with(count, TreeItemRow::new);
        for (k, row) in self.rows.iter_mut().enumerate() {
            let i = first + k;
            let Some(flat) = self.flat.get(i) else {
                continue;
            };
            row.flat_index = i;
            row.label = flat.label.clone();
            row.depth = flat.depth;
            row.has_children = flat.has_children;
            row.expanded = flat.expanded;
            row.selected = self.selection.as_deref() == Some(flat.path.as_slice());
            row.focused = i == self.focused;
            row.has_focus = self.has_focus;
            row.hovered = self.hovered == Some(i);
            row.enabled = self.enabled;
            row.text_painter = self.text_painter.clone();
        }
    }

    /// Mirrors scroll state onto the scrollbar child for its emitted
    /// `ScrollBar` node and thumb painting.
    fn sync_bars(&mut self) {
        self.vbar.offset = self.scroll_y;
        self.vbar.max_offset = self.max_scroll();
        self.vbar.thumb = self.vbar_thumb();
        self.vbar.active = self.thumb_drag.is_some();
    }
}

/// Walks `nodes` depth-first, appending a [`FlatRow`] per node and
/// recursing only into expanded subtrees.
fn flatten_into(nodes: &[TreeNode], out: &mut Vec<FlatRow>) {
    fn walk(node: &TreeNode, path: &mut Vec<usize>, depth: usize, out: &mut Vec<FlatRow>) {
        out.push(FlatRow {
            path: path.clone(),
            depth,
            has_children: !node.children.is_empty(),
            expanded: node.expanded,
            label: node.label.clone(),
        });
        if node.expanded {
            for (i, child) in node.children.iter().enumerate() {
                path.push(i);
                walk(child, path, depth + 1, out);
                path.pop();
            }
        }
    }
    let mut path = Vec::new();
    for (i, node) in nodes.iter().enumerate() {
        path.push(i);
        walk(node, &mut path, 0, out);
        path.pop();
    }
}

impl Default for TreeView {
    fn default() -> Self {
        Self::new()
    }
}

impl Widget for TreeView {
    fn measure(&mut self, cx: &mut LayoutContext, constraints: LayoutConstraints) -> Vec2 {
        let row_px = cx.pt(self.row_height);
        let content_h = self.flat.len() as f32 * row_px;
        let h = content_h.min(MAX_VISIBLE_ROWS * row_px);
        // Width follows the widest label plus its depth indent, the
        // disclosure gutter, and the potential scrollbar.
        let widest = self
            .flat
            .iter()
            .map(|r| {
                r.label.chars().count() as f32 * cx.pt(7.0)
                    + r.depth as f32 * cx.pt(INDENT)
                    + cx.pt(TRI + TRI_GAP)
            })
            .fold(0.0f32, f32::max);
        let w = widest + cx.pt(16.0) + cx.pt(BAR);
        // `clamp` panics when min > max — cap the preferred minimum at
        // the constraint max so zero-constraint probes stay safe.
        let max_w = constraints.max_size.x.max(0.0);
        let max_h = constraints.max_size.y.max(0.0);
        Vec2::new(
            w.clamp(cx.pt(80.0).min(max_w), max_w),
            h.clamp(cx.pt(48.0).min(max_h), max_h),
        )
    }

    fn min_render(&self) -> RenderMinimum {
        // The 48×48pt floor `measure` requests — a couple of rows plus
        // the scrollbar strip.
        RenderMinimum::new(Vec2::new(48.0, 48.0)).with_policy(UnderflowPolicy::Lint)
    }

    fn layout(&mut self, cx: &mut LayoutContext, bounds: Rect) {
        self.cached_bounds = bounds;
        self.scale = cx.scale;
        self.vbar.scale = cx.scale;
        // Declare keyboard focusability on the arena node — the tree
        // is a single tab stop (roving tabindex over the rows).
        if self.enabled {
            cx.hot.flags |= NodeFlags::FOCUSABLE;
        } else {
            cx.hot.flags.remove(NodeFlags::FOCUSABLE);
        }
        self.poll_pending();
        // Smart scrollbar: shown only when content overflows.
        let bar = cx.pt(BAR);
        let show_v = self.content_height() > bounds.height();
        let mut viewport = bounds;
        self.vbar_rect = None;
        if show_v {
            viewport.size.x = (viewport.width() - bar).max(0.0);
            self.vbar_rect = Some(Rect::new(
                bounds.max_x() - bar,
                bounds.min_y(),
                bar,
                bounds.height(),
            ));
        }
        self.viewport = viewport;
        self.vbar.shown = show_v;
        self.scroll_y = self.scroll_y.clamp(0.0, self.max_scroll());
        self.sync_rows();
        // Lay out the pooled rows so the child's layout pass runs with
        // current bounds (`child_bounds` derives them arithmetically).
        let row_px = cx.pt(self.row_height);
        let first = self.first_visible();
        for (k, row) in self.rows.iter_mut().enumerate() {
            let rect = Rect::new(
                viewport.min_x(),
                viewport.min_y() + (first + k) as f32 * row_px - self.scroll_y,
                viewport.width(),
                row_px,
            );
            cx.layout_child(row, rect);
        }
        if let Some(rect) = self.vbar_rect {
            cx.layout_child(&mut self.vbar, rect);
        }
        self.sync_bars();
    }

    fn clips_children(&self) -> bool {
        true
    }

    fn accessibility(&self, node: &mut AccessKitNode) {
        node.set_role(accesskit::Role::Tree);
        node.set_orientation(accesskit::Orientation::Vertical);
        // `size_of_set` lives on the container (unlike ARIA's
        // per-item `aria-setsize`); items carry `position_in_set`.
        node.set_size_of_set(self.flat.len());
        if let Some(ref label) = self.label {
            node.set_label(label.as_str());
        }
        let max = self.max_scroll();
        node.set_scroll_y(f64::from(self.scroll_y));
        node.set_scroll_y_min(0.0);
        node.set_scroll_y_max(f64::from(max));
        node.add_action(accesskit::Action::ScrollUp);
        node.add_action(accesskit::Action::ScrollDown);
        node.add_action(accesskit::Action::SetScrollOffset);
        node.add_action(accesskit::Action::Focus);
        node.add_child_action(accesskit::Action::ScrollIntoView);
        if !self.enabled {
            node.set_disabled();
        }
    }

    fn a11y_prepare(&mut self) {
        self.poll_pending();
    }

    fn event(&mut self, cx: &mut EventContext) -> EventResponse {
        if !self.enabled {
            return EventResponse::Ignored;
        }
        self.poll_pending();
        match cx.event {
            WidgetEvent::PointerPressed {
                position,
                button: PointerButton::Primary,
                count,
            } => {
                if self.vbar_rect.is_some_and(|r| r.contains(*position)) {
                    self.press_bar(*position);
                    return EventResponse::CapturePointer;
                }
                if let Some(i) = self.row_at(*position) {
                    // The disclosure triangle toggles without moving
                    // the selection — the platform-tree convention.
                    if *count == 1
                        && self
                            .triangle_rect(i)
                            .is_some_and(|r| r.contains(*position))
                    {
                        let path = self.flat[i].path.clone();
                        self.toggle(&path);
                        return EventResponse::CaptureFocus;
                    }
                    self.select_row(i);
                    if *count >= 2 && self.flat[i].has_children {
                        let path = self.flat[i].path.clone();
                        self.toggle(&path);
                    }
                    return EventResponse::CaptureFocus;
                }
                EventResponse::Ignored
            }
            WidgetEvent::PointerMoved { position } => {
                if self.thumb_drag.is_some() {
                    self.drag_thumb(*position);
                    return EventResponse::RequestRepaint;
                }
                let hov = self.row_at(*position);
                if hov != self.hovered {
                    self.hovered = hov;
                    self.sync_rows();
                    return EventResponse::RequestRepaint;
                }
                EventResponse::Ignored
            }
            WidgetEvent::PointerReleased {
                button: PointerButton::Primary,
                ..
            } => {
                if self.thumb_drag.is_some() {
                    self.thumb_drag = None;
                    self.sync_bars();
                    return EventResponse::ReleasePointer;
                }
                EventResponse::Ignored
            }
            WidgetEvent::PointerLeave => {
                if self.hovered.take().is_some() {
                    self.sync_rows();
                    return EventResponse::RequestRepaint;
                }
                EventResponse::Ignored
            }
            WidgetEvent::Scroll { delta, .. } => {
                // Nested chaining: an unconsumed delta returns `Ignored`
                // so an ancestor scroll region can take it.
                let applied = self.scroll_by(delta.y);
                if applied != 0.0 {
                    EventResponse::RequestRepaint
                } else {
                    EventResponse::Ignored
                }
            }
            WidgetEvent::KeyPressed { key, .. } => match key.as_str() {
                "ArrowUp" => {
                    self.move_focus_to(self.focused.saturating_sub(1));
                    EventResponse::RequestRepaint
                }
                "ArrowDown" => {
                    self.move_focus_to(self.focused.saturating_add(1));
                    EventResponse::RequestRepaint
                }
                "ArrowRight" => {
                    if let Some(row) = self.flat.get(self.focused) {
                        if row.has_children && !row.expanded {
                            let path = row.path.clone();
                            self.set_expanded(&path, true);
                        } else if row.has_children {
                            // Already expanded: descend to the first
                            // child (the next visible row).
                            self.move_focus_to(self.focused + 1);
                        }
                        // APG: Right on a leaf does nothing.
                    }
                    EventResponse::RequestRepaint
                }
                "ArrowLeft" => {
                    if let Some(row) = self.flat.get(self.focused) {
                        if row.has_children && row.expanded {
                            let path = row.path.clone();
                            self.set_expanded(&path, false);
                        } else if row.depth > 0 {
                            // On a leaf/collapsed row: ascend to the
                            // parent row.
                            let parent = row.path[..row.path.len() - 1].to_vec();
                            if let Some(i) = self.flat_index_of(&parent) {
                                self.move_focus_to(i);
                            }
                        }
                    }
                    EventResponse::RequestRepaint
                }
                "PageUp" => {
                    self.move_focus_to(self.focused.saturating_sub(self.page_size()));
                    EventResponse::RequestRepaint
                }
                "PageDown" => {
                    self.move_focus_to(self.focused.saturating_add(self.page_size()));
                    EventResponse::RequestRepaint
                }
                "Home" => {
                    self.move_focus_to(0);
                    EventResponse::RequestRepaint
                }
                "End" => {
                    self.move_focus_to(self.flat.len().saturating_sub(1));
                    EventResponse::RequestRepaint
                }
                "Enter" => {
                    self.activate(self.focused);
                    EventResponse::Handled
                }
                " " | "Space" => {
                    // Space toggles the focused node (the outline-view
                    // convention) when it has children.
                    if let Some(row) = self.flat.get(self.focused) {
                        if row.has_children {
                            let path = row.path.clone();
                            self.toggle(&path);
                            return EventResponse::RequestRepaint;
                        }
                    }
                    EventResponse::Ignored
                }
                _ => EventResponse::Ignored,
            },
            WidgetEvent::FocusGained => {
                self.has_focus = true;
                self.sync_rows();
                EventResponse::RequestRepaint
            }
            WidgetEvent::FocusLost => {
                self.has_focus = false;
                self.sync_rows();
                EventResponse::RequestRepaint
            }
            WidgetEvent::SemanticAction(action) => match action {
                SemanticAction::Focus => EventResponse::CaptureFocus,
                SemanticAction::Click => {
                    self.activate(self.focused);
                    EventResponse::Handled
                }
                SemanticAction::Expand => {
                    if let Some(row) = self.flat.get(self.focused) {
                        let path = row.path.clone();
                        self.set_expanded(&path, true);
                    }
                    EventResponse::RequestRepaint
                }
                SemanticAction::Collapse => {
                    if let Some(row) = self.flat.get(self.focused) {
                        let path = row.path.clone();
                        self.set_expanded(&path, false);
                    }
                    EventResponse::RequestRepaint
                }
                SemanticAction::ScrollUp => {
                    self.scroll_by(-self.row_px());
                    EventResponse::RequestRepaint
                }
                SemanticAction::ScrollDown => {
                    self.scroll_by(self.row_px());
                    EventResponse::RequestRepaint
                }
                SemanticAction::SetScrollOffset(offset) => {
                    self.set_scroll(offset.y);
                    EventResponse::RequestRepaint
                }
                SemanticAction::ScrollToPoint(point) => {
                    // Point is in this widget's coordinate space.
                    let i = ((point.y - self.viewport.min_y() + self.scroll_y)
                        / self.row_px().max(f32::EPSILON))
                        .max(0.0) as usize;
                    self.ensure_visible(i.min(self.flat.len().saturating_sub(1)));
                    EventResponse::RequestRepaint
                }
                SemanticAction::ScrollIntoView => {
                    self.ensure_visible(self.focused);
                    EventResponse::RequestRepaint
                }
                _ => EventResponse::Ignored,
            },
            _ => EventResponse::Ignored,
        }
    }

    fn paint(&self, cx: &mut PaintContext) {
        // Chrome: the tree face. Rows and the scrollbar paint through
        // the child walk, clipped to the widget bounds by
        // `clips_children`.
        let b = cx.bounds;
        let rect = kurbo::Rect::new(
            f64::from(b.min_x()),
            f64::from(b.min_y()),
            f64::from(b.max_x()),
            f64::from(b.max_y()),
        );
        cx.list
            .push_fill_rect(rect, cx.color(TokenKey::SurfaceColor, SURFACE_BG));
        cx.list
            .push_stroke_rect(rect, cx.pt(1.0), cx.color(TokenKey::BorderColor, BORDER));
    }

    fn child_count(&self) -> usize {
        self.rows.len() + 1
    }

    fn child(&self, index: usize) -> Option<&dyn Widget> {
        if index < self.rows.len() {
            self.rows.get(index).map(|r| r as &dyn Widget)
        } else if index == self.rows.len() {
            Some(&self.vbar)
        } else {
            None
        }
    }

    fn child_mut(&mut self, index: usize) -> Option<&mut dyn Widget> {
        if index < self.rows.len() {
            self.rows.get_mut(index).map(|r| r as &mut dyn Widget)
        } else if index == self.rows.len() {
            Some(&mut self.vbar)
        } else {
            None
        }
    }

    fn child_bounds(&self, index: usize) -> Option<Rect> {
        if index < self.rows.len() {
            self.rows.get(index).map(|r| self.row_rect(r.flat_index))
        } else if index == self.rows.len() {
            self.vbar_rect
        } else {
            None
        }
    }
}

impl std::fmt::Debug for TreeView {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("TreeView")
            .field("visible_rows", &self.flat.len())
            .field("selected", &self.selection)
            .field("focused", &self.focused)
            .field("scroll_y", &self.scroll_y)
            .field("enabled", &self.enabled)
            .finish()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use martensite_core::HotNode;

    fn sample() -> TreeView {
        // root0 (a, b(c))  root1 (d)
        TreeView::new().roots(vec![
            TreeNode::new("root0").with_children(vec![
                TreeNode::new("a"),
                TreeNode::new("b").with_children(vec![TreeNode::new("c")]),
            ]),
            TreeNode::new("root1").with_children(vec![TreeNode::new("d")]),
        ])
    }

    fn laid_out(tree: &mut TreeView, w: f32, h: f32) {
        let mut hot = HotNode::default();
        let mut cx = LayoutContext {
            hot: &mut hot,
            scale: 1.0,
        };
        tree.layout(&mut cx, Rect::new(0.0, 0.0, w, h));
    }

    fn key(k: &str) -> WidgetEvent {
        WidgetEvent::KeyPressed {
            key: k.to_string(),
            repeat: false,
        }
    }

    fn event(t: &mut TreeView, ev: &WidgetEvent) -> EventResponse {
        let mut cx = EventContext {
            event: ev,
            bounds: t.cached_bounds,
            scale: 1.0,
        };
        t.event(&mut cx)
    }

    #[test]
    fn flattens_expanded_subtrees() {
        let t = sample();
        // root0, a, b, c, root1, d — all expanded by default.
        assert_eq!(t.visible_row_count(), 6);
        assert_eq!(t.flat[0].depth, 0);
        assert_eq!(t.flat[3].path, vec![0, 1, 0]); // "c"
        assert_eq!(t.flat[3].depth, 2);
    }

    #[test]
    fn collapse_hides_descendants() {
        let mut t = sample();
        t.set_expanded(&[0], false);
        // root0, root1, d.
        assert_eq!(t.visible_row_count(), 3);
        t.set_expanded(&[0], true);
        assert_eq!(t.visible_row_count(), 6);
        // Toggling a leaf is a no-op.
        t.toggle(&[0, 0]);
        assert_eq!(t.visible_row_count(), 6);
    }

    #[test]
    fn select_by_path_and_row() {
        let mut t = sample();
        t.select_path(&[0, 1, 0]);
        assert_eq!(t.selected_path(), Some(&[0, 1, 0][..]));
        t.select_row(0);
        assert_eq!(t.selected_path(), Some(&[0][..]));
        t.clear_selection();
        assert_eq!(t.selected_path(), None);
    }

    #[test]
    fn arrows_move_focus_and_select() {
        let mut t = sample();
        laid_out(&mut t, 240.0, 200.0);
        event(&mut t, &key("ArrowDown"));
        assert_eq!(t.focused_index(), 1);
        assert_eq!(t.selected_path(), Some(&[0, 0][..]));
        event(&mut t, &key("End"));
        assert_eq!(t.focused_index(), 5);
        assert_eq!(t.selected_path(), Some(&[1, 0][..]));
        event(&mut t, &key("Home"));
        assert_eq!(t.focused_index(), 0);
    }

    #[test]
    fn right_expands_then_descends() {
        let mut t = sample();
        t.set_expanded(&[0, 1], false); // collapse "b"
        laid_out(&mut t, 240.0, 200.0);
        t.select_path(&[0, 1]);
        // Collapsed parent: Right expands.
        event(&mut t, &key("ArrowRight"));
        assert_eq!(t.is_expanded(&[0, 1]), Some(true));
        assert_eq!(t.focused_index(), 2);
        // Now expanded: Right descends to the first child.
        event(&mut t, &key("ArrowRight"));
        assert_eq!(t.focused_index(), 3);
        assert_eq!(t.selected_path(), Some(&[0, 1, 0][..]));
        // Leaf: Right is a no-op.
        event(&mut t, &key("ArrowRight"));
        assert_eq!(t.focused_index(), 3);
    }

    #[test]
    fn left_collapses_then_ascends() {
        let mut t = sample();
        laid_out(&mut t, 240.0, 200.0);
        t.select_path(&[0, 1]);
        // Expanded parent: Left collapses.
        event(&mut t, &key("ArrowLeft"));
        assert_eq!(t.is_expanded(&[0, 1]), Some(false));
        // Now collapsed: Left ascends to the parent.
        event(&mut t, &key("ArrowLeft"));
        assert_eq!(t.selected_path(), Some(&[0][..]));
    }

    #[test]
    fn triangle_click_toggles_without_selecting() {
        let mut t = sample();
        laid_out(&mut t, 240.0, 200.0);
        t.select_path(&[1]);
        // Row 0's triangle sits in the left gutter of the first row.
        let tri = t.triangle_rect(0).expect("root0 has children");
        let press = WidgetEvent::PointerPressed {
            position: Vec2::new(tri.min_x() + tri.width() / 2.0, tri.min_y() + 2.0),
            button: PointerButton::Primary,
            count: 1,
        };
        event(&mut t, &press);
        assert_eq!(t.is_expanded(&[0]), Some(false));
        // Selection unchanged.
        assert_eq!(t.selected_path(), Some(&[1][..]));
    }

    #[test]
    fn row_click_selects_and_double_click_toggles() {
        let mut t = sample();
        laid_out(&mut t, 240.0, 200.0);
        // Click the label area of row 0 (past the triangle).
        let press = |count: u8| WidgetEvent::PointerPressed {
            position: Vec2::new(120.0, 12.0),
            button: PointerButton::Primary,
            count,
        };
        assert_eq!(event(&mut t, &press(1)), EventResponse::CaptureFocus);
        assert_eq!(t.selected_path(), Some(&[0][..]));
        event(&mut t, &press(2));
        assert_eq!(t.is_expanded(&[0]), Some(false));
    }

    #[test]
    fn enter_activates_path() {
        let mut t = sample();
        laid_out(&mut t, 240.0, 200.0);
        t.select_path(&[0, 1]);
        event(&mut t, &key("Enter"));
        assert_eq!(t.take_activated(), Some(vec![0, 1]));
        assert_eq!(t.take_activated(), None);
    }

    #[test]
    fn wheel_scrolls_virtualized_rows() {
        // 100 roots → tall content in a short viewport.
        let mut t = TreeView::new().roots(
            (0..100)
                .map(|i| TreeNode::new(format!("n{i}")))
                .collect(),
        );
        laid_out(&mut t, 240.0, 96.0);
        assert_eq!(t.visible_range(), 0..4);
        let ev = WidgetEvent::Scroll {
            position: Vec2::new(10.0, 10.0),
            delta: Vec2::new(0.0, 48.0),
        };
        assert_eq!(event(&mut t, &ev), EventResponse::RequestRepaint);
        assert_eq!(t.scroll_offset(), 48.0);
        assert_eq!(t.visible_range().start, 2);
        t.set_scroll_offset(t.max_scroll_offset());
        assert_eq!(event(&mut t, &ev), EventResponse::Ignored);
    }

    #[test]
    fn collapse_of_scrolled_tree_clamps() {
        let mut t = TreeView::new().roots(
            (0..100)
                .map(|i| {
                    TreeNode::new(format!("n{i}"))
                        .with_children(vec![TreeNode::new("c")])
                })
                .collect(),
        );
        laid_out(&mut t, 240.0, 96.0);
        t.set_scroll_offset(t.max_scroll_offset());
        t.collapse_all();
        assert_eq!(t.visible_row_count(), 100);
        assert!(t.scroll_offset() <= t.max_scroll_offset());
    }

    #[test]
    fn tree_accessibility_contract() {
        let mut t = sample().label("Files");
        laid_out(&mut t, 240.0, 200.0);
        t.select_path(&[0, 1]);
        let mut node = AccessKitNode::new(accesskit::Role::Unknown);
        t.accessibility(&mut node);
        assert_eq!(node.role(), accesskit::Role::Tree);
        assert_eq!(node.label(), Some("Files"));
        assert_eq!(node.size_of_set(), Some(6));
        assert!(node.supports_action(accesskit::Action::Focus));

        // Row children emit Role::TreeItem with level/expanded/selected.
        let mut item = AccessKitNode::new(accesskit::Role::Unknown);
        t.child(2).unwrap().accessibility(&mut item); // "b" at depth 1
        assert_eq!(item.role(), accesskit::Role::TreeItem);
        assert_eq!(item.level(), Some(1));
        assert_eq!(item.is_expanded(), Some(true));
        assert_eq!(item.is_selected(), Some(true));
        assert_eq!(item.position_in_set(), Some(2));
        assert!(item.supports_action(accesskit::Action::Collapse));
    }

    #[test]
    fn pending_expand_from_row_child() {
        let mut t = sample();
        laid_out(&mut t, 240.0, 200.0);
        let ev = WidgetEvent::SemanticAction(SemanticAction::Collapse);
        let row = t.child_mut(0).unwrap(); // root0, expanded
        let mut cx = EventContext {
            event: &ev,
            bounds: Rect::default(),
            scale: 1.0,
        };
        assert_eq!(row.event(&mut cx), EventResponse::Handled);
        t.poll_pending();
        assert_eq!(t.is_expanded(&[0]), Some(false));
    }

    #[test]
    fn disabled_ignores_input() {
        let mut t = sample().enabled(false);
        laid_out(&mut t, 240.0, 200.0);
        assert_eq!(event(&mut t, &key("ArrowDown")), EventResponse::Ignored);
        let press = WidgetEvent::PointerPressed {
            position: Vec2::new(120.0, 12.0),
            button: PointerButton::Primary,
            count: 1,
        };
        assert_eq!(event(&mut t, &press), EventResponse::Ignored);
    }
}
