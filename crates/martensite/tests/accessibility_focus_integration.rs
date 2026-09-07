//! Integration tests for accessibility and focus subsystems.
//!
//! These tests verify that:
//! 1. The AccessKit adapter correctly builds tree updates from a widget
//!    arena populated with real Martensite widgets.
//! 2. All standard library widgets expose valid AccessKit roles.
//! 3. The focus manager correctly navigates between widgets in the arena.
//! 4. Modal focus scopes trap navigation correctly.
//! 5. Spatial navigation works across a grid of widgets.
//! 6. Accessibility actions are correctly dispatched.

#![forbid(unsafe_code)]

use martensite::widgets::{Button, CheckBox, Container, Flex, Stack, Text, TextInput};
use martensite_access::{
    actions::{decode_action_request, A11yAction, ActionHandler, QueuedActionDispatcher},
    properties::AccessibilityBuilder,
    widget_id_to_node_id, AccessKitAdapter,
};
use martensite_core::widget::Widget;
use martensite_core::{ColdNode, HotNode, NodeFlags, Rect, WidgetArena, WidgetId};
use martensite_focus::{FocusDirection, FocusManager, TabNavigation};

/// Inserts a widget into the arena with default hot/cold nodes and returns its ID.
fn insert_widget(arena: &mut WidgetArena, widget: impl Widget + 'static) -> WidgetId {
    arena.insert(HotNode::default(), ColdNode::new(Box::new(widget)))
}

/// Inserts a focusable, visible widget at the given bounds.
fn insert_focusable_at(
    arena: &mut WidgetArena,
    widget: impl Widget + 'static,
    bounds: Rect,
) -> WidgetId {
    let hot = HotNode {
        bounds,
        flags: NodeFlags::FOCUSABLE | NodeFlags::VISIBLE,
        ..Default::default()
    };
    arena.insert(hot, ColdNode::new(Box::new(widget)))
}

// ===========================================================================
// Accessibility: Tree Update Generation
// ===========================================================================

#[test]
fn accesskit_adapter_builds_tree_from_real_widgets() {
    let mut arena = WidgetArena::new();

    let root = insert_widget(&mut arena, Container::new());
    let child1 = insert_widget(&mut arena, Text::new("Hello"));
    let child2 = insert_widget(&mut arena, Text::new("World"));
    arena.append_child(root, child1).unwrap();
    arena.append_child(root, child2).unwrap();

    let mut adapter = AccessKitAdapter::new(root);
    let update = adapter.build_update(&mut arena);

    // Should have 3 nodes: root + 2 children.
    assert_eq!(update.nodes.len(), 3);

    // Root should have children.
    let root_node = &update.nodes[0].1;
    assert_eq!(root_node.children().len(), 2);

    // Root should be a GenericContainer.
    assert_eq!(root_node.role(), accesskit::Role::GenericContainer);
}

#[test]
fn accesskit_adapter_text_widget_has_text_run_role() {
    let mut arena = WidgetArena::new();
    let root = insert_widget(&mut arena, Text::new("Hello World"));

    let mut adapter = AccessKitAdapter::new(root);
    let update = adapter.build_update(&mut arena);

    let node = &update.nodes[0].1;
    assert_eq!(node.role(), accesskit::Role::TextRun);
    assert_eq!(node.value(), Some("Hello World"));
}

#[test]
fn accesskit_adapter_all_widgets_have_valid_roles() {
    let mut arena = WidgetArena::new();

    let container = insert_widget(&mut arena, Container::new());
    let flex = insert_widget(&mut arena, Flex::row());
    let stack = insert_widget(&mut arena, Stack::new());
    let text = insert_widget(&mut arena, Text::new("Label"));
    arena.append_child(container, flex).unwrap();
    arena.append_child(container, stack).unwrap();
    arena.append_child(container, text).unwrap();

    let mut adapter = AccessKitAdapter::new(container);
    let update = adapter.build_update(&mut arena);

    // Every node should have a non-Unknown role.
    for (_, node) in &update.nodes {
        assert_ne!(
            node.role(),
            accesskit::Role::Unknown,
            "every widget must have a valid (non-Unknown) AccessKit role"
        );
    }
}

