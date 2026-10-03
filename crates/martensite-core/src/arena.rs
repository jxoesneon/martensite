//! Generational slotmap arena, tree hierarchy topology, and idle defragmentation.
//!
//! # Invariant Panics
//!
//! Internal tree-mutation methods use `expect("arena invariant")` on
//! `get_hot`/`get_hot_mut` calls. These are **deliberate** — each is
//! guarded by a prior `is_alive` check on the same `WidgetId`, so the
//! slot is guaranteed to be valid. Converting these to `Result` would
//! require changing the internal API to propagate errors that can
//! only occur if the arena's own data structures are corrupted (a
//! bug, not a user error). If such corruption occurs, panicking with
//! a clear message is the correct behavior (fail-fast).
use std::collections::{HashSet, VecDeque};
use std::iter::FusedIterator;
use std::time::Duration;

use crate::fence::FrameFence;
use crate::id::WidgetId;
use crate::node::{ColdNode, HotNode, NodeFlags};
use crate::overlay::OverlayLayer;
use crate::paint::PaintList;
use crate::widget::{
    EventContext, EventResponse, PaintContext, UnderflowPolicy, Widget, WidgetEvent,
};

/// Error variants for arena tree operations and synchronization.
///
/// # Examples
///
/// ```
/// use martensite_core::{ArenaError, DummyWidget, HotNode, WidgetArena};
///
/// let mut arena = WidgetArena::new();
/// let a = arena.insert_with_widget(HotNode::default(), Box::new(DummyWidget));
///
/// // Self-parenting is rejected with `SelfParenting`.
/// let err = arena.append_child(a, a).unwrap_err();
/// assert_eq!(err, ArenaError::SelfParenting(a));
///
/// // A stale handle is rejected with `InvalidNode`.
/// arena.remove(a);
/// let err = arena.detach(a).unwrap_err();
/// assert_eq!(err, ArenaError::InvalidNode(a));
///
/// // `ArenaError` implements `std::error::Error` and `Display`.
/// let msg = format!("{}", ArenaError::CycleDetected);
/// assert!(msg.contains("Cycle detected"));
/// ```
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ArenaError {
    /// Provided widget handle is invalid or has expired generation.
    InvalidNode(WidgetId),
    /// Parent widget handle is invalid or expired.
    InvalidParent(WidgetId),
    /// Child widget handle is invalid or expired.
    InvalidChild(WidgetId),
    /// Target widget handle is invalid or expired.
    InvalidTarget(WidgetId),
    /// Target widget has no parent node.
    NoParent(WidgetId),
    /// Node is not a child of the specified parent.
    NotAChild {
        /// Specified parent widget.
        parent: WidgetId,
        /// Specified child widget.
        child: WidgetId,
    },
    /// Attempted to set a node as its own parent.
    SelfParenting(WidgetId),
    /// Insertion would form a cycle in the tree hierarchy.
    CycleDetected,
    /// Invalid tree mutation operation requested.
    InvalidOperation(&'static str),
}

impl std::fmt::Display for ArenaError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::InvalidNode(id) => write!(f, "Invalid or expired widget handle: {:?}", id),
            Self::InvalidParent(id) => {
                write!(f, "Invalid or expired parent widget handle: {:?}", id)
            }
            Self::InvalidChild(id) => write!(f, "Invalid or expired child widget handle: {:?}", id),
            Self::InvalidTarget(id) => {
                write!(f, "Invalid or expired target widget handle: {:?}", id)
            }
            Self::NoParent(id) => write!(f, "Widget {:?} has no parent", id),
            Self::NotAChild { parent, child } => {
                write!(
                    f,
                    "Widget {:?} is not a child of parent {:?}",
                    child, parent
                )
            }
            Self::SelfParenting(id) => write!(f, "Cannot parent widget {:?} to itself", id),
            Self::CycleDetected => write!(f, "Cycle detected in scene graph hierarchy"),
            Self::InvalidOperation(msg) => write!(f, "Invalid arena tree operation: {}", msg),
        }
    }
}

impl std::error::Error for ArenaError {}

/// Slot entry in the sparse lookup table.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub(crate) struct Slot {
    /// Generational counter tracking allocation lifecycle.
    pub generation: u32,
    /// Index into dense parallel arrays.
    pub dense_idx: u32,
}

/// Generational slotmap arena maintaining packed 64-byte HotNode elements alongside ColdNode storage.
///
/// # Examples
///
/// Construct an arena, insert nodes, build a tree, and traverse it:
///
/// ```
/// use martensite_core::{DummyWidget, HotNode, WidgetArena};
///
/// let mut arena = WidgetArena::new();
/// let root = arena.insert_with_widget(HotNode::default(), Box::new(DummyWidget));
/// let a = arena.insert_with_widget(HotNode::default(), Box::new(DummyWidget));
/// let b = arena.insert_with_widget(HotNode::default(), Box::new(DummyWidget));
/// arena.append_child(root, a).unwrap();
/// arena.append_child(root, b).unwrap();
///
/// // Pre-order subtree traversal visits root, then its children in order.
/// let visited: Vec<_> = arena.iter_subtree(root).collect();
/// assert_eq!(visited, vec![root, a, b]);
///
/// // Removing a node unparents its children and invalidates the handle.
/// arena.remove(a);
/// assert!(!arena.is_alive(a));
/// assert_eq!(arena.children(root).count(), 1);
/// ```
pub struct WidgetArena {
    /// Sparse slot indirection table.
    pub(crate) slots: Vec<Slot>,
    /// Dense cache-line aligned hot node records.
    pub(crate) hot_nodes: Vec<HotNode>,
    /// Parallel cold node storage (widgets, metadata, accessibility).
    pub(crate) cold_nodes: Vec<ColdNode>,
    /// Reverse mapping from dense index to sparse slot index.
    pub(crate) dense_to_slot: Vec<u32>,
    /// Strict FIFO queue distributing recycled slot indices.
    pub(crate) free_slots: VecDeque<u32>,
    /// In-window popup layer owned by the arena. Popups are painted
    /// after arena content in [`build_paint_list`](Self::build_paint_list),
    /// offered input before arena hit-testing by `martensite-window`'s
    /// `EventRouter`, and emitted into the AccessKit tree by
    /// `martensite-access`'s `AccessKitAdapter`.
    overlay: OverlayLayer,
    /// The most recent widget that asked for keyboard focus —
    /// drained by [`take_focus_request`](Self::take_focus_request).
    pending_focus: Option<WidgetId>,
    /// The active design-token theme, handed to every widget through
    /// [`PaintContext::theme`] during [`build_paint_list`]. Defaults to
    /// an empty theme so widgets fall back to their baked appearance
    /// until [`set_theme`](Self::set_theme) installs one.
    theme: martensite_theme::Theme,
    /// Physical px per logical pt, handed to every widget through
    /// [`PaintContext::scale`] during [`build_paint_list`]. `1.0` until
    /// [`set_scale_factor`](Self::set_scale_factor) reports the real
    /// display density.
    scale_factor: f32,
    /// Ambient shaped-text painter handed to every widget through
    /// [`PaintContext::text_painter`] during [`build_paint_list`].
    /// `None` until [`set_text_painter`](Self::set_text_painter)
    /// installs one — widgets then fall back to `DrawText` placeholder
    /// boxes. `Arc` so widgets may also hold explicit clones.
    text_painter: Option<std::sync::Arc<dyn crate::paint::TextShaper + Send + Sync>>,
    /// Ambient reduced-motion preference — while `true`, loading
    /// placeholders paint statically (`phase = None`) instead of the
    /// animated shimmer sweep. Installed via
    /// [`set_reduced_motion`](Self::set_reduced_motion).
    reduced_motion: bool,
    /// Shared shimmer clock in seconds, advanced by
    /// [`tick`](Self::tick) while any node is loading and fed to
    /// [`Widget::paint_loading`] as the phase — all skeletons sweep in
    /// lock-step on one clock instead of carrying per-widget phases.
    loading_elapsed: f32,
    /// Ambient layout direction published to widgets during layout,
    /// paint, and event passes. Installed via
    /// [`set_layout_direction`](Self::set_layout_direction).
    layout_direction: crate::LayoutDirection,
    /// Ambient locale published to widgets during layout, paint, and
    /// event passes. Installed via [`set_locale`](Self::set_locale).
    locale: crate::Locale,
}

impl std::fmt::Debug for WidgetArena {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("WidgetArena")
            .field("len", &self.len())
            .field("capacity", &self.capacity())
            .field("slots_len", &self.slot_count())
            .field("free_slots_len", &self.free_slots_len())
            .finish()
    }
}

impl Default for WidgetArena {
    fn default() -> Self {
        Self::new()
    }
}

impl WidgetArena {
    /// Construct a new empty WidgetArena with default initial capacity (256 nodes).
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite_core::WidgetArena;
    ///
    /// let arena = WidgetArena::new();
    /// assert!(arena.is_empty());
    /// assert_eq!(arena.len(), 0);
    /// assert!(arena.capacity() >= 256);
    /// ```
    pub fn new() -> Self {
        Self::with_capacity(256)
    }

    /// Construct a new empty WidgetArena pre-allocated to the specified node capacity.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite_core::WidgetArena;
    ///
    /// let arena = WidgetArena::with_capacity(1024);
    /// assert!(arena.is_empty());
    /// assert!(arena.capacity() >= 1024);
    /// ```
    pub fn with_capacity(capacity: usize) -> Self {
        Self {
            slots: Vec::with_capacity(capacity),
            hot_nodes: Vec::with_capacity(capacity),
            cold_nodes: Vec::with_capacity(capacity),
            dense_to_slot: Vec::with_capacity(capacity),
            free_slots: VecDeque::new(),
            overlay: OverlayLayer::new(),
            pending_focus: None,
            theme: martensite_theme::Theme::new("fallback"),
            scale_factor: 1.0,
            text_painter: None,
            reduced_motion: false,
            loading_elapsed: 0.0,
            layout_direction: crate::LayoutDirection::Ltr,
            locale: crate::Locale::default(),
        }
    }

    /// Returns the active design-token theme. The default is an empty
    /// theme — widgets resolve tokens against it and fall back to their
    /// baked defaults, so an unthemed arena reproduces pre-theme
    /// behaviour exactly.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite_core::{TokenKey, WidgetArena};
    ///
    /// let arena = WidgetArena::new();
    /// assert!(arena.theme().color(TokenKey::TextColor).is_none());
    /// ```
    pub fn theme(&self) -> &martensite_theme::Theme {
        &self.theme
    }

    /// Replaces the active design-token theme. The next
    /// [`build_paint_list`](Self::build_paint_list) hands it to every
    /// widget through [`PaintContext::theme`]; mark nodes
    /// `DIRTY_PAINT` (or rebuild the list) so the new tokens take
    /// effect. Callers can animate a switch by installing successive
    /// [`martensite_theme::ThemeDiff::interpolate`] results per frame.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite_core::{TokenKey, WidgetArena};
    ///
    /// let mut arena = WidgetArena::new();
    /// arena.set_theme(martensite_theme::tokens::default_dark());
    /// assert!(arena.theme().color(TokenKey::TextColor).is_some());
    /// ```
    pub fn set_theme(&mut self, theme: martensite_theme::Theme) {
        self.theme = theme;
    }

    /// Returns the ambient layout direction —
    /// [`LayoutDirection::Ltr`](crate::LayoutDirection::Ltr) until
    /// [`set_layout_direction`](Self::set_layout_direction) changes it.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite_core::{LayoutDirection, WidgetArena};
    ///
    /// let arena = WidgetArena::new();
    /// assert_eq!(arena.layout_direction(), LayoutDirection::Ltr);
    /// ```
    pub fn layout_direction(&self) -> crate::LayoutDirection {
        self.layout_direction
    }

    /// Sets the ambient layout direction widgets observe through
    /// [`LayoutContext::direction`](crate::LayoutContext::direction),
    /// [`PaintContext::direction`](crate::PaintContext::direction), and
    /// [`EventContext::direction`](crate::EventContext::direction).
    /// A no-op when unchanged; otherwise every live node is marked
    /// `DIRTY_LAYOUT | DIRTY_PAINT | DIRTY_A11Y` and the overlay layer
    /// is re-synced so popups follow.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite_core::{LayoutDirection, WidgetArena};
    ///
    /// let mut arena = WidgetArena::new();
    /// arena.set_layout_direction(LayoutDirection::Rtl);
    /// assert!(arena.layout_direction().is_rtl());
    /// ```
    pub fn set_layout_direction(&mut self, direction: crate::LayoutDirection) {
        if self.layout_direction == direction {
            return;
        }
        self.layout_direction = direction;
        self.intl_changed();
    }

    /// Returns the ambient locale — `en-US` until
    /// [`set_locale`](Self::set_locale) changes it.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite_core::WidgetArena;
    ///
    /// let arena = WidgetArena::new();
    /// assert_eq!(arena.locale().as_str(), "en-US");
    /// ```
    pub fn locale(&self) -> &crate::Locale {
        &self.locale
    }

    /// Sets the ambient locale widgets observe through
    /// [`LayoutContext::locale`](crate::LayoutContext::locale) and
    /// friends. Does **not** change the layout direction — call
    /// [`set_layout_direction`](Self::set_layout_direction) with
    /// [`LayoutDirection::for_locale`](crate::LayoutDirection::for_locale)
    /// when both should follow. A no-op when unchanged; otherwise every
    /// live node is marked `DIRTY_LAYOUT | DIRTY_PAINT | DIRTY_A11Y`.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite_core::{LayoutDirection, Locale, WidgetArena};
    ///
    /// let mut arena = WidgetArena::new();
    /// arena.set_locale(Locale::new("ar-EG"));
    /// assert_eq!(arena.locale().as_str(), "ar-EG");
    /// assert_eq!(arena.layout_direction(), LayoutDirection::Ltr);
    /// ```
    pub fn set_locale(&mut self, locale: crate::Locale) {
        if self.locale == locale {
            return;
        }
        self.locale = locale;
        self.intl_changed();
    }

    /// Installs this arena's layout direction and locale as the
    /// thread's ambient values until the returned guard drops. The
    /// arena does this itself around paint and event passes and
    /// `LayoutEngine` around layout; call it for manual layout passes
    /// (tests, harness sweeps) that drive `Widget::layout` directly.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite_core::intl::ambient_direction;
    /// use martensite_core::{LayoutDirection, WidgetArena};
    ///
    /// let mut arena = WidgetArena::new();
    /// arena.set_layout_direction(LayoutDirection::Rtl);
    /// {
    ///     let _guard = arena.install_ambient_intl();
    ///     assert!(ambient_direction().is_rtl());
    /// }
    /// assert!(!ambient_direction().is_rtl());
    /// ```
    pub fn install_ambient_intl(&self) -> crate::intl::AmbientIntlGuard {
        crate::intl::install_ambient_intl(self.layout_direction, self.locale.clone())
    }

    /// Dirty-marks every live node for layout, paint, and a11y and
    /// re-syncs the overlay layer after a direction/locale change.
    fn intl_changed(&mut self) {
        for hot in &mut self.hot_nodes {
            hot.flags |= NodeFlags::DIRTY_LAYOUT | NodeFlags::DIRTY_PAINT | NodeFlags::DIRTY_A11Y;
        }
        self.overlay
            .set_intl(self.layout_direction, self.locale.clone());
    }

