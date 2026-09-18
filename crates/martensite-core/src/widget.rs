use crate::node::{HotNode, Rect};
use crate::overlay::OverlayLayer;
use accesskit::Node as AccessKitNode;
use glam::Vec2;
use std::time::Duration;

/// Minimum and maximum size bounds for layout measurement.
///
/// # Examples
///
/// ```
/// use glam::Vec2;
/// use martensite_core::LayoutConstraints;
///
/// let constraints = LayoutConstraints {
///     min_size: Vec2::new(0.0, 0.0),
///     max_size: Vec2::new(200.0, 100.0),
/// };
/// assert_eq!(constraints.min_size, Vec2::ZERO);
/// assert_eq!(constraints.max_size.x, 200.0);
/// ```
#[derive(Copy, Clone, Debug, PartialEq)]
pub struct LayoutConstraints {
    /// Minimum acceptable size.
    pub min_size: Vec2,
    /// Maximum acceptable size.
    pub max_size: Vec2,
}

/// How the framework treats a widget whose allocated bounds fall below
/// its declared [`RenderMinimum`].
///
/// The policy is evaluated lazily at the paint, hit-test, and audit
/// consumption sites — never by a mandatory layout pass — so it applies
/// regardless of which layout authority produced the rect (the Taffy
/// engine, a docking BSP, or manual bounds assignment). Engagement is
/// hysteretic: a node engages when either axis drops below its minimum
/// and releases only when both axes recover past the minimum scaled by
/// [`crate::WidgetArena::UNDERFLOW_RELEASE`], preventing flicker during
/// live resize.
///
/// # Examples
///
/// ```
/// use martensite_core::UnderflowPolicy;
///
/// // `Allow` is the default — the pre-feature behavior, made explicit.
/// assert_eq!(UnderflowPolicy::default(), UnderflowPolicy::Allow);
/// // `Hide` and `Scrim` cover the subtree from input; `Fallback` does not.
/// assert!(UnderflowPolicy::Hide.covers_input());
/// assert!(!UnderflowPolicy::Fallback.covers_input());
/// ```
#[derive(Copy, Clone, Debug, Default, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum UnderflowPolicy {
    /// Paint and hit-test normally — the pre-feature behavior, made
    /// explicit. The `audit_underflow` pass in `martensite-access`
    /// still reports the violation when the app runs it.
    #[default]
    Allow,
    /// Render and behave exactly like [`UnderflowPolicy::Allow`], but
    /// flag the violation whenever the underflow audit runs — the
    /// declaration says "a minimum breach here is a real problem, not
    /// a style choice". Use for widgets whose minimum is a hard
    /// correctness requirement.
    Lint,
    /// Clip the widget's chrome and its entire subtree to its allocated
    /// bounds. Slightly stricter than the status quo, which clips only
    /// children of nodes carrying `CLIPS_CHILDREN`.
    Clip,
    /// Keep the layout slot but skip painting, hit-testing, and event
    /// delivery — Android `INVISIBLE` semantics, not `GONE`: no
    /// relayout is requested and the space stays allocated. The node is
    /// marked `hidden` in the accessibility tree and focus is relocated
    /// away before it engages.
    Hide,
    /// Paint normally, then veil the widget's bounds with a blurred,
    /// translucent scrim and cover the subtree from input. The scrim is
    /// a frosted overlay — it does **not** blur the widget's own
    /// painted output (true content blur requires render-target
    /// machinery deferred to a later `ContentBlur` variant).
    Scrim,
    /// Skip the normal paint and let the widget draw a degraded
    /// representation via [`Widget::paint_underflow`] — a "window too
    /// small" badge, a sparkline, an icon. Input stays live: the
    /// fallback chrome may itself be interactive.
    Fallback,
    /// Remove the node from layout entirely — CSS `display:none`
    /// semantics. The space is freed and siblings reflow. Requires a
    /// layout authority that honours per-node styles (the Taffy engine
    /// applies `display:none` and recomputes once per frame); on
    /// manual-layout paths it degrades to [`UnderflowPolicy::Hide`]
    /// semantics. Restore is speculative — the node returns only when
    /// the freed space could satisfy its floor, with a feasibility
    /// streak that prevents collapse/restore oscillation.
    Collapse,
}

impl UnderflowPolicy {
    /// Whether the engaged policy covers the widget's subtree from
    /// input — pointer, scroll, keyboard, and focus all bypass it.
    pub fn covers_input(self) -> bool {
        matches!(self, Self::Hide | Self::Collapse | Self::Scrim)
    }

    /// Whether the engaged policy marks the node `hidden` in the
    /// accessibility tree.
    pub fn hides_from_a11y(self) -> bool {
        matches!(self, Self::Hide | Self::Collapse)
    }

    /// Whether the policy enforces anything at consumption time.
    /// `Allow` and `Lint` are advisory — they never alter behavior.
    pub fn enforces(self) -> bool {
        !matches!(self, Self::Allow | Self::Lint)
    }
}

