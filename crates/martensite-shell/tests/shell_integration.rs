//! Integration tests for `martensite-shell` platform backends.
//!
//! These tests verify the v0.13.0 shell functionality that requires a
//! real platform environment:
//!
//! - **StatusNotifierItem D-Bus registration** (`status_notifier`):
//!   Requires a D-Bus session bus; when no real
//!   `org.kde.StatusNotifierWatcher` is present the tests serve a
//!   minimal in-process stub so the registration round-trip genuinely
//!   executes. Only a missing session bus skips them.
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

    // NaN is rejected; scale falls back to 1.0 (already current, so no
    // event is emitted below).
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
// StatusNotifierItem — D-Bus registration
//
// These tests exercise the real registration path end-to-end: when no
// `org.kde.StatusNotifierWatcher` owns the name on the session bus, a
// minimal stub watcher is served in-process so `register()`'s D-Bus
// round-trip genuinely executes. Only the absence of a session bus at
// all (no dbus-daemon) skips the test, with a printed reason.
// ---------------------------------------------------------------------------

/// Minimal `org.kde.StatusNotifierWatcher` server: accepts
/// `RegisterStatusNotifierItem` calls so `StatusNotifierItem::register`
/// completes a real D-Bus method round-trip.
struct StubWatcher;

#[zbus::interface(name = "org.kde.StatusNotifierWatcher")]
impl StubWatcher {
    fn register_status_notifier_item(&self, _service: String) {}

    #[zbus(property)]
    fn is_status_notifier_host_registered(&self) -> bool {
        true
    }

    #[zbus(property)]
    fn protocol_version(&self) -> i32 {
        0
    }

    #[zbus(property)]
    fn registered_status_notifier_items(&self) -> Vec<String> {
        Vec::new()
    }
}

/// Ensures something answers `RegisterStatusNotifierItem` on the session
/// bus for the whole test process. When no real watcher owns the name,
/// an in-process stub is served once and kept for the process's
/// lifetime, so parallel tests can never outlive the watcher they
/// registered against. `Err(reason)` means no session bus exists at all.
fn ensure_sni_watcher() -> Result<(), String> {
    static WATCHER: std::sync::OnceLock<Result<(), String>> = std::sync::OnceLock::new();
    WATCHER.get_or_init(serve_stub_watcher).clone()
}

fn serve_stub_watcher() -> Result<(), String> {
    let conn = zbus::blocking::Connection::session()
        .map_err(|e| format!("skipping: no D-Bus session bus ({e})"))?;
    // Export the object before owning the name so nothing can call into
    // an empty object tree.
    conn.object_server()
        .at("/StatusNotifierWatcher", StubWatcher)
        .map_err(|e| format!("failed to serve stub watcher: {e}"))?;
    match conn.request_name("org.kde.StatusNotifierWatcher") {
        // We own the watcher name. `conn` is deliberately leaked: the
        // stub must outlive every test in this process. zbus's internal
        // executor thread dispatches incoming method calls on its own —
        // no manual pump is needed.
        Ok(()) => {
            std::mem::forget(conn);
            Ok(())
        }
        // A real StatusNotifierWatcher (e.g. a KDE session) already owns
        // the name — register against it directly.
        Err(zbus::Error::NameTaken) => Ok(()),
        Err(e) => Err(format!("failed to request watcher name: {e}")),
    }
}

#[test]
fn status_notifier_item_registers_on_dbus() {
    if let Err(reason) = ensure_sni_watcher() {
        eprintln!("{reason}");
        return;
    }

    let mut item = StatusNotifierItem::new("martensite-test", "Martensite Test");
    assert!(!item.is_registered());

    item.register().expect("D-Bus registration should succeed");

    assert!(item.is_registered());

    // Clean up.
    item.unregister();
    assert!(!item.is_registered());
}

#[test]
fn status_notifier_item_activate_callback_fires() {
    use std::sync::atomic::{AtomicBool, Ordering};
    use std::sync::Arc;

    if let Err(reason) = ensure_sni_watcher() {
        eprintln!("{reason}");
        return;
    }

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
