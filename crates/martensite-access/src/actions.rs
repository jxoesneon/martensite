//! Accessibility action dispatching.
//!
//! This module defines the action types that can be dispatched from the
//! platform accessibility subsystem to the Martensite event pipeline,
//! and provides an [`ActionHandler`] for routing them.
//!
//! The flow is:
//! 1. The platform accessibility API (UI Automation, NSAccessibility,
//!    AT-SPI2) sends an action request via AccessKit.
//! 2. The request is decoded into an [`A11yAction`].
//! 3. The [`ActionHandler`] routes it to the appropriate widget via
//!    the widget arena.
//! 4. The widget processes the action and may update its state.

use accesskit::{Action, ActionData};
use martensite_core::{
    EventContext, EventResponse, SemanticAction, WidgetArena, WidgetEvent, WidgetId,
};

/// Maps a decoded [`A11yAction`] to the widget-space
/// [`SemanticAction`] it should be delivered as, or `None` for actions
/// that carry no widget-side semantics (e.g. unsupported `Other`
/// actions without recognised `ActionData`).
///
/// Deliver the result through the normal event pipeline:
/// `WidgetEvent::SemanticAction(..)` to the action's target widget —
/// via `WidgetArena::dispatch_event` for arena targets, or
/// `WidgetArena::internal_widget_mut` /
/// `OverlayLayer::widget_at_mut` for virtual-node targets resolved by
/// `AccessKitAdapter::resolve_internal` / `resolve_overlay`.
///
/// # Examples
///
/// ```
/// use martensite_access::actions::{semantic_action_for, A11yAction, ActionTarget};
/// use martensite_core::{SemanticAction, WidgetId};
///
/// let id = WidgetId::from_parts(0, 1);
/// assert_eq!(
///     semantic_action_for(&A11yAction::Increment(ActionTarget::Arena(id))),
///     Some(SemanticAction::Increment),
/// );
/// ```
pub fn semantic_action_for(action: &A11yAction) -> Option<SemanticAction> {
    Some(match action {
        A11yAction::Click(_) => SemanticAction::Click,
        A11yAction::Focus(_) => SemanticAction::Focus,
        A11yAction::Blur(_) => SemanticAction::Blur,
        A11yAction::SetValue(_, value) => SemanticAction::SetValue(value.clone()),
        A11yAction::Increment(_) => SemanticAction::Increment,
        A11yAction::Decrement(_) => SemanticAction::Decrement,
        A11yAction::Expand(_) => SemanticAction::Expand,
        A11yAction::Collapse(_) => SemanticAction::Collapse,
        A11yAction::HideTooltip(_) => SemanticAction::HideTooltip,
        A11yAction::ShowTooltip(_) => SemanticAction::ShowTooltip,
        A11yAction::ShowContextMenu(_) => SemanticAction::ShowContextMenu,
        A11yAction::Other(_, action, data) => match action {
            Action::ScrollUp => SemanticAction::ScrollUp,
            Action::ScrollDown => SemanticAction::ScrollDown,
            Action::ScrollLeft => SemanticAction::ScrollLeft,
            Action::ScrollRight => SemanticAction::ScrollRight,
            Action::ScrollIntoView => SemanticAction::ScrollIntoView,
            Action::ScrollToPoint => match data {
                Some(ActionData::ScrollToPoint(p)) => {
                    SemanticAction::ScrollToPoint(glam::Vec2::new(p.x as f32, p.y as f32))
                }
                _ => return None,
            },
            Action::SetScrollOffset => match data {
                Some(ActionData::SetScrollOffset(p)) => {
                    SemanticAction::SetScrollOffset(glam::Vec2::new(p.x as f32, p.y as f32))
                }
                _ => return None,
            },
            _ => return None,
        },
    })
}

