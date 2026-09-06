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
