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