    /// Returns the display scale factor (physical px per logical pt).
    /// `1.0` until [`set_scale_factor`](Self::set_scale_factor) is
    /// called — a logical-pixel arena never needs to change it.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite_core::WidgetArena;
    ///
    /// let arena = WidgetArena::new();
    /// assert_eq!(arena.scale_factor(), 1.0);
    /// ```
    pub fn scale_factor(&self) -> f32 {
        self.scale_factor
    }

    /// Reports the display's scale factor so widgets can convert their
    /// baked logical-point constants (font sizes, paddings, target
    /// minimums) into the device-pixel sizes the paint pass emits —
    /// see [`PaintContext::scale`]. Call it when the window moves
    /// between displays or the OS scale changes. A **relayout** is
    /// required for widgets that cache scale-derived geometry in
    /// `layout` (thumb extents, scroll steps); the next
    /// [`build_paint_list`](Self::build_paint_list) then emits at the
    /// new factor.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite_core::WidgetArena;
    ///
    /// let mut arena = WidgetArena::new();
    /// arena.set_scale_factor(2.0);
    /// assert_eq!(arena.scale_factor(), 2.0);
    /// ```
    pub fn set_scale_factor(&mut self, scale: f32) {
        self.scale_factor = if scale.is_finite() && scale > 0.0 {
            scale
        } else {
            1.0
        };
        self.overlay.set_scale_factor(self.scale_factor);
    }

    /// Installs the ambient shaped-text painter handed to every widget
    /// through [`PaintContext::text_painter`]. Call once at startup —
    /// e.g. with `martensite::text_paint::shared_painter()` — and all
    /// facade widgets emit real glyph runs instead of `DrawText`
    /// placeholder boxes. A widget's own explicitly-injected painter
    /// still takes precedence.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite_core::paint::TextShaper;
    /// use martensite_core::{GlyphRun, PaintList, WidgetArena};
    /// use std::sync::Arc;
    ///
    /// struct Nop;
    /// impl TextShaper for Nop {
    ///     fn paint_shaped_text(
    ///         &self,
    ///         _: &mut PaintList,
    ///         _: kurbo::Point,
    ///         _: &str,
    ///         _: f32,
    ///         _: [u8; 4],
    ///     ) {
    ///     }
    /// }
    ///
    /// let mut arena = WidgetArena::new();
    /// arena.set_text_painter(Nop);
    /// assert!(arena.text_painter().is_some());
    /// ```
    pub fn set_text_painter(
        &mut self,
        painter: impl crate::paint::TextShaper + Send + Sync + 'static,
    ) {
        self.text_painter = Some(std::sync::Arc::new(painter));
    }

    /// Returns the ambient text painter installed by
    /// [`set_text_painter`](Self::set_text_painter), if any — pass to
    /// [`OverlayLayer::paint`](crate::overlay::OverlayLayer::paint) so
    /// popup content shapes text identically to arena content.
    pub fn text_painter(&self) -> Option<&(dyn crate::paint::TextShaper + Send + Sync)> {
        self.text_painter.as_deref()
    }

    /// Shared handle to the ambient painter — for installing the
    /// [ambient measurer](crate::paint::install_ambient_measurer)
    /// around manual layout passes that bypass `LayoutEngine`, so
    /// [`LayoutContext::measure_text`](crate::LayoutContext::measure_text)
    /// sees the same glyph metrics the paint pass will use.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite_core::paint::TextShaper;
    /// use martensite_core::WidgetArena;
    ///
    /// struct Nop;
    /// impl TextShaper for Nop {
    ///     fn paint_shaped_text(
    ///         &self,
    ///         _: &mut martensite_core::PaintList,
    ///         _: kurbo::Point,
    ///         _: &str,
    ///         _: f32,
    ///         _: [u8; 4],
    ///     ) {
    ///     }
    /// }
    ///
    /// let mut arena = WidgetArena::new();
    /// assert!(arena.text_painter_shared().is_none());
    /// arena.set_text_painter(Nop);
    /// assert!(arena.text_painter_shared().is_some());
    /// ```
    pub fn text_painter_shared(
        &self,
    ) -> Option<std::sync::Arc<dyn crate::paint::TextShaper + Send + Sync>> {
        self.text_painter.clone()
    }

    /// Returns the arena-owned in-window [`OverlayLayer`].
    ///
    /// The layer holds popups opened by widgets (e.g. `Dropdown`,
    /// `Tooltip`) through [`Widget::sync_overlay`]. Popups live outside
    /// the widget hierarchy but participate in painting, routed input,
    /// and accessibility: [`build_paint_list`](Self::build_paint_list)
    /// appends them after arena content, `martensite-window`'s
    /// `EventRouter` offers pointer/scroll/Escape input to the layer
    /// before arena hit-testing, and `martensite-access`'s
    /// `AccessKitAdapter` emits them as top-level virtual nodes.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite_core::WidgetArena;
    ///
    /// let arena = WidgetArena::new();
    /// assert!(arena.overlay().is_empty());
    /// ```
    pub fn overlay(&self) -> &OverlayLayer {
        &self.overlay
    }

    /// Returns a mutable reference to the arena-owned [`OverlayLayer`].
    ///
    /// Set the viewport with
    /// [`OverlayLayer::set_viewport`](crate::overlay::OverlayLayer::set_viewport)
    /// before any popup opens so placement clamping has meaningful
    /// bounds.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite_core::{Rect, WidgetArena};
    ///
    /// let mut arena = WidgetArena::new();
    /// arena
    ///     .overlay_mut()
    ///     .set_viewport(Rect::new(0.0, 0.0, 800.0, 600.0));
    /// ```
    pub fn overlay_mut(&mut self) -> &mut OverlayLayer {
        &mut self.overlay
    }

    /// Drives one frame of overlay synchronization.
    ///
    /// Calls [`Widget::sync_overlay`] on every live widget **and** every
    /// internal child (recursively), so widgets anywhere in the
    /// hierarchy can open, move, dismiss, or observe their popups; then
    /// runs [`OverlayLayer::layout_pass`] to resolve anchors for entries
    /// opened this frame. Entries opened during a widget's sync are
    /// stamped with that widget as their [`owner`](crate::overlay::OverlayEntry::owner) —
    /// see [`Self::remove`] for how the stamp keeps popups from
    /// outliving dead widgets.
    ///
    /// Afterwards, every widget whose popup was opened, closed, or
    /// touched by an event since the last sync is marked
    /// `DIRTY_PAINT | DIRTY_A11Y` — popup open/close changes emitted
    /// accessibility state (`expanded`, `controls`, `described_by`) and
    /// must not wait for a widget event to reach an incremental
    /// `TreeUpdate`.
    ///
    /// Call once per frame after layout and before
    /// painting/accessibility emission — or drive both steps at once
    /// via [`Self::tick`].
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite_core::{DummyWidget, HotNode, WidgetArena};
    ///
    /// let mut arena = WidgetArena::new();
    /// arena.insert_with_widget(HotNode::default(), Box::new(DummyWidget));
    /// arena.sync_overlays(); // no popups — cheap no-op
    /// ```
    pub fn sync_overlays(&mut self) {
        let mut overlay = std::mem::take(&mut self.overlay);
        let open_before: HashSet<u64> = overlay.entries().map(|e| e.id()).collect();
        let ids: Vec<WidgetId> = self.iter_depth_first().collect();
        for id in ids {
            overlay.set_current_owner(Some(id));
            if let Some(cold) = self.get_cold_mut(id) {
                Self::sync_overlay_recursive(cold.widget.as_mut(), &mut overlay);
            }
            // An owner whose underflow policy hides it — or that
            // entered loading — can no longer project a popup: an
            // invisible or skeletonized widget floating a live surface
            // is worse than either state alone. Close it.
            let orphan = self
                .get_cold(id)
                .and_then(|c| c.underflow_policy())
                .is_some_and(|p| p.hides_from_a11y() || p.covers_input())
                || self.effective_loading(id);
            if orphan {
                overlay.close_owner(id);
            }
        }
        overlay.set_current_owner(None);
        overlay.layout_pass();
        // Owners whose popup state changed: entries opened this frame
        // carry a fresh owner stamp; entries closed (outside press,
        // Escape, owner-driven close) or touched by popup events left
        // their owner in the layer's dirty log.
        let mut owners: Vec<WidgetId> = overlay
            .entries()
            .filter(|e| !open_before.contains(&e.id()))
            .filter_map(|e| e.owner())
            .collect();
        for owner in overlay.take_dirty_owners() {
            if !owners.contains(&owner) {
                owners.push(owner);
            }
        }
        self.overlay = overlay;
        for owner in owners {
            self.mark_dirty(owner);
        }
    }

    /// Release margin for underflow hysteresis: a node engages when its
    /// bounds drop below its [`RenderMinimum`](crate::RenderMinimum) and
    /// releases only when both axes recover past `min × 1.05`. Without
    /// the band, a resize drag hovering at the boundary would flicker
    /// the policy every frame.
    pub const UNDERFLOW_RELEASE: f32 = 1.05;

    /// Re-evaluates `id`'s underflow engagement against its current
    /// [`HotNode::bounds`] — call after assigning a node's bounds.
    ///
    /// The Taffy `LayoutEngine` calls this automatically for every node
    /// it lays out. Manual layout paths (docking BSPs, custom
    /// allocators — anything that writes `hot.bounds` directly) should
    /// call [`Self::update_underflow_all`] once per layout pass, or this
    /// per node. Nodes with no declared [`RenderMinimum`](crate::RenderMinimum)
    /// or an advisory policy (`Allow`/`Lint`) are never engaged — the
    /// paint audit reports their violations independently of this
    /// state.
    ///
    /// `Collapse`-policy nodes engage like any enforcing policy; the
    /// space-freeing half of `Collapse` is applied by the layout engine
    /// — on manual paths the node keeps its slot (Hide semantics).
    ///
    /// # Examples
    ///
    /// ```
    /// use glam::Vec2;
    /// use martensite_core::{ColdNode, DummyWidget, HotNode, NodeFlags,
    ///     Rect, RenderMinimum, UnderflowPolicy, WidgetArena};
    ///
    /// let mut arena = WidgetArena::new();
    /// let mut hot = HotNode::default();
    /// hot.flags |= NodeFlags::VISIBLE;
    /// hot.bounds = Rect::new(0.0, 0.0, 40.0, 20.0); // below the 100x50 floor
    /// let id = arena.insert(
    ///     hot,
    ///     ColdNode::new(Box::new(DummyWidget)).with_render_minimum(
    ///         RenderMinimum::new(Vec2::new(100.0, 50.0))
    ///             .with_policy(UnderflowPolicy::Hide),
    ///     ),
    /// );
    /// arena.update_underflow(id);
    /// assert_eq!(
    ///     arena.get_cold(id).unwrap().underflow_policy(),
    ///     Some(UnderflowPolicy::Hide)
    /// );
    /// ```
    pub fn update_underflow(&mut self, id: WidgetId) {
        let Some(hot) = self.get_hot(id) else {
            return;
        };
        let bounds = hot.bounds;
        let scale = self.scale_factor;
        let Some(cold) = self.get_cold_mut(id) else {
            return;
        };
        let min = cold.effective_render_minimum();
        if min.size == glam::Vec2::ZERO || !min.policy.enforces() {
            cold.underflow_engaged = false;
            return;
        }
        let engaged = cold.underflow_engaged;
        let next = if engaged {
            // Release only when both axes clear the hysteresis band.
            !min.satisfied(bounds, scale, Self::UNDERFLOW_RELEASE)
        } else {
            min.violated(bounds, scale)
        };
        if next != engaged {
            cold.underflow_engaged = next;
            if let Some(h) = self.get_hot_mut(id) {
                h.flags |= NodeFlags::DIRTY_PAINT | NodeFlags::DIRTY_A11Y;
            }
        }
    }

    /// Runs [`Self::update_underflow`] for every live node — the
    /// one-call hook for manual layout paths after they finish
    /// assigning bounds.
    pub fn update_underflow_all(&mut self) {
        let ids: Vec<WidgetId> = self.iter_depth_first().collect();
        for id in ids {
            self.update_underflow(id);
        }
    }

    /// Drives one frame of widget state advancement plus overlay
    /// synchronization — the production frame seam.
    ///
    /// Calls [`Widget::tick`] on every live arena widget and,
    /// recursively, on its internal children; widgets that returned
    /// `true` (animation in flight, delay countdown running) are marked
    /// `DIRTY_PAINT | DIRTY_A11Y`. Loading nodes (see
    /// [`Self::set_loading`]) are additionally marked `DIRTY_PAINT`
    /// only every tick so their placeholders track the shared shimmer
    /// clock without re-emitting accessibility state at frame rate.
    /// Then runs [`Self::sync_overlays`] so effects the tick produced —
    /// a `Tooltip` finishing its hover delay, a `ScrollView` animation
    /// completing — reconcile their popups the same frame.
    ///
    /// Call once per frame from the application/windowing frame loop
    /// (`martensite-access`'s `MartensiteAccessBridge::tick` forwards
    /// here), before `build_paint_list` and before building an
    /// AccessKit `TreeUpdate`.
    ///
    /// # Examples
    ///
    /// ```
    /// use std::time::Duration;
    /// use martensite_core::{DummyWidget, HotNode, WidgetArena};
    ///
    /// let mut arena = WidgetArena::new();
    /// arena.insert_with_widget(HotNode::default(), Box::new(DummyWidget));
    /// arena.tick(Duration::from_millis(16)); // drives widgets + overlays
    /// ```
    pub fn tick(&mut self, dt: Duration) {
        let ids: Vec<WidgetId> = self.iter_depth_first().collect();
        let mut any_loading = false;
        for id in ids {
            let (paint_dirty, semantic_dirty) = self
                .get_cold_mut(id)
                .map(|cold| Self::tick_recursive(cold.widget.as_mut(), dt))
                .unwrap_or((false, false));
            if semantic_dirty {
                self.mark_dirty(id);
            } else if paint_dirty {
                // `tick_paint_only` widgets advance purely visual
                // animation — no semantic change means no a11y
                // re-emission at frame rate.
                self.mark_dirty_paint(id);
            }
            // Loading placeholders repaint on the shared shimmer clock
            // — paint-only: re-emitting accessibility state at frame
            // rate for a purely visual sweep would swamp incremental
            // `TreeUpdate`s.
            if self.effective_loading(id) {
                any_loading = true;
                self.mark_dirty_paint(id);
            }
        }
        // The clock only runs while a skeleton is on screen — otherwise
        // every tick would accumulate time no placeholder consumes.
        if any_loading {
            self.loading_elapsed += dt.as_secs_f32();
        }
        self.sync_overlays();
    }

