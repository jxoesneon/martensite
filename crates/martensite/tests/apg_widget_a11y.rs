//! Accessibility-tree integration tests for the ARIA APG widgets.
//!
//! These tests run the six milestone widgets through the real
//! `AccessKitAdapter` emission path — arena insertion, internal-child
//! virtual nodes, overlay popup emission, `a11y_prepare`/`a11y_fixup`
//! relation wiring, and `SemanticAction` delivery back into the
//! widgets — so regressions in the emitted tree structure (not just
//! widget-local node fields) are caught.

#![forbid(unsafe_code)]

use std::time::Duration;

use accesskit::{Action, ActionRequest, Node as AccessNode, NodeId, Role};
use glam::Vec2;
use martensite::widgets::{
    Dropdown, RadioGroup, ScrollView, Slider, Tabs, Text, Tooltip, TOOLTIP_HOVER_GRACE_MS,
};
use martensite_access::actions::{dispatch_a11y_action, A11yAction, ActionTarget};
use martensite_access::{widget_id_to_node_id, AccessKitAdapter};
use martensite_core::widget::{LayoutConstraints, LayoutContext, Widget};
use martensite_core::{
    ColdNode, EventContext, EventResponse, HotNode, NodeFlags, OverlayAnchor, OverlayLayer,
    PointerButton, Rect, SemanticAction, WidgetArena, WidgetEvent, WidgetId,
};