#[test]
fn accesskit_adapter_focusable_widget_has_focus_action() {
    let mut arena = WidgetArena::new();
    let id = insert_focusable_at(
        &mut arena,
        Text::new("Button"),
        Rect::new(0.0, 0.0, 100.0, 30.0),
    );

    let mut adapter = AccessKitAdapter::new(id);
    let update = adapter.build_update(&mut arena);

    let node = &update.nodes[0].1;
    assert!(node.supports_action(accesskit::Action::Focus));
}

#[test]
fn accesskit_adapter_sets_bounds_from_layout() {
    let mut arena = WidgetArena::new();
    let id = insert_focusable_at(
        &mut arena,
        Text::new("Hello"),
        Rect::new(10.0, 20.0, 100.0, 50.0),
    );

    let mut adapter = AccessKitAdapter::new(id);
    let update = adapter.build_update(&mut arena);

    let node = &update.nodes[0].1;
    let bounds = node.bounds().unwrap();
    assert_eq!(bounds.x0, 10.0);
    assert_eq!(bounds.y0, 20.0);
    assert_eq!(bounds.x1, 110.0);
    assert_eq!(bounds.y1, 70.0);
}

#[test]
fn accesskit_adapter_incremental_update_only_dirty() {
    let mut arena = WidgetArena::new();
    let root = insert_widget(&mut arena, Container::new());
    let child1 = insert_widget(&mut arena, Text::new("A"));
    let child2 = insert_widget(&mut arena, Text::new("B"));
    arena.append_child(root, child1).unwrap();
    arena.append_child(root, child2).unwrap();

    // Mark only child1 as dirty.
    let mut adapter = AccessKitAdapter::new(root);
    adapter.mark_dirty(&mut arena, child1);

    let update = adapter.build_incremental_update(&mut arena).unwrap();
    // Should include child1 (dirty) and root (its parent).
    assert!(!update.nodes.is_empty());
    let node_ids: Vec<_> = update.nodes.iter().map(|(id, _)| *id).collect();
    assert!(node_ids.contains(&widget_id_to_node_id(child1)));
}

#[test]
fn accesskit_adapter_focus_reflected_in_update() {
    let mut arena = WidgetArena::new();
    let root = insert_widget(&mut arena, Container::new());
    let child = insert_focusable_at(
        &mut arena,
        Text::new("Focusable"),
        Rect::new(0.0, 0.0, 50.0, 20.0),
    );
    arena.append_child(root, child).unwrap();

    let mut adapter = AccessKitAdapter::new(root);
    adapter.set_focus(Some(child));
    let update = adapter.build_update(&mut arena);
    assert_eq!(update.focus, widget_id_to_node_id(child));
}

// ===========================================================================
// Accessibility: Action Dispatching
// ===========================================================================

#[test]
fn action_dispatch_click_resolves_to_widget() {
    let mut arena = WidgetArena::new();
    let id = insert_focusable_at(
        &mut arena,
        Text::new("Click me"),
        Rect::new(0.0, 0.0, 80.0, 30.0),
    );

    let request = accesskit::ActionRequest {
        action: accesskit::Action::Click,
        target_node: widget_id_to_node_id(id),
        target_tree: accesskit::TreeId::ROOT,
        data: None,
    };

    let action = decode_action_request(&arena, &request, &accesskit::TreeId::ROOT).unwrap();
    assert_eq!(action, A11yAction::Click(id));
}

#[test]
fn action_dispatch_focus_resolves_to_widget() {
    let mut arena = WidgetArena::new();
    let id = insert_focusable_at(
        &mut arena,
        Text::new("Focus me"),
        Rect::new(0.0, 0.0, 80.0, 30.0),
    );

    let request = accesskit::ActionRequest {
        action: accesskit::Action::Focus,
        target_node: widget_id_to_node_id(id),
        target_tree: accesskit::TreeId::ROOT,
        data: None,
    };

    let action = decode_action_request(&arena, &request, &accesskit::TreeId::ROOT).unwrap();
    assert_eq!(action, A11yAction::Focus(id));
}

