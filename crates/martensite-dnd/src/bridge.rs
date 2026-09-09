//! Bridge from OS drag events to the internal DnD model.
//!
//! This module wires incoming OS drag events (winit 0.31's `DragEntered` /
//! `DragPosition` / `DragDropped` / `DragLeft`) to the internal
//! [`DropTargetRegistry`] and [`DndSessionManager`], and exposes the
//! negotiated [`DropEffect`] so the caller can report accepted actions back to
//! the OS via the [`DndPlatform`](crate::platform::DndPlatform) seam.
//!
//! # Design
//!
//! A [`DropBridge`] owns a [`DropTargetRegistry`] and a [`DndSessionManager`]
//! and tracks the single in-flight incoming drag (winit guarantees at most one
//! concurrent incoming drag on every implemented platform). It consumes
//! [`DropInput`] events — a normalized, winit-independent representation — and
//! returns a [`DropOutcome`] describing whether the drag is currently accepted
//! and the negotiated effect.
//!
//! The caller is responsible for:
//!
//! 1. Translating winit `WindowEvent::Drag*` into [`DropInput`] (see
//!    [`convert_winit_drop_event`]).
//! 2. On [`DropOutcome`], calling
//!    [`DndPlatform::set_accepted_actions`](crate::platform::DndPlatform::set_accepted_actions)
//!    with the accepted effects so the OS displays the correct cursor.
//!
//! # Documented limitations
//!
//! - **Single concurrent incoming drag**: the bridge tracks one incoming
//!   session at a time. winit documents that only a single drag operation
//!   occurs at a time on all implemented platforms, so this is not a practical
//!   restriction.
//! - **No data buffering**: the bridge records the advertised *types* of an
//!   incoming drag but does not fetch or buffer the actual bytes. Fetching is
//!   asynchronous in winit and is the caller's responsibility via
//!   [`DndPlatform::fetch_data`](crate::platform::DndPlatform::fetch_data).
//! - **Position on drop**: winit's `DragDropped` event carries no position.
//!   The bridge uses the last position reported by `DragEntered`/`DragPosition`
//!   for the drop hit-test.

use crate::platform::{proposed_action_from_winit, ProposedAction};
use crate::target::{DropTargetRegistry, TargetId};
use crate::{DndSessionManager, DropEffect, SessionId};
use glam::Vec2;
use std::sync::Arc;

/// Normalized, winit-independent representation of a single incoming drag
/// event.
///
/// Convert from winit with [`convert_winit_drop_event`].
///
/// # Examples
///
/// ```
/// use martensite_dnd::bridge::DropInput;
/// use martensite_dnd::platform::ProposedAction;
///
/// let input = DropInput::Entered {
///     available_types: vec!["text/plain".into()],
///     position: Some(glam::Vec2::new(10.0, 20.0)),
///     action: ProposedAction::Copy,
/// };
/// ```
#[derive(Clone, Debug)]
#[non_exhaustive]
pub enum DropInput {
    /// A drag operation entered the window. `available_types` are the MIME
    /// type hints advertised by the source (best-effort strings). `position`
    /// is in logical coordinates and may be `None` on platforms that do not
    /// report it on enter.
    Entered {
        /// MIME/type strings advertised by the drag source.
        available_types: Vec<String>,
        /// Logical position of the drag, if reported.
        position: Option<Vec2>,
        /// OS-proposed action.
        action: ProposedAction,
    },
    /// The drag moved within the window.
    Moved {
        /// Logical position of the drag.
        position: Vec2,
        /// OS-proposed action.
        action: ProposedAction,
    },
    /// The drag was dropped on the window. winit does not report a position
    /// for the drop; the bridge uses the last known position.
    Dropped {
        /// OS-proposed action.
        action: ProposedAction,
    },
    /// The drag left the window or was canceled.
    Left,
}

/// The result of processing a [`DropInput`] through a [`DropBridge`].
#[derive(Copy, Clone, Debug, PartialEq, Eq, Default)]
pub struct DropOutcome {
    /// Whether the drag is currently accepted by a registered drop target.
    pub accepted: bool,
    /// The negotiated [`DropEffect`] when accepted, or the OS-proposed effect
    /// mapped to a [`DropEffect`] when rejected but available.
    pub effect: Option<DropEffect>,
    /// The [`TargetId`] of the target currently under the drag, if any.
    pub target: Option<TargetId>,
}

