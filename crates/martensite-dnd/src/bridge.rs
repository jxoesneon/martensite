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
//!
//! # Widget delivery
//!
//! [`DropBridge::handle`] only updates registry and session state. To deliver
//! the drop lifecycle to widgets, use [`DropBridge::handle_with`] together with
//! a [`DropSink`] and complete drops with [`DropBridge::deliver_payload`]. The
//! widget-facing events are the same `martensite_core::WidgetEvent` drop
//! variants (`DropHover`, `DropHoverLeave`, `Dropped`) that
//! `martensite-window`'s `EventRouter` delivers through arena hit-testing, so
//! a widget handles drops identically regardless of which routing strategy
//! the application uses.

use crate::platform::{proposed_action_from_winit, ProposedAction};
use crate::target::{DropTargetRegistry, DropTargetState, TargetId};
use crate::{DndSessionManager, DropEffect, SessionId};
use glam::Vec2;
use martensite_core::{DropPayload, EventResponse, WidgetArena, WidgetEvent, WidgetId};
use std::sync::Arc;

/// Receives widget-facing drop events produced by
/// [`DropBridge::handle_with`] and [`DropBridge::deliver_payload`].
///
/// A sink decides how an event reaches the widget identified by the
/// registered [`DropTarget::widget_id`](crate::DropTarget::widget_id). The
/// crate provides three implementations:
///
/// - [`WidgetArena`] — dispatches through
///   [`WidgetArena::dispatch_event`], which installs the ambient
///   internationalization context and bubbles unhandled events to ancestors.
/// - Any `FnMut(WidgetId, &WidgetEvent) -> EventResponse` closure — useful for
///   custom routing, logging, or tests.
/// - [`NoSink`] — delivers nothing.
///
/// # Examples
///
/// ```
/// use martensite_core::{EventResponse, WidgetEvent, WidgetId};
/// use martensite_dnd::bridge::DropSink;
///
/// let mut seen = Vec::new();
/// let mut sink = |id: WidgetId, event: &WidgetEvent| {
///     seen.push((id, event.clone()));
///     EventResponse::Handled
/// };
/// let id = WidgetId::from_parts(1, 1);
/// assert_eq!(
///     sink.deliver(id, &WidgetEvent::DropHoverLeave),
///     EventResponse::Handled
/// );
/// assert_eq!(seen.len(), 1);
/// ```
pub trait DropSink {
    /// Delivers `event` to the widget `target` and returns its response.
    fn deliver(&mut self, target: WidgetId, event: &WidgetEvent) -> EventResponse;
}

impl DropSink for WidgetArena {
    fn deliver(&mut self, target: WidgetId, event: &WidgetEvent) -> EventResponse {
        self.dispatch_event(target, event)
    }
}

impl<F> DropSink for F
where
    F: FnMut(WidgetId, &WidgetEvent) -> EventResponse,
{
    fn deliver(&mut self, target: WidgetId, event: &WidgetEvent) -> EventResponse {
        self(target, event)
    }
}

/// A [`DropSink`] that delivers nothing and always returns
/// [`EventResponse::Ignored`].
///
/// [`DropBridge::handle`] uses this sink, so it only updates registry and
/// session state.
///
/// # Examples
///
/// ```
/// use martensite_core::{EventResponse, WidgetEvent, WidgetId};
/// use martensite_dnd::bridge::{DropSink, NoSink};
///
/// let response = NoSink.deliver(WidgetId::from_parts(1, 1), &WidgetEvent::DropHoverLeave);
/// assert_eq!(response, EventResponse::Ignored);
/// ```
#[derive(Copy, Clone, Debug, Default, PartialEq, Eq)]
pub struct NoSink;

