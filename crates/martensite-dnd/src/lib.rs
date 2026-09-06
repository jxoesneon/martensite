//! Unified Drag-and-Drop engine.
#![forbid(unsafe_code)]

#[derive(Copy, Clone, Debug, PartialEq, Eq, Default)]
pub enum DropEffect {
    #[default]
    None,
    Copy,
    Move,
    Link,
}