    /// Recursive helper for [`tick`](Self::tick): ticks `widget` then
    /// its internal children. Returns `(paint_dirty, semantic_dirty)` —
    /// a widget whose [`Widget::tick_paint_only`] is `true` contributes
    /// to the former only. Children with no allocated bounds are
    /// skipped — the same "not presented" signal the paint walk uses
    /// (`TabPanelChild` reports `None` for hidden panels), so hidden
    /// views stop ticking and stop marking the frame dirty.
    fn tick_recursive(widget: &mut dyn Widget, dt: Duration) -> (bool, bool) {
        let mut paint_dirty = false;
        let mut semantic_dirty = false;
        if widget.tick(dt) {
            paint_dirty = true;
            semantic_dirty = !widget.tick_paint_only();
        }
        for i in 0..widget.child_count() {
            if widget.child_bounds(i).is_none() {
                continue;
            }
            if let Some(child) = widget.child_mut(i) {
                let (p, s) = Self::tick_recursive(child, dt);
                paint_dirty |= p;
                semantic_dirty |= s;
            }
        }
        (paint_dirty, semantic_dirty)
    }

    /// Recursive helper for [`sync_overlays`](Self::sync_overlays).
    /// Unallocated children are skipped like [`tick_recursive`] — a
    /// hidden view must not register overlay content either.
    fn sync_overlay_recursive(widget: &mut dyn Widget, overlay: &mut OverlayLayer) {
        widget.sync_overlay(overlay);
        for i in 0..widget.child_count() {
            if widget.child_bounds(i).is_none() {
                continue;
            }
            if let Some(child) = widget.child_mut(i) {
                Self::sync_overlay_recursive(child, overlay);
            }
        }
    }

    /// Drains the pending keyboard-focus request, if any.
    ///
    /// A request is recorded when a widget answers an event with
    /// [`EventResponse::CaptureFocus`], or when a `PointerPressed` is
    /// handled by a node carrying [`NodeFlags::FOCUSABLE`]
    /// (press-to-focus). The request names the arena widget that
    /// responded — even when the responder was an internal child, the
    /// request resolves to its arena owner.
    ///
    /// Applying it is the app's responsibility: pass the id to
    /// `martensite-focus`'s `FocusManager::set_focus`, then dispatch
    /// [`WidgetEvent::FocusLost`] to the previously focused widget and
    /// [`WidgetEvent::FocusGained`] to the new one via
    /// [`dispatch_event`](Self::dispatch_event).
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite_core::{DummyWidget, HotNode, NodeFlags, WidgetArena};
    ///
    /// let mut arena = WidgetArena::new();
    /// let mut hot = HotNode::default();
    /// hot.flags = NodeFlags::VISIBLE | NodeFlags::FOCUSABLE;
    /// let id = arena.insert_with_widget(hot, Box::new(DummyWidget));
    ///
    /// assert_eq!(arena.take_focus_request(), None);
    /// ```
    pub fn take_focus_request(&mut self) -> Option<WidgetId> {
        self.pending_focus.take()
    }

    /// Records a pending focus request for `id` without routing an
    /// event — the arena-level counterpart of
    /// [`EventResponse::CaptureFocus`].
    ///
    /// Used when a *virtual* node (a widget's internal child or popup
    /// content inside an overlay entry) answers an assistive-technology
    /// action by requesting focus: the request must resolve to the
    /// owning arena widget, which the action dispatcher reaches via
    /// this method. Dead widgets are ignored. Drain with
    /// [`take_focus_request`](Self::take_focus_request) and apply
    /// through `martensite-focus`'s `FocusManager`.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite_core::{DummyWidget, HotNode, WidgetArena};
    ///
    /// let mut arena = WidgetArena::new();
    /// let id = arena.insert_with_widget(HotNode::default(), Box::new(DummyWidget));
    /// arena.request_focus(id);
    /// assert_eq!(arena.take_focus_request(), Some(id));
    /// ```
    pub fn request_focus(&mut self, id: WidgetId) {
        if !self.is_alive(id) {
            return;
        }
        self.pending_focus = Some(id);
        if let Some(h) = self.get_hot_mut(id) {
            h.flags |= NodeFlags::DIRTY_PAINT;
        }
    }

    /// Marks `id` `DIRTY_PAINT | DIRTY_A11Y` — the widget's emitted
    /// appearance and accessibility state are stale and the next frame
    /// must repaint it and include it in an incremental `TreeUpdate`.
    /// Dead widgets are ignored.
    ///
    /// Used by the accessibility action dispatcher when an action is
    /// delivered to a virtual (internal or overlay) target: the
    /// response mutates the owning widget's state without going through
    /// `dispatch_event`, which would have marked it.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite_core::{DummyWidget, HotNode, NodeFlags, WidgetArena};
    ///
    /// let mut arena = WidgetArena::new();
    /// let id = arena.insert_with_widget(HotNode::default(), Box::new(DummyWidget));
    /// arena.mark_dirty(id);
    /// assert!(arena.get_hot(id).unwrap().flags.contains(NodeFlags::DIRTY_A11Y));
    /// ```
    pub fn mark_dirty(&mut self, id: WidgetId) {
        if let Some(h) = self.get_hot_mut(id) {
            h.flags |= NodeFlags::DIRTY_PAINT | NodeFlags::DIRTY_A11Y;
        }
    }

    /// Marks `id` `DIRTY_PAINT` only — the emitted appearance is stale
    /// but its accessibility state is unchanged. Dead widgets are
    /// ignored.
    ///
    /// This is the animation path: shimmering loading placeholders
    /// (and any ticking widget) repaint every frame, and re-emitting
    /// an AccessKit `TreeUpdate` at frame rate would swamp assistive
    /// technology for a purely visual change. Use [`mark_dirty`](Self::mark_dirty)
    /// when emitted a11y state may also have moved.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite_core::{DummyWidget, HotNode, NodeFlags, WidgetArena};
    ///
    /// let mut arena = WidgetArena::new();
    /// let id = arena.insert_with_widget(HotNode::default(), Box::new(DummyWidget));
    /// arena.mark_dirty_paint(id);
    /// let flags = arena.get_hot(id).unwrap().flags;
    /// assert!(flags.contains(NodeFlags::DIRTY_PAINT));
    /// assert!(!flags.contains(NodeFlags::DIRTY_A11Y));
    /// ```
    pub fn mark_dirty_paint(&mut self, id: WidgetId) {
        if let Some(h) = self.get_hot_mut(id) {
            h.flags |= NodeFlags::DIRTY_PAINT;
        }
    }

    /// The resolved loading state of `id` — the instance-level
    /// [`NodeFlags::LOADING`] override OR'd with the widget's own
    /// [`Widget::is_loading`] declaration,
    /// mirroring `ColdNode::effective_render_minimum`'s
    /// instance-plus-type resolution. Every enforcement chokepoint
    /// consults this — never the raw flag or trait alone.
    fn effective_loading(&self, id: WidgetId) -> bool {
        self.get_both(id).is_some_and(|(hot, cold)| {
            hot.flags.contains(NodeFlags::LOADING) || cold.widget.is_loading()
        })
    }

    /// `true` while `id` is loading — either the instance-level
    /// [`NodeFlags::LOADING`] override (see [`Self::set_loading`]) or
    /// the widget's own [`Widget::is_loading`]
    /// declaration. `false` for dead handles.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite_core::{DummyWidget, HotNode, WidgetArena};
    ///
    /// let mut arena = WidgetArena::new();
    /// let id = arena.insert_with_widget(HotNode::default(), Box::new(DummyWidget));
    /// assert!(!arena.node_loading(id));
    /// arena.set_loading(id, true);
    /// assert!(arena.node_loading(id));
    /// ```
    pub fn node_loading(&self, id: WidgetId) -> bool {
        self.effective_loading(id)
    }

    /// Sets or clears the instance-level [`NodeFlags::LOADING`
    /// override](crate::NodeFlags) on `id` — the arena-side half of
    /// the loading protocol that skeletonizes *any* node, including
    /// widgets that never implemented
    /// [`Widget::is_loading`].
    ///
    /// While loading, the node paints its
    /// [`Widget::paint_loading`]
    /// placeholder instead of body and children, is skipped by event
    /// dispatch (target and ancestors alike), and has any owned
    /// overlay popups closed by [`Self::sync_overlays`]. The widget's
    /// own `is_loading` still applies — clearing this flag does not
    /// un-load a widget that declares itself pending.
    ///
    /// On transition the node is marked `DIRTY_PAINT | DIRTY_A11Y`
    /// once (placeholder ↔ content swaps what the a11y tree must
    /// show); repeating the same state is a no-op.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite_core::{DummyWidget, HotNode, NodeFlags, WidgetArena};
    ///
    /// let mut arena = WidgetArena::new();
    /// let id = arena.insert_with_widget(HotNode::default(), Box::new(DummyWidget));
    /// arena.set_loading(id, true);
    /// assert!(arena
    ///     .get_hot(id)
    ///     .unwrap()
    ///     .flags
    ///     .contains(NodeFlags::LOADING));
    /// arena.set_loading(id, false);
    /// assert!(!arena.node_loading(id));
    /// ```
    pub fn set_loading(&mut self, id: WidgetId, loading: bool) {
        let Some((hot, cold)) = self.get_both_mut(id) else {
            return;
        };
        let was = hot.flags.contains(NodeFlags::LOADING) || cold.widget.is_loading();
        hot.flags.set(NodeFlags::LOADING, loading);
        let now = hot.flags.contains(NodeFlags::LOADING) || cold.widget.is_loading();
        if was != now {
            hot.flags |= NodeFlags::DIRTY_PAINT | NodeFlags::DIRTY_A11Y;
        }
    }

    /// `true` while the arena is in reduced-motion mode — loading
    /// placeholders paint as static blocks (`phase = None`) instead of
    /// the animated shimmer sweep. `false` until
    /// [`set_reduced_motion`](Self::set_reduced_motion) installs the
    /// preference.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite_core::WidgetArena;
    ///
    /// let arena = WidgetArena::new();
    /// assert!(!arena.reduced_motion());
    /// ```
    pub fn reduced_motion(&self) -> bool {
        self.reduced_motion
    }

    /// Installs the ambient reduced-motion preference — a
    /// framework-level flag (theme-token equivalent) consumed by the
    /// shared loading painter: while set, every
    /// [`Widget::paint_loading`] receives
    /// `phase = None` and paints the static placeholder. Call when the
    /// OS `prefers-reduced-motion` setting is known or changes;
    /// loading nodes keep repainting on the next
    /// [`tick`](Self::tick) and pick up the static treatment.
    ///
    /// Apps install the OS `prefers-reduced-motion` setting once at
    /// startup via `martensite_window::prefs::apply_platform_preferences`
    /// (env override: `MARTENSITE_REDUCED_MOTION`); per ADR-0040 the
    /// consult is a snapshot — live-change listening is not wired.
    ///
    /// The flag is also *pushed* to every widget via
    /// [`Widget::set_reduced_motion`] so widget-owned animation (a
    /// morphing icon's spring, ADR-0041) can honor it outside the
    /// paint path; widgets inserted while the flag holds are seeded
    /// the same way at insert.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite_core::WidgetArena;
    ///
    /// let mut arena = WidgetArena::new();
    /// arena.set_reduced_motion(true);
    /// assert!(arena.reduced_motion());
    /// ```
    pub fn set_reduced_motion(&mut self, reduced: bool) {
        self.reduced_motion = reduced;
        for id in self.iter_depth_first().collect::<Vec<_>>() {
            if let Some(cold) = self.get_cold_mut(id) {
                cold.widget.set_reduced_motion(reduced);
            }
        }
    }

    /// Return the count of currently active nodes in the arena.
    #[inline(always)]
    pub fn len(&self) -> usize {
        self.hot_nodes.len()
    }

    /// Return true if the arena contains zero active nodes.
    #[inline(always)]
    pub fn is_empty(&self) -> bool {
        self.hot_nodes.is_empty()
    }

    /// Return current allocated dense node capacity.
    #[inline(always)]
    pub fn capacity(&self) -> usize {
        self.hot_nodes.capacity()
    }

    /// Return the number of sparse slot entries currently allocated.
    #[inline(always)]
    pub fn slot_count(&self) -> usize {
        self.slots.len()
    }

    /// Return the number of recycled slot indices awaiting reuse.
    #[inline(always)]
    pub fn free_slots_len(&self) -> usize {
        self.free_slots.len()
    }

    /// Return a read-only view of the dense hot node storage.
    #[inline(always)]
    pub fn hot_nodes(&self) -> &[HotNode] {
        &self.hot_nodes
    }

    /// Return a read-only view of the dense-to-slot reverse mapping.
    #[inline(always)]
    pub fn dense_to_slot(&self) -> &[u32] {
        &self.dense_to_slot
    }

    /// Return the generation of a sparse slot, or `None` if the slot index is out of bounds.
    #[inline(always)]
    pub fn slot_generation(&self, slot_idx: u32) -> Option<u32> {
        self.slots.get(slot_idx as usize).map(|s| s.generation)
    }

    #[cfg(test)]
    /// Test-only helper to directly set a slot's generation value.
    pub fn set_slot_generation_for_test(&mut self, slot_idx: u32, generation: u32) {
        if let Some(slot) = self.slots.get_mut(slot_idx as usize) {
            slot.generation = generation;
        }
    }

    /// Check whether a widget handle is alive and points to a valid active node.
    #[inline(always)]
    pub fn is_alive(&self, id: WidgetId) -> bool {
        self.slots
            .get(id.slot_idx() as usize)
            .is_some_and(|slot| slot.generation == id.generation())
    }

    /// Retrieve an immutable reference to the HotNode for the given widget handle.
    #[inline(always)]
    pub fn get_hot(&self, id: WidgetId) -> Option<&HotNode> {
        let slot = self.slots.get(id.slot_idx() as usize)?;
        if slot.generation == id.generation() {
            Some(&self.hot_nodes[slot.dense_idx as usize])
        } else {
            None
        }
    }

    /// Retrieve a mutable reference to the HotNode for the given widget handle.
    #[inline(always)]
    pub fn get_hot_mut(&mut self, id: WidgetId) -> Option<&mut HotNode> {
        let slot = self.slots.get(id.slot_idx() as usize)?;
        if slot.generation == id.generation() {
            let dense = slot.dense_idx as usize;
            Some(&mut self.hot_nodes[dense])
        } else {
            None
        }
    }

    /// Retrieve an immutable reference to the ColdNode for the given widget handle.
    #[inline(always)]
    pub fn get_cold(&self, id: WidgetId) -> Option<&ColdNode> {
        let slot = self.slots.get(id.slot_idx() as usize)?;
        if slot.generation == id.generation() {
            Some(&self.cold_nodes[slot.dense_idx as usize])
        } else {
            None
        }
    }

    /// Retrieve a mutable reference to the ColdNode for the given widget handle.
    #[inline(always)]
    pub fn get_cold_mut(&mut self, id: WidgetId) -> Option<&mut ColdNode> {
        let slot = self.slots.get(id.slot_idx() as usize)?;
        if slot.generation == id.generation() {
            let dense = slot.dense_idx as usize;
            Some(&mut self.cold_nodes[dense])
        } else {
            None
        }
    }

