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

use std::sync::Arc;

use accesskit::{ActionHandler, ActionRequest, ActivationHandler, DeactivationHandler, TreeUpdate};
use martensite_core::WidgetArena;
use parking_lot::Mutex;

use crate::actions::ActionHandler as MartensiteActionHandler;
use crate::pump::AsyncEventPump;
use crate::AccessKitAdapter;

/// A bridge that connects the Martensite `WidgetArena` and
/// [`AccessKitAdapter`] to the `accesskit_winit` platform adapter.
///
/// It implements [`ActivationHandler`], [`ActionHandler`], and
/// [`DeactivationHandler`] by holding a `Mutex`-guarded arena and adapter.
/// Actions decoded from the platform are forwarded to an optional
/// [`MartensiteActionHandler`].
///
/// By default, decoded actions are enqueued in an [`AsyncEventPump`] and
/// processed in a batch once per frame via
/// [`MartensiteAccessBridge::process_pending_actions`]. This decouples the
/// platform IPC thread (which calls [`ActionHandler::do_action`]) from the
/// main thread, preventing re-entrancy and long stalls. Set
/// [`Self::set_action_handler`] to receive the batched actions.
pub struct MartensiteAccessBridge {
    inner: Mutex<BridgeInner>,
    pump: AsyncEventPump,
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
            pump: AsyncEventPump::new(),
        }
    }

    /// Sets the action handler that receives decoded Martensite actions.
    ///
    /// The handler must be `Send` because the AccessKit platform adapter
    /// may call `do_action` on any thread.
    ///
    /// Actions are not dispatched synchronously from `do_action`; they are
    /// enqueued in the bridge's [`AsyncEventPump`] and dispatched in a batch
    /// when the event loop calls [`Self::process_pending_actions`].
    pub fn set_action_handler<H: MartensiteActionHandler + Send + 'static>(&self, handler: H) {
        let mut inner = self.inner.lock();
        inner.action_handler = Some(Box::new(handler));
    }

    /// Returns a reference to the bridge's [`AsyncEventPump`].
    ///
    /// The pump accumulates decoded actions between frames. Callers can
    /// inspect [`AsyncEventPump::pending`] or manually [`AsyncEventPump::drain`]
    /// if they need the raw batch without dispatching to the handler.
    pub fn pump(&self) -> &AsyncEventPump {
        &self.pump
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
        let mut guard = self.inner.lock();
        let inner = &mut *guard;
        f(&mut inner.arena, &mut inner.adapter)
    }

    /// Builds and returns a full `TreeUpdate` from the current arena state.
    pub fn build_full_update(&self) -> TreeUpdate {
        let mut guard = self.inner.lock();
        let inner = &mut *guard;
        inner.adapter.build_update(&mut inner.arena)
    }

    /// Builds and returns an incremental `TreeUpdate` if there are dirty
    /// nodes or focus changes.
    pub fn build_incremental_update(&self) -> Option<TreeUpdate> {
        let mut guard = self.inner.lock();
        let inner = &mut *guard;
        inner.adapter.build_incremental_update(&mut inner.arena)
    }

    /// Processes all accessibility actions that were enqueued since the last
    /// call, dispatching them in a single batch to the registered
    /// [`MartensiteActionHandler`].
    ///
    /// This is the per-frame entry point: the application's event loop
    /// should call `process_pending_actions` once per frame (e.g. in
    /// `WindowEvent::MainEventsCleared` or after `AboutToWait`) so that
    /// actions arriving on the platform IPC thread are batched and
    /// processed on the main thread without re-entrancy.
    ///
    /// Returns the actions that were processed, in FIFO order. If no
    /// handler is registered, the actions are still drained (and returned)
    /// but not dispatched.
    ///
    /// # Examples
    ///
    /// ```no_run
    /// use martensite_access::winit::MartensiteAccessBridge;
    /// use martensite_access::AccessKitAdapter;
    /// use martensite_core::WidgetArena;
    ///
    /// let mut arena = WidgetArena::new();
    /// let root = arena.insert(Default::default(), Default::default());
    /// let adapter = AccessKitAdapter::new(root);
    /// let bridge = std::sync::Arc::new(MartensiteAccessBridge::new(arena, adapter));
    /// // In the event loop, once per frame:
    /// // bridge.process_pending_actions();
    /// ```
    pub fn process_pending_actions(&self) -> Vec<crate::actions::A11yAction> {
        let batch = self.pump.drain();
        if batch.is_empty() {
            return batch;
        }

        // Take the handler out of `inner` so we do not hold the `inner`
        // lock across the whole batch. `parking_lot::Mutex` is *not*
        // reentrant: invoking the handler while borrowing
        // `action_handler` from the guard (as the previous code did) would
        // deadlock if the handler ever calls back into a
        // `MartensiteAccessBridge` method that also locks `inner`.
        //
        // With the handler taken out, the lock is only re-acquired once
        // per action — in a narrow scope that grants the handler access
        // to the arena — and released again before the next action. This
        // lets other threads make progress between actions and avoids a
        // batch-spanning deadlock.
        let mut handler = self.inner.lock().action_handler.take();

        if let Some(handler) = handler.as_mut() {
            for action in &batch {
                // Per-action lock scope: acquire `inner` only long enough
                // to pass the arena to the handler, then drop the guard.
                let mut guard = self.inner.lock();
                handler.handle_action(&mut guard.arena, action);
                // `guard` is dropped here, releasing the lock until the
                // next action.
            }
        }

        // Put the handler back. If another thread called
        // `set_action_handler` while we were processing, that newer
        // handler wins and we drop the stale one.
        if handler.is_some() {
            let mut guard = self.inner.lock();
            if guard.action_handler.is_none() {
                guard.action_handler = handler;
            }
        }

        batch
    }
}