/// A widget's declared minimum render area and the policy applied on
/// shortfall.
///
/// `size` is in logical points — the same unit [`Widget::measure`]
/// reports. `RenderMinimum::ZERO` disables underflow handling entirely
/// (the default). Declare it via [`Widget::min_render`] for a widget's
/// intrinsic floor, or per arena node via
/// [`crate::ColdNode::with_render_minimum`] — the instance override
/// wins, mirroring `debug_name` precedence.
///
/// # Examples
///
/// ```
/// use glam::Vec2;
/// use martensite_core::{RenderMinimum, UnderflowPolicy};
///
/// let min = RenderMinimum::new(Vec2::new(120.0, 24.0))
///     .with_policy(UnderflowPolicy::Clip);
/// assert!(min.policy.enforces());
/// assert_eq!(RenderMinimum::ZERO.size, Vec2::ZERO);
/// ```
#[derive(Copy, Clone, Debug, Default, PartialEq)]
pub struct RenderMinimum {
    /// Minimum meaningful render size in logical points.
    pub size: Vec2,
    /// Behavior when allocated bounds are smaller than `size`.
    pub policy: UnderflowPolicy,
}

impl RenderMinimum {
    /// No declared minimum — underflow handling is disabled.
    pub const ZERO: Self = Self {
        size: Vec2::ZERO,
        policy: UnderflowPolicy::Allow,
    };

    /// Declare a minimum render size with the default
    /// [`UnderflowPolicy::Allow`] policy.
    pub const fn new(size: Vec2) -> Self {
        Self {
            size,
            policy: UnderflowPolicy::Allow,
        }
    }

    /// Set the shortfall policy.
    #[must_use]
    pub const fn with_policy(self, policy: UnderflowPolicy) -> Self {
        Self { policy, ..self }
    }

    /// Whether `bounds` (device pixels) underflows this minimum under
    /// the given display `scale` (physical px per logical pt).
    pub(crate) fn violated(self, bounds: Rect, scale: f32) -> bool {
        let min = self.size * scale;
        bounds.size.x < min.x || bounds.size.y < min.y
    }

    /// Whether `bounds` (device pixels) satisfies this minimum scaled
    /// by `margin` — `1.0` is an exact fit, `>1.0` adds release
    /// hysteresis.
    pub(crate) fn satisfied(self, bounds: Rect, scale: f32, margin: f32) -> bool {
        let min = self.size * scale * margin;
        bounds.size.x >= min.x && bounds.size.y >= min.y
    }
}

/// Result of processing an input event on a widget.
///
/// # Examples
///
/// ```
/// use martensite_core::EventResponse;
///
/// // `Ignored` propagates; `Handled` stops propagation.
/// assert_ne!(EventResponse::Ignored, EventResponse::Handled);
/// assert_eq!(EventResponse::Ignored, EventResponse::Ignored);
///
/// // A widget can request focus or a repaint after handling an event.
/// let focus = EventResponse::CaptureFocus;
/// let repaint = EventResponse::RequestRepaint;
/// assert_ne!(focus, repaint);
/// ```
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum EventResponse {
    /// The event was not handled and should propagate.
    Ignored,
    /// The event was handled and should not propagate.
    Handled,
    /// The event was handled and this widget requests keyboard focus.
    ///
    /// When this response propagates out of
    /// [`WidgetArena::dispatch_event`](crate::WidgetArena::dispatch_event),
    /// the arena records the responding widget as a *pending focus
    /// request* — drain it with
    /// [`WidgetArena::take_focus_request`](crate::WidgetArena::take_focus_request)
    /// (or
    /// [`EventRouter::take_focus_request`](https://docs.rs/martensite-window)
    /// when routing through `martensite-window`) and apply it through
    /// `martensite-focus`'s `FocusManager`. Focus only lands if the
    /// responding node carries
    /// [`NodeFlags::FOCUSABLE`](crate::NodeFlags::FOCUSABLE); widgets declare
    /// focusability by setting that flag on `cx.hot.flags` inside
    /// [`Widget::layout`]. The responder is also marked `DIRTY_PAINT`.
    ///
    /// Additionally, any `PointerPressed` that is handled by a
    /// `FOCUSABLE` node requests focus implicitly — press-to-focus is
    /// automatic and needs no explicit `CaptureFocus` return.
    CaptureFocus,
    /// The event was handled and a repaint is requested.
    RequestRepaint,
    /// The event was handled and this widget requests pointer capture:
    /// every subsequent event for the pointer that produced this event
    /// is routed to this widget regardless of hit-testing, until the
    /// widget answers with [`EventResponse::ReleasePointer`] (or the
    /// widget is removed from the arena). Used for drag interactions
    /// such as slider thumbs and scroll thumbs that must keep tracking
    /// the pointer outside the widget's bounds.
    ///
    /// The capture is applied by `martensite-window`'s `EventRouter`
    /// when the response propagates out of
    /// [`WidgetArena::dispatch_event`](crate::WidgetArena::dispatch_event);
    /// the responding node is also marked `DIRTY_PAINT` because a
    /// grab/release always changes visual state.
    CapturePointer,
    /// The event was handled and this widget releases its pointer
    /// capture for the pointer that produced this event. See
    /// [`EventResponse::CapturePointer`].
    ReleasePointer,
}

/// Context provided to widgets during the layout pass.
pub struct LayoutContext<'a> {
    /// Mutable access to the hot node being laid out.
    pub hot: &'a mut HotNode,
    /// Physical pixels per logical point — the same factor
    /// [`PaintContext::scale`] carries. Widgets reporting baked
    /// logical-point minimum sizes from [`Widget::measure`] should
    /// multiply them by this so HiDPI minimums stay honest.
    pub scale: f32,
}

