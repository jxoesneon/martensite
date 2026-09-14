//! Accessibility-tree integration tests for the ARIA APG widgets.
//!
//! These tests run the six milestone widgets through the real
//! `AccessKitAdapter` emission path — arena insertion, internal-child
//! virtual nodes, overlay popup emission, `a11y_prepare`/`a11y_fixup`
//! relation wiring, and `SemanticAction` delivery back into the
//! widgets — so regressions in the emitted tree structure (not just
//! widget-local node fields) are caught.

#![forbid(unsafe_code)]

use accesskit::{Action, Node as AccessNode, NodeId, Role};
use glam::Vec2;
use martensite::widgets::{Dropdown, RadioGroup, ScrollView, Slider, Tabs, Text, Tooltip};
use martensite_access::{widget_id_to_node_id, AccessKitAdapter};
use martensite_core::widget::{LayoutContext, Widget};
use martensite_core::{
    ColdNode, EventContext, HotNode, NodeFlags, OverlayLayer, Rect, SemanticAction, WidgetArena,
    WidgetEvent, WidgetId,
};

/// Inserts a laid-out widget into a fresh arena and returns
/// `(arena, root)`.
fn arena_with(mut widget: impl Widget + 'static, bounds: Rect) -> (WidgetArena, WidgetId) {
    let mut hot = HotNode::default();
    let mut cx = LayoutContext { hot: &mut hot };
    widget.layout(&mut cx, bounds);

    let mut arena = WidgetArena::new();
    let mut hot = HotNode::new(taffy::NodeId::new(0));
    hot.flags = NodeFlags::VISIBLE | NodeFlags::HIT_TEST_ENABLED;
    hot.bounds = bounds;
    let id = arena.insert(hot, ColdNode::new(Box::new(widget)));
    (arena, id)
}

/// Finds the first node in the update satisfying `pred`.
fn find_node(
    update: &accesskit::TreeUpdate,
    pred: impl Fn(&AccessNode) -> bool,
) -> Option<&(NodeId, AccessNode)> {
    update.nodes.iter().find(|(_, n)| pred(n))
}

/// Collects every node in the update satisfying `pred`.
fn find_all(
    update: &accesskit::TreeUpdate,
    pred: impl Fn(&AccessNode) -> bool,
) -> Vec<&(NodeId, AccessNode)> {
    update.nodes.iter().filter(|(_, n)| pred(n)).collect()
}

// ---------------------------------------------------------------------------
// Slider
// ---------------------------------------------------------------------------

#[test]
fn slider_emits_apg_contract_and_semantic_increment() {
    let slider = Slider::new(0.0, 100.0).with_value(40.0).label("Volume");
    let (mut arena, root) = arena_with(slider, Rect::new(0.0, 0.0, 200.0, 24.0));
    let mut adapter = AccessKitAdapter::new(root);

    let update = adapter.build_update(&mut arena);
    let (_, node) = find_node(&update, |n| n.role() == Role::Slider).expect("slider node emitted");
    assert_eq!(node.label(), Some("Volume"));
    assert_eq!(node.numeric_value(), Some(40.0));
    assert_eq!(node.min_numeric_value(), Some(0.0));
    assert_eq!(node.max_numeric_value(), Some(100.0));
    assert!(node.supports_action(Action::Increment));
    assert!(node.supports_action(Action::Decrement));

    // Route an AT increment through the semantic event path.
    arena.dispatch_event(
        root,
        &WidgetEvent::SemanticAction(SemanticAction::Increment),
    );
    let update = adapter.build_update(&mut arena);
    let (_, node) = find_node(&update, |n| n.role() == Role::Slider).unwrap();
    assert_eq!(node.numeric_value(), Some(41.0));
}

// ---------------------------------------------------------------------------
// Radio group
// ---------------------------------------------------------------------------

