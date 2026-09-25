//! Comprehensive unit and integration tests for the in-app widget inspector.
//!
//! Validates:
//! - Shortcuts, key combos, and `DevToolsOptions`.
//! - `InspectorState` transitions and select-mode arming.
//! - Select-mode hit-testing equivalence to production router.
//! - Lazy tree model: on-demand expansion and virtualized list handling ($O(\text{visible})$).
//! - Layout inspector: constraint chain computation, underflow & overflow detection.
//! - Properties view: metadata extraction, inline `@` marker parsing, source spans, and signal tracking.
#![cfg(feature = "devtools")]
#![forbid(unsafe_code)]

use accesskit::Role;
use glam::Vec2;
use martensite_core::{
    ColdNode, DummyWidget, HotNode, LayoutConstraints, NodeFlags, Rect, RenderMinimum,
    UnderflowPolicy, WidgetArena,
};
use martensite_devtools::inspector::{
    hit_test_select, Axis, ConstraintViolation, DevToolsOptions, InlineMarkers, InspectionMode,
    InspectorState, InspectorTreeModel, KeyCode, KeyCombo, LayoutInspector, Modifiers, NodeBadges,
    NodeKind, TrackedSignalInfo, WidgetProperties,
};

#[test]
fn test_devtools_options_and_key_combos() {
    let options = DevToolsOptions::default();
    assert_eq!(options.inspector_key, KeyCombo::F12);
    assert_eq!(options.select_mode_key, KeyCombo::CTRL_SHIFT_C);
    assert!(options.enabled);
    assert_eq!(options.virtual_child_threshold, 50);
    assert_eq!(options.frame_budget_ns, 100_000);

    // Test KeyCombo matching
    assert!(KeyCombo::F12.matches(KeyCode::F12, Modifiers::NONE));
    assert!(!KeyCombo::F12.matches(KeyCode::F11, Modifiers::NONE));
    assert!(!KeyCombo::F12.matches(KeyCode::F12, Modifiers::SHIFT));

    assert!(
        KeyCombo::CTRL_SHIFT_C.matches(KeyCode::Char('C'), Modifiers::CONTROL | Modifiers::SHIFT)
    );
    assert!(!KeyCombo::CTRL_SHIFT_C.matches(KeyCode::Char('C'), Modifiers::CONTROL));

    // Test KeyCode parsing
    assert_eq!(KeyCode::from_name("F12"), Some(KeyCode::F12));
    assert_eq!(KeyCode::from_name("f1"), Some(KeyCode::F1));
    assert_eq!(KeyCode::from_name("Escape"), Some(KeyCode::Escape));
    assert_eq!(KeyCode::from_name("Esc"), Some(KeyCode::Escape));
    assert_eq!(KeyCode::from_name("Tab"), Some(KeyCode::Tab));
    assert_eq!(KeyCode::from_name("Enter"), Some(KeyCode::Enter));
    assert_eq!(KeyCode::from_name("Return"), Some(KeyCode::Enter));
    assert_eq!(KeyCode::from_name("Space"), Some(KeyCode::Space));
    assert_eq!(KeyCode::from_name("c"), Some(KeyCode::Char('C')));
    assert_eq!(KeyCode::from_name("Z"), Some(KeyCode::Char('Z')));

    // Test Builder methods
    let custom = DevToolsOptions::new()
        .with_inspector_key(KeyCombo::new(KeyCode::F10, Modifiers::NONE))
        .with_select_mode_key(KeyCombo::new(KeyCode::Char('I'), Modifiers::CONTROL))
        .with_enabled(false)
        .with_virtual_child_threshold(25)
        .with_frame_budget_ns(80_000);

    assert_eq!(custom.inspector_key.key, KeyCode::F10);
    assert_eq!(custom.select_mode_key.key, KeyCode::Char('I'));
    assert!(!custom.enabled);
    assert_eq!(custom.virtual_child_threshold, 25);
    assert_eq!(custom.frame_budget_ns, 80_000);
}

