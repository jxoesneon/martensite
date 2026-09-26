//! Integration tests for W6 Event Observability Dispatch Instrumentation in `martensite-window`.

#![cfg(feature = "devtools")]

use glam::Vec2;
use martensite_core::{
    DummyWidget, EventContext, EventResponse, HotNode, LayoutConstraints, LayoutContext, NodeFlags,
    Rect, Widget, WidgetArena, WidgetEvent,
};
use martensite_window::event::{
    set_debug_events_enabled, Disposition, EventKind, EventRouter, HitRejection, ModifierKeys,
    MouseButton, PointerEvent, PointerId, PointerKind, PointerState,
};
use martensite_window::WindowId;

#[derive(Debug, Default)]
struct ConsumingWidget;

impl Widget for ConsumingWidget {
    fn measure(&mut self, _cx: &mut LayoutContext<'_>, _constraints: LayoutConstraints) -> Vec2 {
        Vec2::ZERO
    }

    fn layout(&mut self, _cx: &mut LayoutContext<'_>, _bounds: Rect) {}

    fn event(&mut self, _cx: &mut EventContext<'_>) -> EventResponse {
        EventResponse::Handled
    }
}

#[derive(Debug, Default)]
struct FocusRequestWidget;

impl Widget for FocusRequestWidget {
    fn measure(&mut self, _cx: &mut LayoutContext<'_>, _constraints: LayoutConstraints) -> Vec2 {
        Vec2::ZERO
    }

