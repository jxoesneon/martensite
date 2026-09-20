//! Event routing pipeline: normalized pointer events, pointer capture, hover
//! tracking, and winit event conversion.
//!
//! The [`EventRouter`] is the central object that decides *which widget*
//! receives a given input event. It combines three concerns:
//!
//! - **Pointer capture** ([`PointerCapture`]): when a widget grabs the pointer
//!   (e.g. during a drag), every subsequent pointer event is routed to that
//!   widget regardless of where the pointer actually is on screen.
//! - **Hover/mouse tracking** ([`MouseTracker`]): per-window last-known pointer
//!   position and the widget currently hovered, used for scroll routing and
//!   hover highlighting.
//! - **Hit-testing** (delegated to [`HitTester`]): when no capture is active,
//!   the widget beneath the pointer is resolved with the two-stage hit-tester
//!   from the [`crate::hit_test`] module.
//!
//! # Winit adaptation note
//!
//! The winit version pinned by the workspace (`0.31.0-beta.3`) unified mouse,
//! touch, and tablet input under the `WindowEvent::PointerMoved` /
//! `WindowEvent::PointerButton` variants. The older `CursorMoved`, `MouseInput`,
//! `Touch`, and `TouchpadPressure` variants no longer exist. Accordingly,
//! [`convert_window_event`] handles `PointerMoved` and `PointerButton` — which
//! cover mouse, touch (via [`PointerSource::Touch`]), and tablet sources — and
//! normalizes them into a single [`PointerEvent`].
//!
//! [`PointerSource::Touch`]: winit::event::PointerSource::Touch
//! [`HitTester`]: crate::hit_test::HitTester

use std::collections::HashMap;
use std::time::{Duration, Instant};

use glam::Vec2;
use martensite_core::{EventResponse, PointerButton, WidgetArena, WidgetEvent, WidgetId};

use crate::dpi::DpiScale;
use crate::hit_test::HitTester;
use crate::WindowEvent;
use crate::WindowId;

bitflags::bitflags! {
    /// Bitfield tracking which keyboard modifier keys are currently active.
    ///
    /// This is a compact `u8` bitfield with one bit per logical modifier group
    /// (`Shift`, `Control`, `Alt`, and `Command`/`Super`/`Meta`). It is
    /// intentionally coarser than winit's per-side [`ModifiersKeys`] — event
    /// handlers rarely need to distinguish left vs. right shift, and the
    /// combined form keeps the normalized [`PointerEvent`] small and cheap to
    /// copy.
    ///
    /// [`ModifiersKeys`]: winit::keyboard::ModifiersKeys
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite_window::event::ModifierKeys;
    ///
    /// let mut mods = ModifierKeys::SHIFT | ModifierKeys::CONTROL;
    /// assert!(mods.contains(ModifierKeys::SHIFT));
    /// assert!(mods.contains(ModifierKeys::CONTROL));
    /// assert!(!mods.contains(ModifierKeys::ALT));
    /// mods.remove(ModifierKeys::SHIFT);
    /// assert!(!mods.contains(ModifierKeys::SHIFT));
    /// ```
    #[derive(Copy, Clone, Debug, Default, PartialEq, Eq, Hash)]
    pub struct ModifierKeys: u8 {
        /// The `Shift` key (either side).
        const SHIFT = 1 << 0;
        /// The `Control` key (either side).
        const CONTROL = 1 << 1;
        /// The `Alt` / `Option` key (either side).
        const ALT = 1 << 2;
        /// The `Command` (macOS) / `Super` / `Meta` / `Windows` key (either side).
        const COMMAND = 1 << 3;
    }
}

/// Identifier for an individual pointer (mouse or touch finger).
///
/// For mouse input a single primary pointer ([`PointerId::PRIMARY`]) is used.
/// For multi-touch, each finger is assigned a distinct id derived from winit's
/// [`FingerId`], so concurrent touches can be tracked independently.
///
/// The id is 64 bits wide because winit derives [`FingerId`] from a `usize`;
/// on iOS it is the address of the `UITouch` object, which does not fit in
/// 32 bits. Converted finger ids are offset by one from the raw winit value
/// so a `FingerId` of `0` cannot alias [`PointerId::PRIMARY`].
///
/// [`FingerId`]: winit::event::FingerId
///
/// # Examples
///
/// ```
/// use martensite_window::event::PointerId;
///
/// let primary = PointerId::PRIMARY;
/// let finger = PointerId::new(3);
/// assert_ne!(primary, finger);
/// assert_eq!(finger.get(), 3);
/// ```
#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash, PartialOrd, Ord, Default)]
pub struct PointerId(u64);

impl PointerId {
    /// The primary pointer id, used for mouse input and any single-pointer
    /// source that does not carry a distinct identifier.
    pub const PRIMARY: Self = Self::new(0);

    /// Creates a new pointer id from a raw `u64`.
    pub const fn new(id: u64) -> Self {
        Self(id)
    }

    /// Returns the raw `u64` value of this pointer id.
    pub const fn get(self) -> u64 {
        self.0
    }
}

/// The normalized input-source kind of a [`PointerEvent`].
///
/// This is the Martensite-side analogue of winit's
/// [`PointerKind`](winit::event::PointerKind), reduced to the kind alone —
/// the per-finger identity of a touch is carried separately by
/// [`PointerEvent::pointer_id`].
///
/// The kind and pointer id are available to consumers at the
/// window/event-routing layer (e.g. to drive pointer capture or to gate
/// hover-only affordances on touch-driven platforms such as iOS and
/// Android). They are intentionally *not* forwarded into the widget-level
/// [`WidgetEvent`] vocabulary produced by [`widget_event_for_pointer`] —
/// `WidgetEvent` lives in `martensite-core`, which cannot name
/// window-layer types.
///
/// # Examples
///
/// ```
/// use martensite_window::event::PointerKind;
///
/// assert_eq!(PointerKind::Mouse, PointerKind::Mouse);
/// assert_ne!(PointerKind::Touch, PointerKind::Mouse);
/// ```
#[derive(Copy, Clone, Debug, Default, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum PointerKind {
    /// A conventional mouse pointer.
    #[default]
    Mouse,
    /// A touchscreen contact (one per finger; the identity is in
    /// [`PointerEvent::pointer_id`]).
    Touch,
    /// A stylus/tablet tool (e.g. Apple Pencil on iOS).
    Tablet,
    /// An input source winit could not classify.
    Unknown,
}

/// The state of a pointer device in a [`PointerEvent`].
///
/// `Moved` events carry no button information (`button` is `None`); `Pressed`
/// and `Released` events carry the button whose state changed.
#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash)]
pub enum PointerState {
    /// A pointer button was pressed.
    Pressed,
    /// A pointer button was released.
    Released,
    /// The pointer moved without a button state change.
    Moved,
}

/// A normalized mouse button, independent of winit.
///
/// This mirrors the common subset of [`winit::event::MouseButton`] while
/// collapsing the extended numbered buttons (`Button6`..`Button32`) into a
/// single [`MouseButton::Other`] variant carrying the 1-based button index.
#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash)]
pub enum MouseButton {
    /// The primary (usually left) button.
    Left,
    /// The secondary (usually right) button.
    Right,
    /// The tertiary (usually middle) button.
    Middle,
    /// The first side button, frequently assigned a back function.
    Back,
    /// The second side button, frequently assigned a forward function.
    Forward,
    /// Any further numbered button, carrying its 1-based index.
    Other(u8),
}

/// A normalized pointer event, independent of winit.
///
/// All positions are in **logical** coordinates (already divided by the
/// window's DPI scale factor). The event is `Copy` so it can be passed around
/// cheaply during dispatch.
///
/// # Examples
///
/// ```
/// use martensite_window::event::{ModifierKeys, MouseButton, PointerEvent, PointerId, PointerKind, PointerState};
/// use glam::Vec2;
///
/// let event = PointerEvent {
///     pointer_id: PointerId::PRIMARY,
///     kind: PointerKind::Mouse,
///     position: Vec2::new(42.0, 17.0),
///     state: PointerState::Pressed,
///     button: Some(MouseButton::Left),
///     modifiers: ModifierKeys::SHIFT,
/// };
/// assert_eq!(event.position, Vec2::new(42.0, 17.0));
/// assert_eq!(event.state, PointerState::Pressed);
/// ```
#[derive(Copy, Clone, Debug, PartialEq)]
pub struct PointerEvent {
    /// The pointer (mouse or touch finger) this event belongs to.
    pub pointer_id: PointerId,
    /// The input-source kind (mouse, touch, tablet) that produced the event.
    ///
    /// For [`PointerKind::Touch`], `pointer_id` identifies the individual
    /// finger; for other kinds it is typically [`PointerId::PRIMARY`].
    ///
    /// The kind is consumed by window-layer routing; it is dropped when a
    /// `PointerEvent` is converted into a `WidgetEvent` for widget
    /// delivery.
    pub kind: PointerKind,
    /// The pointer position in logical coordinates.
    pub position: Vec2,
    /// Whether a button was pressed, released, or the pointer merely moved.
    pub state: PointerState,
    /// The button whose state changed, if any. `None` for `Moved` events.
    pub button: Option<MouseButton>,
    /// The keyboard modifier keys active at the time of the event.
    pub modifiers: ModifierKeys,
}

/// The press-to-press interval within which consecutive presses of the
/// same button on the same pointer continue a multi-click streak.
/// 500 ms matches the platform defaults (macOS `doubleClickInterval`,
/// Windows `SM_GETDOUBLECLICKTIME`).
const CLICK_STREAK_INTERVAL: Duration = Duration::from_millis(500);

/// The maximum distance, in logical px, a press may drift from the
/// previous press and still continue the streak — the same magnitude
/// as `SM_CXDOUBLECLK` on Windows.
const CLICK_STREAK_SLOP: f32 = 4.0;

/// Tracks multi-click streaks per pointer so [`WidgetEvent::PointerPressed`]
/// can carry a `count` — the framework's equivalent of macOS
/// `NSEvent.clickCount`, GTK `n_press`, or the web's `MouseEvent.detail`.
///
/// A press continues the streak when it shares the previous press's
/// pointer and button, lands within [`CLICK_STREAK_INTERVAL`] of it,
/// and stays inside [`CLICK_STREAK_SLOP`]. Anything else restarts the
/// streak at `1`. The streak intentionally ignores which widget was
/// hit — platform click counts behave the same way.
#[derive(Debug, Default)]
struct ClickTracker {
    /// The most recent press in the streak, if any.
    last: Option<ClickStreak>,
}

/// One registered press in a streak.
#[derive(Debug)]
struct ClickStreak {
    /// The pointer that pressed.
    pointer_id: PointerId,
    /// The button that pressed.
    button: Option<MouseButton>,
    /// Logical-space position of the press.
    position: Vec2,
    /// When the press was registered.
    at: Instant,
    /// The streak count that press was assigned.
    count: u8,
}

impl ClickTracker {
    /// Registers `event` and returns its streak count (`1` for a plain
    /// click). Non-press events never continue a streak and return `1`.
    fn register(&mut self, event: &PointerEvent) -> u8 {
        self.register_at(event, Instant::now())
    }

    /// [`register`](Self::register) with an injectable timestamp so
    /// tests can drive the streak without sleeping.
    fn register_at(&mut self, event: &PointerEvent, at: Instant) -> u8 {
        if event.state != PointerState::Pressed {
            return 1;
        }
        let count = self
            .last
            .as_ref()
            .filter(|l| {
                l.pointer_id == event.pointer_id
                    && l.button == event.button
                    && at.duration_since(l.at) <= CLICK_STREAK_INTERVAL
                    && l.position.distance(event.position) <= CLICK_STREAK_SLOP
            })
            .map_or(1, |l| l.count.saturating_add(1));
        self.last = Some(ClickStreak {
            pointer_id: event.pointer_id,
            button: event.button,
            position: event.position,
            at,
            count,
        });
        count
    }
}

/// Tracks which widgets have captured pointer input, per pointer.
///
/// While a capture is active for a given pointer, [`EventRouter`] routes *all*
/// events for that pointer to the captured widget regardless of the pointer's
/// on-screen position. This is the mechanism behind drag operations, slider
/// grabs, and text selection that follows the pointer outside the originating
/// widget.
///
/// Each pointer ([`PointerId`]) is captured independently, so multi-touch
/// interactions can capture different fingers to different widgets at the same
/// time.
///
/// # Examples
///
/// ```
/// use martensite_core::WidgetId;
/// use martensite_window::event::{PointerCapture, PointerId};
///
/// let mut cap = PointerCapture::new();
/// assert!(cap.captured_primary().is_none());
///
/// // `WidgetId::from_parts` is used here only to fabricate a handle for the
/// // example; real code obtains ids from the arena.
/// let widget = WidgetId::from_parts(1, 1);
/// cap.capture_primary(widget);
/// assert_eq!(cap.captured_primary(), Some(widget));
///
/// cap.release_primary();
/// assert!(cap.captured_primary().is_none());
/// ```
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct PointerCapture {
    /// The widget currently holding the capture for each pointer, keyed by
    /// pointer id.
    targets: HashMap<PointerId, WidgetId>,
}