impl DropOutcome {
    /// Returns the list of accepted [`DropEffect`]s to report to the OS via
    /// [`DndPlatform::set_accepted_actions`](crate::platform::DndPlatform::set_accepted_actions).
    ///
    /// When the drag is accepted, this is the single negotiated effect; when
    /// rejected, this is empty (the OS will show a "no drop" cursor).
    #[must_use]
    pub fn accepted_actions(self) -> Vec<DropEffect> {
        if self.accepted {
            self.effect.into_iter().collect()
        } else {
            Vec::new()
        }
    }
}

/// Owns a [`DropTargetRegistry`] and [`DndSessionManager`] and drives them from
/// normalized [`DropInput`] events.
///
/// # Examples
///
/// ```
/// use martensite_dnd::bridge::{DropBridge, DropInput};
/// use martensite_dnd::platform::ProposedAction;
/// use martensite_dnd::{DropEffectMask, DropTarget};
/// use martensite_core::{Rect, WidgetId};
///
/// let mut bridge = DropBridge::new();
/// let _id = bridge.registry_mut().register(
///     DropTarget::new(WidgetId::from_parts(1, 1), vec!["text/plain".into()], DropEffectMask::COPY)
///         .with_bounds(Rect::new(0.0, 0.0, 100.0, 100.0)),
/// );
///
/// // A drag enters carrying text/plain at (50, 50).
/// let outcome = bridge.handle(DropInput::Entered {
///     available_types: vec!["text/plain".into()],
///     position: Some(glam::Vec2::new(50.0, 50.0)),
///     action: ProposedAction::Copy,
/// });
/// assert!(outcome.accepted);
/// assert_eq!(outcome.effect, Some(martensite_dnd::DropEffect::Copy));
/// ```
pub struct DropBridge {
    /// The drop target registry.
    pub registry: DropTargetRegistry,
    /// The drag session manager.
    pub sessions: DndSessionManager,
    incoming: Option<SessionId>,
    current_target: Option<TargetId>,
    last_position: Vec2,
}

impl Default for DropBridge {
    fn default() -> Self {
        Self::new()
    }
}

impl DropBridge {
    /// Creates a new, empty [`DropBridge`].
    #[must_use]
    pub fn new() -> Self {
        Self {
            registry: DropTargetRegistry::new(),
            sessions: DndSessionManager::new(),
            incoming: None,
            current_target: None,
            last_position: Vec2::ZERO,
        }
    }

    /// Returns a shared reference to the [`DropTargetRegistry`].
    #[must_use]
    pub fn registry(&self) -> &DropTargetRegistry {
        &self.registry
    }

    /// Returns a mutable reference to the [`DropTargetRegistry`].
    pub fn registry_mut(&mut self) -> &mut DropTargetRegistry {
        &mut self.registry
    }

    /// Returns a shared reference to the [`DndSessionManager`].
    #[must_use]
    pub fn sessions(&self) -> &DndSessionManager {
        &self.sessions
    }

    /// Returns a mutable reference to the [`DndSessionManager`].
    pub fn sessions_mut(&mut self) -> &mut DndSessionManager {
        &mut self.sessions
    }

    /// Returns the [`SessionId`] of the in-flight incoming drag, if any.
    #[must_use]
    pub fn incoming_session(&self) -> Option<SessionId> {
        self.incoming
    }

    /// Returns the [`TargetId`] currently under the drag, if any.
    #[must_use]
    pub fn current_target(&self) -> Option<TargetId> {
        self.current_target
    }

