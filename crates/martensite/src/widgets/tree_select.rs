//! `TreeSelect` widget: a dropdown face whose popup hosts a tree of
//! options — the Ant `TreeSelect` / WinUI tree-combo equivalent.
//!
//! Shares [`Dropdown`](crate::widgets::Dropdown)'s select-only-combobox
//! seam and [`DatePicker`](crate::widgets::DatePicker)'s
//! stateful-popup architecture:
//!
//! - The face emits `Role::ComboBox` with `aria-haspopup="tree"`,
//!   `aria-expanded`, the selected leaf's label as its value, and
//!   `aria-controls` wired to the popup's tree node (via
//!   [`Widget::a11y_fixup`](martensite_core::Widget::a11y_fixup)
//!   against overlay node ids).
//! - The popup lives in the
//!   [`OverlayLayer`](martensite_core::overlay::OverlayLayer) — placed
//!   below the face, flipping above and clamping near the viewport
//!   edge — and hosts a real
//!   [`TreeView`](crate::widgets::TreeView) internal child (emitted as
//!   `Role::Tree` with `Role::TreeItem` rows), so expansion,
//!   virtualization, scrolling, and the full APG tree keyboard model
//!   (`ArrowRight`/`ArrowLeft` expand/collapse, `Home`/`End`,
//!   `PageUp`/`PageDown`) come for free. The overlay offers keys to
//!   the popup first; the tree is `FOCUSABLE`, so arrow navigation and
//!   `Enter` work whether focus sits on the face or the popup.
//! - **Commit**: selecting a *leaf* commits it — click, `Enter`, or
//!   an AT `Click` on a leaf writes the label into the shared channel,
//!   the face drains it in [`TreeSelect::sync_overlay`], stores it as
//!   the value, queues it for [`TreeSelect::take_selected`], and
//!   closes. Selecting a *branch* toggles its expansion instead (the
//!   Ant convention). `Escape` or an outside press light-dismisses
//!   without committing.
//! - The leaf's [`TreeNode::label`](crate::widgets::TreeNode) is its
//!   value — `TreeNode` carries no separate value field, so
//!   [`TreeSelect::selected`] reports the label.
//!
//! # Examples
//!
//! ```
//! use martensite::widgets::{TreeNode, TreeSelect};
//!
//! let ts = TreeSelect::new().tree(vec![
//!     TreeNode::new("Fruit").with_children(vec![TreeNode::new("Apple")]),
//! ]);
//! assert_eq!(ts.selected(), None);
//! assert!(!ts.is_open());
//! ```

use std::sync::{Arc, Mutex};

use accesskit::Node as AccessKitNode;
use glam::Vec2;
use martensite_core::overlay::{OverlayAnchor, OverlayLayer};
use martensite_core::shape::Shape;
use martensite_core::widget::{
    A11yEmittedNode, EventContext, EventResponse, LayoutConstraints, LayoutContext, OverlayA11yRef,
    PaintContext, PointerButton, SemanticAction, Widget, WidgetEvent,
};
use martensite_core::{NodeFlags, Rect, RenderMinimum, TokenKey, UnderflowPolicy};

use crate::widgets::tree_view::{TreeNode, TreeView};

/// Combobox face height (logical points).
const FACE_H: f32 = 32.0;
/// Face background.
const FACE_BG: [u8; 4] = [250, 250, 252, 255];
/// Face border.
const FACE_BORDER: [u8; 4] = [150, 155, 165, 255];
/// Label ink.
const INK: [u8; 4] = [30, 30, 36, 255];
/// Disabled / placeholder ink.
const INK_MUTED: [u8; 4] = [150, 150, 158, 255];
/// Popup background.
const POPUP_BG: [u8; 4] = [252, 252, 254, 255];
/// Popup border.
const POPUP_BORDER: [u8; 4] = [140, 145, 155, 255];
/// Popup border thickness (logical points) — the tree insets inside it.
const POPUP_EDGE: f32 = 1.0;
/// Fallback accessible name when no [`TreeSelect::label`] is set.
const DEFAULT_LABEL: &str = "Tree select";

/// The path of the node carrying `label`, if any — a depth-first
/// search used to focus the committed value when the popup opens and
/// to resolve AT `SetValue` writes.
fn find_label(nodes: &[TreeNode], label: &str) -> Option<Vec<usize>> {
    fn walk(node: &TreeNode, path: &mut Vec<usize>, label: &str) -> Option<Vec<usize>> {
        if node.label == label {
            return Some(path.clone());
        }
        for (i, child) in node.children.iter().enumerate() {
            path.push(i);
            if let Some(found) = walk(child, path, label) {
                return Some(found);
            }
            path.pop();
        }
        None
    }
    let mut path = Vec::new();
    for (i, node) in nodes.iter().enumerate() {
        path.push(i);
        if let Some(found) = walk(node, &mut path, label) {
            return Some(found);
        }
        path.pop();
    }
    None
}

/// Channel between a [`TreeSelect`] and its live [`TreePopup`]: the
/// surface writes a picked leaf label and close requests; the face
/// drains them in [`TreeSelect::sync_overlay`]. The tree model itself
/// is handed to the surface at construction — it evolves (expansion,
/// scroll) for the popup's lifetime and never syncs back.
#[derive(Debug, Default)]
struct TreeChannel {
    /// A leaf the user picked (click, `Enter`, or AT `Click`).
    picked: Option<String>,
    /// The surface asked to close without picking (embedded `Escape`).
    close_requested: bool,
}

