//! In-window overlay (popup) layer.
//!
//! The [`OverlayLayer`] owns popups that paint above normal content —
//! dropdown listboxes, tooltips, and similar transient surfaces —
//! without opening a separate OS window. Entries are kept in z-order
//! (last opened paints topmost), are clamped into the viewport at
//! layout time, and implement the standard dismissal contract:
//!
//! - a [`WidgetEvent::PointerPressed`] outside every open popup
//!   dismisses all popups and falls through to the content beneath;
//! - `Escape` dismisses the topmost popup.
//!
//! Entries opened via [`OverlayLayer::open_with`] with
//! [`OverlayOptions::modal`] change that contract: a scrim covers the
//! viewport beneath the topmost modal entry, positional events outside
//! it are consumed rather than forwarded, and popups below it are
//! unreachable until it closes — the standard modal-dialog and
//! modal-drawer behavior.
//!
//! Pointer and scroll events are dispatched to popups topmost-first
//! before the caller forwards them to the arena, so popup content
//! hit-tests ahead of in-window content. While an entry's content
//! answers [`EventResponse::CapturePointer`], subsequent pointer
//! events are routed to that entry until it answers
//! [`EventResponse::ReleasePointer`]. Keyboard input is *not* routed
//! into popups (other than `Escape` dismissal): focus stays on the
//! popup's owning widget, so e.g. combobox typeahead works while its
//! listbox is open.
//!
//! The layer is owned by [`WidgetArena`](crate::WidgetArena)
//! ([`overlay`](crate::WidgetArena::overlay) /
//! [`overlay_mut`](crate::WidgetArena::overlay_mut)) in production —
//! `martensite-window`'s `EventRouter` offers it events before arena
//! routing, [`WidgetArena::build_paint_list`](crate::WidgetArena::build_paint_list)
//! appends it after arena content, and `martensite-access`'s
//! `AccessKitAdapter` emits open popups as top-level virtual nodes.
//! The layer can also be used standalone; widget owners reconcile
//! with it once per frame via [`Widget::sync_overlay`](crate::Widget::sync_overlay),
//! driven by [`WidgetArena::sync_overlays`](crate::WidgetArena::sync_overlays).
//!
//! # Examples
//!
//! ```
//! use martensite_core::overlay::{OverlayAnchor, OverlayLayer};
//! use martensite_core::{DummyWidget, Rect};
//!
//! let mut overlay = OverlayLayer::new();
//! overlay.set_viewport(Rect::new(0.0, 0.0, 800.0, 600.0));
//! let id = overlay.open(
//!     Box::new(DummyWidget),
//!     OverlayAnchor::Bounds(Rect::new(10.0, 10.0, 100.0, 30.0)),
//! );
//! assert!(overlay.is_open(id));
//! assert_eq!(overlay.len(), 1);
//! ```

use std::collections::VecDeque;

use glam::Vec2;

use crate::arena::paint_widget_recursive;
use crate::id::WidgetId;
use crate::node::{HotNode, Rect};
use crate::paint::PaintList;
use crate::widget::{
    EventContext, EventResponse, LayoutConstraints, LayoutContext, Widget, WidgetEvent,
};

/// Gap in logical pixels between an anchor edge and a placed popup.
const ANCHOR_GAP: f32 = 4.0;
/// Offset in logical pixels between the pointer and a pointer-anchored
/// popup (tooltips), placed below-right like native platforms.
const POINTER_OFFSET: f32 = 12.0;

/// How an overlay popup is positioned relative to the window.
///
/// # Examples
///
/// ```
/// use glam::Vec2;
/// use martensite_core::overlay::OverlayAnchor;
/// use martensite_core::Rect;
///
/// // A dropdown popup hangs off its combobox's bounds…
/// let anchor = OverlayAnchor::Bounds(Rect::new(0.0, 0.0, 120.0, 32.0));
/// // …a tooltip floats near the pointer.
/// let tip = OverlayAnchor::Pointer(Vec2::new(40.0, 50.0));
/// assert!(matches!(anchor, OverlayAnchor::Bounds(_)));
/// assert!(matches!(tip, OverlayAnchor::Pointer(_)));
/// ```
#[derive(Clone, Debug, PartialEq)]
pub enum OverlayAnchor {
    /// Popup anchored to a widget's window-space bounds — placed below
    /// the anchor when it fits, flipped above when it does not, and
    /// finally clamped into the viewport.
    Bounds(Rect),
    /// Popup anchored to a pointer position, offset below-right and
    /// flipped/clamped into the viewport.
    Pointer(Vec2),
    /// Centered in the viewport — dialogs, alert boxes, command
    /// palettes.
    Center,
    /// Spans the full viewport height pinned to the left edge; the
    /// measured width sets the drawer's depth. Modal with a scrim
    /// when opened via [`OverlayOptions::modal`].
    EdgeLeft,
    /// Spans the full viewport height pinned to the right edge —
    /// inspector drawers, detail panels.
    EdgeRight,
    /// Spans the full viewport width pinned to the top edge.
    EdgeTop,
    /// Spans the full viewport width pinned to the bottom edge —
    /// bottom sheets.
    EdgeBottom,
    /// Pinned to a viewport region with the measured size and an
    /// edge margin (logical px) — toast stacks bottom-right,
    /// notification centers top-right.
    Viewport {
        /// Horizontal placement within the viewport.
        h: ViewportAlign,
        /// Vertical placement within the viewport.
        v: ViewportAlign,
        /// Logical-pixel margin from the anchored edges (and from the
        /// viewport center lines for `Center`).
        margin: f32,
    },
}

/// Horizontal or vertical placement of a [`OverlayAnchor::Viewport`]
/// popup within the viewport.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ViewportAlign {
    /// Flush to the start edge (left/top) plus the margin.
    Start,
    /// Centered on the axis.
    Center,
    /// Flush to the end edge (right/bottom) minus the margin.
    End,
}

/// Behavioral options for an overlay entry, passed to
/// [`OverlayLayer::open_with`]. The default is the historical popup
/// behavior: non-modal, light-dismissed by outside presses.
///
/// # Examples
///
/// ```
/// use martensite_core::overlay::OverlayOptions;
///
/// // A modal dialog: scrim, input blocked, scrim-click does NOT
/// // dismiss — an explicit button is required.
/// let modal = OverlayOptions::modal();
/// assert!(modal.modal && modal.scrim && !modal.scrim_dismiss);
/// // A modal drawer that closes when the scrim is tapped.
/// let drawer = OverlayOptions::modal().light_dismiss();
/// assert!(drawer.scrim_dismiss);
/// ```
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct OverlayOptions {
    /// `true` marks the entry modal: a scrim covers the viewport
    /// beneath it, positional events outside the popup are consumed
    /// (never reach content), and popups stacked *below* the topmost
    /// modal are unreachable until it closes.
    pub modal: bool,
    /// `true` paints a [`martensite_theme::TokenKey::ScrimColor`] fill
    /// over the viewport before this entry. Implied by `modal`.
    pub scrim: bool,
    /// `true` lets a press on the scrim dismiss this modal entry —
    /// the drawer/bottom-sheet convention (dialogs leave it `false`
    /// so an accidental click can't discard a form).
    pub scrim_dismiss: bool,
    /// `true` marks the entry transparent to input: an event its
    /// content ignores falls through to lower popups and window
    /// content instead of being swallowed, the blanket outside-press
    /// dismissal skips it, and Escape never targets it. The toast-stack
    /// contract — a strip of self-expiring cards that must not eat
    /// clicks landing between them.
    pub passthrough: bool,
}