#[test]
fn radio_group_tree_and_at_activation() {
    let group = RadioGroup::new(["Alpha", "Beta", "Gamma"]);
    let (mut arena, root) = arena_with(group, Rect::new(0.0, 0.0, 300.0, 30.0));
    let mut adapter = AccessKitAdapter::new(root);

    let update = adapter.build_update(&mut arena);
    let radios = find_all(&update, |n| n.role() == Role::RadioButton);
    assert_eq!(radios.len(), 3);
    // aria-checked → AccessKit `toggled`.
    let checked: Vec<bool> = radios
        .iter()
        .map(|(_, n)| n.toggled() == Some(accesskit::Toggled::True))
        .collect();
    assert_eq!(checked, vec![true, false, false]);
    // Roving tabindex: exactly one radio advertises Focus.
    let focusable = radios
        .iter()
        .filter(|(_, n)| n.supports_action(Action::Focus))
        .count();
    assert_eq!(focusable, 1);

    // Deliver an AT Click to the third radio through the real virtual-
    // node resolution path.
    let gamma_id = radios[2].0;
    let (owner, path) = adapter
        .resolve_internal(gamma_id)
        .expect("radio resolves to internal path");
    let target = arena
        .internal_widget_mut(owner, path)
        .expect("internal widget resolves");
    let ev = WidgetEvent::SemanticAction(SemanticAction::Click);
    let mut cx = EventContext {
        event: &ev,
        bounds: Rect::default(),
    };
    target.event(&mut cx);

    // Rebuild — a11y_prepare applies the pending activation.
    let update = adapter.build_update(&mut arena);
    let radios = find_all(&update, |n| n.role() == Role::RadioButton);
    let checked: Vec<bool> = radios
        .iter()
        .map(|(_, n)| n.toggled() == Some(accesskit::Toggled::True))
        .collect();
    assert_eq!(checked, vec![false, false, true]);
}

// ---------------------------------------------------------------------------
// Tabs
// ---------------------------------------------------------------------------

#[test]
fn tabs_wire_aria_controls_and_panel_visibility() {
    let tabs = Tabs::with_labels(["One", "Two", "Three"]);
    let (mut arena, root) = arena_with(tabs, Rect::new(0.0, 0.0, 300.0, 200.0));
    let mut adapter = AccessKitAdapter::new(root);

    let update = adapter.build_update(&mut arena);
    find_node(&update, |n| n.role() == Role::TabList).expect("tab list emitted");
    let tabs = find_all(&update, |n| n.role() == Role::Tab);
    assert_eq!(tabs.len(), 3);
    let panels = find_all(&update, |n| n.role() == Role::TabPanel);
    assert_eq!(panels.len(), 3);

    // Every tab controls exactly one panel; the first is selected and
    // its panel is the only visible one.
    let panel_ids: Vec<NodeId> = panels.iter().map(|(id, _)| *id).collect();
    for (i, (_, tab)) in tabs.iter().enumerate() {
        let controls = tab.controls();
        assert_eq!(controls.len(), 1, "tab has aria-controls");
        assert_eq!(controls[0], panel_ids[i]);
        assert_eq!(tab.is_selected(), Some(i == 0));
    }
    let hidden: Vec<bool> = panels.iter().map(|(_, n)| n.is_hidden()).collect();
    assert_eq!(hidden, vec![false, true, true]);
    // Panels are labelled by their tabs.
    for (i, (_, panel)) in panels.iter().enumerate() {
        let labelled_by = panel.labelled_by();
        assert_eq!(labelled_by[0], tabs[i].0);
    }

    // The widget's arena node parents the TabList and panel set.
    let root_id = widget_id_to_node_id(root);
    let (_, widget_node) = update
        .nodes
        .iter()
        .find(|(id, _)| *id == root_id)
        .expect("widget node");
    let tablist_id = find_node(&update, |n| n.role() == Role::TabList)
        .map(|(id, _)| *id)
        .unwrap();
    assert!(widget_node.children().contains(&tablist_id));
}

// ---------------------------------------------------------------------------
// ScrollView
// ---------------------------------------------------------------------------

#[test]
fn scrollview_emits_region_and_scrollbar_nodes() {
    struct Fixed(Vec2);
    impl Widget for Fixed {
        fn measure(
            &mut self,
            _cx: &mut LayoutContext,
            _c: martensite_core::widget::LayoutConstraints,
        ) -> Vec2 {
            self.0
        }
        fn layout(&mut self, _cx: &mut LayoutContext, _b: Rect) {}
    }

    let view = ScrollView::new(Fixed(Vec2::new(80.0, 400.0)));
    let (mut arena, root) = arena_with(view, Rect::new(0.0, 0.0, 100.0, 100.0));
    let mut adapter = AccessKitAdapter::new(root);

    let update = adapter.build_update(&mut arena);
    let (_, region) =
        find_node(&update, |n| n.role() == Role::ScrollView).expect("scroll region emitted");
    assert_eq!(region.scroll_y(), Some(0.0));
    assert_eq!(region.scroll_y_min(), Some(0.0));
    assert_eq!(region.scroll_y_max(), Some(300.0));
    assert!(region.supports_action(Action::ScrollDown));
    assert!(region.supports_action(Action::SetScrollOffset));
    // The vertical scrollbar is emitted visible; the horizontal bar is
    // emitted hidden (no horizontal overflow).
    let bars = find_all(&update, |n| n.role() == Role::ScrollBar && !n.is_hidden());
    assert_eq!(bars.len(), 1);

    // A semantic ScrollDown through the arena updates the emitted
    // scroll offset.
    arena.dispatch_event(
        root,
        &WidgetEvent::SemanticAction(SemanticAction::ScrollDown),
    );
    let update = adapter.build_update(&mut arena);
    let (_, region) = find_node(&update, |n| n.role() == Role::ScrollView).unwrap();
    assert_eq!(region.scroll_y(), Some(48.0));
}