/// The popup surface for a [`TreeSelect`] — a bordered
/// `Role::Dialog` chrome wrapping a [`TreeView`] (internal child 0,
/// emitted as `Role::Tree`). The tree owns its expansion/selection
/// state for the popup's lifetime; results flow back through
/// [`TreeChannel`].
struct TreePopup {
    /// The option tree (internal child 0).
    tree: TreeView,
    /// Result channel back to the owning `TreeSelect`.
    channel: Arc<Mutex<TreeChannel>>,
    /// Popup bounds from the last layout pass.
    bounds: Option<Rect>,
    /// The tree's rect — `bounds` minus the border.
    inner: Option<Rect>,
    /// The selection the popup has already reacted to — commit/toggle
    /// fires on *changes*, so the opening selection is seeded here.
    last_selection: Option<Vec<usize>>,
    /// The silhouette painted last frame — the single source of truth
    /// for `clip_shape`/`hit_shape` so clipping and hit-testing can
    /// never diverge from the visible outline.
    painted_shape: Mutex<Shape>,
}

impl TreePopup {
    fn new(
        nodes: Vec<TreeNode>,
        selected: Option<&str>,
        label: Option<String>,
        channel: Arc<Mutex<TreeChannel>>,
        text_painter: Option<crate::text_paint::SharedTextPainter>,
    ) -> Self {
        // Reveal and select the committed value: resolve its path in
        // the source nodes before the `TreeView` takes ownership (it
        // keeps its roots private), then expand every ancestor so
        // `select_path` can see it.
        let selected_path = selected.and_then(|s| find_label(&nodes, s));
        let mut tree = TreeView::new().roots(nodes);
        tree.label = label;
        if let Some(painter) = text_painter {
            tree = tree.with_text_painter(painter);
        }
        if let Some(path) = selected_path {
            for depth in 1..path.len() {
                tree.set_expanded(&path[..depth], true);
            }
            tree.select_path(&path);
        }
        let last_selection = tree.selected_path().map(<[usize]>::to_vec);
        Self {
            tree,
            channel,
            bounds: None,
            inner: None,
            last_selection,
            painted_shape: Mutex::new(Shape::RECT),
        }
    }

    /// Applies the tree-side consequences of an interaction: an
    /// activation (`Enter` on the focused row, or AT `Click` on the
    /// tree) commits a leaf or toggles a branch; a selection *change*
    /// (pointer press, parked AT row clicks resolved by
    /// `poll_pending`) commits a leaf or toggles a branch — the Ant
    /// `TreeSelect` click contract.
    fn sync_from_tree(&mut self) {
        if let Some(path) = self.tree.take_activated() {
            self.commit_or_toggle(&path);
        }
        let sel = self.tree.selected_path().map(<[usize]>::to_vec);
        if sel != self.last_selection {
            self.last_selection = sel.clone();
            if let Some(path) = sel {
                self.commit_or_toggle(&path);
            }
        }
    }

    /// Commits `path`'s label when it is a leaf; toggles expansion
    /// when it is a branch.
    fn commit_or_toggle(&mut self, path: &[usize]) {
        match self.tree.node(path) {
            Some(node) if node.is_leaf() => {
                self.channel.lock().expect("tree channel poisoned").picked =
                    Some(node.label.clone());
            }
            Some(_) => self.tree.toggle(path),
            None => {}
        }
    }
}

impl Widget for TreePopup {
    fn measure(&mut self, cx: &mut LayoutContext, constraints: LayoutConstraints) -> Vec2 {
        let edge = cx.pt(POPUP_EDGE);
        let inner = LayoutConstraints {
            min_size: Vec2::ZERO,
            max_size: (constraints.max_size - Vec2::splat(edge * 2.0)).max(Vec2::ZERO),
        };
        let s = self.tree.measure(cx, inner);
        (s + Vec2::splat(edge * 2.0)).min(constraints.max_size.max(Vec2::ZERO))
    }

    fn layout(&mut self, cx: &mut LayoutContext, bounds: Rect) {
        self.bounds = Some(bounds);
        // The popup is itself keyboard-focusable — the tree owns the
        // full APG tree key model once open (mirrors `CalendarSurface`).
        cx.hot.flags |= NodeFlags::FOCUSABLE;
        let edge = cx.pt(POPUP_EDGE);
        let inner = Rect::new(
            bounds.min_x() + edge,
            bounds.min_y() + edge,
            (bounds.width() - edge * 2.0).max(0.0),
            (bounds.height() - edge * 2.0).max(0.0),
        );
        self.inner = Some(inner);
        cx.layout_child(&mut self.tree, inner);
        // AT row presses are parked on row children and applied by the
        // tree's `poll_pending` during layout — resolve them here so an
        // AT-driven leaf pick lands in the channel without an event.
        self.sync_from_tree();
    }

    fn accessibility(&self, node: &mut AccessKitNode) {
        node.set_role(accesskit::Role::Dialog);
    }

    fn event(&mut self, cx: &mut EventContext) -> EventResponse {
        // `Escape` while the layer isn't intercepting (ownerless
        // embedded use) asks the owner to close. In arena use the
        // OverlayLayer consumes `Escape` first.
        if let WidgetEvent::KeyPressed { key, .. } = cx.event {
            if key == "Escape" {
                self.channel
                    .lock()
                    .expect("tree channel poisoned")
                    .close_requested = true;
                return EventResponse::Handled;
            }
        }
        let mut child_cx = EventContext {
            event: cx.event,
            bounds: self.inner.unwrap_or(cx.bounds),
            scale: cx.scale,
        };
        let response = self.tree.event(&mut child_cx);
        self.sync_from_tree();
        response
    }

    fn paint(&self, cx: &mut PaintContext) {
        let b = cx.bounds;
        let rect = kurbo::Rect::new(
            f64::from(b.min_x()),
            f64::from(b.min_y()),
            f64::from(b.max_x()),
            f64::from(b.max_y()),
        );
        let popup_shape = Shape::rounded(cx.dim(TokenKey::BorderRadius, 6.0));
        *self.painted_shape.lock().expect("popup shape poisoned") = popup_shape.clone();
        cx.list.push_fill_shape(
            rect,
            &popup_shape,
            cx.color(TokenKey::SurfaceColor, POPUP_BG),
        );
        cx.list.push_stroke_shape(
            rect,
            &popup_shape,
            cx.pt(1.0),
            cx.color(TokenKey::BorderColor, POPUP_BORDER),
        );
    }

