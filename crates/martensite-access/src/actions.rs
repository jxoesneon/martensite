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

use accesskit::{Action, ActionData, ActionRequest};
use martensite_core::{WidgetArena, WidgetId};

use crate::node_id_to_widget_id;

/// A decoded accessibility action targeting a specific widget.
///
/// This is a higher-level representation of an [`accesskit::ActionRequest`],
/// with the target [`WidgetId`] resolved from the AccessKit `NodeId`.
#[derive(Clone, Debug, PartialEq)]
pub enum A11yAction {
    /// Request to click/activate the widget (e.g., a button press).
    Click(WidgetId),
    /// Request to set keyboard focus on the widget.
    Focus(WidgetId),
    /// Request to remove keyboard focus from the widget.
    Blur(WidgetId),
    /// Request to set the widget's value (e.g., text input content).
    SetValue(WidgetId, String),
    /// Request to increment the widget's value (e.g., slider step up).
    Increment(WidgetId),
    /// Request to decrement the widget's value (e.g., slider step down).
    Decrement(WidgetId),
    /// Request to expand a collapsible widget.
    Expand(WidgetId),
    /// Request to collapse an expandable widget.
    Collapse(WidgetId),
    /// Request to dismiss a tooltip.
    HideTooltip(WidgetId),
    /// Request to show a tooltip.
    ShowTooltip(WidgetId),
    /// Request to show a context menu.
    ShowContextMenu(WidgetId),
    /// An unhandled action that doesn't fit the common categories.
    /// Carries the original [`Action`] and any associated [`ActionData`].
    Other(WidgetId, Action, Option<ActionData>),
}

