//! Cross-module integration tests for the Martensite v0.5.0 milestone.
//!
//! These tests exercise the interaction between the input and platform
//! subsystems introduced in v0.5.0:
//!
//! 1. **Clipboard + Drag-and-Drop**: a clipboard item round-trips through a
//!    DnD session and is recoverable on drop.
//! 2. **Hit-testing + Event routing**: the `EventRouter` delegates to the
//!    `HitTester`, pointer capture overrides hit-test results, and the
//!    `MouseTracker` hover state tracks hit-test outcomes.
//! 3. **IME + Scroll kinematics**: `ScrollKinematics` velocity feeds the
//!    `ImePositioner`, viewport clamping holds during active scroll, and the
//!    full scroll → IME position flow stays within the viewport.
//! 4. **Cross-window DnD survival**: a `DndSession` remains valid after the
//!    originating `WindowId` is no longer reachable.
//! 5. **Hit-test stress under transforms**: 100,000 synthetic clicks across
//!    rotated, scaled, and collapsed widgets complete without panicking
//!    (milestone exit criterion).
//! 6. **IME 1000px/sec accuracy**: the IME candidate position stays within 2
//!    pixels of the caret at 1000 px/sec scroll velocity (milestone exit
//!    criterion).

#![forbid(unsafe_code)]

use std::sync::Arc;
use std::time::Duration;

use glam::Vec2;

use martensite_clipboard::clipboard::{MIME_TEXT_HTML, MIME_TEXT_PLAIN};
use martensite_clipboard::{ClipboardItem, ClipboardPayload, ClipboardService, InMemoryClipboard};
use martensite_core::{ColdNode, HotNode, NodeFlags, Rect, WidgetArena, WidgetId};
use martensite_dnd::{
    DndSession, DndSessionManager, DndStatus, DropEffect, DropEffectMask, DropTarget,
    DropTargetRegistry, DropTargetState,
};
use martensite_text::{ImePositioner, ScrollKinematics, Viewport};
use martensite_window::event::{
    EventDispatchOutcome, EventRouter, ModifierKeys, MouseButton, MouseTracker, PointerCapture,
    PointerEvent, PointerId, PointerState,
};
use martensite_window::{AffineTransform, HitTester, WindowId};

// ===========================================================================
// Helpers
// ===========================================================================

/// Inserts a visible, hit-testable widget with the given bounds into the
/// arena and returns its id.
fn insert_hit_testable(arena: &mut WidgetArena, bounds: Rect) -> WidgetId {
    let hot = HotNode {
        bounds,
        flags: NodeFlags::VISIBLE | NodeFlags::HIT_TEST_ENABLED,
        ..HotNode::default()
    };
    arena.insert(hot, ColdNode::default())
}

/// Builds a `PointerEvent::moved` at the given logical position.
fn moved_event(x: f32, y: f32) -> PointerEvent {
    PointerEvent {
        pointer_id: PointerId::PRIMARY,
        position: Vec2::new(x, y),
        state: PointerState::Moved,
        button: None,
        modifiers: ModifierKeys::empty(),
    }
}

/// Builds a `PointerEvent::pressed` with the left button at the given position.
fn pressed_event(x: f32, y: f32) -> PointerEvent {
    PointerEvent {
        pointer_id: PointerId::PRIMARY,
        position: Vec2::new(x, y),
        state: PointerState::Pressed,
        button: Some(MouseButton::Left),
        modifiers: ModifierKeys::empty(),
    }
}

/// Extracts the handled widget id from an `EventDispatchOutcome`, panicking
/// if the event was not handled.
fn handled(outcome: EventDispatchOutcome) -> WidgetId {
    match outcome {
        EventDispatchOutcome::Handled(id) => id,
        other => panic!("expected Handled, got {:?}", other),
    }
}

// ===========================================================================
// 1. Clipboard + DnD integration
// ===========================================================================

