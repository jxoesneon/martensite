//! Core memory arena, widget traits, and scene graph for Martensite.
#![forbid(unsafe_code)]

pub mod arena;
pub mod fence;
pub mod id;
pub mod node;
pub mod widget;

pub use arena::{ArenaError, BreadthFirstIter, Children, DepthFirstIter, SubtreeIter, WidgetArena};
pub use fence::{FrameFence, FrameGuard};
pub use id::WidgetId;
pub use node::{ColdNode, HotNode, NodeFlags, Rect};
pub use widget::{
    AccessibilityContext, DummyWidget, EventContext, EventResponse, LayoutConstraints,
    LayoutContext, PaintContext, Widget,
};

#[cfg(test)]
mod tests;
