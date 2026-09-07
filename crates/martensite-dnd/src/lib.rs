//! Unified Drag-and-Drop engine.
//!
//! This crate provides a detached, process-wide drag-and-drop subsystem for
//! the Martensite GUI framework. The core design principle is that a
//! [`DndSession`] holds an opaque, thread-safe payload that remains valid for
//! the entire lifetime of a drag operation, even if the originating window or
//! widget is evicted from the widget arena mid-flight. The session is only
//! retired when the OS signals a drop completion or cancellation.
//!
//! The [`session`] module provides [`DndSession`] and the process-wide
//! [`DndSessionManager`]. The [`target`] module provides [`DropTarget`]
//! registration, validation, and the drag-enter / drag-leave / drop lifecycle
//! via [`DropTargetRegistry`].
#![forbid(unsafe_code)]
#![deny(missing_docs)]

pub mod session;
pub mod target;

pub use session::{DndSession, DndSessionManager, DndStatus, SessionId};
pub use target::{DropEffectMask, DropTarget, DropTargetRegistry, DropTargetState, TargetId};

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

#[cfg(test)]
mod tests {
    use super::DropEffect;

    #[test]
    fn default_is_none() {
        assert_eq!(DropEffect::default(), DropEffect::None);
    }

    #[test]
    fn variants_are_distinct() {
        assert_ne!(DropEffect::None, DropEffect::Copy);
        assert_ne!(DropEffect::None, DropEffect::Move);
        assert_ne!(DropEffect::None, DropEffect::Link);
        assert_ne!(DropEffect::Copy, DropEffect::Move);
        assert_ne!(DropEffect::Copy, DropEffect::Link);
        assert_ne!(DropEffect::Move, DropEffect::Link);
    }

    #[test]
    fn copy_clone_works() {
        let effect = DropEffect::Copy;
        let cloned = effect;
        assert_eq!(effect, cloned);
    }

    #[test]
    fn debug_format_works() {
        assert_eq!(format!("{:?}", DropEffect::None), "None");
        assert_eq!(format!("{:?}", DropEffect::Copy), "Copy");
        assert_eq!(format!("{:?}", DropEffect::Move), "Move");
        assert_eq!(format!("{:?}", DropEffect::Link), "Link");
    }

    #[test]
    fn partial_eq_and_eq_work() {
        assert!(DropEffect::Copy == DropEffect::Copy);
        assert!(DropEffect::Copy != DropEffect::Move);
    }
}