impl PointerCapture {
    /// Creates a new, empty pointer capture (no pointers captured).
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Captures all subsequent events for `pointer_id` to `widget_id`.
    ///
    /// Once captured, every event for that pointer is routed to `widget_id`
    /// until [`PointerCapture::release`] is called for the same pointer.
    pub fn capture(&mut self, pointer_id: PointerId, widget_id: WidgetId) {
        self.targets.insert(pointer_id, widget_id);
    }

    /// Captures all subsequent events for the primary pointer
    /// ([`PointerId::PRIMARY`]) to `widget_id`.
    ///
    /// Convenience delegate for [`PointerCapture::capture`].
    pub fn capture_primary(&mut self, widget_id: WidgetId) {
        self.capture(PointerId::PRIMARY, widget_id);
    }

    /// Releases the capture for `pointer_id`, if any.
    pub fn release(&mut self, pointer_id: PointerId) {
        self.targets.remove(&pointer_id);
    }

    /// Releases the capture for the primary pointer, if any.
    ///
    /// Convenience delegate for [`PointerCapture::release`].
    pub fn release_primary(&mut self) {
        self.release(PointerId::PRIMARY);
    }

    /// Returns the widget currently holding the capture for `pointer_id`, or
    /// `None` if that pointer is not captured.
    #[must_use]
    pub fn captured(&self, pointer_id: PointerId) -> Option<WidgetId> {
        self.targets.get(&pointer_id).copied()
    }

    /// Returns the widget currently holding the capture for the primary
    /// pointer, or `None`.
    ///
    /// Convenience delegate for [`PointerCapture::captured`].
    #[must_use]
    pub fn captured_primary(&self) -> Option<WidgetId> {
        self.captured(PointerId::PRIMARY)
    }

    /// Releases all active captures for every pointer.
    pub fn clear(&mut self) {
        self.targets.clear();
    }

    /// Returns the number of pointers currently captured.
    #[must_use]
    pub fn active_captures(&self) -> usize {
        self.targets.len()
    }
}

/// Tracks mouse position and hover state per window.
///
/// The tracker is keyed by [`WindowId`] so that a single instance can serve a
/// multi-window application. It records the last-known logical pointer
/// position and the widget currently considered hovered for each window.
///
/// The tracker is *not* per-pointer: it models a single hover position per
/// window (the classic desktop mouse model). On multi-touch platforms the
/// last touch to move wins; per-finger tracking, if needed, must key off
/// [`PointerEvent::pointer_id`] in the event stream itself.
///
/// # Examples
///
/// ```
/// use martensite_core::WidgetId;
/// use martensite_window::event::MouseTracker;
/// use martensite_window::WindowId;
/// use glam::Vec2;
///
/// let mut tracker = MouseTracker::new();
/// let win = WindowId::from_raw(7);
/// tracker.update_position(win, Vec2::new(10.0, 20.0));
/// assert_eq!(tracker.position(win), Some(Vec2::new(10.0, 20.0)));
///
/// let widget = WidgetId::from_parts(1, 1);
/// tracker.set_hovered(win, Some(widget));
/// assert_eq!(tracker.hovered_widget(win), Some(widget));
///
/// tracker.set_hovered(win, None);
/// assert_eq!(tracker.hovered_widget(win), None);
/// ```
#[derive(Debug, Default)]
pub struct MouseTracker {
    /// Last-known logical pointer position per window.
    positions: HashMap<WindowId, Vec2>,
    /// Currently hovered widget per window.
    hovered: HashMap<WindowId, WidgetId>,
}

impl MouseTracker {
    /// Creates a new, empty mouse tracker.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Records the latest pointer `pos` (logical coordinates) for `window_id`.
    pub fn update_position(&mut self, window_id: WindowId, pos: Vec2) {
        self.positions.insert(window_id, pos);
    }

    /// Returns the last-known pointer position for `window_id`, or `None` if
    /// no position has been recorded for that window.
    #[must_use]
    pub fn position(&self, window_id: WindowId) -> Option<Vec2> {
        self.positions.get(&window_id).copied()
    }

    /// Returns the widget currently hovered in `window_id`, or `None`.
    #[must_use]
    pub fn hovered_widget(&self, window_id: WindowId) -> Option<WidgetId> {
        self.hovered.get(&window_id).copied()
    }

    /// Sets or clears the hovered widget for `window_id`. Passing `None`
    /// clears any previously recorded hover state for that window.
    pub fn set_hovered(&mut self, window_id: WindowId, widget_id: Option<WidgetId>) {
        match widget_id {
            Some(id) => {
                self.hovered.insert(window_id, id);
            }
            None => {
                self.hovered.remove(&window_id);
            }
        }
    }

    /// Forgets all tracked state for `window_id` (position and hover).
    ///
    /// Should be called when a window is destroyed to avoid retaining stale
    /// entries.
    pub fn clear_window(&mut self, window_id: WindowId) {
        self.positions.remove(&window_id);
        self.hovered.remove(&window_id);
    }

    /// Returns the number of windows for which a position is currently tracked.
    #[must_use]
    pub fn tracked_window_count(&self) -> usize {
        self.positions.len()
    }
}

/// The outcome of dispatching an event to a widget.
///
/// Callers can use this to decide whether to keep bubbling an event, mark it
/// as consumed, or treat it as ignored (no target found).
///
/// - [`EventDispatchOutcome::Handled`] — a target widget was resolved and the
///   event was routed to it.
/// - [`EventDispatchOutcome::Unhandled`] — no target was found (e.g. the
///   pointer missed every widget, or no widget is hovered for a scroll).
/// - [`EventDispatchOutcome::Ignored`] — the event had no applicable target
///   (e.g. a keyboard event with no focused widget).
///
/// # Examples
///
/// ```
/// use martensite_core::WidgetId;
/// use martensite_window::event::EventDispatchOutcome;
///
/// let widget = WidgetId::from_parts(1, 1);
/// assert_eq!(EventDispatchOutcome::Handled(widget), EventDispatchOutcome::Handled(widget));
/// assert_ne!(EventDispatchOutcome::Handled(widget), EventDispatchOutcome::Unhandled);
/// assert_ne!(EventDispatchOutcome::Unhandled, EventDispatchOutcome::Ignored);
/// ```
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum EventDispatchOutcome {
    /// The event was handled by the given widget.
    Handled(WidgetId),
    /// No target widget was found for the event.
    Unhandled,
    /// The event had no applicable target and was ignored entirely.
    Ignored,
}

/// The main event routing pipeline.
///
/// An [`EventRouter`] owns a [`PointerCapture`] and a [`MouseTracker`] and
/// exposes the three routing entry points required by the v0.5.0 milestone:
///
/// - [`EventRouter::route_pointer_event`] — routes a normalized [`PointerEvent`]
///   to the captured widget (if any) or to the hit-tested widget under the
///   pointer.
/// - [`EventRouter::route_keyboard_event`] — routes to the currently focused
///   widget.
/// - [`EventRouter::route_scroll_event`] — routes a scroll to the widget under
///   the scroll position (the hovered widget for that window's tree).
///
/// The `dispatch_*` delivery methods additionally integrate two
/// framework concerns:
///
/// - **Overlay-first input**: pointer and scroll events are offered to
///   the arena-owned
///   [`OverlayLayer`](martensite_core::overlay::OverlayLayer) before
///   arena hit-testing; an `Escape` key press is consumed by the layer
///   to dismiss the topmost popup. All other keys pass through to the
///   focused widget.
/// - **Focus requests**: `EventResponse::CaptureFocus` responses (and
///   implicit press-to-focus on `FOCUSABLE` nodes) are drained into
///   [`EventRouter::take_focus_request`] for the app's `FocusManager`
///   to apply.
///
/// # Examples
///
/// ```
/// use martensite_core::{ColdNode, HotNode, NodeFlags, Rect, WidgetArena};
/// use martensite_window::event::{
///     EventDispatchOutcome, EventRouter, ModifierKeys, MouseButton, PointerEvent, PointerId,
///     PointerKind, PointerState,
/// };
/// use martensite_window::WindowId;
/// use glam::Vec2;
///
/// let mut arena = WidgetArena::new();
/// let root = arena.insert(
///     HotNode {
///         bounds: Rect::new(0.0, 0.0, 100.0, 100.0),
///         flags: NodeFlags::VISIBLE | NodeFlags::HIT_TEST_ENABLED,
///         ..HotNode::default()
///     },
///     ColdNode::default(),
/// );
///
/// let mut router = EventRouter::new();
/// let win = WindowId::from_raw(1);
/// let event = PointerEvent {
///     pointer_id: PointerId::PRIMARY,
///     kind: PointerKind::Mouse,
///     position: Vec2::new(50.0, 50.0),
///     state: PointerState::Moved,
///     button: None,
///     modifiers: ModifierKeys::empty(),
/// };
/// assert_eq!(
///     router.route_pointer_event(&arena, root, win, &event),
///     EventDispatchOutcome::Handled(root)
/// );
/// ```
#[derive(Debug, Default)]
pub struct EventRouter {
    /// The active pointer capture, if any.
    capture: PointerCapture,
    /// Per-window mouse position and hover state.
    mouse: MouseTracker,
    /// Most recent widget that asked for keyboard focus during
    /// dispatch — drained by
    /// [`take_focus_request`](Self::take_focus_request).
    pending_focus: Option<WidgetId>,
    /// Multi-click streak state feeding `PointerPressed::count`.
    clicks: ClickTracker,
}

impl EventRouter {
    /// Creates a new, empty event router with no capture and no tracked
    /// mouse state.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Returns a shared reference to the underlying [`PointerCapture`].
    #[must_use]
    pub fn pointer_capture(&self) -> &PointerCapture {
        &self.capture
    }

    /// Returns a mutable reference to the underlying [`PointerCapture`].
    pub fn pointer_capture_mut(&mut self) -> &mut PointerCapture {
        &mut self.capture
    }

    /// Returns a shared reference to the underlying [`MouseTracker`].
    #[must_use]
    pub fn mouse_tracker(&self) -> &MouseTracker {
        &self.mouse
    }

    /// Returns a mutable reference to the underlying [`MouseTracker`].
    pub fn mouse_tracker_mut(&mut self) -> &mut MouseTracker {
        &mut self.mouse
    }

    /// Captures all subsequent events for `pointer_id` to `widget_id`.
    ///
    /// Convenience delegate for [`PointerCapture::capture`].
    pub fn capture_pointer(&mut self, pointer_id: PointerId, widget_id: WidgetId) {
        self.capture.capture(pointer_id, widget_id);
    }

    /// Captures all subsequent events for the primary pointer to `widget_id`.
    ///
    /// Convenience delegate for [`PointerCapture::capture_primary`].
    pub fn capture_pointer_primary(&mut self, widget_id: WidgetId) {
        self.capture.capture_primary(widget_id);
    }

    /// Releases the capture for `pointer_id`, if any.
    ///
    /// Convenience delegate for [`PointerCapture::release`].
    pub fn release_pointer(&mut self, pointer_id: PointerId) {
        self.capture.release(pointer_id);
    }

    /// Releases the capture for the primary pointer, if any.
    ///
    /// Convenience delegate for [`PointerCapture::release_primary`].
    pub fn release_pointer_primary(&mut self) {
        self.capture.release_primary();
    }

    /// Returns the widget currently holding the capture for `pointer_id`, or
    /// `None`.
    ///
    /// Convenience delegate for [`PointerCapture::captured`].
    #[must_use]
    pub fn captured_widget(&self, pointer_id: PointerId) -> Option<WidgetId> {
        self.capture.captured(pointer_id)
    }

    /// Returns the widget currently holding the capture for the primary
    /// pointer, or `None`.
    ///
    /// Convenience delegate for [`PointerCapture::captured_primary`].
    #[must_use]
    pub fn captured_widget_primary(&self) -> Option<WidgetId> {
        self.capture.captured_primary()
    }

    /// Drains the most recent keyboard-focus request recorded while
    /// dispatching events.
    ///
    /// Widgets ask for focus by answering an event with
    /// [`EventResponse::CaptureFocus`], or implicitly by handling a
    /// `PointerPressed` on a node carrying `NodeFlags::FOCUSABLE`
    /// (press-to-focus). The arena records the request; the router
    /// surfaces it here so the app can apply it through
    /// `martensite-focus`'s `FocusManager::set_focus`, then dispatch
    /// `WidgetEvent::FocusLost`/`FocusGained` to the old and new
    /// targets.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite_window::event::EventRouter;
    ///
    /// let mut router = EventRouter::new();
    /// assert_eq!(router.take_focus_request(), None);
    /// ```
    pub fn take_focus_request(&mut self) -> Option<WidgetId> {
        self.pending_focus.take()
    }