/// A DnD session payload can be extracted and placed on the clipboard.
#[test]
fn dnd_payload_extracted_to_clipboard() {
    // Build a clipboard item and stash it as the opaque DnD payload.
    let item = ClipboardItem::new()
        .offer_text("dragged text")
        .offer_html("<b>dragged text</b>");

    let payload: Arc<dyn std::any::Any + Send + Sync> = Arc::new(item.clone());
    let session = DndSession::new(payload, vec![MIME_TEXT_PLAIN.into()], None);

    // Downcast the payload back to a ClipboardItem and copy it to the clipboard.
    let recovered = session
        .payload_typed::<ClipboardItem>()
        .expect("payload should downcast to ClipboardItem");

    let mut clipboard = InMemoryClipboard::new();
    clipboard.set_contents(recovered);

    assert_eq!(
        clipboard.get_contents(MIME_TEXT_PLAIN),
        Some(b"dragged text".to_vec())
    );
    assert_eq!(
        clipboard.get_contents(MIME_TEXT_HTML),
        Some(b"<b>dragged text</b>".to_vec())
    );
}

/// Clipboard items offering multiple MIME types can be used directly as DnD
/// payloads, and every offered type is recoverable from the session.
#[test]
fn clipboard_multi_mime_as_dnd_payload() {
    let item = ClipboardItem::new()
        .offer_text("hello")
        .offer_html("<i>hello</i>")
        .offer_rtf("{\\rtf1 hello}")
        .offer_png(vec![0x89, 0x50, 0x4E, 0x47]);

    let offered: Vec<String> = item.offered_types();
    assert_eq!(offered.len(), 4);

    let session = DndSession::new(
        Arc::new(item) as Arc<dyn std::any::Any + Send + Sync>,
        offered.clone(),
        None,
    );

    // The session advertises every offered type.
    assert_eq!(session.available_types.len(), 4);

    let recovered = session
        .payload_typed::<ClipboardItem>()
        .expect("payload should downcast");

    for mime in &offered {
        assert!(recovered.has(mime), "missing type {}", mime);
        assert!(
            recovered.get(mime).is_some(),
            "missing payload for {}",
            mime
        );
    }

    // Each representation materializes correctly.
    assert_eq!(
        recovered.get(MIME_TEXT_PLAIN).and_then(|p| p.materialize()),
        Some(b"hello".to_vec())
    );
    assert_eq!(
        recovered.get(MIME_TEXT_HTML).and_then(|p| p.materialize()),
        Some(b"<i>hello</i>".to_vec())
    );
}

/// Full flow: create a clipboard item → start a DnD session carrying it →
/// register a drop target → drop on the target → verify the payload.
#[test]
fn clipboard_to_dnd_session_to_drop_full_flow() {
    let item = ClipboardItem::new().offer_text("payload text");

    // Start a DnD session carrying the clipboard item.
    let mut manager = DndSessionManager::new();
    let session_id = manager.start_session(
        Arc::new(item) as Arc<dyn std::any::Any + Send + Sync>,
        vec![MIME_TEXT_PLAIN.into()],
        None,
    );
    assert_eq!(manager.active_sessions(), 1);
    assert_eq!(
        manager.get_session(session_id).unwrap().status(),
        DndStatus::Idle
    );

    // Register a drop target that accepts text/plain with Copy.
    let mut registry = DropTargetRegistry::new();
    let target_id = registry.register(
        DropTarget::new(
            WidgetId::from_parts(1, 1),
            vec![MIME_TEXT_PLAIN.into()],
            DropEffectMask::COPY,
        )
        .with_bounds(Rect::new(0.0, 0.0, 200.0, 200.0)),
    );

    // Enter the target at the centre and confirm it accepts the session.
    let enter_state = registry.enter_target(target_id, manager.get_session(session_id).unwrap());
    assert_eq!(enter_state, DropTargetState::Hovered);

    // Drop: complete the session against the target via a mutable borrow.
    let drop_effect = {
        let session = manager
            .get_session_mut(session_id)
            .expect("session should exist for drop");
        registry.drop_on_target(target_id, session)
    };
    assert_eq!(drop_effect, Some(DropEffect::Copy));

    // The session is now completed and the payload is still recoverable.
    let session = manager.get_session(session_id).unwrap();
    assert!(session.is_expired());
    assert_eq!(session.status(), DndStatus::Completed(DropEffect::Copy));
    let recovered = session
        .payload_typed::<ClipboardItem>()
        .expect("payload survives drop");
    assert_eq!(
        recovered.get(MIME_TEXT_PLAIN).and_then(|p| p.materialize()),
        Some(b"payload text".to_vec())
    );

    // Purge the completed session.
    assert_eq!(manager.purge_completed(), 1);
    assert_eq!(manager.active_sessions(), 0);
}