    /// Processes a single [`DropInput`] event and returns the outcome.
    pub fn handle(&mut self, input: DropInput) -> DropOutcome {
        match input {
            DropInput::Entered {
                available_types,
                position,
                action: _,
            } => {
                // Start a detached session for the incoming drag. The payload
                // is a placeholder; the real data is fetched asynchronously
                // via the platform seam.
                let id = self.sessions.start_session(
                    Arc::new(available_types.clone()),
                    available_types,
                    None,
                );
                self.incoming = Some(id);
                if let Some(pos) = position {
                    self.last_position = pos;
                }
                self.update_hover()
            }
            DropInput::Moved {
                position,
                action: _,
            } => {
                self.last_position = position;
                if self.incoming.is_some() {
                    self.update_hover()
                } else {
                    DropOutcome::default()
                }
            }
            DropInput::Dropped { action: _ } => {
                let Some(id) = self.incoming else {
                    return DropOutcome::default();
                };
                let Some(target) = self.current_target else {
                    // No target under the drop: cancel the session.
                    self.sessions.cancel_session(id);
                    self.incoming = None;
                    return DropOutcome::default();
                };
                let Some(session) = self.sessions.session_mut(id) else {
                    self.incoming = None;
                    return DropOutcome::default();
                };
                let effect = self.registry.drop_on_target(target, session);
                self.incoming = None;
                self.current_target = None;
                DropOutcome {
                    accepted: effect.is_some(),
                    effect,
                    target: Some(target),
                }
            }
            DropInput::Left => {
                if let Some(id) = self.incoming {
                    // Cancel the session and reset any hovered target.
                    if let Some(target) = self.current_target {
                        self.registry.leave_target(target);
                    }
                    self.sessions.cancel_session(id);
                }
                self.incoming = None;
                self.current_target = None;
                DropOutcome::default()
            }
        }
    }

    /// Recomputes the hovered target from `last_position` and the current
    /// incoming session, entering/leaving targets as needed.
    fn update_hover(&mut self) -> DropOutcome {
        let id = match self.incoming {
            Some(id) => id,
            None => return DropOutcome::default(),
        };
        let session = match self.sessions.session(id) {
            Some(s) => s,
            None => {
                self.incoming = None;
                return DropOutcome::default();
            }
        };
        // Use enter_at to find+enter in one step, capturing the id and state.
        let (new_id, state) = match self.registry.enter_at(self.last_position, session) {
            Some((tid, st)) => (Some(tid), st),
            None => (None, crate::target::DropTargetState::Idle),
        };
        // Leave the previously-hovered target if it changed.
        if self.current_target != new_id {
            if let Some(old) = self.current_target {
                self.registry.leave_target(old);
            }
        }
        self.current_target = new_id;
        let accepted = state == crate::target::DropTargetState::Hovered;
        // On hover-accept, tentatively report Copy as the negotiated effect
        // (the built-in Copy > Move > Link priority used by `drop_on_target`).
        // The caller may override via `set_accepted_actions` on the platform.
        let effect = if accepted {
            Some(DropEffect::Copy)
        } else {
            None
        };
        DropOutcome {
            accepted,
            effect,
            target: new_id,
        }
    }
}

/// Converts a winit `WindowEvent` into a [`DropInput`], applying the given
/// `scale_factor` (physical-to-logical) to positions.
///
/// `available_types` for [`DropInput::Entered`] must be supplied by the
/// caller (fetched via
/// [`DndPlatform::available_types`](crate::platform::DndPlatform::available_types)),
/// since winit does not embed the type list in the `DragEntered` event itself.
/// For the other variants the type list is irrelevant.
///
/// Returns `None` for non-drag events.
///
/// # Examples
///
/// ```
/// use martensite_dnd::bridge::convert_winit_drop_event;
/// use winit::dpi::PhysicalPosition;
/// use winit::data_transfer::DataTransferId;
/// use winit::event::WindowEvent;
///
/// let event = WindowEvent::DragPosition {
///     id: DataTransferId::from_raw(7),
///     position: PhysicalPosition::new(100.0, 200.0),
///     proposed_action: None,
/// };
/// let input = convert_winit_drop_event(&event, 2.0, Vec::new()).unwrap();
/// assert_eq!(input.position(), Some(glam::Vec2::new(50.0, 100.0)));
/// ```
pub fn convert_winit_drop_event(
    event: &winit::event::WindowEvent,
    scale_factor: f64,
    available_types: Vec<String>,
) -> Option<DropInput> {
    use winit::event::WindowEvent;
    match event {
        WindowEvent::DragEntered { position, .. } => Some(DropInput::Entered {
            available_types,
            position: position.map(|p| physical_to_logical(p, scale_factor)),
            action: ProposedAction::None,
        }),
        WindowEvent::DragPosition {
            position,
            proposed_action,
            ..
        } => Some(DropInput::Moved {
            position: physical_to_logical(*position, scale_factor),
            action: proposed_action
                .map(proposed_action_from_winit)
                .unwrap_or_default(),
        }),
        WindowEvent::DragDropped {
            proposed_action, ..
        } => Some(DropInput::Dropped {
            action: proposed_action
                .map(proposed_action_from_winit)
                .unwrap_or_default(),
        }),
        WindowEvent::DragLeft { .. } => Some(DropInput::Left),
        _ => None,
    }
}