    fn layout(&mut self, _cx: &mut LayoutContext<'_>, _bounds: Rect) {}

    fn event(&mut self, cx: &mut EventContext<'_>) -> EventResponse {
        match cx.event {
            WidgetEvent::PointerPressed { .. } => EventResponse::CaptureFocus,
            _ => EventResponse::Ignored,
        }
    }
}

fn create_hot(x: f32, y: f32, w: f32, h: f32, flags: NodeFlags) -> HotNode {
    HotNode {
        bounds: Rect::new(x, y, w, h),
        flags,
        ..HotNode::default()
    }
}

fn make_click_event(pos: Vec2) -> PointerEvent {
    PointerEvent {
        pointer_id: PointerId::PRIMARY,
        kind: PointerKind::Mouse,
        position: pos,
        state: PointerState::Pressed,
        button: Some(MouseButton::Left),
        modifiers: ModifierKeys::empty(),
    }
}

#[test]
fn test_pointer_event_handled_recorded_in_ledger() {
    let mut arena = WidgetArena::new();
    let root = arena.insert_with_widget(
        create_hot(
            0.0,
            0.0,
            200.0,
            200.0,
            NodeFlags::VISIBLE | NodeFlags::HIT_TEST_ENABLED,
        ),
        Box::new(DummyWidget),
    );
    let child = arena.insert_with_widget(
        create_hot(
            20.0,
            20.0,
            100.0,
            100.0,
            NodeFlags::VISIBLE | NodeFlags::HIT_TEST_ENABLED,
        ),
        Box::new(ConsumingWidget),
    );
    arena.append_child(root, child).unwrap();

    let mut router = EventRouter::new();
    router.set_ledger_enabled(true);
    router.set_frame(42);

    let event = make_click_event(Vec2::new(50.0, 50.0));
    let win = WindowId::from_raw(1);
    let resp = router.dispatch_pointer_event(&mut arena, root, win, &event);
    assert_eq!(resp, Some(EventResponse::Handled));

    let ledger = router.event_ledger();
    assert_eq!(ledger.len(), 1);
    let record = ledger.newest().expect("record present");
    assert_eq!(record.seq, 1);
    assert_eq!(record.frame, 42);
    assert_eq!(record.kind, EventKind::Pointer);
    assert_eq!(record.position, Some((50.0, 50.0).into()));
    assert_eq!(record.disposition, Disposition::Handled(child));
    assert_eq!(record.hit_rejection, None);
    assert_eq!(record.hit_path.len(), 2);
    assert_eq!(record.hit_path.first(), Some(root));
    assert_eq!(record.hit_path.last(), Some(child));
    assert!(record.mentions_widget(child));
    assert!(record.mentions_widget(root));

    let diag = record.format_diagnostic();
    assert!(diag.starts_with("ptr@ 50,50 → hit["));
    assert!(diag.ends_with("handled"));
}

#[test]
fn test_pointer_event_bubbled_recorded_in_ledger() {
    let mut arena = WidgetArena::new();
    let root = arena.insert_with_widget(
        create_hot(
            0.0,
            0.0,
            200.0,
            200.0,
            NodeFlags::VISIBLE | NodeFlags::HIT_TEST_ENABLED,
        ),
        Box::new(ConsumingWidget),
    );
    let child = arena.insert_with_widget(
        create_hot(
            20.0,
            20.0,
            100.0,
            100.0,
            NodeFlags::VISIBLE | NodeFlags::HIT_TEST_ENABLED,
        ),
        Box::new(DummyWidget),
    );
    arena.append_child(root, child).unwrap();

    let mut router = EventRouter::new();
    router.set_ledger_enabled(true);

    let event = make_click_event(Vec2::new(50.0, 50.0));
    let win = WindowId::from_raw(1);
    let resp = router.dispatch_pointer_event(&mut arena, root, win, &event);
    assert_eq!(resp, Some(EventResponse::Handled));

    let record = router.event_ledger().newest().expect("record present");
    assert_eq!(record.disposition, Disposition::BubbledTo(root));
    assert_eq!(record.hit_path.last(), Some(child));
    assert_eq!(record.hit_rejection, None);
}

#[test]
fn test_pointer_event_ignored_recorded_in_ledger() {
    let mut arena = WidgetArena::new();
    let root = arena.insert_with_widget(
        create_hot(
            0.0,
            0.0,
            200.0,
            200.0,
            NodeFlags::VISIBLE | NodeFlags::HIT_TEST_ENABLED,
        ),
        Box::new(DummyWidget),
    );
    let child = arena.insert_with_widget(
        create_hot(
            20.0,
            20.0,
            100.0,
            100.0,
            NodeFlags::VISIBLE | NodeFlags::HIT_TEST_ENABLED,
        ),
        Box::new(DummyWidget),
    );
    arena.append_child(root, child).unwrap();

    let mut router = EventRouter::new();
    router.set_ledger_enabled(true);

    let event = make_click_event(Vec2::new(50.0, 50.0));
    let win = WindowId::from_raw(1);
    let resp = router.dispatch_pointer_event(&mut arena, root, win, &event);
    assert_eq!(resp, Some(EventResponse::Ignored));

    let record = router.event_ledger().newest().expect("record present");
    assert_eq!(record.disposition, Disposition::Ignored);
    assert_eq!(record.hit_path.last(), Some(child));
    assert_eq!(record.hit_rejection, None);
}

#[test]
fn test_pointer_capture_captured_disposition() {
    let mut arena = WidgetArena::new();
    let root = arena.insert_with_widget(
        create_hot(
            0.0,
            0.0,
            200.0,
            200.0,
            NodeFlags::VISIBLE | NodeFlags::HIT_TEST_ENABLED,
        ),
        Box::new(DummyWidget),
    );
    let capturer = arena.insert_with_widget(
        create_hot(
            0.0,
            0.0,
            50.0,
            50.0,
            NodeFlags::VISIBLE | NodeFlags::HIT_TEST_ENABLED,
        ),
        Box::new(ConsumingWidget),
    );
    arena.append_child(root, capturer).unwrap();

    let mut router = EventRouter::new();
    router.set_ledger_enabled(true);
    router.capture_pointer(PointerId::PRIMARY, capturer);

    // Click far away from capturer (180, 180).
    let event = make_click_event(Vec2::new(180.0, 180.0));
    let win = WindowId::from_raw(1);
    let resp = router.dispatch_pointer_event(&mut arena, root, win, &event);
    assert_eq!(resp, Some(EventResponse::Handled));

    let record = router.event_ledger().newest().expect("record present");
    assert_eq!(record.disposition, Disposition::Captured(capturer));
    assert_eq!(record.hit_path.last(), Some(capturer));
}

#[test]
fn test_hit_rejection_disabled_control() {
    let mut arena = WidgetArena::new();
    let root = arena.insert_with_widget(
        create_hot(0.0, 0.0, 200.0, 200.0, NodeFlags::VISIBLE),
        Box::new(DummyWidget),
    );
    // Disabled control: VISIBLE but NOT HIT_TEST_ENABLED.
    let disabled = arena.insert_with_widget(
        create_hot(20.0, 20.0, 100.0, 100.0, NodeFlags::VISIBLE),
        Box::new(ConsumingWidget),
    );
    arena.append_child(root, disabled).unwrap();

    let mut router = EventRouter::new();
    router.set_ledger_enabled(true);

    let event = make_click_event(Vec2::new(50.0, 50.0));
    let win = WindowId::from_raw(1);
    let resp = router.dispatch_pointer_event(&mut arena, root, win, &event);
    assert_eq!(resp, None);

    let record = router.event_ledger().newest().expect("record present");
    assert_eq!(record.disposition, Disposition::Ignored);
    assert_eq!(record.hit_rejection, Some(HitRejection::HitTestDisabled));
    assert_eq!(record.hit_path.last(), Some(disabled));

    let diag = record.format_diagnostic();
    assert!(diag.contains("rejected:HitTestDisabled"));
}

#[test]
fn test_hit_rejection_inert_control() {
    let mut arena = WidgetArena::new();
    let root = arena.insert_with_widget(
        create_hot(0.0, 0.0, 200.0, 200.0, NodeFlags::VISIBLE),
        Box::new(DummyWidget),
    );
    let inert = arena.insert_with_widget(
        create_hot(
            20.0,
            20.0,
            100.0,
            100.0,
            NodeFlags::VISIBLE | NodeFlags::HIT_TEST_ENABLED | NodeFlags::INERT,
        ),
        Box::new(ConsumingWidget),
    );
    arena.append_child(root, inert).unwrap();

    let mut router = EventRouter::new();
    router.set_ledger_enabled(true);

    let event = make_click_event(Vec2::new(50.0, 50.0));
    let win = WindowId::from_raw(1);
    let resp = router.dispatch_pointer_event(&mut arena, root, win, &event);
    assert_eq!(resp, None);

    let record = router.event_ledger().newest().expect("record present");
    assert_eq!(record.hit_rejection, Some(HitRejection::HitTestDisabled));
}

#[test]
fn test_hit_rejection_occluded_by_sibling() {
    let mut arena = WidgetArena::new();
    let root = arena.insert_with_widget(
        create_hot(0.0, 0.0, 200.0, 200.0, NodeFlags::VISIBLE),
        Box::new(DummyWidget),
    );
    // Lower sibling:
    let lower = arena.insert_with_widget(
        create_hot(20.0, 20.0, 100.0, 100.0, NodeFlags::VISIBLE),
        Box::new(ConsumingWidget),
    );
    arena.append_child(root, lower).unwrap();

    // Upper sibling (inserted after -> higher Z): disabled.
    let upper = arena.insert_with_widget(
        create_hot(20.0, 20.0, 100.0, 100.0, NodeFlags::VISIBLE),
        Box::new(DummyWidget),
    );
    arena.append_child(root, upper).unwrap();

    let mut router = EventRouter::new();
    router.set_ledger_enabled(true);

    let event = make_click_event(Vec2::new(50.0, 50.0));
    let win = WindowId::from_raw(1);
    let resp = router.dispatch_pointer_event(&mut arena, root, win, &event);
    assert_eq!(resp, None);

    let record = router.event_ledger().newest().expect("record present");
    assert_eq!(record.hit_rejection, Some(HitRejection::OccludedBy(upper)));
}

#[test]
fn test_hit_rejection_outside_bounds() {
    let mut arena = WidgetArena::new();
    let root = arena.insert_with_widget(
        create_hot(
            0.0,
            0.0,
            100.0,
            100.0,
            NodeFlags::VISIBLE | NodeFlags::HIT_TEST_ENABLED,
        ),
        Box::new(DummyWidget),
    );

    let mut router = EventRouter::new();
    router.set_ledger_enabled(true);

    let event = make_click_event(Vec2::new(500.0, 500.0));
    let win = WindowId::from_raw(1);
    let resp = router.dispatch_pointer_event(&mut arena, root, win, &event);
    assert_eq!(resp, None);

    let record = router.event_ledger().newest().expect("record present");
    assert_eq!(record.hit_rejection, Some(HitRejection::OutsideBounds));
    assert_eq!(record.disposition, Disposition::Ignored);
    assert_eq!(record.hit_path.first(), Some(root));
}

#[test]
fn test_pointer_event_focus_transition_recorded() {
    let mut arena = WidgetArena::new();
    let root = arena.insert_with_widget(
        create_hot(
            0.0,
            0.0,
            200.0,
            200.0,
            NodeFlags::VISIBLE | NodeFlags::HIT_TEST_ENABLED,
        ),
        Box::new(DummyWidget),
    );
    let button = arena.insert_with_widget(
        create_hot(
            20.0,
            20.0,
            100.0,
            100.0,
            NodeFlags::VISIBLE | NodeFlags::HIT_TEST_ENABLED,
        ),
        Box::new(FocusRequestWidget),
    );
    arena.append_child(root, button).unwrap();

    let mut router = EventRouter::new();
    router.set_ledger_enabled(true);

    let event = make_click_event(Vec2::new(50.0, 50.0));
    let win = WindowId::from_raw(1);
    router.dispatch_pointer_event(&mut arena, root, win, &event);

    let record = router.event_ledger().newest().expect("record present");
    assert_eq!(record.focus_to, Some(button));
    assert_eq!(router.take_focus_request(), Some(button));
}

#[test]
fn test_keyboard_event_handled_recorded_in_ledger() {
    let mut arena = WidgetArena::new();
    let root = arena.insert_with_widget(
        create_hot(
            0.0,
            0.0,
            100.0,
            100.0,
            NodeFlags::VISIBLE | NodeFlags::HIT_TEST_ENABLED,
        ),
        Box::new(ConsumingWidget),
    );

    let mut router = EventRouter::new();
    router.set_ledger_enabled(true);
    router.set_frame(10);

    let resp = router.dispatch_keyboard_event(&mut arena, Some(root), "Enter", true, false);
    assert_eq!(resp, Some(EventResponse::Handled));

    let record = router.event_ledger().newest().expect("record present");
    assert_eq!(record.kind, EventKind::Key);
    assert_eq!(record.frame, 10);
    assert_eq!(record.position, None);
    assert_eq!(record.disposition, Disposition::Handled(root));
    assert_eq!(record.hit_path.first(), Some(root));
    assert_eq!(record.focus_from, Some(root));
}

#[test]
fn test_keyboard_event_bubbled_recorded_in_ledger() {
    let mut arena = WidgetArena::new();
    let root = arena.insert_with_widget(
        create_hot(
            0.0,
            0.0,
            100.0,
            100.0,
            NodeFlags::VISIBLE | NodeFlags::HIT_TEST_ENABLED,
        ),
        Box::new(ConsumingWidget),
    );
    let child = arena.insert_with_widget(
        create_hot(
            10.0,
            10.0,
            50.0,
            50.0,
            NodeFlags::VISIBLE | NodeFlags::HIT_TEST_ENABLED,
        ),
        Box::new(DummyWidget),
    );
    arena.append_child(root, child).unwrap();

    let mut router = EventRouter::new();
    router.set_ledger_enabled(true);

    let resp = router.dispatch_keyboard_event(&mut arena, Some(child), "Tab", true, false);
    assert_eq!(resp, Some(EventResponse::Handled));

    let record = router.event_ledger().newest().expect("record present");
    assert_eq!(record.disposition, Disposition::BubbledTo(root));
    assert_eq!(record.hit_path.last(), Some(child));
}

#[test]
fn test_keyboard_event_no_focus_ignored() {
    let mut arena = WidgetArena::new();
    let mut router = EventRouter::new();
    router.set_ledger_enabled(true);

    let resp = router.dispatch_keyboard_event(&mut arena, None, "A", true, false);
    assert_eq!(resp, None);

    let record = router.event_ledger().newest().expect("record present");
    assert_eq!(record.kind, EventKind::Key);
    assert_eq!(record.disposition, Disposition::Ignored);
    assert!(record.hit_path.is_empty());
    assert_eq!(record.focus_from, None);
}

#[test]
fn test_scroll_event_handled_recorded_in_ledger() {
    let mut arena = WidgetArena::new();
    let root = arena.insert_with_widget(
        create_hot(
            0.0,
            0.0,
            200.0,
            200.0,
            NodeFlags::VISIBLE | NodeFlags::HIT_TEST_ENABLED,
        ),
        Box::new(DummyWidget),
    );
    let scroll_target = arena.insert_with_widget(
        create_hot(
            10.0,
            10.0,
            100.0,
            100.0,
            NodeFlags::VISIBLE | NodeFlags::HIT_TEST_ENABLED,
        ),
        Box::new(ConsumingWidget),
    );
    arena.append_child(root, scroll_target).unwrap();

    let mut router = EventRouter::new();
    router.set_ledger_enabled(true);
    let win = WindowId::from_raw(1);

    // Hover over scroll_target first.
    let move_ev = PointerEvent {
        pointer_id: PointerId::PRIMARY,
        kind: PointerKind::Mouse,
        position: Vec2::new(50.0, 50.0),
        state: PointerState::Moved,
        button: None,
        modifiers: ModifierKeys::empty(),
    };
    router.route_pointer_event(&arena, root, win, &move_ev);

    // Now dispatch scroll.
    let resp = router.dispatch_scroll_event(&mut arena, win, Vec2::new(0.0, -15.0));
    assert_eq!(resp, Some(EventResponse::Handled));

    let record = router.event_ledger().newest().expect("record present");
    assert_eq!(record.kind, EventKind::Scroll);
    assert_eq!(record.position, Some((50.0, 50.0).into()));
    assert_eq!(record.disposition, Disposition::Handled(scroll_target));
    assert_eq!(record.hit_path.last(), Some(scroll_target));
}

#[test]
fn test_scroll_event_bubbled_to_parent() {
    let mut arena = WidgetArena::new();
    let root = arena.insert_with_widget(
        create_hot(
            0.0,
            0.0,
            200.0,
            200.0,
            NodeFlags::VISIBLE | NodeFlags::HIT_TEST_ENABLED,
        ),
        Box::new(ConsumingWidget),
    );
    let inner = arena.insert_with_widget(
        create_hot(
            10.0,
            10.0,
            100.0,
            100.0,
            NodeFlags::VISIBLE | NodeFlags::HIT_TEST_ENABLED,
        ),
        Box::new(DummyWidget),
    );
    arena.append_child(root, inner).unwrap();

    let mut router = EventRouter::new();
    router.set_ledger_enabled(true);
    let win = WindowId::from_raw(1);

    let move_ev = PointerEvent {
        pointer_id: PointerId::PRIMARY,
        kind: PointerKind::Mouse,
        position: Vec2::new(50.0, 50.0),
        state: PointerState::Moved,
        button: None,
        modifiers: ModifierKeys::empty(),
    };
    router.route_pointer_event(&arena, root, win, &move_ev);

    let resp = router.dispatch_scroll_event(&mut arena, win, Vec2::new(0.0, -15.0));
    assert_eq!(resp, Some(EventResponse::Handled));

    let record = router.event_ledger().newest().expect("record present");
    assert_eq!(record.disposition, Disposition::BubbledTo(root));
}

#[test]
fn test_scroll_event_unhandled_when_no_hover() {
    let mut arena = WidgetArena::new();
    let mut router = EventRouter::new();
    router.set_ledger_enabled(true);
    let win = WindowId::from_raw(1);

    let resp = router.dispatch_scroll_event(&mut arena, win, Vec2::new(0.0, 10.0));
    assert_eq!(resp, None);

    let record = router.event_ledger().newest().expect("record present");
    assert_eq!(record.kind, EventKind::Scroll);
    assert_eq!(record.disposition, Disposition::Ignored);
    assert_eq!(record.hit_rejection, Some(HitRejection::OutsideBounds));
}

#[test]
fn test_zero_cost_when_disabled() {
    let mut arena = WidgetArena::new();
    let root = arena.insert_with_widget(
        create_hot(
            0.0,
            0.0,
            100.0,
            100.0,
            NodeFlags::VISIBLE | NodeFlags::HIT_TEST_ENABLED,
        ),
        Box::new(ConsumingWidget),
    );
    let mut router = EventRouter::new();
    // Ledger is NOT enabled (default).
    assert!(!router.is_ledger_enabled());

    let win = WindowId::from_raw(1);
    let click = make_click_event(Vec2::new(50.0, 50.0));
    router.dispatch_pointer_event(&mut arena, root, win, &click);
    router.dispatch_keyboard_event(&mut arena, Some(root), "Enter", true, false);
    router.dispatch_scroll_event(&mut arena, win, Vec2::new(0.0, 5.0));

    // Ledger must be completely empty!
    assert_eq!(router.event_ledger().len(), 0);
}

#[test]
fn test_global_debug_events_toggle() {
    let mut arena = WidgetArena::new();
    let root = arena.insert_with_widget(
        create_hot(
            0.0,
            0.0,
            100.0,
            100.0,
            NodeFlags::VISIBLE | NodeFlags::HIT_TEST_ENABLED,
        ),
        Box::new(ConsumingWidget),
    );
    let mut router = EventRouter::new();

    // Enable via global devtools toggle
    set_debug_events_enabled(true);
    assert!(router.is_ledger_enabled());

    let win = WindowId::from_raw(1);
    let click = make_click_event(Vec2::new(50.0, 50.0));
    router.dispatch_pointer_event(&mut arena, root, win, &click);
    assert_eq!(router.event_ledger().len(), 1);

    // Disable global toggle
    set_debug_events_enabled(false);
    assert!(!router.is_ledger_enabled());
}

#[test]
fn test_ring_buffer_wrapping_preserves_capacity() {
    let mut arena = WidgetArena::new();
    let root = arena.insert_with_widget(
        create_hot(
            0.0,
            0.0,
            100.0,
            100.0,
            NodeFlags::VISIBLE | NodeFlags::HIT_TEST_ENABLED,
        ),
        Box::new(ConsumingWidget),
    );
    let mut router = EventRouter::new();
    router.set_ledger_enabled(true);

    let win = WindowId::from_raw(1);
    let click = make_click_event(Vec2::new(50.0, 50.0));

    // Dispatch 1100 events into the 1024-capacity ring buffer
    for f in 0..1100 {
        router.set_frame(f);
        router.dispatch_pointer_event(&mut arena, root, win, &click);
    }

    let ledger = router.event_ledger();
    assert_eq!(ledger.len(), 1024);
    assert_eq!(ledger.capacity(), 1024);
    assert_eq!(ledger.oldest().unwrap().seq, 77);
    assert_eq!(ledger.newest().unwrap().seq, 1100);
}
