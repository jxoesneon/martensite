//! Core memory arena, widget traits, and scene graph for Martensite.
#![forbid(unsafe_code)]

/// Generational slot-map arena for widget storage.
pub mod arena;
/// Frame synchronization fences for double-buffered rendering.
pub mod fence;
/// Opaque, niche-optimized widget identifier type.
pub mod id;
/// Hot and cold node representations for cache-friendly scene graph storage.
pub mod node;
/// Widget trait and rendering/layout/event context types.
pub mod widget;

pub use arena::{ArenaError, BreadthFirstIter, Children, DepthFirstIter, SubtreeIter, WidgetArena};
pub use fence::{FrameFence, FrameGuard};
pub use id::WidgetId;
pub use node::{ColdNode, HotNode, InlineTextCache, NodeFlags, Rect};
pub use widget::{
    AccessibilityContext, DummyWidget, EventContext, EventResponse, LayoutConstraints,
    LayoutContext, PaintContext, Widget,
};

#[cfg(test)]
mod tests;