    fn clips_children(&self) -> bool {
        true
    }

    fn clip_shape(&self) -> Option<Shape> {
        Some(
            self.painted_shape
                .lock()
                .expect("popup shape poisoned")
                .clone(),
        )
    }

    fn hit_shape(&self) -> Option<Shape> {
        Some(
            self.painted_shape
                .lock()
                .expect("popup shape poisoned")
                .clone(),
        )
    }

    fn child_count(&self) -> usize {
        1
    }

    fn child(&self, _index: usize) -> Option<&dyn Widget> {
        Some(&self.tree)
    }

    fn child_mut(&mut self, _index: usize) -> Option<&mut dyn Widget> {
        Some(&mut self.tree)
    }

    fn child_bounds(&self, _index: usize) -> Option<Rect> {
        self.inner
    }
}

/// A select-only combobox whose popup is a tree of options.
///
/// Owns no popup widget itself — [`sync_overlay`](Self::sync_overlay)
/// reconciles an overlay entry each frame so the tree paints above
/// window content and is emitted into the accessibility tree.
///
/// # Examples
///
/// ```
/// use martensite::widgets::{TreeNode, TreeSelect};
///
/// let mut ts = TreeSelect::new().tree(vec![TreeNode::new("a")]);
/// ts.open();
/// assert!(ts.is_open());
/// ts.close();
/// assert!(!ts.is_open());
/// ```
pub struct TreeSelect {
    /// Optional accessible label for the combobox.
    pub label: Option<String>,
    /// Text shown when no leaf is selected.
    pub placeholder: String,
    /// Whether the combobox accepts input.
    pub enabled: bool,
    /// The option tree model — cloned into the popup on open.
    nodes: Vec<TreeNode>,
    /// The committed leaf label.
    selected: Option<String>,
    /// Whether the popup is logically open.
    open: bool,
    /// Overlay entry id of the open popup.
    popup_id: Option<u64>,
    /// Result channel shared with the live surface.
    channel: Arc<Mutex<TreeChannel>>,
    /// One-shot pick awaiting [`TreeSelect::take_selected`].
    selected_pending: Option<String>,
    /// Face bounds from the last layout pass.
    cached_bounds: Rect,
    /// The bounds the live popup was last anchored to — `sync_overlay`
    /// re-anchors when `cached_bounds` moves so an open popup tracks
    /// its face (mirrors `Dropdown`).
    last_anchor: Option<Rect>,
    /// Shared shaped-text painter — `paint` emits real `GlyphRun`s
    /// when set, `DrawText` placeholder boxes otherwise. Propagated to
    /// the popup's tree rows when the surface opens.
    text_painter: Option<crate::text_paint::SharedTextPainter>,
}

impl TreeSelect {
    /// Creates an empty tree select; supply options with
    /// [`tree`](Self::tree).
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite::widgets::TreeSelect;
    ///
    /// let ts = TreeSelect::new();
    /// assert_eq!(ts.selected(), None);
    /// assert!(!ts.is_open());
    /// ```
    pub fn new() -> Self {
        Self {
            label: None,
            placeholder: String::new(),
            enabled: true,
            nodes: Vec::new(),
            selected: None,
            open: false,
            popup_id: None,
            channel: Arc::new(Mutex::new(TreeChannel::default())),
            selected_pending: None,
            cached_bounds: Rect::default(),
            last_anchor: None,
            text_painter: None,
        }
    }

    /// Sets the option tree (builder version).
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite::widgets::{TreeNode, TreeSelect};
    ///
    /// let ts = TreeSelect::new().tree(vec![TreeNode::new("a")]);
    /// assert_eq!(ts.node_count(), 1);
    /// ```
    #[must_use]
    pub fn tree(mut self, nodes: Vec<TreeNode>) -> Self {
        self.set_tree(nodes);
        self
    }

    /// Replaces the option tree (mutable version). A `selected` label
    /// the new tree no longer contains is cleared.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite::widgets::{TreeNode, TreeSelect};
    ///
    /// let mut ts = TreeSelect::new();
    /// ts.set_tree(vec![TreeNode::new("a"), TreeNode::new("b")]);
    /// assert_eq!(ts.node_count(), 2);
    /// ```
    pub fn set_tree(&mut self, nodes: Vec<TreeNode>) {
        self.nodes = nodes;
        if let Some(sel) = self.selected.clone() {
            if find_label(&self.nodes, &sel).is_none() {
                self.selected = None;
            }
        }
        if self.nodes.is_empty() {
            self.open = false;
        }
    }

    /// Sets the accessible label.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite::widgets::TreeSelect;
    ///
    /// let ts = TreeSelect::new().label("Location");
    /// assert_eq!(ts.label.as_deref(), Some("Location"));
    /// ```
    #[inline]
    #[must_use]
    pub fn label(mut self, label: impl Into<String>) -> Self {
        self.label = Some(label.into());
        self
    }

    /// Sets placeholder text shown while no leaf is selected.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite::widgets::TreeSelect;
    ///
    /// let ts = TreeSelect::new().placeholder("Pick…");
    /// assert_eq!(ts.placeholder, "Pick…");
    /// ```
    #[inline]
    #[must_use]
    pub fn placeholder(mut self, text: impl Into<String>) -> Self {
        self.placeholder = text.into();
        self
    }

    /// Sets whether the combobox is enabled.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite::widgets::TreeSelect;
    ///
    /// let ts = TreeSelect::new().enabled(false);
    /// assert!(!ts.enabled);
    /// ```
    #[inline]
    #[must_use]
    pub fn enabled(mut self, enabled: bool) -> Self {
        self.enabled = enabled;
        self
    }

