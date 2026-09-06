use crate::node::{HotNode, Rect};
use accesskit::Node as AccessKitNode;
use glam::Vec2;

/// Minimum and maximum size bounds for layout measurement.
#[derive(Copy, Clone, Debug, PartialEq)]
pub struct LayoutConstraints {
    /// Minimum acceptable size.
    pub min_size: Vec2,
    /// Maximum acceptable size.
    pub max_size: Vec2,
}

/// Result of processing an input event on a widget.
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
#[derive(Copy, Clone, Debug, Default, PartialEq, Eq)]
pub struct DummyWidget;

impl Widget for DummyWidget {
    fn measure(&mut self, _cx: &mut LayoutContext, _constraints: LayoutConstraints) -> Vec2 {
        Vec2::ZERO
    }

    fn layout(&mut self, _cx: &mut LayoutContext, _bounds: Rect) {}
}
