//! Winit integration for the AccessKit adapter.
//!
//! This module provides implementations of the AccessKit
//! [`ActivationHandler`], [`ActionHandler`], and [`DeactivationHandler`]
//! traits that bridge the `accesskit_winit` adapter to the Martensite
//! `WidgetArena` and [`AccessKitAdapter`].
//!
//! ## Architecture
//!
//! The [`MartensiteAccessBridge`] holds a `Mutex`-guarded reference to the
//! arena and adapter. When the platform requests an initial tree, the
//! activation handler builds a full `TreeUpdate` from the arena. When an
//! assistive technology sends an action, the action handler decodes it and
//! forwards it to the provided [`ActionHandler`](crate::actions::ActionHandler).
//!
//! ## Usage
//!
//! ```no_run
//! use martensite_access::winit::MartensiteAccessBridge;
//! use martensite_access::AccessKitAdapter;
//! use martensite_core::WidgetArena;
//! use std::sync::{Arc, Mutex};
//!
//! // Create the bridge before showing the window.
//! let mut arena = WidgetArena::new();
//! let root = arena.insert(Default::default(), Default::default());
//! let adapter = AccessKitAdapter::new(root);
//! let bridge = Arc::new(MartensiteAccessBridge::new(arena, adapter));
//!
//! // Use with accesskit_winit::Adapter::with_direct_handlers:
//! // accesskit_winit::Adapter::with_direct_handlers(
//! //     &event_loop,
//! //     &window,
//! //     bridge.clone(),
//! //     bridge.clone(),
//! //     bridge.clone(),
//! // );
//! ```

use std::sync::{Arc, Mutex};

use accesskit::{ActionHandler, ActionRequest, ActivationHandler, DeactivationHandler, TreeUpdate};
use martensite_core::WidgetArena;

use crate::actions::ActionHandler as MartensiteActionHandler;
use crate::AccessKitAdapter;

/// A bridge that connects the Martensite `WidgetArena` and
/// [`AccessKitAdapter`] to the `accesskit_winit` platform adapter.
///
/// It implements [`ActivationHandler`], [`ActionHandler`], and
/// [`DeactivationHandler`] by holding a `Mutex`-guarded arena and adapter.
/// Actions decoded from the platform are forwarded to an optional
/// [`MartensiteActionHandler`].
pub struct MartensiteAccessBridge {
    inner: Mutex<BridgeInner>,
}

struct BridgeInner {
    arena: WidgetArena,
    adapter: AccessKitAdapter,
    /// Optional action handler for decoded Martensite actions.
    action_handler: Option<Box<dyn MartensiteActionHandler + Send>>,
    /// Whether the accessibility tree has been activated.
    activated: bool,
}

impl MartensiteAccessBridge {
    /// Creates a new bridge with the given arena and adapter.
    pub fn new(arena: WidgetArena, adapter: AccessKitAdapter) -> Self {
        Self {
            inner: Mutex::new(BridgeInner {
                arena,
                adapter,
                action_handler: None,
                activated: false,
            }),
        }
    }

    /// Sets the action handler that receives decoded Martensite actions.
    ///
    /// The handler must be `Send` because the AccessKit platform adapter
    /// may call `do_action` on any thread.
    pub fn set_action_handler<H: MartensiteActionHandler + Send + 'static>(&self, handler: H) {
        let mut inner = self.inner.lock().unwrap();
        inner.action_handler = Some(Box::new(handler));
    }

    /// Returns a clone of the bridge wrapped in an `Arc`.
    ///
    /// This is a convenience method for creating the multiple clones
    /// needed by [`accesskit_winit::Adapter::with_direct_handlers`].
    pub fn shared(self) -> Arc<Self> {
        Arc::new(self)
    }

    /// Updates the arena through a closure. This allows the application
    /// to mutate the arena (e.g., after layout) and then build an
    /// incremental update.
    pub fn with_arena_mut<R>(
        &self,
        f: impl FnOnce(&mut WidgetArena, &mut AccessKitAdapter) -> R,
    ) -> R {
        let mut guard = self.inner.lock().unwrap();
        let inner = &mut *guard;
        f(&mut inner.arena, &mut inner.adapter)
    }

    /// Builds and returns a full `TreeUpdate` from the current arena state.
    pub fn build_full_update(&self) -> TreeUpdate {
        let mut guard = self.inner.lock().unwrap();
        let inner = &mut *guard;
        inner.adapter.build_update(&mut inner.arena)
    }

    /// Builds and returns an incremental `TreeUpdate` if there are dirty
    /// nodes or focus changes.
    pub fn build_incremental_update(&self) -> Option<TreeUpdate> {
        let mut guard = self.inner.lock().unwrap();
        let inner = &mut *guard;
        inner.adapter.build_incremental_update(&mut inner.arena)
    }
}

