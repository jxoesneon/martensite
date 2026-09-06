//! Core memory arena, widget traits, and scene graph for Martensite.
#![forbid(unsafe_code)]

pub mod arena;
pub mod id;
pub mod node;
pub mod widget;

pub use id::WidgetId;
pub use node::{HotNode, ColdNode, NodeFlags, Rect};
pub use arena::WidgetArena;
pub use widget::{Widget, EventResponse, LayoutConstraints, LayoutContext, EventContext, PaintContext, AccessibilityContext};