#[test]
fn test_inspector_state_transitions_and_hotkeys() {
    let mut state = InspectorState::new();
    assert!(!state.is_active());
    assert!(!state.is_select_mode_armed());
    assert_eq!(state.selected(), None);
    assert_eq!(state.hovered(), None);
    assert_eq!(state.mode(), InspectionMode::Tree);
    assert_eq!(state.mode().title(), "Elements");

    // Toggle active
    assert!(state.toggle_active());
    assert!(state.is_active());
    assert!(!state.toggle_active());
    assert!(!state.is_active());

    // Arm select mode opens the inspector
    state.arm_select_mode();
    assert!(state.is_select_mode_armed());
    assert!(state.is_active());

    // Disarm select mode
    state.disarm_select_mode();
    assert!(!state.is_select_mode_armed());
    assert!(state.is_active()); // Inspector remains open

    // Closing the inspector also disarms select mode
    state.arm_select_mode();
    state.set_active(false);
    assert!(!state.is_active());
    assert!(!state.is_select_mode_armed());

    // Key handling: F12 toggles active
    assert!(state.handle_key("F12", Modifiers::NONE));
    assert!(state.is_active());
    assert!(state.handle_key("F12", Modifiers::NONE));
    assert!(!state.is_active());

    // Key handling: Ctrl+Shift+C arms select mode and activates
    assert!(state.handle_key("C", Modifiers::CONTROL | Modifiers::SHIFT));
    assert!(state.is_select_mode_armed());
    assert!(state.is_active());

    // Modes and titles
    state.set_mode(InspectionMode::Layout);
    assert_eq!(state.mode(), InspectionMode::Layout);
    assert_eq!(state.mode().title(), "Layout");

    state.set_mode(InspectionMode::Properties);
    assert_eq!(state.mode().title(), "Properties");

    state.set_mode(InspectionMode::Lint);
    assert_eq!(state.mode().title(), "Design Lint");

    state.set_mode(InspectionMode::Accessibility);
    assert_eq!(state.mode().title(), "Accessibility");

    state.set_mode(InspectionMode::Events);
    assert_eq!(state.mode().title(), "Events");

    // Budget tracking
    state.record_collection_duration(60_000); // 0.06 ms
    assert!(state.overhead_under_budget());
    state.record_collection_duration(150_000); // 0.15 ms
    assert!(!state.overhead_under_budget());
}

