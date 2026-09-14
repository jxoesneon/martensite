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
#[derive(Clone, Debug)]
pub enum OverlayAnchor {
    /// Popup anchored to a widget's window-space bounds — placed below
    /// the anchor when it fits, flipped above when it does not, and
    /// finally clamped into the viewport.
    Bounds(Rect),
    /// Popup anchored to a pointer position, offset below-right and
    /// flipped/clamped into the viewport.
    Pointer(Vec2),
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
}

/// Z-ordered collection of in-window popup surfaces.
///
/// See the [module documentation](self) for the integration contract.
pub struct OverlayLayer {
    /// Entries in bottom→top z-order; `entries.last()` is topmost.
    entries: Vec<OverlayEntry>,
    /// Next overlay id to hand out (monotonic, never reused).
    next_id: u64,
    /// Window viewport in logical pixels used for clamping.
    viewport: Rect,
    /// FIFO of entry ids dismissed by [`Self::dispatch_event`]
    /// (outside press or Escape), for owners that poll for closure.
    dismissed: VecDeque<u64>,
    /// Entry currently holding overlay-level pointer capture.
    capture: Option<u64>,
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
            capture: None,
        }
    }

    /// The viewport popups are clamped to.
    pub fn viewport(&self) -> Rect {
        self.viewport
    }

    /// Sets the viewport popups are clamped to and re-marks every open
    /// entry for layout so it re-clamps on the next
    /// [`Self::layout_pass`].
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
        });
        id
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
        let before = self.entries.len();
        self.entries.retain(|e| e.id != id);
        self.entries.len() != before
    }

    /// Closes every open popup.
    pub fn clear(&mut self) {
        self.entries.clear();
        self.capture = None;
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
        entry.content = content;
        entry.needs_layout = true;
        true
    }

    /// Updates an entry's anchor and re-marks it for layout.
    pub fn set_anchor(&mut self, id: u64, anchor: OverlayAnchor) -> bool {
        let Some(entry) = self.entry_mut(id) else {
            return false;
        };
        entry.anchor = anchor;
        entry.needs_layout = true;
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
            let mut cx = LayoutContext { hot: &mut hot };
            let desired = entry.content.measure(
                &mut cx,
                LayoutConstraints {
                    min_size: Vec2::ZERO,
                    max_size: viewport.size,
                },
            );
            let resolved = place(&entry.anchor, desired, viewport);
            entry.content.layout(&mut cx, resolved);
            entry.resolved = resolved;
            entry.needs_layout = false;
        }
    }

    /// Appends paint commands for all open popups, bottom→top, so the
    /// result paints above previously-recorded content.
    ///
    /// Call after [`crate::WidgetArena::build_paint_list`] with the
    /// same [`PaintList`].
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
    /// layer.paint(&mut list);
    /// // `DummyWidget` emits no chrome.
    /// assert!(list.is_empty());
    /// ```
    pub fn paint(&self, list: &mut PaintList) {
        for entry in &self.entries {
            paint_widget_recursive(entry.content(), entry.resolved, list);
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
    ///   [`EventResponse::Handled`]. **Other keys are ignored** —
    ///   keyboard input stays with the focused arena widget (the
    ///   popup's owner), so e.g. combobox typeahead keeps working
    ///   while a listbox popup is open.
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

        // Keyboard: only Escape is meaningful to the layer — it
        // dismisses the topmost popup. Every other key falls through so
        // the focused arena widget (the popup's owner) keeps receiving
        // input; popup content is a stateless view of owner state and
        // never owns focus.
        if matches!(
            event,
            WidgetEvent::KeyPressed { .. } | WidgetEvent::KeyReleased { .. }
        ) {
            if let WidgetEvent::KeyPressed { key, .. } = event {
                if key == "Escape" {
                    if let Some(top) = self.entries.last().map(|e| e.id) {
                        self.close(top);
                        self.dismissed.push_back(top);
                        return EventResponse::Handled;
                    }
                }
            }
            return EventResponse::Ignored;
        }

        // Pointer capture: deliver to the capturing entry regardless of
        // hit-testing.
        if let Some(captured) = self.capture {
            if let Some(index) = self.entries.iter().position(|e| e.id == captured) {
                let entry = &mut self.entries[index];
                let mut cx = EventContext {
                    event,
                    bounds: entry.resolved,
                };
                let response = entry.content.event(&mut cx);
                let id = entry.id;
                self.apply_capture_response(id, response);
                return swallow_ignored(response);
            }
            self.capture = None;
        }

        // Positional events hit-test topmost-first.
        if let Some(position) = event.position() {
            for index in (0..self.entries.len()).rev() {
                if self.entries[index].resolved.contains(position) {
                    let entry = &mut self.entries[index];
                    let mut cx = EventContext {
                        event,
                        bounds: entry.resolved,
                    };
                    let response = entry.content.event(&mut cx);
                    let id = entry.id;
                    self.apply_capture_response(id, response);
                    return swallow_ignored(response);
                }
            }
            // Outside every popup: a press dismisses all popups and
            // falls through to window content.
            if matches!(event, WidgetEvent::PointerPressed { .. }) && !self.entries.is_empty() {
                // Record every dismissed id — owners reconcile their
                // own state via `take_dismissed`.
                self.dismissed.extend(self.entries.iter().map(|e| e.id));
                self.entries.clear();
                self.capture = None;
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
fn place(anchor: &OverlayAnchor, desired: Vec2, viewport: Rect) -> Rect {
    // A popup can never be larger than the viewport.
    let size = Vec2::new(
        desired.x.clamp(0.0, viewport.width().max(0.0)),
        desired.y.clamp(0.0, viewport.height().max(0.0)),
    );

    let mut origin = match anchor {
        OverlayAnchor::Bounds(a) => {
            let below_y = a.max_y() + ANCHOR_GAP;
            let above_y = a.min_y() - size.y - ANCHOR_GAP;
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
        OverlayAnchor::Pointer(p) => Vec2::new(p.x + POINTER_OFFSET, p.y + POINTER_OFFSET),
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
        layer.paint(&mut list);
        assert_eq!(list.commands.len(), 2);
        // Bottom entry paints first so the topmost lands above it.
        assert!(matches!(
            list.commands[0],
            PaintCommand::FillRect(_, [1, 0, 0, 255])
        ));
        assert!(matches!(
            list.commands[1],
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
        };
        assert_eq!(layer.dispatch_event(&outside), EventResponse::Ignored);
        assert!(!layer.is_open(id));
    }
}
