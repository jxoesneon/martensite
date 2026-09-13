use crate::node::{HotNode, Rect};
use accesskit::Node as AccessKitNode;
use glam::Vec2;

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
    CaptureFocus,
    /// The event was handled and a repaint is requested.
    RequestRepaint,
}

/// Context provided to widgets during the layout pass.
pub struct LayoutContext<'a> {
    /// Mutable access to the hot node being laid out.
    pub hot: &'a mut HotNode,
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
        /// Window-space position in logical pixels.
        position: Vec2,
    },
    /// A pointer button was pressed inside the widget's bounds.
    PointerPressed {
        /// Window-space position in logical pixels.
        position: Vec2,
        /// Which button was pressed.
        button: PointerButton,
    },
    /// A pointer button was released.
    PointerReleased {
        /// Window-space position in logical pixels.
        position: Vec2,
        /// Which button was released.
        button: PointerButton,
    },
    /// A scroll gesture occurred over the widget's bounds.
    Scroll {
        /// Window-space position in logical pixels.
        position: Vec2,
        /// Scroll delta in logical pixels (positive = content up/right).
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
    /// The widget gained keyboard focus.
    FocusGained,
    /// The widget lost keyboard focus.
    FocusLost,
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
/// };
/// assert_eq!(cx.bounds.width(), 100.0);
/// ```
pub struct EventContext<'a> {
    /// The event being delivered.
    pub event: &'a WidgetEvent,
    /// The widget's screen-space bounds in logical pixels.
    pub bounds: Rect,
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
/// use martensite_core::{PaintContext, PaintList, Rect};
///
/// let mut list = PaintList::new();
/// {
///     let mut cx = PaintContext {
///         list: &mut list,
///         bounds: Rect::new(0.0, 0.0, 50.0, 20.0),
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
    /// The widget's screen-space bounds in logical pixels.
    pub bounds: Rect,
}

/// Context provided to widgets during accessibility tree construction.
pub struct AccessibilityContext {}

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
            let mut child_cx = EventContext {
                event: cx.event,
                bounds: child_bounds,
            };
            let Some(child) = self.child_mut(i) else {
                continue;
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

    /// Record this widget's own paint commands into the context's paint
    /// list. Default is a no-op — the widget contributes no chrome.
    ///
    /// Internal children are painted by the framework's tree walk, which
    /// recurses via [`Widget::child_count`]/[`Widget::child`]/
    /// [`Widget::child_bounds`] after this method returns — a `paint`
    /// implementation only needs to emit the widget's *own* chrome.
    fn paint(&self, _cx: &mut PaintContext) {}

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
/// let mut cx = LayoutContext { hot: &mut hot };
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