impl A11yAction {
    /// Returns the target [`WidgetId`] of this action.
    pub fn target(&self) -> WidgetId {
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
            | Self::Other(id, _, _) => *id,
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

/// Decodes an AccessKit `ActionRequest` into an [`A11yAction`].
///
/// Returns `None` if:
/// - The target `NodeId` cannot be resolved to a live [`WidgetId`].
/// - The `target_tree` does not match `expected_tree_id`.
/// - The action data is malformed (e.g., `SetValue` without `Value` or
///   `NumericValue` data).
pub fn decode_action_request(
    arena: &WidgetArena,
    request: &ActionRequest,
    expected_tree_id: &accesskit::TreeId,
) -> Option<A11yAction> {
    // Validate that the action targets our tree.
    if request.target_tree != *expected_tree_id {
        return None;
    }

    let widget_id = node_id_to_widget_id(request.target_node)?;
    if !arena.is_alive(widget_id) {
        return None;
    }

    Some(match request.action {
        Action::Click => A11yAction::Click(widget_id),
        Action::Focus => A11yAction::Focus(widget_id),
        Action::Blur => A11yAction::Blur(widget_id),
        Action::SetValue => {
            let value = match &request.data {
                Some(ActionData::Value(v)) => v.to_string(),
                Some(ActionData::NumericValue(n)) => n.to_string(),
                // Malformed: SetValue requires Value or NumericValue data.
                _ => return None,
            };
            A11yAction::SetValue(widget_id, value)
        }
        Action::Increment => A11yAction::Increment(widget_id),
        Action::Decrement => A11yAction::Decrement(widget_id),
        Action::Expand => A11yAction::Expand(widget_id),
        Action::Collapse => A11yAction::Collapse(widget_id),
        Action::HideTooltip => A11yAction::HideTooltip(widget_id),
        Action::ShowTooltip => A11yAction::ShowTooltip(widget_id),
        Action::ShowContextMenu => A11yAction::ShowContextMenu(widget_id),
        other => A11yAction::Other(widget_id, other, request.data.clone()),
    })
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
/// use martensite_access::actions::{A11yAction, QueuedActionDispatcher};
/// use martensite_core::{WidgetArena, WidgetId};
///
/// let mut dispatcher = QueuedActionDispatcher::new();
/// let id = WidgetId::from_parts(0, 1);
/// dispatcher.enqueue(A11yAction::Click(id));
///
/// let actions = dispatcher.drain();
/// assert_eq!(actions.len(), 1);
/// assert_eq!(actions[0], A11yAction::Click(id));
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
    use martensite_core::{ColdNode, HotNode, WidgetArena};

    fn make_arena() -> (WidgetArena, WidgetId) {
        let mut arena = WidgetArena::new();
        let id = arena.insert(HotNode::default(), ColdNode::default());
        (arena, id)
    }

    #[test]
    fn action_target_returns_widget_id() {
        let id = WidgetId::from_parts(5, 1);
        assert_eq!(A11yAction::Click(id).target(), id);
        assert_eq!(A11yAction::Focus(id).target(), id);
        assert_eq!(A11yAction::Blur(id).target(), id);
        assert_eq!(A11yAction::SetValue(id, "x".into()).target(), id);
        assert_eq!(A11yAction::Increment(id).target(), id);
        assert_eq!(A11yAction::Decrement(id).target(), id);
        assert_eq!(A11yAction::Expand(id).target(), id);
        assert_eq!(A11yAction::Collapse(id).target(), id);
        assert_eq!(A11yAction::HideTooltip(id).target(), id);
        assert_eq!(A11yAction::ShowTooltip(id).target(), id);
        assert_eq!(A11yAction::ShowContextMenu(id).target(), id);
        assert_eq!(A11yAction::Other(id, Action::Click, None).target(), id);
    }

    #[test]
    fn decode_click_action() {
        let (arena, id) = make_arena();
        let request = ActionRequest {
            target_node: crate::widget_id_to_node_id(id),
            target_tree: accesskit::TreeId::ROOT,
            action: Action::Click,
            data: None,
        };
        let action = decode_action_request(&arena, &request, &accesskit::TreeId::ROOT).unwrap();
        assert_eq!(action, A11yAction::Click(id));
    }

    #[test]
    fn decode_focus_action() {
        let (arena, id) = make_arena();
        let request = ActionRequest {
            target_node: crate::widget_id_to_node_id(id),
            target_tree: accesskit::TreeId::ROOT,
            action: Action::Focus,
            data: None,
        };
        let action = decode_action_request(&arena, &request, &accesskit::TreeId::ROOT).unwrap();
        assert_eq!(action, A11yAction::Focus(id));
    }

    #[test]
    fn decode_set_value_action() {
        let (arena, id) = make_arena();
        let request = ActionRequest {
            target_node: crate::widget_id_to_node_id(id),
            target_tree: accesskit::TreeId::ROOT,
            action: Action::SetValue,
            data: Some(accesskit::ActionData::Value("hello".into())),
        };
        let action = decode_action_request(&arena, &request, &accesskit::TreeId::ROOT).unwrap();
        assert_eq!(action, A11yAction::SetValue(id, "hello".to_string()));
    }

    #[test]
    fn decode_set_value_numeric_action() {
        let (arena, id) = make_arena();
        let request = ActionRequest {
            target_node: crate::widget_id_to_node_id(id),
            target_tree: accesskit::TreeId::ROOT,
            action: Action::SetValue,
            data: Some(accesskit::ActionData::NumericValue(42.0)),
        };
        let action = decode_action_request(&arena, &request, &accesskit::TreeId::ROOT).unwrap();
        assert_eq!(action, A11yAction::SetValue(id, "42".to_string()));
    }

    #[test]
    fn decode_set_value_malformed_returns_none() {
        let (arena, id) = make_arena();
        let request = ActionRequest {
            target_node: crate::widget_id_to_node_id(id),
            target_tree: accesskit::TreeId::ROOT,
            action: Action::SetValue,
            data: None, // Missing required data
        };
        assert!(decode_action_request(&arena, &request, &accesskit::TreeId::ROOT).is_none());
    }

    #[test]
    fn decode_increment_action() {
        let (arena, id) = make_arena();
        let request = ActionRequest {
            target_node: crate::widget_id_to_node_id(id),
            target_tree: accesskit::TreeId::ROOT,
            action: Action::Increment,
            data: None,
        };
        let action = decode_action_request(&arena, &request, &accesskit::TreeId::ROOT).unwrap();
        assert_eq!(action, A11yAction::Increment(id));
    }

    #[test]
    fn decode_returns_none_for_dead_widget() {
        let (mut arena, id) = make_arena();
        arena.remove(id);
        let request = ActionRequest {
            target_node: crate::widget_id_to_node_id(id),
            target_tree: accesskit::TreeId::ROOT,
            action: Action::Click,
            data: None,
        };
        assert!(decode_action_request(&arena, &request, &accesskit::TreeId::ROOT).is_none());
    }

    #[test]
    fn decode_returns_none_for_wrong_tree() {
        let (arena, id) = make_arena();
        let wrong_tree = accesskit::TreeId(uuid::Uuid::new_v4());
        let request = ActionRequest {
            target_node: crate::widget_id_to_node_id(id),
            target_tree: wrong_tree,
            action: Action::Click,
            data: None,
        };
        assert!(decode_action_request(&arena, &request, &accesskit::TreeId::ROOT).is_none());
    }

    #[test]
    fn queued_dispatcher_enqueue_and_drain() {
        let mut dispatcher = QueuedActionDispatcher::new();
        let id = WidgetId::from_parts(0, 1);

        assert!(dispatcher.is_empty());
        dispatcher.enqueue(A11yAction::Click(id));
        dispatcher.enqueue(A11yAction::Focus(id));
        assert_eq!(dispatcher.len(), 2);

        let actions = dispatcher.drain();
        assert_eq!(actions.len(), 2);
        assert_eq!(actions[0], A11yAction::Click(id));
        assert_eq!(actions[1], A11yAction::Focus(id));
        assert!(dispatcher.is_empty());
    }

    #[test]
    fn queued_dispatcher_peek() {
        let mut dispatcher = QueuedActionDispatcher::new();
        let id = WidgetId::from_parts(0, 1);
        dispatcher.enqueue(A11yAction::Click(id));
        assert_eq!(dispatcher.peek(), Some(&A11yAction::Click(id)));
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
            assert_eq!(action, &A11yAction::Focus(id));
        });
        handler.handle_action(&mut arena, &A11yAction::Focus(id));
    }