impl DropSink for NoSink {
    fn deliver(&mut self, _target: WidgetId, _event: &WidgetEvent) -> EventResponse {
        EventResponse::Ignored
    }
}

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
    /// Whether `last_position` was reported for the in-flight drag.
    position_known: bool,
    /// The accepted target (and its widget) that last received `DropHover`.
    hovered: Option<(TargetId, WidgetId)>,
    /// Widget and position awaiting the asynchronous drop payload.
    pending_drop: Option<(WidgetId, Option<Vec2>)>,
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
            position_known: false,
            hovered: None,
            pending_drop: None,
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

    /// Returns the widget awaiting the payload of an accepted drop, if any.
    ///
    /// Set when [`handle_with`](Self::handle_with) (or [`handle`](Self::handle))
    /// processes an accepted [`DropInput::Dropped`]; cleared by
    /// [`deliver_payload`](Self::deliver_payload).
    #[must_use]
    pub fn pending_drop_target(&self) -> Option<WidgetId> {
        self.pending_drop.map(|(widget, _)| widget)
    }

    /// Processes a single [`DropInput`] event and returns the outcome.
    ///
    /// This updates registry and session state only; no widget receives an
    /// event. It is equivalent to [`handle_with`](Self::handle_with) with a
    /// [`NoSink`].
    pub fn handle(&mut self, input: DropInput) -> DropOutcome {
        self.handle_with(input, &mut NoSink)
    }

    /// Processes a single [`DropInput`] event, delivers the resulting
    /// widget-facing drop events to `sink`, and returns the outcome.
    ///
    /// Negotiation and state transitions are identical to
    /// [`handle`](Self::handle). Events are delivered to the
    /// [`DropTarget::widget_id`](crate::DropTarget::widget_id) of the
    /// registered target under the drag:
    ///
    /// - `Entered`/`Moved` over a target that accepts the drag (state
    ///   [`DropTargetState::Hovered`]) — [`WidgetEvent::DropHover`] with the
    ///   current position. When the accepted target changes, the previous
    ///   target's widget first receives [`WidgetEvent::DropHoverLeave`].
    ///   Targets that reject the drag (MIME type or effect mismatch) receive
    ///   **no** hover events, so a widget only paints a drop affordance when
    ///   the drop would succeed.
    /// - `Moved` off every accepting target, or `Left` — `DropHoverLeave` to
    ///   the previously hovered widget.
    /// - `Dropped` accepted by the target — nothing is delivered yet; the
    ///   widget and last position are recorded as the pending drop. Fetch the
    ///   transfer data and call [`deliver_payload`](Self::deliver_payload)
    ///   when it arrives to deliver [`WidgetEvent::Dropped`].
    /// - `Dropped` rejected or over no target — `DropHoverLeave` to the
    ///   hovered widget, if any, and any pending drop is discarded.
    ///
    /// # Coordinate space
    ///
    /// Drag positions and registered [`DropTarget`](crate::DropTarget)
    /// bounds must share one coordinate space; the same position is
    /// forwarded in `DropHover`/`Dropped`. Arena-based applications that lay
    /// out in physical pixels (as the stock runner does) should register
    /// physical-pixel bounds and pass a `scale_factor` of `1.0` to
    /// [`convert_winit_drop_event`].
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite_core::{DropPayload, EventResponse, Rect, WidgetEvent, WidgetId};
    /// use martensite_dnd::bridge::{DropBridge, DropInput};
    /// use martensite_dnd::platform::ProposedAction;
    /// use martensite_dnd::{DropEffectMask, DropTarget};
    ///
    /// let widget = WidgetId::from_parts(1, 1);
    /// let mut bridge = DropBridge::new();
    /// bridge.registry_mut().register(
    ///     DropTarget::new(widget, vec!["text/plain".into()], DropEffectMask::COPY)
    ///         .with_bounds(Rect::new(0.0, 0.0, 100.0, 100.0)),
    /// );
    ///
    /// let mut log = Vec::new();
    /// let mut sink = |id: WidgetId, event: &WidgetEvent| {
    ///     log.push((id, event.clone()));
    ///     EventResponse::Handled
    /// };
    /// bridge.handle_with(
    ///     DropInput::Entered {
    ///         available_types: vec!["text/plain".into()],
    ///         position: Some(glam::Vec2::new(10.0, 10.0)),
    ///         action: ProposedAction::Copy,
    ///     },
    ///     &mut sink,
    /// );
    /// let outcome = bridge.handle_with(
    ///     DropInput::Dropped { action: ProposedAction::Copy },
    ///     &mut sink,
    /// );
    /// assert!(outcome.accepted);
    /// assert_eq!(bridge.pending_drop_target(), Some(widget));
    ///
    /// // The payload arrives asynchronously.
    /// bridge.deliver_payload(DropPayload::Text("hi".into()), &mut sink);
    /// assert!(matches!(log[0].1, WidgetEvent::DropHover { .. }));
    /// assert!(matches!(log[1].1, WidgetEvent::Dropped { .. }));
    /// ```
    pub fn handle_with(&mut self, input: DropInput, sink: &mut dyn DropSink) -> DropOutcome {
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
                self.position_known = position.is_some();
                if let Some(pos) = position {
                    self.last_position = pos;
                }
                self.update_hover(sink)
            }
            DropInput::Moved {
                position,
                action: _,
            } => {
                self.last_position = position;
                self.position_known = true;
                if self.incoming.is_some() {
                    self.update_hover(sink)
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
                    self.reject_drop(sink);
                    return DropOutcome::default();
                };
                let Some(session) = self.sessions.session_mut(id) else {
                    self.incoming = None;
                    self.reject_drop(sink);
                    return DropOutcome::default();
                };
                let effect = self.registry.drop_on_target(target, session);
                self.incoming = None;
                self.current_target = None;
                let widget = self.registry.get(target).map(|t| t.widget_id);
                match (effect, widget) {
                    (Some(_), Some(widget)) => {
                        // The drop ends the hover; `Dropped` follows with the
                        // payload, so no `DropHoverLeave` is sent (matching
                        // the arena router).
                        self.hovered = None;
                        let position = self.position_known.then_some(self.last_position);
                        self.pending_drop = Some((widget, position));
                    }
                    _ => self.reject_drop(sink),
                }
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
                // A drag left/cancelled after an accepted `Dropped` must
                // not leave the payload deliverable — `deliver_payload`
                // would otherwise still hand it to the target's widget.
                self.pending_drop = None;
                if let Some((_, widget)) = self.hovered.take() {
                    sink.deliver(widget, &WidgetEvent::DropHoverLeave);
                }
                DropOutcome::default()
            }
        }
    }

    /// Delivers [`WidgetEvent::Dropped`] carrying `payload` to the pending
    /// drop target recorded by an accepted [`DropInput::Dropped`], and clears
    /// the pending drop.
    ///
    /// Returns the widget's [`EventResponse`], or `None` when no drop is
    /// pending.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite_core::DropPayload;
    /// use martensite_dnd::bridge::{DropBridge, NoSink};
    ///
    /// let mut bridge = DropBridge::new();
    /// assert!(bridge
    ///     .deliver_payload(DropPayload::Text("x".into()), &mut NoSink)
    ///     .is_none());
    /// ```
    pub fn deliver_payload(
        &mut self,
        payload: DropPayload,
        sink: &mut dyn DropSink,
    ) -> Option<EventResponse> {
        let (widget, position) = self.pending_drop.take()?;
        Some(sink.deliver(widget, &WidgetEvent::Dropped { position, payload }))
    }

    /// Ends hover delivery for a drop that was rejected or landed on no
    /// target.
    fn reject_drop(&mut self, sink: &mut dyn DropSink) {
        self.pending_drop = None;
        if let Some((_, widget)) = self.hovered.take() {
            sink.deliver(widget, &WidgetEvent::DropHoverLeave);
        }
    }

    /// Recomputes the hovered target and delivers hover transitions to
    /// `sink`.
    fn update_hover(&mut self, sink: &mut dyn DropSink) -> DropOutcome {
        // No position has been reported for the in-flight drag (e.g. an
        // `Entered` that carries `position: None`): hit-testing against
        // `last_position` would use its `Vec2::ZERO` default or the
        // previous drag's endpoint, fabricating a hover — and an
        // `accepted` outcome — for a target the drag never visited.
        if !self.position_known {
            return DropOutcome::default();
        }
        let outcome = self.update_hover_state();
        let new_hovered = if outcome.accepted {
            outcome
                .target
                .and_then(|tid| self.registry.get(tid).map(|target| (tid, target.widget_id)))
        } else {
            None
        };
        if self.hovered.map(|(tid, _)| tid) != new_hovered.map(|(tid, _)| tid) {
            if let Some((_, old)) = self.hovered {
                sink.deliver(old, &WidgetEvent::DropHoverLeave);
            }
        }
        self.hovered = new_hovered;
        if let Some((_, widget)) = new_hovered {
            sink.deliver(
                widget,
                &WidgetEvent::DropHover {
                    position: self.last_position,
                },
            );
        }
        outcome
    }

    /// Recomputes the hovered target from `last_position` and the current
    /// incoming session, entering/leaving targets as needed.
    fn update_hover_state(&mut self) -> DropOutcome {
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
            None => (None, DropTargetState::Idle),
        };
        // Leave the previously-hovered target if it changed.
        if self.current_target != new_id {
            if let Some(old) = self.current_target {
                self.registry.leave_target(old);
            }
        }
        self.current_target = new_id;
        let accepted = state == DropTargetState::Hovered;
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
/// # Coordinate space
///
/// The converted positions must be in the same coordinate space as the
/// bounds registered on the [`DropBridge`]'s targets. A `scale_factor` of the
/// window's DPI scale yields logical positions (pair with logical bounds);
/// arena-based applications that lay out in physical pixels (as the stock
/// runner does) should register physical-pixel bounds and pass `1.0`.
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
        assert!(bridge.current_target().is_none());
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

    // ---- Widget delivery (`handle_with` / `deliver_payload`) ----

    const W1: WidgetId = WidgetId::from_parts(1, 1);
    const W2: WidgetId = WidgetId::from_parts(2, 1);

    /// Two side-by-side `text/plain` targets: W1 at x 0..100, W2 at
    /// x 100..200 (both y 0..100).
    fn two_target_bridge() -> DropBridge {
        let mut bridge = registered_bridge();
        bridge.registry_mut().register(
            DropTarget::new(W2, vec!["text/plain".into()], DropEffectMask::COPY)
                .with_bounds(Rect::new(100.0, 0.0, 100.0, 100.0)),
        );
        bridge
    }

    type Log = Vec<(WidgetId, WidgetEvent)>;

    fn recorder(log: &mut Log) -> impl FnMut(WidgetId, &WidgetEvent) -> EventResponse + '_ {
        move |id, event| {
            log.push((id, event.clone()));
            EventResponse::Handled
        }
    }

    fn entered(x: f32, y: f32) -> DropInput {
        DropInput::Entered {
            available_types: vec!["text/plain".into()],
            position: Some(Vec2::new(x, y)),
            action: ProposedAction::Copy,
        }
    }

    fn moved(x: f32, y: f32) -> DropInput {
        DropInput::Moved {
            position: Vec2::new(x, y),
            action: ProposedAction::Copy,
        }
    }

    fn dropped() -> DropInput {
        DropInput::Dropped {
            action: ProposedAction::Copy,
        }
    }

    fn hover(x: f32, y: f32) -> WidgetEvent {
        WidgetEvent::DropHover {
            position: Vec2::new(x, y),
        }
    }

    #[test]
    fn handle_with_enter_over_target_delivers_hover() {
        let mut bridge = registered_bridge();
        let mut log = Log::new();
        let outcome = bridge.handle_with(entered(10.0, 20.0), &mut recorder(&mut log));
        assert!(outcome.accepted);
        assert_eq!(log, vec![(W1, hover(10.0, 20.0))]);
    }

    #[test]
    fn handle_with_move_within_target_delivers_hover_with_new_position() {
        let mut bridge = registered_bridge();
        let mut log = Log::new();
        let mut sink = recorder(&mut log);
        bridge.handle_with(entered(10.0, 10.0), &mut sink);
        bridge.handle_with(moved(30.0, 40.0), &mut sink);
        drop(sink);
        assert_eq!(log, vec![(W1, hover(10.0, 10.0)), (W1, hover(30.0, 40.0))]);
    }

    #[test]
    fn handle_with_move_to_second_target_leaves_first() {
        let mut bridge = two_target_bridge();
        let mut log = Log::new();
        let mut sink = recorder(&mut log);
        bridge.handle_with(entered(10.0, 10.0), &mut sink);
        bridge.handle_with(moved(150.0, 10.0), &mut sink);
        drop(sink);
        assert_eq!(
            log,
            vec![
                (W1, hover(10.0, 10.0)),
                (W1, WidgetEvent::DropHoverLeave),
                (W2, hover(150.0, 10.0)),
            ]
        );
    }

    #[test]
    fn handle_with_move_off_targets_delivers_leave() {
        let mut bridge = registered_bridge();
        let mut log = Log::new();
        let mut sink = recorder(&mut log);
        bridge.handle_with(entered(10.0, 10.0), &mut sink);
        bridge.handle_with(moved(500.0, 500.0), &mut sink);
        // Further moves outside deliver nothing more.
        bridge.handle_with(moved(600.0, 600.0), &mut sink);
        drop(sink);
        assert_eq!(
            log,
            vec![(W1, hover(10.0, 10.0)), (W1, WidgetEvent::DropHoverLeave)]
        );
    }

    #[test]
    fn handle_with_left_delivers_leave() {
        let mut bridge = registered_bridge();
        let mut log = Log::new();
        let mut sink = recorder(&mut log);
        bridge.handle_with(entered(10.0, 10.0), &mut sink);
        bridge.handle_with(DropInput::Left, &mut sink);
        // A second Left has nothing to leave.
        bridge.handle_with(DropInput::Left, &mut sink);
        drop(sink);
        assert_eq!(
            log,
            vec![(W1, hover(10.0, 10.0)), (W1, WidgetEvent::DropHoverLeave)]
        );
        assert!(bridge.pending_drop_target().is_none());
    }

    #[test]
    fn handle_with_accepted_drop_then_payload_delivers_dropped() {
        let mut bridge = two_target_bridge();
        let mut log = Log::new();
        let mut sink = recorder(&mut log);
        bridge.handle_with(entered(10.0, 10.0), &mut sink);
        bridge.handle_with(moved(150.0, 20.0), &mut sink);
        let outcome = bridge.handle_with(dropped(), &mut sink);
        assert!(outcome.accepted);
        assert_eq!(bridge.pending_drop_target(), Some(W2));
        let response = bridge.deliver_payload(DropPayload::Text("hello".into()), &mut sink);
        assert_eq!(response, Some(EventResponse::Handled));
        assert!(bridge.pending_drop_target().is_none());
        // The pending drop is consumed exactly once.
        assert!(bridge
            .deliver_payload(DropPayload::Text("again".into()), &mut sink)
            .is_none());
        drop(sink);
        assert_eq!(
            log,
            vec![
                (W1, hover(10.0, 10.0)),
                (W1, WidgetEvent::DropHoverLeave),
                (W2, hover(150.0, 20.0)),
                (
                    W2,
                    WidgetEvent::Dropped {
                        position: Some(Vec2::new(150.0, 20.0)),
                        payload: DropPayload::Text("hello".into()),
                    }
                ),
            ]
        );
    }

    #[test]
    fn handle_with_positionless_enter_fabricates_no_hover() {
        let mut bridge = registered_bridge();
        // The target's bounds include the origin, which `last_position`
        // defaults to: a positionless `Entered` must not be accepted by
        // hit-testing that stale point.
        let mut log = Log::new();
        let mut sink = recorder(&mut log);
        let outcome = bridge.handle_with(
            DropInput::Entered {
                available_types: vec!["text/plain".into()],
                position: None,
                action: ProposedAction::Copy,
            },
            &mut sink,
        );
        assert!(!outcome.accepted);
        assert_eq!(outcome.target, None);
        assert!(bridge.current_target().is_none());
        drop(sink);
        assert!(log.is_empty(), "no hover may be fabricated: {log:?}");
    }

    #[test]
    fn handle_with_moved_after_positionless_enter_delivers_hover() {
        let mut bridge = registered_bridge();
        let mut log = Log::new();
        let mut sink = recorder(&mut log);
        bridge.handle_with(
            DropInput::Entered {
                available_types: vec!["text/plain".into()],
                position: None,
                action: ProposedAction::Copy,
            },
            &mut sink,
        );
        // The first `Moved` reports the position, re-arming hover
        // delivery for the rest of the drag.
        let outcome = bridge.handle_with(moved(10.0, 10.0), &mut sink);
        assert!(outcome.accepted);
        assert!(bridge.current_target().is_some());
        drop(sink);
        assert_eq!(log, vec![(W1, hover(10.0, 10.0))]);
    }

    #[test]
    fn handle_with_drop_without_reported_position_is_rejected() {
        let mut bridge = registered_bridge();
        // Bounds include the origin, which `last_position` defaults to:
        // a drop that never reported a position lands on no target.
        let mut log = Log::new();
        let mut sink = recorder(&mut log);
        bridge.handle_with(
            DropInput::Entered {
                available_types: vec!["text/plain".into()],
                position: None,
                action: ProposedAction::Copy,
            },
            &mut sink,
        );
        let outcome = bridge.handle_with(dropped(), &mut sink);
        assert!(!outcome.accepted);
        assert!(bridge.pending_drop_target().is_none());
        assert!(bridge
            .deliver_payload(DropPayload::Bytes(vec![1]), &mut sink)
            .is_none());
        drop(sink);
        assert!(log.is_empty());
    }

    #[test]
    fn handle_with_left_after_accepted_drop_clears_pending() {
        let mut bridge = registered_bridge();
        let mut log = Log::new();
        let mut sink = recorder(&mut log);
        bridge.handle_with(entered(10.0, 10.0), &mut sink);
        let outcome = bridge.handle_with(dropped(), &mut sink);
        assert!(outcome.accepted);
        assert_eq!(bridge.pending_drop_target(), Some(W1));
        // A `DragLeft` arriving after `DragDropped` cancels the pending
        // payload rather than letting a later `deliver_payload` land a
        // drop the user already cancelled.
        bridge.handle_with(DropInput::Left, &mut sink);
        assert!(bridge.pending_drop_target().is_none());
        assert!(bridge
            .deliver_payload(DropPayload::Text("x".into()), &mut sink)
            .is_none());
        drop(sink);
        assert_eq!(log, vec![(W1, hover(10.0, 10.0))]);
    }

    #[test]
    fn handle_with_rejected_type_delivers_no_hover() {
        let mut bridge = registered_bridge();
        let mut log = Log::new();
        let mut sink = recorder(&mut log);
        let outcome = bridge.handle_with(
            DropInput::Entered {
                available_types: vec!["image/png".into()],
                position: Some(Vec2::new(10.0, 10.0)),
                action: ProposedAction::Copy,
            },
            &mut sink,
        );
        assert!(!outcome.accepted);
        bridge.handle_with(moved(20.0, 20.0), &mut sink);
        let outcome = bridge.handle_with(dropped(), &mut sink);
        assert!(!outcome.accepted);
        assert!(bridge.pending_drop_target().is_none());
        assert!(bridge
            .deliver_payload(DropPayload::Text("x".into()), &mut sink)
            .is_none());
        drop(sink);
        assert!(
            log.is_empty(),
            "rejected target must get no events: {log:?}"
        );
    }

    #[test]
    fn handle_with_drop_without_target_delivers_nothing() {
        let mut bridge = registered_bridge();
        let mut log = Log::new();
        let mut sink = recorder(&mut log);
        bridge.handle_with(entered(500.0, 500.0), &mut sink);
        let outcome = bridge.handle_with(dropped(), &mut sink);
        assert!(!outcome.accepted);
        assert!(bridge.pending_drop_target().is_none());
        assert!(bridge
            .deliver_payload(DropPayload::Text("x".into()), &mut sink)
            .is_none());
        drop(sink);
        assert!(log.is_empty());
    }

    #[test]
    fn handle_with_rejected_drop_on_hovered_target_delivers_leave() {
        let mut bridge = registered_bridge();
        let mut log = Log::new();
        let mut sink = recorder(&mut log);
        bridge.handle_with(entered(10.0, 10.0), &mut sink);
        // The target vanishes before the drop lands: the drop is rejected
        // and the hovered widget's affordance must be cleared.
        let tid = bridge.current_target().unwrap();
        bridge.registry_mut().unregister(tid);
        let outcome = bridge.handle_with(dropped(), &mut sink);
        assert!(!outcome.accepted);
        assert!(bridge.pending_drop_target().is_none());
        drop(sink);
        assert_eq!(
            log,
            vec![(W1, hover(10.0, 10.0)), (W1, WidgetEvent::DropHoverLeave)]
        );
    }

    #[test]
    fn handle_records_pending_drop_without_delivering() {
        let mut bridge = registered_bridge();
        bridge.handle(entered(10.0, 10.0));
        assert!(bridge.handle(dropped()).accepted);
        assert_eq!(bridge.pending_drop_target(), Some(W1));
        let mut log = Log::new();
        bridge.deliver_payload(DropPayload::Text("x".into()), &mut recorder(&mut log));
        assert_eq!(log.len(), 1);
        assert!(matches!(log[0].1, WidgetEvent::Dropped { .. }));
    }

    #[test]
    fn no_sink_ignores_events() {
        assert_eq!(
            NoSink.deliver(W1, &WidgetEvent::DropHoverLeave),
            EventResponse::Ignored
        );
    }

    /// A widget recording the drop-phase events it receives.
    struct DropLog {
        log: Arc<std::sync::Mutex<Vec<WidgetEvent>>>,
    }

    impl martensite_core::Widget for DropLog {
        fn measure(
            &mut self,
            _cx: &mut martensite_core::LayoutContext,
            _constraints: martensite_core::LayoutConstraints,
        ) -> Vec2 {
            Vec2::ZERO
        }
        fn layout(&mut self, _cx: &mut martensite_core::LayoutContext, _bounds: Rect) {}
        fn event(&mut self, cx: &mut martensite_core::EventContext) -> EventResponse {
            self.log.lock().unwrap().push(cx.event.clone());
            EventResponse::Handled
        }
    }

    #[test]
    fn widget_arena_sink_delivers_dropped_end_to_end() {
        let log = Arc::new(std::sync::Mutex::new(Vec::new()));
        let mut arena = WidgetArena::new();
        let widget = arena.insert_with_widget(
            martensite_core::HotNode {
                bounds: Rect::new(0.0, 0.0, 50.0, 50.0),
                flags: martensite_core::NodeFlags::VISIBLE,
                ..martensite_core::HotNode::default()
            },
            Box::new(DropLog {
                log: Arc::clone(&log),
            }),
        );
        let mut bridge = DropBridge::new();
        bridge.registry_mut().register(
            DropTarget::new(widget, vec!["text/uri-list".into()], DropEffectMask::COPY)
                .with_bounds(Rect::new(0.0, 0.0, 50.0, 50.0)),
        );
        bridge.handle_with(
            DropInput::Entered {
                available_types: vec!["text/uri-list".into()],
                position: Some(Vec2::new(5.0, 5.0)),
                action: ProposedAction::Copy,
            },
            &mut arena,
        );
        assert!(bridge.handle_with(dropped(), &mut arena).accepted);
        let payload = DropPayload::Files(vec!["/tmp/a.png".into()]);
        assert_eq!(
            bridge.deliver_payload(payload.clone(), &mut arena),
            Some(EventResponse::Handled)
        );
        assert_eq!(
            log.lock().unwrap().as_slice(),
            &[
                hover(5.0, 5.0),
                WidgetEvent::Dropped {
                    position: Some(Vec2::new(5.0, 5.0)),
                    payload,
                },
            ]
        );
    }
}