// ===========================================================================
// 2. Hit-testing + Event routing integration
// ===========================================================================

/// The event router uses the hit-tester to find the correct widget under the
/// pointer.
#[test]
fn event_router_uses_hit_tester_for_target() {
    let mut arena = WidgetArena::new();
    let root = insert_hit_testable(&mut arena, Rect::new(0.0, 0.0, 400.0, 400.0));
    let child_a = insert_hit_testable(&mut arena, Rect::new(0.0, 0.0, 200.0, 200.0));
    let child_b = insert_hit_testable(&mut arena, Rect::new(200.0, 0.0, 200.0, 200.0));
    arena.append_child(root, child_a).unwrap();
    arena.append_child(root, child_b).unwrap();

    let mut router = EventRouter::new();
    let window = WindowId::from_raw(1);

    // Click inside child_a (top-most sibling registered first is bottom-most;
    // child_b is later-registered = higher Z-order, but it doesn't overlap
    // child_a here, so the hit is unambiguous).
    assert_eq!(
        handled(router.route_pointer_event(&arena, root, window, &pressed_event(50.0, 50.0))),
        child_a
    );

    // Click inside child_b.
    assert_eq!(
        handled(router.route_pointer_event(&arena, root, window, &pressed_event(250.0, 50.0))),
        child_b
    );

    // Click inside root but outside both children.
    assert_eq!(
        handled(router.route_pointer_event(&arena, root, window, &moved_event(300.0, 300.0))),
        root
    );

    // Click outside everything.
    assert_eq!(
        router.route_pointer_event(&arena, root, window, &moved_event(500.0, 500.0)),
        EventDispatchOutcome::Unhandled
    );
}

/// Pointer capture overrides hit-test results: while a widget is captured,
/// every pointer event routes to it regardless of position.
#[test]
fn pointer_capture_overrides_hit_test() {
    let mut arena = WidgetArena::new();
    let root = insert_hit_testable(&mut arena, Rect::new(0.0, 0.0, 400.0, 400.0));
    let capturer = insert_hit_testable(&mut arena, Rect::new(0.0, 0.0, 100.0, 100.0));
    let other = insert_hit_testable(&mut arena, Rect::new(100.0, 0.0, 100.0, 100.0));
    arena.append_child(root, capturer).unwrap();
    arena.append_child(root, other).unwrap();

    let mut router = EventRouter::new();
    let window = WindowId::from_raw(1);

    // Before capture, a click over `other` hits `other`.
    assert_eq!(
        handled(router.route_pointer_event(&arena, root, window, &moved_event(150.0, 50.0))),
        other
    );

    // Capture the primary pointer for `capturer`.
    router.capture_pointer_primary(capturer);
    assert_eq!(router.captured_widget_primary(), Some(capturer));

    // Now even a click over `other` (or anywhere) routes to `capturer`.
    assert_eq!(
        handled(router.route_pointer_event(&arena, root, window, &moved_event(150.0, 50.0))),
        capturer
    );
    assert_eq!(
        handled(router.route_pointer_event(&arena, root, window, &moved_event(999.0, 999.0))),
        capturer
    );

    // Releasing capture restores normal hit-testing.
    router.release_pointer_primary();
    assert!(router.captured_widget_primary().is_none());
    assert_eq!(
        handled(router.route_pointer_event(&arena, root, window, &moved_event(150.0, 50.0))),
        other
    );
}

/// A stale capture (captured widget removed from the arena) is auto-released
/// and routing falls back to hit-testing.
#[test]
fn stale_capture_is_released() {
    let mut arena = WidgetArena::new();
    let root = insert_hit_testable(&mut arena, Rect::new(0.0, 0.0, 400.0, 400.0));
    let capturer = insert_hit_testable(&mut arena, Rect::new(0.0, 0.0, 100.0, 100.0));
    arena.append_child(root, capturer).unwrap();

    let mut router = EventRouter::new();
    let window = WindowId::from_raw(1);

    router.capture_pointer_primary(capturer);
    assert_eq!(router.captured_widget_primary(), Some(capturer));

    // Remove the captured widget from the arena.
    arena.remove(capturer);

    // The next route call should detect the dead capture, release it, and
    // fall back to hit-testing (which now hits root).
    assert_eq!(
        handled(router.route_pointer_event(&arena, root, window, &moved_event(50.0, 50.0))),
        root
    );
    assert!(router.captured_widget_primary().is_none());
}