    /// Shares a [`crate::text_paint::TextPainter`] so `paint` emits
    /// real glyph runs instead of `DrawText` placeholder boxes. The
    /// popup's tree rows inherit it when the surface opens.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite::text_paint::shared_painter;
    /// use martensite::widgets::TreeSelect;
    ///
    /// let ts = TreeSelect::new().with_text_painter(shared_painter());
    /// ```
    #[must_use]
    pub fn with_text_painter(mut self, painter: crate::text_paint::SharedTextPainter) -> Self {
        self.text_painter = Some(painter);
        self
    }

    /// The committed leaf label, if any.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite::widgets::{TreeNode, TreeSelect};
    ///
    /// let mut ts = TreeSelect::new().tree(vec![TreeNode::new("a")]);
    /// ts.set_selected(Some("a"));
    /// assert_eq!(ts.selected(), Some("a"));
    /// ```
    #[inline]
    pub fn selected(&self) -> Option<&str> {
        self.selected.as_deref()
    }

    /// Sets the selected leaf label programmatically — labels the
    /// tree does not contain (or that belong to a branch) are
    /// ignored. Does not report through
    /// [`take_selected`](Self::take_selected) (that seam is user picks
    /// only).
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite::widgets::{TreeNode, TreeSelect};
    ///
    /// let mut ts = TreeSelect::new().tree(vec![TreeNode::new("a")]);
    /// ts.set_selected(Some("a"));
    /// assert_eq!(ts.selected(), Some("a"));
    /// ts.set_selected(Some("missing"));
    /// assert_eq!(ts.selected(), Some("a")); // unknown labels ignored
    /// ts.set_selected(None::<String>);
    /// assert_eq!(ts.selected(), None);
    /// ```
    pub fn set_selected(&mut self, selected: Option<impl Into<String>>) {
        match selected.map(Into::into) {
            Some(label) => {
                if let Some(path) = find_label(&self.nodes, &label) {
                    let leaf = path
                        .last()
                        .and_then(|&i| self.node_at(&path).map(|n| (i, n)))
                        .is_some_and(|(_, n)| n.is_leaf());
                    if leaf {
                        self.selected = Some(label);
                    }
                }
            }
            None => self.selected = None,
        }
    }

    /// Drains the leaf the user picked in the popup (click, `Enter`,
    /// or AT `Click`) — `None` when nothing new was picked.
    /// Programmatic [`set_selected`](Self::set_selected) does not feed
    /// this seam.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite::widgets::TreeSelect;
    ///
    /// let mut ts = TreeSelect::new();
    /// assert_eq!(ts.take_selected(), None);
    /// ```
    pub fn take_selected(&mut self) -> Option<String> {
        self.selected_pending.take()
    }

    /// Number of root nodes in the option tree.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite::widgets::{TreeNode, TreeSelect};
    ///
    /// let ts = TreeSelect::new().tree(vec![TreeNode::new("a"), TreeNode::new("b")]);
    /// assert_eq!(ts.node_count(), 2);
    /// ```
    #[inline]
    pub fn node_count(&self) -> usize {
        self.nodes.len()
    }

    /// Whether the popup is logically open.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite::widgets::{TreeNode, TreeSelect};
    ///
    /// let mut ts = TreeSelect::new().tree(vec![TreeNode::new("a")]);
    /// ts.open();
    /// assert!(ts.is_open());
    /// ```
    #[inline]
    pub fn is_open(&self) -> bool {
        self.open
    }

    /// The overlay entry id of the open popup, if any.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite::widgets::TreeSelect;
    ///
    /// assert_eq!(TreeSelect::new().popup_id(), None);
    /// ```
    #[inline]
    pub fn popup_id(&self) -> Option<u64> {
        self.popup_id
    }

    /// Opens the tree popup on the next
    /// [`sync_overlay`](Self::sync_overlay). A no-op while disabled or
    /// when the tree is empty (mirrors `Dropdown::open`).
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite::widgets::{TreeNode, TreeSelect};
    ///
    /// let mut ts = TreeSelect::new().tree(vec![TreeNode::new("a")]);
    /// ts.open();
    /// assert!(ts.is_open());
    /// ```
    pub fn open(&mut self) {
        if !self.enabled || self.nodes.is_empty() {
            return;
        }
        self.open = true;
        let mut channel = self.channel.lock().expect("tree channel poisoned");
        channel.picked = None;
        channel.close_requested = false;
    }

    /// Closes the popup without changing the value.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite::widgets::{TreeNode, TreeSelect};
    ///
    /// let mut ts = TreeSelect::new().tree(vec![TreeNode::new("a")]);
    /// ts.open();
    /// ts.close();
    /// assert!(!ts.is_open());
    /// ```
    pub fn close(&mut self) {
        self.open = false;
    }

    /// The node at `path` in the option tree, if it exists.
    fn node_at(&self, path: &[usize]) -> Option<&TreeNode> {
        let (first, rest) = path.split_first()?;
        let mut node = self.nodes.get(*first)?;
        for &i in rest {
            node = node.children.get(i)?;
        }
        Some(node)
    }

    /// Applies a drained channel state — shared by `sync_overlay` and
    /// `a11y_prepare`.
    fn drain_channel(&mut self) {
        let (picked, close_req) = {
            let mut channel = self.channel.lock().expect("tree channel poisoned");
            (
                channel.picked.take(),
                std::mem::take(&mut channel.close_requested),
            )
        };
        if let Some(label) = picked {
            self.selected = Some(label.clone());
            self.selected_pending = Some(label);
            self.open = false;
        }
        if close_req {
            self.open = false;
        }
    }