impl LayoutContext<'_> {
    /// Converts a logical-point size to this context's coordinate space.
    pub fn pt(&self, v: f32) -> f32 {
        v * self.scale
    }

    /// Lays out an internal child widget, preserving this node's
    /// [`NodeFlags::FOCUSABLE`](crate::NodeFlags::FOCUSABLE) flag.
    ///
    /// Internal children share the parent's `cx.hot`, so a child that
    /// clears the flag (e.g. a disabled `Button` running
    /// `flags.remove(FOCUSABLE)`) would otherwise clobber a flag the
    /// parent or an earlier sibling set. Focusability is union
    /// semantics: a node is focusable if the parent *or any* enabled
    /// child is — children may set the flag, only this node's own
    /// `layout` may clear it.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite_core::{DummyWidget, HotNode, LayoutContext, Rect};
    ///
    /// let mut child = DummyWidget;
    /// let mut hot = HotNode::default();
    /// hot.flags |= martensite_core::NodeFlags::FOCUSABLE;
    /// let mut cx = LayoutContext {
    ///     hot: &mut hot,
    ///     scale: 1.0,
    /// };
    /// cx.layout_child(&mut child, Rect::new(0.0, 0.0, 50.0, 20.0));
    /// ```
    pub fn layout_child(&mut self, child: &mut dyn Widget, bounds: Rect) {
        let had = self.hot.flags.contains(crate::NodeFlags::FOCUSABLE);
        child.layout(self, bounds);
        if had {
            self.hot.flags |= crate::NodeFlags::FOCUSABLE;
        }
    }
}

/// A normalized, framework-level input event delivered to widgets.
///
/// `martensite-window`'s [`EventRouter`] converts raw `winit` events into
/// these vocabulary types before dispatch. Widgets match on the variant to
/// implement interaction.
///
/// # Examples
///
/// ```
/// use glam::Vec2;
/// use martensite_core::{PointerButton, WidgetEvent};
///
/// let press = WidgetEvent::PointerPressed {
///     position: Vec2::new(10.0, 20.0),
///     button: PointerButton::Primary,
/// };
/// assert!(matches!(press, WidgetEvent::PointerPressed { .. }));
/// ```
///
/// [`EventRouter`]: https://docs.rs/martensite-window
#[derive(Clone, Debug, PartialEq)]
#[non_exhaustive]
pub enum WidgetEvent {
    /// The pointer moved over the widget's bounds.
    PointerMoved {
        /// Window-space position, in the same device-pixel space as
        /// [`EventContext::bounds`].
        position: Vec2,
    },
    /// A pointer button was pressed inside the widget's bounds.
    PointerPressed {
        /// Window-space position, in the same device-pixel space as
        /// [`EventContext::bounds`].
        position: Vec2,
        /// Which button was pressed.
        button: PointerButton,
    },
    /// A pointer button was released.
    PointerReleased {
        /// Window-space position, in the same device-pixel space as
        /// [`EventContext::bounds`].
        position: Vec2,
        /// Which button was released.
        button: PointerButton,
    },
    /// A scroll gesture occurred over the widget's bounds.
    Scroll {
        /// Window-space position, in the same device-pixel space as
        /// [`EventContext::bounds`].
        position: Vec2,
        /// Scroll delta in the same space as `position` (positive = content up/right).
        delta: Vec2,
    },
    /// A key was pressed while the widget held focus.
    KeyPressed {
        /// The logical key name (e.g. `"Enter"`, `"a"`, `"Space"`).
        key: String,
        /// Whether this is an OS auto-repeat event.
        repeat: bool,
    },
    /// A key was released while the widget held focus.
    KeyReleased {
        /// The logical key name.
        key: String,
    },
    /// An IME composition committed text to the focused widget.
    ImeCommitted {
        /// The committed text.
        text: String,
    },
    /// The pointer entered the widget's bounds (hover begin). Sent to
    /// the previously-unhovered widget when the hovered hit-test
    /// target changes, paired with [`PointerLeave`](Self::PointerLeave)
    /// on the old target.
    PointerEnter,
    /// The pointer left the widget's bounds (hover end). Unlike
    /// `PointerMoved` — which stops arriving once the pointer exits —
    /// this is dispatched on the *transition*, letting widgets such as
    /// `Tooltip` detect hover exit.
    PointerLeave,
    /// The widget gained keyboard focus.
    FocusGained,
    /// The widget lost keyboard focus.
    FocusLost,
    /// An assistive-technology action delivered to the widget — the
    /// widget-space form of an AccessKit `ActionRequest`, decoded by
    /// `martensite-access` into [`SemanticAction`] and dispatched
    /// through the normal event pipeline.
    SemanticAction(SemanticAction),
}

