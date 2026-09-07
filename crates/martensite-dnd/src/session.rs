//! Detached, process-wide drag-and-drop sessions.
//!
//! A [`DndSession`] holds an opaque, thread-safe payload (`Arc<dyn Any + Send
//! + Sync>`) that remains valid for the entire lifetime of a drag operation,
//! even if the originating window or widget is evicted from the widget arena
//! mid-flight. The session is only retired when the OS signals a drop
//! completion or cancellation, at which point [`DndSession::complete`] or
//! [`DndSession::cancel`] transitions it to a terminal status.
//!
//! [`DndSessionManager`] tracks all active sessions process-wide using a
//! monotonically increasing [`SessionId`] counter.

use crate::DropEffect;
use std::any::Any;
use std::collections::HashMap;
use std::sync::Arc;
use winit::window::WindowId;

/// Unique identifier for a [`DndSession`] within a [`DndSessionManager`].
///
/// IDs are allocated from a monotonically increasing `u64` counter and are
/// never reused within a single manager instance.
#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct SessionId(pub u64);

/// Lifecycle status of a [`DndSession`].
#[derive(Copy, Clone, Debug, PartialEq, Eq, Default)]
pub enum DndStatus {
    /// The session has been created but no terminal event has occurred yet.
    #[default]
    Idle,
    /// The session is actively being dragged across the platform.
    Active,
    /// The drop completed successfully with the given [`DropEffect`].
    Completed(DropEffect),
    /// The drag was cancelled (e.g. user released over no target or pressed
    /// escape).
    Cancelled,
}

/// A detached drag-and-drop session holding an opaque, thread-safe payload.
///
/// The payload is stored as `Arc<dyn Any + Send + Sync>` so that it outlives
/// the originating window/widget even if that window is evicted from the
/// widget arena while the drag is still in flight.
///
/// # Examples
///
/// ```
/// use martensite_dnd::{DndSession, DropEffect};
/// use std::sync::Arc;
///
/// let session = DndSession::new(Arc::new(42_i32), vec!["text/plain".into()], None);
/// assert_eq!(session.payload_typed::<i32>(), Some(&42));
/// assert!(!session.is_expired());
/// ```
pub struct DndSession {
    /// The opaque, thread-safe payload carried by this drag operation.
    pub payload: Arc<dyn Any + Send + Sync>,
    /// MIME types advertised as available by the drag source.
    pub available_types: Vec<String>,
    /// The window that originated the drag, if known. `None` when the
    /// originating window has already been destroyed or the drag was
    /// initiated without a window.
    pub source_window: Option<WindowId>,
    status: DndStatus,
}

impl DndSession {
    /// Creates a new session in the [`DndStatus::Idle`] state.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite_dnd::{DndSession, DndStatus};
    /// use std::sync::Arc;
    ///
    /// let session = DndSession::new(Arc::new("hi"), vec![], None);
    /// assert_eq!(session.status(), DndStatus::Idle);
    /// ```
    pub fn new(
        payload: Arc<dyn Any + Send + Sync>,
        available_types: Vec<String>,
        source_window: Option<WindowId>,
    ) -> Self {
        Self {
            payload,
            available_types,
            source_window,
            status: DndStatus::Idle,
        }
    }

    /// Attempts to downcast the payload to a concrete type `T`.
    ///
    /// Returns `Some(&T)` if the payload is exactly of type `T`, otherwise
    /// `None`.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite_dnd::DndSession;
    /// use std::sync::Arc;
    ///
    /// let session = DndSession::new(Arc::new(100_u64), vec![], None);
    /// assert_eq!(session.payload_typed::<u64>(), Some(&100_u64));
    /// assert_eq!(session.payload_typed::<i32>(), None);
    /// ```
    pub fn payload_typed<T: Any + Send + Sync>(&self) -> Option<&T> {
        self.payload.downcast_ref::<T>()
    }

    /// Returns `true` if the session has reached a terminal status
    /// ([`DndStatus::Completed`] or [`DndStatus::Cancelled`]).
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite_dnd::{DndSession, DropEffect};
    /// use std::sync::Arc;
    ///
    /// let mut session = DndSession::new(Arc::new(0_i32), vec![], None);
    /// assert!(!session.is_expired());
    /// session.complete(DropEffect::Copy);
    /// assert!(session.is_expired());
    /// ```
    pub fn is_expired(&self) -> bool {
        matches!(self.status, DndStatus::Completed(_) | DndStatus::Cancelled)
    }