#[test]
fn action_dispatch_queued_handler() {
    let mut arena = WidgetArena::new();
    let id = insert_focusable_at(
        &mut arena,
        Text::new("Test"),
        Rect::new(0.0, 0.0, 50.0, 20.0),
    );

    let mut dispatcher = QueuedActionDispatcher::new();
    dispatcher.handle_action(&mut arena, &A11yAction::Click(id));
    dispatcher.handle_action(&mut arena, &A11yAction::Focus(id));

    let actions = dispatcher.drain();
    assert_eq!(actions.len(), 2);
    assert_eq!(actions[0], A11yAction::Click(id));
    assert_eq!(actions[1], A11yAction::Focus(id));
}

// ===========================================================================
// Accessibility: Property Builder
// ===========================================================================

#[test]
fn accessibility_builder_applies_to_text_widget_node() {
    let mut arena = WidgetArena::new();
    let _id = insert_widget(&mut arena, Text::new("Submit"));

    let mut adapter = AccessKitAdapter::new(_id);
    let update = adapter.build_update(&mut arena);

    // The Text widget's accessibility hook sets role to TextRun and value.
    let node = &update.nodes[0].1;
    assert_eq!(node.role(), accesskit::Role::TextRun);
    assert_eq!(node.value(), Some("Submit"));
}

#[test]
fn accessibility_builder_creates_button_node() {
    let mut node = accesskit::Node::new(accesskit::Role::Unknown);
    AccessibilityBuilder::new(accesskit::Role::Button)
        .label("OK")
        .focusable()
        .clickable()
        .apply(&mut node);

    assert_eq!(node.role(), accesskit::Role::Button);
    assert_eq!(node.label(), Some("OK"));
    assert!(node.supports_action(accesskit::Action::Focus));
    assert!(node.supports_action(accesskit::Action::Click));
}

// ===========================================================================
// Focus: Tab Navigation
// ===========================================================================

#[test]
fn focus_tab_navigation_cycles_through_widgets() {
    let mut arena = WidgetArena::new();
    let a = insert_focusable_at(&mut arena, Text::new("A"), Rect::new(0.0, 0.0, 50.0, 20.0));
    let b = insert_focusable_at(&mut arena, Text::new("B"), Rect::new(60.0, 0.0, 50.0, 20.0));
    let c = insert_focusable_at(
        &mut arena,
        Text::new("C"),
        Rect::new(120.0, 0.0, 50.0, 20.0),
    );

    let mut manager = FocusManager::new();

    // Tab forward: A → B → C → A (wraps).
    assert_eq!(manager.tab(&arena, TabNavigation::Forward), Some(a));
    assert_eq!(manager.tab(&arena, TabNavigation::Forward), Some(b));
    assert_eq!(manager.tab(&arena, TabNavigation::Forward), Some(c));
    assert_eq!(manager.tab(&arena, TabNavigation::Forward), Some(a));

    // Tab reverse from A: wraps to C → B → A.
    assert_eq!(manager.tab(&arena, TabNavigation::Reverse), Some(c));
    assert_eq!(manager.tab(&arena, TabNavigation::Reverse), Some(b));
    assert_eq!(manager.tab(&arena, TabNavigation::Reverse), Some(a));
}

#[test]
fn focus_tab_skips_non_focusable_widgets() {
    let mut arena = WidgetArena::new();
    let a = insert_focusable_at(&mut arena, Text::new("A"), Rect::new(0.0, 0.0, 50.0, 20.0));
    let _non = insert_widget(&mut arena, Text::new("non-focusable"));
    let b = insert_focusable_at(
        &mut arena,
        Text::new("B"),
        Rect::new(120.0, 0.0, 50.0, 20.0),
    );

    let mut manager = FocusManager::new();
    assert_eq!(manager.tab(&arena, TabNavigation::Forward), Some(a));
    assert_eq!(manager.tab(&arena, TabNavigation::Forward), Some(b));
}

// ===========================================================================
// Focus: Spatial Navigation
// ===========================================================================