impl OverlayOptions {
    /// Options for a modal surface: scrim painted, input blocked,
    /// scrim clicks consumed but not dismissing.
    pub fn modal() -> Self {
        Self {
            modal: true,
            scrim: true,
            scrim_dismiss: false,
            passthrough: false,
        }
    }

    /// Options for a non-interactive notification layer: no scrim, no
    /// modality, events its content ignores fall through to content,
    /// and outside presses never dismiss it.
    pub fn passthrough() -> Self {
        Self {
            passthrough: true,
            ..Self::default()
        }
    }

    /// Sets `scrim_dismiss` — builder-style.
    pub fn light_dismiss(mut self) -> Self {
        self.scrim_dismiss = true;
        self
    }
}

/// A single popup managed by an [`OverlayLayer`].
///
/// Obtain entries through [`OverlayLayer::entries`] or look them up by
/// id via [`OverlayLayer::entry`]; fields are exposed read-only through
/// the accessor methods.
pub struct OverlayEntry {
    /// Stable id returned by [`OverlayLayer::open`].
    id: u64,
    /// Placement anchor captured at `open` time.
    anchor: OverlayAnchor,
    /// Resolved window-space rect after the last layout pass.
    resolved: Rect,
    /// Whether the entry still needs measure/placement/layout.
    needs_layout: bool,
    /// The popup's widget tree, painted above window content.
    content: Box<dyn Widget>,
    /// The arena widget that opened this popup, stamped by
    /// [`OverlayLayer::open`] while `WidgetArena::sync_overlays` is
    /// driving the owner's [`Widget::sync_overlay`]. `None` for popups
    /// opened directly on the layer (outside a widget sync). Lets the
    /// arena dirty-mark owners when their popups close or change, and
    /// close orphaned popups when an owner is removed.
    owner: Option<WidgetId>,
    /// Modal/scrim behavior from [`OverlayOptions`] at `open` time.
    options: OverlayOptions,
}

impl OverlayEntry {
    /// The entry's stable id.
    pub fn id(&self) -> u64 {
        self.id
    }

    /// The anchor this entry was opened with.
    pub fn anchor(&self) -> &OverlayAnchor {
        &self.anchor
    }

    /// The entry's resolved window-space bounds (valid after
    /// [`OverlayLayer::layout_pass`]).
    pub fn bounds(&self) -> Rect {
        self.resolved
    }

    /// Borrow the popup's content widget.
    pub fn content(&self) -> &dyn Widget {
        &*self.content
    }

    /// Mutably borrow the popup's content widget.
    pub fn content_mut(&mut self) -> &mut dyn Widget {
        &mut *self.content
    }

    /// The arena widget that opened this popup, if it was opened while
    /// [`WidgetArena::sync_overlays`](crate::WidgetArena::sync_overlays)
    /// was driving that widget's [`Widget::sync_overlay`].
    pub fn owner(&self) -> Option<WidgetId> {
        self.owner
    }

    /// The [`OverlayOptions`] this entry was opened with.
    pub fn options(&self) -> OverlayOptions {
        self.options
    }
}

/// Z-ordered collection of in-window popup surfaces.
///
/// See the [module documentation](self) for the integration contract.
pub struct OverlayLayer {
    /// Entries in bottom→top z-order; `entries.last()` is topmost.
    entries: Vec<OverlayEntry>,
    /// Next overlay id to hand out (monotonic, never reused).
    next_id: u64,
    /// Window viewport in the arena's device-pixel space, used for
    /// clamping — callers pass the window's physical size.
    viewport: Rect,
    /// FIFO of entry ids dismissed by [`Self::dispatch_event`]
    /// (outside press or Escape), for owners that poll for closure.
    dismissed: VecDeque<u64>,
    /// Owners of entries that were closed (any path) or received an
    /// event since the last drain — drained by
    /// `WidgetArena::sync_overlays` to dirty-mark the owning widgets so
    /// their emitted a11y state tracks the popup.
    dirty_owners: VecDeque<Option<WidgetId>>,
    /// `true` while popup contents may have changed since the last
    /// accessibility emission — set on event delivery and close/open so
    /// incremental updates re-emit open popups. Drained by the
    /// `martensite-access` adapter via [`Self::take_content_dirty`].
    content_dirty: bool,
    /// The widget whose [`Widget::sync_overlay`] is currently running
    /// under `WidgetArena::sync_overlays`; stamped onto entries opened
    /// during that call.
    current_owner: Option<WidgetId>,
    /// Entry currently holding overlay-level pointer capture.
    capture: Option<u64>,
    /// Physical px per logical pt — forwarded into
    /// [`LayoutContext::scale`] during [`Self::layout_pass`]. Mirrors
    /// [`WidgetArena::scale_factor`]; `1.0` until set.
    scale_factor: f32,
}

impl Default for OverlayLayer {
    fn default() -> Self {
        Self::new()
    }
}

impl OverlayLayer {
    /// Creates an empty layer with a zero-size viewport; call
    /// [`Self::set_viewport`] before the first layout pass.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite_core::overlay::OverlayLayer;
    ///
    /// let layer = OverlayLayer::new();
    /// assert!(layer.is_empty());
    /// ```
    pub fn new() -> Self {
        Self {
            entries: Vec::new(),
            next_id: 1,
            viewport: Rect::default(),
            dismissed: VecDeque::new(),
            dirty_owners: VecDeque::new(),
            content_dirty: false,
            current_owner: None,
            capture: None,
            scale_factor: 1.0,
        }
    }

    /// The viewport popups are clamped to.
    pub fn viewport(&self) -> Rect {
        self.viewport
    }

    /// Sets the viewport popups are clamped to and re-marks every open
    /// entry for layout so it re-clamps on the next
    /// [`Self::layout_pass`]. Pass the window's physical size — the
    /// layer operates in the arena's device-pixel space.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite_core::overlay::OverlayLayer;
    /// use martensite_core::Rect;
    ///
    /// let mut layer = OverlayLayer::new();
    /// layer.set_viewport(Rect::new(0.0, 0.0, 800.0, 600.0));
    /// assert_eq!(layer.viewport().width(), 800.0);
    /// ```
    pub fn set_viewport(&mut self, viewport: Rect) {
        self.viewport = viewport;
        self.remark_all();
    }

    /// The display scale factor forwarded into
    /// [`LayoutContext::scale`] during [`Self::layout_pass`].
    ///
    /// `1.0` until [`Self::set_scale_factor`] reports the real factor.
    /// `WidgetArena` propagates its own factor automatically.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite_core::overlay::OverlayLayer;
    ///
    /// let mut layer = OverlayLayer::new();
    /// assert_eq!(layer.scale_factor(), 1.0);
    /// layer.set_scale_factor(2.0);
    /// assert_eq!(layer.scale_factor(), 2.0);
    /// ```
    pub fn scale_factor(&self) -> f32 {
        self.scale_factor
    }

    /// Reports the display's scale factor so popup content can scale
    /// its baked logical-point sizes. Re-marks open entries for layout.
    /// `WidgetArena` calls this from
    /// [`WidgetArena::set_scale_factor`](crate::WidgetArena::set_scale_factor).
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite_core::overlay::OverlayLayer;
    ///
    /// let mut layer = OverlayLayer::new();
    /// layer.set_scale_factor(2.0);
    /// ```
    pub fn set_scale_factor(&mut self, scale: f32) {
        let scale = if scale.is_finite() && scale > 0.0 {
            scale
        } else {
            1.0
        };
        self.scale_factor = scale;
        self.remark_all();
    }

    /// Re-marks every open entry for layout so the next
    /// [`Self::layout_pass`] re-resolves them.
    fn remark_all(&mut self) {
        for entry in &mut self.entries {
            entry.needs_layout = true;
        }
    }