    /// Marks the session as completed with the negotiated `effect`.
    ///
    /// No-op if the session is already in a terminal status.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite_dnd::{DndSession, DndStatus, DropEffect};
    /// use std::sync::Arc;
    ///
    /// let mut session = DndSession::new(Arc::new(0_i32), vec![], None);
    /// session.complete(DropEffect::Move);
    /// assert_eq!(session.status(), DndStatus::Completed(DropEffect::Move));
    /// ```
    pub fn complete(&mut self, effect: DropEffect) {
        if !self.is_expired() {
            self.status = DndStatus::Completed(effect);
        }
    }

    /// Marks the session as cancelled.
    ///
    /// No-op if the session is already in a terminal status.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite_dnd::{DndSession, DndStatus};
    /// use std::sync::Arc;
    ///
    /// let mut session = DndSession::new(Arc::new(0_i32), vec![], None);
    /// session.cancel();
    /// assert_eq!(session.status(), DndStatus::Cancelled);
    /// ```
    pub fn cancel(&mut self) {
        if !self.is_expired() {
            self.status = DndStatus::Cancelled;
        }
    }

    /// Returns the current [`DndStatus`] of the session.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite_dnd::{DndSession, DndStatus};
    /// use std::sync::Arc;
    ///
    /// let session = DndSession::new(Arc::new(0_i32), vec![], None);
    /// assert_eq!(session.status(), DndStatus::Idle);
    /// ```
    pub fn status(&self) -> DndStatus {
        self.status
    }
}

/// Process-wide manager tracking all active [`DndSession`]s by [`SessionId`].
///
/// Sessions remain registered until explicitly purged via
/// [`DndSessionManager::purge_completed`], ensuring payloads stay alive even
/// after the originating window is dropped.
///
/// # Threading model
///
/// [`DndSessionManager`] is **not** [`Sync`] and is intended for
/// single-threaded use (typically owned by the UI thread that drives the
/// event loop). It stores [`DndSession`]s in a plain [`HashMap`] with no
/// internal locking, so concurrent access from multiple threads is a data
/// race. To share a manager across threads, wrap it in a
/// [`std::sync::Mutex`]:
///
/// ```
/// use martensite_dnd::DndSessionManager;
/// use std::sync::{Arc, Mutex};
///
/// let manager = Arc::new(Mutex::new(DndSessionManager::new()));
/// // Clone the Arc for worker threads; lock before each access.
/// let mut m = manager.lock().unwrap();
/// let _id = m.start_session(std::sync::Arc::new(0_i32), vec![], None);
/// ```
///
/// # Examples
///
/// ```
/// use martensite_dnd::{DndSessionManager, DropEffect};
/// use std::sync::Arc;
///
/// let mut manager = DndSessionManager::new();
/// let id = manager.start_session(Arc::new("payload"), vec!["text/plain".into()], None);
/// assert_eq!(manager.active_sessions(), 1);
/// assert!(manager.complete_session(id, DropEffect::Copy));
/// assert_eq!(manager.purge_completed(), 1);
/// assert_eq!(manager.active_sessions(), 0);
/// ```
pub struct DndSessionManager {
    sessions: HashMap<SessionId, DndSession>,
    next_id: u64,
}

impl Default for DndSessionManager {
    fn default() -> Self {
        Self::new()
    }
}

impl DndSessionManager {
    /// Creates a new, empty session manager.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite_dnd::DndSessionManager;
    ///
    /// let manager = DndSessionManager::new();
    /// assert_eq!(manager.active_sessions(), 0);
    /// ```
    pub fn new() -> Self {
        Self {
            sessions: HashMap::new(),
            next_id: 0,
        }
    }

    /// Starts a new drag session and returns its [`SessionId`].
    ///
    /// The returned [`SessionId`] is backed by a monotonically increasing
    /// `u64` counter internal to this manager. At the realistic rate of one
    /// billion sessions per second, the counter would take over 584 years to
    /// wrap, so [`SessionId`] wrap is not a practical concern for any real
    /// process lifetime and IDs are never reused within a single manager
    /// instance.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite_dnd::DndSessionManager;
    /// use std::sync::Arc;
    ///
    /// let mut manager = DndSessionManager::new();
    /// let id = manager.start_session(Arc::new(1_i32), vec![], None);
    /// assert!(manager.get_session(id).is_some());
    /// ```
    pub fn start_session(
        &mut self,
        payload: Arc<dyn Any + Send + Sync>,
        available_types: Vec<String>,
        source_window: Option<WindowId>,
    ) -> SessionId {
        let id = SessionId(self.next_id);
        self.next_id += 1;
        self.sessions
            .insert(id, DndSession::new(payload, available_types, source_window));
        id
    }

