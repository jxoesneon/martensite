use crate::node::{Rect, HotNode};
use glam::Vec2;
use accesskit::Node as AccessKitNode;

#[derive(Copy, Clone, Debug, PartialEq)]
pub struct LayoutConstraints {
    pub min_size: Vec2,
    pub max_size: Vec2,
}

#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum EventResponse {
    Ignored,
    Handled,
    CaptureFocus,
    RequestRepaint,
}

pub struct LayoutContext<'a> {
    pub hot: &'a mut HotNode,
}

pub struct EventContext {}
pub struct PaintContext {}
pub struct AccessibilityContext {}

pub trait Widget: Send + Sync + 'static {
    fn measure(&mut self, cx: &mut LayoutContext, constraints: LayoutConstraints) -> Vec2;
    fn layout(&mut self, cx: &mut LayoutContext, bounds: Rect);
    fn event(&mut self, cx: &mut EventContext) -> EventResponse { EventResponse::Ignored }
    fn accessibility(&self, node: &mut AccessKitNode) {}
    fn paint(&self, cx: &mut PaintContext) {}
}