#[test]
fn focus_spatial_navigation_grid() {
    let mut arena = WidgetArena::new();

    // 3x3 grid with irregular spacing.
    let positions = [
        (0.0, 0.0),
        (60.0, 0.0),
        (130.0, 0.0),
        (0.0, 50.0),
        (60.0, 50.0),
        (130.0, 50.0),
        (0.0, 120.0),
        (60.0, 120.0),
        (130.0, 120.0),
    ];

    let mut ids = Vec::new();
    for &(x, y) in &positions {
        ids.push(insert_focusable_at(
            &mut arena,
            Text::new("btn"),
            Rect::new(x, y, 20.0, 20.0),
        ));
    }

    let mut manager = FocusManager::new();
    manager.set_focus(&mut arena, ids[4]); // center

    // Right from center → right-center.
    assert_eq!(
        manager.navigate(&arena, FocusDirection::Right),
        Some(ids[5])
    );
    // Left from center → left-center.
    manager.set_focus(&mut arena, ids[4]);
    assert_eq!(manager.navigate(&arena, FocusDirection::Left), Some(ids[3]));
    // Up from center → top-center.
    manager.set_focus(&mut arena, ids[4]);
    assert_eq!(manager.navigate(&arena, FocusDirection::Up), Some(ids[1]));
    // Down from center → bottom-center.
    manager.set_focus(&mut arena, ids[4]);
    assert_eq!(manager.navigate(&arena, FocusDirection::Down), Some(ids[7]));
}

#[test]
fn focus_spatial_navigation_no_candidates_returns_none() {
    let mut arena = WidgetArena::new();
    let id = insert_focusable_at(
        &mut arena,
        Text::new("lonely"),
        Rect::new(0.0, 0.0, 50.0, 20.0),
    );

    let mut manager = FocusManager::new();
    manager.set_focus(&mut arena, id);
    assert_eq!(manager.navigate(&arena, FocusDirection::Right), None);
}

// ===========================================================================
// Focus: Modal Scope Trapping
// ===========================================================================

#[test]
fn focus_modal_scope_traps_tab_navigation() {
    let mut arena = WidgetArena::new();
    let outside = insert_focusable_at(
        &mut arena,
        Text::new("outside"),
        Rect::new(0.0, 0.0, 50.0, 20.0),
    );
    let modal_root = insert_focusable_at(
        &mut arena,
        Container::new(),
        Rect::new(100.0, 100.0, 200.0, 200.0),
    );
    let modal_child = insert_focusable_at(
        &mut arena,
        Text::new("modal"),
        Rect::new(110.0, 110.0, 50.0, 20.0),
    );
    arena.append_child(modal_root, modal_child).unwrap();

    let mut manager = FocusManager::new();
    manager.set_focus(&mut arena, outside);
    manager.push_scope(&arena, modal_root);

    // Tab should only cycle within the modal scope.
    let next = manager.tab(&arena, TabNavigation::Forward);
    assert!(next == Some(modal_root) || next == Some(modal_child));
    let next = manager.tab(&arena, TabNavigation::Forward);
    assert!(next == Some(modal_root) || next == Some(modal_child));

    // Should never escape to outside.
    assert_ne!(manager.current_focus(), Some(outside));
}

#[test]
fn focus_modal_scope_restores_focus_on_pop() {
    let mut arena = WidgetArena::new();
    let prior = insert_focusable_at(
        &mut arena,
        Text::new("prior"),
        Rect::new(0.0, 0.0, 50.0, 20.0),
    );
    let modal = insert_focusable_at(
        &mut arena,
        Container::new(),
        Rect::new(100.0, 100.0, 200.0, 200.0),
    );

    let mut manager = FocusManager::new();
    manager.set_focus(&mut arena, prior);
    manager.push_scope(&arena, modal);
    manager.set_focus(&mut arena, modal);

    // Pop scope — should restore to prior.
    let restored = manager.pop_scope(&arena);
    assert_eq!(restored, Some(prior));
    assert_eq!(manager.current_focus(), Some(prior));
}

