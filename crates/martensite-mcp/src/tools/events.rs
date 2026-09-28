//! Category G (part 1) — Interaction, Replay & TimeMachine tools
//! (spec §3.15–3.17).

use std::borrow::Cow;

use rmcp::handler::server::router::tool::{AsyncTool, ToolBase};
use rmcp::model::ToolAnnotations;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::error::McpError;
use crate::server::MartensiteMcp;
use crate::types::{EventRecordDescriptor, Pagination};

/// Synthetic event types accepted by `martensite_dispatch_event` (spec §3.15).
const DISPATCH_EVENT_TYPES: &[&str] = &[
    "pointer_click",
    "pointer_move",
    "scroll",
    "key_press",
    "text_input",
];

/// TimeMachine actions accepted by `martensite_step_timemachine` (spec §3.17).
const TIMEMACHINE_ACTIONS: &[&str] = &[
    "pause",
    "resume",
    "step_forward",
    "step_backward",
    "restore_checkpoint",
];

/// Parameters for `martensite_dispatch_event`.
#[derive(Debug, Clone, Default, Deserialize, JsonSchema)]
#[serde(default)]
pub struct DispatchEventParams {
    /// `"pointer_click"`, `"pointer_move"`, `"scroll"`, `"key_press"`,
    /// `"text_input"`.
    pub event_type: String,
    /// `[x, y]` logical coordinates for pointer events.
    pub position: Option<[f64; 2]>,
    /// Virtual key code or text for keyboard events.
    pub key: Option<String>,
    /// Text committed by an IME/text event (`"text_input"`).
    pub text: Option<String>,
    /// `[dx, dy]` scroll delta for scroll events.
    pub delta: Option<[f64; 2]>,
}

/// Structured result of `martensite_dispatch_event`.
///
/// # Examples
///
/// ```
/// use martensite_mcp::tools::events::DispatchEventOutput;
///
/// let out = DispatchEventOutput {
///     response: "Handled".to_string(),
///     hit_path: vec![3, 7],
/// };
/// assert_eq!(out.hit_path.len(), 2);
/// ```
#[derive(Debug, Clone, Default, PartialEq, Serialize, JsonSchema)]
pub struct DispatchEventOutput {
    /// `EventResponse`/`Disposition` produced by the router
    /// (`Handled`, `Ignored`, `RequestFocus`, `RequestRepaint`, ...).
    pub response: String,
    /// Resolved hit-test path of widget ids, root-first.
    pub hit_path: Vec<u64>,
}

impl DispatchEventOutput {
    /// Builds the output from the raw `event_dispatch` dev-channel payload.
    ///
    /// `response` carries the router verdict; `hit_path` is a bare id list.
    fn from_wire(result: &serde_json::Value) -> Self {
        let response = match result.get("response") {
            Some(serde_json::Value::String(s)) => s.clone(),
            Some(other) => other.to_string(),
            None => result
                .as_str()
                .map(str::to_string)
                .unwrap_or_else(|| result.to_string()),
        };
        let hit_path = result
            .get("hit_path")
            .and_then(serde_json::Value::as_array)
            .map(|arr| arr.iter().filter_map(serde_json::Value::as_u64).collect())
            .unwrap_or_default();
        Self { response, hit_path }
    }
}

/// Coerces a wire value (integer, float, or decimal string) into `u64`.
fn wire_u64(v: &serde_json::Value) -> Option<u64> {
    match v {
        serde_json::Value::Number(n) => {
            n.as_u64().or_else(|| n.as_f64().map(|f| f.max(0.0) as u64))
        }
        serde_json::Value::String(s) => s.trim().parse().ok(),
        _ => None,
    }
}

/// Coerces a wire value into a display string (numbers become decimal
/// strings; anything else is serialized verbatim).
fn wire_string(v: Option<&serde_json::Value>) -> String {
    match v {
        Some(serde_json::Value::String(s)) => s.clone(),
        Some(other) => other.to_string(),
        None => String::new(),
    }
}

/// `martensite_dispatch_event`: inject a synthetic input event into the
/// running app's `EventRouter` and observe the routing response.
pub struct DispatchEventTool;

impl ToolBase for DispatchEventTool {
    type Parameter = DispatchEventParams;
    type Output = DispatchEventOutput;
    type Error = McpError;