/// Where an [`A11yAction`] is delivered.
///
/// AccessKit `NodeId`s cover three kinds of target: real arena
/// widgets, widget-internal virtual nodes (a radio group's options, a
/// tab bar's tabs), and virtual nodes inside overlay popups (a
/// dropdown's listbox options). The adapter's
/// [`decode_action`](crate::AccessKitAdapter::decode_action)
/// resolves the `NodeId` into one of these; [`dispatch_a11y_action`]
/// then routes the semantic event to the right widget.
///
/// # Examples
///
/// ```
/// use martensite_access::actions::ActionTarget;
/// use martensite_core::WidgetId;
///
/// let id = WidgetId::from_parts(0, 1);
/// let arena_target = ActionTarget::Arena(id);
/// let internal = ActionTarget::Internal(id, vec![2]); // third internal child
/// let popup = ActionTarget::Overlay(7, vec![0]); // inside overlay entry 7
/// assert_eq!(arena_target.owner(), Some(id));
/// assert_eq!(internal.owner(), Some(id));
/// assert_eq!(popup.owner(), None); // owner resolved via the entry
/// ```
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ActionTarget {
    /// An arena widget node.
    Arena(WidgetId),
    /// A widget-internal virtual node: `(owning arena widget, child
    /// index path)` — the form `AccessKitAdapter::resolve_internal`
    /// returns.
    Internal(WidgetId, Vec<u32>),
    /// A virtual node inside an overlay popup: `(overlay entry id,
    /// child index path)` — the form `AccessKitAdapter::resolve_overlay`
    /// returns.
    Overlay(u64, Vec<u32>),
}

impl ActionTarget {
    /// The arena widget that owns this target.
    ///
    /// For [`ActionTarget::Overlay`] this is `None` — the entry id is
    /// resolved to its stamped owner at dispatch time (see
    /// [`OverlayEntry::owner`](martensite_core::overlay::OverlayEntry::owner)).
    pub fn owner(&self) -> Option<WidgetId> {
        match self {
            Self::Arena(id) | Self::Internal(id, _) => Some(*id),
            Self::Overlay(_, _) => None,
        }
    }
}

impl From<WidgetId> for ActionTarget {
    fn from(id: WidgetId) -> Self {
        Self::Arena(id)
    }
}

/// A decoded accessibility action targeting a specific widget or
/// virtual node.
///
/// This is a higher-level representation of an [`accesskit::ActionRequest`],
/// with the target [`ActionTarget`] resolved from the AccessKit `NodeId`
/// by the adapter's emission maps.
#[derive(Clone, Debug, PartialEq)]
pub enum A11yAction {
    /// Request to click/activate the widget (e.g., a button press).
    Click(ActionTarget),
    /// Request to set keyboard focus on the widget.
    Focus(ActionTarget),
    /// Request to remove keyboard focus from the widget.
    Blur(ActionTarget),
    /// Request to set the widget's value (e.g., text input content).
    SetValue(ActionTarget, String),
    /// Request to increment the widget's value (e.g., slider step up).
    Increment(ActionTarget),
    /// Request to decrement the widget's value (e.g., slider step down).
    Decrement(ActionTarget),
    /// Request to expand a collapsible widget.
    Expand(ActionTarget),
    /// Request to collapse an expandable widget.
    Collapse(ActionTarget),
    /// Request to dismiss a tooltip.
    HideTooltip(ActionTarget),
    /// Request to show a tooltip.
    ShowTooltip(ActionTarget),
    /// Request to show a context menu.
    ShowContextMenu(ActionTarget),
    /// An unhandled action that doesn't fit the common categories.
    /// Carries the original [`Action`] and any associated [`ActionData`].
    Other(ActionTarget, Action, Option<ActionData>),
}

impl A11yAction {
    /// Returns the target of this action — an arena widget or a
    /// virtual node inside one.
    pub fn target(&self) -> ActionTarget {
        match self {
            Self::Click(id)
            | Self::Focus(id)
            | Self::Blur(id)
            | Self::SetValue(id, _)
            | Self::Increment(id)
            | Self::Decrement(id)
            | Self::Expand(id)
            | Self::Collapse(id)
            | Self::HideTooltip(id)
            | Self::ShowTooltip(id)
            | Self::ShowContextMenu(id)
            | Self::Other(id, _, _) => id.clone(),
        }
    }

    /// Returns the associated [`ActionData`] for this action, if any.
    pub fn data(&self) -> Option<&ActionData> {
        match self {
            Self::SetValue(_, _) => None, // Value is already extracted as String.
            Self::Other(_, _, data) => data.as_ref(),
            _ => None,
        }
    }
}