#[test]
fn focus_modal_scope_pop_fallback_when_prior_dead() {
    let mut arena = WidgetArena::new();
    let prior = insert_focusable_at(
        &mut arena,
        Text::new("prior"),
        Rect::new(0.0, 0.0, 50.0, 20.0),
    );
    let modal = insert_focusable_at(
        &mut arena,
        Container::new(),
        Rect::new(100.0, 100.0, 200.0, 200.0),
    );
    let _fallback = insert_focusable_at(
        &mut arena,
        Text::new("fallback"),
        Rect::new(200.0, 0.0, 50.0, 20.0),
    );

    let mut manager = FocusManager::new();
    manager.set_focus(&mut arena, prior);
    manager.push_scope(&arena, modal);

    // Remove prior while modal is active.
    arena.remove(prior);

    // Pop — should fall back to a focusable widget.
    let restored = manager.pop_scope(&arena);
    assert!(restored.is_some());
    assert_ne!(restored, Some(prior));
}

#[test]
fn focus_nested_modal_scopes() {
    let mut arena = WidgetArena::new();
    let outer = insert_focusable_at(
        &mut arena,
        Container::new(),
        Rect::new(0.0, 0.0, 400.0, 400.0),
    );
    let inner = insert_focusable_at(
        &mut arena,
        Container::new(),
        Rect::new(50.0, 50.0, 200.0, 200.0),
    );
    let inner_child = insert_focusable_at(
        &mut arena,
        Text::new("deep"),
        Rect::new(60.0, 60.0, 50.0, 20.0),
    );
    arena.append_child(outer, inner).unwrap();
    arena.append_child(inner, inner_child).unwrap();

    let mut manager = FocusManager::new();
    manager.set_focus(&mut arena, outer);
    manager.push_scope(&arena, outer);
    manager.set_focus(&mut arena, inner);
    manager.push_scope(&arena, inner);
    manager.set_focus(&mut arena, inner_child);

    // Pop inner scope — should restore to inner.
    let restored = manager.pop_scope(&arena);
    assert_eq!(restored, Some(inner));

    // Pop outer scope — should restore to outer.
    let restored = manager.pop_scope(&arena);
    assert_eq!(restored, Some(outer));

    // No more scopes.
    assert!(!manager.has_active_scope());
}

// ===========================================================================
// Combined: Accessibility + Focus
// ===========================================================================

#[test]
fn access_adapter_and_focus_manager_work_together() {
    let mut arena = WidgetArena::new();
    let root = insert_focusable_at(
        &mut arena,
        Container::new(),
        Rect::new(0.0, 0.0, 400.0, 300.0),
    );
    let btn1 = insert_focusable_at(
        &mut arena,
        Text::new("OK"),
        Rect::new(10.0, 10.0, 80.0, 30.0),
    );
    let btn2 = insert_focusable_at(
        &mut arena,
        Text::new("Cancel"),
        Rect::new(100.0, 10.0, 80.0, 30.0),
    );
    arena.append_child(root, btn1).unwrap();
    arena.append_child(root, btn2).unwrap();

    // Build accessibility tree.
    let mut adapter = AccessKitAdapter::new(root);
    let update = adapter.build_update(&mut arena);
    assert_eq!(update.nodes.len(), 3);

    // All focusable widgets should have Focus action.
    for (_, node) in &update.nodes {
        assert!(
            node.supports_action(accesskit::Action::Focus),
            "all focusable widgets should expose Focus action"
        );
    }

    // Use focus manager to navigate.
    let mut manager = FocusManager::new();
    manager.set_focus(&mut arena, btn1);
    adapter.set_focus(Some(btn1));

    // Spatial navigation: right from btn1 → btn2.
    let next = manager.navigate(&arena, FocusDirection::Right);
    assert_eq!(next, Some(btn2));

    // Update adapter focus to match.
    adapter.set_focus(Some(btn2));
    let update = adapter.build_update(&mut arena);
    assert_eq!(update.focus, widget_id_to_node_id(btn2));
}