    /// Opens a popup anchored as described and returns its id.
    ///
    /// The popup is placed on top of the z-order and laid out during
    /// the next [`Self::layout_pass`].
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite_core::overlay::{OverlayAnchor, OverlayLayer};
    /// use martensite_core::{DummyWidget, Rect};
    ///
    /// let mut layer = OverlayLayer::new();
    /// let id = layer.open(
    ///     Box::new(DummyWidget),
    ///     OverlayAnchor::Bounds(Rect::new(0.0, 0.0, 50.0, 20.0)),
    /// );
    /// assert!(layer.is_open(id));
    /// ```
    pub fn open(&mut self, content: Box<dyn Widget>, anchor: OverlayAnchor) -> u64 {
        self.open_with(content, anchor, OverlayOptions::default())
    }

    /// Opens a popup with explicit [`OverlayOptions`] — `modal` for
    /// dialogs and drawers that block input to the content beneath.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite_core::overlay::{OverlayAnchor, OverlayLayer, OverlayOptions};
    /// use martensite_core::{DummyWidget, Rect};
    ///
    /// let mut layer = OverlayLayer::new();
    /// layer.set_viewport(Rect::new(0.0, 0.0, 800.0, 600.0));
    /// let id = layer.open_with(
    ///     Box::new(DummyWidget),
    ///     OverlayAnchor::Center,
    ///     OverlayOptions::modal(),
    /// );
    /// assert!(layer.has_modal());
    /// assert!(layer.is_open(id));
    /// ```
    pub fn open_with(
        &mut self,
        content: Box<dyn Widget>,
        anchor: OverlayAnchor,
        options: OverlayOptions,
    ) -> u64 {
        let id = self.next_id;
        self.next_id = self
            .next_id
            .checked_add(1)
            .expect("overlay id space exhausted");
        self.entries.push(OverlayEntry {
            id,
            anchor,
            resolved: Rect::default(),
            needs_layout: true,
            content,
            owner: self.current_owner,
            options,
        });
        self.content_dirty = true;
        id
    }

    /// `true` while any open entry is modal — the layer then blocks
    /// positional input outside the topmost modal's z-range.
    pub fn has_modal(&self) -> bool {
        self.entries.iter().any(|e| e.options.modal)
    }

    /// Closes the popup with the given id. Returns `true` if it was
    /// open.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite_core::overlay::{OverlayAnchor, OverlayLayer};
    /// use martensite_core::{DummyWidget, Rect};
    ///
    /// let mut layer = OverlayLayer::new();
    /// let id = layer.open(
    ///     Box::new(DummyWidget),
    ///     OverlayAnchor::Pointer(glam::Vec2::ZERO),
    /// );
    /// assert!(layer.close(id));
    /// assert!(!layer.close(id));
    /// ```
    pub fn close(&mut self, id: u64) -> bool {
        if self.capture == Some(id) {
            self.capture = None;
        }
        if let Some(position) = self.entries.iter().position(|e| e.id == id) {
            let entry = self.entries.remove(position);
            self.note_dirty_owner(entry.owner);
            true
        } else {
            false
        }
    }

    /// Closes every popup owned by `owner` (stamped at `open` while the
    /// arena was syncing that widget) and returns how many were closed.
    /// Called by `WidgetArena::remove` so a dead widget cannot leave an
    /// orphaned popup painting and hit-testing above content.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite_core::overlay::{OverlayAnchor, OverlayLayer};
    /// use martensite_core::{DummyWidget, Rect, WidgetId};
    ///
    /// let mut layer = OverlayLayer::new();
    /// // Direct opens are ownerless — nothing to close here.
    /// layer.open(
    ///     Box::new(DummyWidget),
    ///     OverlayAnchor::Pointer(glam::Vec2::ZERO),
    /// );
    /// assert_eq!(layer.close_owner(WidgetId::from_parts(0, 1)), 0);
    /// assert_eq!(layer.len(), 1);
    /// ```
    pub fn close_owner(&mut self, owner: WidgetId) -> usize {
        let ids: Vec<u64> = self
            .entries
            .iter()
            .filter(|e| e.owner == Some(owner))
            .map(|e| e.id)
            .collect();
        for id in &ids {
            self.close(*id);
        }
        ids.len()
    }

    /// Closes every open popup.
    pub fn clear(&mut self) {
        let owners: Vec<Option<WidgetId>> = self.entries.iter().map(|e| e.owner).collect();
        self.entries.clear();
        self.capture = None;
        for owner in owners {
            self.note_dirty_owner(owner);
        }
    }

    /// Returns `true` if a popup with `id` is currently open.
    pub fn is_open(&self, id: u64) -> bool {
        self.entries.iter().any(|e| e.id == id)
    }

    /// Number of open popups.
    pub fn len(&self) -> usize {
        self.entries.len()
    }

    /// Whether no popups are open.
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    /// Iterate open entries in bottom→top z-order.
    pub fn entries(&self) -> impl ExactSizeIterator<Item = &OverlayEntry> {
        self.entries.iter()
    }

    /// Look up an entry by id.
    pub fn entry(&self, id: u64) -> Option<&OverlayEntry> {
        self.entries.iter().find(|e| e.id == id)
    }

    /// Look up an entry by id, mutably.
    pub fn entry_mut(&mut self, id: u64) -> Option<&mut OverlayEntry> {
        self.entries.iter_mut().find(|e| e.id == id)
    }

    /// The id of the topmost popup, if any.
    pub fn topmost(&self) -> Option<u64> {
        self.entries.last().map(|e| e.id)
    }

    /// The resolved bounds of the popup with `id`, if open and laid
    /// out.
    pub fn entry_bounds(&self, id: u64) -> Option<Rect> {
        self.entry(id).map(OverlayEntry::bounds)
    }

    /// Replaces an entry's content widget and marks it for layout.
    ///
    /// Used by owners that rebuild popup content when their model
    /// changes while keeping the same overlay id (and z-position).
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite_core::overlay::{OverlayAnchor, OverlayLayer};
    /// use martensite_core::{DummyWidget, Rect};
    ///
    /// let mut layer = OverlayLayer::new();
    /// let id = layer.open(
    ///     Box::new(DummyWidget),
    ///     OverlayAnchor::Pointer(glam::Vec2::ZERO),
    /// );
    /// layer.replace_content(id, Box::new(DummyWidget));
    /// assert!(layer.is_open(id));
    /// ```
    pub fn replace_content(&mut self, id: u64, content: Box<dyn Widget>) -> bool {
        let Some(entry) = self.entry_mut(id) else {
            return false;
        };
        let owner = entry.owner;
        entry.content = content;
        entry.needs_layout = true;
        self.note_dirty_owner(owner);
        true
    }

    /// Updates an entry's anchor and re-marks it for layout.
    pub fn set_anchor(&mut self, id: u64, anchor: OverlayAnchor) -> bool {
        let Some(entry) = self.entry_mut(id) else {
            return false;
        };
        let owner = entry.owner;
        entry.anchor = anchor;
        entry.needs_layout = true;
        self.note_dirty_owner(owner);
        true
    }