/// Delivers a decoded [`A11yAction`] to its target as a
/// [`WidgetEvent::SemanticAction`].
///
/// - [`ActionTarget::Arena`] dispatches through
///   [`WidgetArena::dispatch_event`] — the normal pipeline, with
///   arena-level bubbling, dirty-marking, and focus capture.
/// - [`ActionTarget::Internal`] reaches the widget at `(owner, path)`
///   via [`WidgetArena::internal_widget_mut`].
/// - [`ActionTarget::Overlay`] reaches popup content at `(entry, path)`
///   via
///   [`OverlayLayer::widget_at_mut`](martensite_core::overlay::OverlayLayer::widget_at_mut).
///
/// For virtual targets the owning arena widget is then asked to
/// `a11y_prepare` (draining activations the child parked on it — e.g. a
/// radio option's `select` request) and dirty-marked so the change is
/// emitted by the next `TreeUpdate`. A `CaptureFocus` response — or a
/// `Focus` action on a virtual node, where focus resolves to the
/// owner — becomes an arena focus request via
/// [`WidgetArena::request_focus`].
///
/// Returns the target widget's response; [`EventResponse::Ignored`]
/// when the action has no widget-side semantics or the target no
/// longer exists.
///
/// # Examples
///
/// ```
/// use martensite_access::actions::{dispatch_a11y_action, A11yAction, ActionTarget};
/// use martensite_core::{DummyWidget, HotNode, WidgetArena};
///
/// let mut arena = WidgetArena::new();
/// let id = arena.insert_with_widget(HotNode::default(), Box::new(DummyWidget));
/// // `DummyWidget` ignores everything, but the plumbing routes it.
/// assert_eq!(
///     dispatch_a11y_action(&mut arena, &A11yAction::Click(ActionTarget::Arena(id))),
///     martensite_core::EventResponse::Ignored,
/// );
/// ```
pub fn dispatch_a11y_action(arena: &mut WidgetArena, action: &A11yAction) -> EventResponse {
    let Some(semantic) = semantic_action_for(action) else {
        return EventResponse::Ignored;
    };
    // `Focus` on a virtual node resolves to a focus request on its
    // owning arena widget — decided here, before `semantic` moves.
    let wants_owner_focus = semantic == SemanticAction::Focus;
    let event = WidgetEvent::SemanticAction(semantic);
    let scale = arena.scale_factor();
    match action.target() {
        ActionTarget::Arena(id) => arena.dispatch_event(id, &event),
        ActionTarget::Internal(owner, path) => {
            let bounds = arena.get_hot(owner).map(|h| h.bounds).unwrap_or_default();
            let response = match arena.internal_widget_mut(owner, &path) {
                Some(widget) => widget.event(&mut EventContext {
                    event: &event,
                    bounds,
                    scale,
                }),
                None => return EventResponse::Ignored,
            };
            finish_virtual_dispatch(arena, owner, wants_owner_focus, response)
        }
        ActionTarget::Overlay(entry, path) => {
            let owner = arena.overlay().entry(entry).and_then(|e| e.owner());
            let bounds = arena
                .overlay()
                .entry(entry)
                .map(|e| e.bounds())
                .unwrap_or_default();
            let response = match arena.overlay_mut().widget_at_mut(entry, &path) {
                Some(widget) => widget.event(&mut EventContext {
                    event: &event,
                    bounds,
                    scale,
                }),
                None => return EventResponse::Ignored,
            };
            match owner {
                Some(owner) => finish_virtual_dispatch(arena, owner, wants_owner_focus, response),
                None => response,
            }
        }
    }
}

/// Post-dispatch bookkeeping for actions delivered to virtual targets:
/// drain activations the child parked on its owner, mark the owner for
/// re-emission, and translate focus requests into arena focus requests.
fn finish_virtual_dispatch(
    arena: &mut WidgetArena,
    owner: WidgetId,
    wants_owner_focus: bool,
    response: EventResponse,
) -> EventResponse {
    if let Some(owner_widget) = arena.internal_widget_mut(owner, &[]) {
        owner_widget.a11y_prepare();
    }
    arena.mark_dirty(owner);
    if response == EventResponse::CaptureFocus || wants_owner_focus {
        arena.request_focus(owner);
    }
    response
}

