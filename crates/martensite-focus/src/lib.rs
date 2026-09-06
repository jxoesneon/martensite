//! 2D Spatial projected-beam focus engine.
#![forbid(unsafe_code)]

#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum FocusDirection { Up, Down, Left, Right }

#[derive(Default)]
pub struct FocusManager {
    pub current_focus: Option<martensite_core::WidgetId>,
}


impl FocusManager {
    pub fn new() -> Self {
        Self { current_focus: None }
    }
}
