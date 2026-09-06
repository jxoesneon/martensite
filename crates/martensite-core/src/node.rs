use crate::id::WidgetId;
use glam::Vec2;

#[derive(Copy, Clone, Debug, Default, PartialEq)]
pub struct Rect {
    pub origin: Vec2,
    pub size: Vec2,
}

impl Rect {
    #[inline(always)]
    pub fn new(x: f32, y: f32, w: f32, h: f32) -> Self {
        Self { origin: Vec2::new(x, y), size: Vec2::new(w, h) }
    }
    #[inline(always)] pub fn min_x(&self) -> f32 { self.origin.x }
    #[inline(always)] pub fn max_x(&self) -> f32 { self.origin.x + self.size.x }
    #[inline(always)] pub fn min_y(&self) -> f32 { self.origin.y }
    #[inline(always)] pub fn max_y(&self) -> f32 { self.origin.y + self.size.y }
    #[inline(always)] pub fn width(&self) -> f32 { self.size.x }
    #[inline(always)] pub fn height(&self) -> f32 { self.size.y }
}

bitflags::bitflags! {
    #[derive(Copy, Clone, Debug, PartialEq, Eq)]
    pub struct NodeFlags: u32 {
        const DIRTY_LAYOUT       = 1 << 0;
        const DIRTY_PAINT        = 1 << 1;
        const DIRTY_A11Y         = 1 << 2;
        const VISIBLE            = 1 << 3;
        const HIT_TEST_ENABLED   = 1 << 4;
        const FOCUSABLE          = 1 << 5;
        const CLIPS_CHILDREN     = 1 << 6;
        const HOVERED            = 1 << 7;
        const PRESSED            = 1 << 8;
        const INERT              = 1 << 9;
    }
}

/// 64-Byte Cache-Line Aligned Hot Node Data
#[repr(C, align(64))]
#[derive(Copy, Clone, Debug)]
pub struct HotNode {
    pub layout_id: taffy::NodeId,
    pub bounds: Rect,
    pub flags: NodeFlags,
    pub layer_depth: u32,
    pub parent: Option<WidgetId>,
    pub first_child: Option<WidgetId>,
    pub next_sibling: Option<WidgetId>,
    pub prev_sibling: Option<WidgetId>,
}

pub struct ColdNode {
    pub debug_name: Option<&'static str>,
    pub tooltip: Option<String>,
    pub a11y_role: accesskit::Role,
    pub a11y_name: Option<String>,
    pub widget: Box<dyn crate::widget::Widget>,
}
