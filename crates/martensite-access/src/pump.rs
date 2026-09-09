//! Asynchronous, batched accessibility event pump.
//!
//! The platform accessibility subsystem (Windows UI Automation, macOS
//! NSAccessibility, Linux AT-SPI2) may deliver action requests on an IPC
//! thread at arbitrary times. Processing them synchronously inside the
//! AccessKit `ActionHandler::do_action` callback risks re-entrancy and
//! long main-thread stalls when the handler mutates the widget arena.
//!
//! [`AsyncEventPump`] decouples *enqueueing* (on the IPC thread) from
//! *processing* (on the main thread, once per frame). `do_action` pushes
//! decoded [`A11yAction`](crate::actions::A11yAction)s into a lock-free-ish `Mutex<VecDeque>` queue
//! and returns immediately. The application's event loop calls
//! [`AsyncEventPump::flush`] (or [`AsyncEventPump::drain`]) once per frame
//! to batch-process all pending actions in a single, predictable pass.
//!
//! # Examples
//!
//! ```
//! use martensite_access::actions::A11yAction;
//! use martensite_access::pump::AsyncEventPump;
//! use martensite_core::WidgetId;
//!
//! let pump = AsyncEventPump::new();
//! let id = WidgetId::from_parts(0, 1);
//!
//! // IPC thread enqueues actions.
//! pump.push(A11yAction::Click(id));
//! pump.push(A11yAction::Focus(id));
//! assert_eq!(pump.pending(), 2);
//!
//! // Main thread drains once per frame.
//! let batch = pump.drain();
//! assert_eq!(batch.len(), 2);
//! assert_eq!(batch[0], A11yAction::Click(id));
//! assert!(pump.is_empty());
//! ```

use std::collections::VecDeque;
use std::sync::atomic::{AtomicUsize, Ordering};

use parking_lot::Mutex;
use tracing::warn;

use crate::actions::{A11yAction, ActionHandler};
use martensite_core::WidgetArena;

/// Maximum number of actions [`AsyncEventPump`] buffers before it starts
/// dropping the oldest entries.
///
/// This bounds memory use when an assistive technology enqueues actions
/// faster than the UI thread flushes them. When the queue is full, the
/// *oldest* action is dropped to make room for the new one, because the
/// newest actions reflect the user's most recent intent and are the most
/// relevant to dispatch. Dropped actions are counted in
/// [`AsyncEventPump::dropped_count`] and logged with `tracing::warn!`.
pub const MAX_QUEUE_SIZE: usize = 1024;

/// A thread-safe, batched queue of accessibility actions.
///
/// Actions are pushed onto an internal `Mutex<VecDeque>` (the
/// [`parking_lot`] mutex is poison-free) and batch-drained once per frame
/// by the application's event loop. This decouples the platform IPC thread
/// (which calls [`Self::push`] via `do_action`) from the main thread
/// (which calls [`Self::flush`] or [`Self::drain`]).
///
/// The queue is bounded at [`MAX_QUEUE_SIZE`] entries. If a producer
/// enqueues faster than the UI thread drains, the oldest actions are
/// dropped (and counted in [`Self::dropped_count`]) so memory growth stays
/// bounded while preserving the most recent actions.
pub struct AsyncEventPump {
    queue: Mutex<VecDeque<A11yAction>>,
    dropped: AtomicUsize,
}

impl AsyncEventPump {
    /// Creates a new empty event pump.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite_access::pump::AsyncEventPump;
    ///
    /// let pump = AsyncEventPump::new();
    /// assert!(pump.is_empty());
    /// ```
    pub fn new() -> Self {
        Self {
            queue: Mutex::new(VecDeque::new()),
            dropped: AtomicUsize::new(0),
        }
    }

