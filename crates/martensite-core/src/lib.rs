//! Core memory arena, widget traits, and scene graph for Martensite.
//!
//! # Examples
//!
//! Building a small scene graph from hot/cold nodes and traversing it:
//!
//! ```
//! use martensite_core::{ColdNode, DummyWidget, HotNode, WidgetArena};
//!
//! let mut arena = WidgetArena::new();
//! let parent = arena.insert_with_widget(HotNode::default(), Box::new(DummyWidget));
//! let child = arena.insert_with_widget(HotNode::default(), Box::new(DummyWidget));
//! arena.append_child(parent, child).unwrap();
//!
//! assert_eq!(arena.len(), 2);
//! assert!(arena.is_alive(parent));
//! assert_eq!(arena.children(parent).count(), 1);
//! ```
#![forbid(unsafe_code)]
#![deny(missing_docs)]

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
pub use fence::{FrameFence, FrameGuard, DEFAULT_LEASE_TIMEOUT};
pub use id::{SurfaceId, WidgetId};
pub use node::{ColdNode, HotNode, InlineTextCache, NodeFlags, Rect};
pub use widget::{
    AccessibilityContext, DummyWidget, EventContext, EventResponse, LayoutConstraints,
    LayoutContext, PaintContext, Widget,
};

#[cfg(test)]
mod tests;