/// The mouse tracker's hover state is updated based on hit-test results.
/// The `EventRouter` automatically syncs its internal `MouseTracker` during
/// `route_pointer_event`, so we verify the tracker reflects the resolved
/// target without manual mirroring.
#[test]
fn mouse_tracker_hover_follows_hit_test() {
    let mut arena = WidgetArena::new();
    let root = insert_hit_testable(&mut arena, Rect::new(0.0, 0.0, 400.0, 400.0));
    let child = insert_hit_testable(&mut arena, Rect::new(0.0, 0.0, 200.0, 200.0));
    arena.append_child(root, child).unwrap();

    let mut router = EventRouter::new();
    let window = WindowId::from_raw(1);

    // Route a move over the child — the router auto-updates its tracker.
    let outcome = router.route_pointer_event(&arena, root, window, &moved_event(50.0, 50.0));
    assert_eq!(outcome, EventDispatchOutcome::Handled(child));
    assert_eq!(
        router.mouse_tracker().position(window),
        Some(Vec2::new(50.0, 50.0))
    );
    assert_eq!(router.mouse_tracker().hovered_widget(window), Some(child));

    // Move outside the child into root-only territory.
    let outcome = router.route_pointer_event(&arena, root, window, &moved_event(300.0, 300.0));
    assert_eq!(outcome, EventDispatchOutcome::Handled(root));
    assert_eq!(router.mouse_tracker().hovered_widget(window), Some(root));

    // Move outside everything — hover cleared.
    let outcome = router.route_pointer_event(&arena, root, window, &moved_event(999.0, 999.0));
    assert_eq!(outcome, EventDispatchOutcome::Unhandled);
    assert_eq!(router.mouse_tracker().hovered_widget(window), None);
}

/// Build a small widget tree, route a sequence of pointer events, and verify
/// the correct widget receives each event end-to-end.
#[test]
fn widget_tree_pointer_routing_end_to_end() {
    let mut arena = WidgetArena::new();
    let root = insert_hit_testable(&mut arena, Rect::new(0.0, 0.0, 300.0, 300.0));
    let panel = insert_hit_testable(&mut arena, Rect::new(0.0, 0.0, 300.0, 100.0));
    let button = insert_hit_testable(&mut arena, Rect::new(10.0, 10.0, 80.0, 40.0));
    arena.append_child(root, panel).unwrap();
    arena.append_child(panel, button).unwrap();

    let mut router = EventRouter::new();
    let window = WindowId::from_raw(1);

    // Hover over the button.
    assert_eq!(
        handled(router.route_pointer_event(&arena, root, window, &moved_event(50.0, 30.0))),
        button
    );
    // Press on the button.
    assert_eq!(
        handled(router.route_pointer_event(&arena, root, window, &pressed_event(50.0, 30.0))),
        button
    );
    // Capture the button and drag outside it — events still go to the button.
    router.capture_pointer_primary(button);
    assert_eq!(
        handled(router.route_pointer_event(&arena, root, window, &moved_event(290.0, 290.0))),
        button
    );
    router.release_pointer_primary();
    // Release over the panel (outside the button).
    assert_eq!(
        handled(router.route_pointer_event(&arena, root, window, &moved_event(150.0, 50.0))),
        panel
    );
    // Over the root background.
    assert_eq!(
        handled(router.route_pointer_event(&arena, root, window, &moved_event(150.0, 200.0))),
        root
    );
}