    fn name() -> Cow<'static, str> {
        "martensite_dispatch_event".into()
    }

    fn description() -> Option<Cow<'static, str>> {
        Some(
            "Dispatch a synthetic pointer/keyboard/scroll event into the \
             running application's EventRouter; returns the EventResponse \
             and resolved hit-test path."
                .into(),
        )
    }

    fn annotations() -> Option<ToolAnnotations> {
        crate::tools::mutation_annotations()
    }
}

impl AsyncTool<MartensiteMcp> for DispatchEventTool {
    async fn invoke(
        service: &MartensiteMcp,
        param: Self::Parameter,
    ) -> Result<Self::Output, Self::Error> {
        if !DISPATCH_EVENT_TYPES.contains(&param.event_type.as_str()) {
            return Err(McpError::InvalidParameter(format!(
                "event_type `{}` is invalid; expected one of: {}",
                param.event_type,
                DISPATCH_EVENT_TYPES.join(", ")
            )));
        }

        let mut params = serde_json::json!({ "event_type": param.event_type });
        if let Some(position) = param.position {
            params["position"] = serde_json::json!(position);
        }
        if let Some(key) = param.key {
            params["key"] = serde_json::json!(key);
        }
        if let Some(text) = param.text {
            params["text"] = serde_json::json!(text);
        }
        if let Some(delta) = param.delta {
            params["delta"] = serde_json::json!(delta);
        }

        // Live-only tool (spec §3.15): requires a running dev session.
        let result = service.live_call("event_dispatch", params)?;
        Ok(DispatchEventOutput::from_wire(&result))
    }
}

/// Parameters for `martensite_get_event_ledger`.
#[derive(Debug, Clone, Default, Deserialize, JsonSchema)]
#[serde(default)]
pub struct EventLedgerParams {
    /// Maximum ledger entries to return (default 50, max 200).
    pub limit: Option<u32>,
}

impl EventLedgerParams {
    /// Effective result limit, clamped to the spec maximum.
    #[must_use]
    pub fn limit(&self) -> u32 {
        self.limit.unwrap_or(50).min(200)
    }
}

/// Structured result of `martensite_get_event_ledger`.
///
/// # Examples
///
/// ```
/// use martensite_mcp::tools::events::EventLedgerOutput;
/// use martensite_mcp::types::Pagination;
///
/// let out = EventLedgerOutput {
///     events: Vec::new(),
///     pagination: Pagination::new(0, 50, 0),
/// };
/// assert!(out.pagination.is_last_page());
/// ```
#[derive(Debug, Clone, Default, PartialEq, Serialize, JsonSchema)]
pub struct EventLedgerOutput {
    /// Routed-event records returned by the dev app.
    pub events: Vec<EventRecordDescriptor>,
    /// Pagination envelope for follow-up ledger reads.
    pub pagination: Pagination,
}

/// `martensite_get_event_ledger`: recent routed-event history with
/// hit-test targets, bubbling ancestry, and drop reasons.
pub struct EventLedgerTool;

impl ToolBase for EventLedgerTool {
    type Parameter = EventLedgerParams;
    type Output = EventLedgerOutput;
    type Error = McpError;

    fn name() -> Cow<'static, str> {
        "martensite_get_event_ledger".into()
    }

    fn description() -> Option<Cow<'static, str>> {
        Some(
            "Retrieve the recent history of routed events: timestamp, type, \
             hit-test target, bubbling ancestry, and rejection reasons."
                .into(),
        )
    }

    fn annotations() -> Option<ToolAnnotations> {
        crate::tools::read_only_annotations()
    }
}

impl AsyncTool<MartensiteMcp> for EventLedgerTool {
    async fn invoke(
        service: &MartensiteMcp,
        param: Self::Parameter,
    ) -> Result<Self::Output, Self::Error> {
        let limit = param.limit();
        // Live tool (spec §3.16): offline mode surfaces a requires-live error.
        let result = service.live_call("event_ledger", serde_json::json!({ "limit": limit }))?;

        // `{events: [...], total: N}` envelope.
        let records_value = result
            .get("events")
            .cloned()
            .unwrap_or(serde_json::Value::Array(Vec::new()));
        let total = result
            .get("total")
            .and_then(serde_json::Value::as_u64)
            .map(|t| t as usize);

        let events = parse_event_records(records_value)?;
        let total = total.unwrap_or(events.len());
        Ok(EventLedgerOutput {
            events,
            pagination: Pagination::new(total, limit as usize, 0),
        })
    }
}