    /// Walks `Widget::child_mut` indices inside an entry's content to
    /// reach a nested widget — the overlay analogue of the adapter's
    /// `resolve_internal`, used to deliver accessibility actions to
    /// popup content.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite_core::overlay::{OverlayAnchor, OverlayLayer};
    /// use martensite_core::{DummyWidget, Rect};
    ///
    /// let mut layer = OverlayLayer::new();
    /// let id = layer.open(
    ///     Box::new(DummyWidget),
    ///     OverlayAnchor::Pointer(glam::Vec2::ZERO),
    /// );
    /// // `DummyWidget` has no children — only the empty path resolves.
    /// assert!(layer.widget_at_mut(id, &[]).is_some());
    /// assert!(layer.widget_at_mut(id, &[0]).is_none());
    /// ```
    pub fn widget_at_mut(&mut self, id: u64, path: &[u32]) -> Option<&mut dyn Widget> {
        let entry = self.entry_mut(id)?;
        let mut widget = entry.content_mut();
        for &index in path {
            widget = widget.child_mut(index as usize)?;
        }
        Some(widget)
    }

    /// Returns and clears the id of the oldest popup dismissed by
    /// event dispatch (outside press or Escape).
    ///
    /// A single event can dismiss several popups — an outside press
    /// closes the whole layer — so callers should drain in a loop:
    /// `while let Some(id) = layer.take_dismissed() { … }`.
    ///
    /// Owners such as dropdowns poll this to reconcile their own
    /// expanded state after the layer closed their popup.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite_core::overlay::OverlayLayer;
    ///
    /// let mut layer = OverlayLayer::new();
    /// assert_eq!(layer.take_dismissed(), None);
    /// ```
    pub fn take_dismissed(&mut self) -> Option<u64> {
        self.dismissed.pop_front()
    }

    /// Records that `owner`'s popup state changed (popup closed, or
    /// popup content received an event) so `WidgetArena::sync_overlays`
    /// can dirty-mark it for repaint and accessibility re-emission.
    /// Bounded: stale marks beyond the cap are dropped — a missed mark
    /// only delays re-emission by a frame since `sync_overlays`
    /// reconciles open/closed sets each pass.
    fn note_dirty_owner(&mut self, owner: Option<WidgetId>) {
        const CAP: usize = 64;
        if self.dirty_owners.len() >= CAP {
            self.dirty_owners.pop_front();
        }
        self.dirty_owners.push_back(owner);
        self.content_dirty = true;
    }

    /// Drains the owners whose popup state changed since the last
    /// drain. Called by `WidgetArena::sync_overlays`.
    pub(crate) fn take_dirty_owners(&mut self) -> Vec<WidgetId> {
        self.dirty_owners.drain(..).flatten().collect()
    }

    /// `true` while popup contents may have changed since the flag was
    /// last drained — set when an entry opens, closes, or receives an
    /// event. The `martensite-access` adapter consults this before
    /// deciding an incremental update has nothing to emit.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite_core::overlay::{OverlayAnchor, OverlayLayer};
    /// use martensite_core::{DummyWidget, Rect};
    ///
    /// let mut layer = OverlayLayer::new();
    /// assert!(!layer.take_content_dirty());
    /// layer.open(
    ///     Box::new(DummyWidget),
    ///     OverlayAnchor::Pointer(glam::Vec2::ZERO),
    /// );
    /// assert!(layer.take_content_dirty());
    /// assert!(!layer.take_content_dirty());
    /// ```
    pub fn take_content_dirty(&mut self) -> bool {
        std::mem::take(&mut self.content_dirty)
    }

    /// Stamps subsequent [`open`](Self::open) calls with `owner`.
    /// Called by `WidgetArena::sync_overlays` around each widget's
    /// `sync_overlay` so popups remember which arena widget owns them.
    pub(crate) fn set_current_owner(&mut self, owner: Option<WidgetId>) {
        self.current_owner = owner;
    }

    /// Measure, place, and lay out every entry that needs it.
    ///
    /// Entries are measured against the viewport, placed relative to
    /// their anchor (below-first for [`OverlayAnchor::Bounds`],
    /// below-right for [`OverlayAnchor::Pointer`]), and clamped so the
    /// resolved rect never extends outside the viewport (shrinking if
    /// the popup is larger than the viewport itself).
    ///
    /// # Examples
    ///
    /// ```
    /// use glam::Vec2;
    /// use martensite_core::overlay::{OverlayAnchor, OverlayLayer};
    /// use martensite_core::{DummyWidget, LayoutConstraints, LayoutContext, Rect, Widget};
    /// use martensite_core::HotNode;
    ///
    /// struct Big;
    /// impl Widget for Big {
    ///     fn measure(&mut self, _cx: &mut LayoutContext, _c: LayoutConstraints) -> Vec2 {
    ///         Vec2::new(1000.0, 1000.0)
    ///     }
    ///     fn layout(&mut self, _cx: &mut LayoutContext, _b: Rect) {}
    /// }
    ///
    /// let mut layer = OverlayLayer::new();
    /// layer.set_viewport(Rect::new(0.0, 0.0, 200.0, 100.0));
    /// let id = layer.open(Box::new(Big), OverlayAnchor::Pointer(Vec2::new(190.0, 90.0)));
    /// layer.layout_pass();
    /// let b = layer.entry_bounds(id).unwrap();
    /// // Clamped into the viewport even though the widget wants 1000×1000.
    /// assert!(b.max_x() <= 200.0 && b.max_y() <= 100.0);
    /// ```
    pub fn layout_pass(&mut self) {
        let viewport = self.viewport;
        for entry in &mut self.entries {
            if !entry.needs_layout {
                continue;
            }
            let mut hot = HotNode::default();
            let mut cx = LayoutContext {
                hot: &mut hot,
                scale: self.scale_factor,
            };
            let desired = entry.content.measure(
                &mut cx,
                LayoutConstraints {
                    min_size: Vec2::ZERO,
                    max_size: viewport.size,
                },
            );
            let resolved = place(&entry.anchor, desired, viewport, self.scale_factor);
            entry.content.layout(&mut cx, resolved);
            entry.resolved = resolved;
            entry.needs_layout = false;
        }
    }

    /// Appends paint commands for all open popups, bottom→top, so the
    /// result paints above previously-recorded content.
    ///
    /// For standalone layers only —
    /// [`WidgetArena::build_paint_list`](crate::WidgetArena::build_paint_list)
    /// already calls this on the arena-owned layer; painting it again
    /// emits every popup twice (covered-text and self-overlap audit
    /// findings).
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite_core::overlay::{OverlayAnchor, OverlayLayer};
    /// use martensite_core::{DummyWidget, PaintList, Rect};
    ///
    /// let mut layer = OverlayLayer::new();
    /// layer.set_viewport(Rect::new(0.0, 0.0, 800.0, 600.0));
    /// layer.open(
    ///     Box::new(DummyWidget),
    ///     OverlayAnchor::Pointer(glam::Vec2::ZERO),
    /// );
    /// layer.layout_pass();
    ///
    /// let mut list = PaintList::new();
    /// layer.paint(&mut list, &martensite_theme::Theme::new("fallback"), None);
    /// // `DummyWidget` emits no chrome — only its provenance scope plus
    /// // the "Overlay" wrapper scope.
    /// assert_eq!(list.commands.len(), 4);
    /// ```
    ///
    /// `theme` is forwarded to popup content through
    /// [`PaintContext::theme`](crate::PaintContext::theme); callers
    /// should pass [`WidgetArena::theme`](crate::WidgetArena::theme) so
    /// popups resolve the same tokens as arena content. Scale comes from
    /// [`Self::set_scale_factor`] — the same value `layout_pass`
    /// measured with, so popup layout and paint can never disagree.
    pub fn paint(
        &self,
        list: &mut PaintList,
        theme: &martensite_theme::Theme,
        text_painter: Option<&(dyn crate::paint::TextShaper + Send + Sync)>,
    ) {
        // One parent scope around all popup content: the paint audit
        // treats "Overlay"-scoped fills/text as intentional occlusion,
        // so a popup covering page content is not flagged as a bug —
        // while same-scope defects (a popup covering its own text)
        // still are.
        if !self.entries.is_empty() {
            list.push_scope(None, "Overlay", crate::arena::rect_to_kurbo(self.viewport));
            let scrim = theme
                .color(martensite_theme::TokenKey::ScrimColor)
                .map_or([8, 10, 16, 102], |c| c.to_srgba8());
            let scrim_rect = crate::arena::rect_to_kurbo(self.viewport);
            for entry in &self.entries {
                if entry.options.scrim || entry.options.modal {
                    list.push_fill_rect(scrim_rect, scrim);
                }
                paint_widget_recursive(
                    entry.content(),
                    entry.resolved,
                    list,
                    theme,
                    self.scale_factor,
                    text_painter,
                );
            }
            list.pop_scope();
        }
    }