#[test]
fn test_select_mode_hit_testing_with_ancestry_chain() {
    let mut arena = WidgetArena::new();

    // Scene Graph:
    // Root [0, 0, 800, 600]
    //   Panel [100, 100, 400, 300]
    //     Button [120, 120, 100, 40] (HIT_TEST_ENABLED)
    //     HiddenLabel [120, 180, 100, 40] (!VISIBLE)
    //     InertWidget [120, 240, 100, 40] (INERT)
    //   BackgroundDecor [0, 0, 800, 600] (!HIT_TEST_ENABLED)

    let root = arena.insert(
        HotNode {
            bounds: Rect::new(0.0, 0.0, 800.0, 600.0),
            flags: NodeFlags::VISIBLE | NodeFlags::HIT_TEST_ENABLED,
            ..HotNode::default()
        },
        ColdNode::new(Box::new(DummyWidget)).with_name("Root"),
    );

    let panel = arena.insert(
        HotNode {
            bounds: Rect::new(100.0, 100.0, 400.0, 300.0),
            flags: NodeFlags::VISIBLE, // Not directly hit-testable, but children are
            ..HotNode::default()
        },
        ColdNode::new(Box::new(DummyWidget)).with_name("Panel"),
    );
    arena.append_child(root, panel).unwrap();

    let button = arena.insert(
        HotNode {
            bounds: Rect::new(120.0, 120.0, 100.0, 40.0),
            flags: NodeFlags::VISIBLE | NodeFlags::HIT_TEST_ENABLED,
            ..HotNode::default()
        },
        ColdNode::new(Box::new(DummyWidget)).with_name("Button"),
    );
    arena.append_child(panel, button).unwrap();

    let hidden_label = arena.insert(
        HotNode {
            bounds: Rect::new(120.0, 180.0, 100.0, 40.0),
            flags: NodeFlags::HIT_TEST_ENABLED, // Not visible!
            ..HotNode::default()
        },
        ColdNode::new(Box::new(DummyWidget)).with_name("HiddenLabel"),
    );
    arena.append_child(panel, hidden_label).unwrap();

    let inert_widget = arena.insert(
        HotNode {
            bounds: Rect::new(120.0, 240.0, 100.0, 40.0),
            flags: NodeFlags::VISIBLE | NodeFlags::HIT_TEST_ENABLED | NodeFlags::INERT,
            ..HotNode::default()
        },
        ColdNode::new(Box::new(DummyWidget)).with_name("InertWidget"),
    );
    arena.append_child(panel, inert_widget).unwrap();

    // 1. Click directly inside Button: (150, 130)
    let hit = hit_test_select(&arena, root, Vec2::new(150.0, 130.0)).expect("should hit button");
    assert_eq!(hit.widget_id, button);
    assert_eq!(hit.local_point, Vec2::new(30.0, 10.0));
    assert_eq!(hit.screen_point, Vec2::new(150.0, 130.0));
    assert_eq!(hit.ancestry, vec![root, panel, button]);

    // 2. Click hidden widget area: (150, 190) -> should skip hidden widget and hit root (or none if panel not hit-test enabled)
    let hit_hidden =
        hit_test_select(&arena, root, Vec2::new(150.0, 190.0)).expect("should hit root");
    assert_eq!(hit_hidden.widget_id, root);
    assert_eq!(hit_hidden.ancestry, vec![root]);

    // 3. Click inert widget area: (150, 250) -> should skip inert widget and hit root
    let hit_inert =
        hit_test_select(&arena, root, Vec2::new(150.0, 250.0)).expect("should hit root");
    assert_eq!(hit_inert.widget_id, root);

    // 4. Click outside root: (900, 900) -> should be None
    assert!(hit_test_select(&arena, root, Vec2::new(900.0, 900.0)).is_none());

    // 5. Test InspectorState integration with handle_pointer_click
    let mut state = InspectorState::new();
    state.arm_select_mode();
    let selected = state.handle_pointer_click(&arena, root, Vec2::new(150.0, 130.0));
    assert_eq!(selected, Some(button));
    assert_eq!(state.selected(), Some(button));
    assert_eq!(state.ancestry(), &[root, panel, button]);
    assert!(!state.is_select_mode_armed()); // Select mode was disarmed after selection
}

#[test]
fn test_lazy_tree_model_on_demand_expansion() {
    let mut arena = WidgetArena::new();
    let root = arena.insert_with_widget(
        HotNode {
            bounds: Rect::new(0.0, 0.0, 800.0, 600.0),
            ..HotNode::default()
        },
        Box::new(DummyWidget),
    );
    let child1 = arena.insert_with_widget(
        HotNode {
            bounds: Rect::new(10.0, 10.0, 200.0, 100.0),
            ..HotNode::default()
        },
        Box::new(DummyWidget),
    );
    let child2 = arena.insert_with_widget(
        HotNode {
            bounds: Rect::new(10.0, 120.0, 200.0, 100.0),
            ..HotNode::default()
        },
        Box::new(DummyWidget),
    );
    arena.append_child(root, child1).unwrap();
    arena.append_child(root, child2).unwrap();

    let grandchild = arena.insert_with_widget(
        HotNode {
            bounds: Rect::new(20.0, 20.0, 50.0, 50.0),
            ..HotNode::default()
        },
        Box::new(DummyWidget),
    );
    arena.append_child(child1, grandchild).unwrap();

    let mut model = InspectorTreeModel::new(root);

    // When NOT expanded: children are None (lazy)
    let node = model.resolve_node(&arena, root, 0).expect("root node");
    assert_eq!(node.id, root);
    assert_eq!(node.child_count, 2);
    assert_eq!(node.children, None);

    // Flattening visible before expand returns only root
    let visible = model.flatten_visible(&arena);
    assert_eq!(visible.len(), 1);
    assert_eq!(visible[0].id, root);

    // Expand root
    model.expand(root);
    let node_expanded = model.resolve_node(&arena, root, 0).expect("root node");
    assert!(node_expanded.is_expanded);
    let children = node_expanded.children.expect("children resolved");
    assert_eq!(children.len(), 2);
    assert_eq!(children[0].id, child1);
    assert_eq!(children[1].id, child2);
    // child1 itself is not expanded, so its children are None
    assert_eq!(children[0].children, None);

    // Flattening now returns root, child1, child2
    let visible_after = model.flatten_visible(&arena);
    assert_eq!(visible_after.len(), 3);
    assert_eq!(visible_after[0].id, root);
    assert_eq!(visible_after[1].id, child1);
    assert_eq!(visible_after[2].id, child2);

    // Expand to grandchild
    model.expand_to_widget(&arena, grandchild);
    assert!(model.is_expanded(root));
    assert!(model.is_expanded(child1));

    let visible_deep = model.flatten_visible(&arena);
    assert_eq!(visible_deep.len(), 4);
    assert_eq!(visible_deep[0].id, root);
    assert_eq!(visible_deep[1].id, child1);
    assert_eq!(visible_deep[2].id, grandchild);
    assert_eq!(visible_deep[3].id, child2);
}