#[test]
fn all_stdlib_widgets_have_non_unknown_roles() {
    let mut arena = WidgetArena::new();

    let container = insert_widget(&mut arena, Container::new());
    let flex = insert_widget(&mut arena, Flex::row());
    let stack = insert_widget(&mut arena, Stack::new());
    let text = insert_widget(&mut arena, Text::new("text"));
    // All widgets must be in the subtree of the adapter root.
    arena.append_child(container, flex).unwrap();
    arena.append_child(container, stack).unwrap();
    arena.append_child(container, text).unwrap();

    // All widgets should produce valid accessibility nodes.
    let mut adapter = AccessKitAdapter::new(container);
    let update = adapter.build_update(&mut arena);

    let roles: Vec<accesskit::Role> = update.nodes.iter().map(|(_, n)| n.role()).collect();

    // None should be Unknown.
    for role in &roles {
        assert_ne!(
            *role,
            accesskit::Role::Unknown,
            "widget has Unknown role — all stdlib widgets must declare a valid role"
        );
    }

    // Container should be GenericContainer.
    assert!(roles.contains(&accesskit::Role::GenericContainer));
    // Text should be TextRun.
    assert!(roles.contains(&accesskit::Role::TextRun));
}

// ===========================================================================
// Interactive Standard Widgets: Accessibility Roles, Labels, Actions
// ===========================================================================

#[test]
fn button_widget_exposes_button_role_label_and_click_action() {
    let mut arena = WidgetArena::new();
    let id = insert_focusable_at(
        &mut arena,
        Button::new("Submit"),
        Rect::new(0.0, 0.0, 100.0, 32.0),
    );

    let mut adapter = AccessKitAdapter::new(id);
    let update = adapter.build_update(&mut arena);
    let node = &update.nodes[0].1;

    assert_eq!(node.role(), accesskit::Role::Button);
    assert_eq!(node.label(), Some("Submit"));
    assert!(node.supports_action(accesskit::Action::Click));
    assert!(node.supports_action(accesskit::Action::Focus));
}

#[test]
fn button_widget_disabled_sets_disabled_state() {
    let mut arena = WidgetArena::new();
    let id = insert_focusable_at(
        &mut arena,
        Button::new("Submit").enabled(false),
        Rect::new(0.0, 0.0, 100.0, 32.0),
    );

    let mut adapter = AccessKitAdapter::new(id);
    let update = adapter.build_update(&mut arena);
    let node = &update.nodes[0].1;
    assert!(node.is_disabled());
}

#[test]
fn checkbox_widget_exposes_checkbox_role_label_and_toggled_state() {
    let mut arena = WidgetArena::new();
    let id = insert_focusable_at(
        &mut arena,
        CheckBox::new("Accept terms").checked(true),
        Rect::new(0.0, 0.0, 20.0, 20.0),
    );

    let mut adapter = AccessKitAdapter::new(id);
    let update = adapter.build_update(&mut arena);
    let node = &update.nodes[0].1;

    assert_eq!(node.role(), accesskit::Role::CheckBox);
    assert_eq!(node.label(), Some("Accept terms"));
    assert_eq!(node.toggled(), Some(accesskit::Toggled::True));
    assert!(node.supports_action(accesskit::Action::Click));
    assert!(node.supports_action(accesskit::Action::Focus));
}

#[test]
fn checkbox_widget_unchecked_has_false_toggled() {
    let mut arena = WidgetArena::new();
    let id = insert_focusable_at(
        &mut arena,
        CheckBox::new("Decline"),
        Rect::new(0.0, 0.0, 20.0, 20.0),
    );

    let mut adapter = AccessKitAdapter::new(id);
    let update = adapter.build_update(&mut arena);
    let node = &update.nodes[0].1;
    assert_eq!(node.toggled(), Some(accesskit::Toggled::False));
}

#[test]
fn text_input_widget_exposes_text_input_role_label_and_value() {
    let mut arena = WidgetArena::new();
    let id = insert_focusable_at(
        &mut arena,
        TextInput::new("Email").value("user@test.com"),
        Rect::new(0.0, 0.0, 120.0, 24.0),
    );

    let mut adapter = AccessKitAdapter::new(id);
    let update = adapter.build_update(&mut arena);
    let node = &update.nodes[0].1;

    assert_eq!(node.role(), accesskit::Role::TextInput);
    assert_eq!(node.label(), Some("Email"));
    assert_eq!(node.value(), Some("user@test.com"));
    assert!(node.supports_action(accesskit::Action::Focus));
    assert!(node.supports_action(accesskit::Action::SetValue));
}