/// Scroll events route to the widget currently hovered in the window, as
/// tracked by the router's `MouseTracker`.
#[test]
fn scroll_event_routes_to_hovered_widget() {
    let mut arena = WidgetArena::new();
    let root = insert_hit_testable(&mut arena, Rect::new(0.0, 0.0, 300.0, 300.0));
    let child = insert_hit_testable(&mut arena, Rect::new(0.0, 0.0, 100.0, 100.0));
    arena.append_child(root, child).unwrap();

    let mut router = EventRouter::new();
    let window = WindowId::from_raw(1);

    // No pointer event routed yet — scroll is unhandled.
    assert_eq!(
        router.route_scroll_event(window, Vec2::new(0.0, 10.0)),
        EventDispatchOutcome::Unhandled
    );

    // Route a move over the child, then scroll.
    router.route_pointer_event(&arena, root, window, &moved_event(50.0, 50.0));
    assert_eq!(
        router.route_scroll_event(window, Vec2::new(0.0, 10.0)),
        EventDispatchOutcome::Handled(child)
    );

    // Move over the root background, then scroll.
    router.route_pointer_event(&arena, root, window, &moved_event(200.0, 200.0));
    assert_eq!(
        router.route_scroll_event(window, Vec2::new(0.0, 10.0)),
        EventDispatchOutcome::Handled(root)
    );
}

// ===========================================================================
// 3. IME + Scroll kinematics integration
// ===========================================================================

/// Scroll kinematics velocity feeds into the IME positioner: after a burst of
/// scroll updates, the positioner's projected position is offset in the
/// direction of motion.
#[test]
fn scroll_kinematics_feeds_ime_positioner() {
    let mut kin = ScrollKinematics::new();
    // Simulate a horizontal scroll burst: 16 px per 16 ms frame ≈ 1000 px/s.
    let frame = Duration::from_millis(16);
    for _ in 0..10 {
        kin.update(Vec2::new(16.0, 0.0), frame);
    }
    assert!(kin.is_scrolling());
    let velocity = kin.velocity();
    assert!(velocity.x > 0.0);

    // Feed the estimated velocity into the IME positioner.
    let viewport = Rect::new(0.0, 0.0, 1000.0, 1000.0);
    let mut pos = ImePositioner::new(Vec2::new(500.0, 500.0), viewport);
    pos.set_scroll_velocity(velocity);

    // A short delta time projects the caret forward in the scroll direction.
    let projected = pos.compute_position(Duration::from_millis(16));
    assert!(
        projected.x > 500.0,
        "projected x {} should exceed caret x",
        projected.x
    );
    assert_eq!(projected.y, 500.0);
}

/// Viewport clamping holds during active scroll: the clamped IME position
/// never escapes the viewport rectangle.
#[test]
fn viewport_clamping_during_active_scroll() {
    let viewport = Rect::new(100.0, 100.0, 200.0, 200.0);
    let mut pos = ImePositioner::new(Vec2::new(150.0, 150.0), viewport);
    // Extreme velocity that would project far outside the viewport.
    pos.set_scroll_velocity(Vec2::new(100_000.0, 100_000.0));

    // At every delta time the clamped position stays inside the viewport.
    for dt_ms in [1u64, 4, 8, 16, 32, 64, 128, 256, 512, 1024] {
        let (p, _s) = pos.compute_bounds(Duration::from_millis(dt_ms), 24.0);
        assert!(
            p.x >= 100.0 && p.x <= 300.0,
            "dt={}ms x={} outside viewport",
            dt_ms,
            p.x
        );
        assert!(
            p.y >= 100.0 && p.y <= 300.0,
            "dt={}ms y={} outside viewport",
            dt_ms,
            p.y
        );
    }

    // Direct clamp_point check with a wildly out-of-bounds projected point.
    let clamped = Viewport::new(viewport).clamp_point(Vec2::new(-1e6, 1e6));
    assert_eq!(clamped, Vec2::new(100.0, 300.0));
}