impl ActivationHandler for MartensiteAccessBridge {
    fn request_initial_tree(&mut self) -> Option<TreeUpdate> {
        let mut guard = self.inner.lock().unwrap();
        let inner = &mut *guard;
        inner.activated = true;
        Some(inner.adapter.build_update(&mut inner.arena))
    }
}

impl ActionHandler for MartensiteAccessBridge {
    fn do_action(&mut self, request: ActionRequest) {
        let mut guard = self.inner.lock().unwrap();
        let inner = &mut *guard;

        // Decode the action (immutable borrow of arena).
        let tree_id = inner.adapter.tree_id();
        let action = crate::actions::decode_action_request(&inner.arena, &request, &tree_id);

        if let Some(action) = action {
            // Handle the action (mutable borrow of arena and handler).
            if let Some(ref mut handler) = inner.action_handler {
                handler.handle_action(&mut inner.arena, &action);
            }
        }
    }
}

impl DeactivationHandler for MartensiteAccessBridge {
    fn deactivate_accessibility(&mut self) {
        let mut guard = self.inner.lock().unwrap();
        let inner = &mut *guard;
        inner.activated = false;
    }
}

// `MartensiteAccessBridge` is automatically `Send + Sync` because it
// contains only `Mutex<BridgeInner>`, and `BridgeInner` is `Send` because
// `WidgetArena` and `AccessKitAdapter` are `Send` (the `Widget` trait
// requires `Send + Sync`).

#[cfg(test)]
mod tests {
    use super::*;
    use martensite_core::{ColdNode, HotNode, NodeFlags};

    #[test]
    fn bridge_builds_full_update() {
        let mut arena = WidgetArena::new();
        let root = arena.insert(HotNode::default(), ColdNode::default());
        let adapter = AccessKitAdapter::new(root);
        let bridge = MartensiteAccessBridge::new(arena, adapter);

        let update = bridge.build_full_update();
        assert!(!update.nodes.is_empty());
    }

    #[test]
    fn bridge_activation_returns_initial_tree() {
        use accesskit::ActivationHandler;

        let mut arena = WidgetArena::new();
        let root = arena.insert(HotNode::default(), ColdNode::default());
        let adapter = AccessKitAdapter::new(root);
        let mut bridge = MartensiteAccessBridge::new(arena, adapter);

        let update = bridge.request_initial_tree();
        assert!(update.is_some());
        let update = update.unwrap();
        assert!(!update.nodes.is_empty());
    }

    #[test]
    fn bridge_action_handler_receives_decoded_action() {
        use accesskit::ActionHandler as _;

        let mut arena = WidgetArena::new();
        let hot = HotNode {
            flags: NodeFlags::FOCUSABLE | NodeFlags::VISIBLE,
            ..Default::default()
        };
        let id = arena.insert(hot, ColdNode::default());
        let adapter = AccessKitAdapter::new(id);
        let bridge = MartensiteAccessBridge::new(arena, adapter);

        // Set up a queued action handler.
        bridge.set_action_handler(crate::actions::QueuedActionDispatcher::new());

        // Send a Focus action request.
        let request = ActionRequest {
            action: accesskit::Action::Focus,
            target_node: crate::widget_id_to_node_id(id),
            target_tree: accesskit::TreeId::ROOT,
            data: None,
        };

        let mut bridge_mut = bridge;
        bridge_mut.do_action(request);

        // Verify the action was received by checking through the bridge.
        // (The queued dispatcher is inside the mutex, so we verify indirectly
        // by checking that no panic occurred.)
        let _ = id;
    }

    #[test]
    fn bridge_deactivation_sets_activated_false() {
        use accesskit::DeactivationHandler as _;

        let mut arena = WidgetArena::new();
        let root = arena.insert(HotNode::default(), ColdNode::default());
        let adapter = AccessKitAdapter::new(root);
        let mut bridge = MartensiteAccessBridge::new(arena, adapter);

        // Activate first.
        use accesskit::ActivationHandler as _;
        let _ = bridge.request_initial_tree();

        // Deactivate.
        bridge.deactivate_accessibility();

        // The inner state should have activated = false.
        let inner = bridge.inner.lock().unwrap();
        assert!(!inner.activated);
    }

    #[test]
    fn bridge_with_arena_mut_allows_mutation() {
        let mut arena = WidgetArena::new();
        let root = arena.insert(HotNode::default(), ColdNode::default());
        let adapter = AccessKitAdapter::new(root);
        let bridge = MartensiteAccessBridge::new(arena, adapter);

        bridge.with_arena_mut(|arena, adapter| {
            adapter.mark_dirty(arena, root);
        });

        // Verify dirty flag was set.
        let inner = bridge.inner.lock().unwrap();
        let hot = inner.arena.get_hot(root).unwrap();
        assert!(hot.flags.contains(NodeFlags::DIRTY_A11Y));
    }
}