impl ActivationHandler for MartensiteAccessBridge {
    fn request_initial_tree(&mut self) -> Option<TreeUpdate> {
        let mut guard = self.inner.lock();
        let inner = &mut *guard;
        inner.activated = true;
        Some(inner.adapter.build_update(&mut inner.arena))
    }
}

impl ActionHandler for MartensiteAccessBridge {
    fn do_action(&mut self, request: ActionRequest) {
        let mut guard = self.inner.lock();
        let inner = &mut *guard;

        // Decode the action (immutable borrow of arena) and enqueue it for
        // batched processing on the main thread. This returns immediately,
        // avoiding re-entrancy and long IPC-thread stalls. The application's
        // event loop drains the queue via `process_pending_actions` once
        // per frame.
        let tree_id = inner.adapter.tree_id();
        let action = crate::actions::decode_action_request(&inner.arena, &request, &tree_id);

        if let Some(action) = action {
            self.pump.push(action);
        }
    }
}

impl DeactivationHandler for MartensiteAccessBridge {
    fn deactivate_accessibility(&mut self) {
        let mut guard = self.inner.lock();
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

        // do_action now enqueues; the action is not processed until
        // process_pending_actions is called.
        assert_eq!(bridge_mut.pump().pending(), 1);

        // Process the batch — the handler should now receive the action.
        let processed = bridge_mut.process_pending_actions();
        assert_eq!(processed.len(), 1);
        assert_eq!(processed[0], crate::actions::A11yAction::Focus(id));
        assert!(bridge_mut.pump().is_empty());
    }

    #[test]
    fn bridge_do_action_enqueues_without_processing() {
        use accesskit::ActionHandler as _;

        let mut arena = WidgetArena::new();
        let hot = HotNode {
            flags: NodeFlags::FOCUSABLE | NodeFlags::VISIBLE,
            ..Default::default()
        };
        let id = arena.insert(hot, ColdNode::default());
        let adapter = AccessKitAdapter::new(id);
        let bridge = MartensiteAccessBridge::new(arena, adapter);

        // No handler set — do_action should still enqueue.
        let request = ActionRequest {
            action: accesskit::Action::Focus,
            target_node: crate::widget_id_to_node_id(id),
            target_tree: accesskit::TreeId::ROOT,
            data: None,
        };
        let mut bridge_mut = bridge;
        bridge_mut.do_action(request);
        assert_eq!(bridge_mut.pump().pending(), 1);

        // process_pending_actions drains even without a handler.
        let processed = bridge_mut.process_pending_actions();
        assert_eq!(processed.len(), 1);
        assert!(bridge_mut.pump().is_empty());
    }