/// Deserializes a wire array of event records, tolerating records that omit
/// `seq` by assigning the array index (ledger order is preserved either way).
fn parse_event_records(value: serde_json::Value) -> Result<Vec<EventRecordDescriptor>, McpError> {
    let serde_json::Value::Array(items) = value else {
        return Err(McpError::Ipc(
            "event_ledger payload field `events` was not an array".to_string(),
        ));
    };
    items
        .into_iter()
        .enumerate()
        .map(|(idx, mut item)| {
            serde_json::from_value::<EventRecordDescriptor>(item.clone())
                .or_else(|_| {
                    if let serde_json::Value::Object(map) = &mut item {
                        map.entry("seq")
                            .or_insert_with(|| serde_json::Value::from(idx as u64));
                    }
                    serde_json::from_value::<EventRecordDescriptor>(item)
                })
                .map_err(|e| McpError::Ipc(format!("malformed event_ledger record #{idx}: {e}")))
        })
        .collect()
}

/// Parameters for `martensite_step_timemachine`.
#[derive(Debug, Clone, Default, Deserialize, JsonSchema)]
#[serde(default)]
pub struct TimemachineParams {
    /// `"pause"`, `"resume"`, `"step_forward"`, `"step_backward"`,
    /// `"restore_checkpoint"`.
    pub action: String,
    /// Specific frame checkpoint to restore (with `restore_checkpoint`).
    pub checkpoint_id: Option<u64>,
}

/// Structured result of `martensite_step_timemachine`.
///
/// # Examples
///
/// ```
/// use martensite_mcp::tools::events::TimemachineOutput;
///
/// let out = TimemachineOutput {
///     action: "step_forward".to_string(),
///     frame_timestamp: 12,
///     arena_fingerprint: 0xdead_beef,
///     signal_state_hash: "3a7f9c".to_string(),
///     checkpoint_id: Some(4),
/// };
/// assert_eq!(out.frame_timestamp, 12);
/// ```
#[derive(Debug, Clone, Default, PartialEq, Serialize, JsonSchema)]
pub struct TimemachineOutput {
    /// Action that was applied (echoed back for correlation).
    pub action: String,
    /// Frame timestamp / commit counter reported after the action.
    pub frame_timestamp: u64,
    /// `WidgetArena::state_fingerprint` after the action.
    pub arena_fingerprint: u64,
    /// Hash of the captured source-signal snapshot.
    pub signal_state_hash: String,
    /// Checkpoint restored or produced by the action, when applicable.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub checkpoint_id: Option<u64>,
}

impl TimemachineOutput {
    /// Builds the output from the raw `timemachine_step` dev-channel payload.
    fn from_wire(
        action: &str,
        requested_checkpoint: Option<u64>,
        result: &serde_json::Value,
    ) -> Self {
        Self {
            action: result
                .get("action")
                .and_then(serde_json::Value::as_str)
                .map(str::to_string)
                .unwrap_or_else(|| action.to_string()),
            frame_timestamp: result
                .get("frame_timestamp")
                .and_then(wire_u64)
                .unwrap_or(0),
            arena_fingerprint: result
                .get("arena_fingerprint")
                .and_then(wire_u64)
                .unwrap_or(0),
            signal_state_hash: wire_string(result.get("signal_state_hash")),
            checkpoint_id: result
                .get("checkpoint_id")
                .and_then(wire_u64)
                .or(requested_checkpoint),
        }
    }
}

/// `martensite_step_timemachine`: deterministic TimeMachine debugger control.
pub struct TimemachineTool;

impl ToolBase for TimemachineTool {
    type Parameter = TimemachineParams;
    type Output = TimemachineOutput;
    type Error = McpError;

    fn name() -> Cow<'static, str> {
        "martensite_step_timemachine".into()
    }

    fn description() -> Option<Cow<'static, str>> {
        Some(
            "Control the deterministic TimeMachine debugger: pause, resume, \
             step forward/backward, or restore a checkpoint; returns frame \
             timestamp, arena fingerprint, and signal state hash."
                .into(),
        )
    }

    fn annotations() -> Option<ToolAnnotations> {
        crate::tools::mutation_annotations()
    }
}