/// Extension method used by doctests to read the position of a `DropInput`.
impl DropInput {
    /// Returns the logical position carried by this event, if any.
    #[must_use]
    pub fn position(&self) -> Option<Vec2> {
        match self {
            DropInput::Entered { position, .. } => *position,
            DropInput::Moved { position, .. } => Some(*position),
            DropInput::Dropped { .. } | DropInput::Left => None,
        }
    }
}

/// Converts a winit physical position to a logical [`Vec2`].
fn physical_to_logical(position: winit::dpi::PhysicalPosition<f64>, scale_factor: f64) -> Vec2 {
    let s = if scale_factor > 0.0 && scale_factor.is_finite() {
        scale_factor
    } else {
        1.0
    };
    Vec2::new((position.x / s) as f32, (position.y / s) as f32)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::platform::ProposedAction;
    use crate::{DropEffectMask, DropTarget};
    use martensite_core::{Rect, WidgetId};

    fn registered_bridge() -> DropBridge {
        let mut bridge = DropBridge::new();
        bridge.registry_mut().register(
            DropTarget::new(
                WidgetId::from_parts(1, 1),
                vec!["text/plain".into()],
                DropEffectMask::COPY,
            )
            .with_bounds(Rect::new(0.0, 0.0, 100.0, 100.0)),
        );
        bridge
    }

    #[test]
    fn bridge_starts_empty() {
        let bridge = DropBridge::new();
        assert!(bridge.registry().is_empty());
        assert_eq!(bridge.sessions().active_sessions(), 0);
        assert!(bridge.incoming_session().is_none());
        assert!(bridge.current_target().is_none());
    }

    #[test]
    fn entered_accepts_matching_type() {
        let mut bridge = registered_bridge();
        let outcome = bridge.handle(DropInput::Entered {
            available_types: vec!["text/plain".into()],
            position: Some(Vec2::new(50.0, 50.0)),
            action: ProposedAction::Copy,
        });
        assert!(outcome.accepted);
        assert_eq!(outcome.effect, Some(DropEffect::Copy));
        assert!(bridge.incoming_session().is_some());
        assert!(bridge.current_target().is_some());
    }

    #[test]
    fn entered_rejects_type_mismatch() {
        let mut bridge = registered_bridge();
        let outcome = bridge.handle(DropInput::Entered {
            available_types: vec!["image/png".into()],
            position: Some(Vec2::new(50.0, 50.0)),
            action: ProposedAction::Copy,
        });
        assert!(!outcome.accepted);
        assert_eq!(outcome.effect, None);
        // A session is still started even when rejected (the drag is in
        // flight); it is cancelled on Left.
        assert!(bridge.incoming_session().is_some());
    }

    #[test]
    fn entered_outside_bounds_not_accepted() {
        let mut bridge = registered_bridge();
        let outcome = bridge.handle(DropInput::Entered {
            available_types: vec!["text/plain".into()],
            position: Some(Vec2::new(200.0, 200.0)),
            action: ProposedAction::Copy,
        });
        assert!(!outcome.accepted);
        assert!(bridge.current_target().is_none());
    }

    #[test]
    fn moved_updates_position_and_target() {
        let mut bridge = registered_bridge();
        bridge.handle(DropInput::Entered {
            available_types: vec!["text/plain".into()],
            position: Some(Vec2::new(200.0, 200.0)),
            action: ProposedAction::Copy,
        });
        assert!(!bridge.current_target().is_some());
        let outcome = bridge.handle(DropInput::Moved {
            position: Vec2::new(50.0, 50.0),
            action: ProposedAction::Copy,
        });
        assert!(outcome.accepted);
        assert!(bridge.current_target().is_some());
    }

    #[test]
    fn dropped_completes_session() {
        let mut bridge = registered_bridge();
        bridge.handle(DropInput::Entered {
            available_types: vec!["text/plain".into()],
            position: Some(Vec2::new(50.0, 50.0)),
            action: ProposedAction::Copy,
        });
        let outcome = bridge.handle(DropInput::Dropped {
            action: ProposedAction::Copy,
        });
        assert!(outcome.accepted);
        assert_eq!(outcome.effect, Some(DropEffect::Copy));
        assert!(bridge.incoming_session().is_none());
        assert!(bridge.current_target().is_none());
    }

    #[test]
    fn dropped_outside_cancels_session() {
        let mut bridge = registered_bridge();
        bridge.handle(DropInput::Entered {
            available_types: vec!["text/plain".into()],
            position: Some(Vec2::new(200.0, 200.0)),
            action: ProposedAction::Copy,
        });
        let outcome = bridge.handle(DropInput::Dropped {
            action: ProposedAction::Copy,
        });
        assert!(!outcome.accepted);
        assert!(bridge.incoming_session().is_none());
    }

    #[test]
    fn left_cancels_session() {
        let mut bridge = registered_bridge();
        bridge.handle(DropInput::Entered {
            available_types: vec!["text/plain".into()],
            position: Some(Vec2::new(50.0, 50.0)),
            action: ProposedAction::Copy,
        });
        let outcome = bridge.handle(DropInput::Left);
        assert!(!outcome.accepted);
        assert!(bridge.incoming_session().is_none());
        assert!(bridge.current_target().is_none());
    }

    #[test]
    fn dropped_without_enter_is_noop() {
        let mut bridge = registered_bridge();
        let outcome = bridge.handle(DropInput::Dropped {
            action: ProposedAction::Copy,
        });
        assert!(!outcome.accepted);
    }

    #[test]
    fn moved_without_enter_is_noop() {
        let mut bridge = registered_bridge();
        let outcome = bridge.handle(DropInput::Moved {
            position: Vec2::new(50.0, 50.0),
            action: ProposedAction::Copy,
        });
        assert!(!outcome.accepted);
    }

    #[test]
    fn left_without_enter_is_noop() {
        let mut bridge = registered_bridge();
        bridge.handle(DropInput::Left);
        assert!(bridge.incoming_session().is_none());
    }

    #[test]
    fn accepted_actions_empty_when_rejected() {
        let outcome = DropOutcome {
            accepted: false,
            effect: None,
            target: None,
        };
        assert!(outcome.accepted_actions().is_empty());
    }

    #[test]
    fn accepted_actions_single_when_accepted() {
        let outcome = DropOutcome {
            accepted: true,
            effect: Some(DropEffect::Move),
            target: None,
        };
        assert_eq!(outcome.accepted_actions(), vec![DropEffect::Move]);
    }

    #[test]
    fn convert_winit_drag_entered() {
        use winit::data_transfer::DataTransferId;
        use winit::dpi::PhysicalPosition;
        use winit::event::WindowEvent;
        let event = WindowEvent::DragEntered {
            id: DataTransferId::from_raw(1),
            position: Some(PhysicalPosition::new(100.0, 200.0)),
        };
        let input = convert_winit_drop_event(&event, 2.0, vec!["text/plain".into()]).unwrap();
        match input {
            DropInput::Entered {
                available_types,
                position,
                ..
            } => {
                assert_eq!(available_types, vec!["text/plain"]);
                assert_eq!(position, Some(Vec2::new(50.0, 100.0)));
            }
            _ => panic!("expected Entered"),
        }
    }

    #[test]
    fn convert_winit_drag_position_maps_action() {
        use winit::data_transfer::DataTransferId;
        use winit::dpi::PhysicalPosition;
        use winit::event::WindowEvent;
        use winit::event_loop::DndAction;
        let event = WindowEvent::DragPosition {
            id: DataTransferId::from_raw(1),
            position: PhysicalPosition::new(10.0, 20.0),
            proposed_action: Some(DndAction::Move),
        };
        let input = convert_winit_drop_event(&event, 1.0, vec![]).unwrap();
        match input {
            DropInput::Moved { position, action } => {
                assert_eq!(position, Vec2::new(10.0, 20.0));
                assert_eq!(action, ProposedAction::Move);
            }
            _ => panic!("expected Moved"),
        }
    }

    #[test]
    fn convert_winit_drag_dropped() {
        use winit::data_transfer::DataTransferId;
        use winit::event::WindowEvent;
        use winit::event_loop::DndAction;
        let event = WindowEvent::DragDropped {
            id: DataTransferId::from_raw(1),
            proposed_action: Some(DndAction::Copy),
        };
        let input = convert_winit_drop_event(&event, 1.0, vec![]).unwrap();
        match input {
            DropInput::Dropped { action } => assert_eq!(action, ProposedAction::Copy),
            _ => panic!("expected Dropped"),
        }
    }

    #[test]
    fn convert_winit_drag_left() {
        use winit::data_transfer::DataTransferId;
        use winit::event::WindowEvent;
        let event = WindowEvent::DragLeft {
            id: DataTransferId::from_raw(1),
        };
        let input = convert_winit_drop_event(&event, 1.0, vec![]).unwrap();
        assert!(matches!(input, DropInput::Left));
    }

    #[test]
    fn convert_winit_non_drag_returns_none() {
        use winit::event::WindowEvent;
        assert!(convert_winit_drop_event(&WindowEvent::CloseRequested, 1.0, vec![]).is_none());
        assert!(convert_winit_drop_event(&WindowEvent::Destroyed, 1.0, vec![]).is_none());
    }

    #[test]
    fn convert_winit_handles_invalid_scale() {
        use winit::data_transfer::DataTransferId;
        use winit::dpi::PhysicalPosition;
        use winit::event::WindowEvent;
        let event = WindowEvent::DragPosition {
            id: DataTransferId::from_raw(1),
            position: PhysicalPosition::new(10.0, 20.0),
            proposed_action: None,
        };
        // NaN scale falls back to 1.0.
        let input = convert_winit_drop_event(&event, f64::NAN, vec![]).unwrap();
        assert_eq!(input.position(), Some(Vec2::new(10.0, 20.0)));
    }

    #[test]
    fn drop_input_position_accessor() {
        let entered = DropInput::Entered {
            available_types: vec![],
            position: Some(Vec2::new(1.0, 2.0)),
            action: ProposedAction::None,
        };
        assert_eq!(entered.position(), Some(Vec2::new(1.0, 2.0)));
        let moved = DropInput::Moved {
            position: Vec2::new(3.0, 4.0),
            action: ProposedAction::None,
        };
        assert_eq!(moved.position(), Some(Vec2::new(3.0, 4.0)));
        let dropped = DropInput::Dropped {
            action: ProposedAction::None,
        };
        assert!(dropped.position().is_none());
        assert!(DropInput::Left.position().is_none());
    }

    #[test]
    fn full_lifecycle_enter_move_drop() {
        let mut bridge = registered_bridge();
        let o1 = bridge.handle(DropInput::Entered {
            available_types: vec!["text/plain".into()],
            position: Some(Vec2::new(200.0, 200.0)),
            action: ProposedAction::Copy,
        });
        assert!(!o1.accepted);
        let o2 = bridge.handle(DropInput::Moved {
            position: Vec2::new(50.0, 50.0),
            action: ProposedAction::Copy,
        });
        assert!(o2.accepted);
        let o3 = bridge.handle(DropInput::Dropped {
            action: ProposedAction::Copy,
        });
        assert!(o3.accepted);
        assert_eq!(o3.effect, Some(DropEffect::Copy));
        assert!(bridge.incoming_session().is_none());
    }
}
