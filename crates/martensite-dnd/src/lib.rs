//! Unified Drag-and-Drop engine.
#![forbid(unsafe_code)]

/// Describes the effect of a drag-and-drop operation on the source data.
#[derive(Copy, Clone, Debug, PartialEq, Eq, Default)]
pub enum DropEffect {
    /// No effect; the drop is ignored.
    #[default]
    None,
    /// The data is copied to the target.
    Copy,
    /// The data is moved to the target.
    Move,
    /// A link to the source data is created at the target.
    Link,
}
