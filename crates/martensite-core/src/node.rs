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
        Self {
            origin: Vec2::new(x, y),
            size: Vec2::new(w, h),
        }
    }
    #[inline(always)]
    pub fn min_x(&self) -> f32 {
        self.origin.x
    }
    #[inline(always)]
    pub fn max_x(&self) -> f32 {
        self.origin.x + self.size.x
    }
    #[inline(always)]
    pub fn min_y(&self) -> f32 {
        self.origin.y
    }
    #[inline(always)]
    pub fn max_y(&self) -> f32 {
        self.origin.y + self.size.y
    }
    #[inline(always)]
    pub fn width(&self) -> f32 {
        self.size.x
    }
    #[inline(always)]
    pub fn height(&self) -> f32 {
        self.size.y
    }
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
#[derive(Copy, Clone, Debug, PartialEq)]
pub struct HotNode {
    pub bounds: Rect,                   // 16 bytes (offset 0..16)
    pub layout_id: taffy::NodeId,       // 8 bytes  (offset 16..24)
    pub flags: NodeFlags,               // 4 bytes  (offset 24..28)
    pub depth_rank: u16,                // 2 bytes  (offset 28..30)
    pub z_index: i16,                   // 2 bytes  (offset 30..32)
    pub parent: Option<WidgetId>,       // 8 bytes  (offset 32..40)
    pub first_child: Option<WidgetId>,  // 8 bytes  (offset 40..48)
    pub next_sibling: Option<WidgetId>, // 8 bytes  (offset 48..56)
    pub prev_sibling: Option<WidgetId>, // 8 bytes  (offset 56..64)
}

// Compile-time invariant verification: HotNode must be exactly 64 bytes on 64-bit platforms
const _: () = assert!(std::mem::size_of::<HotNode>() == 64);
const _: () = assert!(std::mem::align_of::<HotNode>() == 64);

impl HotNode {
    /// Construct a HotNode with default zeroed layout and hierarchy fields.
    #[inline(always)]
    pub const fn new(layout_id: taffy::NodeId) -> Self {
        Self {
            bounds: Rect {
                origin: Vec2::ZERO,
                size: Vec2::ZERO,
            },
            layout_id,
            flags: NodeFlags::empty(),
            depth_rank: 0,
            z_index: 0,
            parent: None,
            first_child: None,
            next_sibling: None,
            prev_sibling: None,
        }
    }

    /// Retrieve topological depth rank.
    #[inline(always)]
    pub const fn depth_rank(&self) -> u16 {
        self.depth_rank
    }

    /// Retrieve layer depth (compatibility alias for depth_rank).
    #[inline(always)]
    pub const fn layer_depth(&self) -> u16 {
        self.depth_rank
    }

    /// Update topological depth rank.
    #[inline(always)]
    pub fn set_depth_rank(&mut self, depth: u16) {
        self.depth_rank = depth;
    }

    /// Update layer depth (compatibility alias for depth_rank).
    #[inline(always)]
    pub fn set_layer_depth(&mut self, depth: u16) {
        self.depth_rank = depth;
    }
}

impl Default for HotNode {
    #[inline(always)]
    fn default() -> Self {
        Self::new(taffy::NodeId::new(0))
    }
}

pub struct ColdNode {
    pub debug_name: Option<&'static str>,
    pub tooltip: Option<String>,
    pub a11y_role: accesskit::Role,
    pub a11y_name: Option<String>,
    pub widget: Box<dyn crate::widget::Widget>,
}

impl ColdNode {
    /// Construct a ColdNode wrapping a concrete widget.
    pub fn new(widget: Box<dyn crate::widget::Widget>) -> Self {
        Self {
            debug_name: None,
            tooltip: None,
            a11y_role: accesskit::Role::GenericContainer,
            a11y_name: None,
            widget,
        }
    }

    /// Set debug name.
    pub fn with_name(mut self, name: &'static str) -> Self {
        self.debug_name = Some(name);
        self
    }

    /// Set accessibility role.
    pub fn with_role(mut self, role: accesskit::Role) -> Self {
        self.a11y_role = role;
        self
    }

    /// Set tooltip text.
    pub fn with_tooltip(mut self, tooltip: impl Into<String>) -> Self {
        self.tooltip = Some(tooltip.into());
        self
    }

    /// Set accessibility name.
    pub fn with_a11y_name(mut self, name: impl Into<String>) -> Self {
        self.a11y_name = Some(name.into());
        self
    }
}

impl Default for ColdNode {
    fn default() -> Self {
        Self::new(Box::new(crate::widget::DummyWidget))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::mem::{align_of, size_of};

    #[test]
    fn test_sizes() {
        assert_eq!(size_of::<HotNode>(), 64);
        assert_eq!(align_of::<HotNode>(), 64);
        assert_eq!(size_of::<WidgetId>(), 8);
        assert_eq!(size_of::<Option<WidgetId>>(), 8);
        assert_eq!(size_of::<taffy::NodeId>(), 8);
        assert_eq!(size_of::<Rect>(), 16);
        assert_eq!(size_of::<NodeFlags>(), 4);
    }
}