#[test]
fn test_lazy_tree_model_virtualization_threshold() {
    let mut arena = WidgetArena::new();
    let container = arena.insert_with_widget(
        HotNode {
            bounds: Rect::new(0.0, 0.0, 500.0, 1000.0),
            ..HotNode::default()
        },
        Box::new(DummyWidget),
    );

    // Add 100 children (e.g. simulated large table or list)
    for _ in 0..100 {
        let row = arena.insert_with_widget(
            HotNode {
                bounds: Rect::new(0.0, 0.0, 500.0, 20.0),
                ..HotNode::default()
            },
            Box::new(DummyWidget),
        );
        arena.append_child(container, row).unwrap();
    }

    let mut model = InspectorTreeModel::new(container).with_virtual_threshold(15);
    model.expand(container);

    let resolved = model
        .resolve_node(&arena, container, 0)
        .expect("resolved container");
    let children = resolved.children.expect("children materialized");

    // Exactly 15 item nodes + 1 virtual placeholder (+85 rows)
    assert_eq!(children.len(), 16);
    assert!(children[15].is_virtual_placeholder);
    assert_eq!(
        children[15].virtual_placeholder_label.as_deref(),
        Some("+85 rows")
    );
    assert_eq!(children[15].display_label(), "+85 rows");
}

#[test]
fn test_tree_node_badges_and_indicators() {
    let mut badges = NodeBadges::default();
    assert!(badges.is_empty());
    assert_eq!(badges.format_badges(), "");

    badges.has_active_lints = true;
    assert_eq!(badges.format_badges(), "⚠");

    badges.signal_fired = true;
    assert_eq!(badges.format_badges(), "⚠ ↻");

    badges.has_suppressed_lints = true;
    assert_eq!(badges.format_badges(), "⚠ ↻ ⛔");

    let mut arena = WidgetArena::new();
    let widget = arena.insert(
        HotNode::default(),
        ColdNode::new(Box::new(DummyWidget)).with_name("Gauge@alarm"),
    );

    let mut model = InspectorTreeModel::new(widget);
    model.set_lint_stats(widget, 3, 1);
    model.set_signal_stats(widget, 2, true);

    let node = model.resolve_node(&arena, widget, 0).unwrap();
    assert!(node.badges.has_active_lints);
    assert!(node.badges.signal_fired);
    assert!(node.badges.has_suppressed_lints);
    assert_eq!(node.active_signal_count, 2);

    // Clear frame signal
    model.clear_signal_fired();
    let node2 = model.resolve_node(&arena, widget, 0).unwrap();
    assert!(!node2.badges.signal_fired);
}