#[test]
fn text_input_read_only_no_set_value_action() {
    let mut arena = WidgetArena::new();
    let id = insert_focusable_at(
        &mut arena,
        TextInput::new("Read Only").read_only(true),
        Rect::new(0.0, 0.0, 120.0, 24.0),
    );

    let mut adapter = AccessKitAdapter::new(id);
    let update = adapter.build_update(&mut arena);
    let node = &update.nodes[0].1;
    assert!(!node.supports_action(accesskit::Action::SetValue));
}

#[test]
fn all_interactive_widgets_have_valid_roles_labels_and_actions() {
    let mut arena = WidgetArena::new();
    let root = insert_widget(&mut arena, Container::new());
    let button = insert_focusable_at(
        &mut arena,
        Button::new("OK"),
        Rect::new(10.0, 10.0, 80.0, 32.0),
    );
    let checkbox = insert_focusable_at(
        &mut arena,
        CheckBox::new("Agree"),
        Rect::new(10.0, 50.0, 20.0, 20.0),
    );
    let text_input = insert_focusable_at(
        &mut arena,
        TextInput::new("Name"),
        Rect::new(10.0, 80.0, 120.0, 24.0),
    );
    arena.append_child(root, button).unwrap();
    arena.append_child(root, checkbox).unwrap();
    arena.append_child(root, text_input).unwrap();

    let mut adapter = AccessKitAdapter::new(root);
    let update = adapter.build_update(&mut arena);

    // Every interactive widget should have a non-Unknown role, a label,
    // and at least the Focus action.
    for (id, node) in &update.nodes {
        if id == &widget_id_to_node_id(root) {
            continue; // Container is not interactive.
        }
        assert_ne!(
            node.role(),
            accesskit::Role::Unknown,
            "interactive widget must have a valid role"
        );
        assert!(
            node.label().is_some(),
            "interactive widget must have an accessible label"
        );
        assert!(
            node.supports_action(accesskit::Action::Focus),
            "interactive widget must expose Focus action"
        );
    }

    // Button should have Click action.
    let btn_node = update
        .nodes
        .iter()
        .find(|(id, _)| *id == widget_id_to_node_id(button))
        .map(|(_, n)| n)
        .unwrap();
    assert!(btn_node.supports_action(accesskit::Action::Click));

    // CheckBox should have Click action and toggled state.
    let cb_node = update
        .nodes
        .iter()
        .find(|(id, _)| *id == widget_id_to_node_id(checkbox))
        .map(|(_, n)| n)
        .unwrap();
    assert!(cb_node.supports_action(accesskit::Action::Click));
    assert!(cb_node.toggled().is_some());

    // TextInput should have SetValue action.
    let ti_node = update
        .nodes
        .iter()
        .find(|(id, _)| *id == widget_id_to_node_id(text_input))
        .map(|(_, n)| n)
        .unwrap();
    assert!(ti_node.supports_action(accesskit::Action::SetValue));
}

// ===========================================================================
// Focus: Additional Review-Gap Tests
// ===========================================================================

#[test]
fn shift_tab_trapped_inside_modal_scope() {
    let mut arena = WidgetArena::new();
    let outside = insert_focusable_at(
        &mut arena,
        Text::new("outside"),
        Rect::new(0.0, 0.0, 50.0, 20.0),
    );
    let modal_root = insert_focusable_at(
        &mut arena,
        Container::new(),
        Rect::new(100.0, 100.0, 200.0, 200.0),
    );
    let modal_child = insert_focusable_at(
        &mut arena,
        Text::new("modal"),
        Rect::new(110.0, 110.0, 50.0, 20.0),
    );
    arena.append_child(modal_root, modal_child).unwrap();

    let mut manager = FocusManager::new();
    manager.set_focus(&mut arena, outside);
    manager.push_scope(&arena, modal_root);

    // Shift+Tab should only cycle within the modal scope.
    let next = manager.tab(&arena, TabNavigation::Reverse);
    assert!(next == Some(modal_root) || next == Some(modal_child));
    let next = manager.tab(&arena, TabNavigation::Reverse);
    assert!(next == Some(modal_root) || next == Some(modal_child));

    // Should never escape to outside.
    assert_ne!(manager.current_focus(), Some(outside));
}