/// A handler trait for accessibility actions.
///
/// Implement this to receive decoded accessibility actions from the
/// platform. The handler can then dispatch them into the Martensite
/// event pipeline or update widget state directly.
pub trait ActionHandler {
    /// Called when an accessibility action is dispatched.
    fn handle_action(&mut self, arena: &mut WidgetArena, action: &A11yAction);
}

/// A simple action dispatcher that collects actions for batch processing.
///
/// This is useful for testing and for architectures where actions are
/// queued and processed in a single pass.
///
/// # Examples
///
/// ```
/// use martensite_access::actions::{A11yAction, ActionTarget, QueuedActionDispatcher};
/// use martensite_core::{WidgetArena, WidgetId};
///
/// let mut dispatcher = QueuedActionDispatcher::new();
/// let id = WidgetId::from_parts(0, 1);
/// dispatcher.enqueue(A11yAction::Click(ActionTarget::Arena(id)));
///
/// let actions = dispatcher.drain();
/// assert_eq!(actions.len(), 1);
/// assert_eq!(actions[0], A11yAction::Click(ActionTarget::Arena(id)));
/// ```
pub struct QueuedActionDispatcher {
    queue: Vec<A11yAction>,
}

impl QueuedActionDispatcher {
    /// Creates a new empty dispatcher.
    pub fn new() -> Self {
        Self { queue: Vec::new() }
    }

    /// Enqueues an action for later processing.
    pub fn enqueue(&mut self, action: A11yAction) {
        self.queue.push(action);
    }

    /// Returns the number of queued actions.
    pub fn len(&self) -> usize {
        self.queue.len()
    }

    /// Returns `true` if no actions are queued.
    pub fn is_empty(&self) -> bool {
        self.queue.is_empty()
    }

    /// Drains all queued actions, returning them in FIFO order.
    pub fn drain(&mut self) -> Vec<A11yAction> {
        std::mem::take(&mut self.queue)
    }

    /// Peeks at the next action without removing it.
    pub fn peek(&self) -> Option<&A11yAction> {
        self.queue.first()
    }
}

impl Default for QueuedActionDispatcher {
    fn default() -> Self {
        Self::new()
    }
}

impl ActionHandler for QueuedActionDispatcher {
    fn handle_action(&mut self, _arena: &mut WidgetArena, action: &A11yAction) {
        self.queue.push(action.clone());
    }
}

/// A closure-based action handler for ergonomic inline usage.
pub struct ClosureActionHandler<F>
where
    F: FnMut(&mut WidgetArena, &A11yAction),
{
    handler: F,
}

impl<F> ClosureActionHandler<F>
where
    F: FnMut(&mut WidgetArena, &A11yAction),
{
    /// Creates a new handler from a closure.
    pub fn new(handler: F) -> Self {
        Self { handler }
    }
}