/// A semantic action an assistive technology requests of a widget.
///
/// Mirrors the `accesskit::Action` vocabulary in a `martensite-core`
/// type so widgets handle AT actions through [`Widget::event`] like
/// any other input, without core depending on the access adapter.
///
/// # Examples
///
/// ```
/// use martensite_core::SemanticAction;
///
/// let action = SemanticAction::Increment;
/// assert_ne!(action, SemanticAction::Decrement);
/// ```
#[derive(Clone, Debug, PartialEq)]
#[non_exhaustive]
pub enum SemanticAction {
    /// Activate the widget (`accesskit::Action::Click`).
    Click,
    /// Move keyboard focus to the widget.
    ///
    /// A widget that honours AT focus requests returns
    /// [`EventResponse::CaptureFocus`]; the arena then records a pending
    /// focus request the app drains via
    /// [`WidgetArena::take_focus_request`](crate::WidgetArena::take_focus_request)
    /// and applies through `martensite-focus`'s `FocusManager`.
    Focus,
    /// Remove keyboard focus from the widget.
    ///
    /// Widgets cannot unilaterally clear focus — the app maps
    /// `A11yAction::Blur` to `FocusManager::clear_focus`, which
    /// dispatches [`WidgetEvent::FocusLost`] to the old target. A widget
    /// may answer `Blur` with [`EventResponse::Ignored`].
    Blur,
    /// Set the widget's value; carries the value as text — numeric
    /// widgets parse it, matching `A11yAction::SetValue`.
    SetValue(String),
    /// Increment the widget's value by one step.
    Increment,
    /// Decrement the widget's value by one step.
    Decrement,
    /// Expand a collapsible widget (e.g. open a combobox popup).
    Expand,
    /// Collapse an expanded widget.
    Collapse,
    /// Show the widget's tooltip.
    ShowTooltip,
    /// Hide the widget's tooltip.
    HideTooltip,
    /// Show the widget's context menu.
    ShowContextMenu,
    /// Scroll up by one unit (`accesskit::Action::ScrollUp`).
    ScrollUp,
    /// Scroll down by one unit.
    ScrollDown,
    /// Scroll left by one unit.
    ScrollLeft,
    /// Scroll right by one unit.
    ScrollRight,
    /// Scroll so this widget becomes visible in its scrollable
    /// ancestor(s) (`accesskit::Action::ScrollIntoView`).
    ScrollIntoView,
    /// Scroll the widget so the given point (in its own coordinate
    /// space) is visible (`accesskit::Action::ScrollToPoint`).
    ScrollToPoint(Vec2),
    /// Set the scroll offset directly
    /// (`accesskit::Action::SetScrollOffset` with
    /// `ActionData::SetScrollOffset`).
    SetScrollOffset(Vec2),
}

impl WidgetEvent {
    /// Returns the window-space position for positional events, or `None`
    /// for keyboard/IME/focus events.
    ///
    /// # Examples
    ///
    /// ```
    /// use glam::Vec2;
    /// use martensite_core::{PointerButton, WidgetEvent};
    ///
    /// let p = WidgetEvent::PointerPressed {
    ///     position: Vec2::new(5.0, 5.0),
    ///     button: PointerButton::Primary,
    /// };
    /// assert_eq!(p.position(), Some(Vec2::new(5.0, 5.0)));
    /// assert_eq!(WidgetEvent::FocusGained.position(), None);
    /// ```
    pub fn position(&self) -> Option<Vec2> {
        match self {
            Self::PointerMoved { position }
            | Self::PointerPressed { position, .. }
            | Self::PointerReleased { position, .. }
            | Self::Scroll { position, .. } => Some(*position),
            _ => None,
        }
    }
}

/// A pointer (mouse / touch / stylus) button.
///
/// # Examples
///
/// ```
/// use martensite_core::PointerButton;
///
/// assert_eq!(PointerButton::Primary, PointerButton::Primary);
/// assert_ne!(PointerButton::Primary, PointerButton::Secondary);
/// ```
#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum PointerButton {
    /// The primary button (usually left mouse / touch contact).
    Primary,
    /// The secondary button (usually right mouse).
    Secondary,
    /// The middle button (usually wheel click).
    Middle,
    /// The browser-back button.
    Back,
    /// The browser-forward button.
    Forward,
    /// Any other platform-specific button, by raw code.
    Other(u16),
}

/// Context provided to widgets during event processing.
///
/// Carries the normalized [`WidgetEvent`] and the widget's screen-space
/// bounds so position-relative logic (e.g. "was the press in my left
/// half?") doesn't require a second arena lookup.
///
/// # Examples
///
/// ```
/// use glam::Vec2;
/// use martensite_core::{EventContext, Rect, WidgetEvent};
///
/// let event = WidgetEvent::FocusGained;
/// let cx = EventContext {
///     event: &event,
///     bounds: Rect::new(0.0, 0.0, 100.0, 30.0),
///     scale: 1.0,
/// };
/// assert_eq!(cx.bounds.width(), 100.0);
/// ```
pub struct EventContext<'a> {
    /// The event being delivered.
    pub event: &'a WidgetEvent,
    /// The widget's screen-space bounds in device pixels (equal to
    /// logical points at `scale == 1.0`).
    pub bounds: Rect,
    /// Physical pixels per logical point — needed to evaluate declared
    /// [`RenderMinimum`] sizes (logical pt) against `bounds` (device
    /// px) when forwarding events to internal children.
    pub scale: f32,
}