    /// Enqueues an action for later batch processing.
    ///
    /// This is intended to be called from the AccessKit `do_action`
    /// callback (which may run on an IPC thread). It acquires the internal
    /// mutex briefly and appends to the back of the queue.
    ///
    /// The queue is bounded at [`MAX_QUEUE_SIZE`] entries. If it is full,
    /// the *oldest* action is dropped (and counted in
    /// [`Self::dropped_count`]) before the new action is appended, so the
    /// newest actions are always preserved. A drop is logged with
    /// `tracing::warn!`.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite_access::actions::A11yAction;
    /// use martensite_access::pump::AsyncEventPump;
    /// use martensite_core::WidgetId;
    ///
    /// let pump = AsyncEventPump::new();
    /// let id = WidgetId::from_parts(0, 1);
    /// pump.push(A11yAction::Focus(id));
    /// assert_eq!(pump.pending(), 1);
    /// ```
    pub fn push(&self, action: A11yAction) {
        let mut guard = self.queue.lock();
        if guard.len() >= MAX_QUEUE_SIZE {
            // Queue is full: drop the oldest entry to make room. Newest
            // actions matter more for AT feedback, so we keep the tail.
            if let Some(dropped) = guard.pop_front() {
                self.dropped.fetch_add(1, Ordering::Relaxed);
                warn!(
                    action = ?dropped,
                    queue_len = guard.len(),
                    max = MAX_QUEUE_SIZE,
                    "accessibility action queue is full; dropped oldest action"
                );
            }
        }
        guard.push_back(action);
    }

    /// Returns the number of actions that have been dropped due to the
    /// queue being full (see [`MAX_QUEUE_SIZE`]) since the pump was
    /// created.
    ///
    /// This is a best-effort counter intended for diagnostics and
    /// telemetry. It uses relaxed atomic ordering.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite_access::pump::AsyncEventPump;
    ///
    /// let pump = AsyncEventPump::new();
    /// assert_eq!(pump.dropped_count(), 0);
    /// ```
    pub fn dropped_count(&self) -> usize {
        self.dropped.load(Ordering::Relaxed)
    }

    /// Returns the number of actions currently queued.
    pub fn pending(&self) -> usize {
        self.queue.lock().len()
    }

    /// Returns `true` if no actions are queued.
    pub fn is_empty(&self) -> bool {
        self.queue.lock().is_empty()
    }

    /// Batch-drains all queued actions, returning them in FIFO order.
    ///
    /// The queue is left empty after this call. This does *not* process
    /// the actions; use [`Self::flush`] to drain and dispatch them to a
    /// handler in one step.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite_access::actions::A11yAction;
    /// use martensite_access::pump::AsyncEventPump;
    /// use martensite_core::WidgetId;
    ///
    /// let pump = AsyncEventPump::new();
    /// let id = WidgetId::from_parts(0, 1);
    /// pump.push(A11yAction::Click(id));
    /// pump.push(A11yAction::Focus(id));
    ///
    /// let batch = pump.drain();
    /// assert_eq!(batch.len(), 2);
    /// assert!(pump.is_empty());
    /// ```
    pub fn drain(&self) -> Vec<A11yAction> {
        let mut guard = self.queue.lock();
        guard.drain(..).collect()
    }

    /// Drains all queued actions and dispatches them to `handler` in a
    /// single batch, mutating `arena` for each action.
    ///
    /// This is the per-frame entry point: the application's event loop
    /// calls `flush` once per frame to process all actions that arrived
    /// since the last frame. Returns the actions that were processed
    /// (in FIFO order) so callers can inspect or log them.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite_access::actions::{A11yAction, ActionHandler, QueuedActionDispatcher};
    /// use martensite_access::pump::AsyncEventPump;
    /// use martensite_core::{WidgetArena, WidgetId};
    ///
    /// let pump = AsyncEventPump::new();
    /// let id = WidgetId::from_parts(0, 1);
    /// pump.push(A11yAction::Click(id));
    ///
    /// let mut arena = WidgetArena::new();
    /// let mut handler = QueuedActionDispatcher::new();
    /// let processed = pump.flush(&mut arena, &mut handler);
    /// assert_eq!(processed.len(), 1);
    /// assert!(pump.is_empty());
    /// ```
    pub fn flush(
        &self,
        arena: &mut WidgetArena,
        handler: &mut dyn ActionHandler,
    ) -> Vec<A11yAction> {
        let batch = self.drain();
        for action in &batch {
            handler.handle_action(arena, action);
        }
        batch
    }
}