    /// Returns a shared reference to the session with the given `id`, if any.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite_dnd::DndSessionManager;
    /// use std::sync::Arc;
    ///
    /// let mut manager = DndSessionManager::new();
    /// let id = manager.start_session(Arc::new(1_i32), vec![], None);
    /// assert!(manager.get_session(id).is_some());
    /// assert!(manager.get_session((id.0 + 1).into()).is_none());
    /// ```
    pub fn get_session(&self, id: SessionId) -> Option<&DndSession> {
        self.sessions.get(&id)
    }

    /// Returns an exclusive reference to the session with the given `id`, if
    /// any.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite_dnd::{DndSessionManager, DropEffect};
    /// use std::sync::Arc;
    ///
    /// let mut manager = DndSessionManager::new();
    /// let id = manager.start_session(Arc::new(1_i32), vec![], None);
    /// if let Some(session) = manager.get_session_mut(id) {
    ///     session.complete(DropEffect::Copy);
    /// }
    /// ```
    pub fn get_session_mut(&mut self, id: SessionId) -> Option<&mut DndSession> {
        self.sessions.get_mut(&id)
    }

    /// Marks the session with the given `id` as completed with `effect`.
    ///
    /// Returns `true` if the session existed and was transitioned, `false`
    /// otherwise.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite_dnd::{DndSessionManager, DropEffect};
    /// use std::sync::Arc;
    ///
    /// let mut manager = DndSessionManager::new();
    /// let id = manager.start_session(Arc::new(1_i32), vec![], None);
    /// assert!(manager.complete_session(id, DropEffect::Copy));
    /// assert!(!manager.complete_session(id, DropEffect::Copy));
    /// ```
    pub fn complete_session(&mut self, id: SessionId, effect: DropEffect) -> bool {
        if let Some(session) = self.sessions.get_mut(&id) {
            if !session.is_expired() {
                session.complete(effect);
                return true;
            }
        }
        false
    }

    /// Marks the session with the given `id` as cancelled.
    ///
    /// Returns `true` if the session existed and was transitioned, `false`
    /// otherwise.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite_dnd::DndSessionManager;
    /// use std::sync::Arc;
    ///
    /// let mut manager = DndSessionManager::new();
    /// let id = manager.start_session(Arc::new(1_i32), vec![], None);
    /// assert!(manager.cancel_session(id));
    /// assert!(!manager.cancel_session(id));
    /// ```
    pub fn cancel_session(&mut self, id: SessionId) -> bool {
        if let Some(session) = self.sessions.get_mut(&id) {
            if !session.is_expired() {
                session.cancel();
                return true;
            }
        }
        false
    }

    /// Returns the number of currently registered sessions (including
    /// completed/cancelled ones not yet purged).
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite_dnd::DndSessionManager;
    /// use std::sync::Arc;
    ///
    /// let mut manager = DndSessionManager::new();
    /// manager.start_session(Arc::new(1_i32), vec![], None);
    /// manager.start_session(Arc::new(2_i32), vec![], None);
    /// assert_eq!(manager.active_sessions(), 2);
    /// ```
    pub fn active_sessions(&self) -> usize {
        self.sessions.len()
    }

    /// Removes all sessions in a terminal status and returns the count
    /// removed.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite_dnd::{DndSessionManager, DropEffect};
    /// use std::sync::Arc;
    ///
    /// let mut manager = DndSessionManager::new();
    /// let a = manager.start_session(Arc::new(1_i32), vec![], None);
    /// let b = manager.start_session(Arc::new(2_i32), vec![], None);
    /// manager.complete_session(a, DropEffect::Copy);
    /// assert_eq!(manager.purge_completed(), 1);
    /// assert_eq!(manager.active_sessions(), 1);
    /// ```
    pub fn purge_completed(&mut self) -> usize {
        let before = self.sessions.len();
        self.sessions.retain(|_, s| !s.is_expired());
        before - self.sessions.len()
    }
}

