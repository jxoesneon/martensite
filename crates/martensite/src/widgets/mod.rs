//! Base widgets: `Container`, `Flex`, `Stack`, `Text`.
//!
//! These are the foundational building blocks for Martensite UIs.
//! Each widget implements the [`Widget`](martensite_core::widget::Widget)
//! trait and integrates with the arena, layout, text, and rendering
//! pipelines.

pub mod container;
pub mod flex;
pub mod stack;
pub mod text;

pub use container::Container;
pub use flex::{Flex, FlexDirection};
pub use stack::Stack;
pub use text::Text;