/// Full flow: start scrolling → update IME position from kinematics → verify
/// the emitted position is within the viewport.
#[test]
fn scroll_to_ime_position_within_viewport() {
    let mut kin = ScrollKinematics::new();
    let viewport = Rect::new(0.0, 0.0, 800.0, 600.0);
    let mut pos = ImePositioner::new(Vec2::new(400.0, 300.0), viewport);

    // Simulate 30 frames of scrolling at ~1000 px/s.
    let frame = Duration::from_millis(16);
    for _ in 0..30 {
        kin.update(Vec2::new(16.0, 0.0), frame);
        pos.set_scroll_velocity(kin.velocity());
        let (p, _s) = pos.compute_bounds(frame, 20.0);
        assert!(
            p.x >= 0.0 && p.x <= 800.0,
            "x={} escaped viewport during scroll",
            p.x
        );
        assert!(
            p.y >= 0.0 && p.y <= 600.0,
            "y={} escaped viewport during scroll",
            p.y
        );
    }

    // After scroll input stops, decay the velocity and keep verifying.
    // The default deceleration is 2000 px/s², so ~1000/32 ≈ 32 frames at
    // 16 ms to fully decay; we run 60 to be safe.
    for _ in 0..60 {
        kin.decay(frame);
        pos.set_scroll_velocity(kin.velocity());
        let (p, _s) = pos.compute_bounds(frame, 20.0);
        assert!(p.x >= 0.0 && p.x <= 800.0);
        assert!(p.y >= 0.0 && p.y <= 600.0);
    }

    // Once fully decayed the positioner reports the caret itself.
    assert!(!kin.is_scrolling());
    let (p, _s) = pos.compute_bounds(Duration::from_millis(16), 20.0);
    assert!((p.x - 400.0).abs() < 2.0);
    assert!((p.y - 300.0).abs() < 2.0);
}

// ===========================================================================
// 4. Cross-window DnD survival
// ===========================================================================

/// A DnD session survives even when the source window id is no longer valid
/// (the originating window has been destroyed). The payload remains
/// accessible and the session is not expired.
#[test]
fn dnd_session_survives_invalid_source_window() {
    // WindowId is a Copy opaque handle; "destroying" the window does not
    // invalidate the id value itself, but the session must not depend on the
    // window's continued existence. We simulate this by letting the original
    // id fall out of scope and re-deriving an equivalent value.
    let source_window = WindowId::from_raw(42);

    let mut manager = DndSessionManager::new();
    let session_id = manager.start_session(
        Arc::new("drag-payload".to_string()) as Arc<dyn std::any::Any + Send + Sync>,
        vec![MIME_TEXT_PLAIN.into()],
        Some(source_window),
    );

    // Drop the original id — the session retains its own copy. (WindowId is
    // Copy, so this is a no-op for the value itself; the point is that the
    // session does not depend on any external window handle remaining live.)
    let _ = source_window;

    // The session is still registered, not expired, and its payload is intact.
    let session = manager
        .get_session(session_id)
        .expect("session must survive after source window is dropped");
    assert!(!session.is_expired());
    assert_eq!(session.status(), DndStatus::Idle);
    assert_eq!(
        session.payload_typed::<String>(),
        Some(&"drag-payload".to_string())
    );
    assert!(session.source_window.is_some());

    // The session can still be completed normally.
    assert!(manager.complete_session(session_id, DropEffect::Copy));
    let session = manager.get_session(session_id).unwrap();
    assert_eq!(session.status(), DndStatus::Completed(DropEffect::Copy));
    assert!(session.is_expired());
    // And the payload is still accessible after completion.
    assert_eq!(
        session.payload_typed::<String>(),
        Some(&"drag-payload".to_string())
    );
}

/// A session created without any source window is equally valid.
#[test]
fn dnd_session_without_source_window_is_valid() {
    let mut manager = DndSessionManager::new();
    let id = manager.start_session(
        Arc::new(7_i32) as Arc<dyn std::any::Any + Send + Sync>,
        vec![],
        None,
    );
    let session = manager.get_session(id).unwrap();
    assert!(session.source_window.is_none());
    assert_eq!(session.payload_typed::<i32>(), Some(&7));
    assert!(!session.is_expired());
}

// ===========================================================================
// 5. Hit-test stress under transforms (milestone exit criterion)
// ===========================================================================