impl From<u64> for SessionId {
    fn from(value: u64) -> Self {
        SessionId(value)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn session_starts_idle() {
        let session = DndSession::new(Arc::new(42_i32), vec!["text/plain".into()], None);
        assert_eq!(session.status(), DndStatus::Idle);
        assert!(!session.is_expired());
    }

    #[test]
    fn payload_downcasts() {
        let session = DndSession::new(Arc::new(42_i32), vec![], None);
        assert_eq!(session.payload_typed::<i32>(), Some(&42));
        assert_eq!(session.payload_typed::<u64>(), None);
    }

    #[test]
    fn completion_marks_expired() {
        let mut session = DndSession::new(Arc::new(0_i32), vec![], None);
        session.complete(DropEffect::Copy);
        assert_eq!(session.status(), DndStatus::Completed(DropEffect::Copy));
        assert!(session.is_expired());
    }

    #[test]
    fn cancellation_marks_expired() {
        let mut session = DndSession::new(Arc::new(0_i32), vec![], None);
        session.cancel();
        assert_eq!(session.status(), DndStatus::Cancelled);
        assert!(session.is_expired());
    }

    #[test]
    fn complete_after_cancel_is_noop() {
        let mut session = DndSession::new(Arc::new(0_i32), vec![], None);
        session.cancel();
        session.complete(DropEffect::Copy);
        assert_eq!(session.status(), DndStatus::Cancelled);
    }

    #[test]
    fn cancel_after_complete_is_noop() {
        let mut session = DndSession::new(Arc::new(0_i32), vec![], None);
        session.complete(DropEffect::Move);
        session.cancel();
        assert_eq!(session.status(), DndStatus::Completed(DropEffect::Move));
    }

    #[test]
    fn manager_starts_empty() {
        let manager = DndSessionManager::new();
        assert_eq!(manager.active_sessions(), 0);
    }

    #[test]
    fn manager_start_and_get() {
        let mut manager = DndSessionManager::new();
        let id = manager.start_session(Arc::new("payload"), vec!["text/plain".into()], None);
        assert_eq!(manager.active_sessions(), 1);
        let session = manager.get_session(id).expect("session should exist");
        assert_eq!(session.available_types, vec!["text/plain"]);
    }

    #[test]
    fn manager_get_missing_returns_none() {
        let mut manager = DndSessionManager::new();
        let _id = manager.start_session(Arc::new(0_i32), vec![], None);
        assert!(manager.get_session(SessionId(999)).is_none());
    }

    #[test]
    fn manager_complete_session() {
        let mut manager = DndSessionManager::new();
        let id = manager.start_session(Arc::new(0_i32), vec![], None);
        assert!(manager.complete_session(id, DropEffect::Copy));
        assert_eq!(
            manager.get_session(id).unwrap().status(),
            DndStatus::Completed(DropEffect::Copy)
        );
    }

    #[test]
    fn manager_complete_missing_returns_false() {
        let mut manager = DndSessionManager::new();
        assert!(!manager.complete_session(SessionId(0), DropEffect::Copy));
    }

    #[test]
    fn manager_cancel_session() {
        let mut manager = DndSessionManager::new();
        let id = manager.start_session(Arc::new(0_i32), vec![], None);
        assert!(manager.cancel_session(id));
        assert_eq!(
            manager.get_session(id).unwrap().status(),
            DndStatus::Cancelled
        );
    }

    #[test]
    fn manager_purge_completed() {
        let mut manager = DndSessionManager::new();
        let a = manager.start_session(Arc::new(1_i32), vec![], None);
        let b = manager.start_session(Arc::new(2_i32), vec![], None);
        let _c = manager.start_session(Arc::new(3_i32), vec![], None);
        manager.complete_session(a, DropEffect::Copy);
        manager.cancel_session(b);
        assert_eq!(manager.active_sessions(), 3);
        assert_eq!(manager.purge_completed(), 2);
        assert_eq!(manager.active_sessions(), 1);
    }

    #[test]
    fn session_survives_after_source_window_dropped() {
        // WindowId is Copy, so we simulate "dropping the source window" by
        // simply discarding the id; the session must remain valid.
        let id = WindowId::from_raw(0);
        let session = DndSession::new(Arc::new(42_i32), vec![], Some(id));
        let _ = id;
        assert_eq!(session.payload_typed::<i32>(), Some(&42));
        assert!(!session.is_expired());
    }

    #[test]
    fn multiple_concurrent_sessions() {
        let mut manager = DndSessionManager::new();
        let a = manager.start_session(Arc::new(1_i32), vec![], None);
        let b = manager.start_session(Arc::new(2_i32), vec![], None);
        let c = manager.start_session(Arc::new(3_i32), vec![], None);
        assert_ne!(a, b);
        assert_ne!(b, c);
        assert_ne!(a, c);
        assert_eq!(manager.active_sessions(), 3);
        assert_eq!(
            manager.get_session(a).unwrap().payload_typed::<i32>(),
            Some(&1)
        );
        assert_eq!(
            manager.get_session(b).unwrap().payload_typed::<i32>(),
            Some(&2)
        );
        assert_eq!(
            manager.get_session(c).unwrap().payload_typed::<i32>(),
            Some(&3)
        );
    }

    #[test]
    fn session_ids_are_monotonic() {
        let mut manager = DndSessionManager::new();
        let a = manager.start_session(Arc::new(0_i32), vec![], None);
        let b = manager.start_session(Arc::new(0_i32), vec![], None);
        assert!(b.0 > a.0);
    }
}
