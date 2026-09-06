//! 2D Spatial projected-beam focus engine.
#![forbid(unsafe_code)]

/// A cardinal direction in which focus may be projected within the 2D spatial layout.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum FocusDirection {
    /// Focus projected upward (decreasing vertical coordinate).
    Up,
    /// Focus projected downward (increasing vertical coordinate).
    Down,
    /// Focus projected leftward (decreasing horizontal coordinate).
    Left,
    /// Focus projected rightward (increasing horizontal coordinate).
    Right,
}

/// Tracks and manages the currently focused widget within a 2D spatial layout.
#[derive(Default)]
pub struct FocusManager {
    /// The widget currently holding focus, if any.
    pub current_focus: Option<martensite_core::WidgetId>,
}

impl FocusManager {
    /// Creates a new `FocusManager` with no widget currently focused.
    pub fn new() -> Self {
        Self {
            current_focus: None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{FocusDirection, FocusManager};
    use martensite_core::WidgetId;

    #[test]
    fn focus_direction_variants_distinct() {
        assert_ne!(FocusDirection::Up, FocusDirection::Down);
        assert_ne!(FocusDirection::Up, FocusDirection::Left);
        assert_ne!(FocusDirection::Up, FocusDirection::Right);
        assert_ne!(FocusDirection::Down, FocusDirection::Left);
        assert_ne!(FocusDirection::Down, FocusDirection::Right);
        assert_ne!(FocusDirection::Left, FocusDirection::Right);
    }

    #[test]
    fn focus_direction_copy_clone_debug_eq() {
        let dir = FocusDirection::Up;
        let cloned = dir;
        assert_eq!(dir, cloned);
        assert_eq!(format!("{:?}", FocusDirection::Up), "Up");
        assert_eq!(format!("{:?}", FocusDirection::Down), "Down");
        assert_eq!(format!("{:?}", FocusDirection::Left), "Left");
        assert_eq!(format!("{:?}", FocusDirection::Right), "Right");
    }

    #[test]
    fn new_has_no_focus() {
        let manager = FocusManager::new();
        assert!(manager.current_focus.is_none());
    }

    #[test]
    fn default_has_no_focus() {
        let manager = FocusManager::default();
        assert!(manager.current_focus.is_none());
    }

    #[test]
    fn can_store_and_read_widget_id() {
        let mut manager = FocusManager::new();
        let id = WidgetId::new(1, 1).expect("valid widget id");
        manager.current_focus = Some(id);
        assert_eq!(manager.current_focus, Some(id));
    }
}