    /// Moves any pending focus request off the arena into the router.
    fn drain_focus(&mut self, arena: &mut WidgetArena) {
        if let Some(id) = arena.take_focus_request() {
            self.pending_focus = Some(id);
        }
    }

    /// Routes a normalized pointer event to its target widget.
    ///
    /// Routing priority:
    ///
    /// 1. If a pointer capture is active for `event.pointer_id` and the
    ///    captured widget is still alive in `arena`, the event is routed to
    ///    that widget regardless of `event.position`. (A capture whose target
    ///    has since been removed from the arena is automatically released.)
    /// 2. Otherwise the event is routed to the front-most widget beneath
    ///    `event.position` via the two-stage [`HitTester`].
    ///
    /// The per-window [`MouseTracker`] is kept in sync: the pointer position is
    /// updated for `window_id`, and the resolved target (captured or
    /// hit-tested) is recorded as the hovered widget so subsequent scroll
    /// events route correctly.
    ///
    /// Returns [`EventDispatchOutcome::Handled`] when a target was found, or
    /// [`EventDispatchOutcome::Unhandled`] when the pointer missed every
    /// widget (or `root` is dead).
    pub fn route_pointer_event(
        &mut self,
        arena: &WidgetArena,
        root: WidgetId,
        window_id: WindowId,
        event: &PointerEvent,
    ) -> EventDispatchOutcome {
        // Keep the per-window mouse position in sync so callers that read the
        // tracker after routing see the latest position.
        self.mouse.update_position(window_id, event.position);

        let target = if let Some(captured) = self.capture.captured(event.pointer_id) {
            // A captured widget that became underflow-covered mid-gesture
            // (engaged Hide/Collapse/Scrim) can no longer be the gesture's
            // referent — release the capture and resume hit-testing,
            // same as a dead capture.
            let covered = arena
                .get_cold(captured)
                .and_then(|c| c.underflow_policy())
                .is_some_and(|p| p.covers_input());
            if arena.is_alive(captured) && !covered {
                Some(captured)
            } else {
                // The captured widget is gone or covered — release the
                // stale capture so subsequent events resume hit-testing.
                self.capture.release(event.pointer_id);
                let tester = HitTester::new(arena);
                tester.hit_test(root, event.position).map(|r| r.widget_id)
            }
        } else {
            let tester = HitTester::new(arena);
            tester.hit_test(root, event.position).map(|r| r.widget_id)
        };

        // Record the resolved target as the hovered widget for this window so
        // scroll routing can reuse it without re-hit-testing.
        self.mouse.set_hovered(window_id, target);

        match target {
            Some(id) => EventDispatchOutcome::Handled(id),
            None => EventDispatchOutcome::Unhandled,
        }
    }

    /// Routes a keyboard event to the currently focused widget.
    ///
    /// The router itself does not track focus (that is the responsibility of
    /// the focus crate); this method simply wraps `focused` so the caller can
    /// chain dispatch uniformly.
    ///
    /// Returns [`EventDispatchOutcome::Handled`] when a widget is focused, or
    /// [`EventDispatchOutcome::Ignored`] when no widget is focused.
    pub fn route_keyboard_event(&mut self, focused: Option<WidgetId>) -> EventDispatchOutcome {
        match focused {
            Some(id) => EventDispatchOutcome::Handled(id),
            None => EventDispatchOutcome::Ignored,
        }
    }

    /// Routes a scroll event to the widget currently hovered in `window_id`.
    ///
    /// The widget under the scroll position is, by definition, the hovered
    /// widget for that window's tree — which [`MouseTracker`] keeps up to date
    /// as pointer events are routed through [`route_pointer_event`]. This
    /// method therefore reads the tracked hover state directly instead of
    /// re-hit-testing. The `delta` is accepted for API completeness and future
    /// dispatch logic but does not affect target selection.
    ///
    /// Returns [`EventDispatchOutcome::Handled`] when a hovered widget exists,
    /// or [`EventDispatchOutcome::Unhandled`] when no widget is hovered (e.g.
    /// the pointer is outside every widget or no pointer event has been routed
    /// for this window yet).
    ///
    /// [`route_pointer_event`]: EventRouter::route_pointer_event
    pub fn route_scroll_event(&mut self, window_id: WindowId, delta: Vec2) -> EventDispatchOutcome {
        // `delta` is part of the routing contract but does not change *which*
        // widget receives the scroll; acknowledge it to avoid an unused
        // variable warning.
        let _ = delta;
        match self.mouse.hovered_widget(window_id) {
            Some(id) => EventDispatchOutcome::Handled(id),
            None => EventDispatchOutcome::Unhandled,
        }
    }

    /// Routes a pointer event *and* delivers it to the resolved widget.
    ///
    /// This is the production event-delivery path: the target is resolved
    /// exactly as in [`route_pointer_event`](Self::route_pointer_event)
    /// (pointer capture first, then hit-testing), then the event is
    /// delivered through [`WidgetArena::dispatch_event`], which invokes
    /// `Widget::event` on the target and bubbles to ancestors on
    /// [`EventResponse::Ignored`].
    ///
    /// Returns the widget's [`EventResponse`], or `None` when no widget
    /// was hit. `EventResponse::RequestRepaint` already marks the
    /// responding node `DIRTY_PAINT` inside `dispatch_event`.
    ///
    /// [`EventResponse::CapturePointer`] from the *responding* widget
    /// (which may be an ancestor of the hit target after bubbling)
    /// captures `event.pointer_id` to that widget;
    /// [`EventResponse::ReleasePointer`] releases the capture for that
    /// pointer.
    ///
    /// # Examples
    ///
    /// ```
    /// use glam::Vec2;
    /// use martensite_core::{DummyWidget, HotNode, NodeFlags, WidgetArena};
    /// use martensite_window::event::{
    ///     EventRouter, ModifierKeys, MouseButton, PointerEvent, PointerId, PointerKind,
    ///     PointerState,
    /// };
    /// use martensite_window::WindowId;
    ///
    /// let mut arena = WidgetArena::new();
    /// let mut hot = HotNode::default();
    /// hot.flags = NodeFlags::VISIBLE | NodeFlags::HIT_TEST_ENABLED;
    /// hot.bounds = martensite_core::Rect::new(0.0, 0.0, 100.0, 100.0);
    /// let root = arena.insert_with_widget(hot, Box::new(DummyWidget));
    ///
    /// let mut router = EventRouter::new();
    /// let event = PointerEvent {
    ///     pointer_id: PointerId::PRIMARY,
    ///     kind: PointerKind::Mouse,
    ///     position: Vec2::new(10.0, 10.0),
    ///     state: PointerState::Pressed,
    ///     button: Some(MouseButton::Left),
    ///     modifiers: ModifierKeys::empty(),
    /// };
    /// // `DummyWidget` ignores input — dispatch still resolves.
    /// assert_eq!(
    ///     router.dispatch_pointer_event(&mut arena, root, WindowId::from_raw(1), &event),
    ///     Some(martensite_core::EventResponse::Ignored),
    /// );
    /// ```
    pub fn dispatch_pointer_event(
        &mut self,
        arena: &mut WidgetArena,
        root: WidgetId,
        window_id: WindowId,
        event: &PointerEvent,
    ) -> Option<EventResponse> {
        // Register the press with the click tracker before any dispatch
        // path so popups and window content see the same streak count.
        let click_count = self.clicks.register(event);
        // Popups in the arena-owned overlay hit-test ahead of window
        // content — unless an arena widget holds pointer capture: a
        // drag that began on window content must keep tracking even
        // while the pointer crosses a popup.
        let arena_captured = self
            .capture
            .captured(event.pointer_id)
            .is_some_and(|id| arena.is_alive(id));
        if !arena_captured {
            let overlay_response = arena
                .overlay_mut()
                .dispatch_event(&widget_event_for_pointer(event, click_count));
            if overlay_response != EventResponse::Ignored {
                // The popup consumed the event. Keep the tracked pointer
                // position fresh but leave hover untouched — the widget
                // beneath the popup stays "hovered" (e.g. a dropdown
                // trigger while browsing its options), so no spurious
                // `PointerLeave` fires.
                self.mouse.update_position(window_id, event.position);
                self.drain_focus(arena);
                return Some(overlay_response);
            }
            // `Ignored` means the press landed outside every popup
            // (dismissing them) or no popup is open — fall through to
            // normal routing.
        }

        // Capture the hovered widget before routing so hover transitions
        // can be detected. `PointerMoved` stops arriving once the pointer
        // exits a widget's bounds, so `PointerEnter`/`PointerLeave` are
        // dispatched explicitly at the boundary.
        let prev_hovered = self.mouse.hovered_widget(window_id);
        let outcome = self.route_pointer_event(arena, root, window_id, event);
        let now_hovered = self.mouse.hovered_widget(window_id);
        if event.state == PointerState::Moved && prev_hovered != now_hovered {
            if let Some(old) = prev_hovered {
                if arena.is_alive(old) {
                    let _ = arena.dispatch_event(old, &WidgetEvent::PointerLeave);
                }
            }
            if let Some(new) = now_hovered {
                let _ = arena.dispatch_event(new, &WidgetEvent::PointerEnter);
            }
        }
        let result = match outcome {
            EventDispatchOutcome::Handled(id) => {
                match arena.dispatch_event_ex(id, &widget_event_for_pointer(event, click_count)) {
                    Some((responder, response)) => {
                        match response {
                            EventResponse::CapturePointer => {
                                self.capture_pointer(event.pointer_id, responder);
                            }
                            EventResponse::ReleasePointer => {
                                self.release_pointer(event.pointer_id);
                            }
                            _ => {}
                        }
                        Some(response)
                    }
                    // The event bubbled past the root: it was delivered
                    // but ignored, which is `Some(Ignored)` — not `None`
                    // (that would mean no widget was hit at all).
                    None => Some(EventResponse::Ignored),
                }
            }
            _ => None,
        };
        self.drain_focus(arena);
        result
    }

    /// Routes a keyboard event to the focused widget and delivers it.
    ///
    /// Wraps [`route_keyboard_event`](Self::route_keyboard_event): when a
    /// widget is focused, `key` is delivered through
    /// [`WidgetArena::dispatch_event`] as a
    /// [`WidgetEvent::KeyPressed`]/[`WidgetEvent::KeyReleased`]. Returns
    /// `None` when no widget is focused.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite_core::{DummyWidget, HotNode, WidgetArena};
    /// use martensite_window::event::EventRouter;
    ///
    /// let mut arena = WidgetArena::new();
    /// let root = arena.insert_with_widget(HotNode::default(), Box::new(DummyWidget));
    /// let mut router = EventRouter::new();
    /// // No focus → no delivery.
    /// assert!(router
    ///     .dispatch_keyboard_event(&mut arena, None, "Enter", true, false)
    ///     .is_none());
    /// ```
    pub fn dispatch_keyboard_event(
        &mut self,
        arena: &mut WidgetArena,
        focused: Option<WidgetId>,
        key: &str,
        pressed: bool,
        repeat: bool,
    ) -> Option<EventResponse> {
        let event = if pressed {
            WidgetEvent::KeyPressed {
                key: key.to_string(),
                repeat,
            }
        } else {
            WidgetEvent::KeyReleased {
                key: key.to_string(),
            }
        };
        // `Escape` dismisses the topmost open popup; the overlay offers
        // other keys to the popup's content first and falls through on
        // `Ignored`, so focused-widget interactions such as combobox
        // typeahead keep working while a popup is open.
        let overlay_response = arena.overlay_mut().dispatch_event(&event);
        if overlay_response != EventResponse::Ignored {
            self.drain_focus(arena);
            return Some(overlay_response);
        }
        let result = match self.route_keyboard_event(focused) {
            EventDispatchOutcome::Handled(id) => Some(arena.dispatch_event(id, &event)),
            _ => None,
        };
        self.drain_focus(arena);
        result
    }

    /// Routes a scroll event to the hovered widget and delivers it.
    ///
    /// Wraps [`route_scroll_event`](Self::route_scroll_event); the hover
    /// position tracked for `window_id` supplies the event position.
    /// Returns `None` when no widget is hovered.
    pub fn dispatch_scroll_event(
        &mut self,
        arena: &mut WidgetArena,
        window_id: WindowId,
        delta: Vec2,
    ) -> Option<EventResponse> {
        let position = self.mouse.position(window_id).unwrap_or(Vec2::ZERO);
        // Scroll over a popup goes to the popup (e.g. a long listbox).
        let overlay_response = arena
            .overlay_mut()
            .dispatch_event(&WidgetEvent::Scroll { position, delta });
        if overlay_response != EventResponse::Ignored {
            self.drain_focus(arena);
            return Some(overlay_response);
        }
        let result = match self.route_scroll_event(window_id, delta) {
            EventDispatchOutcome::Handled(id) => {
                Some(arena.dispatch_event(id, &WidgetEvent::Scroll { position, delta }))
            }
            _ => None,
        };
        self.drain_focus(arena);
        result
    }