impl Default for AsyncEventPump {
    fn default() -> Self {
        Self::new()
    }
}

impl std::fmt::Debug for AsyncEventPump {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("AsyncEventPump")
            .field("pending", &self.pending())
            .field("dropped", &self.dropped_count())
            .finish()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::actions::QueuedActionDispatcher;
    use martensite_core::{ColdNode, HotNode, NodeFlags, WidgetArena, WidgetId};

    fn sample_id() -> WidgetId {
        WidgetId::from_parts(0, 1)
    }

    #[test]
    fn new_pump_is_empty() {
        let pump = AsyncEventPump::new();
        assert!(pump.is_empty());
        assert_eq!(pump.pending(), 0);
    }

    #[test]
    fn default_is_empty() {
        let pump = AsyncEventPump::default();
        assert!(pump.is_empty());
    }

    #[test]
    fn push_increments_pending() {
        let pump = AsyncEventPump::new();
        let id = sample_id();
        pump.push(A11yAction::Click(id));
        assert_eq!(pump.pending(), 1);
        pump.push(A11yAction::Focus(id));
        assert_eq!(pump.pending(), 2);
        assert!(!pump.is_empty());
    }

    #[test]
    fn drain_returns_fifo_order() {
        let pump = AsyncEventPump::new();
        let id = sample_id();
        pump.push(A11yAction::Click(id));
        pump.push(A11yAction::Focus(id));
        pump.push(A11yAction::Blur(id));

        let batch = pump.drain();
        assert_eq!(batch.len(), 3);
        assert_eq!(batch[0], A11yAction::Click(id));
        assert_eq!(batch[1], A11yAction::Focus(id));
        assert_eq!(batch[2], A11yAction::Blur(id));
        assert!(pump.is_empty());
    }

    #[test]
    fn drain_empty_returns_empty_vec() {
        let pump = AsyncEventPump::new();
        let batch = pump.drain();
        assert!(batch.is_empty());
    }

    #[test]
    fn drain_can_be_called_twice() {
        let pump = AsyncEventPump::new();
        let id = sample_id();
        pump.push(A11yAction::Click(id));
        let first = pump.drain();
        assert_eq!(first.len(), 1);
        let second = pump.drain();
        assert!(second.is_empty());
    }

    #[test]
    fn flush_dispatches_all_actions_to_handler() {
        let pump = AsyncEventPump::new();
        let id = sample_id();
        pump.push(A11yAction::Click(id));
        pump.push(A11yAction::Focus(id));
        pump.push(A11yAction::Blur(id));

        let mut arena = WidgetArena::new();
        let mut handler = QueuedActionDispatcher::new();

        let processed = pump.flush(&mut arena, &mut handler);
        assert_eq!(processed.len(), 3);
        assert_eq!(handler.len(), 3);
        assert!(pump.is_empty());
        // The handler received the same actions in the same order.
        let drained = handler.drain();
        assert_eq!(drained, processed);
    }

    #[test]
    fn flush_empty_does_nothing() {
        let pump = AsyncEventPump::new();
        let mut arena = WidgetArena::new();
        let mut handler = QueuedActionDispatcher::new();
        let processed = pump.flush(&mut arena, &mut handler);
        assert!(processed.is_empty());
        assert!(handler.is_empty());
    }

    #[test]
    fn flush_processes_actions_in_batches() {
        // Simulate two frames: enqueue some actions, flush, enqueue more, flush.
        let pump = AsyncEventPump::new();
        let id = sample_id();
        let mut arena = WidgetArena::new();
        let mut handler = QueuedActionDispatcher::new();

        // Frame 1: two actions.
        pump.push(A11yAction::Click(id));
        pump.push(A11yAction::Focus(id));
        let frame1 = pump.flush(&mut arena, &mut handler);
        assert_eq!(frame1.len(), 2);
        assert!(pump.is_empty());

        // Frame 2: one action.
        pump.push(A11yAction::Blur(id));
        let frame2 = pump.flush(&mut arena, &mut handler);
        assert_eq!(frame2.len(), 1);
        assert!(pump.is_empty());

        // Total dispatched.
        assert_eq!(handler.len(), 3);
    }