impl<F> ActionHandler for ClosureActionHandler<F>
where
    F: FnMut(&mut WidgetArena, &A11yAction),
{
    fn handle_action(&mut self, arena: &mut WidgetArena, action: &A11yAction) {
        (self.handler)(arena, action);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::AccessKitAdapter;
    use accesskit::ActionRequest;
    use martensite_core::{ColdNode, HotNode, WidgetArena};

    fn make_arena() -> (WidgetArena, WidgetId) {
        let mut arena = WidgetArena::new();
        let id = arena.insert(HotNode::default(), ColdNode::default());
        (arena, id)
    }

    fn arena_target(id: WidgetId) -> ActionTarget {
        ActionTarget::Arena(id)
    }

    #[test]
    fn action_target_returns_action_target() {
        let id = WidgetId::from_parts(5, 1);
        let t = arena_target(id);
        assert_eq!(A11yAction::Click(t.clone()).target(), t);
        assert_eq!(A11yAction::Focus(t.clone()).target(), t);
        assert_eq!(A11yAction::Blur(t.clone()).target(), t);
        assert_eq!(A11yAction::SetValue(t.clone(), "x".into()).target(), t);
        assert_eq!(A11yAction::Increment(t.clone()).target(), t);
        assert_eq!(A11yAction::Decrement(t.clone()).target(), t);
        assert_eq!(A11yAction::Expand(t.clone()).target(), t);
        assert_eq!(A11yAction::Collapse(t.clone()).target(), t);
        assert_eq!(A11yAction::HideTooltip(t.clone()).target(), t);
        assert_eq!(A11yAction::ShowTooltip(t.clone()).target(), t);
        assert_eq!(A11yAction::ShowContextMenu(t.clone()).target(), t);
        assert_eq!(
            A11yAction::Other(t.clone(), Action::Click, None).target(),
            t
        );
    }

    #[test]
    fn decode_click_action() {
        let (arena, id) = make_arena();
        let adapter = AccessKitAdapter::new(id);
        let request = ActionRequest {
            target_node: crate::widget_id_to_node_id(id),
            target_tree: accesskit::TreeId::ROOT,
            action: Action::Click,
            data: None,
        };
        let action = adapter.decode_action(&arena, &request).unwrap();
        assert_eq!(action, A11yAction::Click(arena_target(id)));
    }

    #[test]
    fn decode_focus_action() {
        let (arena, id) = make_arena();
        let adapter = AccessKitAdapter::new(id);
        let request = ActionRequest {
            target_node: crate::widget_id_to_node_id(id),
            target_tree: accesskit::TreeId::ROOT,
            action: Action::Focus,
            data: None,
        };
        let action = adapter.decode_action(&arena, &request).unwrap();
        assert_eq!(action, A11yAction::Focus(arena_target(id)));
    }

    #[test]
    fn decode_set_value_action() {
        let (arena, id) = make_arena();
        let adapter = AccessKitAdapter::new(id);
        let request = ActionRequest {
            target_node: crate::widget_id_to_node_id(id),
            target_tree: accesskit::TreeId::ROOT,
            action: Action::SetValue,
            data: Some(accesskit::ActionData::Value("hello".into())),
        };
        let action = adapter.decode_action(&arena, &request).unwrap();
        assert_eq!(
            action,
            A11yAction::SetValue(arena_target(id), "hello".to_string())
        );
    }

    #[test]
    fn decode_set_value_numeric_action() {
        let (arena, id) = make_arena();
        let adapter = AccessKitAdapter::new(id);
        let request = ActionRequest {
            target_node: crate::widget_id_to_node_id(id),
            target_tree: accesskit::TreeId::ROOT,
            action: Action::SetValue,
            data: Some(accesskit::ActionData::NumericValue(42.0)),
        };
        let action = adapter.decode_action(&arena, &request).unwrap();
        assert_eq!(
            action,
            A11yAction::SetValue(arena_target(id), "42".to_string())
        );
    }

    #[test]
    fn decode_set_value_malformed_returns_none() {
        let (arena, id) = make_arena();
        let adapter = AccessKitAdapter::new(id);
        let request = ActionRequest {
            target_node: crate::widget_id_to_node_id(id),
            target_tree: accesskit::TreeId::ROOT,
            action: Action::SetValue,
            data: None, // Missing required data
        };
        assert!(adapter.decode_action(&arena, &request).is_none());
    }

    #[test]
    fn decode_increment_action() {
        let (arena, id) = make_arena();
        let adapter = AccessKitAdapter::new(id);
        let request = ActionRequest {
            target_node: crate::widget_id_to_node_id(id),
            target_tree: accesskit::TreeId::ROOT,
            action: Action::Increment,
            data: None,
        };
        let action = adapter.decode_action(&arena, &request).unwrap();
        assert_eq!(action, A11yAction::Increment(arena_target(id)));
    }

    #[test]
    fn decode_returns_none_for_dead_widget() {
        let (mut arena, id) = make_arena();
        let adapter = AccessKitAdapter::new(id);
        arena.remove(id);
        let request = ActionRequest {
            target_node: crate::widget_id_to_node_id(id),
            target_tree: accesskit::TreeId::ROOT,
            action: Action::Click,
            data: None,
        };
        assert!(adapter.decode_action(&arena, &request).is_none());
    }

    #[test]
    fn decode_returns_none_for_wrong_tree() {
        let (arena, id) = make_arena();
        let adapter = AccessKitAdapter::new(id);
        let wrong_tree = accesskit::TreeId(uuid::Uuid::new_v4());
        let request = ActionRequest {
            target_node: crate::widget_id_to_node_id(id),
            target_tree: wrong_tree,
            action: Action::Click,
            data: None,
        };
        assert!(adapter.decode_action(&arena, &request).is_none());
    }

    #[test]
    fn queued_dispatcher_enqueue_and_drain() {
        let mut dispatcher = QueuedActionDispatcher::new();
        let id = WidgetId::from_parts(0, 1);

        assert!(dispatcher.is_empty());
        dispatcher.enqueue(A11yAction::Click(arena_target(id)));
        dispatcher.enqueue(A11yAction::Focus(arena_target(id)));
        assert_eq!(dispatcher.len(), 2);

        let actions = dispatcher.drain();
        assert_eq!(actions.len(), 2);
        assert_eq!(actions[0], A11yAction::Click(arena_target(id)));
        assert_eq!(actions[1], A11yAction::Focus(arena_target(id)));
        assert!(dispatcher.is_empty());
    }

    #[test]
    fn queued_dispatcher_peek() {
        let mut dispatcher = QueuedActionDispatcher::new();
        let id = WidgetId::from_parts(0, 1);
        dispatcher.enqueue(A11yAction::Click(arena_target(id)));
        assert_eq!(
            dispatcher.peek(),
            Some(&A11yAction::Click(arena_target(id)))
        );
    }

    #[test]
    fn queued_dispatcher_default_is_empty() {
        let dispatcher = QueuedActionDispatcher::default();
        assert!(dispatcher.is_empty());
    }

    #[test]
    fn closure_handler_invokes_closure() {
        let (mut arena, id) = make_arena();
        let mut handler = ClosureActionHandler::new(|_arena, action| {
            assert_eq!(action, &A11yAction::Focus(arena_target(id)));
        });
        handler.handle_action(&mut arena, &A11yAction::Focus(arena_target(id)));
    }

    #[test]
    fn queued_dispatcher_as_action_handler() {
        let (mut arena, id) = make_arena();
        let mut dispatcher = QueuedActionDispatcher::new();
        dispatcher.handle_action(&mut arena, &A11yAction::Click(arena_target(id)));
        assert_eq!(dispatcher.len(), 1);
    }

    #[test]
    fn decode_expand_and_collapse() {
        let (arena, id) = make_arena();
        let adapter = AccessKitAdapter::new(id);
        let req_expand = ActionRequest {
            target_node: crate::widget_id_to_node_id(id),
            target_tree: accesskit::TreeId::ROOT,
            action: Action::Expand,
            data: None,
        };
        assert_eq!(
            adapter.decode_action(&arena, &req_expand).unwrap(),
            A11yAction::Expand(arena_target(id))
        );

        let req_collapse = ActionRequest {
            target_node: crate::widget_id_to_node_id(id),
            target_tree: accesskit::TreeId::ROOT,
            action: Action::Collapse,
            data: None,
        };
        assert_eq!(
            adapter.decode_action(&arena, &req_collapse).unwrap(),
            A11yAction::Collapse(arena_target(id))
        );
    }

    #[test]
    fn decode_other_action() {
        let (arena, id) = make_arena();
        let adapter = AccessKitAdapter::new(id);
        let request = ActionRequest {
            target_node: crate::widget_id_to_node_id(id),
            target_tree: accesskit::TreeId::ROOT,
            action: Action::ScrollIntoView,
            data: None,
        };
        let action = adapter.decode_action(&arena, &request).unwrap();
        assert_eq!(
            action,
            A11yAction::Other(arena_target(id), Action::ScrollIntoView, None)
        );
    }

    #[test]
    fn decode_other_action_preserves_data() {
        let (arena, id) = make_arena();
        let adapter = AccessKitAdapter::new(id);
        let request = ActionRequest {
            target_node: crate::widget_id_to_node_id(id),
            target_tree: accesskit::TreeId::ROOT,
            action: Action::ScrollIntoView,
            data: Some(ActionData::NumericValue(10.0)),
        };
        let action = adapter.decode_action(&arena, &request).unwrap();
        assert_eq!(action.target(), arena_target(id));
        assert!(action.data().is_some());
        assert!(matches!(
            action.data(),
            Some(ActionData::NumericValue(10.0))
        ));
    }
}