    #[test]
    fn queued_dispatcher_as_action_handler() {
        let (mut arena, id) = make_arena();
        let mut dispatcher = QueuedActionDispatcher::new();
        dispatcher.handle_action(&mut arena, &A11yAction::Click(id));
        assert_eq!(dispatcher.len(), 1);
    }

    #[test]
    fn decode_expand_and_collapse() {
        let (arena, id) = make_arena();
        let req_expand = ActionRequest {
            target_node: crate::widget_id_to_node_id(id),
            target_tree: accesskit::TreeId::ROOT,
            action: Action::Expand,
            data: None,
        };
        assert_eq!(
            decode_action_request(&arena, &req_expand, &accesskit::TreeId::ROOT).unwrap(),
            A11yAction::Expand(id)
        );

        let req_collapse = ActionRequest {
            target_node: crate::widget_id_to_node_id(id),
            target_tree: accesskit::TreeId::ROOT,
            action: Action::Collapse,
            data: None,
        };
        assert_eq!(
            decode_action_request(&arena, &req_collapse, &accesskit::TreeId::ROOT).unwrap(),
            A11yAction::Collapse(id)
        );
    }

    #[test]
    fn decode_other_action() {
        let (arena, id) = make_arena();
        let request = ActionRequest {
            target_node: crate::widget_id_to_node_id(id),
            target_tree: accesskit::TreeId::ROOT,
            action: Action::ScrollIntoView,
            data: None,
        };
        let action = decode_action_request(&arena, &request, &accesskit::TreeId::ROOT).unwrap();
        assert_eq!(action, A11yAction::Other(id, Action::ScrollIntoView, None));
    }

    #[test]
    fn decode_other_action_preserves_data() {
        let (arena, id) = make_arena();
        let request = ActionRequest {
            target_node: crate::widget_id_to_node_id(id),
            target_tree: accesskit::TreeId::ROOT,
            action: Action::ScrollIntoView,
            data: Some(ActionData::NumericValue(10.0)),
        };
        let action = decode_action_request(&arena, &request, &accesskit::TreeId::ROOT).unwrap();
        assert_eq!(action.target(), id);
        assert!(action.data().is_some());
        assert!(matches!(
            action.data(),
            Some(ActionData::NumericValue(10.0))
        ));
    }
}