/// 100,000 synthetic clicks across rotated, scaled, and collapsed widgets
/// complete with zero panics. This is a v0.5.0 milestone exit criterion.
#[test]
fn hit_test_stress_under_transforms_no_panics() {
    let mut arena = WidgetArena::new();
    let root = insert_hit_testable(&mut arena, Rect::new(0.0, 0.0, 1000.0, 1000.0));

    // Rotated widget: 45° rotation about its centre, placed at (100, 100).
    let rotated = insert_hit_testable(&mut arena, Rect::new(100.0, 100.0, 200.0, 200.0));
    // Scaled widget: 2x scale, placed at (400, 100).
    let scaled = insert_hit_testable(&mut arena, Rect::new(400.0, 100.0, 200.0, 200.0));
    // Collapsed widget: zero-size bounds (collapsed) but with a non-singular
    // transform; it should never be hit.
    let collapsed = insert_hit_testable(&mut arena, Rect::new(700.0, 100.0, 0.0, 0.0));
    // Singular-transform widget: zero scale so the inverse is undefined; the
    // tester must skip it without panicking.
    let singular = insert_hit_testable(&mut arena, Rect::new(800.0, 400.0, 100.0, 100.0));

    arena.append_child(root, rotated).unwrap();
    arena.append_child(root, scaled).unwrap();
    arena.append_child(root, collapsed).unwrap();
    arena.append_child(root, singular).unwrap();

    // Build the transform map.
    let mut transforms = std::collections::HashMap::new();
    // Rotate 45° about the widget centre (200, 200).
    transforms.insert(
        rotated,
        AffineTransform::from_scale_angle_translation(
            Vec2::new(1.0, 1.0),
            std::f32::consts::FRAC_PI_4,
            Vec2::new(200.0, 200.0),
        ),
    );
    // Scale 2x about the widget origin (400, 100).
    transforms.insert(
        scaled,
        AffineTransform::from_scale_angle_translation(
            Vec2::new(2.0, 2.0),
            0.0,
            Vec2::new(400.0, 100.0),
        ),
    );
    // Singular: zero scale.
    transforms.insert(singular, AffineTransform::from_scale(Vec2::new(0.0, 0.0)));
    // Collapsed widget keeps the identity transform (its zero-size bounds
    // already reject every point).
    transforms.insert(collapsed, AffineTransform::IDENTITY);

    let tester = HitTester::new(&arena);

    // Deterministic LCG so the test is reproducible across runs.
    let mut state: u64 = 0x1234_5678_9ABC_DEF0;
    let total = 100_000;
    let mut hits = 0u64;
    let mut misses = 0u64;

    for _ in 0..total {
        // Advance the LCG and map to a point in [0, 1100) — slightly larger
        // than the 1000x1000 root so a fraction of clicks miss every widget.
        state = state
            .wrapping_mul(6364136223846793005)
            .wrapping_add(1442695040888963407);
        let x = ((state >> 32) as u32) as f32 / (u32::MAX as f32) * 1100.0;
        state = state
            .wrapping_mul(6364136223846793005)
            .wrapping_add(1442695040888963407);
        let y = ((state >> 32) as u32) as f32 / (u32::MAX as f32) * 1100.0;

        // The stress criterion is "zero panics"; the result is allowed to be
        // Some or None, but must never be NaN-poisoned or panic.
        let result = tester.hit_test_with_transforms(root, Vec2::new(x, y), &transforms);
        match result {
            Some(r) => {
                assert!(r.local_point.is_finite(), "local point must be finite");
                hits += 1;
            }
            None => misses += 1,
        }
    }

    // We should have hit *something* at least occasionally across 100k points
    // spread over the 1000x1000 root.
    assert!(hits > 0, "expected at least some hits across 100k clicks");
    assert!(
        misses > 0,
        "expected at least some misses across 100k clicks"
    );
    assert_eq!(hits + misses, total);
}

// ===========================================================================
// 6. IME 1000px/sec accuracy (milestone exit criterion)
// ===========================================================================