#[test]
fn spatial_navigation_trapped_inside_modal_scope() {
    let mut arena = WidgetArena::new();
    let outside = insert_focusable_at(
        &mut arena,
        Text::new("outside"),
        Rect::new(0.0, 0.0, 50.0, 20.0),
    );
    let modal_root = insert_focusable_at(
        &mut arena,
        Container::new(),
        Rect::new(100.0, 100.0, 200.0, 200.0),
    );
    let modal_child = insert_focusable_at(
        &mut arena,
        Text::new("modal"),
        Rect::new(200.0, 100.0, 50.0, 20.0),
    );
    arena.append_child(modal_root, modal_child).unwrap();

    let mut manager = FocusManager::new();
    manager.set_focus(&mut arena, outside);
    manager.push_scope(&arena, modal_root);

    // Navigate right — should stay within the modal scope.
    let next = manager.navigate(&arena, FocusDirection::Right);
    assert!(next == Some(modal_root) || next == Some(modal_child));
    assert_ne!(next, Some(outside));
}

#[test]
fn programmatic_set_focus_cannot_escape_modal_scope() {
    let mut arena = WidgetArena::new();
    let outside = insert_focusable_at(
        &mut arena,
        Text::new("outside"),
        Rect::new(0.0, 0.0, 50.0, 20.0),
    );
    let modal_root = insert_focusable_at(
        &mut arena,
        Container::new(),
        Rect::new(100.0, 100.0, 200.0, 200.0),
    );

    let mut manager = FocusManager::new();
    manager.set_focus(&mut arena, modal_root);
    manager.push_scope(&arena, modal_root);

    // Attempting to set focus outside the scope should fail.
    manager.set_focus(&mut arena, outside);
    assert_ne!(manager.current_focus(), Some(outside));
}

#[test]
fn inert_widget_cannot_receive_tab_focus() {
    let mut arena = WidgetArena::new();
    let a = insert_focusable_at(&mut arena, Text::new("A"), Rect::new(0.0, 0.0, 50.0, 20.0));
    // Insert an inert (disabled) but focusable+visible widget.
    let hot = HotNode {
        bounds: Rect::new(60.0, 0.0, 50.0, 20.0),
        flags: NodeFlags::FOCUSABLE | NodeFlags::VISIBLE | NodeFlags::INERT,
        ..Default::default()
    };
    let _inert = arena.insert(hot, ColdNode::new(Box::new(Text::new("disabled"))));
    let b = insert_focusable_at(
        &mut arena,
        Text::new("B"),
        Rect::new(120.0, 0.0, 50.0, 20.0),
    );

    let mut manager = FocusManager::new();
    assert_eq!(manager.tab(&arena, TabNavigation::Forward), Some(a));
    // Should skip the inert widget.
    assert_eq!(manager.tab(&arena, TabNavigation::Forward), Some(b));
}

#[test]
fn incremental_update_propagates_parent_on_child_removal() {
    let mut arena = WidgetArena::new();
    let root = insert_widget(&mut arena, Container::new());
    let child = insert_widget(&mut arena, Text::new("child"));
    arena.append_child(root, child).unwrap();

    let mut adapter = AccessKitAdapter::new(root);
    // Initialize with a full update.
    let _ = adapter.build_update(&mut arena);

    // Remove the child.
    arena.remove(child);
    // Mark root as dirty (simulating what the framework would do).
    adapter.mark_dirty(&mut arena, root);

    let update = adapter.build_incremental_update(&mut arena).unwrap();
    // Root should be in the update with an updated (empty) children list.
    let root_node = update
        .nodes
        .iter()
        .find(|(id, _)| *id == widget_id_to_node_id(root))
        .map(|(_, n)| n);
    assert!(
        root_node.is_some(),
        "parent root should be in incremental update"
    );
    let root_node = root_node.unwrap();
    assert_eq!(
        root_node.children().len(),
        0,
        "root should have no children after child removal"
    );
}