    /// Routes an IME event to the focused widget and delivers it.
    ///
    /// `ImeEvent::Committed` becomes [`WidgetEvent::ImeCommitted`];
    /// `ImeEvent::Preedit` becomes [`WidgetEvent::ImePreedit`]. Follows
    /// the same focused-widget routing as
    /// [`dispatch_keyboard_event`](Self::dispatch_keyboard_event).
    /// Returns `None` when no widget is focused.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite_core::{DummyWidget, HotNode, WidgetArena};
    /// use martensite_window::event::{EventRouter, ImeEvent};
    ///
    /// let mut arena = WidgetArena::new();
    /// let root = arena.insert_with_widget(HotNode::default(), Box::new(DummyWidget));
    /// let mut router = EventRouter::new();
    /// // No focus → no delivery.
    /// assert!(router
    ///     .dispatch_ime_event(&mut arena, None, &ImeEvent::Committed("a".into()))
    ///     .is_none());
    /// ```
    pub fn dispatch_ime_event(
        &mut self,
        arena: &mut WidgetArena,
        focused: Option<WidgetId>,
        event: &ImeEvent,
    ) -> Option<EventResponse> {
        let widget_event = match event {
            ImeEvent::Committed(text) => WidgetEvent::ImeCommitted { text: text.clone() },
            ImeEvent::Preedit { text, cursor } => WidgetEvent::ImePreedit {
                text: text.clone(),
                cursor: *cursor,
            },
        };
        let result = match self.route_keyboard_event(focused) {
            EventDispatchOutcome::Handled(id) => Some(arena.dispatch_event(id, &widget_event)),
            _ => None,
        };
        self.drain_focus(arena);
        result
    }
}

/// A normalized IME event destined for the focused widget.
///
/// This is the widget-delivery subset of winit's
/// [`Ime`](winit::event::Ime): `Enabled`/`Disabled` are lifecycle
/// notifications for the host (keyboard visibility, capability tracking)
/// rather than widget events, so they are not represented here. A
/// cancelled or ended composition arrives as
/// [`Preedit`](Self::Preedit) with empty `text`.
///
/// # Examples
///
/// ```
/// use martensite_window::event::ImeEvent;
///
/// let e = ImeEvent::Preedit {
///     text: "かな".to_string(),
///     cursor: Some((0, 3)),
/// };
/// assert!(matches!(e, ImeEvent::Preedit { .. }));
/// ```
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ImeEvent {
    /// The composition committed `text` to the widget.
    Committed(String),
    /// The composition string changed (or cleared when `text` is empty).
    Preedit {
        /// The in-progress composition.
        text: String,
        /// Byte-range of the caret within `text`, if shown.
        cursor: Option<(usize, usize)>,
    },
}

/// Maps a winit [`Ime`](winit::event::Ime) event to an [`ImeEvent`].
///
/// `Enabled`/`Disabled` return `None` — they are host lifecycle
/// notifications, not widget events. `Preedit` carries the composition
/// string and caret byte-range; `Commit` carries the committed text.
///
/// # Examples
///
/// ```
/// use martensite_window::event::{ime_event_for_winit, ImeEvent};
///
/// let e = winit::event::Ime::Commit("é".to_string());
/// assert_eq!(ime_event_for_winit(&e), Some(ImeEvent::Committed("é".into())));
/// ```
#[must_use]
pub fn ime_event_for_winit(ime: &winit::event::Ime) -> Option<ImeEvent> {
    match ime {
        winit::event::Ime::Commit(text) => Some(ImeEvent::Committed(text.clone())),
        winit::event::Ime::Preedit(text, cursor) => Some(ImeEvent::Preedit {
            text: text.clone(),
            cursor: *cursor,
        }),
        // Enabled/Disabled are host lifecycle notifications, not
        // widget-delivery events; future winit variants are ignored too.
        _ => None,
    }
}

/// Converts a normalized [`PointerEvent`] into the widget-level
/// [`WidgetEvent`] vocabulary. `click_count` is the streak value
/// computed by the router's `ClickTracker` for `Pressed` events;
/// pass `1` for other states.
#[must_use]
pub fn widget_event_for_pointer(event: &PointerEvent, click_count: u8) -> WidgetEvent {
    let position = event.position;
    match event.state {
        PointerState::Moved => WidgetEvent::PointerMoved { position },
        PointerState::Pressed => WidgetEvent::PointerPressed {
            position,
            button: convert_pointer_button(event.button.unwrap_or(MouseButton::Left)),
            count: click_count,
        },
        PointerState::Released => WidgetEvent::PointerReleased {
            position,
            button: convert_pointer_button(event.button.unwrap_or(MouseButton::Left)),
        },
    }
}

/// Converts the normalized [`MouseButton`] into [`PointerButton`].
#[must_use]
pub fn convert_pointer_button(button: MouseButton) -> PointerButton {
    match button {
        MouseButton::Left => PointerButton::Primary,
        MouseButton::Right => PointerButton::Secondary,
        MouseButton::Middle => PointerButton::Middle,
        MouseButton::Back => PointerButton::Back,
        MouseButton::Forward => PointerButton::Forward,
        MouseButton::Other(n) => PointerButton::Other(u16::from(n)),
    }
}

// ---------------------------------------------------------------------------
// Winit event conversion
// ---------------------------------------------------------------------------

/// Converts a winit [`MouseButton`] into the normalized [`MouseButton`].
///
/// The five named buttons map directly; all numbered buttons (`Button6`..
/// `Button32`) collapse into [`MouseButton::Other`] carrying their 1-based
/// index.
///
/// # Examples
///
/// ```
/// use martensite_window::event::{convert_mouse_button, MouseButton};
///
/// assert_eq!(convert_mouse_button(winit::event::MouseButton::Left), MouseButton::Left);
/// assert_eq!(convert_mouse_button(winit::event::MouseButton::Right), MouseButton::Right);
/// assert_eq!(convert_mouse_button(winit::event::MouseButton::Middle), MouseButton::Middle);
/// ```
#[must_use]
pub fn convert_mouse_button(button: winit::event::MouseButton) -> MouseButton {
    match button {
        winit::event::MouseButton::Left => MouseButton::Left,
        winit::event::MouseButton::Right => MouseButton::Right,
        winit::event::MouseButton::Middle => MouseButton::Middle,
        winit::event::MouseButton::Back => MouseButton::Back,
        winit::event::MouseButton::Forward => MouseButton::Forward,
        // The remaining variants are the numbered `Button6`..`Button32`. They
        // are `#[repr(u8)]` with values 5..=31, so casting to `u8` yields the
        // 0-based index; add 1 to make it 1-based as documented.
        _ => MouseButton::Other(button as u8 + 1),
    }
}

/// Converts winit's combined modifier state into the normalized
/// [`ModifierKeys`].
///
/// This accepts the [`Modifiers`] value carried by
/// [`WindowEvent::ModifiersChanged`] and extracts the four logical modifier
/// groups (Shift, Control, Alt, Meta/Command). Per-side distinctions (left vs.
/// right) are intentionally collapsed.
///
/// [`WindowEvent::ModifiersChanged`]: winit::event::WindowEvent::ModifiersChanged
/// [`Modifiers`]: winit::event::Modifiers
///
/// # Examples
///
/// ```
/// use martensite_window::event::{convert_modifiers, ModifierKeys};
/// use winit::event::Modifiers;
/// use winit::keyboard::ModifiersState;
///
/// let mods = Modifiers::new(ModifiersState::SHIFT | ModifiersState::CONTROL, winit::keyboard::ModifiersKeys::empty());
/// let keys = convert_modifiers(&mods);
/// assert!(keys.contains(ModifierKeys::SHIFT));
/// assert!(keys.contains(ModifierKeys::CONTROL));
/// assert!(!keys.contains(ModifierKeys::ALT));
/// ```
#[must_use]
pub fn convert_modifiers(modifiers: &winit::event::Modifiers) -> ModifierKeys {
    convert_modifiers_state(&modifiers.state())
}

/// Converts winit's [`ModifiersState`] bitflags into the normalized
/// [`ModifierKeys`].
///
/// This is the lower-level helper used by [`convert_modifiers`]; it is exposed
/// for callers that track a [`ModifiersState`] directly rather than a full
/// [`Modifiers`] value.
///
/// [`ModifiersState`]: winit::keyboard::ModifiersState
/// [`Modifiers`]: winit::event::Modifiers
///
/// # Examples
///
/// ```
/// use martensite_window::event::{convert_modifiers_state, ModifierKeys};
/// use winit::keyboard::ModifiersState;
///
/// let state = ModifiersState::SHIFT | ModifiersState::ALT;
/// let keys = convert_modifiers_state(&state);
/// assert!(keys.contains(ModifierKeys::SHIFT));
/// assert!(keys.contains(ModifierKeys::ALT));
/// assert!(!keys.contains(ModifierKeys::CONTROL));
/// ```
#[must_use]
pub fn convert_modifiers_state(state: &winit::keyboard::ModifiersState) -> ModifierKeys {
    let mut keys = ModifierKeys::empty();
    if state.shift_key() {
        keys |= ModifierKeys::SHIFT;
    }
    if state.control_key() {
        keys |= ModifierKeys::CONTROL;
    }
    if state.alt_key() {
        keys |= ModifierKeys::ALT;
    }
    if state.meta_key() {
        keys |= ModifierKeys::COMMAND;
    }
    keys
}

/// Converts a winit [`WindowEvent`] into a normalized [`PointerEvent`].
///
/// The following variants produce a [`PointerEvent`]:
///
/// - [`WindowEvent::PointerMoved`] — becomes a [`PointerState::Moved`] event
///   with `button = None`. Mouse, touch, and tablet sources are all handled.
/// - [`WindowEvent::PointerButton`] — becomes a [`PointerState::Pressed`] or
///   [`PointerState::Released`] event carrying the affected button.
///
/// All other variants return `None`.
///
/// Physical coordinates from winit are converted to logical coordinates using
/// `scale`. The `modifiers` field of the returned event is left empty because
/// winit delivers modifier changes through a separate
/// [`WindowEvent::ModifiersChanged`] event; callers should overlay the
/// currently-tracked modifier state (see [`convert_modifiers`]) when they need
/// it.
///
/// Fields that have no `PointerEvent` counterpart are dropped: the touch
/// `force` (3D Touch pressure) carried by touch `PointerSource`s, the
/// `primary` flag on `PointerMoved` (redundant with
/// [`PointerId::PRIMARY`]/per-finger ids), and `device_id`.
///
/// [`WindowEvent::PointerMoved`]: winit::event::WindowEvent::PointerMoved
/// [`WindowEvent::PointerButton`]: winit::event::WindowEvent::PointerButton
/// [`WindowEvent::ModifiersChanged`]: winit::event::WindowEvent::ModifiersChanged
///
/// # Examples
///
/// ```
/// use martensite_window::dpi::DpiScale;
/// use martensite_window::event::{convert_window_event, PointerState};
/// use winit::dpi::PhysicalPosition;
/// use winit::event::{PointerSource, WindowEvent};
///
/// let scale = DpiScale::new(2.0);
/// let event = WindowEvent::PointerMoved {
///     device_id: None,
///     position: PhysicalPosition::new(100.0, 200.0),
///     primary: true,
///     source: PointerSource::Mouse,
/// };
/// let pe = convert_window_event(&event, &scale).expect("pointer moved converts");
/// assert_eq!(pe.state, PointerState::Moved);
/// // 100 physical / 2.0 scale = 50 logical.
/// assert_eq!(pe.position, glam::Vec2::new(50.0, 100.0));
/// ```
#[must_use]
pub fn convert_window_event(event: &WindowEvent, scale: &DpiScale) -> Option<PointerEvent> {
    use winit::event::{ElementState, WindowEvent};

    match event {
        WindowEvent::PointerMoved {
            position, source, ..
        } => {
            let pos = physical_to_logical(*position, scale);
            Some(PointerEvent {
                pointer_id: pointer_id_from_source(source),
                kind: pointer_kind_from_source(source),
                position: pos,
                state: PointerState::Moved,
                button: None,
                modifiers: ModifierKeys::empty(),
            })
        }
        WindowEvent::PointerButton {
            state,
            position,
            button,
            ..
        } => {
            let pos = physical_to_logical(*position, scale);
            let (pointer_id, kind, mb) = button_source_info(button);
            let pointer_state = match state {
                ElementState::Pressed => PointerState::Pressed,
                ElementState::Released => PointerState::Released,
            };
            Some(PointerEvent {
                pointer_id,
                kind,
                position: pos,
                state: pointer_state,
                button: mb,
                modifiers: ModifierKeys::empty(),
            })
        }
        _ => None,
    }
}