    #[test]
    fn bridge_batches_multiple_actions() {
        use accesskit::ActionHandler as _;

        let mut arena = WidgetArena::new();
        let hot = HotNode {
            flags: NodeFlags::FOCUSABLE | NodeFlags::VISIBLE,
            ..Default::default()
        };
        let id = arena.insert(hot, ColdNode::default());
        let adapter = AccessKitAdapter::new(id);
        let bridge = MartensiteAccessBridge::new(arena, adapter);
        bridge.set_action_handler(crate::actions::QueuedActionDispatcher::new());

        let make_request = |action: accesskit::Action| ActionRequest {
            action,
            target_node: crate::widget_id_to_node_id(id),
            target_tree: accesskit::TreeId::ROOT,
            data: None,
        };

        let mut bridge_mut = bridge;
        // Enqueue three actions across "frames".
        bridge_mut.do_action(make_request(accesskit::Action::Focus));
        bridge_mut.do_action(make_request(accesskit::Action::Click));
        bridge_mut.do_action(make_request(accesskit::Action::Blur));
        assert_eq!(bridge_mut.pump().pending(), 3);

        // Flush once — all three processed in a single batch.
        let processed = bridge_mut.process_pending_actions();
        assert_eq!(processed.len(), 3);
        assert!(bridge_mut.pump().is_empty());
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
        let inner = bridge.inner.lock();
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
        let inner = bridge.inner.lock();
        let hot = inner.arena.get_hot(root).unwrap();
        assert!(hot.flags.contains(NodeFlags::DIRTY_A11Y));
    }

    #[test]
    fn bridge_process_pending_actions_restores_handler() {
        use accesskit::ActionHandler as _;

        // The handler is taken out of `inner` during processing and put
        // back afterwards. Two consecutive `process_pending_actions`
        // calls must both dispatch to the handler, proving the handler is
        // restored (not lost) after the first batch.
        let mut arena = WidgetArena::new();
        let hot = HotNode {
            flags: NodeFlags::FOCUSABLE | NodeFlags::VISIBLE,
            ..Default::default()
        };
        let id = arena.insert(hot, ColdNode::default());
        let adapter = AccessKitAdapter::new(id);
        let bridge = MartensiteAccessBridge::new(arena, adapter);
        bridge.set_action_handler(crate::actions::QueuedActionDispatcher::new());

        let make_request = |action: accesskit::Action| ActionRequest {
            action,
            target_node: crate::widget_id_to_node_id(id),
            target_tree: accesskit::TreeId::ROOT,
            data: None,
        };

        let mut bridge_mut = bridge;
        bridge_mut.do_action(make_request(accesskit::Action::Focus));
        let first = bridge_mut.process_pending_actions();
        assert_eq!(first.len(), 1);

        // A second batch must still be dispatched (handler was restored).
        bridge_mut.do_action(make_request(accesskit::Action::Click));
        let second = bridge_mut.process_pending_actions();
        assert_eq!(second.len(), 1);
        assert_eq!(second[0], crate::actions::A11yAction::Click(id));
    }

    #[test]
    fn bridge_process_pending_actions_empty_is_short_circuit() {
        // When there is nothing to drain, the handler must not be touched
        // (it should remain registered and usable for the next non-empty
        // batch).
        let mut arena = WidgetArena::new();
        let hot = HotNode {
            flags: NodeFlags::FOCUSABLE | NodeFlags::VISIBLE,
            ..Default::default()
        };
        let id = arena.insert(hot, ColdNode::default());
        let adapter = AccessKitAdapter::new(id);
        let bridge = MartensiteAccessBridge::new(arena, adapter);
        bridge.set_action_handler(crate::actions::QueuedActionDispatcher::new());

        let processed = bridge.process_pending_actions();
        assert!(processed.is_empty());

        // Handler is still registered.
        let inner = bridge.inner.lock();
        assert!(inner.action_handler.is_some());
    }
}