#[test]
fn test_layout_inspector_constraint_chain() {
    let mut arena = WidgetArena::new();

    // Hierarchy:
    // Root [0, 0, 1000, 800]
    //   Container [50, 50, 600, 400]
    //     Card [10, 10, 250, 150]
    let root = arena.insert(
        HotNode {
            bounds: Rect::new(0.0, 0.0, 1000.0, 800.0),
            ..HotNode::default()
        },
        ColdNode::new(Box::new(DummyWidget)).with_name("WindowRoot"),
    );
    let container = arena.insert(
        HotNode {
            bounds: Rect::new(50.0, 50.0, 600.0, 400.0),
            ..HotNode::default()
        },
        ColdNode::new(Box::new(DummyWidget)).with_name("Container"),
    );
    arena.append_child(root, container).unwrap();

    let card = arena.insert(
        HotNode {
            bounds: Rect::new(60.0, 60.0, 250.0, 150.0),
            ..HotNode::default()
        },
        ColdNode::new(Box::new(DummyWidget)).with_name("Card"),
    );
    arena.append_child(container, card).unwrap();

    let inspection = LayoutInspector::inspect(&arena, card).expect("inspected card");
    assert_eq!(inspection.widget_id, card);
    assert_eq!(
        inspection.allocated_bounds,
        Rect::new(60.0, 60.0, 250.0, 150.0)
    );
    assert_eq!(inspection.constraint_chain.len(), 3);

    // Check step 0 (root)
    assert_eq!(inspection.constraint_chain[0].widget_id, root);
    assert_eq!(
        inspection.constraint_chain[0].debug_name.as_deref(),
        Some("WindowRoot")
    );
    assert_eq!(
        inspection.constraint_chain[0].resolved_size,
        Vec2::new(1000.0, 800.0)
    );

    // Check step 1 (container)
    assert_eq!(inspection.constraint_chain[1].widget_id, container);
    assert_eq!(
        inspection.constraint_chain[1].offered_constraints,
        LayoutConstraints {
            min_size: Vec2::ZERO,
            max_size: Vec2::new(1000.0, 800.0)
        }
    );

    // Check step 2 (card)
    assert_eq!(inspection.constraint_chain[2].widget_id, card);
    assert_eq!(
        inspection.constraint_chain[2].offered_constraints,
        LayoutConstraints {
            min_size: Vec2::ZERO,
            max_size: Vec2::new(600.0, 400.0)
        }
    );
    assert!(inspection.constraint_chain[2].violations.is_empty());
    assert!(!inspection.style_summary.is_underflowed);
    assert!(!inspection.style_summary.is_overflowed);
}

#[test]
fn test_layout_inspector_underflow_and_overflow_violations() {
    let mut arena = WidgetArena::new();

    // Parent [0, 0, 200, 200]
    let parent = arena.insert(
        HotNode {
            bounds: Rect::new(0.0, 0.0, 200.0, 200.0),
            ..HotNode::default()
        },
        ColdNode::new(Box::new(DummyWidget)).with_name("Parent"),
    );

    // Child with underflow: declared minimum 150x50, but allocated 100x50 (deficit 50 on X)
    // and also overflowing parent on Y: bounds origin y=160, height=60 -> max_y = 220 > parent max_y = 200 (excess 20 on Y)
    let child = arena.insert(
        HotNode {
            bounds: Rect::new(10.0, 160.0, 100.0, 60.0),
            ..HotNode::default()
        },
        ColdNode::new(Box::new(DummyWidget))
            .with_name("BadWidget")
            .with_render_minimum(
                RenderMinimum::new(Vec2::new(150.0, 50.0)).with_policy(UnderflowPolicy::Clip),
            ),
    );
    arena.append_child(parent, child).unwrap();

    let inspection = LayoutInspector::inspect(&arena, child).expect("inspected child");

    assert!(inspection.style_summary.is_underflowed);
    assert!(inspection.style_summary.is_overflowed);

    let overflow = inspection.overflow.expect("overflow info present");
    assert!(!overflow.has_horizontal_overflow);
    assert!(overflow.has_vertical_overflow);
    assert!((overflow.overflow_y - 20.0).abs() < 0.01);

    let step = &inspection.constraint_chain[1];
    assert!(step.violations.iter().any(|v| matches!(
        v,
        ConstraintViolation::Underflow {
            axis: Axis::Horizontal,
            deficit,
            minimum,
            policy: UnderflowPolicy::Clip,
        } if (*deficit - 50.0).abs() < 0.01 && (*minimum - 150.0).abs() < 0.01
    )));

    assert!(step.violations.iter().any(|v| matches!(
        v,
        ConstraintViolation::Overflow {
            axis: Axis::Vertical,
            excess,
            parent_size,
            child_size,
        } if (*excess - 20.0).abs() < 0.01 && (*parent_size - 200.0).abs() < 0.01 && (*child_size - 60.0).abs() < 0.01
    )));
}