/// The OS-proposed action for an incoming drag, mapped onto the common
/// [`DropAction`] set where possible.
///
/// `Ask` and `Private` have no [`DropAction`] equivalent (they are
/// platform-specific) and are preserved verbatim so callers can round-trip
/// them back to the OS.
///
/// # Examples
///
/// ```
/// use martensite_window::event::DropAction;
///
/// // `None` is the default when the OS proposes no action.
/// assert_eq!(DropAction::default(), DropAction::None);
/// assert_ne!(DropAction::Copy, DropAction::Move);
/// assert_ne!(DropAction::Link, DropAction::Ask);
/// ```
#[derive(Copy, Clone, Debug, PartialEq, Eq, Default)]
pub enum DropAction {
    /// No action proposed by the OS.
    #[default]
    None,
    /// Copy the data.
    Copy,
    /// Move the data.
    Move,
    /// Link the data.
    Link,
    /// Ask the user what to do (platform-specific).
    Ask,
    /// Private source/destination negotiation (macOS).
    Private,
}

/// A normalized drag-and-drop event, independent of winit.
///
/// All positions are in **logical** coordinates (already divided by the
/// window's DPI scale factor). This mirrors winit 0.31's `DragEntered` /
/// `DragPosition` / `DragDropped` / `DragLeft` events, which replaced the
/// legacy `DroppedFile` / `HoveredFile` / `HoveredFileCancelled` events from
/// earlier winit versions.
///
/// `convert_drop_event` produces these from a winit `WindowEvent`. Note that
/// winit's `DragEntered`/`DragPosition`/`DragDropped` events do **not** embed
/// the list of MIME types advertised by the source; that must be fetched
/// separately via the winit `ActiveEventLoop` data-transfer API (see the
/// `martensite-dnd` `DndPlatform` seam). [`DropEvent`] therefore carries no
/// type list — the caller correlates it with the platform-fetched types.
///
/// # Examples
///
/// ```
/// use martensite_window::dpi::DpiScale;
/// use martensite_window::event::{convert_drop_event, DropEvent};
/// use winit::data_transfer::DataTransferId;
/// use winit::dpi::PhysicalPosition;
/// use winit::event::WindowEvent;
///
/// let scale = DpiScale::new(2.0);
/// let event = WindowEvent::DragPosition {
///     id: DataTransferId::from_raw(7),
///     position: PhysicalPosition::new(100.0, 200.0),
///     proposed_action: None,
/// };
/// let drop = convert_drop_event(&event, &scale).expect("drag position converts");
/// assert_eq!(drop.position(), Some(glam::Vec2::new(50.0, 100.0)));
/// ```
#[derive(Clone, Debug, PartialEq)]
#[non_exhaustive]
pub enum DropEvent {
    /// A drag operation entered the window. `position` may be `None` on
    /// platforms that do not report it on enter.
    Entered {
        /// Logical position of the drag, if reported.
        position: Option<Vec2>,
        /// OS-proposed action (often `None` on enter).
        action: DropAction,
    },
    /// The drag moved within the window.
    Moved {
        /// Logical position of the drag.
        position: Vec2,
        /// OS-proposed action.
        action: DropAction,
    },
    /// The drag was dropped on the window. winit does not report a position
    /// for the drop; use the last position from `Entered`/`Moved`.
    Dropped {
        /// OS-proposed action.
        action: DropAction,
    },
    /// The drag left the window or was canceled.
    Left,
}

impl DropEvent {
    /// Returns the logical position carried by this event, if any.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite_window::event::{DropAction, DropEvent};
    ///
    /// let e = DropEvent::Moved {
    ///     position: glam::Vec2::new(1.0, 2.0),
    ///     action: DropAction::Copy,
    /// };
    /// assert_eq!(e.position(), Some(glam::Vec2::new(1.0, 2.0)));
    /// assert!(DropEvent::Left.position().is_none());
    /// ```
    #[must_use]
    pub fn position(&self) -> Option<Vec2> {
        match self {
            DropEvent::Entered { position, .. } => *position,
            DropEvent::Moved { position, .. } => Some(*position),
            DropEvent::Dropped { .. } | DropEvent::Left => None,
        }
    }
}

/// Converts a winit [`WindowEvent`] into a normalized [`DropEvent`].
///
/// The following winit variants produce a [`DropEvent`]:
///
/// - [`WindowEvent::DragEntered`] → [`DropEvent::Entered`]
/// - [`WindowEvent::DragPosition`] → [`DropEvent::Moved`]
/// - [`WindowEvent::DragDropped`] → [`DropEvent::Dropped`]
/// - [`WindowEvent::DragLeft`] → [`DropEvent::Left`]
///
/// All other variants return `None`. Physical coordinates from winit are
/// converted to logical coordinates using `scale`.
///
/// [`WindowEvent::DragEntered`]: winit::event::WindowEvent::DragEntered
/// [`WindowEvent::DragPosition`]: winit::event::WindowEvent::DragPosition
/// [`WindowEvent::DragDropped`]: winit::event::WindowEvent::DragDropped
/// [`WindowEvent::DragLeft`]: winit::event::WindowEvent::DragLeft
///
/// # Examples
///
/// ```
/// use martensite_window::dpi::DpiScale;
/// use martensite_window::event::{convert_drop_event, DropAction, DropEvent};
/// use winit::data_transfer::DataTransferId;
/// use winit::event::WindowEvent;
/// use winit::event_loop::DndAction;
///
/// let scale = DpiScale::new(1.0);
/// let event = WindowEvent::DragDropped {
///     id: DataTransferId::from_raw(1),
///     proposed_action: Some(DndAction::Copy),
/// };
/// let drop = convert_drop_event(&event, &scale).expect("drag dropped converts");
/// assert_eq!(drop, DropEvent::Dropped { action: DropAction::Copy });
/// ```
#[must_use]
pub fn convert_drop_event(event: &WindowEvent, scale: &DpiScale) -> Option<DropEvent> {
    use winit::event::WindowEvent;

    match event {
        WindowEvent::DragEntered { position, .. } => Some(DropEvent::Entered {
            position: position.map(|p| physical_to_logical(p, scale)),
            action: DropAction::default(),
        }),
        WindowEvent::DragPosition {
            position,
            proposed_action,
            ..
        } => Some(DropEvent::Moved {
            position: physical_to_logical(*position, scale),
            action: drop_action_from_winit(*proposed_action),
        }),
        WindowEvent::DragDropped {
            proposed_action, ..
        } => Some(DropEvent::Dropped {
            action: drop_action_from_winit(*proposed_action),
        }),
        WindowEvent::DragLeft { .. } => Some(DropEvent::Left),
        _ => None,
    }
}

/// Maps a winit `DndAction` (or `None`) to a [`DropAction`].
fn drop_action_from_winit(action: Option<winit::event_loop::DndAction>) -> DropAction {
    use winit::event_loop::DndAction;
    match action {
        Some(DndAction::Copy) => DropAction::Copy,
        Some(DndAction::Move) => DropAction::Move,
        Some(DndAction::Link) => DropAction::Link,
        Some(DndAction::Ask) => DropAction::Ask,
        Some(DndAction::Private) => DropAction::Private,
        // `DndAction` is `#[non_exhaustive]`; unknown variants map to `None`.
        Some(_) | None => DropAction::None,
    }
}

/// Converts a winit [`PhysicalPosition<f64>`] into a logical [`Vec2`] using
/// `scale`.
///
/// [`PhysicalPosition<f64>`]: winit::dpi::PhysicalPosition
fn physical_to_logical(position: winit::dpi::PhysicalPosition<f64>, scale: &DpiScale) -> Vec2 {
    Vec2::new(
        scale.to_logical(position.x) as f32,
        scale.to_logical(position.y) as f32,
    )
}

/// Converts a winit [`FingerId`] into a [`PointerId`] that cannot collide
/// with [`PointerId::PRIMARY`].
///
/// `FingerId::into_raw` may return `0` for a fresh touch sequence, which
/// would alias `PointerId::PRIMARY` (mouse). Offsetting by one keeps every
/// finger distinct from the primary pointer. `saturating_add` is used so a
/// hypothetical `u64::MAX` raw id saturates instead of wrapping to `0`.
///
/// [`FingerId`]: winit::event::FingerId
fn pointer_id_from_finger(finger_id: &winit::event::FingerId) -> PointerId {
    PointerId::new((finger_id.into_raw() as u64).saturating_add(1))
}

/// Derives a [`PointerId`] from a winit [`PointerSource`].
///
/// Mouse and tablet sources map to the primary pointer; touch sources carry a
/// distinct id per finger. The raw [`FingerId`] value is a `usize` — on iOS
/// the address of the `UITouch` object — so it is widened, not truncated.
///
/// [`PointerSource`]: winit::event::PointerSource
/// [`FingerId`]: winit::event::FingerId
fn pointer_id_from_source(source: &winit::event::PointerSource) -> PointerId {
    use winit::event::PointerSource;

    match source {
        PointerSource::Touch { finger_id, .. } => pointer_id_from_finger(finger_id),
        // Mouse, tablet, and unknown sources share the primary pointer id.
        _ => PointerId::PRIMARY,
    }
}

/// Derives a normalized [`PointerKind`] from a winit [`PointerSource`].
///
/// [`PointerSource`]: winit::event::PointerSource
fn pointer_kind_from_source(source: &winit::event::PointerSource) -> PointerKind {
    use winit::event::PointerSource;

    match source {
        PointerSource::Mouse => PointerKind::Mouse,
        PointerSource::Touch { .. } => PointerKind::Touch,
        PointerSource::TabletTool { .. } => PointerKind::Tablet,
        // `PointerSource` is `#[non_exhaustive]`; unknown and future
        // sources map to `PointerKind::Unknown`.
        _ => PointerKind::Unknown,
    }
}