    #[test]
    fn flush_handler_mutates_arena() {
        // Verify the handler actually receives a mutable arena reference.
        let pump = AsyncEventPump::new();
        let mut arena = WidgetArena::new();
        let target = arena.insert(
            HotNode {
                flags: NodeFlags::FOCUSABLE | NodeFlags::VISIBLE,
                ..Default::default()
            },
            ColdNode::default(),
        );
        pump.push(A11yAction::Focus(target));

        let mut handler = crate::actions::ClosureActionHandler::new(|arena, action| {
            if let A11yAction::Focus(id) = action {
                // Mutate the arena to prove we received it.
                if let Some(hot) = arena.get_hot_mut(*id) {
                    hot.flags |= NodeFlags::HOVERED;
                }
            }
        });
        pump.flush(&mut arena, &mut handler);
        let hot = arena.get_hot(target).unwrap();
        assert!(hot.flags.contains(NodeFlags::HOVERED));
    }

    #[test]
    fn debug_format_shows_pending() {
        let pump = AsyncEventPump::new();
        let id = sample_id();
        pump.push(A11yAction::Click(id));
        let s = format!("{:?}", pump);
        assert!(s.contains("AsyncEventPump"));
        assert!(s.contains("1"));
    }

    #[test]
    fn debug_format_shows_dropped() {
        let pump = AsyncEventPump::new();
        let id = sample_id();
        // Overfill the queue to force drops.
        for _ in 0..(MAX_QUEUE_SIZE + 3) {
            pump.push(A11yAction::Click(id));
        }
        let s = format!("{:?}", pump);
        assert!(s.contains("dropped"));
        assert!(s.contains("3"));
    }

    #[test]
    fn queue_is_bounded_at_max_size() {
        let pump = AsyncEventPump::new();
        let id = sample_id();

        // Fill the queue exactly to capacity.
        for i in 0..MAX_QUEUE_SIZE {
            pump.push(A11yAction::Click(id));
            assert_eq!(pump.pending(), i + 1, "queue should grow until full");
        }
        assert_eq!(pump.pending(), MAX_QUEUE_SIZE);
        assert_eq!(pump.dropped_count(), 0);

        // Pushing beyond capacity drops the oldest entry each time but
        // keeps the queue size capped.
        for extra in 1..=5 {
            pump.push(A11yAction::Focus(id));
            assert_eq!(
                pump.pending(),
                MAX_QUEUE_SIZE,
                "queue must stay capped at MAX_QUEUE_SIZE"
            );
            assert_eq!(pump.dropped_count(), extra);
        }
    }

    #[test]
    fn queue_drops_oldest_keeps_newest() {
        let pump = AsyncEventPump::new();
        // Use distinct ids (slot_idx = i, generation = 1) so we can
        // identify which actions survived the drops. Generation must be
        // non-zero for `WidgetId::from_parts`.
        for i in 0..MAX_QUEUE_SIZE {
            let id = WidgetId::from_parts(i as u32, 1);
            pump.push(A11yAction::Click(id));
        }
        // Now overflow with Focus actions carrying a sentinel id.
        let sentinel = WidgetId::from_parts(9999, 1);
        for _ in 0..3 {
            pump.push(A11yAction::Focus(sentinel));
        }

        let batch = pump.drain();
        assert_eq!(batch.len(), MAX_QUEUE_SIZE);
        assert_eq!(pump.dropped_count(), 3);

        // The three newest entries should be the Focus(sentinel) actions.
        let tail: Vec<_> = batch.iter().rev().take(3).collect();
        assert!(tail.iter().all(|a| **a == A11yAction::Focus(sentinel)));

        // The oldest surviving entry should be the 4th pushed Click
        // (slot indices 0, 1, 2 were dropped).
        let oldest_surviving = batch[0].clone();
        assert_eq!(
            oldest_surviving,
            A11yAction::Click(WidgetId::from_parts(3, 1))
        );
    }

    #[test]
    fn dropped_count_starts_at_zero() {
        let pump = AsyncEventPump::new();
        assert_eq!(pump.dropped_count(), 0);
    }
}