    /// Dispatches an event to the overlay before window content.
    ///
    /// Ordering and semantics:
    ///
    /// - While an entry holds overlay pointer capture, the event goes
    ///   to that entry regardless of position.
    /// - Positional events go to the topmost entry whose resolved
    ///   bounds contain the position; a popup that swallows the event
    ///   returns the content's response (an `Ignored` from inside a
    ///   popup still counts as [`EventResponse::Handled`] — the popup
    ///   sits above the content beneath it).
    /// - A [`WidgetEvent::PointerPressed`] that lands outside every
    ///   popup dismisses all of them and returns
    ///   [`EventResponse::Ignored`], letting the press continue to the
    ///   content beneath.
    /// - `Escape` dismisses the topmost popup and returns
    ///   [`EventResponse::Handled`]. Other key events are offered to
    ///   the topmost popup's content first; an `Ignored` response
    ///   falls through so the focused arena widget (the popup's
    ///   owner) keeps receiving input — combobox typeahead keeps
    ///   working while a listbox popup is open, while popups with
    ///   their own key handling (menu arrow keys, Enter) get it.
    /// - Scroll and other positional events hit-test like pointer
    ///   events, so popup content can scroll.
    ///
    /// [`EventResponse::CapturePointer`] /
    /// [`EventResponse::ReleasePointer`] from popup content are
    /// honoured by the layer itself.
    ///
    /// Entries opened since the last [`Self::layout_pass`] are laid out
    /// lazily here (when a viewport is set) so hit-testing never sees a
    /// stale zero-size rect between `open` and the frame's layout pass.
    ///
    /// # Examples
    ///
    /// ```
    /// use glam::Vec2;
    /// use martensite_core::overlay::{OverlayAnchor, OverlayLayer};
    /// use martensite_core::{
    ///     DummyWidget, EventResponse, PointerButton, Rect, WidgetEvent,
    /// };
    ///
    /// let mut layer = OverlayLayer::new();
    /// layer.set_viewport(Rect::new(0.0, 0.0, 800.0, 600.0));
    /// layer.open(
    ///     Box::new(DummyWidget),
    ///     OverlayAnchor::Pointer(Vec2::new(10.0, 10.0)),
    /// );
    /// layer.layout_pass();
    ///
    /// // A press outside the popup dismisses it and falls through.
    /// let press = WidgetEvent::PointerPressed {
    ///     position: Vec2::new(700.0, 500.0),
    ///     button: PointerButton::Primary,
    ///     count: 1,
    /// };
    /// assert_eq!(layer.dispatch_event(&press), EventResponse::Ignored);
    /// assert!(layer.is_empty());
    /// ```
    pub fn dispatch_event(&mut self, event: &WidgetEvent) -> EventResponse {
        // Entries opened since the last layout pass would hit-test
        // against a stale zero-size rect; resolve them first so a press
        // between `open` and the frame's `layout_pass` doesn't count as
        // an outside click. Skipped while the viewport is unset —
        // placement needs a real clamp rect.
        if self.viewport.width() > 0.0 && self.viewport.height() > 0.0 {
            self.layout_pass();
        }

        // Keyboard: Escape dismisses the topmost popup. Every other
        // key is offered to the topmost popup's content first —
        // popup widgets like menus have real keyboard handling
        // (arrow-key highlight, Enter to commit). Unlike positional
        // events an `Ignored` key is NOT swallowed: it falls through
        // so the focused arena widget (the popup's owner) keeps
        // receiving input it cares about — combobox typeahead, grid
        // navigation.
        if matches!(
            event,
            WidgetEvent::KeyPressed { .. } | WidgetEvent::KeyReleased { .. }
        ) {
            if let WidgetEvent::KeyPressed { key, .. } = event {
                if key == "Escape" {
                    // Passthrough entries (toast strips) are never the
                    // Escape target — the dismissible popup below them is.
                    if let Some(top) = self
                        .entries
                        .iter()
                        .rev()
                        .find(|e| !e.options.passthrough)
                        .map(|e| e.id)
                    {
                        self.close(top);
                        self.dismissed.push_back(top);
                        return EventResponse::Handled;
                    }
                }
            }
            if let Some(entry) = self.entries.last_mut() {
                let owner = entry.owner;
                let mut cx = EventContext {
                    event,
                    bounds: entry.resolved,
                    scale: self.scale_factor,
                };
                let response = entry.content.event(&mut cx);
                let id = entry.id;
                self.apply_capture_response(id, response);
                self.note_dirty_owner(owner);
                if response != EventResponse::Ignored {
                    return response;
                }
            }
            return EventResponse::Ignored;
        }

        // Pointer capture: deliver to the capturing entry regardless of
        // hit-testing.
        if let Some(captured) = self.capture {
            if let Some(index) = self.entries.iter().position(|e| e.id == captured) {
                let entry = &mut self.entries[index];
                let owner = entry.owner;
                let mut cx = EventContext {
                    event,
                    bounds: entry.resolved,
                    scale: self.scale_factor,
                };
                let response = entry.content.event(&mut cx);
                let id = entry.id;
                self.apply_capture_response(id, response);
                self.note_dirty_owner(owner);
                return swallow_ignored(response);
            }
            self.capture = None;
        }

        // Positional events hit-test topmost-first. While a modal entry
        // is open the scrim is the event floor: entries below the
        // topmost modal are unreachable (it covers them), and events
        // outside every reachable popup land on the scrim — consumed,
        // never forwarded to window content.
        if let Some(position) = event.position() {
            let floor = self
                .entries
                .iter()
                .rposition(|e| e.options.modal)
                .unwrap_or(0);
            for index in (floor..self.entries.len()).rev() {
                if self.entries[index].resolved.contains(position) {
                    let entry = &mut self.entries[index];
                    let owner = entry.owner;
                    let mut cx = EventContext {
                        event,
                        bounds: entry.resolved,
                        scale: self.scale_factor,
                    };
                    let response = entry.content.event(&mut cx);
                    let id = entry.id;
                    self.apply_capture_response(id, response);
                    self.note_dirty_owner(owner);
                    // A passthrough entry that ignores the event lets
                    // it continue down the stack — its bounds cover a
                    // strip, not every card in it.
                    if self.entries[index].options.passthrough && response == EventResponse::Ignored
                    {
                        continue;
                    }
                    return swallow_ignored(response);
                }
            }
            if let Some(m) = self.entries.iter().rposition(|e| e.options.modal) {
                // On the scrim. A press light-dismisses the popups
                // stacked above the modal, and the modal itself only
                // when it opted into scrim dismissal.
                if matches!(event, WidgetEvent::PointerPressed { .. }) {
                    let above: Vec<u64> = self.entries[m + 1..]
                        .iter()
                        .filter(|e| !e.options.passthrough)
                        .map(|e| e.id)
                        .collect();
                    self.dismissed.extend(above.iter().copied());
                    for id in above {
                        self.close(id);
                    }
                    if self.entries.get(m).is_some_and(|e| e.options.scrim_dismiss) {
                        let id = self.entries[m].id;
                        self.dismissed.push_back(id);
                        self.close(id);
                    }
                }
                return EventResponse::Handled;
            }
            // Outside every popup: a press dismisses the dismissible
            // popups (passthrough layers survive — toasts must outlive
            // unrelated clicks) and falls through to window content.
            if matches!(event, WidgetEvent::PointerPressed { .. })
                && self.entries.iter().any(|e| !e.options.passthrough)
            {
                let ids: Vec<u64> = self
                    .entries
                    .iter()
                    .filter(|e| !e.options.passthrough)
                    .map(|e| e.id)
                    .collect();
                self.dismissed.extend(ids.iter().copied());
                for id in ids {
                    self.close(id);
                }
                return EventResponse::Ignored;
            }
        }
        EventResponse::Ignored
    }