    /// Retrieve immutable references to both HotNode and ColdNode concurrently.
    #[inline(always)]
    pub fn get_both(&self, id: WidgetId) -> Option<(&HotNode, &ColdNode)> {
        let slot = self.slots.get(id.slot_idx() as usize)?;
        if slot.generation == id.generation() {
            let dense = slot.dense_idx as usize;
            Some((&self.hot_nodes[dense], &self.cold_nodes[dense]))
        } else {
            None
        }
    }

    /// Retrieve mutable references to both HotNode and ColdNode concurrently.
    #[inline(always)]
    pub fn get_both_mut(&mut self, id: WidgetId) -> Option<(&mut HotNode, &mut ColdNode)> {
        let slot = self.slots.get(id.slot_idx() as usize)?;
        if slot.generation == id.generation() {
            let dense = slot.dense_idx as usize;
            Some((&mut self.hot_nodes[dense], &mut self.cold_nodes[dense]))
        } else {
            None
        }
    }

    /// Insert a new node into the arena, returning a generational handle.
    pub fn insert(&mut self, hot: HotNode, cold: ColdNode) -> WidgetId {
        let mut cold = cold;
        // A node born while reduced motion holds sees the preference
        // immediately — same push `set_reduced_motion` delivers to
        // existing widgets.
        if self.reduced_motion {
            cold.widget.set_reduced_motion(true);
        }
        let dense_idx = self.hot_nodes.len() as u32;
        self.hot_nodes.push(hot);
        self.cold_nodes.push(cold);

        // Strict FIFO slot recycling: pop from the front of the queue
        let slot_idx = if let Some(free_idx) = self.free_slots.pop_front() {
            let slot = &mut self.slots[free_idx as usize];
            slot.dense_idx = dense_idx;
            free_idx
        } else {
            let idx = self.slots.len() as u32;
            self.slots.push(Slot {
                generation: 1,
                dense_idx,
            });
            idx
        };

        self.dense_to_slot.push(slot_idx);
        // WidgetId::new returns None only if generation is zero.
        // Generation is set to 1 above for new slots and incremented
        // (skipping zero) for reused slots, so it is always >= 1.
        // The fallback handles the theoretical edge case where
        // generation wraps, which is unreachable in practice.
        WidgetId::new(slot_idx, self.slots[slot_idx as usize].generation).unwrap_or_else(|| {
            // Fallback: construct a valid WidgetId with generation 1.
            // This path is unreachable but avoids a panic.
            WidgetId::from_parts(slot_idx, 1)
        })
    }

    /// Insert a new node wrapping a boxed widget implementation with default cold metadata.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite_core::{DummyWidget, HotNode, WidgetArena};
    ///
    /// let mut arena = WidgetArena::new();
    /// let id = arena.insert_with_widget(HotNode::default(), Box::new(DummyWidget));
    /// assert!(arena.is_alive(id));
    /// assert_eq!(arena.len(), 1);
    /// ```
    pub fn insert_with_widget(
        &mut self,
        hot: HotNode,
        widget: Box<dyn crate::widget::Widget>,
    ) -> WidgetId {
        self.insert(hot, ColdNode::new(widget))
    }

    /// Remove a node from the arena, unparenting its children and detaching from its hierarchy.
    ///
    /// Overlay popups owned by the widget (stamped during
    /// [`sync_overlays`](Self::sync_overlays)) are closed so they cannot
    /// keep painting/hit-testing with a dead owner, and a pending focus
    /// request naming this widget is dropped.
    pub fn remove(&mut self, id: WidgetId) -> Option<(HotNode, ColdNode)> {
        let slot = self.slots.get(id.slot_idx() as usize)?;
        if slot.generation != id.generation() {
            return None;
        }

        // 0. Orphaned state: close popups this widget owns and drop a
        // stale focus request — a dead widget must not leave its popup
        // painting above content or focus a non-existent node.
        self.overlay.close_owner(id);
        if self.pending_focus == Some(id) {
            self.pending_focus = None;
        }

        // 1. Unparent all immediate children so they become clean root-level nodes
        let mut child_opt = self.first_child(id);
        if let Some(hot) = self.get_hot_mut(id) {
            hot.first_child = None;
        }
        while let Some(child_id) = child_opt {
            let next_sibling = self.next_sibling(child_id);
            if let Some(hot) = self.get_hot_mut(child_id) {
                hot.parent = None;
                hot.prev_sibling = None;
                hot.next_sibling = None;
                hot.depth_rank = 0;
            }
            self.update_subtree_depths(child_id, 0);
            child_opt = next_sibling;
        }

        // 2. Detach the target node from its parent and sibling chains.
        // Detach failure is acceptable here because the node is being
        // removed entirely; if it was already detached, that's fine.
        if self.detach(id).is_err() {
            // Node may have already been detached; continue with removal.
        }

        // 3. Re-read slot reference to advance generation skipping zero
        let slot = &mut self.slots[id.slot_idx() as usize];
        slot.generation = if slot.generation == u32::MAX {
            1
        } else {
            slot.generation + 1
        };

        let removed_dense = slot.dense_idx as usize;
        let last_dense = self.hot_nodes.len() - 1;

        let hot = self.hot_nodes.swap_remove(removed_dense);
        let cold = self.cold_nodes.swap_remove(removed_dense);
        self.dense_to_slot.swap_remove(removed_dense);

        // FIFO recycling: push freed slot index to the back
        self.free_slots.push_back(id.slot_idx());

        if removed_dense != last_dense {
            let relocated_slot_idx = self.dense_to_slot[removed_dense] as usize;
            self.slots[relocated_slot_idx].dense_idx = removed_dense as u32;
        }

        Some((hot, cold))
    }

    // --- Tree Hierarchy Accessors ---

    /// Retrieve the parent handle of the specified node.
    #[inline(always)]
    pub fn parent(&self, id: WidgetId) -> Option<WidgetId> {
        if !self.is_alive(id) {
            return None;
        }
        self.get_hot(id).and_then(|h| h.parent)
    }

    /// Retrieve the first child handle of the specified node.
    #[inline(always)]
    pub fn first_child(&self, id: WidgetId) -> Option<WidgetId> {
        if !self.is_alive(id) {
            return None;
        }
        self.get_hot(id).and_then(|h| h.first_child)
    }

    /// Retrieve the last child handle of the specified node.
    pub fn last_child(&self, id: WidgetId) -> Option<WidgetId> {
        let mut curr = self.first_child(id)?;
        while let Some(next) = self.next_sibling(curr) {
            curr = next;
        }
        Some(curr)
    }

    /// Retrieve the next sibling handle of the specified node.
    #[inline(always)]
    pub fn next_sibling(&self, id: WidgetId) -> Option<WidgetId> {
        if !self.is_alive(id) {
            return None;
        }
        self.get_hot(id).and_then(|h| h.next_sibling)
    }

    /// Retrieve the previous sibling handle of the specified node.
    #[inline(always)]
    pub fn prev_sibling(&self, id: WidgetId) -> Option<WidgetId> {
        if !self.is_alive(id) {
            return None;
        }
        self.get_hot(id).and_then(|h| h.prev_sibling)
    }

    /// Retrieve the topological depth rank of the specified node.
    #[inline(always)]
    pub fn depth_rank(&self, id: WidgetId) -> Option<u16> {
        if !self.is_alive(id) {
            return None;
        }
        self.get_hot(id).map(|h| h.depth_rank)
    }

    /// Check if `ancestor` is an ancestor of `descendant` (or identical).
    pub fn is_ancestor_of(&self, ancestor: WidgetId, descendant: WidgetId) -> bool {
        if ancestor == descendant {
            return true;
        }
        let mut curr = self.parent(descendant);
        while let Some(p) = curr {
            if p == ancestor {
                return true;
            }
            curr = self.parent(p);
        }
        false
    }

    // --- Tree Hierarchy Mutations ---

    /// Detach a node from its parent and sibling chains, leaving it as an unattached root.
    pub fn detach(&mut self, id: WidgetId) -> Result<(), ArenaError> {
        if !self.is_alive(id) {
            return Err(ArenaError::InvalidNode(id));
        }

        let (parent_opt, prev_opt, next_opt) = {
            let hot = self.get_hot(id).expect("arena invariant");
            (hot.parent, hot.prev_sibling, hot.next_sibling)
        };

        // Update parent's first_child pointer if id was head
        if let Some(parent_id) = parent_opt {
            if self.is_alive(parent_id) {
                let is_first = self
                    .get_hot(parent_id)
                    .expect("arena invariant")
                    .first_child
                    == Some(id);
                if is_first {
                    self.get_hot_mut(parent_id)
                        .expect("arena invariant")
                        .first_child = next_opt;
                }
            }
        }

        // Link prev sibling to next sibling
        if let Some(prev_id) = prev_opt {
            if self.is_alive(prev_id) {
                self.get_hot_mut(prev_id)
                    .expect("arena invariant")
                    .next_sibling = next_opt;
            }
        }

        // Link next sibling to prev sibling
        if let Some(next_id) = next_opt {
            if self.is_alive(next_id) {
                self.get_hot_mut(next_id)
                    .expect("arena invariant")
                    .prev_sibling = prev_opt;
            }
        }

        // Clear id's sibling and parent links
        {
            let hot = self.get_hot_mut(id).expect("arena invariant");
            hot.parent = None;
            hot.prev_sibling = None;
            hot.next_sibling = None;
            hot.depth_rank = 0;
        }

        self.update_subtree_depths(id, 0);
        Ok(())
    }

    /// Append a child node to the end of a parent's children list.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite_core::{ArenaError, DummyWidget, HotNode, WidgetArena};
    ///
    /// let mut arena = WidgetArena::new();
    /// let parent = arena.insert_with_widget(HotNode::default(), Box::new(DummyWidget));
    /// let a = arena.insert_with_widget(HotNode::default(), Box::new(DummyWidget));
    /// let b = arena.insert_with_widget(HotNode::default(), Box::new(DummyWidget));
    ///
    /// arena.append_child(parent, a).unwrap();
    /// arena.append_child(parent, b).unwrap();
    /// assert_eq!(arena.children(parent).collect::<Vec<_>>(), vec![a, b]);
    /// assert_eq!(arena.parent(a), Some(parent));
    ///
    /// // Forming a cycle is rejected.
    /// assert_eq!(arena.append_child(a, parent).unwrap_err(), ArenaError::CycleDetected);
    /// ```
    pub fn append_child(&mut self, parent: WidgetId, child: WidgetId) -> Result<(), ArenaError> {
        if !self.is_alive(parent) {
            return Err(ArenaError::InvalidParent(parent));
        }
        if !self.is_alive(child) {
            return Err(ArenaError::InvalidChild(child));
        }
        if parent == child {
            return Err(ArenaError::SelfParenting(parent));
        }
        if self.is_ancestor_of(child, parent) {
            return Err(ArenaError::CycleDetected);
        }

        // Detach child from existing location
        self.detach(child)?;

        let parent_first = self.get_hot(parent).expect("arena invariant").first_child;
        match parent_first {
            None => {
                self.get_hot_mut(parent)
                    .expect("arena invariant")
                    .first_child = Some(child);
                let child_hot = self.get_hot_mut(child).expect("arena invariant");
                child_hot.parent = Some(parent);
                child_hot.prev_sibling = None;
                child_hot.next_sibling = None;
            }
            Some(first) => {
                let mut last = first;
                while let Some(next) = self.next_sibling(last) {
                    last = next;
                }
                self.get_hot_mut(last)
                    .expect("arena invariant")
                    .next_sibling = Some(child);
                let child_hot = self.get_hot_mut(child).expect("arena invariant");
                child_hot.parent = Some(parent);
                child_hot.prev_sibling = Some(last);
                child_hot.next_sibling = None;
            }
        }

        let parent_depth = self.get_hot(parent).expect("arena invariant").depth_rank;
        let child_depth = parent_depth.saturating_add(1);
        self.get_hot_mut(child).expect("arena invariant").depth_rank = child_depth;
        self.update_subtree_depths(child, child_depth);

        Ok(())
    }

    /// Prepend a child node to the beginning of a parent's children list.
    pub fn prepend_child(&mut self, parent: WidgetId, child: WidgetId) -> Result<(), ArenaError> {
        if !self.is_alive(parent) {
            return Err(ArenaError::InvalidParent(parent));
        }
        if !self.is_alive(child) {
            return Err(ArenaError::InvalidChild(child));
        }
        if parent == child {
            return Err(ArenaError::SelfParenting(parent));
        }
        if self.is_ancestor_of(child, parent) {
            return Err(ArenaError::CycleDetected);
        }

        self.detach(child)?;

        let old_first = self.get_hot(parent).expect("arena invariant").first_child;
        self.get_hot_mut(parent)
            .expect("arena invariant")
            .first_child = Some(child);

        let child_hot = self.get_hot_mut(child).expect("arena invariant");
        child_hot.parent = Some(parent);
        child_hot.prev_sibling = None;
        child_hot.next_sibling = old_first;

        if let Some(old_first_id) = old_first {
            self.get_hot_mut(old_first_id)
                .expect("arena invariant")
                .prev_sibling = Some(child);
        }

        let parent_depth = self.get_hot(parent).expect("arena invariant").depth_rank;
        let child_depth = parent_depth.saturating_add(1);
        self.get_hot_mut(child).expect("arena invariant").depth_rank = child_depth;
        self.update_subtree_depths(child, child_depth);

        Ok(())
    }

    /// Insert a node immediately before a target sibling.
    pub fn insert_before(&mut self, target: WidgetId, node: WidgetId) -> Result<(), ArenaError> {
        if !self.is_alive(target) {
            return Err(ArenaError::InvalidTarget(target));
        }
        if !self.is_alive(node) {
            return Err(ArenaError::InvalidNode(node));
        }
        if target == node {
            return Err(ArenaError::InvalidOperation(
                "Cannot insert node before itself",
            ));
        }

        let parent = self.parent(target).ok_or(ArenaError::NoParent(target))?;
        if self.is_ancestor_of(node, parent) {
            return Err(ArenaError::CycleDetected);
        }

        self.detach(node)?;

        let target_prev = self.get_hot(target).expect("arena invariant").prev_sibling;
        {
            let node_hot = self.get_hot_mut(node).expect("arena invariant");
            node_hot.parent = Some(parent);
            node_hot.prev_sibling = target_prev;
            node_hot.next_sibling = Some(target);
        }

        self.get_hot_mut(target)
            .expect("arena invariant")
            .prev_sibling = Some(node);

        if let Some(prev_id) = target_prev {
            self.get_hot_mut(prev_id)
                .expect("arena invariant")
                .next_sibling = Some(node);
        } else {
            self.get_hot_mut(parent)
                .expect("arena invariant")
                .first_child = Some(node);
        }

        let parent_depth = self.get_hot(parent).expect("arena invariant").depth_rank;
        let node_depth = parent_depth.saturating_add(1);
        self.get_hot_mut(node).expect("arena invariant").depth_rank = node_depth;
        self.update_subtree_depths(node, node_depth);

        Ok(())
    }