// ---------------------------------------------------------------------------
// Dropdown + overlay
// ---------------------------------------------------------------------------

#[test]
fn dropdown_popup_emits_listbox_and_relations() {
    let mut dd = Dropdown::new(["Red", "Green", "Blue"]).label("Colour");
    {
        let mut hot = HotNode::default();
        let mut cx = LayoutContext { hot: &mut hot };
        dd.layout(&mut cx, Rect::new(10.0, 10.0, 160.0, 32.0));
    }
    dd.open();
    let mut overlay = OverlayLayer::new();
    overlay.set_viewport(Rect::new(0.0, 0.0, 800.0, 600.0));
    dd.sync_overlay(&mut overlay);
    overlay.layout_pass();
    let popup_id = dd.popup_id().expect("popup opened");

    let (mut arena, root) = arena_with(dd, Rect::new(10.0, 10.0, 160.0, 32.0));
    let mut adapter = AccessKitAdapter::new(root);
    let update = adapter.build_update_with_overlay(&mut arena, &overlay);

    // Combobox: expanded, controls → listbox, activedescendant →
    // highlighted option.
    let (_, combo) = find_node(&update, |n| n.role() == Role::ComboBox).expect("combobox emitted");
    assert_eq!(combo.is_expanded(), Some(true));
    let controls = combo.controls();
    let listbox = find_node(&update, |n| n.role() == Role::ListBox).expect("popup listbox emitted");
    assert_eq!(controls, &[listbox.0]);
    let options = find_all(&update, |n| n.role() == Role::ListBoxOption);
    assert_eq!(options.len(), 3);
    let active = combo
        .active_descendant()
        .expect("activedescendant set while open");
    assert_eq!(active, options[0].0); // highlighted == selected == 0

    // The popup root is a top-level child of the tree root.
    let tree_root = widget_id_to_node_id(root);
    let (_, tree_root_node) = update
        .nodes
        .iter()
        .find(|(id, _)| *id == tree_root)
        .expect("tree root node");
    assert!(tree_root_node.children().contains(&listbox.0));

    // The popup's virtual nodes resolve back to (entry, path) pairs
    // for action dispatch.
    let (entry, path) = adapter
        .resolve_overlay(options[1].0)
        .expect("option resolves to overlay path");
    assert_eq!(entry, popup_id);
    assert_eq!(path, &[0, 0, 1]);
}

// ---------------------------------------------------------------------------
// Tooltip + overlay
// ---------------------------------------------------------------------------

#[test]
fn tooltip_described_by_wires_to_bubble() {
    let mut tip = Tooltip::new(Text::new("Save"), "Save the document");
    {
        let mut hot = HotNode::default();
        let mut cx = LayoutContext { hot: &mut hot };
        tip.layout(&mut cx, Rect::new(10.0, 10.0, 100.0, 40.0));
    }
    tip.show();
    let mut overlay = OverlayLayer::new();
    overlay.set_viewport(Rect::new(0.0, 0.0, 800.0, 600.0));
    tip.sync_overlay(&mut overlay);
    overlay.layout_pass();

    let (mut arena, root) = arena_with(tip, Rect::new(10.0, 10.0, 100.0, 40.0));
    let mut adapter = AccessKitAdapter::new(root);
    let update = adapter.build_update_with_overlay(&mut arena, &overlay);

    let (bubble_id, bubble) =
        find_node(&update, |n| n.role() == Role::Tooltip).expect("tooltip bubble emitted");
    assert_eq!(bubble.label(), Some("Save the document"));

    // The trigger node describes-by the bubble.
    let trigger = find_node(&update, |n| n.described_by().contains(bubble_id));
    assert!(trigger.is_some(), "trigger has aria-describedby → bubble");
}
