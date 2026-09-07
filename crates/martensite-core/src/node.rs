use crate::id::WidgetId;
use glam::Vec2;

/// Axis-aligned rectangle in 2D screen space.
#[derive(Copy, Clone, Debug, Default, PartialEq)]
pub struct Rect {
    /// Top-left corner position.
    pub origin: Vec2,
    /// Width and height.
    pub size: Vec2,
}

impl Rect {
    /// Create a rectangle from `(x, y, width, height)`.
    #[inline(always)]
    pub fn new(x: f32, y: f32, w: f32, h: f32) -> Self {
        Self {
            origin: Vec2::new(x, y),
            size: Vec2::new(w, h),
        }
    }
    /// Returns the minimum x-coordinate (left edge).
    #[inline(always)]
    pub fn min_x(&self) -> f32 {
        self.origin.x
    }
    /// Returns the maximum x-coordinate (right edge).
    #[inline(always)]
    pub fn max_x(&self) -> f32 {
        self.origin.x + self.size.x
    }
    /// Returns the minimum y-coordinate (top edge).
    #[inline(always)]
    pub fn min_y(&self) -> f32 {
        self.origin.y
    }
    /// Returns the maximum y-coordinate (bottom edge).
    #[inline(always)]
    pub fn max_y(&self) -> f32 {
        self.origin.y + self.size.y
    }
    /// Returns the width.
    #[inline(always)]
    pub fn width(&self) -> f32 {
        self.size.x
    }
    /// Returns the height.
    #[inline(always)]
    pub fn height(&self) -> f32 {
        self.size.y
    }
}

bitflags::bitflags! {
    /// Bitflags tracking layout, paint, accessibility, and interaction state for a node.
    #[derive(Copy, Clone, Debug, PartialEq, Eq)]
    pub struct NodeFlags: u32 {
        /// Layout needs recalculation.
        const DIRTY_LAYOUT       = 1 << 0;
        /// Paint needs re-recording.
        const DIRTY_PAINT        = 1 << 1;
        /// Accessibility tree needs update.
        const DIRTY_A11Y         = 1 << 2;
        /// Node is visible.
        const VISIBLE            = 1 << 3;
        /// Node participates in hit testing.
        const HIT_TEST_ENABLED   = 1 << 4;
        /// Node can receive keyboard focus.
        const FOCUSABLE          = 1 << 5;
        /// Children are clipped to this node's bounds.
        const CLIPS_CHILDREN     = 1 << 6;
        /// Node is currently hovered.
        const HOVERED            = 1 << 7;
        /// Node is currently pressed.
        const PRESSED            = 1 << 8;
        /// Node is inert (ignores input).
        const INERT              = 1 << 9;
    }
}

/// 64-byte cache-line aligned hot node data.
///
/// Stores the most frequently accessed fields for rendering and traversal,
/// packed into exactly one 64-byte cache line on 64-bit platforms.
#[repr(C, align(64))]
#[derive(Copy, Clone, Debug, PartialEq)]
pub struct HotNode {
    /// Screen-space bounding rectangle.
    pub bounds: Rect, // 16 bytes (offset 0..16)
    /// Taffy layout node identifier.
    pub layout_id: taffy::NodeId, // 8 bytes  (offset 16..24)
    /// Dirty/visibility/interaction flags.
    pub flags: NodeFlags, // 4 bytes  (offset 24..28)
    /// Topological depth from root (BFS rank).
    pub depth_rank: u16, // 2 bytes  (offset 28..30)
    /// Z-ordering index within siblings.
    pub z_index: i16, // 2 bytes  (offset 30..32)
    /// Parent widget identifier, if any.
    pub parent: Option<WidgetId>, // 8 bytes  (offset 32..40)
    /// First child widget identifier, if any.
    pub first_child: Option<WidgetId>, // 8 bytes  (offset 40..48)
    /// Next sibling widget identifier, if any.
    pub next_sibling: Option<WidgetId>, // 8 bytes  (offset 48..56)
    /// Previous sibling widget identifier, if any.
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

/// Inline text measurement cache storing the last 4 measured (width, height) constraint pairs.
///
/// Packaged inside [`ColdNode`] to eliminate redundant text shaping passes
/// during iterative flexbox layout probes in $O(1)$ time without locking or heap allocation.
#[derive(Copy, Clone, Debug, PartialEq)]
pub struct InlineTextCache {
    /// Four inline constraint/size slots: `(width_constraint, measured_width, measured_height)`.
    entries: [(f32, f32, f32); 4],
    /// Ring-buffer cursor for FIFO replacement when all 4 slots are occupied.
    cursor: usize,
}

impl InlineTextCache {
    /// Creates an empty inline text measurement cache.
    #[inline(always)]
    pub const fn new() -> Self {
        Self {
            entries: [(f32::NAN, f32::NAN, f32::NAN); 4],
            cursor: 0,
        }
    }