    /// Insert a node immediately after a target sibling.
    pub fn insert_after(&mut self, target: WidgetId, node: WidgetId) -> Result<(), ArenaError> {
        if !self.is_alive(target) {
            return Err(ArenaError::InvalidTarget(target));
        }
        if !self.is_alive(node) {
            return Err(ArenaError::InvalidNode(node));
        }
        if target == node {
            return Err(ArenaError::InvalidOperation(
                "Cannot insert node after itself",
            ));
        }

        let parent = self.parent(target).ok_or(ArenaError::NoParent(target))?;
        if self.is_ancestor_of(node, parent) {
            return Err(ArenaError::CycleDetected);
        }

        self.detach(node)?;

        let target_next = self.get_hot(target).expect("arena invariant").next_sibling;
        {
            let node_hot = self.get_hot_mut(node).expect("arena invariant");
            node_hot.parent = Some(parent);
            node_hot.prev_sibling = Some(target);
            node_hot.next_sibling = target_next;
        }

        self.get_hot_mut(target)
            .expect("arena invariant")
            .next_sibling = Some(node);

        if let Some(next_id) = target_next {
            self.get_hot_mut(next_id)
                .expect("arena invariant")
                .prev_sibling = Some(node);
        }

        let parent_depth = self.get_hot(parent).expect("arena invariant").depth_rank;
        let node_depth = parent_depth.saturating_add(1);
        self.get_hot_mut(node).expect("arena invariant").depth_rank = node_depth;
        self.update_subtree_depths(node, node_depth);

        Ok(())
    }

    /// Remove a child from a parent node.
    pub fn remove_child(&mut self, parent: WidgetId, child: WidgetId) -> Result<(), ArenaError> {
        if !self.is_alive(parent) {
            return Err(ArenaError::InvalidParent(parent));
        }
        if !self.is_alive(child) {
            return Err(ArenaError::InvalidChild(child));
        }
        if self.parent(child) != Some(parent) {
            return Err(ArenaError::NotAChild { parent, child });
        }

        self.detach(child)
    }

    /// Recursively update depth ranks across all descendants of the specified node.
    fn update_subtree_depths(&mut self, root: WidgetId, root_depth: u16) {
        let mut child_opt = self.first_child(root);
        while let Some(child_id) = child_opt {
            let next_sibling = self.next_sibling(child_id);
            let child_depth = root_depth.saturating_add(1);
            if let Some(hot) = self.get_hot_mut(child_id) {
                hot.depth_rank = child_depth;
            }
            self.update_subtree_depths(child_id, child_depth);
            child_opt = next_sibling;
        }
    }

    // --- Iterators ---