/// Context provided to widgets during the paint pass.
///
/// Widgets record drawing operations into [`PaintContext::list`]; the
/// commands are clipped to [`PaintContext::bounds`] by the caller's clip
/// stack discipline.
///
/// # Examples
///
/// ```
/// use martensite_core::{PaintContext, PaintList, Rect, Theme};
///
/// let mut list = PaintList::new();
/// let theme = Theme::new("fallback");
/// {
///     let mut cx = PaintContext {
///         list: &mut list,
///         bounds: Rect::new(0.0, 0.0, 50.0, 20.0),
///         theme: &theme,
///         scale: 1.0,
///         text_painter: None,
///     };
///     cx.list.push_fill_rect(
///         kurbo::Rect::new(0.0, 0.0, 50.0, 20.0),
///         [30, 30, 30, 255],
///     );
/// }
/// assert_eq!(list.len(), 1);
/// ```
pub struct PaintContext<'a> {
    /// The command list to record this widget's output into.
    pub list: &'a mut crate::paint::PaintList,
    /// The widget's screen-space bounds in device pixels (equal to
    /// logical points at `scale == 1.0`).
    pub bounds: Rect,
    /// The active design-token theme for this paint pass. Widgets
    /// resolve [`martensite_theme::TokenKey`]s through it and should
    /// fall back to their baked defaults when a token is absent — an
    /// empty theme reproduces the widget's unthemed appearance.
    ///
    /// The arena supplies this from
    /// [`WidgetArena::theme`](crate::WidgetArena::theme); applications
    /// change it via
    /// [`WidgetArena::set_theme`](crate::WidgetArena::set_theme).
    pub theme: &'a martensite_theme::Theme,
    /// The display scale factor (physical px per logical pt) reported
    /// to the arena via
    /// [`WidgetArena::set_scale_factor`](crate::WidgetArena::set_scale_factor).
    /// Defaults to `1.0`. Widgets that bake sizes in logical points —
    /// font sizes, paddings, hit-target minimums — multiply them by
    /// this (or call [`PaintContext::pt`]) so the emitted device-pixel
    /// geometry stays the intended physical size on HiDPI displays.
    /// Arenas whose bounds are already logical pixels leave it at `1.0`.
    pub scale: f32,
    /// The ambient shaped-text painter installed via
    /// [`WidgetArena::set_text_painter`](crate::WidgetArena::set_text_painter),
    /// if any. Widgets that emit text should prefer an explicit
    /// painter of their own when set, then this ambient one, and only
    /// fall back to [`PaintList::push_text`](crate::PaintList::push_text)'s
    /// placeholder boxes when neither exists.
    pub text_painter: Option<&'a (dyn crate::paint::TextShaper + Send + Sync)>,
}

impl PaintContext<'_> {
    /// Converts a logical-point size to the paint pass's device-pixel
    /// size — `pt * scale`. Use for every baked constant a widget emits
    /// (font sizes, corner radii, paddings) so HiDPI arenas stay
    /// legible.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite_core::{PaintContext, PaintList, Rect, Theme};
    ///
    /// let mut list = PaintList::new();
    /// let theme = Theme::new("fallback");
    /// let cx = PaintContext {
    ///     list: &mut list,
    ///     bounds: Rect::new(0.0, 0.0, 10.0, 10.0),
    ///     theme: &theme,
    ///     scale: 2.0,
    ///     text_painter: None,
    /// };
    /// assert_eq!(cx.pt(14.0), 28.0);
    /// ```
    pub fn pt(&self, v: f32) -> f32 {
        v * self.scale
    }

    /// `f64` variant of [`PaintContext::pt`].
    pub fn ptf(&self, v: f64) -> f64 {
        v * f64::from(self.scale)
    }
    /// Resolves a color [`martensite_theme::TokenKey`] to `[u8; 4]`
    /// sRGBA, or `fallback` when the token is absent or not a color.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite_core::{PaintContext, PaintList, Rect, Theme};
    /// use martensite_theme::TokenKey;
    ///
    /// let mut list = PaintList::new();
    /// let theme = Theme::new("fallback");
    /// let cx = PaintContext {
    ///     list: &mut list,
    ///     bounds: Rect::new(0.0, 0.0, 10.0, 10.0),
    ///     theme: &theme,
    ///     scale: 1.0,
    ///     text_painter: None,
    /// };
    /// assert_eq!(cx.color(TokenKey::BackgroundColor, [9, 9, 9, 255]), [9, 9, 9, 255]);
    /// ```
    pub fn color(&self, key: martensite_theme::TokenKey, fallback: [u8; 4]) -> [u8; 4] {
        self.theme.color(key).map_or(fallback, |c| c.to_srgba8())
    }
}

/// Context provided to widgets during accessibility tree construction.
pub struct AccessibilityContext {}

/// A descendant AccessKit node emitted for a widget during tree
/// construction, handed to [`Widget::a11y_fixup`] so widgets can wire
/// relations that require the children's minted `NodeId`s —
/// `aria-controls` and `aria-describedby` cannot be expressed inside
/// [`Widget::accessibility`] because the descendant ids do not exist
/// yet at that point.
///
/// # Examples
///
/// ```
/// use martensite_core::A11yEmittedNode;
///
/// // Constructed by the adapter; widgets read `path` and `id`, and
/// // mutate `node`.
/// let emitted = A11yEmittedNode {
///     path: vec![0],
///     id: accesskit::NodeId(7),
///     node: accesskit::Node::new(accesskit::Role::Unknown),
/// };
/// assert_eq!(emitted.path, vec![0]);
/// ```
pub struct A11yEmittedNode {
    /// The chain of [`Widget::child`] indices that reaches this node
    /// within the widget's internal subtree.
    pub path: Vec<u32>,
    /// The AccessKit `NodeId` minted for this node.
    pub id: accesskit::NodeId,
    /// The emitted node, patchable in place.
    pub node: AccessKitNode,
}

/// A read-only reference to a node emitted for an
/// [`OverlayLayer`] popup subtree, handed
/// to [`Widget::a11y_fixup`] so a widget can resolve the `NodeId`s of
/// popups it opened — e.g. a combobox wiring
/// `aria-activedescendant` to one of its listbox options.
///
/// # Examples
///
/// ```
/// use martensite_core::OverlayA11yRef;
///
/// let r = OverlayA11yRef {
///     entry: 3,
///     path: vec![2],
///     id: accesskit::NodeId(9),
/// };
/// assert_eq!(r.entry, 3);
/// ```
pub struct OverlayA11yRef {
    /// The overlay entry id, as returned by
    /// [`OverlayLayer::open`](crate::overlay::OverlayLayer::open).
    pub entry: u64,
    /// The chain of [`Widget::child`] indices reaching the node inside
    /// the popup's internal widget tree; empty for the popup root.
    pub path: Vec<u32>,
    /// The AccessKit `NodeId` minted for this node.
    pub id: accesskit::NodeId,
}