/// Inserts a laid-out widget into a fresh arena and returns
/// `(arena, root)`.
///
/// Widget-declared flags (e.g. `NodeFlags::FOCUSABLE`, set inside
/// `Widget::layout`) are propagated into the arena node the way the
/// production layout pass writes into arena hot nodes.
fn arena_with(mut widget: impl Widget + 'static, bounds: Rect) -> (WidgetArena, WidgetId) {
    let mut scratch = HotNode::default();
    let mut cx = LayoutContext {
        hot: &mut scratch,
        scale: 1.0,
    };
    widget.layout(&mut cx, bounds);
    let declared = scratch.flags;

    let mut arena = WidgetArena::new();
    let mut hot = HotNode::new(taffy::NodeId::new(0));
    hot.flags = NodeFlags::VISIBLE | NodeFlags::HIT_TEST_ENABLED | declared;
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
        scale: 1.0,
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
        let mut cx = LayoutContext {
            hot: &mut hot,
            scale: 1.0,
        };
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
        let mut cx = LayoutContext {
            hot: &mut hot,
            scale: 1.0,
        };
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

// ---------------------------------------------------------------------------
// Focus requests
// ---------------------------------------------------------------------------

#[test]
fn press_on_focusable_widget_records_focus_request() {
    let slider = Slider::new(0.0, 100.0).with_value(40.0);
    let (mut arena, root) = arena_with(slider, Rect::new(0.0, 0.0, 200.0, 24.0));

    let press = WidgetEvent::PointerPressed {
        position: Vec2::new(50.0, 12.0),
        button: PointerButton::Primary,
    };
    assert_eq!(
        arena.dispatch_event(root, &press),
        EventResponse::CapturePointer
    );
    // Implicit press-to-focus: the handled press on a `FOCUSABLE` node
    // records a pending focus request the app drains into its
    // `FocusManager`.
    assert_eq!(arena.take_focus_request(), Some(root));
    assert_eq!(arena.take_focus_request(), None);
}

#[test]
fn semantic_focus_action_records_focus_request() {
    let group = RadioGroup::new(["A", "B"]);
    let (mut arena, root) = arena_with(group, Rect::new(0.0, 0.0, 200.0, 24.0));

    arena.dispatch_event(root, &WidgetEvent::SemanticAction(SemanticAction::Focus));
    // `CaptureFocus` from the widget becomes a drainable focus request.
    assert_eq!(arena.take_focus_request(), Some(root));
}

// ---------------------------------------------------------------------------
// Overlay routing contracts (arena-owned OverlayLayer)
// ---------------------------------------------------------------------------

/// Fixed-size stub popup content for overlay tests.
struct Fixed(Vec2);

impl Widget for Fixed {
    fn measure(&mut self, _cx: &mut LayoutContext, _c: LayoutConstraints) -> Vec2 {
        self.0
    }
    fn layout(&mut self, _cx: &mut LayoutContext, _b: Rect) {}
}

#[test]
fn dropdown_typeahead_stays_with_focused_owner_while_popup_open() {
    let mut dd = Dropdown::new(["Red", "Green", "Blue"]);
    dd.open();
    let (mut arena, root) = arena_with(dd, Rect::new(10.0, 10.0, 160.0, 32.0));
    arena
        .overlay_mut()
        .set_viewport(Rect::new(0.0, 0.0, 800.0, 600.0));
    arena.sync_overlays();
    assert_eq!(arena.overlay().len(), 1, "popup open in arena overlay");

    let key = WidgetEvent::KeyPressed {
        key: "g".to_string(),
        repeat: false,
    };
    // Ordinary keys pass through the overlay — only Escape is
    // consumed — so the focused combobox keeps receiving input.
    assert_eq!(
        arena.overlay_mut().dispatch_event(&key),
        EventResponse::Ignored
    );
    arena.dispatch_event(root, &key);

    let mut adapter = AccessKitAdapter::new(root);
    let update = adapter.build_update(&mut arena);
    let (_, combo) = find_node(&update, |n| n.role() == Role::ComboBox).unwrap();
    let options = find_all(&update, |n| n.role() == Role::ListBoxOption);
    assert_eq!(options.len(), 3);
    assert_eq!(
        combo.active_descendant(),
        Some(options[1].0),
        "typeahead moved activedescendant to \"Green\""
    );
}

#[test]
fn overlay_escape_and_outside_press_record_dismissals() {
    let (mut arena, _root) = arena_with(Text::new("base"), Rect::new(0.0, 0.0, 100.0, 100.0));
    arena
        .overlay_mut()
        .set_viewport(Rect::new(0.0, 0.0, 800.0, 600.0));
    let a = arena.overlay_mut().open(
        Box::new(Fixed(Vec2::new(40.0, 20.0))),
        OverlayAnchor::Bounds(Rect::new(10.0, 10.0, 50.0, 20.0)),
    );
    let b = arena.overlay_mut().open(
        Box::new(Fixed(Vec2::new(40.0, 20.0))),
        OverlayAnchor::Bounds(Rect::new(100.0, 100.0, 50.0, 20.0)),
    );

    // Escape dismisses only the topmost popup and records it.
    let escape = WidgetEvent::KeyPressed {
        key: "Escape".to_string(),
        repeat: false,
    };
    assert_eq!(
        arena.overlay_mut().dispatch_event(&escape),
        EventResponse::Handled
    );
    assert!(arena.overlay().is_open(a));
    assert!(!arena.overlay().is_open(b));
    assert_eq!(arena.overlay_mut().take_dismissed(), Some(b));

    // An outside press dismisses the remaining popup, records it for
    // owners to reconcile, and falls through to window content.
    let press = WidgetEvent::PointerPressed {
        position: Vec2::new(700.0, 500.0),
        button: PointerButton::Primary,
    };
    assert_eq!(
        arena.overlay_mut().dispatch_event(&press),
        EventResponse::Ignored
    );
    assert_eq!(arena.overlay_mut().take_dismissed(), Some(a));
    assert_eq!(arena.overlay_mut().take_dismissed(), None);
    assert!(arena.overlay().is_empty());
}

#[test]
fn overlay_lazily_lays_out_fresh_popup_for_hit_testing() {
    let mut overlay = OverlayLayer::new();
    overlay.set_viewport(Rect::new(0.0, 0.0, 800.0, 600.0));
    // Bounds anchor (10,10,70,30) → popup placed below at y = 44.
    let id = overlay.open(
        Box::new(Fixed(Vec2::new(60.0, 40.0))),
        OverlayAnchor::Bounds(Rect::new(10.0, 10.0, 70.0, 30.0)),
    );
    // No `layout_pass` yet: dispatch lays the entry out lazily so a
    // press inside the popup's resolved bounds doesn't count as an
    // outside click.
    let inside = WidgetEvent::PointerPressed {
        position: Vec2::new(20.0, 50.0),
        button: PointerButton::Primary,
    };
    assert_ne!(overlay.dispatch_event(&inside), EventResponse::Ignored);
    assert!(
        overlay.is_open(id),
        "press hit the popup, not an outside click"
    );
}

// ---------------------------------------------------------------------------
// Tooltip WCAG 1.4.13 hoverable
// ---------------------------------------------------------------------------

#[test]
fn tooltip_hover_grace_bridges_pointer_to_bubble() {
    let mut tip = Tooltip::new(Text::new("Save"), "tip").delay_ms(500);
    let bounds = Rect::new(10.0, 10.0, 100.0, 40.0);
    {
        let mut hot = HotNode::default();
        let mut cx = LayoutContext {
            hot: &mut hot,
            scale: 1.0,
        };
        tip.layout(&mut cx, bounds);
    }
    let send = |tip: &mut Tooltip, ev: WidgetEvent| {
        let mut cx = EventContext {
            event: &ev,
            bounds,
            scale: 1.0,
        };
        tip.event(&mut cx)
    };

    send(&mut tip, WidgetEvent::PointerEnter);
    tip.tick(Duration::from_millis(500));
    assert!(tip.is_shown());

    let mut overlay = OverlayLayer::new();
    overlay.set_viewport(Rect::new(0.0, 0.0, 800.0, 600.0));
    tip.sync_overlay(&mut overlay);
    overlay.layout_pass();
    let popup_id = tip.popup_id().expect("popup opened");
    let bubble = overlay.entry_bounds(popup_id).expect("bubble laid out");

    // Pointer leaves the trigger: the popup survives the grace window…
    send(&mut tip, WidgetEvent::PointerLeave);
    tip.tick(Duration::from_millis(TOOLTIP_HOVER_GRACE_MS - 100));
    assert!(tip.is_shown(), "grace window keeps the popup open");

    // …long enough for the pointer to reach the bubble — activity on
    // the popup cancels the countdown at the next sync.
    let inside = WidgetEvent::PointerMoved {
        position: Vec2::new(bubble.min_x() + 5.0, bubble.min_y() + 5.0),
    };
    overlay.dispatch_event(&inside);
    tip.sync_overlay(&mut overlay);
    tip.tick(Duration::from_millis(TOOLTIP_HOVER_GRACE_MS));
    assert!(tip.is_shown(), "bubble hover keeps the popup open");
    assert!(overlay.is_open(popup_id));

    // Leaving again without reaching the popup lets the grace expire;
    // the next sync closes the entry.
    send(&mut tip, WidgetEvent::PointerLeave);
    tip.tick(Duration::from_millis(TOOLTIP_HOVER_GRACE_MS));
    assert!(!tip.is_shown());
    tip.sync_overlay(&mut overlay);
    assert!(!overlay.is_open(popup_id));
}

// ---------------------------------------------------------------------------
// ScrollView content hover forwarding
// ---------------------------------------------------------------------------

#[test]
fn scrollview_forwards_hover_to_content() {
    /// Content widget that records pointer events through a shared log.
    struct HoverLog {
        seen: std::sync::Arc<std::sync::Mutex<Vec<&'static str>>>,
    }

    impl Widget for HoverLog {
        fn measure(&mut self, _cx: &mut LayoutContext, _c: LayoutConstraints) -> Vec2 {
            Vec2::new(80.0, 400.0)
        }
        fn layout(&mut self, _cx: &mut LayoutContext, _b: Rect) {}
        fn event(&mut self, cx: &mut EventContext) -> EventResponse {
            let tag = match cx.event {
                WidgetEvent::PointerMoved { .. } => Some("moved"),
                WidgetEvent::PointerEnter => Some("enter"),
                WidgetEvent::PointerLeave => Some("leave"),
                _ => None,
            };
            if let Some(tag) = tag {
                self.seen.lock().unwrap().push(tag);
            }
            EventResponse::Ignored
        }
    }

    let seen = std::sync::Arc::new(std::sync::Mutex::new(Vec::new()));
    let view = ScrollView::new(HoverLog {
        seen: std::sync::Arc::clone(&seen),
    });
    let (mut arena, root) = arena_with(view, Rect::new(0.0, 0.0, 100.0, 100.0));

    arena.dispatch_event(root, &WidgetEvent::PointerEnter);
    arena.dispatch_event(
        root,
        &WidgetEvent::PointerMoved {
            position: Vec2::new(50.0, 50.0),
        },
    );
    arena.dispatch_event(root, &WidgetEvent::PointerLeave);

    assert_eq!(
        seen.lock().unwrap().as_slice(),
        &["enter", "moved", "leave"],
        "hover events reach the scrollable content"
    );
}

// ---------------------------------------------------------------------------
// Incremental updates + overlays (HIGH-1)
// ---------------------------------------------------------------------------

#[test]
fn incremental_update_emits_popup_opened_after_full_build() {
    let dd = Dropdown::new(["Red", "Green", "Blue"]).label("Colour");
    let (mut arena, root) = arena_with(dd, Rect::new(10.0, 10.0, 160.0, 32.0));
    arena
        .overlay_mut()
        .set_viewport(Rect::new(0.0, 0.0, 800.0, 600.0));
    let mut adapter = AccessKitAdapter::new(root);

    // Initial full build: combobox closed, no popup in the tree.
    let update = adapter.build_update(&mut arena);
    let (_, combo) = find_node(&update, |n| n.role() == Role::ComboBox).unwrap();
    assert_eq!(combo.is_expanded(), Some(false));
    assert!(find_node(&update, |n| n.role() == Role::ListBox).is_none());
    assert!(
        adapter.build_incremental_update(&mut arena).is_none(),
        "nothing dirty — no incremental update"
    );

    // AT Expand opens the popup; the arena's overlay sync stamps the
    // owner and dirty-marks it for re-emission.
    arena.dispatch_event(root, &WidgetEvent::SemanticAction(SemanticAction::Expand));
    arena.sync_overlays();
    assert_eq!(arena.overlay().len(), 1, "popup open in arena overlay");

    // The incremental update carries the popup subtree and refreshed
    // relations — it must not be invisible to the adapter.
    let update = adapter
        .build_incremental_update(&mut arena)
        .expect("popup open forces an incremental update");
    let (listbox_id, listbox) =
        find_node(&update, |n| n.role() == Role::ListBox).expect("listbox emitted incrementally");
    assert!(find_all(&update, |n| n.role() == Role::ListBoxOption).len() == 3);
    let _ = listbox;

    // The popup root is attached to the tree root's children, and the
    // re-emitted combobox resolves aria-controls → the listbox.
    let tree_root = widget_id_to_node_id(root);
    let (_, tree_root_node) = update
        .nodes
        .iter()
        .find(|(id, _)| *id == tree_root)
        .expect("tree root re-emitted");
    assert!(
        tree_root_node.children().contains(listbox_id),
        "popup root appended to tree root children"
    );
    let (_, combo) = find_node(&update, |n| n.role() == Role::ComboBox).unwrap();
    assert_eq!(combo.is_expanded(), Some(true));
    assert_eq!(combo.controls(), &[*listbox_id]);

    // Closing the popup produces another update whose root children no
    // longer list the popup — and the closed entry's virtual ids are
    // pruned from resolution.
    arena.dispatch_event(root, &WidgetEvent::SemanticAction(SemanticAction::Collapse));
    arena.sync_overlays();
    assert!(arena.overlay().is_empty());
    let update = adapter
        .build_incremental_update(&mut arena)
        .expect("popup close forces an incremental update");
    let (_, tree_root_node) = update
        .nodes
        .iter()
        .find(|(id, _)| *id == tree_root)
        .expect("tree root re-emitted on close");
    assert!(!tree_root_node.children().contains(listbox_id));
    assert!(
        adapter.resolve_overlay(*listbox_id).is_none(),
        "closed popup's virtual ids are pruned"
    );
}

// ---------------------------------------------------------------------------
// AT action routing to virtual targets (HIGH-3)
// ---------------------------------------------------------------------------

#[test]
fn at_action_on_virtual_option_reaches_handler() {
    let group = RadioGroup::new(["Alpha", "Beta", "Gamma"]);
    let (mut arena, root) = arena_with(group, Rect::new(0.0, 0.0, 300.0, 30.0));
    let mut adapter = AccessKitAdapter::new(root);
    let update = adapter.build_update(&mut arena);
    let radios = find_all(&update, |n| n.role() == Role::RadioButton);
    assert_eq!(radios.len(), 3);

    // Decode an AT Click aimed at the third option's *virtual* node —
    // the path that previously failed `node_id_to_widget_id` and was
    // dropped.
    let request = ActionRequest {
        action: Action::Click,
        target_node: radios[2].0,
        target_tree: accesskit::TreeId::ROOT,
        data: None,
    };
    let action = adapter
        .decode_action(&arena, &request)
        .expect("virtual-node action decodes");
    assert!(
        matches!(&action, A11yAction::Click(ActionTarget::Internal(o, p))
            if *o == root && !p.is_empty()),
        "decoded as an internal target of the radio group: {action:?}"
    );

    // Route it through the semantic dispatcher: the option parks the
    // activation on its owner, which `dispatch_a11y_action` drains via
    // `a11y_prepare`.
    dispatch_a11y_action(&mut arena, &action);

    let update = adapter.build_update(&mut arena);
    let checked: Vec<bool> = find_all(&update, |n| n.role() == Role::RadioButton)
        .iter()
        .map(|(_, n)| n.toggled() == Some(accesskit::Toggled::True))
        .collect();
    assert_eq!(checked, vec![false, false, true]);
}

#[test]
fn at_action_on_popup_option_commits_selection() {
    let dd = Dropdown::new(["Red", "Green", "Blue"]);
    let (mut arena, root) = arena_with(dd, Rect::new(10.0, 10.0, 160.0, 32.0));
    arena
        .overlay_mut()
        .set_viewport(Rect::new(0.0, 0.0, 800.0, 600.0));
    let mut adapter = AccessKitAdapter::new(root);
    adapter.build_update(&mut arena);

    // Open the popup through the arena path.
    arena.dispatch_event(root, &WidgetEvent::SemanticAction(SemanticAction::Expand));
    arena.sync_overlays();
    let update = adapter.build_update(&mut arena);
    let options = find_all(&update, |n| n.role() == Role::ListBoxOption);
    assert_eq!(options.len(), 3);

    // AT Click on the "Blue" option's overlay virtual node.
    let request = ActionRequest {
        action: Action::Click,
        target_node: options[2].0,
        target_tree: accesskit::TreeId::ROOT,
        data: None,
    };
    let action = adapter
        .decode_action(&arena, &request)
        .expect("overlay-node action decodes");
    assert!(
        matches!(&action, A11yAction::Click(ActionTarget::Overlay(_, _))),
        "decoded as an overlay target: {action:?}"
    );

    dispatch_a11y_action(&mut arena, &action);

    // The commit reached the owning dropdown via the shared slot and
    // was drained by the dispatch's `a11y_prepare` on the owner.
    let update = adapter.build_update(&mut arena);
    let (_, combo) = find_node(&update, |n| n.role() == Role::ComboBox).unwrap();
    assert_eq!(combo.value(), Some("Blue"));
}

// ---------------------------------------------------------------------------
// Arena-hosted tooltip ticking (HIGH-2 / MEDIUM-6)
// ---------------------------------------------------------------------------

#[test]
fn arena_tick_shows_tooltip_after_hover_delay() {
    let tip = Tooltip::new(Text::new("Save"), "tip").delay_ms(500);
    let (mut arena, root) = arena_with(tip, Rect::new(10.0, 10.0, 100.0, 40.0));
    arena
        .overlay_mut()
        .set_viewport(Rect::new(0.0, 0.0, 800.0, 600.0));
    let mut adapter = AccessKitAdapter::new(root);
    adapter.build_update(&mut arena);

    // Pointer enters the trigger — the tooltip starts its hover delay
    // but cannot advance it until a frame tick reaches the widget.
    arena.dispatch_event(root, &WidgetEvent::PointerEnter);
    arena.tick(Duration::from_millis(400));
    assert!(
        arena.overlay().is_empty(),
        "delay not yet reached — no popup"
    );

    // One more frame crosses the delay: `arena.tick` advances the
    // widget and syncs overlays, so the popup opens the same frame.
    arena.tick(Duration::from_millis(100));
    assert_eq!(
        arena.overlay().len(),
        1,
        "arena-hosted tooltip opened its popup via the frame seam"
    );
    let entry = arena.overlay().entries().next().unwrap();
    assert_eq!(entry.owner(), Some(root), "popup stamped with its owner");

    // The emitted tree wires aria-describedby to the bubble.
    let update = adapter.build_update(&mut arena);
    let (bubble_id, _) = find_node(&update, |n| n.role() == Role::Tooltip).expect("bubble emitted");
    assert!(
        find_node(&update, |n| n.described_by().contains(bubble_id)).is_some(),
        "trigger has aria-describedby → bubble"
    );
}

// ---------------------------------------------------------------------------
// Arena removal cleanup (MEDIUM-4)
// ---------------------------------------------------------------------------

#[test]
fn remove_closes_owned_popup_and_clears_focus_request() {
    let dd = Dropdown::new(["A", "B"]);
    let (mut arena, root) = arena_with(dd, Rect::new(10.0, 10.0, 160.0, 32.0));
    arena
        .overlay_mut()
        .set_viewport(Rect::new(0.0, 0.0, 800.0, 600.0));
    arena.dispatch_event(root, &WidgetEvent::SemanticAction(SemanticAction::Expand));
    arena.sync_overlays();
    assert_eq!(arena.overlay().len(), 1);
    assert_eq!(
        arena.overlay().entries().next().unwrap().owner(),
        Some(root)
    );

    // A focus request naming the soon-to-be-removed widget must not
    // outlive it either.
    arena.request_focus(root);
    assert_eq!(arena.take_focus_request(), Some(root));
    arena.request_focus(root);

    arena.remove(root);

    assert!(
        arena.overlay().is_empty(),
        "removing the owner closed its popup"
    );
    assert_eq!(
        arena.take_focus_request(),
        None,
        "stale focus request cleared on remove"
    );
}
