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

/// Context provided to widgets during event processing.
pub struct EventContext {}

/// Context provided to widgets during the paint pass.
pub struct PaintContext {}

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
/// let mut cx = EventContext {};
/// assert_eq!(w.event(&mut cx), EventResponse::Ignored);
/// // The default `paint` and `accessibility` implementations are no-ops.
/// let mut paint_cx = PaintContext {};
/// w.paint(&mut paint_cx);
/// ```
pub trait Widget: Send + Sync + 'static {
    /// Measure the widget's desired size given layout constraints.
    fn measure(&mut self, cx: &mut LayoutContext, constraints: LayoutConstraints) -> Vec2;
    /// Position the widget within the given bounds.
    fn layout(&mut self, cx: &mut LayoutContext, bounds: Rect);
    /// Process an input event. Default implementation ignores the event.
    fn event(&mut self, _cx: &mut EventContext) -> EventResponse {
        EventResponse::Ignored
    }
    /// Populate the AccessKit accessibility node. Default is a no-op.
    fn accessibility(&self, _node: &mut AccessKitNode) {}
    /// Record paint commands. Default is a no-op.
    fn paint(&self, _cx: &mut PaintContext) {}
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
