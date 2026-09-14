//! iOS end-to-end smoke test for [`IosAdapter`].
//!
//! This test binary uses `harness = false` (see `Cargo.toml`) so its
//! `main` runs on the process's real main thread — a hard requirement:
//! `accesskit_ios::SubclassingAdapter` internally unwraps
//! `MainThreadMarker::new()`, which returns `None` on the worker threads
//! libtest spawns, so no `#[test]`-harness test could construct the
//! adapter. The custom harness is the only way to exercise the path.
//!
//! The test is additionally env-gated like the workspace's hardware
//! gates (`MARTENSITE_MEDIA_4K120`, etc.): it is a no-op unless
//! `MARTENSITE_IOS_SIM_TESTS=1` is set. Cargo cannot launch test
//! binaries on the simulator, so the flow is manual:
//!
//! ```sh
//! cargo test -p martensite-access-platform --target aarch64-apple-ios-sim \
//!     --no-run
//! # Deploy target/aarch64-apple-ios-sim/debug/deps/ios_adapter-* into a
//! # simulator (xcrun simctl spawn) and run with MARTENSITE_IOS_SIM_TESTS=1.
//! ```
//!
//! What it exercises end-to-end, on ios-sim without requiring VoiceOver:
//!
//! 1. A real `UIView` is created on the main thread.
//! 2. [`IosAdapter::new`] dynamically subclasses it (the Objective-C FFI
//!    path).
//! 3. UIKit's `accessibilityElements` is queried on the view — the
//!    dynamically installed `UIAccessibilityContainer` method forwards
//!    into the adapter, which calls `request_initial_tree`, builds the
//!    AccessKit consumer tree, and materializes platform nodes.
//! 4. The activation flag and the returned element array are asserted.
//!
//! Note: `update_if_active`/`QueuedEvents` only produce events once an
//! assistive technology reports running; the element-enumeration path
//! activates the tree unconditionally, which is what this test drives.

#[cfg(target_os = "ios")]
fn main() {
    if std::env::var_os("MARTENSITE_IOS_SIM_TESTS").is_none() {
        println!("ios_adapter: skipped (set MARTENSITE_IOS_SIM_TESTS=1 on ios-sim)");
        return;
    }
    imp::run();
    println!("ios_adapter: PASS");
}

#[cfg(not(target_os = "ios"))]
fn main() {
    // Compiles everywhere; a no-op off iOS.
}

// The `msg_send!`/`UIView::new` FFI calls in this test are exactly what
// the crate's unsafe whitelist exists for; the workspace `unsafe_code`
// deny-lint is relaxed here (test-only, iOS-only, manual env gate).
#[cfg(target_os = "ios")]
#[allow(unsafe_code)]
mod imp {
    use std::ffi::c_void;
    use std::ptr::NonNull;
    use std::sync::atomic::{AtomicBool, Ordering};
    use std::sync::Arc;

    use accesskit::{
        ActionHandler, ActionRequest, ActivationHandler, DeactivationHandler, Node, NodeId, Role,
        TreeId, TreeInfo, TreeUpdate,
    };
    use martensite_access_platform::ios::IosAdapter;
    use objc2::msg_send;
    use objc2_foundation::{MainThreadMarker, NSArray, NSObject};
    use objc2_ui_kit::UIView;

    /// Handlers shared across the three AccessKit callback traits; the
    /// flags record whether the platform adapter invoked them.
    struct Handlers {
        activated: Arc<AtomicBool>,
        actions: Arc<AtomicBool>,
        deactivated: Arc<AtomicBool>,
    }

    impl Clone for Handlers {
        fn clone(&self) -> Self {
            Self {
                activated: Arc::clone(&self.activated),
                actions: Arc::clone(&self.actions),
                deactivated: Arc::clone(&self.deactivated),
            }
        }
    }

    impl ActivationHandler for Handlers {
        fn request_initial_tree(&mut self) -> Option<TreeUpdate> {
            self.activated.store(true, Ordering::SeqCst);
            let root = NodeId(1);
            let button = NodeId(2);
            let mut root_node = Node::new(Role::Window);
            root_node.set_label("ios_adapter root");
            root_node.set_children([button]);
            let mut button_node = Node::new(Role::Button);
            button_node.set_label("Tap me");
            Some(TreeUpdate {
                nodes: vec![(root, root_node), (button, button_node)],
                tree: Some(TreeInfo::new(root)),
                tree_id: TreeId::ROOT,
                focus: button,
            })
        }
    }

    impl ActionHandler for Handlers {
        fn do_action(&mut self, _request: ActionRequest) {
            self.actions.store(true, Ordering::SeqCst);
        }
    }

    impl DeactivationHandler for Handlers {
        fn deactivate_accessibility(&mut self) {
            self.deactivated.store(true, Ordering::SeqCst);
        }
    }

    pub fn run() {
        // We are on the real main thread (custom test harness `main`),
        // so `MainThreadMarker::new()` succeeds — a libtest `#[test]`
        // worker thread would fail here and inside the adapter.
        let mtm = MainThreadMarker::new().expect("test must run on the main thread");

        let activated = Arc::new(AtomicBool::new(false));
        let actions = Arc::new(AtomicBool::new(false));
        let deactivated = Arc::new(AtomicBool::new(false));
        let handlers = Handlers {
            activated: Arc::clone(&activated),
            actions: Arc::clone(&actions),
            deactivated: Arc::clone(&deactivated),
        };

        // SAFETY: `mtm` proves we are on the main thread, the only
        // requirement `UIView::new` has beyond ordinary allocation.
        let view = unsafe { UIView::new(mtm) };

        // The FFI boundary itself lives inside `IosAdapter`; the call
        // site is safe. `view` is a live `UIView` created above, and the
        // adapter is constructed before the view is ever shown.
        let adapter = IosAdapter::new(
            NonNull::from(&*view).cast::<c_void>(),
            handlers.clone(),
            handlers.clone(),
            handlers,
        );

        // Drive the UIAccessibilityContainer path exactly as UIKit would:
        // `accessibilityElements` on the view resolves to the method the
        // dynamic subclass installed, which forces tree initialization
        // (request_initial_tree) and materializes platform nodes.
        //
        // SAFETY: `view` is a live `UIView`; `accessibilityElements` is a
        // method on its (subclassed) class returning an autoreleased
        // `NSArray *`.
        let elements: *mut NSArray<NSObject> = unsafe { msg_send![&*view, accessibilityElements] };

        assert!(
            activated.load(Ordering::SeqCst),
            "request_initial_tree was not invoked by the adapter"
        );
        assert!(!elements.is_null(), "accessibilityElements returned null");
        // SAFETY: `elements` is a valid autoreleased NSArray pointer.
        let count = unsafe { (*elements).count() };
        assert!(
            count >= 1,
            "accessibilityElements returned {count} elements; expected >= 1"
        );
        println!("ios_adapter: {count} accessibility element(s) exported");

        // The incremental update path must not panic once the tree is
        // active; QueuedEvents may be empty with no assistive tech.
        adapter.update_if_active(|| {
            let mut node = Node::new(Role::Button);
            node.set_label("Tap me");
            TreeUpdate {
                nodes: vec![(NodeId(2), node)],
                tree: None,
                tree_id: TreeId::ROOT,
                focus: NodeId(2),
            }
        });

        drop(adapter);
        println!(
            "ios_adapter: adapter dropped; deactivation handler called: {}",
            deactivated.load(Ordering::SeqCst)
        );
    }
}