/// Extracts the [`PointerId`], normalized [`PointerKind`], and normalized
/// [`MouseButton`] from a winit [`ButtonSource`].
///
/// Touch presses are reported as [`MouseButton::Left`] (the conventional
/// primary button for touch). Tablet buttons are mapped through their
/// standard mouse-button equivalents when available.
///
/// [`ButtonSource`]: winit::event::ButtonSource
fn button_source_info(
    button: &winit::event::ButtonSource,
) -> (PointerId, PointerKind, Option<MouseButton>) {
    use winit::event::{ButtonSource, TabletToolButton};

    match button {
        ButtonSource::Mouse(mouse) => (
            PointerId::PRIMARY,
            PointerKind::Mouse,
            Some(convert_mouse_button(*mouse)),
        ),
        ButtonSource::Touch { finger_id, .. } => (
            pointer_id_from_finger(finger_id),
            PointerKind::Touch,
            Some(MouseButton::Left),
        ),
        ButtonSource::TabletTool {
            button: tool_button,
            ..
        } => {
            let mb = match tool_button {
                TabletToolButton::Contact => Some(MouseButton::Left),
                TabletToolButton::Barrel => Some(MouseButton::Right),
                TabletToolButton::Other(raw) => match raw {
                    1 => Some(MouseButton::Middle),
                    3 => Some(MouseButton::Back),
                    4 => Some(MouseButton::Forward),
                    _ => Some(MouseButton::Other(*raw as u8)),
                },
            };
            (PointerId::PRIMARY, PointerKind::Tablet, mb)
        }
        ButtonSource::Unknown(_) => (PointerId::PRIMARY, PointerKind::Unknown, None),
        // `ButtonSource` is `#[non_exhaustive]`; future winit versions may
        // add sources. Route them to the primary pointer with no button.
        _ => (PointerId::PRIMARY, PointerKind::Unknown, None),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use martensite_core::{ColdNode, HotNode, NodeFlags, Rect, WidgetArena};

    fn press_at(pos: Vec2) -> PointerEvent {
        PointerEvent {
            pointer_id: PointerId::PRIMARY,
            kind: PointerKind::Mouse,
            position: pos,
            state: PointerState::Pressed,
            button: Some(MouseButton::Left),
            modifiers: ModifierKeys::empty(),
        }
    }

    #[test]
    fn click_streak_increments_within_interval() {
        let mut tracker = ClickTracker::default();
        let t0 = Instant::now();
        let ev = press_at(Vec2::new(10.0, 10.0));
        assert_eq!(tracker.register_at(&ev, t0), 1);
        assert_eq!(tracker.register_at(&ev, t0 + Duration::from_millis(200)), 2);
        assert_eq!(tracker.register_at(&ev, t0 + Duration::from_millis(400)), 3);
    }

    #[test]
    fn click_streak_resets_on_interval_slop_button_and_pointer() {
        let mut tracker = ClickTracker::default();
        let t0 = Instant::now();
        let ev = press_at(Vec2::new(10.0, 10.0));
        assert_eq!(tracker.register_at(&ev, t0), 1);

        // Past the interval → restart.
        assert_eq!(tracker.register_at(&ev, t0 + Duration::from_secs(1)), 1);
        // Within the interval but beyond the slop radius → restart.
        let far = press_at(Vec2::new(10.0 + CLICK_STREAK_SLOP + 1.0, 10.0));
        assert_eq!(
            tracker.register_at(
                &far,
                t0 + Duration::from_secs(1) + Duration::from_millis(10)
            ),
            1
        );
        // Different button → restart.
        let mut right = far;
        right.button = Some(MouseButton::Right);
        assert_eq!(
            tracker.register_at(
                &right,
                t0 + Duration::from_secs(1) + Duration::from_millis(20)
            ),
            1
        );
        // Different pointer → restart.
        let mut touch = press_at(Vec2::new(10.0 + CLICK_STREAK_SLOP + 1.0, 10.0));
        touch.pointer_id = PointerId(7);
        assert_eq!(
            tracker.register_at(
                &touch,
                t0 + Duration::from_secs(1) + Duration::from_millis(30)
            ),
            1
        );
        // Moves and releases never extend or restart the streak.
        let mut moved = touch;
        moved.state = PointerState::Moved;
        assert_eq!(tracker.register_at(&moved, t0 + Duration::from_secs(2)), 1);
        // A press within the interval of the last press continues it.
        assert_eq!(
            tracker.register_at(
                &touch,
                t0 + Duration::from_secs(1) + Duration::from_millis(400)
            ),
            2
        );
        // One past the interval restarts.
        assert_eq!(tracker.register_at(&touch, t0 + Duration::from_secs(3)), 1);
    }
    use winit::dpi::PhysicalPosition;
    use winit::event::{
        ButtonSource, ElementState, FingerId, Force, Modifiers, MouseButton as WinitMouseButton,
        PointerSource, WindowEvent,
    };
    use winit::keyboard::{ModifiersKeys, ModifiersState};

    /// Build a visible, hit-testable node with the given bounds.
    fn hot_node(x: f32, y: f32, w: f32, h: f32) -> HotNode {
        HotNode {
            bounds: Rect::new(x, y, w, h),
            flags: NodeFlags::VISIBLE | NodeFlags::HIT_TEST_ENABLED,
            ..HotNode::default()
        }
    }

    /// Insert a node and return its id.
    fn insert(arena: &mut WidgetArena, x: f32, y: f32, w: f32, h: f32) -> WidgetId {
        arena.insert(hot_node(x, y, w, h), ColdNode::default())
    }

    // -----------------------------------------------------------------
    // PointerCapture
    // -----------------------------------------------------------------

    #[test]
    fn pointer_capture_starts_empty() {
        let cap = PointerCapture::new();
        assert!(cap.captured_primary().is_none());
        assert_eq!(cap.active_captures(), 0);
    }

    #[test]
    fn pointer_capture_routes_all_events_to_captured_widget() {
        let mut arena = WidgetArena::new();
        let a = insert(&mut arena, 0.0, 0.0, 100.0, 100.0);
        let b = insert(&mut arena, 200.0, 0.0, 100.0, 100.0);

        let win = WindowId::from_raw(1);
        let mut router = EventRouter::new();
        router.capture_pointer_primary(a);

        // Event positioned over `b` should still route to captured `a`.
        let event = PointerEvent {
            pointer_id: PointerId::PRIMARY,
            kind: PointerKind::Mouse,
            position: Vec2::new(250.0, 50.0),
            state: PointerState::Moved,
            button: None,
            modifiers: ModifierKeys::empty(),
        };
        assert_eq!(
            router.route_pointer_event(&arena, a, win, &event),
            EventDispatchOutcome::Handled(a)
        );
        // Root is `a`; capture overrides hit-test even when the point is over
        // a different subtree root `b`.
        assert_eq!(
            router.route_pointer_event(&arena, b, win, &event),
            EventDispatchOutcome::Handled(a)
        );
    }

    #[test]
    fn pointer_capture_released_when_target_becomes_covered() {
        let mut arena = WidgetArena::new();
        let root = insert(&mut arena, 0.0, 0.0, 300.0, 300.0);
        let a = arena.insert(
            hot_node(0.0, 0.0, 40.0, 10.0),
            ColdNode::default().with_render_minimum(
                martensite_core::RenderMinimum::new(Vec2::new(80.0, 24.0))
                    .with_policy(martensite_core::UnderflowPolicy::Hide),
            ),
        );
        let b = insert(&mut arena, 200.0, 0.0, 100.0, 100.0);
        arena.append_child(root, a).unwrap();
        arena.append_child(root, b).unwrap();

        let win = WindowId::from_raw(1);
        let mut router = EventRouter::new();
        router.capture_pointer_primary(a);

        // The captured widget becomes underflow-covered mid-gesture —
        // capture is revoked and routing falls back to hit-testing.
        arena.update_underflow(a);
        let event = PointerEvent {
            pointer_id: PointerId::PRIMARY,
            kind: PointerKind::Mouse,
            position: Vec2::new(250.0, 50.0),
            state: PointerState::Moved,
            button: None,
            modifiers: ModifierKeys::empty(),
        };
        assert_eq!(
            router.route_pointer_event(&arena, root, win, &event),
            EventDispatchOutcome::Handled(b)
        );
        assert!(router.capture.captured(PointerId::PRIMARY).is_none());
    }

    #[test]
    fn pointer_capture_release_resumes_hit_testing() {
        let mut arena = WidgetArena::new();
        let a = insert(&mut arena, 0.0, 0.0, 100.0, 100.0);
        let b = insert(&mut arena, 200.0, 0.0, 100.0, 100.0);

        let win = WindowId::from_raw(1);
        let mut router = EventRouter::new();
        router.capture_pointer_primary(a);

        let event = PointerEvent {
            pointer_id: PointerId::PRIMARY,
            kind: PointerKind::Mouse,
            position: Vec2::new(250.0, 50.0),
            state: PointerState::Moved,
            button: None,
            modifiers: ModifierKeys::empty(),
        };
        assert_eq!(
            router.route_pointer_event(&arena, a, win, &event),
            EventDispatchOutcome::Handled(a)
        );

        router.release_pointer_primary();
        // Now routing should hit-test; point is over `b`.
        assert_eq!(
            router.route_pointer_event(&arena, b, win, &event),
            EventDispatchOutcome::Handled(b)
        );
    }

    #[test]
    fn pointer_capture_dead_widget_auto_releases() {
        let mut arena = WidgetArena::new();
        let a = insert(&mut arena, 0.0, 0.0, 100.0, 100.0);

        let win = WindowId::from_raw(1);
        let mut router = EventRouter::new();
        router.capture_pointer_primary(a);

        // Remove the captured widget from the arena.
        arena.remove(a);

        let event = PointerEvent {
            pointer_id: PointerId::PRIMARY,
            kind: PointerKind::Mouse,
            position: Vec2::new(50.0, 50.0),
            state: PointerState::Moved,
            button: None,
            modifiers: ModifierKeys::empty(),
        };
        // No other live widget to hit; capture is stale and should release.
        assert_eq!(
            router.route_pointer_event(&arena, a, win, &event),
            EventDispatchOutcome::Unhandled
        );
        assert!(router.captured_widget_primary().is_none());
    }

    #[test]
    fn pointer_capture_default_is_empty() {
        let cap = PointerCapture::default();
        assert!(cap.captured_primary().is_none());
        assert_eq!(cap.active_captures(), 0);
    }

    #[test]
    fn pointer_capture_multi_pointer_independent() {
        let mut cap = PointerCapture::new();
        let widget_a = WidgetId::from_parts(1, 1);
        let widget_b = WidgetId::from_parts(2, 1);
        let finger_1 = PointerId::new(1);
        let finger_2 = PointerId::new(2);

        // Two different fingers captured to two different widgets.
        cap.capture(finger_1, widget_a);
        cap.capture(finger_2, widget_b);
        assert_eq!(cap.active_captures(), 2);
        assert_eq!(cap.captured(finger_1), Some(widget_a));
        assert_eq!(cap.captured(finger_2), Some(widget_b));

        // Releasing one finger does not affect the other.
        cap.release(finger_1);
        assert_eq!(cap.active_captures(), 1);
        assert!(cap.captured(finger_1).is_none());
        assert_eq!(cap.captured(finger_2), Some(widget_b));

        // `clear` releases every remaining capture.
        cap.clear();
        assert_eq!(cap.active_captures(), 0);
        assert!(cap.captured(finger_2).is_none());
    }

    // -----------------------------------------------------------------
    // MouseTracker
    // -----------------------------------------------------------------

    #[test]
    fn mouse_tracker_position_tracking_per_window() {
        let mut tracker = MouseTracker::new();
        let w1 = WindowId::from_raw(1);
        let w2 = WindowId::from_raw(2);

        assert_eq!(tracker.position(w1), None);
        tracker.update_position(w1, Vec2::new(10.0, 20.0));
        tracker.update_position(w2, Vec2::new(30.0, 40.0));

        assert_eq!(tracker.position(w1), Some(Vec2::new(10.0, 20.0)));
        assert_eq!(tracker.position(w2), Some(Vec2::new(30.0, 40.0)));

        // Updating one window does not affect the other.
        tracker.update_position(w1, Vec2::new(99.0, 99.0));
        assert_eq!(tracker.position(w1), Some(Vec2::new(99.0, 99.0)));
        assert_eq!(tracker.position(w2), Some(Vec2::new(30.0, 40.0)));
        assert_eq!(tracker.tracked_window_count(), 2);
    }

    #[test]
    fn mouse_tracker_hover_state_tracking() {
        let mut tracker = MouseTracker::new();
        let win = WindowId::from_raw(1);
        let widget = WidgetId::from_parts(1, 1);

        assert_eq!(tracker.hovered_widget(win), None);
        tracker.set_hovered(win, Some(widget));
        assert_eq!(tracker.hovered_widget(win), Some(widget));
        tracker.set_hovered(win, None);
        assert_eq!(tracker.hovered_widget(win), None);
    }

    #[test]
    fn mouse_tracker_hover_is_per_window() {
        let mut tracker = MouseTracker::new();
        let w1 = WindowId::from_raw(1);
        let w2 = WindowId::from_raw(2);
        let widget_a = WidgetId::from_parts(1, 1);
        let widget_b = WidgetId::from_parts(2, 1);

        tracker.set_hovered(w1, Some(widget_a));
        tracker.set_hovered(w2, Some(widget_b));
        assert_eq!(tracker.hovered_widget(w1), Some(widget_a));
        assert_eq!(tracker.hovered_widget(w2), Some(widget_b));
    }

    #[test]
    fn mouse_tracker_clear_window_removes_all_state() {
        let mut tracker = MouseTracker::new();
        let win = WindowId::from_raw(1);
        let widget = WidgetId::from_parts(1, 1);

        tracker.update_position(win, Vec2::new(5.0, 5.0));
        tracker.set_hovered(win, Some(widget));
        assert_eq!(tracker.tracked_window_count(), 1);

        tracker.clear_window(win);
        assert_eq!(tracker.position(win), None);
        assert_eq!(tracker.hovered_widget(win), None);
        assert_eq!(tracker.tracked_window_count(), 0);
    }

    // -----------------------------------------------------------------
    // EventRouter — hit-test routing
    // -----------------------------------------------------------------

    #[test]
    fn router_routes_to_correct_widget_by_position() {
        let mut arena = WidgetArena::new();
        let root = insert(&mut arena, 0.0, 0.0, 200.0, 200.0);
        let child = insert(&mut arena, 50.0, 50.0, 100.0, 100.0);
        arena.append_child(root, child).unwrap();

        let win = WindowId::from_raw(1);
        let mut router = EventRouter::new();

        // Point over child → child wins (topmost).
        let over_child = PointerEvent {
            pointer_id: PointerId::PRIMARY,
            kind: PointerKind::Mouse,
            position: Vec2::new(75.0, 75.0),
            state: PointerState::Moved,
            button: None,
            modifiers: ModifierKeys::empty(),
        };
        assert_eq!(
            router.route_pointer_event(&arena, root, win, &over_child),
            EventDispatchOutcome::Handled(child)
        );

        // Point over root only → root wins.
        let over_root = PointerEvent {
            pointer_id: PointerId::PRIMARY,
            kind: PointerKind::Mouse,
            position: Vec2::new(10.0, 10.0),
            state: PointerState::Moved,
            button: None,
            modifiers: ModifierKeys::empty(),
        };
        assert_eq!(
            router.route_pointer_event(&arena, root, win, &over_root),
            EventDispatchOutcome::Handled(root)
        );
    }

    #[test]
    fn router_captured_widget_takes_priority_over_hit_test() {
        let mut arena = WidgetArena::new();
        let root = insert(&mut arena, 0.0, 0.0, 200.0, 200.0);
        let child = insert(&mut arena, 50.0, 50.0, 100.0, 100.0);
        arena.append_child(root, child).unwrap();

        let win = WindowId::from_raw(1);
        let mut router = EventRouter::new();
        router.capture_pointer_primary(root);

        // Point is over child, but capture forces routing to root.
        let event = PointerEvent {
            pointer_id: PointerId::PRIMARY,
            kind: PointerKind::Mouse,
            position: Vec2::new(75.0, 75.0),
            state: PointerState::Pressed,
            button: Some(MouseButton::Left),
            modifiers: ModifierKeys::empty(),
        };
        assert_eq!(
            router.route_pointer_event(&arena, root, win, &event),
            EventDispatchOutcome::Handled(root)
        );
    }

    #[test]
    fn router_keyboard_event_routes_to_focused() {
        let mut router = EventRouter::new();
        let focused = WidgetId::from_parts(3, 1);
        assert_eq!(
            router.route_keyboard_event(Some(focused)),
            EventDispatchOutcome::Handled(focused)
        );
        assert_eq!(
            router.route_keyboard_event(None),
            EventDispatchOutcome::Ignored
        );
    }

    #[test]
    fn router_ime_event_delivers_to_focused() {
        use martensite_core::DummyWidget;
        let mut arena = WidgetArena::new();
        let widget =
            arena.insert_with_widget(hot_node(0.0, 0.0, 10.0, 10.0), Box::new(DummyWidget));
        let mut router = EventRouter::new();

        // No focus → no delivery.
        assert!(router
            .dispatch_ime_event(&mut arena, None, &ImeEvent::Committed("a".into()))
            .is_none());
        // Both variants deliver to the focused widget.
        assert!(router
            .dispatch_ime_event(&mut arena, Some(widget), &ImeEvent::Committed("x".into()))
            .is_some());
        assert!(router
            .dispatch_ime_event(
                &mut arena,
                Some(widget),
                &ImeEvent::Preedit {
                    text: "w".into(),
                    cursor: Some((0, 1)),
                }
            )
            .is_some());
    }

    #[test]
    fn ime_event_for_winit_maps_variants() {
        let commit = winit::event::Ime::Commit("é".into());
        assert_eq!(
            ime_event_for_winit(&commit),
            Some(ImeEvent::Committed("é".into()))
        );
        let pre = winit::event::Ime::Preedit("nich".into(), Some((0, 4)));
        assert_eq!(
            ime_event_for_winit(&pre),
            Some(ImeEvent::Preedit {
                text: "nich".into(),
                cursor: Some((0, 4)),
            })
        );
        // Lifecycle notifications never reach widgets.
        assert!(ime_event_for_winit(&winit::event::Ime::Enabled).is_none());
        assert!(ime_event_for_winit(&winit::event::Ime::Disabled).is_none());
    }

    // -----------------------------------------------------------------
    // Scroll event routing
    // -----------------------------------------------------------------

    #[test]
    fn router_scroll_routes_to_hovered_widget() {
        let mut arena = WidgetArena::new();
        let root = insert(&mut arena, 0.0, 0.0, 200.0, 200.0);
        let child = insert(&mut arena, 50.0, 50.0, 100.0, 100.0);
        arena.append_child(root, child).unwrap();

        let win = WindowId::from_raw(1);
        let mut router = EventRouter::new();

        // Establish hover over the child by routing a pointer event first.
        let over_child = PointerEvent {
            pointer_id: PointerId::PRIMARY,
            kind: PointerKind::Mouse,
            position: Vec2::new(75.0, 75.0),
            state: PointerState::Moved,
            button: None,
            modifiers: ModifierKeys::empty(),
        };
        router.route_pointer_event(&arena, root, win, &over_child);
        assert_eq!(
            router.route_scroll_event(win, Vec2::new(0.0, 10.0)),
            EventDispatchOutcome::Handled(child)
        );

        // Move the pointer over the root only and scroll again.
        let over_root = PointerEvent {
            pointer_id: PointerId::PRIMARY,
            kind: PointerKind::Mouse,
            position: Vec2::new(10.0, 10.0),
            state: PointerState::Moved,
            button: None,
            modifiers: ModifierKeys::empty(),
        };
        router.route_pointer_event(&arena, root, win, &over_root);
        assert_eq!(
            router.route_scroll_event(win, Vec2::new(0.0, 10.0)),
            EventDispatchOutcome::Handled(root)
        );
    }

    #[test]
    fn router_scroll_unhandled_when_no_hover() {
        let mut router = EventRouter::new();
        let win = WindowId::from_raw(1);
        // No pointer event has been routed for this window, so nothing is
        // hovered and the scroll is unhandled.
        assert_eq!(
            router.route_scroll_event(win, Vec2::new(0.0, 1.0)),
            EventDispatchOutcome::Unhandled
        );
    }

    // -----------------------------------------------------------------
    // ModifierKeys
    // -----------------------------------------------------------------

    #[test]
    fn modifier_keys_all_combinations() {
        assert_eq!(ModifierKeys::empty(), ModifierKeys::empty());
        assert!(ModifierKeys::SHIFT.contains(ModifierKeys::SHIFT));
        assert!(!ModifierKeys::SHIFT.contains(ModifierKeys::CONTROL));

        let all =
            ModifierKeys::SHIFT | ModifierKeys::CONTROL | ModifierKeys::ALT | ModifierKeys::COMMAND;
        assert!(all.contains(ModifierKeys::SHIFT));
        assert!(all.contains(ModifierKeys::CONTROL));
        assert!(all.contains(ModifierKeys::ALT));
        assert!(all.contains(ModifierKeys::COMMAND));

        let shift_ctrl = ModifierKeys::SHIFT | ModifierKeys::CONTROL;
        assert_eq!(shift_ctrl, ModifierKeys::SHIFT | ModifierKeys::CONTROL);
        assert_ne!(shift_ctrl, ModifierKeys::SHIFT | ModifierKeys::ALT);

        // Removal.
        let mut m = shift_ctrl;
        m.remove(ModifierKeys::SHIFT);
        assert_eq!(m, ModifierKeys::CONTROL);
    }

    #[test]
    fn convert_modifiers_state_all_combinations() {
        let empty = convert_modifiers_state(&ModifiersState::empty());
        assert_eq!(empty, ModifierKeys::empty());

        let shift = convert_modifiers_state(&ModifiersState::SHIFT);
        assert_eq!(shift, ModifierKeys::SHIFT);

        let ctrl = convert_modifiers_state(&ModifiersState::CONTROL);
        assert_eq!(ctrl, ModifierKeys::CONTROL);

        let alt = convert_modifiers_state(&ModifiersState::ALT);
        assert_eq!(alt, ModifierKeys::ALT);

        let meta = convert_modifiers_state(&ModifiersState::META);
        assert_eq!(meta, ModifierKeys::COMMAND);

        let combo = convert_modifiers_state(&(ModifiersState::SHIFT | ModifiersState::CONTROL));
        assert_eq!(combo, ModifierKeys::SHIFT | ModifierKeys::CONTROL);

        let all = convert_modifiers_state(
            &(ModifiersState::SHIFT
                | ModifiersState::CONTROL
                | ModifiersState::ALT
                | ModifiersState::META),
        );
        assert_eq!(
            all,
            ModifierKeys::SHIFT | ModifierKeys::CONTROL | ModifierKeys::ALT | ModifierKeys::COMMAND
        );
    }

    #[test]
    fn convert_modifiers_from_modifiers_value() {
        let mods = Modifiers::new(
            ModifiersState::SHIFT | ModifiersState::ALT,
            ModifiersKeys::empty(),
        );
        assert_eq!(
            convert_modifiers(&mods),
            ModifierKeys::SHIFT | ModifierKeys::ALT
        );
    }

    // -----------------------------------------------------------------
    // Winit event conversion
    // -----------------------------------------------------------------

    #[test]
    fn convert_mouse_button_named() {
        assert_eq!(
            convert_mouse_button(WinitMouseButton::Left),
            MouseButton::Left
        );
        assert_eq!(
            convert_mouse_button(WinitMouseButton::Right),
            MouseButton::Right
        );
        assert_eq!(
            convert_mouse_button(WinitMouseButton::Middle),
            MouseButton::Middle
        );
        assert_eq!(
            convert_mouse_button(WinitMouseButton::Back),
            MouseButton::Back
        );
        assert_eq!(
            convert_mouse_button(WinitMouseButton::Forward),
            MouseButton::Forward
        );
    }

    #[test]
    fn convert_mouse_button_extended() {
        // Button6 is repr(u8) value 5 → Other(6) (1-based).
        assert_eq!(
            convert_mouse_button(WinitMouseButton::Button6),
            MouseButton::Other(6)
        );
        assert_eq!(
            convert_mouse_button(WinitMouseButton::Button32),
            MouseButton::Other(32)
        );
    }

    #[test]
    fn convert_window_event_pointer_moved_mouse() {
        let scale = DpiScale::new(2.0);
        let event = WindowEvent::PointerMoved {
            device_id: None,
            position: PhysicalPosition::new(100.0, 200.0),
            primary: true,
            source: PointerSource::Mouse,
        };
        let pe = convert_window_event(&event, &scale).expect("pointer moved converts");
        assert_eq!(pe.pointer_id, PointerId::PRIMARY);
        assert_eq!(pe.state, PointerState::Moved);
        assert_eq!(pe.button, None);
        assert_eq!(pe.position, Vec2::new(50.0, 100.0));
        assert_eq!(pe.modifiers, ModifierKeys::empty());
    }

    #[test]
    fn convert_window_event_pointer_moved_touch() {
        let scale = DpiScale::new(1.0);
        let event = WindowEvent::PointerMoved {
            device_id: None,
            position: PhysicalPosition::new(10.0, 20.0),
            primary: false,
            source: PointerSource::Touch {
                finger_id: FingerId::from_raw(3),
                force: Some(Force::Normalized(0.5)),
            },
        };
        let pe = convert_window_event(&event, &scale).expect("touch moved converts");
        // Finger ids are offset by one so raw id 0 cannot alias PRIMARY.
        assert_eq!(pe.pointer_id, PointerId::new(4));
        assert_eq!(pe.kind, PointerKind::Touch);
        assert_eq!(pe.state, PointerState::Moved);
        assert_eq!(pe.position, Vec2::new(10.0, 20.0));
    }

    #[test]
    fn convert_window_event_touch_id_zero_never_primary() {
        let scale = DpiScale::new(1.0);
        let event = WindowEvent::PointerMoved {
            device_id: None,
            position: PhysicalPosition::new(1.0, 1.0),
            primary: false,
            source: PointerSource::Touch {
                finger_id: FingerId::from_raw(0),
                force: None,
            },
        };
        let pe = convert_window_event(&event, &scale).expect("touch moved converts");
        // A raw finger id of 0 must not alias the primary (mouse) pointer.
        assert_ne!(pe.pointer_id, PointerId::PRIMARY);
        assert_eq!(pe.pointer_id, PointerId::new(1));
    }

    #[test]
    fn convert_window_event_pointer_button_pressed() {
        let scale = DpiScale::new(1.0);
        let event = WindowEvent::PointerButton {
            device_id: None,
            state: ElementState::Pressed,
            position: PhysicalPosition::new(42.0, 17.0),
            primary: true,
            button: ButtonSource::Mouse(WinitMouseButton::Left),
            is_macos_activation_click: false,
        };
        let pe = convert_window_event(&event, &scale).expect("pointer button converts");
        assert_eq!(pe.state, PointerState::Pressed);
        assert_eq!(pe.button, Some(MouseButton::Left));
        assert_eq!(pe.position, Vec2::new(42.0, 17.0));
        assert_eq!(pe.pointer_id, PointerId::PRIMARY);
        assert_eq!(pe.kind, PointerKind::Mouse);
    }

    #[test]
    fn convert_window_event_pointer_button_released() {
        let scale = DpiScale::new(1.0);
        let event = WindowEvent::PointerButton {
            device_id: None,
            state: ElementState::Released,
            position: PhysicalPosition::new(42.0, 17.0),
            primary: true,
            button: ButtonSource::Mouse(WinitMouseButton::Right),
            is_macos_activation_click: false,
        };
        let pe = convert_window_event(&event, &scale).expect("pointer button converts");
        assert_eq!(pe.state, PointerState::Released);
        assert_eq!(pe.button, Some(MouseButton::Right));
    }

    #[test]
    fn convert_window_event_pointer_button_touch() {
        let scale = DpiScale::new(1.0);
        let event = WindowEvent::PointerButton {
            device_id: None,
            state: ElementState::Pressed,
            position: PhysicalPosition::new(5.0, 5.0),
            primary: false,
            button: ButtonSource::Touch {
                finger_id: FingerId::from_raw(7),
                force: None,
            },
            is_macos_activation_click: false,
        };
        let pe = convert_window_event(&event, &scale).expect("touch button converts");
        assert_eq!(pe.pointer_id, PointerId::new(8));
        assert_eq!(pe.kind, PointerKind::Touch);
        assert_eq!(pe.button, Some(MouseButton::Left));
        assert_eq!(pe.state, PointerState::Pressed);
    }

    #[test]
    fn convert_window_event_non_pointer_returns_none() {
        let scale = DpiScale::new(1.0);
        assert!(convert_window_event(&WindowEvent::CloseRequested, &scale).is_none());
        assert!(convert_window_event(&WindowEvent::Destroyed, &scale).is_none());
    }

    #[test]
    fn convert_window_event_scales_coordinates() {
        let scale = DpiScale::new(1.5);
        let event = WindowEvent::PointerMoved {
            device_id: None,
            position: PhysicalPosition::new(150.0, 300.0),
            primary: true,
            source: PointerSource::Mouse,
        };
        let pe = convert_window_event(&event, &scale).expect("converts");
        // 150 / 1.5 = 100; 300 / 1.5 = 200.
        assert_eq!(pe.position, Vec2::new(100.0, 200.0));
    }

    // -----------------------------------------------------------------
    // Drop events (winit 0.31 DragEntered/DragPosition/DragDropped/DragLeft)
    // -----------------------------------------------------------------

    #[test]
    fn convert_drop_event_drag_entered() {
        let scale = DpiScale::new(2.0);
        let event = WindowEvent::DragEntered {
            id: winit::data_transfer::DataTransferId::from_raw(1),
            position: Some(PhysicalPosition::new(100.0, 200.0)),
        };
        let drop = convert_drop_event(&event, &scale).expect("entered converts");
        match drop {
            DropEvent::Entered { position, action } => {
                assert_eq!(position, Some(Vec2::new(50.0, 100.0)));
                assert_eq!(action, DropAction::None);
            }
            _ => panic!("expected Entered"),
        }
    }

    #[test]
    fn convert_drop_event_drag_entered_no_position() {
        let scale = DpiScale::new(1.0);
        let event = WindowEvent::DragEntered {
            id: winit::data_transfer::DataTransferId::from_raw(1),
            position: None,
        };
        let drop = convert_drop_event(&event, &scale).expect("entered converts");
        assert!(matches!(drop, DropEvent::Entered { position: None, .. }));
    }

    #[test]
    fn convert_drop_event_drag_position_maps_action() {
        let scale = DpiScale::new(1.0);
        let event = WindowEvent::DragPosition {
            id: winit::data_transfer::DataTransferId::from_raw(1),
            position: PhysicalPosition::new(10.0, 20.0),
            proposed_action: Some(winit::event_loop::DndAction::Move),
        };
        let drop = convert_drop_event(&event, &scale).expect("moved converts");
        assert_eq!(
            drop,
            DropEvent::Moved {
                position: Vec2::new(10.0, 20.0),
                action: DropAction::Move,
            }
        );
    }

    #[test]
    fn convert_drop_event_drag_dropped() {
        let scale = DpiScale::new(1.0);
        let event = WindowEvent::DragDropped {
            id: winit::data_transfer::DataTransferId::from_raw(1),
            proposed_action: Some(winit::event_loop::DndAction::Copy),
        };
        let drop = convert_drop_event(&event, &scale).expect("dropped converts");
        assert_eq!(
            drop,
            DropEvent::Dropped {
                action: DropAction::Copy
            }
        );
    }

    #[test]
    fn convert_drop_event_drag_left() {
        let scale = DpiScale::new(1.0);
        let event = WindowEvent::DragLeft {
            id: winit::data_transfer::DataTransferId::from_raw(1),
        };
        let drop = convert_drop_event(&event, &scale).expect("left converts");
        assert_eq!(drop, DropEvent::Left);
    }

    #[test]
    fn convert_drop_event_non_drag_returns_none() {
        let scale = DpiScale::new(1.0);
        assert!(convert_drop_event(&WindowEvent::CloseRequested, &scale).is_none());
        assert!(convert_drop_event(&WindowEvent::Destroyed, &scale).is_none());
        assert!(convert_drop_event(&WindowEvent::RedrawRequested, &scale).is_none());
    }

    #[test]
    fn convert_drop_event_scales_coordinates() {
        let scale = DpiScale::new(4.0);
        let event = WindowEvent::DragPosition {
            id: winit::data_transfer::DataTransferId::from_raw(1),
            position: PhysicalPosition::new(400.0, 800.0),
            proposed_action: None,
        };
        let drop = convert_drop_event(&event, &scale).expect("converts");
        assert_eq!(drop.position(), Some(Vec2::new(100.0, 200.0)));
    }

    #[test]
    fn drop_event_position_accessor() {
        let entered = DropEvent::Entered {
            position: Some(Vec2::new(1.0, 2.0)),
            action: DropAction::None,
        };
        assert_eq!(entered.position(), Some(Vec2::new(1.0, 2.0)));
        let moved = DropEvent::Moved {
            position: Vec2::new(3.0, 4.0),
            action: DropAction::Copy,
        };
        assert_eq!(moved.position(), Some(Vec2::new(3.0, 4.0)));
        let dropped = DropEvent::Dropped {
            action: DropAction::Copy,
        };
        assert!(dropped.position().is_none());
        assert!(DropEvent::Left.position().is_none());
    }

    #[test]
    fn drop_action_default_is_none() {
        assert_eq!(DropAction::default(), DropAction::None);
    }

    #[test]
    fn drop_action_variants_distinct() {
        assert_ne!(DropAction::None, DropAction::Copy);
        assert_ne!(DropAction::Copy, DropAction::Move);
        assert_ne!(DropAction::Move, DropAction::Link);
        assert_ne!(DropAction::Link, DropAction::Ask);
        assert_ne!(DropAction::Ask, DropAction::Private);
    }

    #[test]
    fn drop_action_from_winit_unknown_maps_to_none() {
        // `proposed_action: None` maps to `DropAction::None`.
        let scale = DpiScale::new(1.0);
        let event = WindowEvent::DragPosition {
            id: winit::data_transfer::DataTransferId::from_raw(1),
            position: PhysicalPosition::new(0.0, 0.0),
            proposed_action: None,
        };
        let drop = convert_drop_event(&event, &scale).expect("converts");
        assert_eq!(
            drop,
            DropEvent::Moved {
                position: Vec2::ZERO,
                action: DropAction::None,
            }
        );
    }

    // -----------------------------------------------------------------
    // Multi-window
    // -----------------------------------------------------------------

    #[test]
    fn multi_window_events_routed_to_correct_window() {
        // Two independent arenas model two windows' widget trees. The router
        // is shared; the caller selects the root per window.
        let mut arena_a = WidgetArena::new();
        let root_a = insert(&mut arena_a, 0.0, 0.0, 100.0, 100.0);

        let mut arena_b = WidgetArena::new();
        let root_b = insert(&mut arena_b, 0.0, 0.0, 100.0, 100.0);

        let w1 = WindowId::from_raw(1);
        let w2 = WindowId::from_raw(2);
        let mut router = EventRouter::new();

        let event = PointerEvent {
            pointer_id: PointerId::PRIMARY,
            kind: PointerKind::Mouse,
            position: Vec2::new(50.0, 50.0),
            state: PointerState::Moved,
            button: None,
            modifiers: ModifierKeys::empty(),
        };

        assert_eq!(
            router.route_pointer_event(&arena_a, root_a, w1, &event),
            EventDispatchOutcome::Handled(root_a)
        );
        assert_eq!(
            router.route_pointer_event(&arena_b, root_b, w2, &event),
            EventDispatchOutcome::Handled(root_b)
        );

        // `route_pointer_event` keeps the per-window mouse position in sync.
        assert_eq!(
            router.mouse_tracker().position(w1),
            Some(Vec2::new(50.0, 50.0))
        );
        assert_eq!(
            router.mouse_tracker().position(w2),
            Some(Vec2::new(50.0, 50.0))
        );

        // Mouse tracker keeps per-window state distinct when updated directly.
        router
            .mouse_tracker_mut()
            .update_position(w1, Vec2::new(1.0, 2.0));
        router
            .mouse_tracker_mut()
            .update_position(w2, Vec2::new(3.0, 4.0));
        assert_eq!(
            router.mouse_tracker().position(w1),
            Some(Vec2::new(1.0, 2.0))
        );
        assert_eq!(
            router.mouse_tracker().position(w2),
            Some(Vec2::new(3.0, 4.0))
        );
    }

    // -----------------------------------------------------------------
    // Edge cases
    // -----------------------------------------------------------------

    #[test]
    fn empty_arena_dead_root_returns_unhandled() {
        let arena = WidgetArena::new();
        let win = WindowId::from_raw(1);
        let mut router = EventRouter::new();
        let dead = WidgetId::from_parts(999, 1);
        let event = PointerEvent {
            pointer_id: PointerId::PRIMARY,
            kind: PointerKind::Mouse,
            position: Vec2::new(50.0, 50.0),
            state: PointerState::Moved,
            button: None,
            modifiers: ModifierKeys::empty(),
        };
        assert_eq!(
            router.route_pointer_event(&arena, dead, win, &event),
            EventDispatchOutcome::Unhandled
        );
    }

    #[test]
    fn root_only_tree_hit_inside_and_outside() {
        let mut arena = WidgetArena::new();
        let root = insert(&mut arena, 0.0, 0.0, 100.0, 100.0);
        let win = WindowId::from_raw(1);
        let mut router = EventRouter::new();

        let inside = PointerEvent {
            pointer_id: PointerId::PRIMARY,
            kind: PointerKind::Mouse,
            position: Vec2::new(50.0, 50.0),
            state: PointerState::Moved,
            button: None,
            modifiers: ModifierKeys::empty(),
        };
        assert_eq!(
            router.route_pointer_event(&arena, root, win, &inside),
            EventDispatchOutcome::Handled(root)
        );

        let outside = PointerEvent {
            pointer_id: PointerId::PRIMARY,
            kind: PointerKind::Mouse,
            position: Vec2::new(500.0, 500.0),
            state: PointerState::Moved,
            button: None,
            modifiers: ModifierKeys::empty(),
        };
        assert_eq!(
            router.route_pointer_event(&arena, root, win, &outside),
            EventDispatchOutcome::Unhandled
        );
    }

    #[test]
    fn point_outside_all_widgets_returns_unhandled() {
        let mut arena = WidgetArena::new();
        let root = insert(&mut arena, 0.0, 0.0, 100.0, 100.0);
        let other = insert(&mut arena, 200.0, 200.0, 100.0, 100.0);
        arena.append_child(root, other).unwrap();

        let win = WindowId::from_raw(1);
        let mut router = EventRouter::new();
        let event = PointerEvent {
            pointer_id: PointerId::PRIMARY,
            kind: PointerKind::Mouse,
            position: Vec2::new(150.0, 150.0),
            state: PointerState::Moved,
            button: None,
            modifiers: ModifierKeys::empty(),
        };
        assert_eq!(
            router.route_pointer_event(&arena, root, win, &event),
            EventDispatchOutcome::Unhandled
        );
    }

    #[test]
    fn route_pointer_event_updates_hovered_widget() {
        let mut arena = WidgetArena::new();
        let root = insert(&mut arena, 0.0, 0.0, 200.0, 200.0);
        let child = insert(&mut arena, 50.0, 50.0, 100.0, 100.0);
        arena.append_child(root, child).unwrap();

        let win = WindowId::from_raw(1);
        let mut router = EventRouter::new();

        let over_child = PointerEvent {
            pointer_id: PointerId::PRIMARY,
            kind: PointerKind::Mouse,
            position: Vec2::new(75.0, 75.0),
            state: PointerState::Moved,
            button: None,
            modifiers: ModifierKeys::empty(),
        };
        router.route_pointer_event(&arena, root, win, &over_child);
        assert_eq!(router.mouse_tracker().hovered_widget(win), Some(child));

        // A miss clears the hovered widget for that window.
        let miss = PointerEvent {
            pointer_id: PointerId::PRIMARY,
            kind: PointerKind::Mouse,
            position: Vec2::new(500.0, 500.0),
            state: PointerState::Moved,
            button: None,
            modifiers: ModifierKeys::empty(),
        };
        router.route_pointer_event(&arena, root, win, &miss);
        assert_eq!(router.mouse_tracker().hovered_widget(win), None);
    }

    #[test]
    fn event_dispatch_outcome_variants_distinct() {
        let w = WidgetId::from_parts(1, 1);
        assert_ne!(
            EventDispatchOutcome::Handled(w),
            EventDispatchOutcome::Unhandled
        );
        assert_ne!(
            EventDispatchOutcome::Unhandled,
            EventDispatchOutcome::Ignored
        );
        assert_eq!(
            EventDispatchOutcome::Handled(w),
            EventDispatchOutcome::Handled(w)
        );
    }

    #[test]
    fn pointer_id_primary_is_zero() {
        assert_eq!(PointerId::PRIMARY, PointerId::new(0));
        assert_eq!(PointerId::PRIMARY.get(), 0);
    }
}