    /// Returns a zero-allocation iterator over the immediate children of `id`.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite_core::{DummyWidget, HotNode, WidgetArena};
    ///
    /// let mut arena = WidgetArena::new();
    /// let parent = arena.insert_with_widget(HotNode::default(), Box::new(DummyWidget));
    /// let a = arena.insert_with_widget(HotNode::default(), Box::new(DummyWidget));
    /// let b = arena.insert_with_widget(HotNode::default(), Box::new(DummyWidget));
    /// arena.append_child(parent, a).unwrap();
    /// arena.append_child(parent, b).unwrap();
    ///
    /// // Forward iteration yields children in insertion order.
    /// assert_eq!(arena.children(parent).collect::<Vec<_>>(), vec![a, b]);
    /// // `Children` is a double-ended iterator.
    /// assert_eq!(arena.children(parent).rev().collect::<Vec<_>>(), vec![b, a]);
    /// ```
    pub fn children(&self, id: WidgetId) -> Children<'_> {
        let front = self.first_child(id);
        let back = self.last_child(id);
        Children {
            arena: self,
            front,
            back,
        }
    }

    /// Returns a zero-allocation depth-first pre-order iterator over the subtree rooted at `id`.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite_core::{DummyWidget, HotNode, WidgetArena};
    ///
    /// let mut arena = WidgetArena::new();
    /// // Build: root
    /// //        ├── a
    /// //        │   └── a1
    /// //        └── b
    /// let root = arena.insert_with_widget(HotNode::default(), Box::new(DummyWidget));
    /// let a = arena.insert_with_widget(HotNode::default(), Box::new(DummyWidget));
    /// let a1 = arena.insert_with_widget(HotNode::default(), Box::new(DummyWidget));
    /// let b = arena.insert_with_widget(HotNode::default(), Box::new(DummyWidget));
    /// arena.append_child(root, a).unwrap();
    /// arena.append_child(a, a1).unwrap();
    /// arena.append_child(root, b).unwrap();
    ///
    /// // Pre-order DFS visits parents before their children.
    /// assert_eq!(arena.iter_subtree(root).collect::<Vec<_>>(), vec![root, a, a1, b]);
    /// ```
    pub fn iter_subtree(&self, id: WidgetId) -> SubtreeIter<'_> {
        SubtreeIter {
            arena: self,
            root: id,
            current: None,
            started: false,
        }
    }

    /// Returns a zero-allocation depth-first iterator visiting all trees in the arena.
    pub fn iter_depth_first(&self) -> DepthFirstIter<'_> {
        DepthFirstIter {
            arena: self,
            root_dense_idx: 0,
            current_subtree: None,
        }
    }

    /// Renders the widget hierarchy as an indented tree for debugging —
    /// one `Name [x,y w×h]` line per node, recursing through both arena
    /// children and widget-internal children (the `child`/`child_bounds`
    /// protocol), so composite widgets show their real contents rather
    /// than a single opaque node. Instance `debug_name`s win over type
    /// names, matching the paint-audit scope labels.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite_core::{DummyWidget, HotNode, NodeFlags, WidgetArena};
    ///
    /// let mut arena = WidgetArena::new();
    /// let mut hot = HotNode::default();
    /// hot.flags |= NodeFlags::VISIBLE;
    /// let root = arena.insert_with_widget(hot, Box::new(DummyWidget));
    /// let tree = arena.debug_tree();
    /// assert!(tree.contains("DummyWidget"));
    /// ```
    pub fn debug_tree(&self) -> String {
        let mut out = String::new();
        let mut prefixes: Vec<bool> = Vec::new();
        // Roots: live nodes with no parent and no previous sibling,
        // in slot order (which is insertion order until slots recycle).
        for i in 0..self.hot_nodes.len() {
            let hot = &self.hot_nodes[i];
            if hot.parent.is_some() || hot.prev_sibling.is_some() {
                continue;
            }
            let slot_idx = self.dense_to_slot[i];
            let slot = self.slots[slot_idx as usize];
            let id = WidgetId::from_parts(slot_idx, slot.generation);
            let more_roots = self
                .hot_nodes
                .iter()
                .enumerate()
                .skip(i + 1)
                .any(|(_, h)| h.parent.is_none() && h.prev_sibling.is_none());
            self.fmt_arena_node(id, more_roots, &mut prefixes, &mut out);
        }
        out
    }

    /// Formats one arena node and its whole subtree into `out`.
    /// `prefixes` tracks, per ancestor level, whether a `│` continuation
    /// is needed; `has_next_root`/`next_sibling` decide `├──` vs `└──`.
    fn fmt_arena_node(
        &self,
        id: WidgetId,
        more_siblings: bool,
        prefixes: &mut Vec<bool>,
        out: &mut String,
    ) {
        let (Some(hot), Some(cold)) = (self.get_hot(id), self.get_cold(id)) else {
            return;
        };
        for &cont in prefixes.iter() {
            out.push_str(if cont { "│   " } else { "    " });
        }
        out.push_str(if more_siblings {
            "├── "
        } else {
            "└── "
        });
        let name = cold.debug_name.unwrap_or_else(|| cold.widget.debug_name());
        let b = hot.bounds;
        let _ = std::fmt::Write::write_fmt(
            out,
            format_args!(
                "{name} [{:.0},{:.0} {:.0}×{:.0}]",
                b.origin.x, b.origin.y, b.size.x, b.size.y
            ),
        );
        if !hot.flags.contains(crate::node::NodeFlags::VISIBLE) {
            out.push_str(" (hidden)");
        }
        out.push('\n');
        // Arena children, then widget-internal children — the same
        // document order the paint walk uses.
        let kids: Vec<WidgetId> = self.children(id).collect();
        let kid_count = kids.len();
        let inner_count = cold.widget.child_count();
        prefixes.push(more_siblings);
        for (i, kid) in kids.iter().enumerate() {
            self.fmt_arena_node(*kid, i + 1 < kid_count || inner_count > 0, prefixes, out);
        }
        for i in 0..inner_count {
            self.fmt_internal_node(&*cold.widget, i, i + 1 < inner_count, prefixes, out);
        }
        prefixes.pop();
    }

    /// Formats one widget-internal child subtree into `out`.
    fn fmt_internal_node(
        &self,
        parent: &dyn crate::widget::Widget,
        index: usize,
        more_siblings: bool,
        prefixes: &mut Vec<bool>,
        out: &mut String,
    ) {
        let Some(child) = parent.child(index) else {
            return;
        };
        for &cont in prefixes.iter() {
            out.push_str(if cont { "│   " } else { "    " });
        }
        out.push_str(if more_siblings {
            "├── "
        } else {
            "└── "
        });
        out.push_str(child.debug_name());
        if let Some(b) = parent.child_bounds(index) {
            let _ = std::fmt::Write::write_fmt(
                out,
                format_args!(
                    " [{:.0},{:.0} {:.0}×{:.0}]",
                    b.origin.x, b.origin.y, b.size.x, b.size.y
                ),
            );
        }
        out.push('\n');
        let n = child.child_count();
        prefixes.push(more_siblings);
        for i in 0..n {
            self.fmt_internal_node(child, i, i + 1 < n, prefixes, out);
        }
        prefixes.pop();
    }

    /// Returns a breadth-first iterator visiting all trees in the arena level-by-level.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite_core::{DummyWidget, HotNode, WidgetArena};
    ///
    /// let mut arena = WidgetArena::new();
    /// // Build: root
    /// //        ├── a
    /// //        │   └── a1
    /// //        └── b
    /// let root = arena.insert_with_widget(HotNode::default(), Box::new(DummyWidget));
    /// let a = arena.insert_with_widget(HotNode::default(), Box::new(DummyWidget));
    /// let a1 = arena.insert_with_widget(HotNode::default(), Box::new(DummyWidget));
    /// let b = arena.insert_with_widget(HotNode::default(), Box::new(DummyWidget));
    /// arena.append_child(root, a).unwrap();
    /// arena.append_child(a, a1).unwrap();
    /// arena.append_child(root, b).unwrap();
    ///
    /// // BFS visits each level fully before descending: root, then a & b, then a1.
    /// assert_eq!(arena.iter_breadth_first().collect::<Vec<_>>(), vec![root, a, b, a1]);
    /// ```
    pub fn iter_breadth_first(&self) -> BreadthFirstIter<'_> {
        let mut queue = VecDeque::new();
        for i in 0..self.hot_nodes.len() {
            let hot = &self.hot_nodes[i];
            if hot.parent.is_none() && hot.prev_sibling.is_none() {
                let slot_idx = self.dense_to_slot[i];
                let slot = self.slots[slot_idx as usize];
                // Generation is never zero for an active slot (see arena docs).
                queue.push_back(WidgetId::from_parts(slot_idx, slot.generation));
            }
        }
        BreadthFirstIter { arena: self, queue }
    }

    /// Returns a breadth-first iterator visiting the subtree rooted at `root` level-by-level.
    pub fn iter_subtree_breadth_first(&self, root: WidgetId) -> BreadthFirstIter<'_> {
        let mut queue = VecDeque::new();
        if self.is_alive(root) {
            queue.push_back(root);
        }
        BreadthFirstIter { arena: self, queue }
    }

    // --- Compaction and Reader Synchronization ---

    /// Synchronizes with active reader leases before commencing arena compaction.
    ///
    /// Blocks until all readers drop their leases or the configured timeout elapses,
    /// in which case stale reader leases are forcibly reclaimed and the epoch is incremented.
    pub fn begin_compaction(&mut self, fence: &FrameFence) -> Result<(), ArenaError> {
        fence.mark_compaction_start();
        fence.wait_for_quiescence();
        Ok(())
    }

    /// Concludes an active compaction pass, clearing the compaction flag.
    pub fn end_compaction(&mut self, fence: &FrameFence) {
        fence.mark_compaction_end();
    }

    /// Shrinks unused capacity across all arena buffers to compact memory during idle periods.
    pub fn shrink_to_fit_idle(&mut self) {
        if self.dense_to_slot.is_empty() {
            self.slots.clear();
            self.free_slots.clear();
        } else {
            let max_active_slot = self.dense_to_slot.iter().copied().max().unwrap_or(0);
            let needed_slots = (max_active_slot + 1) as usize;
            if needed_slots < self.slots.len() {
                self.slots.truncate(needed_slots);
                self.free_slots
                    .retain(|&slot_idx| slot_idx <= max_active_slot);
            }
        }

        self.hot_nodes.shrink_to_fit();
        self.cold_nodes.shrink_to_fit();
        self.dense_to_slot.shrink_to_fit();
        self.slots.shrink_to_fit();
        self.free_slots.shrink_to_fit();
    }

    /// Coordinates compaction synchronization via FrameFence and executes idle shrink_to_fit.
    pub fn compact_and_shrink_idle(&mut self, fence: &FrameFence) -> Result<(), ArenaError> {
        self.begin_compaction(fence)?;
        self.shrink_to_fit_idle();
        self.end_compaction(fence);
        Ok(())
    }

    /// Deliver `event` to the widget at `target`, bubbling to arena
    /// ancestors while widgets return [`EventResponse::Ignored`].
    ///
    /// The hit-test resolution (which `WidgetId` receives a positional
    /// event) is the caller's job — `martensite-window`'s `EventRouter`
    /// produces the target. This method owns in-arena propagation:
    ///
    /// - `INERT` nodes are skipped (they ignore input) — the event keeps
    ///   bubbling to the next ancestor.
    /// - [`EventResponse::RequestRepaint`] additionally marks the
    ///   responding node [`NodeFlags::DIRTY_PAINT`].
    /// - Within a node, the widget's own `event` implementation governs
    ///   internal children (the trait default forwards to
    ///   [`Widget::child_mut`] in reverse order,
    ///   gated on [`Widget::child_bounds`]).
    ///
    /// Returns the terminal response, or `Ignored` if the event bubbled
    /// past the root.
    ///
    /// # Examples
    ///
    /// ```
    /// use glam::Vec2;
    /// use martensite_core::{
    ///     DummyWidget, EventResponse, HotNode, PointerButton, WidgetArena, WidgetEvent,
    /// };
    ///
    /// let mut arena = WidgetArena::new();
    /// let root = arena.insert_with_widget(HotNode::default(), Box::new(DummyWidget));
    ///
    /// // `DummyWidget` ignores input — the event bubbles past the root.
    /// let event = WidgetEvent::PointerPressed {
    ///     position: Vec2::ZERO,
    ///     button: PointerButton::Primary,
    ///     count: 1,
    /// };
    /// assert_eq!(arena.dispatch_event(root, &event), EventResponse::Ignored);
    /// ```
    pub fn dispatch_event(&mut self, target: WidgetId, event: &WidgetEvent) -> EventResponse {
        let _intl = self.install_ambient_intl();
        self.dispatch_event_ex(target, event)
            .map(|(_, response)| response)
            .unwrap_or(EventResponse::Ignored)
    }

    /// Like [`Self::dispatch_event`], but also reports the arena node
    /// that produced the terminal response.
    ///
    /// Event routers need the responder's [`WidgetId`] to apply
    /// [`EventResponse::CapturePointer`] /
    /// [`EventResponse::ReleasePointer`] to the widget that actually
    /// handled the event — which may be an ancestor of the hit target
    /// after bubbling — rather than to the hit target itself.
    ///
    /// Returns `Some((responder, response))` when a widget handled the
    /// event, or `None` if it bubbled past the root.
    pub fn dispatch_event_ex(
        &mut self,
        target: WidgetId,
        event: &WidgetEvent,
    ) -> Option<(WidgetId, EventResponse)> {
        let _intl = self.install_ambient_intl();
        // A loading node covers its whole subtree, and keyboard focus /
        // SemanticAction delivery reach targets without hit-testing —
        // dispatch must begin above the topmost loading ancestor rather
        // than trusting the bubble walk to skip it after intermediate
        // descendants already ran.
        let mut current = Some(target);
        let mut cursor = Some(target);
        while let Some(id) = cursor {
            let parent = self.get_hot(id).and_then(|h| h.parent);
            if self.effective_loading(id) {
                current = parent;
            }
            cursor = parent;
        }
        while let Some(id) = current {
            let Some(hot) = self.get_hot(id) else {
                break;
            };
            let bounds = hot.bounds;
            let parent = hot.parent;
            // Inert, invisible, input-covered (engaged Hide/Collapse/
            // Scrim), and loading nodes never receive events — the
            // event bubbles to the parent instead.
            if hot.flags.contains(NodeFlags::INERT) || !hot.flags.contains(NodeFlags::VISIBLE) {
                current = parent;
                continue;
            }
            let covered = self
                .get_cold(id)
                .and_then(|c| c.underflow_policy())
                .is_some_and(|p| p.covers_input())
                || self.effective_loading(id);
            if covered {
                current = parent;
                continue;
            }
            let scale = self.scale_factor;
            let Some(cold) = self.get_cold_mut(id) else {
                break;
            };
            let mut cx = EventContext {
                event,
                bounds,
                scale,
            };
            let response = cold.widget.event(&mut cx);
            match response {
                EventResponse::Ignored => current = parent,
                EventResponse::CaptureFocus => {
                    // Explicit focus request: record the responder and
                    // repaint so a focus indicator can appear.
                    self.pending_focus = Some(id);
                    if let Some(h) = self.get_hot_mut(id) {
                        h.flags |= NodeFlags::DIRTY_PAINT | NodeFlags::DIRTY_A11Y;
                    }
                    return Some((id, response));
                }
                EventResponse::RequestRepaint
                | EventResponse::CapturePointer
                | EventResponse::ReleasePointer => {
                    // Press-to-focus: a handled press on a focusable
                    // node requests focus implicitly, matching
                    // platform mousedown-to-focus conventions.
                    if matches!(event, WidgetEvent::PointerPressed { .. })
                        && self
                            .get_hot(id)
                            .is_some_and(|h| h.flags.contains(NodeFlags::FOCUSABLE))
                    {
                        self.pending_focus = Some(id);
                    }
                    // A handled event may change emitted accessibility
                    // state (expanded, selected, value) — mark the
                    // responder for re-emission in the next incremental
                    // `TreeUpdate`, alongside the repaint.
                    if let Some(h) = self.get_hot_mut(id) {
                        h.flags |= NodeFlags::DIRTY_PAINT | NodeFlags::DIRTY_A11Y;
                    }
                    return Some((id, response));
                }
                other => {
                    // `Handled` — still honour implicit press-to-focus,
                    // and dirty-mark like the responses above: handled
                    // events routinely mutate emitted a11y state.
                    if matches!(event, WidgetEvent::PointerPressed { .. }) {
                        if let Some(h) = self.get_hot_mut(id) {
                            if h.flags.contains(NodeFlags::FOCUSABLE) {
                                self.pending_focus = Some(id);
                            }
                        }
                    }
                    if let Some(h) = self.get_hot_mut(id) {
                        h.flags |= NodeFlags::DIRTY_PAINT | NodeFlags::DIRTY_A11Y;
                    }
                    return Some((id, other));
                }
            }
        }
        None
    }

    /// Walks a `Widget::child_mut` index path from an arena widget to a
    /// nested internal child, returning it mutably.
    ///
    /// Used to deliver accessibility actions to internal targets: the
    /// adapter's `resolve_internal` yields `(owner, path)` pairs and
    /// this reaches the widget at that path — the internal analogue of
    /// `OverlayLayer::widget_at_mut`.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite_core::{DummyWidget, HotNode, WidgetArena};
    ///
    /// let mut arena = WidgetArena::new();
    /// let root = arena.insert_with_widget(HotNode::default(), Box::new(DummyWidget));
    /// // `DummyWidget` has no internal children — only the empty path resolves.
    /// assert!(arena.internal_widget_mut(root, &[]).is_some());
    /// assert!(arena.internal_widget_mut(root, &[0]).is_none());
    /// ```
    pub fn internal_widget_mut(
        &mut self,
        owner: WidgetId,
        path: &[u32],
    ) -> Option<&mut dyn crate::Widget> {
        let cold = self.get_cold_mut(owner)?;
        let mut widget: &mut dyn crate::Widget = &mut *cold.widget;
        for &index in path {
            widget = widget.child_mut(index as usize)?;
        }
        Some(widget)
    }

    /// Record the visible subtree rooted at `root` into `list`, in
    /// document paint order.
    ///
    /// For each arena node: invisible subtrees are skipped entirely, the
    /// widget's own `paint` emits its chrome first, then internal
    /// children (via the `Widget::child_count`/`child`/`child_bounds`
    /// protocol), then arena children in sibling order. Popups open in
    /// the arena-owned [`OverlayLayer`]
    /// are appended last — above all window content.
    ///
    /// Every widget's commands are wrapped in
    /// [`PaintCommand::PushScope`](crate::PaintCommand::PushScope) /
    /// [`PaintCommand::PopScope`](crate::PaintCommand::PopScope)
    /// provenance markers — nested so the scope tree mirrors the widget
    /// tree — letting downstream tooling (the paint audit) attribute
    /// findings to the emitting widget.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite_core::{
    ///     DummyWidget, HotNode, NodeFlags, PaintList, WidgetArena,
    /// };
    ///
    /// let mut arena = WidgetArena::new();
    /// let mut hot = HotNode::default();
    /// hot.flags = NodeFlags::VISIBLE;
    /// let root = arena.insert_with_widget(hot, Box::new(DummyWidget));
    ///
    /// let mut list = PaintList::new();
    /// arena.build_paint_list(root, &mut list);
    /// // `DummyWidget` emits no chrome — only its provenance scope.
    /// assert_eq!(list.commands.len(), 2);
    /// ```
    pub fn build_paint_list(&self, root: WidgetId, list: &mut PaintList) {
        let _intl = self.install_ambient_intl();
        // `None` = no known viewport (an unlaid-out root has degenerate
        // bounds) — nothing is culled. `Some` = the surface rect, the
        // outermost region any paint can show through.
        let visible = self
            .get_hot(root)
            .map(|h| h.bounds)
            .filter(|b| has_area(*b));
        self.paint_node(root, list, visible, &self.theme);
        // In-window popups paint above everything else, on the same
        // shimmer clock as arena skeletons.
        self.overlay.paint_with_phase(
            list,
            &self.theme,
            self.text_painter.as_deref(),
            self.loading_phase(),
        );
    }

    /// The shimmer phase for this frame — `None` under reduced motion
    /// (static placeholder), else the shared clock's position in the
    /// sweep.
    fn loading_phase(&self) -> Option<f32> {
        if self.reduced_motion {
            None
        } else {
            Some(crate::loading::sweep_phase(self.loading_elapsed))
        }
    }

    /// Recursive helper for [`WidgetArena::build_paint_list`].
    ///
    /// `visible` is the intersection of every ancestor clip — a node
    /// whose bounds do not intersect it emits only dead commands
    /// (backend-clipped output), so the whole subtree is skipped.
    /// This is what virtualizes scrolled containers: a 300-cell
    /// scroll region paints its handful of onscreen cells, not all
    /// 300 every frame. `None` means no region is known (unlaid-out
    /// tree) and nothing is culled; a node with degenerate bounds is
    /// kept regardless — it may still emit commands the paint audit
    /// needs to see.
    fn paint_node(
        &self,
        id: WidgetId,
        list: &mut PaintList,
        visible: Option<crate::Rect>,
        inherited_theme: &martensite_theme::Theme,
    ) {
        let Some(hot) = self.get_hot(id) else {
            return;
        };
        if !hot.flags.contains(NodeFlags::VISIBLE) {
            return;
        }
        if !paints_in(hot.bounds, visible) {
            return;
        }
        let Some(cold) = self.get_cold(id) else {
            return;
        };

        // Per-widget subtree overrides — the ambient guard lives
        // through this node's paint and the arena-children recursion;
        // `theme` resolves for this node and flows to descendants.
        let _intl = cold
            .widget
            .intl_override()
            .map(|(d, l)| crate::intl::install_ambient_intl(d, l));
        let theme = cold.widget.theme_override().unwrap_or(inherited_theme);

        // Underflow enforcement — `Hide`/`Collapse` keep the layout
        // slot but paint nothing (Android `INVISIBLE` semantics; the
        // space-freeing half of `Collapse` is the layout engine's job).
        let policy = cold.underflow_policy();
        if matches!(
            policy,
            Some(UnderflowPolicy::Hide | UnderflowPolicy::Collapse)
        ) {
            return;
        }

        // Provenance scope — covers this widget's paint commands, its
        // internal children, AND its arena children, so the scope tree
        // mirrors the widget tree exactly. Backends ignore the marker;
        // the audit attributes findings to the innermost scope. The
        // instance-level `ColdNode::debug_name` wins over the widget
        // type's `debug_name` — apps naming nodes get their name.
        // `paint_extent` widens the provenance bounds for widgets with
        // designed overhang (badge-on-corner) so the audit doesn't flag
        // intended output as a container leak.
        list.push_scope(
            Some(id),
            cold.debug_name.unwrap_or_else(|| cold.widget.debug_name()),
            rect_to_kurbo(cold.widget.paint_extent().unwrap_or(hot.bounds)),
        );

        // `Fallback` replaces the whole subtree with the widget's
        // degraded chrome — nothing else paints.
        if matches!(policy, Some(UnderflowPolicy::Fallback)) {
            let mut cx = PaintContext {
                list,
                bounds: hot.bounds,
                theme,
                scale: self.scale_factor,
                text_painter: self.text_painter.as_deref(),
            };
            cold.widget.paint_underflow(&mut cx);
            list.pop_scope();
            return;
        }

        // Loading enforcement — the placeholder replaces the widget's
        // body AND suppresses both child domains (the internal-child
        // walk inside `paint_widget_body` and the arena-children loop
        // below). Underflow wins ordering: engaged Hide/Collapse never
        // reach this arm, and Fallback handles itself above.
        if self.effective_loading(id) {
            let mut cx = PaintContext {
                list,
                bounds: hot.bounds,
                theme,
                scale: self.scale_factor,
                text_painter: self.text_painter.as_deref(),
            };
            cold.widget.paint_loading(&mut cx, self.loading_phase());
            list.pop_scope();
            return;
        }

        // `Clip` wraps the widget's chrome AND its entire subtree —
        // stricter than `CLIPS_CHILDREN`, which clips children only.
        if matches!(policy, Some(UnderflowPolicy::Clip)) {
            list.push_clip(rect_to_kurbo(hot.bounds));
        }

        // The `Clip` underflow policy narrows the visible region for
        // the whole subtree the same way the clip pair does.
        let body_visible = if matches!(policy, Some(UnderflowPolicy::Clip)) {
            narrow_visible(visible, hot.bounds)
        } else {
            visible
        };
        paint_widget_body(
            &*cold.widget,
            hot.bounds,
            list,
            theme,
            self.scale_factor,
            self.text_painter.as_deref(),
            body_visible,
            self.loading_phase(),
        );

        // Arena children honour the node's `CLIPS_CHILDREN` flag: their
        // paint commands are wrapped in a clip for the node bounds —
        // following `Widget::clip_shape` when the widget declares a
        // silhouette. (`Widget::clips_children` governs *internal*
        // children inside `paint_widget_body`.)
        let clip_children = hot.flags.contains(NodeFlags::CLIPS_CHILDREN);
        if clip_children {
            let kb = rect_to_kurbo(hot.bounds);
            match cold.widget.clip_shape() {
                Some(shape) => list.push_clip_shape(kb, &shape),
                None => list.push_clip(kb),
            }
        }
        let child_visible = if clip_children {
            narrow_visible(body_visible, hot.bounds)
        } else {
            body_visible
        };
        let mut child = hot.first_child;
        while let Some(child_id) = child {
            self.paint_node(child_id, list, child_visible, theme);
            child = self.get_hot(child_id).and_then(|h| h.next_sibling);
        }
        if clip_children {
            list.pop_clip();
        }

        // `paint_overlay` emits last inside the scope — a focus ring or
        // selection outline stays visible over opaque arena children.
        // It sits inside the `Clip` policy clip and under the `Scrim`
        // veil, matching every other emit from this subtree.
        {
            let mut cx = PaintContext {
                list,
                bounds: hot.bounds,
                theme,
                scale: self.scale_factor,
                text_painter: self.text_painter.as_deref(),
            };
            cold.widget.paint_overlay(&mut cx);
        }

        if matches!(policy, Some(UnderflowPolicy::Clip)) {
            list.pop_clip();
        }
        // `Scrim` veils the painted subtree with a frosted overlay —
        // the bounds stay legible as *occupied*, the cramped content
        // underneath does not.
        if matches!(policy, Some(UnderflowPolicy::Scrim)) {
            let (rect, radius, color) = scrim_veil(hot.bounds, theme);
            list.push_blurred_rect(rect, radius, color);
        }
        list.pop_scope();
    }
}

/// The frosted-veil parameters for an engaged
/// [`UnderflowPolicy::Scrim`]: the widget's bounds, a fixed blur
/// radius, and a translucent tint derived from the theme's surface
/// color. This is a cover over the region — it does not blur the
/// widget's painted output.
fn scrim_veil(bounds: crate::Rect, theme: &martensite_theme::Theme) -> ([f32; 4], f32, [f32; 4]) {
    const SCRIM_BLUR_RADIUS: f32 = 10.0;
    const SCRIM_ALPHA: f32 = 0.55;
    let color = theme
        .color(martensite_theme::TokenKey::SurfaceColor)
        .map(|c| {
            let (r, g, b) = c.to_srgb();
            [r, g, b, c.alpha * SCRIM_ALPHA]
        })
        .unwrap_or([0.0, 0.0, 0.0, SCRIM_ALPHA]);
    (
        [
            bounds.min_x(),
            bounds.min_y(),
            bounds.width(),
            bounds.height(),
        ],
        SCRIM_BLUR_RADIUS,
        color,
    )
}

/// Does a rectangle enclose positive area? `false` for empty and NaN
/// rects — both mean "no known allocation" to the paint walk.
fn has_area(r: crate::Rect) -> bool {
    r.width() > 0.0 && r.height() > 0.0
}