/// Core widget trait that all Martensite UI components implement.
///
/// Widgets are stored in the [`WidgetArena`](crate::WidgetArena) and receive
/// layout, paint, event, and accessibility callbacks from the framework.
///
/// # Examples
///
/// A minimal widget that reports a fixed desired size and ignores events:
///
/// ```
/// use glam::Vec2;
/// use martensite_core::{
///     EventContext, EventResponse, LayoutConstraints, LayoutContext, PaintContext, Rect, Widget,
/// };
///
/// struct FixedSize(Vec2);
///
/// impl Widget for FixedSize {
///     fn measure(&mut self, _cx: &mut LayoutContext, _c: LayoutConstraints) -> Vec2 {
///         self.0
///     }
///     fn layout(&mut self, _cx: &mut LayoutContext, _bounds: Rect) {}
/// }
///
/// let mut w = FixedSize(Vec2::new(50.0, 25.0));
/// // The default `event` implementation ignores input.
/// let event = martensite_core::WidgetEvent::FocusGained;
/// let mut cx = EventContext {
///     event: &event,
///     bounds: Rect::new(0.0, 0.0, 50.0, 25.0),
///     scale: 1.0,
/// };
/// assert_eq!(w.event(&mut cx), EventResponse::Ignored);
/// ```
pub trait Widget: Send + Sync + 'static {
    /// Measure the widget's desired size given layout constraints.
    fn measure(&mut self, cx: &mut LayoutContext, constraints: LayoutConstraints) -> Vec2;
    /// Position the widget within the given bounds.
    fn layout(&mut self, cx: &mut LayoutContext, bounds: Rect);

    /// Process an input event.
    ///
    /// The default implementation forwards the event to internal children
    /// (see [`Widget::child_count`]) in reverse order — topmost first —
    /// gated on the child's bounds for positional events. It returns the
    /// first non-[`Ignored`](EventResponse::Ignored) response, or
    /// `Ignored` if no child handled it. Leaf widgets override this to
    /// implement their own interaction.
    fn event(&mut self, cx: &mut EventContext) -> EventResponse {
        let n = self.child_count();
        for i in (0..n).rev() {
            let Some(child_bounds) = self.child_bounds(i) else {
                continue;
            };
            if let Some(pos) = cx.event.position() {
                if !child_bounds.contains(pos) {
                    continue;
                }
            }
            let Some(child) = self.child_mut(i) else {
                continue;
            };
            // Underflowed internal children whose policy covers input
            // are skipped — they have no arena node, so the check is
            // threshold-only (no hysteresis state).
            let min = child.min_render();
            if min.policy.covers_input() && min.violated(child_bounds, cx.scale) {
                continue;
            }
            let mut child_cx = EventContext {
                event: cx.event,
                bounds: child_bounds,
                scale: cx.scale,
            };
            match child.event(&mut child_cx) {
                EventResponse::Ignored => continue,
                response => return response,
            }
        }
        EventResponse::Ignored
    }

    /// Populate the AccessKit accessibility node. Default is a no-op.
    fn accessibility(&self, _node: &mut AccessKitNode) {}

    /// Prepare the widget for accessibility emission.
    ///
    /// Called once per widget before its node and internal-children
    /// subtree are built during a `TreeUpdate` — the place to apply
    /// pending assistive-technology activations received by internal
    /// children (e.g. a `SemanticAction::Click` delivered to a radio
    /// option) so the emitted tree reflects them. Popup content is
    /// *not* prepared: popup widgets are expected to be stateless
    /// views of owner state.
    ///
    /// Default: no-op.
    fn a11y_prepare(&mut self) {}

    /// Post-process the accessibility nodes emitted for this widget's
    /// internal subtree, plus this widget's own node.
    ///
    /// Called once per widget after its whole subtree has been emitted.
    /// `emitted` lists every descendant node minted for the widget's
    /// internal children (mutable — relations such as
    /// `aria-describedby` or `aria-controls` can be patched onto them);
    /// `overlay_nodes` lists the `NodeId`s minted for every popup
    /// currently open in the [`OverlayLayer`]
    /// (read-only — popups are emitted separately); `this_node` is the
    /// widget's own node. Use this hook for `aria-activedescendant`,
    /// `aria-controls`, and `aria-describedby`, which cannot be set
    /// inside [`Self::accessibility`] because descendant `NodeId`s are
    /// minted afterwards.
    ///
    /// Default: no-op.
    fn a11y_fixup(
        &self,
        _emitted: &mut Vec<A11yEmittedNode>,
        _overlay_nodes: &[OverlayA11yRef],
        _this_node: &mut AccessKitNode,
    ) {
    }

    /// Reconcile this widget's popup state with an [`OverlayLayer`].
    ///
    /// Widgets that own overlay popups (e.g. `Dropdown`, `Tooltip`)
    /// implement this to open their popup in `layer` while open, close
    /// it while closed, keep the anchor in sync with their layout
    /// bounds, and observe layer-initiated dismissal (outside click,
    /// Escape) via
    /// [`OverlayLayer::take_dismissed`](crate::overlay::OverlayLayer::take_dismissed).
    /// It is called once per frame per widget by
    /// [`WidgetArena::sync_overlays`](crate::WidgetArena::sync_overlays),
    /// which walks arena widgets *and* their internal children before
    /// running
    /// [`OverlayLayer::layout_pass`](crate::overlay::OverlayLayer::layout_pass).
    ///
    /// Default: no-op (the widget owns no popups).
    fn sync_overlay(&mut self, _overlay: &mut OverlayLayer) {}

    /// Advance time-dependent state by `dt`.
    ///
    /// Called once per frame on every live arena widget — and,
    /// recursively, on its internal children — by
    /// [`WidgetArena::tick`](crate::WidgetArena::tick). Widgets with
    /// elapsed-time behaviour (a `Tooltip`'s hover delay, a
    /// `ScrollView`'s decaying scroll animation) implement this to
    /// move that state forward; `WidgetArena::tick` then runs
    /// [`Self::sync_overlay`] reconciliation so effects like a tooltip
    /// opening its popup land the same frame.
    ///
    /// Return `true` while a repaint (or accessibility re-emission) is
    /// needed — e.g. an animation in flight or a delay countdown
    /// running — so the arena can dirty-mark the widget. Return
    /// `false` when the tick changed nothing visible.
    ///
    /// Default: no-op returning `false` (the widget is not
    /// time-dependent).
    fn tick(&mut self, _dt: Duration) -> bool {
        false
    }

    /// Whether this widget's internal children and arena children are
    /// clipped to the widget's bounds during paint traversal.
    ///
    /// When `true`, the paint walk emits a
    /// [`PaintCommand::ClipRect`](crate::PaintCommand::ClipRect) for the
    /// widget bounds before recursing into children and a
    /// [`PaintCommand::PopClip`](crate::PaintCommand::PopClip)
    /// afterwards, so descendants cannot draw outside the widget —
    /// required by scrollable regions. Overlay popups are always painted
    /// unclipped.
    ///
    /// Default: `false`.
    fn clips_children(&self) -> bool {
        false
    }

    /// Record this widget's own paint commands into the context's paint
    /// list. Default is a no-op — the widget contributes no chrome.
    ///
    /// Internal children are painted by the framework's tree walk, which
    /// recurses via [`Widget::child_count`]/[`Widget::child`]/
    /// [`Widget::child_bounds`] after this method returns — a `paint`
    /// implementation only needs to emit the widget's *own* chrome.
    fn paint(&self, _cx: &mut PaintContext) {}

    /// The widget's declared minimum render area and the policy applied
    /// when allocated bounds fall below it.
    ///
    /// The declaration is *not* a measure probe: it must be cheap and
    /// stable — return a constant, or a floor cached during
    /// `measure`/`layout`/`tick`. `RenderMinimum::ZERO` (the default)
    /// disables underflow handling. A per-node
    /// [`ColdNode::render_minimum`](crate::ColdNode) override takes
    /// precedence over this method — instance wins over type, matching
    /// [`Widget::debug_name`] precedence.
    ///
    /// Enforcement is lazy — evaluated against the node's final
    /// [`HotNode::bounds`](crate::HotNode) at paint, hit-test, and audit
    /// time — so it applies regardless of which layout authority
    /// assigned the bounds. Engagement requires
    /// [`WidgetArena::update_underflow`](crate::WidgetArena::update_underflow)
    /// (or `update_underflow_all`) to run after bounds assignment; the
    /// Taffy [`LayoutEngine`] does this automatically. Internal children
    /// are evaluated threshold-only, without hysteresis.
    ///
    /// # Examples
    ///
    /// ```
    /// use glam::Vec2;
    /// use martensite_core::{DummyWidget, RenderMinimum, Widget};
    ///
    /// assert_eq!(DummyWidget.min_render(), RenderMinimum::ZERO);
    /// ```
    fn min_render(&self) -> RenderMinimum {
        RenderMinimum::ZERO
    }

    /// Degraded chrome painted *instead of* this widget's normal body
    /// and children when the resolved policy is
    /// [`UnderflowPolicy::Fallback`] and the widget is underflowed.
    ///
    /// Default: paints nothing. `cx.bounds` is the *allocated* rect —
    /// the fallback is defined for the space it actually has.
    fn paint_underflow(&self, _cx: &mut PaintContext) {}

    /// Human-meaningful identity for this widget in diagnostics —
    /// the paint walker's `PushScope` markers and any lint output that
    /// names a component. Defaults to the concrete Rust type name
    /// (`mycrate::MyPanel`); override with a stable label like
    /// `"Process Grid"` when the type name is unhelpful.
    ///
    /// `&'static` because names must be embeddable in the owned
    /// `PaintCommand` stream without allocation — dynamic labels are
    /// not supported. A node's [`ColdNode::debug_name`](crate::ColdNode)
    /// field, when set, takes precedence over this method in the
    /// arena's scope emission (instance name wins over type name).
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite_core::{DummyWidget, Widget};
    ///
    /// // The default is the concrete type name.
    /// assert!(DummyWidget.debug_name().contains("DummyWidget"));
    /// ```
    fn debug_name(&self) -> &'static str {
        std::any::type_name::<Self>()
    }

    /// Number of internal (non-arena) children this widget manages.
    ///
    /// Container widgets such as `Flex` keep their children inside the
    /// widget rather than as arena nodes. The framework's event, paint,
    /// and accessibility walks enumerate them through this protocol.
    /// Default: zero.
    fn child_count(&self) -> usize {
        0
    }

    /// Borrow the `index`-th internal child, or `None` if out of range.
    fn child(&self, _index: usize) -> Option<&dyn Widget> {
        None
    }

    /// Mutably borrow the `index`-th internal child.
    fn child_mut(&mut self, _index: usize) -> Option<&mut dyn Widget> {
        None
    }

    /// Screen-space bounds assigned to the `index`-th internal child
    /// during the last `layout` call, or `None` if the child has not been
    /// laid out or the index is out of range.
    fn child_bounds(&self, _index: usize) -> Option<Rect> {
        None
    }

    /// Views this widget as `&mut dyn Any` so journaled commands can
    /// reach concrete widget internals (e.g. a `Counter`'s tick field).
    ///
    /// Only compiled with the `devtools-timemachine` feature. The default
    /// returns `None`; widgets that expose mutable state to time-travel
    /// commands should return `Some(self)`.
    ///
    /// # Examples
    ///
    /// ```
    /// use glam::Vec2;
    /// use martensite_core::{LayoutConstraints, LayoutContext, Rect, Widget};
    ///
    /// struct Counter(u32);
    /// impl Widget for Counter {
    ///     fn measure(&mut self, _cx: &mut LayoutContext, _c: LayoutConstraints) -> Vec2 {
    ///         Vec2::ZERO
    ///     }
    ///     fn layout(&mut self, _cx: &mut LayoutContext, _b: Rect) {}
    ///     fn as_any_mut(&mut self) -> Option<&mut dyn std::any::Any> {
    ///         Some(self)
    ///     }
    /// }
    ///
    /// let mut w = Counter(0);
    /// let any = Widget::as_any_mut(&mut w).unwrap();
    /// any.downcast_mut::<Counter>().unwrap().0 = 7;
    /// assert_eq!(w.0, 7);
    /// ```
    #[cfg(feature = "devtools-timemachine")]
    fn as_any_mut(&mut self) -> Option<&mut dyn std::any::Any> {
        None
    }

    /// Captures this widget's internal state for a time-travel snapshot.
    ///
    /// Only compiled with the `devtools-timemachine` feature. The default
    /// implementation returns `None`, meaning the widget contributes no
    /// restorable state beyond its [`ColdNode`](crate::ColdNode) metadata
    /// — widgets that keep mutable internal state (counters, text, scroll
    /// offsets) should override this so arena snapshots can round-trip it.
    ///
    /// # Examples
    ///
    /// ```
    /// use glam::Vec2;
    /// use martensite_core::{
    ///     LayoutConstraints, LayoutContext, Rect, TimemachineState, Widget,
    /// };
    ///
    /// #[derive(Debug)]
    /// struct CounterState(u32);
    /// impl TimemachineState for CounterState {
    ///     fn fingerprint(&self) -> u64 { self.0 as u64 }
    ///     fn as_any(&self) -> &dyn std::any::Any { self }
    /// }
    ///
    /// struct Counter(u32);
    /// impl Widget for Counter {
    ///     fn measure(&mut self, _cx: &mut LayoutContext, _c: LayoutConstraints) -> Vec2 {
    ///         Vec2::ZERO
    ///     }
    ///     fn layout(&mut self, _cx: &mut LayoutContext, _b: Rect) {}
    ///     fn timemachine_snapshot(&self) -> Option<Box<dyn TimemachineState>> {
    ///         Some(Box::new(CounterState(self.0)))
    ///     }
    ///     fn timemachine_restore(&mut self, state: &dyn TimemachineState) -> bool {
    ///         let Some(s) = state.as_any().downcast_ref::<CounterState>() else {
    ///             return false;
    ///         };
    ///         self.0 = s.0;
    ///         true
    ///     }
    /// }
    /// ```
    #[cfg(feature = "devtools-timemachine")]
    fn timemachine_snapshot(&self) -> Option<Box<dyn crate::snapshot::TimemachineState>> {
        None
    }

    /// Restores state previously captured by
    /// [`timemachine_snapshot`](Widget::timemachine_snapshot).
    ///
    /// The implementation should downcast `state` (via
    /// [`TimemachineState::as_any`](crate::snapshot::TimemachineState::as_any))
    /// to its own state type and return `true` on success. The default
    /// implementation rejects all state (`false`), matching the `None`
    /// snapshot default.
    #[cfg(feature = "devtools-timemachine")]
    fn timemachine_restore(&mut self, _state: &dyn crate::snapshot::TimemachineState) -> bool {
        false
    }
}

/// Default inert widget implementation for placeholder nodes and testing.
///
/// # Examples
///
/// ```
/// use glam::Vec2;
/// use martensite_core::{DummyWidget, LayoutConstraints, LayoutContext, Rect, Widget};
///
/// let mut w = DummyWidget;
/// let mut hot = martensite_core::HotNode::default();
/// let mut cx = LayoutContext {
///     hot: &mut hot,
///     scale: 1.0,
/// };
/// // `DummyWidget` measures to zero and lays out as a no-op.
/// assert_eq!(w.measure(&mut cx, LayoutConstraints { min_size: Vec2::ZERO, max_size: Vec2::new(100.0, 100.0) }), Vec2::ZERO);
/// w.layout(&mut cx, Rect::new(0.0, 0.0, 0.0, 0.0));
/// ```
#[derive(Copy, Clone, Debug, Default, PartialEq, Eq)]
pub struct DummyWidget;

impl Widget for DummyWidget {
    fn measure(&mut self, _cx: &mut LayoutContext, _constraints: LayoutConstraints) -> Vec2 {
        Vec2::ZERO
    }

    fn layout(&mut self, _cx: &mut LayoutContext, _bounds: Rect) {}
}