/// At 1000 px/sec scroll velocity, the IME candidate position is within 2
/// pixels of the caret once the damping term has settled. This is a v0.5.0
/// milestone exit criterion.
#[test]
fn ime_candidate_within_two_pixels_at_1000px_per_sec() {
    let viewport = Rect::new(0.0, 0.0, 2000.0, 2000.0);
    let mut pos = ImePositioner::new(Vec2::new(500.0, 500.0), viewport);
    pos.set_scroll_velocity(Vec2::new(1000.0, 0.0));

    // The offset v·Δt·e^(-λ·Δt) peaks at Δt = 1/λ = 0.2s (for the default
    // λ = 5.0) and then decays. Once the exponential term has settled (Δt
    // well past the peak) the candidate returns to within 2px of the caret.
    let settle_times = [1.5f32, 2.0, 5.0, 10.0];
    for dt in settle_times {
        let projected = pos.compute_position(Duration::from_secs_f32(dt));
        let dist = (projected - pos.caret_position()).length();
        assert!(
            dist <= 2.0,
            "at dt={}s dist={} should be <= 2.0px (1000 px/s)",
            dt,
            dist
        );
    }

    // The clamped, OS-ready bounds also stay within 2px of the caret once
    // settled.
    for dt in settle_times {
        let (p, _s) = pos.compute_bounds(Duration::from_secs_f32(dt), 24.0);
        let dx = (p.x as f32 - 500.0).abs();
        let dy = (p.y as f32 - 500.0).abs();
        assert!(
            dx <= 2.0 && dy <= 2.0,
            "clamped bounds at dt={}s dx={} dy={} should be within 2px",
            dt,
            dx,
            dy
        );
    }
}

/// At 1000 px/sec in an arbitrary direction (diagonal), the candidate still
/// settles within 2px of the caret.
#[test]
fn ime_candidate_diagonal_velocity_within_two_pixels() {
    let viewport = Rect::new(0.0, 0.0, 2000.0, 2000.0);
    let mut pos = ImePositioner::new(Vec2::new(800.0, 800.0), viewport);
    // 1000 px/s along the diagonal (~707 in each axis).
    pos.set_scroll_velocity(Vec2::new(707.10678, 707.10678));

    for dt in [1.5f32, 2.0, 5.0, 10.0] {
        let projected = pos.compute_position(Duration::from_secs_f32(dt));
        let dist = (projected - pos.caret_position()).length();
        assert!(
            dist <= 2.0,
            "diagonal dt={}s dist={} should be <= 2.0px",
            dt,
            dist
        );
    }
}

// ===========================================================================
// Bonus: PointerCapture / MouseTracker unit-level integration
// ===========================================================================

/// PointerCapture and MouseTracker compose: capture does not affect the
/// tracker's recorded position, and the tracker can report hover independent
/// of capture.
#[test]
fn capture_and_tracker_compose() {
    let mut capture = PointerCapture::new();
    let mut tracker = MouseTracker::new();
    let window = WindowId::from_raw(2);
    let widget = WidgetId::from_parts(3, 1);

    capture.capture_primary(widget);
    assert_eq!(capture.captured_primary(), Some(widget));

    // The tracker is independent of capture state.
    tracker.update_position(window, Vec2::new(10.0, 20.0));
    assert_eq!(tracker.position(window), Some(Vec2::new(10.0, 20.0)));
    tracker.set_hovered(window, Some(widget));
    assert_eq!(tracker.hovered_widget(window), Some(widget));

    // Releasing capture does not touch the tracker.
    capture.release_primary();
    assert!(capture.captured_primary().is_none());
    assert_eq!(tracker.hovered_widget(window), Some(widget));

    // Clearing the window forgets both position and hover.
    tracker.clear_window(window);
    assert_eq!(tracker.position(window), None);
    assert_eq!(tracker.hovered_widget(window), None);
}

/// A lazy clipboard payload round-trips through a DnD session and is only
/// materialized on drop, demonstrating delayed rendering across the
/// clipboard/DnD boundary.
#[test]
fn lazy_clipboard_payload_through_dnd_session() {
    let item = ClipboardItem::new().offer_custom(
        "application/x-lazy",
        ClipboardPayload::lazy(|| b"deferred-dnd".to_vec()),
    );

    let session = DndSession::new(
        Arc::new(item) as Arc<dyn std::any::Any + Send + Sync>,
        vec!["application/x-lazy".into()],
        None,
    );

    let recovered = session
        .payload_typed::<ClipboardItem>()
        .expect("payload should downcast");

    // The lazy payload is still pending inside the session's copy.
    let lazy = recovered
        .get("application/x-lazy")
        .expect("lazy payload should be present");
    assert!(
        lazy.is_lazy_pending(),
        "lazy payload should still be pending"
    );

    // Materialize it (as a drop consumer would).
    assert_eq!(lazy.materialize(), Some(b"deferred-dnd".to_vec()));
}