    /// Reconciles the overlay with the combobox's open state.
    ///
    /// Call once per frame before `OverlayLayer::layout_pass`:
    ///
    /// - applies a leaf pick or close request made inside the popup;
    /// - opens/closes the tree entry to match
    ///   [`is_open`](Self::is_open);
    /// - notices overlay-level dismissal (outside press, `Escape`) and
    ///   resets `open`/`popup_id`;
    /// - re-anchors a live popup whose face moved.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite::widgets::{TreeNode, TreeSelect};
    /// use martensite_core::overlay::OverlayLayer;
    /// use martensite_core::{HotNode, LayoutContext, Rect, Widget};
    ///
    /// let mut ts = TreeSelect::new().tree(vec![TreeNode::new("a")]);
    /// let mut hot = HotNode::default();
    /// let mut cx = LayoutContext {
    ///     hot: &mut hot,
    ///     scale: 1.0,
    /// };
    /// ts.layout(&mut cx, Rect::new(10.0, 10.0, 160.0, 32.0));
    ///
    /// let mut overlay = OverlayLayer::new();
    /// overlay.set_viewport(Rect::new(0.0, 0.0, 800.0, 600.0));
    /// ts.open();
    /// ts.sync_overlay(&mut overlay);
    /// overlay.layout_pass();
    /// assert_eq!(overlay.len(), 1);
    /// ```
    pub fn sync_overlay(&mut self, overlay: &mut OverlayLayer) {
        self.drain_channel();
        // The layer dismissed our popup (outside press / Escape).
        if let Some(id) = self.popup_id {
            if !overlay.is_open(id) {
                self.popup_id = None;
                self.open = false;
                self.last_anchor = None;
            }
        }
        if self.open && self.popup_id.is_none() {
            let popup = TreePopup::new(
                self.nodes.clone(),
                self.selected.as_deref(),
                self.label.clone(),
                Arc::clone(&self.channel),
                self.text_painter.clone(),
            );
            self.popup_id =
                Some(overlay.open(Box::new(popup), OverlayAnchor::Bounds(self.cached_bounds)));
            self.last_anchor = Some(self.cached_bounds);
        } else if !self.open {
            if let Some(id) = self.popup_id.take() {
                overlay.close(id);
            }
            self.last_anchor = None;
        } else if let Some(id) = self.popup_id {
            // The face moved while open (resize, scale change,
            // relayout, dock rearrange) — re-anchor so the popup
            // tracks it. Guarded on change so a settled popup doesn't
            // re-mark layout every tick.
            if self.last_anchor != Some(self.cached_bounds) {
                overlay.set_anchor(id, OverlayAnchor::Bounds(self.cached_bounds));
                self.last_anchor = Some(self.cached_bounds);
            }
        }
    }
}

impl Default for TreeSelect {
    fn default() -> Self {
        Self::new()
    }
}

impl Widget for TreeSelect {
    fn measure(&mut self, cx: &mut LayoutContext, constraints: LayoutConstraints) -> Vec2 {
        // Approximate face width — real shaping lives in the
        // `martensite-text` pipeline. The widest label anywhere in the
        // tree (roots and descendants) drives it, like `Dropdown`
        // sizing to its widest option.
        fn widest(nodes: &[TreeNode]) -> usize {
            nodes
                .iter()
                .map(|n| n.label.chars().count().max(widest(&n.children)))
                .fold(0, usize::max)
        }
        let w = widest(&self.nodes).max(self.placeholder.chars().count()) as f32 * cx.pt(7.0)
            + cx.pt(48.0);
        // `clamp` panics when min > max — a zero-constraint probe
        // hands us `max_size.x == 0`, so cap the preferred minimum at
        // the max (the same guard `Dropdown::measure` uses).
        let max_w = constraints.max_size.x.max(0.0);
        Vec2::new(
            w.clamp(cx.pt(80.0).min(max_w), max_w),
            cx.pt(FACE_H).min(constraints.max_size.y.max(0.0)),
        )
    }

    fn min_render(&self) -> RenderMinimum {
        // The 80×FACE_H floor `measure` requests — narrower or shorter
        // than this and the face (label + chevron) cannot render legibly.
        RenderMinimum::new(Vec2::new(80.0, FACE_H)).with_policy(UnderflowPolicy::Lint)
    }

    fn layout(&mut self, cx: &mut LayoutContext, bounds: Rect) {
        self.cached_bounds = bounds;
        // Declare keyboard focusability on the arena node.
        if self.enabled {
            cx.hot.flags |= NodeFlags::FOCUSABLE;
        } else {
            cx.hot.flags.remove(NodeFlags::FOCUSABLE);
        }
    }

    fn accessibility(&self, node: &mut AccessKitNode) {
        node.set_role(accesskit::Role::ComboBox);
        node.set_label(self.label.as_deref().unwrap_or(DEFAULT_LABEL));
        node.set_value(
            self.selected
                .clone()
                .unwrap_or_else(|| self.placeholder.clone()),
        );
        node.set_has_popup(accesskit::HasPopup::Tree);
        node.set_expanded(self.open);
        node.add_action(accesskit::Action::Click);
        node.add_action(accesskit::Action::Expand);
        node.add_action(accesskit::Action::Collapse);
        // `SetValue` selects a leaf by label.
        node.add_action(accesskit::Action::SetValue);
        if self.enabled {
            node.add_action(accesskit::Action::Focus);
        }
        if !self.enabled {
            node.set_disabled();
        }
    }

    fn a11y_prepare(&mut self) {
        // An AT-driven pick recorded through the surface lands in the
        // channel — drain it here too so the emitted tree reflects the
        // commit even when the action bypassed `sync_overlay`.
        self.drain_channel();
    }