    /// Queries the cache for a measured size matching the given `available_width`.
    ///
    /// Matches if the difference between cached width and `available_width` is within 0.01px.
    /// For non-finite values (infinity, NaN), matches only on exact equality.
    /// Returns `(measured_width, measured_height)` on hit.
    #[inline(always)]
    pub fn get(&self, available_width: f32) -> Option<(f32, f32)> {
        self.entries
            .iter()
            .find(|(w, _, _)| {
                if available_width.is_finite() && w.is_finite() {
                    (*w - available_width).abs() < 0.01
                } else {
                    *w == available_width
                }
            })
            .map(|(_, mw, mh)| (*mw, *mh))
    }

    /// Inserts or updates a `(available_width, measured_width, measured_height)` triple into the cache.
    ///
    /// If an entry matching `available_width` (within 0.01px for finite values,
    /// exact equality for non-finite values) already exists, its measured size
    /// is updated in-place. Otherwise, the oldest slot is overwritten in FIFO order.
    #[inline]
    pub fn put(&mut self, available_width: f32, measured_width: f32, measured_height: f32) {
        for entry in &mut self.entries {
            let matches = if available_width.is_finite() && entry.0.is_finite() {
                (entry.0 - available_width).abs() < 0.01
            } else {
                entry.0 == available_width
            };
            if matches {
                entry.1 = measured_width;
                entry.2 = measured_height;
                return;
            }
        }
        self.entries[self.cursor] = (available_width, measured_width, measured_height);
        self.cursor = (self.cursor + 1) % 4;
    }

    /// Clears all entries from the inline text cache.
    #[inline(always)]
    pub fn clear(&mut self) {
        self.entries = [(f32::NAN, f32::NAN, f32::NAN); 4];
        self.cursor = 0;
    }

    /// Returns a slice of the 4 inline slot entries.
    #[inline(always)]
    pub const fn entries(&self) -> &[(f32, f32, f32); 4] {
        &self.entries
    }

    /// Returns `true` if no valid entries are currently stored.
    #[inline]
    pub fn is_empty(&self) -> bool {
        self.entries.iter().all(|(w, _, _)| w.is_nan())
    }

    /// Returns the number of valid entries currently stored.
    #[inline]
    pub fn len(&self) -> usize {
        self.entries.iter().filter(|(w, _, _)| !w.is_nan()).count()
    }
}

impl Default for InlineTextCache {
    #[inline(always)]
    fn default() -> Self {
        Self::new()
    }
}

/// Cold node data storing infrequently accessed widget state.
///
/// Contains accessibility metadata, debug names, tooltips, and the
/// boxed widget trait object. Stored separately from [`HotNode`] to
/// preserve cache locality during rendering and traversal.
pub struct ColdNode {
    /// Optional debug name for diagnostics.
    pub debug_name: Option<&'static str>,
    /// Optional tooltip text.
    pub tooltip: Option<String>,
    /// Accessibility role.
    pub a11y_role: accesskit::Role,
    /// Accessibility name.
    pub a11y_name: Option<String>,
    /// Tier 1 inline text measurement cache for $O(1)$ flexbox constraint probes.
    pub text_cache: InlineTextCache,
    /// Boxed widget implementation.
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
            text_cache: InlineTextCache::new(),
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

    /// Set inline text cache.
    pub fn with_text_cache(mut self, text_cache: InlineTextCache) -> Self {
        self.text_cache = text_cache;
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