/// Do two rectangles share any area? Used by the paint walk to cull
/// subtrees fully outside the active clip region.
fn rects_intersect(a: crate::Rect, b: crate::Rect) -> bool {
    a.min_x() < b.max_x() && b.min_x() < a.max_x() && a.min_y() < b.max_y() && b.min_y() < a.max_y()
}

/// May a node at `bounds` contribute paint inside `visible`?
///
/// The cull is an optimization for offscreen content, not a semantic
/// filter. `visible == None` means no region is known (unlaid-out
/// root) — nothing is culled. A widget with degenerate `bounds` was
/// never allocated; it may still emit commands (widgets can paint
/// outside their bounds — that is the defect class the paint audit
/// exists to catch), so it is kept. `Some(empty)` still culls: a clip
/// narrowed to nothing genuinely shows nothing.
fn paints_in(bounds: crate::Rect, visible: Option<crate::Rect>) -> bool {
    match visible {
        None => true,
        Some(v) => !has_area(bounds) || rects_intersect(bounds, v),
    }
}

/// Narrow the visible region by a clip rect. `None` (no known region)
/// becomes `Some(clip)` — the clip is the first real bound on
/// visibility.
fn narrow_visible(visible: Option<crate::Rect>, clip: crate::Rect) -> Option<crate::Rect> {
    Some(visible.map_or(clip, |v| rect_intersection(v, clip)))
}

/// The overlapping region of two rectangles (empty when disjoint).
fn rect_intersection(a: crate::Rect, b: crate::Rect) -> crate::Rect {
    let x0 = a.min_x().max(b.min_x());
    let y0 = a.min_y().max(b.min_y());
    let x1 = a.max_x().min(b.max_x());
    let y1 = a.max_y().min(b.max_y());
    crate::Rect::new(x0, y0, (x1 - x0).max(0.0), (y1 - y0).max(0.0))
}

/// Convert a [`crate::Rect`] to the `kurbo` rectangle paint commands use.
pub(crate) fn rect_to_kurbo(rect: crate::Rect) -> kurbo::Rect {
    kurbo::Rect::new(
        f64::from(rect.min_x()),
        f64::from(rect.min_y()),
        f64::from(rect.max_x()),
        f64::from(rect.max_y()),
    )
}

/// Paint a widget and its internal children recursively.
///
/// Emits the widget's own chrome via `paint`, then recurses into each
/// internal child using the child's layout-assigned bounds. Internal
/// children have no arena nodes, so this walk is driven entirely by the
/// `Widget::child_count`/`child`/`child_bounds` protocol. When
/// [`Widget::clips_children`](crate::Widget::clips_children) reports
/// `true` the recursion is wrapped in a
/// [`PaintCommand::ClipRect`](crate::PaintCommand::ClipRect) /
/// [`PaintCommand::PopClip`](crate::PaintCommand::PopClip) pair for
/// `bounds`.
///
/// `pub(crate)` so the [`OverlayLayer`](crate::overlay::OverlayLayer)
/// can paint popup content through the same walk. `phase` is the
/// shared shimmer phase handed to [`Widget::paint_loading`] — `None`
/// paints static placeholders.
#[allow(clippy::too_many_arguments)]
pub(crate) fn paint_widget_recursive(
    widget: &dyn crate::Widget,
    bounds: crate::Rect,
    list: &mut PaintList,
    theme: &martensite_theme::Theme,
    scale: f32,
    text_painter: Option<&(dyn crate::paint::TextShaper + Send + Sync)>,
    visible: Option<crate::Rect>,
    phase: Option<f32>,
) {
    // Scope with no arena handle — callers of this entry point (overlay
    // content) have no `WidgetId` to report. Internal children recurse
    // through this same function and get their own scopes.
    list.push_scope(
        None,
        widget.debug_name(),
        rect_to_kurbo(widget.paint_extent().unwrap_or(bounds)),
    );
    paint_widget_body(
        widget,
        bounds,
        list,
        theme,
        scale,
        text_painter,
        visible,
        phase,
    );
    list.pop_scope();
}

/// Paint a widget's chrome and internal children *without* a scope —
/// [`WidgetArena::paint_node`] manages the scope itself so it can also
/// cover arena children. [`Widget::clips_children`] wraps the internal
/// children in a clip pair, matching the arena-level `CLIPS_CHILDREN`
/// behaviour.
#[allow(clippy::too_many_arguments)]
fn paint_widget_body(
    widget: &dyn crate::Widget,
    bounds: crate::Rect,
    list: &mut PaintList,
    theme: &martensite_theme::Theme,
    scale: f32,
    text_painter: Option<&(dyn crate::paint::TextShaper + Send + Sync)>,
    visible: Option<crate::Rect>,
    phase: Option<f32>,
) {
    let mut cx = PaintContext {
        list,
        bounds,
        theme,
        scale,
        text_painter,
    };
    // Internal-node subtree overrides — same contract as
    // `WidgetArena::paint_node`: ambient direction/locale guard covers
    // this widget's paint and every descendant, `theme` resolves for
    // the subtree below.
    let _intl = widget
        .intl_override()
        .map(|(d, l)| crate::intl::install_ambient_intl(d, l));
    let theme = widget.theme_override().unwrap_or(theme);

    // A loading internal widget swaps its body AND descendants for the
    // placeholder — this consult also covers overlay popup content,
    // which reaches here through `paint_widget_recursive` and never
    // passes `WidgetArena::paint_node`.
    if widget.is_loading() {
        widget.paint_loading(&mut cx, phase);
        return;
    }
    widget.paint(&mut cx);
    let clip = widget.clips_children();
    if clip {
        // `clip_shape` lets a shaped widget clip children to its
        // silhouette (e.g. rounded corners) rather than the raw bounds.
        match widget.clip_shape() {
            Some(shape) => cx.list.push_clip_shape(rect_to_kurbo(bounds), &shape),
            None => cx.list.push_clip(rect_to_kurbo(bounds)),
        }
    }
    let child_visible = if clip {
        narrow_visible(visible, bounds)
    } else {
        visible
    };
    for i in 0..widget.child_count() {
        let (Some(child), Some(child_bounds)) = (widget.child(i), widget.child_bounds(i)) else {
            continue;
        };
        // A per-child clip (e.g. a ScrollView's content vs its
        // scrollbar strips) narrows both the virtualization test and
        // the emitted commands.
        let extra_clip = widget
            .child_clip(i)
            .map(|c| rect_intersection(child_bounds, c));
        let child_visible = match extra_clip {
            Some(c) => narrow_visible(child_visible, c),
            None => child_visible,
        };
        // Virtualization: a child fully outside the visible region
        // emits only backend-clipped dead commands — skip it.
        if !paints_in(child_bounds, child_visible) {
            continue;
        }
        if let Some(c) = extra_clip {
            cx.list.push_clip(rect_to_kurbo(c));
        }
        paint_underflowed_child(
            child,
            child_bounds,
            &mut *cx.list,
            theme,
            scale,
            text_painter,
            child_visible,
            phase,
        );
        if extra_clip.is_some() {
            cx.list.pop_clip();
        }
    }
    if clip {
        cx.list.pop_clip();
    }
}

/// Paints one internal child, applying its [`Widget::min_render`]
/// underflow policy when the child's bounds underflow the declared
/// minimum. Internal children have no `ColdNode`, so evaluation is
/// threshold-only — no hysteresis state and no per-instance override.
#[allow(clippy::too_many_arguments)]
fn paint_underflowed_child(
    child: &dyn Widget,
    bounds: crate::Rect,
    list: &mut PaintList,
    theme: &martensite_theme::Theme,
    scale: f32,
    text_painter: Option<&(dyn crate::paint::TextShaper + Send + Sync)>,
    visible: Option<crate::Rect>,
    phase: Option<f32>,
) {
    let min = child.min_render();
    let policy = if min.policy.enforces() && min.violated(bounds, scale) {
        Some(min.policy)
    } else {
        None
    };
    match policy {
        Some(UnderflowPolicy::Hide | UnderflowPolicy::Collapse) => {}
        Some(UnderflowPolicy::Fallback) => {
            list.push_scope(None, child.debug_name(), rect_to_kurbo(bounds));
            let mut cx = PaintContext {
                list,
                bounds,
                theme,
                scale,
                text_painter,
            };
            child.paint_underflow(&mut cx);
            list.pop_scope();
        }
        Some(UnderflowPolicy::Clip) => {
            list.push_clip(rect_to_kurbo(bounds));
            paint_widget_recursive(
                child,
                bounds,
                list,
                theme,
                scale,
                text_painter,
                narrow_visible(visible, bounds),
                phase,
            );
            list.pop_clip();
        }
        Some(UnderflowPolicy::Scrim) => {
            paint_widget_recursive(
                child,
                bounds,
                list,
                theme,
                scale,
                text_painter,
                visible,
                phase,
            );
            let (rect, radius, color) = scrim_veil(bounds, theme);
            list.push_blurred_rect(rect, radius, color);
        }
        _ => paint_widget_recursive(
            child,
            bounds,
            list,
            theme,
            scale,
            text_painter,
            visible,
            phase,
        ),
    }
}

// --- Iterator Implementations ---

/// Double-ended iterator over the direct children of a widget node.
///
/// # Examples
///
/// ```
/// use martensite_core::{DummyWidget, HotNode, WidgetArena};
///
/// let mut arena = WidgetArena::new();
/// let parent = arena.insert_with_widget(HotNode::default(), Box::new(DummyWidget));
/// let a = arena.insert_with_widget(HotNode::default(), Box::new(DummyWidget));
/// let b = arena.insert_with_widget(HotNode::default(), Box::new(DummyWidget));
/// arena.append_child(parent, a).unwrap();
/// arena.append_child(parent, b).unwrap();
///
/// let children = arena.children(parent);
/// assert_eq!(children.count(), 2);
/// ```
#[derive(Clone, Debug)]
pub struct Children<'a> {
    arena: &'a WidgetArena,
    front: Option<WidgetId>,
    back: Option<WidgetId>,
}

impl<'a> Iterator for Children<'a> {
    type Item = WidgetId;

    fn next(&mut self) -> Option<Self::Item> {
        let curr = self.front?;
        if self.front == self.back {
            self.front = None;
            self.back = None;
        } else {
            self.front = self.arena.next_sibling(curr);
        }
        Some(curr)
    }
}

impl<'a> DoubleEndedIterator for Children<'a> {
    fn next_back(&mut self) -> Option<Self::Item> {
        let curr = self.back?;
        if self.front == self.back {
            self.front = None;
            self.back = None;
        } else {
            self.back = self.arena.prev_sibling(curr);
        }
        Some(curr)
    }
}

impl<'a> FusedIterator for Children<'a> {}

/// Zero-allocation pre-order depth-first traversal iterator over a node subtree.
///
/// # Examples
///
/// ```
/// use martensite_core::{DummyWidget, HotNode, WidgetArena};
///
/// let mut arena = WidgetArena::new();
/// let root = arena.insert_with_widget(HotNode::default(), Box::new(DummyWidget));
/// let child = arena.insert_with_widget(HotNode::default(), Box::new(DummyWidget));
/// arena.append_child(root, child).unwrap();
///
/// let mut it = arena.iter_subtree(root);
/// assert_eq!(it.next(), Some(root));
/// assert_eq!(it.next(), Some(child));
/// assert_eq!(it.next(), None);
/// ```
#[derive(Clone, Debug)]
pub struct SubtreeIter<'a> {
    arena: &'a WidgetArena,
    root: WidgetId,
    current: Option<WidgetId>,
    started: bool,
}

impl<'a> Iterator for SubtreeIter<'a> {
    type Item = WidgetId;

    fn next(&mut self) -> Option<Self::Item> {
        if !self.started {
            self.started = true;
            if self.arena.is_alive(self.root) {
                self.current = Some(self.root);
                return Some(self.root);
            } else {
                return None;
            }
        }

        let curr = self.current?;

        // 1. Visit first child if available
        if let Some(child) = self.arena.first_child(curr) {
            self.current = Some(child);
            return Some(child);
        }

        // 2. Walk up parent chain looking for an ancestor's next sibling
        let mut node = curr;
        loop {
            if node == self.root {
                self.current = None;
                return None;
            }
            if let Some(sibling) = self.arena.next_sibling(node) {
                self.current = Some(sibling);
                return Some(sibling);
            }
            match self.arena.parent(node) {
                Some(parent) => node = parent,
                None => {
                    self.current = None;
                    return None;
                }
            }
        }
    }
}

impl<'a> FusedIterator for SubtreeIter<'a> {}

/// Zero-allocation depth-first iterator traversing all trees in the arena.
///
/// # Examples
///
/// ```
/// use martensite_core::{DummyWidget, HotNode, WidgetArena};
///
/// let mut arena = WidgetArena::new();
/// let root = arena.insert_with_widget(HotNode::default(), Box::new(DummyWidget));
/// let child = arena.insert_with_widget(HotNode::default(), Box::new(DummyWidget));
/// arena.append_child(root, child).unwrap();
///
/// // Visits every tree in the arena in depth-first pre-order.
/// assert_eq!(arena.iter_depth_first().collect::<Vec<_>>(), vec![root, child]);
/// ```
#[derive(Clone, Debug)]
pub struct DepthFirstIter<'a> {
    arena: &'a WidgetArena,
    root_dense_idx: usize,
    current_subtree: Option<SubtreeIter<'a>>,
}

impl<'a> Iterator for DepthFirstIter<'a> {
    type Item = WidgetId;

    fn next(&mut self) -> Option<Self::Item> {
        loop {
            if let Some(ref mut subtree) = self.current_subtree {
                if let Some(id) = subtree.next() {
                    return Some(id);
                }
            }

            if self.root_dense_idx >= self.arena.hot_nodes.len() {
                self.current_subtree = None;
                return None;
            }

            let dense_idx = self.root_dense_idx;
            self.root_dense_idx += 1;

            let slot_idx = self.arena.dense_to_slot[dense_idx];
            let slot = self.arena.slots[slot_idx as usize];
            // Generation is never zero for an active slot (see arena docs).
            let id = WidgetId::from_parts(slot_idx, slot.generation);
            let hot = &self.arena.hot_nodes[dense_idx];

            if hot.parent.is_none() && hot.prev_sibling.is_none() {
                self.current_subtree = Some(self.arena.iter_subtree(id));
            }
        }
    }
}

impl<'a> FusedIterator for DepthFirstIter<'a> {}

/// Breadth-first iterator traversing trees level-by-level using a FIFO queue.
///
/// # Examples
///
/// ```
/// use martensite_core::{DummyWidget, HotNode, WidgetArena};
///
/// let mut arena = WidgetArena::new();
/// let root = arena.insert_with_widget(HotNode::default(), Box::new(DummyWidget));
/// let a = arena.insert_with_widget(HotNode::default(), Box::new(DummyWidget));
/// arena.append_child(root, a).unwrap();
///
/// // `iter_subtree_breadth_first` walks a single subtree level-by-level.
/// assert_eq!(arena.iter_subtree_breadth_first(root).collect::<Vec<_>>(), vec![root, a]);
/// ```
#[derive(Clone, Debug)]
pub struct BreadthFirstIter<'a> {
    arena: &'a WidgetArena,
    queue: VecDeque<WidgetId>,
}