    /// Applies a content response to overlay-level pointer capture.
    fn apply_capture_response(&mut self, entry: u64, response: EventResponse) {
        match response {
            EventResponse::CapturePointer => self.capture = Some(entry),
            EventResponse::ReleasePointer if self.capture == Some(entry) => {
                self.capture = None;
            }
            _ => {}
        }
    }
}

/// An `Ignored` response from inside a popup still consumes the event —
/// the popup paints above whatever lies beneath it, so events must not
/// fall through to that content.
fn swallow_ignored(response: EventResponse) -> EventResponse {
    match response {
        EventResponse::Ignored => EventResponse::Handled,
        other => other,
    }
}

/// Resolves a popup rect for `anchor` at `desired` size inside
/// `viewport`, applying placement preference, flip, and clamp.
/// `scale` converts the logical-point gap/offset constants into the
/// viewport's device-pixel space.
fn place(anchor: &OverlayAnchor, desired: Vec2, viewport: Rect, scale: f32) -> Rect {
    // A popup can never be larger than the viewport.
    let size = Vec2::new(
        desired.x.clamp(0.0, viewport.width().max(0.0)),
        desired.y.clamp(0.0, viewport.height().max(0.0)),
    );
    // Edge anchors span the viewport on the flow axis — the measured
    // size supplies only the depth (drawer width, sheet height).
    let size = match anchor {
        OverlayAnchor::EdgeLeft | OverlayAnchor::EdgeRight => {
            Vec2::new(size.x, viewport.height().max(0.0))
        }
        OverlayAnchor::EdgeTop | OverlayAnchor::EdgeBottom => {
            Vec2::new(viewport.width().max(0.0), size.y)
        }
        _ => size,
    };
    let gap = ANCHOR_GAP * scale;
    let offset = POINTER_OFFSET * scale;
    let align_axis = |a: ViewportAlign, min: f32, max: f32, extent: f32, m: f32| match a {
        ViewportAlign::Start => min + m,
        ViewportAlign::Center => min + (max - min - extent) / 2.0,
        ViewportAlign::End => max - m - extent,
    };

    let mut origin = match anchor {
        OverlayAnchor::Bounds(a) => {
            let below_y = a.max_y() + gap;
            let above_y = a.min_y() - size.y - gap;
            let y = if below_y + size.y <= viewport.max_y() {
                below_y
            } else if above_y >= viewport.min_y() {
                above_y
            } else {
                // Neither side fully fits — prefer below and let the
                // clamp pull it back on screen.
                below_y
            };
            Vec2::new(a.min_x(), y)
        }
        OverlayAnchor::Pointer(p) => Vec2::new(p.x + offset, p.y + offset),
        OverlayAnchor::Center => Vec2::new(
            viewport.min_x() + (viewport.width() - size.x) / 2.0,
            viewport.min_y() + (viewport.height() - size.y) / 2.0,
        ),
        OverlayAnchor::Viewport { h, v, margin } => {
            let m = margin * scale;
            Vec2::new(
                align_axis(*h, viewport.min_x(), viewport.max_x(), size.x, m),
                align_axis(*v, viewport.min_y(), viewport.max_y(), size.y, m),
            )
        }
        OverlayAnchor::EdgeLeft => Vec2::new(viewport.min_x(), viewport.min_y()),
        OverlayAnchor::EdgeRight => Vec2::new(viewport.max_x() - size.x, viewport.min_y()),
        OverlayAnchor::EdgeTop => Vec2::new(viewport.min_x(), viewport.min_y()),
        OverlayAnchor::EdgeBottom => Vec2::new(viewport.min_x(), viewport.max_y() - size.y),
    };

    // Clamp the origin so the rect stays inside the viewport; `max`
    // guards keep `clamp` well-formed when the popup is as large as the
    // viewport itself.
    let max_origin_x = (viewport.max_x() - size.x).max(viewport.min_x());
    let max_origin_y = (viewport.max_y() - size.y).max(viewport.min_y());
    origin.x = origin.x.clamp(viewport.min_x(), max_origin_x);
    origin.y = origin.y.clamp(viewport.min_y(), max_origin_y);

    Rect::new(origin.x, origin.y, size.x, size.y)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{DummyWidget, PaintCommand, PointerButton};

    use glam::Vec2;

    /// Probe widget that records a marker rect of a given color so paint
    /// order is observable.
    struct Marker(u8);

    impl Widget for Marker {
        fn measure(&mut self, _cx: &mut LayoutContext, _c: LayoutConstraints) -> Vec2 {
            Vec2::new(20.0, 20.0)
        }
        fn layout(&mut self, _cx: &mut LayoutContext, _b: Rect) {}
        fn paint(&self, cx: &mut crate::PaintContext) {
            cx.list
                .push_fill_rect(kurbo::Rect::new(0.0, 0.0, 1.0, 1.0), [self.0, 0, 0, 255]);
        }
    }

    fn layer() -> OverlayLayer {
        let mut layer = OverlayLayer::new();
        layer.set_viewport(Rect::new(0.0, 0.0, 800.0, 600.0));
        layer
    }

    #[test]
    fn outside_press_dismisses_all_popups() {
        let mut layer = layer();
        let a = layer.open(
            Box::new(DummyWidget),
            OverlayAnchor::Bounds(Rect::new(10.0, 10.0, 50.0, 20.0)),
        );
        let b = layer.open(
            Box::new(DummyWidget),
            OverlayAnchor::Bounds(Rect::new(100.0, 100.0, 50.0, 20.0)),
        );
        layer.layout_pass();
        assert!(layer.is_open(a) && layer.is_open(b));

        let press = WidgetEvent::PointerPressed {
            position: Vec2::new(700.0, 500.0),
            button: PointerButton::Primary,
            count: 1,
        };
        assert_eq!(layer.dispatch_event(&press), EventResponse::Ignored);
        assert!(layer.is_empty());
        // Every dismissed popup is recorded, in z-order.
        assert_eq!(layer.take_dismissed(), Some(a));
        assert_eq!(layer.take_dismissed(), Some(b));
        assert_eq!(layer.take_dismissed(), None);
    }

    #[test]
    fn press_inside_popup_does_not_dismiss() {
        let mut layer = layer();
        let id = layer.open(
            Box::new(Marker(1)),
            OverlayAnchor::Bounds(Rect::new(10.0, 10.0, 50.0, 20.0)),
        );
        layer.layout_pass();
        let b = layer.entry_bounds(id).unwrap();
        let inside = Vec2::new(b.min_x() + 1.0, b.min_y() + 1.0);
        let press = WidgetEvent::PointerPressed {
            position: inside,
            button: PointerButton::Primary,
            count: 1,
        };
        // `DummyWidget` ignores input but the popup swallows the event.
        assert_eq!(layer.dispatch_event(&press), EventResponse::Handled);
        assert!(layer.is_open(id));
    }

    #[test]
    fn escape_dismisses_topmost_only() {
        let mut layer = layer();
        let a = layer.open(
            Box::new(DummyWidget),
            OverlayAnchor::Pointer(Vec2::new(10.0, 10.0)),
        );
        let b = layer.open(
            Box::new(DummyWidget),
            OverlayAnchor::Pointer(Vec2::new(20.0, 20.0)),
        );
        let escape = WidgetEvent::KeyPressed {
            key: "Escape".to_string(),
            repeat: false,
        };
        assert_eq!(layer.dispatch_event(&escape), EventResponse::Handled);
        assert!(layer.is_open(a));
        assert!(!layer.is_open(b));
        assert_eq!(layer.take_dismissed(), Some(b));
    }

    #[test]
    fn non_escape_keys_reach_topmost_popup() {
        // Popup widgets (menus, listboxes) handle their own arrow/Enter
        // keys — the layer must offer non-Escape keys to the topmost
        // entry before falling through to the focused arena widget.
        struct KeySpy(std::sync::Arc<std::sync::Mutex<Vec<String>>>);
        impl Widget for KeySpy {
            fn measure(&mut self, _cx: &mut LayoutContext, _c: LayoutConstraints) -> Vec2 {
                Vec2::new(20.0, 20.0)
            }
            fn layout(&mut self, _cx: &mut LayoutContext, _b: Rect) {}
            fn event(&mut self, cx: &mut EventContext) -> EventResponse {
                match cx.event {
                    WidgetEvent::KeyPressed { key, .. } => {
                        self.0.lock().unwrap().push(key.clone());
                        EventResponse::Handled
                    }
                    _ => EventResponse::Ignored,
                }
            }
        }

        let mut layer = layer();
        let seen = std::sync::Arc::new(std::sync::Mutex::new(Vec::new()));
        layer.open(
            Box::new(KeySpy(std::sync::Arc::clone(&seen))),
            OverlayAnchor::Pointer(Vec2::new(10.0, 10.0)),
        );
        layer.layout_pass();

        let down = WidgetEvent::KeyPressed {
            key: "ArrowDown".to_string(),
            repeat: false,
        };
        assert_eq!(layer.dispatch_event(&down), EventResponse::Handled);
        assert_eq!(*seen.lock().unwrap(), vec!["ArrowDown".to_string()]);
    }

    #[test]
    fn ignored_keys_fall_through_to_owner() {
        // A popup that ignores a key must not swallow it — the focused
        // arena widget (combobox typeahead, grid nav) still gets it.
        let mut layer = layer();
        layer.open(
            Box::new(DummyWidget),
            OverlayAnchor::Pointer(Vec2::new(10.0, 10.0)),
        );
        layer.layout_pass();
        let key = WidgetEvent::KeyPressed {
            key: "a".to_string(),
            repeat: false,
        };
        assert_eq!(layer.dispatch_event(&key), EventResponse::Ignored);
        // And the popup stays open — key fall-through is not dismissal.
        assert!(!layer.is_empty());
    }

    #[test]
    fn popup_clamps_to_viewport_edges() {
        let mut layer = layer();
        // Anchor in the bottom-right corner; popup must clamp inside.
        let id = layer.open(
            Box::new(Marker(1)),
            OverlayAnchor::Pointer(Vec2::new(790.0, 590.0)),
        );
        layer.layout_pass();
        let b = layer.entry_bounds(id).unwrap();
        assert!(b.max_x() <= 800.0, "right edge: {:?}", b);
        assert!(b.max_y() <= 600.0, "bottom edge: {:?}", b);
        assert!(b.min_x() >= 0.0 && b.min_y() >= 0.0);
    }

    #[test]
    fn bounds_anchor_prefers_below_then_flips_above() {
        let mut layer = layer();
        let below = layer.open(
            Box::new(Marker(1)),
            OverlayAnchor::Bounds(Rect::new(10.0, 10.0, 50.0, 20.0)),
        );
        // Anchor hugging the bottom edge — no room below, flips above.
        let above = layer.open(
            Box::new(Marker(2)),
            OverlayAnchor::Bounds(Rect::new(10.0, 590.0, 50.0, 10.0)),
        );
        layer.layout_pass();
        let b = layer.entry_bounds(below).unwrap();
        assert_eq!(b.min_y(), 30.0 + ANCHOR_GAP);
        let a = layer.entry_bounds(above).unwrap();
        assert_eq!(a.max_y(), 590.0 - ANCHOR_GAP);
    }

    #[test]
    fn paint_order_is_z_order() {
        let mut layer = layer();
        layer.open(
            Box::new(Marker(1)),
            OverlayAnchor::Pointer(Vec2::new(0.0, 0.0)),
        );
        layer.open(
            Box::new(Marker(2)),
            OverlayAnchor::Pointer(Vec2::new(5.0, 5.0)),
        );
        layer.layout_pass();
        let mut list = PaintList::new();
        layer.paint(&mut list, &martensite_theme::Theme::new("fallback"), None);
        // Each entry is wrapped in a provenance scope — filter to the
        // fills to check paint order.
        let fills: Vec<_> = list
            .commands
            .iter()
            .filter(|c| matches!(c, PaintCommand::FillRect(..)))
            .collect();
        assert_eq!(fills.len(), 2);
        // Bottom entry paints first so the topmost lands above it.
        assert!(matches!(
            fills[0],
            PaintCommand::FillRect(_, [1, 0, 0, 255])
        ));
        assert!(matches!(
            fills[1],
            PaintCommand::FillRect(_, [2, 0, 0, 255])
        ));
    }

    #[test]
    fn topmost_receives_events_first() {
        struct Grab;
        impl Widget for Grab {
            fn measure(&mut self, _cx: &mut LayoutContext, _c: LayoutConstraints) -> Vec2 {
                Vec2::new(100.0, 100.0)
            }
            fn layout(&mut self, _cx: &mut LayoutContext, _b: Rect) {}
            fn event(&mut self, _cx: &mut EventContext) -> EventResponse {
                EventResponse::RequestRepaint
            }
        }

        let mut layer = layer();
        // Bottom entry covers the whole viewport; top entry is small but
        // sits on top — the press must reach the top entry.
        layer.open(
            Box::new(Marker(1)),
            OverlayAnchor::Bounds(Rect::new(0.0, 0.0, 800.0, 600.0)),
        );
        let top = layer.open(
            Box::new(Grab),
            OverlayAnchor::Pointer(Vec2::new(50.0, 50.0)),
        );
        layer.layout_pass();
        // Press inside the top entry's resolved bounds.
        let b = layer.entry_bounds(top).unwrap();
        let press = WidgetEvent::PointerPressed {
            position: Vec2::new(b.min_x() + 5.0, b.min_y() + 5.0),
            button: PointerButton::Primary,
            count: 1,
        };
        assert_eq!(layer.dispatch_event(&press), EventResponse::RequestRepaint);
    }

    #[test]
    fn overlay_pointer_capture_routes_to_entry() {
        struct Capturing;
        impl Widget for Capturing {
            fn measure(&mut self, _cx: &mut LayoutContext, _c: LayoutConstraints) -> Vec2 {
                Vec2::new(50.0, 50.0)
            }
            fn layout(&mut self, _cx: &mut LayoutContext, _b: Rect) {}
            fn event(&mut self, cx: &mut EventContext) -> EventResponse {
                match cx.event {
                    WidgetEvent::PointerPressed { .. } => EventResponse::CapturePointer,
                    WidgetEvent::PointerReleased { .. } => EventResponse::ReleasePointer,
                    _ => EventResponse::Handled,
                }
            }
        }

        let mut layer = layer();
        let id = layer.open(
            Box::new(Capturing),
            OverlayAnchor::Pointer(Vec2::new(0.0, 0.0)),
        );
        layer.layout_pass();

        // Press inside the entry's resolved bounds (Pointer anchors are
        // offset below-right of the anchor point).
        let b = layer.entry_bounds(id).unwrap();
        let press = WidgetEvent::PointerPressed {
            position: Vec2::new(b.min_x() + 1.0, b.min_y() + 1.0),
            button: PointerButton::Primary,
            count: 1,
        };
        assert_eq!(layer.dispatch_event(&press), EventResponse::CapturePointer);

        // A move far outside the popup still reaches it while captured.
        let moved = WidgetEvent::PointerMoved {
            position: Vec2::new(700.0, 500.0),
        };
        assert_eq!(layer.dispatch_event(&moved), EventResponse::Handled);

        let release = WidgetEvent::PointerReleased {
            position: Vec2::new(700.0, 500.0),
            button: PointerButton::Primary,
        };
        assert_eq!(
            layer.dispatch_event(&release),
            EventResponse::ReleasePointer
        );

        // After release, an outside press dismisses the popup.
        let outside = WidgetEvent::PointerPressed {
            position: Vec2::new(700.0, 500.0),
            button: PointerButton::Primary,
            count: 1,
        };
        assert_eq!(layer.dispatch_event(&outside), EventResponse::Ignored);
        assert!(!layer.is_open(id));
    }

    #[test]
    fn modal_blocks_outside_press_and_content() {
        let mut layer = layer();
        let id = layer.open_with(
            Box::new(DummyWidget),
            OverlayAnchor::Center,
            OverlayOptions::modal(),
        );
        layer.layout_pass();
        assert!(layer.has_modal());

        // Press on the scrim — consumed, modal stays, nothing falls
        // through to content.
        let press = WidgetEvent::PointerPressed {
            position: Vec2::new(10.0, 10.0),
            button: PointerButton::Primary,
            count: 1,
        };
        assert_eq!(layer.dispatch_event(&press), EventResponse::Handled);
        assert!(layer.is_open(id));
        // Moves and scrolls are swallowed by the scrim too.
        let mv = WidgetEvent::PointerMoved {
            position: Vec2::new(10.0, 10.0),
        };
        assert_eq!(layer.dispatch_event(&mv), EventResponse::Handled);
        // Escape still dismisses (the cancel convention).
        let esc = WidgetEvent::KeyPressed {
            key: "Escape".into(),
            repeat: false,
        };
        assert_eq!(layer.dispatch_event(&esc), EventResponse::Handled);
        assert!(!layer.is_open(id));
    }

    #[test]
    fn scrim_dismiss_modal_closes_on_outside_press() {
        let mut layer = layer();
        let id = layer.open_with(
            Box::new(DummyWidget),
            OverlayAnchor::EdgeRight,
            OverlayOptions::modal().light_dismiss(),
        );
        layer.layout_pass();
        let press = WidgetEvent::PointerPressed {
            position: Vec2::new(10.0, 10.0),
            button: PointerButton::Primary,
            count: 1,
        };
        assert_eq!(layer.dispatch_event(&press), EventResponse::Handled);
        assert!(!layer.is_open(id));
        assert_eq!(layer.take_dismissed(), Some(id));
    }

    #[test]
    fn popup_below_modal_is_unreachable() {
        let mut layer = layer();
        // A non-modal popup opened first…
        let menu = layer.open(
            Box::new(DummyWidget),
            OverlayAnchor::Pointer(Vec2::new(10.0, 10.0)),
        );
        // …then a modal dialog on top of it.
        layer.open_with(
            Box::new(DummyWidget),
            OverlayAnchor::Center,
            OverlayOptions::modal(),
        );
        layer.layout_pass();
        let mb = layer.entry_bounds(menu).unwrap();
        // A press inside the lower popup's rect hits the scrim, not
        // the popup — and must not dismiss the modal either.
        let press = WidgetEvent::PointerPressed {
            position: Vec2::new(mb.min_x() + 1.0, mb.min_y() + 1.0),
            button: PointerButton::Primary,
            count: 1,
        };
        assert_eq!(layer.dispatch_event(&press), EventResponse::Handled);
        assert!(layer.is_open(menu));
        assert_eq!(layer.len(), 2);
    }

    #[test]
    fn popup_above_modal_light_dismisses_on_scrim_press() {
        let mut layer = layer();
        layer.open_with(
            Box::new(DummyWidget),
            OverlayAnchor::Center,
            OverlayOptions::modal(),
        );
        let tip = layer.open(
            Box::new(DummyWidget),
            OverlayAnchor::Pointer(Vec2::new(10.0, 10.0)),
        );
        layer.layout_pass();
        let press = WidgetEvent::PointerPressed {
            position: Vec2::new(400.0, 10.0),
            button: PointerButton::Primary,
            count: 1,
        };
        assert_eq!(layer.dispatch_event(&press), EventResponse::Handled);
        // The non-modal popup above the modal light-dismissed; the
        // modal (no scrim_dismiss) stayed.
        assert!(!layer.is_open(tip));
        assert_eq!(layer.len(), 1);
        assert!(layer.has_modal());
    }

    #[test]
    fn modal_paints_scrim() {
        let mut layer = layer();
        layer.open_with(
            Box::new(DummyWidget),
            OverlayAnchor::Center,
            OverlayOptions::modal(),
        );
        layer.layout_pass();
        let mut list = PaintList::new();
        layer.paint(&mut list, &martensite_theme::Theme::new("t"), None);
        let scrimmed = list.commands.iter().any(|c| {
            matches!(
                c,
                PaintCommand::FillRect(_, color)
                    if color[3] > 0 && color[3] < 255
            )
        });
        assert!(scrimmed, "modal entry must emit a translucent scrim");
    }

    #[test]
    fn edge_and_viewport_anchors_place() {
        let mut layer = layer();
        // Right drawer: 300pt wide, full viewport height.
        struct Sized(Vec2);
        impl Widget for Sized {
            fn measure(&mut self, _cx: &mut LayoutContext, _c: LayoutConstraints) -> Vec2 {
                self.0
            }
            fn layout(&mut self, _cx: &mut LayoutContext, _b: Rect) {}
        }
        let d = layer.open_with(
            Box::new(Sized(Vec2::new(300.0, 50.0))),
            OverlayAnchor::EdgeRight,
            OverlayOptions::modal().light_dismiss(),
        );
        let t = layer.open(
            Box::new(Sized(Vec2::new(120.0, 60.0))),
            OverlayAnchor::Viewport {
                h: ViewportAlign::End,
                v: ViewportAlign::End,
                margin: 12.0,
            },
        );
        let c = layer.open_with(
            Box::new(Sized(Vec2::new(200.0, 100.0))),
            OverlayAnchor::Center,
            OverlayOptions::modal(),
        );
        layer.layout_pass();
        let db = layer.entry_bounds(d).unwrap();
        assert_eq!(db, Rect::new(500.0, 0.0, 300.0, 600.0));
        let tb = layer.entry_bounds(t).unwrap();
        assert_eq!(tb, Rect::new(668.0, 528.0, 120.0, 60.0));
        let cb = layer.entry_bounds(c).unwrap();
        assert_eq!(cb, Rect::new(300.0, 250.0, 200.0, 100.0));
    }
}