#[test]
fn test_properties_view_extraction_and_markers() {
    let mut arena = WidgetArena::new();
    let id = arena.insert(
        HotNode {
            flags: NodeFlags::VISIBLE | NodeFlags::FOCUSABLE,
            ..HotNode::default()
        },
        ColdNode::new(Box::new(DummyWidget))
            .with_name("ActionButton#src/ui/header.rs:88@level:2@alarm@priority:1@kpi@destructive@lint:contrast,touch-target")
            .with_role(Role::Button)
            .with_a11y_name("Trigger Alarm")
            .with_tooltip("Emergency shutdown"),
    );

    let props = WidgetProperties::from_arena(&arena, id).expect("properties extracted");
    assert_eq!(props.widget_id, id);
    assert_eq!(props.display_name, "ActionButton");
    assert_eq!(
        props.source_location.as_deref(),
        Some("src/ui/header.rs:88")
    );
    assert_eq!(props.a11y_role, Role::Button);
    assert_eq!(props.a11y_name.as_deref(), Some("Trigger Alarm"));
    assert_eq!(props.tooltip.as_deref(), Some("Emergency shutdown"));
    assert_eq!(props.node_kind, NodeKind::Interactive);
    assert!(props.flags.contains(NodeFlags::VISIBLE));
    assert!(props.flags.contains(NodeFlags::FOCUSABLE));

    // Verify inline markers
    assert_eq!(props.markers.isa_level, Some(2));
    assert!(props.markers.is_alarm);
    assert_eq!(props.markers.priority, Some(1));
    assert!(props.markers.is_kpi);
    assert!(props.markers.is_destructive);
    assert_eq!(
        props.markers.lint_suppressions,
        vec!["contrast".to_string(), "touch-target".to_string()]
    );

    // Tracked signals
    let props_with_signals = props.with_signals(vec![
        TrackedSignalInfo::new("status", "Active", false),
        TrackedSignalInfo::new("fault_count", "3", true),
    ]);
    assert_eq!(props_with_signals.tracked_signals.len(), 2);
    assert_eq!(props_with_signals.tracked_signals[0].name, "status");
    assert_eq!(props_with_signals.tracked_signals[0].value_repr, "Active");
    assert!(!props_with_signals.tracked_signals[0].updated_this_frame);
    assert_eq!(props_with_signals.tracked_signals[1].name, "fault_count");
    assert!(props_with_signals.tracked_signals[1].updated_this_frame);
}

#[test]
fn test_marker_parsing_edge_cases() {
    let (name, markers) = InlineMarkers::parse("SimpleWidget");
    assert_eq!(name, "SimpleWidget");
    assert_eq!(markers, InlineMarkers::default());

    let (name, markers) = InlineMarkers::parse("Panel@level:4@custom_tag:foo@kpi");
    assert_eq!(name, "Panel");
    assert_eq!(markers.isa_level, Some(4));
    assert!(markers.is_kpi);
    assert_eq!(
        markers.custom_markers,
        vec![("custom_tag".to_string(), Some("foo".to_string()))]
    );

    // Invalid level (>4 or <1) ignored
    let (_, m_inv) = InlineMarkers::parse("Widget@level:5@level:0");
    assert_eq!(m_inv.isa_level, None);
}