impl<'a> Iterator for BreadthFirstIter<'a> {
    type Item = WidgetId;

    fn next(&mut self) -> Option<Self::Item> {
        let id = self.queue.pop_front()?;
        let mut child = self.arena.first_child(id);
        while let Some(c) = child {
            self.queue.push_back(c);
            child = self.arena.next_sibling(c);
        }
        Some(id)
    }
}

impl<'a> FusedIterator for BreadthFirstIter<'a> {}

#[cfg(test)]
mod scope_tests {
    use crate::{
        DummyWidget, HotNode, LayoutConstraints, LayoutContext, NodeFlags, PaintCommand, PaintList,
        Rect, Widget, WidgetArena,
    };
    use glam::Vec2;

    fn visible(flags: NodeFlags) -> HotNode {
        let mut hot = HotNode::default();
        hot.flags |= NodeFlags::VISIBLE | flags;
        hot
    }

    /// Widget with one internal child — exercises the scope nesting of
    /// widget-internal children (no arena nodes of their own).
    struct ParentWithChild {
        child: DummyWidget,
    }

    impl Widget for ParentWithChild {
        fn debug_name(&self) -> &'static str {
            "Parent"
        }
        fn measure(&mut self, _cx: &mut LayoutContext, _c: LayoutConstraints) -> Vec2 {
            Vec2::ZERO
        }
        fn layout(&mut self, _cx: &mut LayoutContext, _bounds: Rect) {}
        fn child_count(&self) -> usize {
            1
        }
        fn child(&self, i: usize) -> Option<&dyn Widget> {
            (i == 0).then_some(&self.child)
        }
        fn child_bounds(&self, i: usize) -> Option<Rect> {
            (i == 0).then_some(Rect::new(0.0, 0.0, 10.0, 10.0))
        }
    }

    #[test]
    fn paint_list_emits_balanced_scopes() {
        let mut arena = WidgetArena::new();
        let root = arena.insert_with_widget(visible(NodeFlags::empty()), Box::new(DummyWidget));
        let child = arena.insert_with_widget(visible(NodeFlags::empty()), Box::new(DummyWidget));
        arena.append_child(root, child).unwrap();

        let mut list = PaintList::new();
        arena.build_paint_list(root, &mut list);

        let mut depth = 0i32;
        let mut names = Vec::new();
        let mut ids = Vec::new();
        for cmd in &list.commands {
            match cmd {
                PaintCommand::PushScope { id, name, .. } => {
                    depth += 1;
                    names.push(*name);
                    ids.push(*id);
                }
                PaintCommand::PopScope => depth -= 1,
                _ => {}
            }
        }
        assert_eq!(depth, 0, "scopes must balance");
        assert_eq!(names.len(), 2, "root + arena child each open a scope");
        assert!(names.iter().all(|n| n.contains("DummyWidget")));
        assert_eq!(ids, vec![Some(root), Some(child)]);
        // Child scope nests inside the parent's — the scope tree mirrors
        // the widget tree. The second PushScope must precede the first
        // PopScope (child opens before parent closes).
        let first_pop = list
            .commands
            .iter()
            .position(|c| matches!(c, PaintCommand::PopScope))
            .unwrap();
        let second_push = list
            .commands
            .iter()
            .enumerate()
            .filter(|(_, c)| matches!(c, PaintCommand::PushScope { .. }))
            .nth(1)
            .map(|(i, _)| i)
            .unwrap();
        assert!(second_push < first_pop, "child scope must nest");
    }

    #[test]
    fn cold_node_debug_name_wins_over_type_name() {
        let mut arena = WidgetArena::new();
        let mut hot = visible(NodeFlags::empty());
        hot.bounds = Rect::new(0.0, 0.0, 10.0, 10.0);
        let id = arena.insert_with_widget(hot, Box::new(DummyWidget));
        if let Some(cold) = arena.get_cold_mut(id) {
            cold.debug_name = Some("Process Grid");
        }

        let mut list = PaintList::new();
        arena.build_paint_list(id, &mut list);
        let name = list.commands.iter().find_map(|c| match c {
            PaintCommand::PushScope { name, .. } => Some(*name),
            _ => None,
        });
        assert_eq!(name, Some("Process Grid"));
    }

    /// Widget that clips its single internal child — exercises the
    /// virtualization cull for widget-internal children.
    struct ClippingParent {
        child: DummyWidget,
        child_bounds: Rect,
    }

    impl Widget for ClippingParent {
        fn debug_name(&self) -> &'static str {
            "ClippingParent"
        }
        fn measure(&mut self, _cx: &mut LayoutContext, _c: LayoutConstraints) -> Vec2 {
            Vec2::ZERO
        }
        fn layout(&mut self, _cx: &mut LayoutContext, _bounds: Rect) {}
        fn clips_children(&self) -> bool {
            true
        }
        fn child_count(&self) -> usize {
            1
        }
        fn child(&self, i: usize) -> Option<&dyn Widget> {
            (i == 0).then_some(&self.child)
        }
        fn child_bounds(&self, i: usize) -> Option<Rect> {
            (i == 0).then_some(self.child_bounds)
        }
    }

    fn scope_names(list: &PaintList) -> Vec<&'static str> {
        list.commands
            .iter()
            .filter_map(|c| match c {
                PaintCommand::PushScope { name, .. } => Some(*name),
                _ => None,
            })
            .collect()
    }

    #[test]
    fn paint_culls_arena_child_outside_root_bounds() {
        let mut arena = WidgetArena::new();
        let mut root_hot = visible(NodeFlags::empty());
        root_hot.bounds = Rect::new(0.0, 0.0, 100.0, 100.0);
        let root = arena.insert_with_widget(root_hot, Box::new(DummyWidget));
        let mut child_hot = visible(NodeFlags::empty());
        child_hot.bounds = Rect::new(500.0, 500.0, 10.0, 10.0);
        let child = arena.insert_with_widget(child_hot, Box::new(DummyWidget));
        arena.append_child(root, child).unwrap();

        let mut list = PaintList::new();
        arena.build_paint_list(root, &mut list);
        // The offscreen child emits only backend-clipped dead commands —
        // its whole subtree, scope included, is skipped.
        assert_eq!(scope_names(&list).len(), 1);
    }

    #[test]
    fn paint_culls_internal_child_outside_clip() {
        let mut arena = WidgetArena::new();
        let mut root_hot = visible(NodeFlags::empty());
        root_hot.bounds = Rect::new(0.0, 0.0, 100.0, 100.0);
        let root = arena.insert_with_widget(
            root_hot,
            Box::new(ClippingParent {
                child: DummyWidget,
                // Fully outside the parent's 100x100 clip.
                child_bounds: Rect::new(500.0, 500.0, 10.0, 10.0),
            }),
        );

        let mut list = PaintList::new();
        arena.build_paint_list(root, &mut list);
        assert_eq!(scope_names(&list), vec!["ClippingParent"]);
    }

    #[test]
    fn paint_keeps_partially_visible_internal_child() {
        let mut arena = WidgetArena::new();
        let mut root_hot = visible(NodeFlags::empty());
        root_hot.bounds = Rect::new(0.0, 0.0, 100.0, 100.0);
        let root = arena.insert_with_widget(
            root_hot,
            Box::new(ClippingParent {
                child: DummyWidget,
                // Straddles the clip edge — the onscreen half still paints.
                child_bounds: Rect::new(90.0, 90.0, 50.0, 50.0),
            }),
        );

        let mut list = PaintList::new();
        arena.build_paint_list(root, &mut list);
        assert_eq!(scope_names(&list).len(), 2);
    }

    /// A widget counting its own ticks — proves `tick_recursive`
    /// skips children with no allocated bounds (the "not presented"
    /// signal hidden tabs and suspended controls rely on).
    struct TickCounter(std::sync::Arc<std::sync::atomic::AtomicUsize>);

    impl Widget for TickCounter {
        fn debug_name(&self) -> &'static str {
            "TickCounter"
        }
        fn measure(&mut self, _cx: &mut LayoutContext, _c: LayoutConstraints) -> Vec2 {
            Vec2::ZERO
        }
        fn layout(&mut self, _cx: &mut LayoutContext, _bounds: Rect) {}
        fn tick(&mut self, _dt: std::time::Duration) -> bool {
            self.0.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
            false
        }
    }

    /// Parent whose single child is presented only when `shown` —
    /// mirrors `TabPanelChild`'s `child_bounds → None` gate. The flag
    /// is a shared atomic so the test can flip it without downcasting
    /// (`as_any_mut` is feature-gated).
    struct ShownGatedParent {
        child: TickCounter,
        shown: std::sync::Arc<std::sync::atomic::AtomicBool>,
    }

    impl Widget for ShownGatedParent {
        fn debug_name(&self) -> &'static str {
            "ShownGatedParent"
        }
        fn measure(&mut self, _cx: &mut LayoutContext, _c: LayoutConstraints) -> Vec2 {
            Vec2::ZERO
        }
        fn layout(&mut self, _cx: &mut LayoutContext, _bounds: Rect) {}
        fn child_count(&self) -> usize {
            1
        }
        fn child(&self, i: usize) -> Option<&dyn Widget> {
            (i == 0).then_some(&self.child)
        }
        fn child_mut(&mut self, i: usize) -> Option<&mut dyn Widget> {
            (i == 0).then_some(&mut self.child)
        }
        fn child_bounds(&self, i: usize) -> Option<Rect> {
            (i == 0 && self.shown.load(std::sync::atomic::Ordering::Relaxed))
                .then_some(Rect::new(0.0, 0.0, 10.0, 10.0))
        }
    }

    #[test]
    fn tick_skips_unallocated_internal_child() {
        use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
        use std::sync::Arc;
        use std::time::Duration;

        let ticks = Arc::new(AtomicUsize::new(0));
        let shown = Arc::new(AtomicBool::new(false));
        let mut arena = WidgetArena::new();
        let _root = arena.insert_with_widget(
            visible(NodeFlags::empty()),
            Box::new(ShownGatedParent {
                child: TickCounter(ticks.clone()),
                shown: shown.clone(),
            }),
        );

        arena.tick(Duration::from_millis(16));
        arena.tick(Duration::from_millis(16));
        assert_eq!(
            ticks.load(Ordering::Relaxed),
            0,
            "hidden child must not tick"
        );

        // Re-show the child: the same walk ticks it again.
        shown.store(true, Ordering::Relaxed);
        arena.tick(Duration::from_millis(16));
        assert_eq!(
            ticks.load(Ordering::Relaxed),
            1,
            "shown child resumes ticking"
        );
    }

    #[test]
    fn internal_children_get_own_scopes() {
        let mut arena = WidgetArena::new();
        let root = arena.insert_with_widget(
            visible(NodeFlags::empty()),
            Box::new(ParentWithChild { child: DummyWidget }),
        );

        let mut list = PaintList::new();
        arena.build_paint_list(root, &mut list);

        let scopes: Vec<_> = list
            .commands
            .iter()
            .filter_map(|c| match c {
                PaintCommand::PushScope { id, name, .. } => Some((*id, *name)),
                _ => None,
            })
            .collect();
        assert_eq!(scopes.len(), 2);
        assert_eq!(scopes[0], (Some(root), "Parent"));
        // Internal child: no arena handle, but still named.
        assert_eq!(scopes[1].0, None);
        assert!(scopes[1].1.contains("DummyWidget"));
    }

    #[test]
    fn intl_and_theme_overrides_scope_to_subtree() {
        use crate::intl::install_ambient_intl;
        use crate::LayoutDirection;

        struct Probe;
        impl crate::Widget for Probe {
            fn measure(
                &mut self,
                _cx: &mut crate::LayoutContext,
                _c: crate::LayoutConstraints,
            ) -> glam::Vec2 {
                glam::Vec2::new(10.0, 10.0)
            }
            fn layout(&mut self, _cx: &mut crate::LayoutContext, _b: crate::Rect) {}
            fn paint(&self, cx: &mut crate::PaintContext) {
                // Probe records the ambient direction and a marker
                // token's presence so the parent test can assert which
                // theme/direction this subtree painted under.
                let dir = cx.direction();
                let has_marker = cx
                    .theme
                    .color(martensite_theme::TokenKey::AccentColor)
                    .is_some();
                cx.list.push_fill_rect(
                    kurbo::Rect::new(0.0, 0.0, 1.0, 1.0),
                    if dir.is_rtl() && has_marker {
                        [255, 0, 0, 255]
                    } else {
                        [0, 0, 255, 255]
                    },
                );
            }
        }

        struct Stage {
            child: crate::widget::DummyWidget,
            probe: Probe,
            theme: martensite_theme::Theme,
        }
        impl crate::Widget for Stage {
            fn measure(
                &mut self,
                _cx: &mut crate::LayoutContext,
                _c: crate::LayoutConstraints,
            ) -> glam::Vec2 {
                glam::Vec2::new(10.0, 10.0)
            }
            fn layout(&mut self, cx: &mut crate::LayoutContext, b: crate::Rect) {
                // Stage scopes layout for its children by installing the
                // ambient values around layout_child — see
                // `Widget::intl_override`.
                let _g = install_ambient_intl(LayoutDirection::Rtl, crate::Locale::new("ar"));
                cx.layout_child(&mut self.probe, b);
                cx.layout_child(&mut self.child, b);
            }
            fn intl_override(&self) -> Option<(LayoutDirection, crate::Locale)> {
                Some((LayoutDirection::Rtl, crate::Locale::new("ar")))
            }
            fn theme_override(&self) -> Option<&martensite_theme::Theme> {
                Some(&self.theme)
            }
            fn child_count(&self) -> usize {
                2
            }
            fn child(&self, i: usize) -> Option<&dyn crate::Widget> {
                match i {
                    0 => Some(&self.probe),
                    _ => Some(&self.child),
                }
            }
            fn child_bounds(&self, _i: usize) -> Option<crate::Rect> {
                Some(crate::Rect::new(0.0, 0.0, 10.0, 10.0))
            }
        }

        let mut arena = WidgetArena::new();
        let stage = arena.insert(
            HotNode::new(taffy::NodeId::new(1)),
            crate::ColdNode::new(Box::new(Stage {
                child: crate::widget::DummyWidget,
                probe: Probe,
                theme: martensite_theme::tokens::default_dark(),
            })),
        );
        {
            let hot = arena.get_hot_mut(stage).unwrap();
            hot.bounds = crate::Rect::new(0.0, 0.0, 10.0, 10.0);
            hot.flags |= NodeFlags::VISIBLE;
        }

        let mut list = crate::PaintList::new();
        arena.build_paint_list(stage, &mut list);
        // The internal Probe painted under the stage's Rtl + dark-theme
        // override (red fill); a sibling probe outside the stage would
        // see Ltr + the arena theme.
        let fills: Vec<_> = list
            .commands
            .iter()
            .filter_map(|c| match c {
                crate::PaintCommand::FillRect(_, color) => Some(*color),
                _ => None,
            })
            .collect();
        assert!(
            fills.contains(&[255, 0, 0, 255]),
            "probe fill: {fills:?} all={:?}",
            list.commands
        );
    }
}