    fn a11y_fixup(
        &self,
        _emitted: &mut Vec<A11yEmittedNode>,
        overlay_nodes: &[OverlayA11yRef],
        this_node: &mut AccessKitNode,
    ) {
        let Some(popup) = self.popup_id else {
            return;
        };
        // aria-controls → the popup's Tree node (child 0 of the
        // Dialog chrome), falling back to the popup root.
        let tree_id = overlay_nodes
            .iter()
            .find(|r| r.entry == popup && r.path.as_slice() == [0])
            .or_else(|| {
                overlay_nodes
                    .iter()
                    .find(|r| r.entry == popup && r.path.is_empty())
            })
            .map(|r| r.id);
        if let Some(id) = tree_id {
            this_node.set_controls(vec![id]);
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
            } => {
                if self.open {
                    self.close();
                } else {
                    self.open();
                }
                EventResponse::CaptureFocus
            }
            WidgetEvent::KeyPressed { key, .. } => match key.as_str() {
                // Normally unreachable while open — the OverlayLayer
                // consumes `Escape` first. Kept for ownerless-embedded
                // use (mirrors `Dropdown`/`DatePicker`).
                "Escape" => {
                    if self.open {
                        self.close();
                        return EventResponse::RequestRepaint;
                    }
                    EventResponse::Ignored
                }
                "Enter" | " " | "Space" => {
                    if self.open {
                        self.close();
                    } else {
                        self.open();
                    }
                    EventResponse::RequestRepaint
                }
                // Open-only affordances — while open the tree inside
                // the popup owns navigation (the layer offers keys to
                // the popup first), so these fall through.
                "ArrowDown" | "ArrowUp" => {
                    if !self.open {
                        self.open();
                        return EventResponse::RequestRepaint;
                    }
                    EventResponse::Ignored
                }
                _ => EventResponse::Ignored,
            },
            WidgetEvent::SemanticAction(action) => match action {
                SemanticAction::Expand => {
                    self.open();
                    EventResponse::RequestRepaint
                }
                SemanticAction::Collapse => {
                    self.close();
                    EventResponse::RequestRepaint
                }
                SemanticAction::Click => {
                    if self.open {
                        self.close();
                    } else {
                        self.open();
                    }
                    EventResponse::RequestRepaint
                }
                SemanticAction::SetValue(text) => {
                    // Select a leaf by its label; unknown or branch
                    // labels are refused.
                    if find_label(&self.nodes, text)
                        .and_then(|path| self.node_at(&path))
                        .is_some_and(|n| n.is_leaf())
                    {
                        self.selected = Some(text.clone());
                        self.selected_pending = Some(text.clone());
                        self.close();
                        return EventResponse::RequestRepaint;
                    }
                    EventResponse::Ignored
                }
                SemanticAction::Focus => EventResponse::CaptureFocus,
                _ => EventResponse::Ignored,
            },
            _ => EventResponse::Ignored,
        }
    }

    fn sync_overlay(&mut self, overlay: &mut OverlayLayer) {
        // Delegate to the inherent method so `TreeSelect::sync_overlay`
        // and the `Widget` trait seam stay in lock-step.
        TreeSelect::sync_overlay(self, overlay);
    }

    fn paint(&self, cx: &mut PaintContext) {
        let b = cx.bounds;
        let rect = kurbo::Rect::new(
            f64::from(b.min_x()),
            f64::from(b.min_y()),
            f64::from(b.max_x()),
            f64::from(b.max_y()),
        );
        let face = Shape::rounded(cx.dim(TokenKey::BorderRadiusSmall, 3.0));
        cx.list
            .push_fill_shape(rect, &face, cx.color(TokenKey::SurfaceColor, FACE_BG));
        cx.list.push_stroke_shape(
            rect,
            &face,
            cx.pt(1.0),
            cx.color(TokenKey::BorderColor, FACE_BORDER),
        );
        let ink = if self.enabled && self.selected.is_some() {
            cx.color(TokenKey::TextColor, INK)
        } else {
            cx.color(TokenKey::TextMutedColor, INK_MUTED)
        };
        let text = self
            .selected
            .clone()
            .unwrap_or_else(|| self.placeholder.clone());
        let font_px = cx.pt(14.0);
        // Clip the selected label to the face minus the chevron zone —
        // a long label can't spill past the field edge.
        let text_x = b.min_x() + cx.pt(10.0);
        crate::text_paint::paint_label_clipped(
            crate::text_paint::resolve_painter(&self.text_painter, cx.text_painter),
            cx.list,
            kurbo::Rect::new(
                f64::from(text_x),
                f64::from(b.min_y()),
                f64::from(b.max_x() - cx.pt(24.0)),
                f64::from(b.max_y()),
            ),
            kurbo::Point::new(
                f64::from(text_x),
                f64::from(b.min_y() + (b.height() - font_px) / 2.0),
            ),
            &text,
            font_px,
            ink,
        );
        // Disclosure triangle (mirrors `Dropdown`'s chevron).
        let cx_mid = f64::from(b.max_x() - cx.pt(16.0));
        let cy = f64::from(b.min_y() + b.height() / 2.0);
        let tri = kurbo::BezPath::from_vec(vec![
            kurbo::PathEl::MoveTo(kurbo::Point::new(cx_mid - 4.0, cy - 2.0)),
            kurbo::PathEl::LineTo(kurbo::Point::new(cx_mid + 4.0, cy - 2.0)),
            kurbo::PathEl::LineTo(kurbo::Point::new(cx_mid, cy + 3.0)),
            kurbo::PathEl::ClosePath,
        ]);
        cx.list.push_stroke_path(tri, cx.pt(1.6), ink);
    }
}

impl std::fmt::Debug for TreeSelect {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("TreeSelect")
            .field("label", &self.label)
            .field("selected", &self.selected)
            .field("enabled", &self.enabled)
            .field("open", &self.open)
            .field("roots", &self.nodes.len())
            .finish()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use martensite_core::HotNode;

    fn nodes() -> Vec<TreeNode> {
        // Group(Leaf1, Leaf2), Solo.
        vec![
            TreeNode::new("Group")
                .with_children(vec![TreeNode::new("Leaf1"), TreeNode::new("Leaf2")]),
            TreeNode::new("Solo"),
        ]
    }

    fn laid_out(ts: &mut TreeSelect) {
        let mut hot = HotNode::default();
        let mut cx = LayoutContext {
            hot: &mut hot,
            scale: 1.0,
        };
        ts.layout(&mut cx, Rect::new(10.0, 10.0, 160.0, 32.0));
    }

    fn overlay() -> OverlayLayer {
        let mut o = OverlayLayer::new();
        o.set_viewport(Rect::new(0.0, 0.0, 800.0, 600.0));
        o
    }

