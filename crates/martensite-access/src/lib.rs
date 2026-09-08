//! Native AccessKit accessibility adapter.
//!
//! This crate provides the bridge between the Martensite `WidgetArena` and
//! the platform accessibility subsystem (Windows UI Automation, macOS
//! NSAccessibility, Linux AT-SPI2) via AccessKit.
//!
//! ## Architecture
//!
//! The [`AccessKitAdapter`] walks the widget arena and produces
//! `TreeUpdate` payloads. Rather than regenerating
//! the full tree on every state change, it tracks dirty nodes via
//! [`NodeFlags::DIRTY_A11Y`](martensite_core::NodeFlags::DIRTY_A11Y) and
//! emits only the mutated nodes in each update.
//!
//! Each widget's [`Widget::accessibility`](martensite_core::widget::Widget::accessibility)
//! hook is called to populate role-specific properties on the AccessKit
//! node, while common properties (bounds, label, role, focusability) are
//! derived from the `HotNode` and `ColdNode` fields.
#![forbid(unsafe_code)]

pub mod actions;
pub mod adapter;
/// Screen reader caret tracking and text selection updates.
pub mod caret;
/// Automated WCAG 2.2 AA/AAA accessibility evaluation and Section 508 VPAT verification.
pub mod compliance;
pub mod properties;
pub mod winit;

pub use accesskit::{Node, NodeId, Rect, Role, TreeUpdate};
pub use adapter::AccessKitAdapter;
pub use caret::{CaretTracker, TextAffinity, TextBoundary, TextSelection};
pub use compliance::{
    check_target_size, check_text_contrast, check_ui_component_contrast, contrast_ratio,
    relative_luminance, ColorRgba, FocusAppearanceCheck, Section508VpatReport, TextSize, WcagLevel,
};
pub use properties::AccessibilityBuilder;

/// Converts a [`martensite_core::WidgetId`] to an [`accesskit::NodeId`].
///
/// The mapping is a direct reinterpretation of the 64-bit widget handle
/// as an AccessKit node identifier. This is stable across arena
/// generations because `WidgetId` embeds the generation counter.
#[inline]
pub fn widget_id_to_node_id(id: martensite_core::WidgetId) -> NodeId {
    NodeId(id.to_u64())
}

/// Converts an [`accesskit::NodeId`] back to a [`martensite_core::WidgetId`].
///
/// Returns `None` if the underlying value is zero (which would correspond
/// to an invalid `WidgetId` with generation zero).
#[inline]
pub fn node_id_to_widget_id(id: NodeId) -> Option<martensite_core::WidgetId> {
    martensite_core::WidgetId::from_u64(id.0)
}

/// Converts a [`martensite_core::Rect`] (origin + size, f32) to an
/// [`accesskit::Rect`] (x0/y0/x1/y1, f64).
#[inline]
pub fn rect_to_accesskit(r: martensite_core::Rect) -> Rect {
    Rect::new(
        r.min_x() as f64,
        r.min_y() as f64,
        r.max_x() as f64,
        r.max_y() as f64,
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn node_id_is_distinct() {
        let a = NodeId(1);
        let b = NodeId(2);
        assert_ne!(a, b, "distinct NodeId values must compare unequal");
        assert_eq!(a, NodeId(1), "equal NodeId values must compare equal");
    }

    #[test]
    fn node_id_from_u64() {
        let id = NodeId::from(42u64);
        assert_eq!(id, NodeId(42), "NodeId::from should wrap the given value");
    }

    #[test]
    fn role_variants_are_distinct() {
        assert_ne!(Role::Button, Role::TextInput);
        assert_ne!(Role::Button, Role::RadioButton);
        assert_ne!(Role::TextInput, Role::RadioButton);
    }

    #[test]
    fn node_new_has_given_role() {
        let node = Node::new(Role::Button);
        assert_eq!(node.role(), Role::Button);

        let text_node = Node::new(Role::TextInput);
        assert_eq!(text_node.role(), Role::TextInput);
        assert_ne!(node.role(), text_node.role());
    }

    #[test]
    fn widget_id_to_node_id_roundtrip() {
        let wid = martensite_core::WidgetId::from_parts(5, 1);
        let nid = widget_id_to_node_id(wid);
        assert_eq!(nid, NodeId(wid.to_u64()));
        let back = node_id_to_widget_id(nid).unwrap();
        assert_eq!(back, wid);
    }

    #[test]
    fn node_id_to_widget_id_none_for_zero() {
        assert!(node_id_to_widget_id(NodeId(0)).is_none());
    }

    #[test]
    fn rect_to_accesskit_conversion() {
        let r = martensite_core::Rect::new(10.0, 20.0, 100.0, 50.0);
        let a = rect_to_accesskit(r);
        assert_eq!(a.x0, 10.0);
        assert_eq!(a.y0, 20.0);
        assert_eq!(a.x1, 110.0);
        assert_eq!(a.y1, 70.0);
    }
}