impl AsyncTool<MartensiteMcp> for TimemachineTool {
    async fn invoke(
        service: &MartensiteMcp,
        param: Self::Parameter,
    ) -> Result<Self::Output, Self::Error> {
        if !TIMEMACHINE_ACTIONS.contains(&param.action.as_str()) {
            return Err(McpError::InvalidAction(format!(
                "action `{}` is invalid; expected one of: {}",
                param.action,
                TIMEMACHINE_ACTIONS.join(", ")
            )));
        }
        if param.action == "restore_checkpoint" && param.checkpoint_id.is_none() {
            return Err(McpError::InvalidParameter(
                "action `restore_checkpoint` requires `checkpoint_id`".to_string(),
            ));
        }

        let mut params = serde_json::json!({ "action": param.action });
        if let Some(checkpoint_id) = param.checkpoint_id {
            params["checkpoint_id"] = serde_json::json!(checkpoint_id);
        }

        // Live-only tool (spec §3.17): requires a running dev session.
        let result = service.live_call("timemachine_step", params)?;
        Ok(TimemachineOutput::from_wire(
            &param.action,
            param.checkpoint_id,
            &result,
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::server::McpServerOptions;

    #[test]
    fn dispatch_params_accept_text() {
        let params: DispatchEventParams = serde_json::from_value(serde_json::json!({
            "event_type": "text_input",
            "text": "arrow"
        }))
        .unwrap();
        assert_eq!(params.text.as_deref(), Some("arrow"));
    }

    #[tokio::test]
    async fn dispatch_rejects_unknown_event_type() {
        let server = MartensiteMcp::new(McpServerOptions::offline());
        let params = DispatchEventParams {
            event_type: "quantum_flick".to_string(),
            ..DispatchEventParams::default()
        };
        let res = DispatchEventTool::invoke(&server, params).await;
        assert!(matches!(res, Err(McpError::InvalidParameter(_))));
    }

    #[tokio::test]
    async fn timemachine_rejects_unknown_action() {
        let server = MartensiteMcp::new(McpServerOptions::offline());
        let params = TimemachineParams {
            action: "rewind_universe".to_string(),
            checkpoint_id: None,
        };
        let res = TimemachineTool::invoke(&server, params).await;
        assert!(matches!(res, Err(McpError::InvalidAction(_))));
    }

    #[tokio::test]
    async fn timemachine_restore_requires_checkpoint() {
        let server = MartensiteMcp::new(McpServerOptions::offline());
        let params = TimemachineParams {
            action: "restore_checkpoint".to_string(),
            checkpoint_id: None,
        };
        let res = TimemachineTool::invoke(&server, params).await;
        assert!(matches!(res, Err(McpError::InvalidParameter(_))));
    }

    #[test]
    fn dispatch_output_parses_wire_payload() {
        let wire = serde_json::json!({
            "response": "Handled",
            "hit_path": [1, 9, 4],
        });
        let out = DispatchEventOutput::from_wire(&wire);
        assert_eq!(out.response, "Handled");
        assert_eq!(out.hit_path, vec![1, 9, 4]);
    }

    #[test]
    fn ledger_records_tolerate_missing_seq() {
        let wire = serde_json::json!([
            {"seq": 5, "event_type": "pointer_click"},
            {"event_type": "key_press"},
        ]);
        let events = parse_event_records(wire).expect("parses");
        assert_eq!(events.len(), 2);
        assert_eq!(events[0].seq, 5);
        assert_eq!(events[1].seq, 1);
        assert_eq!(events[1].event_type, "key_press");
    }

    #[test]
    fn timemachine_output_parses_wire_payload() {
        let wire = serde_json::json!({
            "frame_timestamp": 42,
            "arena_fingerprint": "1234",
            "signal_state_hash": 77,
        });
        let out = TimemachineOutput::from_wire("pause", None, &wire);
        assert_eq!(out.action, "pause");
        assert_eq!(out.frame_timestamp, 42);
        assert_eq!(out.arena_fingerprint, 1234);
        assert_eq!(out.signal_state_hash, "77");
    }
}