    fn key(k: &str) -> WidgetEvent {
        WidgetEvent::KeyPressed {
            key: k.to_string(),
            repeat: false,
        }
    }

    fn event(ts: &mut TreeSelect, ev: &WidgetEvent) -> EventResponse {
        let mut cx = EventContext {
            event: ev,
            bounds: ts.cached_bounds,
            scale: 1.0,
        };
        ts.event(&mut cx)
    }

    /// Opens the popup into `o`, returning its entry id and resolved
    /// bounds.
    fn open_popup(ts: &mut TreeSelect, o: &mut OverlayLayer) -> (u64, Rect) {
        ts.open();
        ts.sync_overlay(o);
        o.layout_pass();
        let id = ts.popup_id.expect("popup id");
        (id, o.entry_bounds(id).unwrap())
    }

    /// Presses the popup row `index` (24pt rows inside the 1px edge)
    /// through the layer's real dispatch.
    fn press_row(o: &mut OverlayLayer, popup: Rect, index: usize) -> EventResponse {
        let press = WidgetEvent::PointerPressed {
            position: Vec2::new(
                popup.min_x() + popup.width() / 2.0,
                popup.min_y() + 1.0 + index as f32 * 24.0 + 12.0,
            ),
            button: PointerButton::Primary,
            count: 1,
        };
        o.dispatch_event(&press)
    }

    #[test]
    fn open_close_and_popup_id() {
        let mut ts = TreeSelect::new().tree(nodes());
        laid_out(&mut ts);
        assert!(!ts.is_open());
        let mut o = overlay();
        let (id, _) = open_popup(&mut ts, &mut o);
        assert!(ts.is_open());
        assert_eq!(ts.popup_id, Some(id));
        assert_eq!(o.len(), 1);
        ts.close();
        ts.sync_overlay(&mut o);
        assert_eq!(o.len(), 0);
        assert_eq!(ts.popup_id, None);
    }

    #[test]
    fn empty_tree_refuses_open() {
        let mut ts = TreeSelect::new();
        ts.open();
        assert!(!ts.is_open());
    }

    #[test]
    fn face_press_and_keys_toggle() {
        let mut ts = TreeSelect::new().tree(nodes());
        laid_out(&mut ts);
        let press = WidgetEvent::PointerPressed {
            position: Vec2::new(20.0, 20.0),
            button: PointerButton::Primary,
            count: 1,
        };
        assert_eq!(event(&mut ts, &press), EventResponse::CaptureFocus);
        assert!(ts.is_open());
        event(&mut ts, &key("Escape"));
        assert!(!ts.is_open());
        event(&mut ts, &key("ArrowDown"));
        assert!(ts.is_open());
        ts.close();
        event(&mut ts, &key("Enter"));
        assert!(ts.is_open());
    }

    #[test]
    fn leaf_click_commits_and_closes() {
        let mut ts = TreeSelect::new().tree(nodes());
        laid_out(&mut ts);
        let mut o = overlay();
        let (_, popup) = open_popup(&mut ts, &mut o);
        // Rows: Group(0), Leaf1(1), Leaf2(2), Solo(3).
        press_row(&mut o, popup, 1);
        ts.sync_overlay(&mut o);
        assert_eq!(ts.take_selected(), Some("Leaf1".to_string()));
        assert_eq!(ts.selected(), Some("Leaf1"));
        assert!(!ts.is_open());
        assert_eq!(o.len(), 0);
    }

    #[test]
    fn branch_click_toggles_instead_of_committing() {
        let mut ts = TreeSelect::new().tree(nodes());
        laid_out(&mut ts);
        let mut o = overlay();
        let (_, popup) = open_popup(&mut ts, &mut o);
        // Click the "Group" branch row — it collapses, so what was
        // "Solo" at row 3 moves up to row 1.
        press_row(&mut o, popup, 0);
        ts.sync_overlay(&mut o);
        assert!(ts.is_open());
        assert_eq!(ts.take_selected(), None);
        // Row 1 is now "Solo" — a leaf press there commits it.
        press_row(&mut o, popup, 1);
        ts.sync_overlay(&mut o);
        assert_eq!(ts.take_selected(), Some("Solo".to_string()));
    }

    #[test]
    fn enter_on_focused_leaf_commits() {
        let mut ts = TreeSelect::new().tree(nodes());
        laid_out(&mut ts);
        let mut o = overlay();
        open_popup(&mut ts, &mut o);
        // Keys go to the popup's tree: focus starts on row 0 (Group);
        // ArrowDown lands on Leaf1, Enter commits it.
        assert_eq!(
            o.dispatch_event(&key("ArrowDown")),
            EventResponse::RequestRepaint
        );
        assert_eq!(o.dispatch_event(&key("Enter")), EventResponse::Handled);
        ts.sync_overlay(&mut o);
        assert_eq!(ts.take_selected(), Some("Leaf1".to_string()));
        assert!(!ts.is_open());
    }

    #[test]
    fn enter_on_branch_toggles() {
        let mut ts = TreeSelect::new().tree(nodes());
        laid_out(&mut ts);
        let mut o = overlay();
        let (_, popup) = open_popup(&mut ts, &mut o);
        // Focus row 0 = Group branch: Enter toggles it collapsed.
        o.dispatch_event(&key("Enter"));
        ts.sync_overlay(&mut o);
        assert!(ts.is_open());
        assert_eq!(ts.take_selected(), None);
        // Collapsed: "Solo" now sits at row 1 — commit it.
        press_row(&mut o, popup, 1);
        ts.sync_overlay(&mut o);
        assert_eq!(ts.take_selected(), Some("Solo".to_string()));
    }

    #[test]
    fn outside_press_dismisses_without_commit() {
        let mut ts = TreeSelect::new().tree(nodes());
        laid_out(&mut ts);
        let mut o = overlay();
        open_popup(&mut ts, &mut o);
        let press = WidgetEvent::PointerPressed {
            position: Vec2::new(700.0, 500.0),
            button: PointerButton::Primary,
            count: 1,
        };
        assert_eq!(o.dispatch_event(&press), EventResponse::Ignored);
        ts.sync_overlay(&mut o);
        assert!(!ts.is_open());
        assert_eq!(ts.popup_id, None);
        assert_eq!(ts.take_selected(), None);
    }

