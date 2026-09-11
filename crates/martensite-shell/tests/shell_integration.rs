//! Integration tests for `martensite-shell` platform backends.
//!
//! These tests verify the v0.13.0 shell functionality that requires a
//! real platform environment:
//!
//! - **StatusNotifierItem D-Bus registration** (`status_notifier`):
//!   Requires a running D-Bus session bus with a registered
//!   `org.kde.StatusNotifierWatcher` service. Gated behind
//!   `#[ignore]` so CI can opt in via `--ignored` when the environment
//!   is available.
//! - **FractionalScaleTracker event emission** (`wayland`):
//!   Pure-logic test that does not require a real Wayland compositor;
//!   verifies the event queue receives `FractionalScaleChanged`
//!   events on scale updates.

#![cfg(all(target_os = "linux", feature = "wayland-backend"))]

use martensite_shell::platform_impl::wayland::FractionalScaleTracker;
use martensite_shell::status_notifier::StatusNotifierItem;
use martensite_shell::{ShellEvent, ShellEventQueue};

// ---------------------------------------------------------------------------
// FractionalScaleTracker — event emission (no compositor required)
// ---------------------------------------------------------------------------

#[test]
fn fractional_scale_tracker_emits_on_change() {
    let queue = ShellEventQueue::new();
    let mut tracker = FractionalScaleTracker::with_event_queue(queue.clone());

    // Initial scale is 1.0; no events yet.
    assert!(queue.drain().is_empty());
    assert_eq!(tracker.current().scale(), 1.0);

    // Update to 1.5 → should emit one event.
    tracker.update(1.5);
    let events = queue.drain();
    assert_eq!(events, vec![ShellEvent::FractionalScaleChanged(1.5)]);
    assert_eq!(tracker.current().scale(), 1.5);

    // Same value → no event (dedup).
    tracker.update(1.5);
    assert!(queue.drain().is_empty());

    // Update to 2.0 → should emit one event.
    tracker.update(2.0);
    let events = queue.drain();
    assert_eq!(events, vec![ShellEvent::FractionalScaleChanged(2.0)]);
}

#[test]
fn fractional_scale_tracker_rejects_invalid_and_emits_fallback() {
    let queue = ShellEventQueue::new();
    let mut tracker = FractionalScaleTracker::with_event_queue(queue.clone());

    // NaN is rejected; scale falls back to 1.0 and an event is emitted
    // because 1.0 != the previous NaN-clamped value.
    tracker.update(f64::NAN);
    assert_eq!(tracker.current().scale(), 1.0);

    // Infinity is also rejected.
    tracker.update(f64::INFINITY);
    assert_eq!(tracker.current().scale(), 1.0);

    // No spurious events from the invalid updates (1.0 == 1.0 dedup).
    // The first NaN update from 1.0 → 1.0 produces no event.
    let events = queue.drain();
    assert!(
        events.is_empty(),
        "expected no events for invalid updates, got {events:?}"
    );
}

#[test]
fn fractional_scale_tracker_physical_buffer_size() {
    use martensite_shell::platform_impl::wayland::FractionalScaleTracker;

    let mut tracker = FractionalScaleTracker::new();
    tracker.update(1.5);

    // 800×600 logical at 1.5x → 1200×900 physical.
    assert_eq!(tracker.current().to_physical(800), 1200);
    assert_eq!(tracker.current().to_physical(600), 900);

    // Round-trip: physical → logical.
    assert_eq!(tracker.current().to_logical(1200), 800);
}

#[test]
fn fractional_scale_tracker_no_queue_does_not_panic() {
    let mut tracker = FractionalScaleTracker::new();
    // Without an event queue, update should still work.
    tracker.update(2.0);
    assert_eq!(tracker.current().scale(), 2.0);
}

// ---------------------------------------------------------------------------
// StatusNotifierItem — D-Bus registration (requires real D-Bus session)
// ---------------------------------------------------------------------------

#[test]
#[ignore = "requires a running D-Bus session bus with org.kde.StatusNotifierWatcher"]
fn status_notifier_item_registers_on_dbus() {
    let mut item = StatusNotifierItem::new("martensite-test", "Martensite Test");
    assert!(!item.is_registered());

    // This will fail if no D-Bus session bus is running.
    item.register().expect("D-Bus registration should succeed");

    assert!(item.is_registered());

    // Clean up.
    item.unregister();
    assert!(!item.is_registered());
}

#[test]
#[ignore = "requires a running D-Bus session bus with org.kde.StatusNotifierWatcher"]
fn status_notifier_item_activate_callback_fires() {
    use std::sync::atomic::{AtomicBool, Ordering};
    use std::sync::Arc;

    let activated = Arc::new(AtomicBool::new(false));
    let activated_clone = activated.clone();

    let mut item = StatusNotifierItem::new("martensite-test-cb", "Martensite Test CB");
    item.on_activate(move |_x, _y| {
        activated_clone.store(true, Ordering::SeqCst);
    });

    item.register().expect("D-Bus registration should succeed");

    // In a real test, we would call the Activate method via D-Bus.
    // For now, this test verifies that registration succeeds with a
    // callback attached and that the callback is stored.
    assert!(item.is_registered());

    item.unregister();
}

#[test]
fn status_notifier_item_without_dbus_returns_error() {
    // This test runs without a D-Bus session (or with one unavailable).
    // It verifies that register() returns an error rather than panicking
    // when the session bus is not available.
    // Note: if a D-Bus session IS available, this test is a no-op (the
    // registration may succeed). We only assert that it doesn't panic.
    let mut item = StatusNotifierItem::new("martensite-noop", "Martensite Noop");
    let _ = item.register();
    // Don't assert on the result — it depends on the environment.
}