    #[test]
    fn layer_escape_dismisses() {
        let mut ts = TreeSelect::new().tree(nodes());
        laid_out(&mut ts);
        let mut o = overlay();
        open_popup(&mut ts, &mut o);
        assert_eq!(o.dispatch_event(&key("Escape")), EventResponse::Handled);
        ts.sync_overlay(&mut o);
        assert!(!ts.is_open());
    }

    #[test]
    fn popup_escape_requests_close() {
        let mut ts = TreeSelect::new().tree(nodes());
        laid_out(&mut ts);
        let mut o = overlay();
        let (id, _) = open_popup(&mut ts, &mut o);
        // Ownerless path: deliver Escape straight to the popup content.
        let popup = o.widget_at_mut(id, &[]).expect("popup");
        let mut cx = EventContext {
            event: &key("Escape"),
            bounds: Rect::default(),
            scale: 1.0,
        };
        assert_eq!(popup.event(&mut cx), EventResponse::Handled);
        ts.sync_overlay(&mut o);
        assert!(!ts.is_open());
    }

    #[test]
    fn reopen_selects_committed_value() {
        let mut ts = TreeSelect::new().tree(nodes());
        laid_out(&mut ts);
        let mut o = overlay();
        let (_, popup) = open_popup(&mut ts, &mut o);
        press_row(&mut o, popup, 3); // Solo
        ts.sync_overlay(&mut o);
        assert_eq!(ts.selected(), Some("Solo"));
        // Reopening selects the committed leaf's row in the new tree.
        let (id, _) = open_popup(&mut ts, &mut o);
        let tree = o.widget_at_mut(id, &[0]).expect("tree");
        // Solo is the last visible row; selection was restored — a
        // single ArrowDown + Enter re-commits it.
        assert_eq!(
            tree.event(&mut EventContext {
                event: &key("End"),
                bounds: Rect::default(),
                scale: 1.0,
            }),
            EventResponse::RequestRepaint
        );
        ts.sync_overlay(&mut o);
        assert_eq!(o.dispatch_event(&key("Enter")), EventResponse::Handled);
        ts.sync_overlay(&mut o);
        assert_eq!(ts.take_selected(), Some("Solo".to_string()));
    }

    #[test]
    fn set_selected_validates_leaf() {
        let mut ts = TreeSelect::new().tree(nodes());
        ts.set_selected(Some("Leaf2"));
        assert_eq!(ts.selected(), Some("Leaf2"));
        // Branches and unknown labels are refused.
        ts.set_selected(Some("Group"));
        assert_eq!(ts.selected(), Some("Leaf2"));
        ts.set_selected(Some("nope"));
        assert_eq!(ts.selected(), Some("Leaf2"));
        ts.set_selected(None::<String>);
        assert_eq!(ts.selected(), None);
        // Programmatic writes never feed the pick seam.
        assert_eq!(ts.take_selected(), None);
    }

    #[test]
    fn set_tree_drops_stale_selection() {
        let mut ts = TreeSelect::new().tree(nodes());
        ts.set_selected(Some("Solo"));
        ts.set_tree(vec![TreeNode::new("other")]);
        assert_eq!(ts.selected(), None);
    }

    #[test]
    fn semantic_actions() {
        let mut ts = TreeSelect::new().tree(nodes());
        laid_out(&mut ts);
        event(
            &mut ts,
            &WidgetEvent::SemanticAction(SemanticAction::Expand),
        );
        assert!(ts.is_open());
        event(
            &mut ts,
            &WidgetEvent::SemanticAction(SemanticAction::Collapse),
        );
        assert!(!ts.is_open());
        // SetValue picks a leaf by label.
        event(
            &mut ts,
            &WidgetEvent::SemanticAction(SemanticAction::SetValue("Solo".to_string())),
        );
        assert_eq!(ts.selected(), Some("Solo"));
        assert_eq!(ts.take_selected(), Some("Solo".to_string()));
        // A branch label is refused.
        assert_eq!(
            event(
                &mut ts,
                &WidgetEvent::SemanticAction(SemanticAction::SetValue("Group".to_string())),
            ),
            EventResponse::Ignored
        );
        assert_eq!(ts.selected(), Some("Solo"));
    }

    #[test]
    fn combobox_accessibility() {
        let mut ts = TreeSelect::new().tree(nodes()).label("Pick");
        laid_out(&mut ts);
        ts.set_selected(Some("Solo"));
        ts.open();
        let mut node = AccessKitNode::new(accesskit::Role::Unknown);
        ts.accessibility(&mut node);
        assert_eq!(node.role(), accesskit::Role::ComboBox);
        assert_eq!(node.label(), Some("Pick"));
        assert_eq!(node.value(), Some("Solo"));
        assert_eq!(node.has_popup(), Some(accesskit::HasPopup::Tree));
        assert_eq!(node.is_expanded(), Some(true));
        assert!(node.supports_action(accesskit::Action::Expand));
        assert!(node.supports_action(accesskit::Action::Collapse));
    }

    #[test]
    fn disabled_ignores_events() {
        let mut ts = TreeSelect::new().tree(nodes()).enabled(false);
        laid_out(&mut ts);
        assert_eq!(event(&mut ts, &key("Enter")), EventResponse::Ignored);
        let press = WidgetEvent::PointerPressed {
            position: Vec2::new(20.0, 20.0),
            button: PointerButton::Primary,
            count: 1,
        };
        assert_eq!(event(&mut ts, &press), EventResponse::Ignored);
        ts.open();
        assert!(!ts.is_open());
        let mut node = AccessKitNode::new(accesskit::Role::Unknown);
        ts.accessibility(&mut node);
        assert!(node.is_disabled());
    }
}
