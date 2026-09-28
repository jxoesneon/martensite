//! Category C — Reactive State & Signal DAG tools (spec §3.5–3.6).
//!
//! Both tools are live-only (ADR-0039, invariant D9 — semantic honesty):
//! push-pull reactive state (`SignalId`, dirty flags, producer/consumer
//! edges, scheduler rank) exists only inside a running application's
//! `ReactiveRuntime`, so each tool forwards over the ADR-0038 dev channel and
//! surfaces the offline-mode IPC error when no `cargo martensite dev`
//! session answers. The server never fabricates a signal graph offline.

use std::borrow::Cow;

use rmcp::handler::server::router::tool::{AsyncTool, ToolBase};
use rmcp::model::ToolAnnotations;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use serde_json::json;

use crate::error::McpError;
use crate::server::{MartensiteMcp, ServerMode};
use crate::types::{Pagination, SignalDescriptor};

/// Parameters for `martensite_inspect_signals`.
///
/// # Examples
///
/// ```
/// use martensite_mcp::tools::reactive::InspectSignalsParams;
///
/// let p = InspectSignalsParams::default();
/// assert_eq!(p.limit(), 50);
/// assert_eq!(p.offset(), 0);
/// assert!(!p.only_dirty());
/// ```
#[derive(Debug, Clone, Default, Deserialize, JsonSchema)]
#[serde(default)]
pub struct InspectSignalsParams {
    /// Specific signal id to inspect.
    pub signal_id: Option<String>,
    /// Return only signals subscribed to by this widget node.
    pub node_id: Option<String>,
    /// Only signals flagged dirty this evaluation tick (default false).
    pub only_dirty: Option<bool>,
    /// Maximum signals returned (default 50, max 200).
    pub limit: Option<u32>,
    /// Pagination offset (default 0).
    pub offset: Option<u32>,
}

impl InspectSignalsParams {
    /// Effective result limit, clamped to the spec maximum.
    #[must_use]
    pub fn limit(&self) -> u32 {
        self.limit.unwrap_or(50).min(200)
    }

    /// Effective pagination offset.
    #[must_use]
    pub fn offset(&self) -> u32 {
        self.offset.unwrap_or(0)
    }

    /// Effective dirty-only filter.
    #[must_use]
    pub fn only_dirty(&self) -> bool {
        self.only_dirty.unwrap_or(false)
    }

    /// `signal_id` filter trimmed of whitespace, `None` when blank.
    fn signal_id(&self) -> Option<&str> {
        self.signal_id
            .as_deref()
            .map(str::trim)
            .filter(|s| !s.is_empty())
    }

    /// `node_id` filter trimmed of whitespace, `None` when blank.
    fn node_id(&self) -> Option<&str> {
        self.node_id
            .as_deref()
            .map(str::trim)
            .filter(|s| !s.is_empty())
    }
}

/// Result payload of `martensite_inspect_signals`.
///
/// # Examples
///
/// ```
/// use martensite_mcp::tools::reactive::InspectSignalsOutput;
/// use martensite_mcp::types::Pagination;
///
/// let out = InspectSignalsOutput {
///     signals: Vec::new(),
///     pagination: Pagination::new(0, 50, 0),
///     mode: "live".to_string(),
/// };
/// assert!(out.pagination.is_last_page());
/// ```
#[derive(Debug, Clone, Serialize, JsonSchema)]
pub struct InspectSignalsOutput {
    /// Signal descriptors materialized for the requested page.
    pub signals: Vec<SignalDescriptor>,
    /// Token-safe pagination envelope over the filtered result set.
    pub pagination: Pagination,
    /// Server mode that produced the answer (`live` on success).
    pub mode: String,
}

/// `martensite_inspect_signals`: push-pull reactive DAG introspection.
pub struct InspectSignalsTool;

impl ToolBase for InspectSignalsTool {
    type Parameter = InspectSignalsParams;
    type Output = InspectSignalsOutput;
    type Error = McpError;

    fn name() -> Cow<'static, str> {
        "martensite_inspect_signals".into()
    }

    fn description() -> Option<Cow<'static, str>> {
        Some(
            "Introspect the reactive signal DAG: current values, subscriber \
             counts, producer/consumer edges, scheduler rank, dirty flags — \
             with token-safe pagination. Requires a live `cargo martensite dev` \
             session."
                .into(),
        )
    }

    fn annotations() -> Option<ToolAnnotations> {
        crate::tools::read_only_annotations()
    }
}

impl AsyncTool<MartensiteMcp> for InspectSignalsTool {
    async fn invoke(
        service: &MartensiteMcp,
        param: Self::Parameter,
    ) -> Result<Self::Output, Self::Error> {
        let limit = param.limit() as usize;
        let offset = param.offset() as usize;
        let signal_id = param.signal_id().map(str::to_string);
        let node_id = param.node_id().map(str::to_string);

        // Live-only per spec: `live_call` produces the offline IPC error.
        let value = service.live_call(
            "signals_list",
            json!({
                "signal_id": signal_id,
                "node_id": node_id,
                "only_dirty": param.only_dirty(),
                "limit": limit,
                "offset": offset,
            }),
        )?;
        let mut signals = parse_signal_list(value)?;

        // Re-apply the field-exact filters client-side: idempotent when the
        // app already filtered, and keeps the pagination envelope honest when
        // it did not (D9 — never report a count over unfiltered data).
        if let Some(id) = signal_id.as_deref() {
            // `signal_id` also matches adapter names — callers address
            // signals as `"paused"` as often as `"SignalId(42)"`.
            signals.retain(|s| s.id == id || s.name.as_deref() == Some(id));
        }
        if param.only_dirty() {
            signals.retain(|s| s.dirty);
        }
        if let Some(node) = node_id.as_deref() {
            // `SignalDescriptor::consumers` is a list of opaque strings; only
            // second-guess the app when the payload actually carries consumer
            // detail, otherwise its own node filter is authoritative.
            if signals.iter().any(|s| !s.consumers.is_empty()) {
                signals.retain(|s| s.consumers.iter().any(|c| consumer_matches(c, node)));
            }
        }

        let total = signals.len();
        let page = signals.into_iter().skip(offset).take(limit).collect();
        Ok(InspectSignalsOutput {
            signals: page,
            pagination: Pagination::new(total, limit, offset),
            mode: mode_str(service).to_string(),
        })
    }
}

/// Parameters for `martensite_trigger_signal`.
#[derive(Debug, Clone, Default, Deserialize, JsonSchema)]
#[serde(default)]
pub struct TriggerSignalParams {
    /// Id of the signal to update.
    pub signal_id: String,
    /// Serialized JSON value to assign (type-checked against schema).
    pub new_value_json: String,
}

/// Result payload of `martensite_trigger_signal`.
///
/// # Examples
///
/// ```
/// use martensite_mcp::tools::reactive::TriggerSignalOutput;
///
/// let out = TriggerSignalOutput {
///     signal_id: "signal:7".to_string(),
///     applied: true,
///     invalidated_widgets: vec![12, 13],
///     reason: None,
///     mode: "live".to_string(),
/// };
/// assert!(out.applied);
/// ```
#[derive(Debug, Clone, Serialize, JsonSchema)]
pub struct TriggerSignalOutput {
    /// Signal the mutation targeted.
    pub signal_id: String,
    /// Whether the dev app applied the new value.
    pub applied: bool,
    /// Widget ids marked dirty for the next frame.
    #[serde(default)]
    pub invalidated_widgets: Vec<u64>,
    /// Rejection or type-check detail reported by the app, if any.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reason: Option<String>,
    /// Server mode that produced the answer (`live` on success).
    pub mode: String,
}

/// `martensite_trigger_signal`: mutate a tweak-gated signal in dev mode.
pub struct TriggerSignalTool;

impl ToolBase for TriggerSignalTool {
    type Parameter = TriggerSignalParams;
    type Output = TriggerSignalOutput;
    type Error = McpError;

    fn name() -> Cow<'static, str> {
        "martensite_trigger_signal".into()
    }

    fn description() -> Option<Cow<'static, str>> {
        Some(
            "Mutate a registered tweakable signal value to test dynamic UI \
             transitions; strictly type-checked and tweak-gated, returning \
             invalidated widgets marked dirty. Requires a live \
             `cargo martensite dev` session."
                .into(),
        )
    }

    fn annotations() -> Option<ToolAnnotations> {
        crate::tools::mutation_annotations()
    }
}

impl AsyncTool<MartensiteMcp> for TriggerSignalTool {
    async fn invoke(
        service: &MartensiteMcp,
        param: Self::Parameter,
    ) -> Result<Self::Output, Self::Error> {
        let signal_id = param.signal_id.trim();
        if signal_id.is_empty() {
            return Err(McpError::InvalidParameter(
                "`signal_id` must be a non-empty signal identifier".to_string(),
            ));
        }
        // Validate the serialized value client-side BEFORE the dev-channel
        // round trip: malformed JSON is a client error (-32602), not an
        // app-side type-check rejection.
        let value: serde_json::Value =
            serde_json::from_str(&param.new_value_json).map_err(|e| {
                McpError::InvalidParameter(format!("`new_value_json` is not valid JSON: {e}"))
            })?;

        let resp = service.live_call(
            "signal_trigger",
            json!({ "signal_id": signal_id, "value": value }),
        )?;
        Ok(trigger_output(signal_id, resp, mode_str(service)))
    }
}

/// Decodes the `signals_list` payload, tolerating either a bare
/// `[SignalDescriptor]` array or a `{ "signals": [...] }` envelope.
fn parse_signal_list(value: serde_json::Value) -> Result<Vec<SignalDescriptor>, McpError> {
    let payload = match value {
        serde_json::Value::Object(mut map) => map
            .remove("signals")
            .unwrap_or(serde_json::Value::Object(map)),
        other => other,
    };
    serde_json::from_value(payload)
        .map_err(|e| McpError::Ipc(format!("malformed `signals_list` response payload: {e}")))
}

/// Builds the typed `signal_trigger` result from the app's wire response —
/// `{applied, invalidated_widgets, reason}`.
fn trigger_output(signal_id: &str, resp: serde_json::Value, mode: &str) -> TriggerSignalOutput {
    let (applied, invalidated_widgets, reason) = match &resp {
        serde_json::Value::Object(map) => {
            let applied = map
                .get("applied")
                .and_then(serde_json::Value::as_bool)
                .unwrap_or(true);
            let invalidated_widgets = map
                .get("invalidated_widgets")
                .and_then(serde_json::Value::as_array)
                .map(|a| collect_u64s(a))
                .unwrap_or_default();
            let reason = map
                .get("reason")
                .and_then(serde_json::Value::as_str)
                .map(str::to_string);
            (applied, invalidated_widgets, reason)
        }
        _ => (true, Vec::new(), None),
    };
    TriggerSignalOutput {
        signal_id: signal_id.to_string(),
        applied,
        invalidated_widgets,
        reason,
        mode: mode.to_string(),
    }
}

/// Collects widget ids from a JSON array, tolerating integer or
/// numeric-string encodings.
fn collect_u64s(items: &[serde_json::Value]) -> Vec<u64> {
    items
        .iter()
        .filter_map(|v| {
            v.as_u64()
                .or_else(|| v.as_str().and_then(|s| s.parse().ok()))
        })
        .collect()
}

/// Whether an opaque consumer string denotes widget `node` — accepts a bare
/// id or the `widget:<id>` / `widget#<id>` / `widget(<id>)` encodings the
/// devtools inspector emits.
fn consumer_matches(consumer: &str, node: &str) -> bool {
    consumer == node
        || consumer.ends_with(&format!(":{node}"))
        || consumer.ends_with(&format!("#{node}"))
        || consumer.ends_with(&format!("({node})"))
}

/// Parameters for `martensite_set_signal`.
///
/// # Examples
///
/// ```
/// use martensite_mcp::tools::reactive::SetSignalParams;
///
/// let p = SetSignalParams {
///     signal_id: "counter".to_string(),
///     value: serde_json::json!(42),
/// };
/// assert_eq!(p.signal_id, "counter");
/// ```
#[derive(Debug, Clone, Default, Deserialize, JsonSchema)]
#[serde(default)]
pub struct SetSignalParams {
    /// Registered adapter name or numeric signal id to write.
    pub signal_id: String,
    /// JSON value to assign; validated by the app's `SignalAdapter`.
    pub value: serde_json::Value,
}

/// Result payload of `martensite_set_signal`.
///
/// # Examples
///
/// ```
/// use martensite_mcp::tools::reactive::SetSignalOutput;
///
/// let out = SetSignalOutput {
///     signal_id: "counter".to_string(),
///     applied: true,
///     reason: None,
///     mode: "live".to_string(),
/// };
/// assert!(out.applied);
/// ```
#[derive(Debug, Clone, Serialize, JsonSchema)]
pub struct SetSignalOutput {
    /// Signal the mutation targeted (echoed for correlation).
    pub signal_id: String,
    /// Whether the dev app's signal adapter applied the value.
    pub applied: bool,
    /// Rejection or type-check detail reported by the app, if any.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reason: Option<String>,
    /// Server mode that produced the answer (`live` on success).
    pub mode: String,
}

/// `martensite_set_signal`: write a JSON value into a reactive signal via
/// its registered `SignalAdapter`.
pub struct SetSignalTool;

impl ToolBase for SetSignalTool {
    type Parameter = SetSignalParams;
    type Output = SetSignalOutput;
    type Error = McpError;

    fn name() -> Cow<'static, str> {
        "martensite_set_signal".into()
    }

    fn description() -> Option<Cow<'static, str>> {
        Some(
            "Write a JSON value into a reactive signal through its registered \
             SignalAdapter (type-checked by the app); signals without an \
             adapter are inspectable but not writable. Requires a live \
             `cargo martensite dev` session."
                .into(),
        )
    }

    fn annotations() -> Option<ToolAnnotations> {
        crate::tools::mutation_annotations()
    }
}

impl AsyncTool<MartensiteMcp> for SetSignalTool {
    async fn invoke(
        service: &MartensiteMcp,
        param: Self::Parameter,
    ) -> Result<Self::Output, Self::Error> {
        let signal_id = param.signal_id.trim();
        if signal_id.is_empty() {
            return Err(McpError::InvalidParameter(
                "`signal_id` must be a non-empty signal identifier".to_string(),
            ));
        }

        // Live-only per spec: `live_call` produces the offline IPC error.
        let resp = service.live_call(
            "signal_set",
            json!({ "signal_id": signal_id, "value": param.value }),
        )?;
        Ok(set_signal_output(signal_id, &resp, mode_str(service)))
    }
}

/// Builds the typed `signal_set` result from the app's wire response —
/// `{applied, reason}`.
fn set_signal_output(signal_id: &str, resp: &serde_json::Value, mode: &str) -> SetSignalOutput {
    let (applied, reason) = match resp {
        serde_json::Value::Object(map) => {
            let applied = map
                .get("applied")
                .and_then(serde_json::Value::as_bool)
                .unwrap_or(true);
            let reason = map
                .get("reason")
                .and_then(serde_json::Value::as_str)
                .map(str::to_string);
            (applied, reason)
        }
        _ => (true, None),
    };
    SetSignalOutput {
        signal_id: signal_id.to_string(),
        applied,
        reason,
        mode: mode.to_string(),
    }
}

/// Current server mode as a stable wire string (`live` / `offline`).
fn mode_str(service: &MartensiteMcp) -> &'static str {
    match service.mode() {
        ServerMode::Live { .. } => "live",
        ServerMode::Offline => "offline",
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::server::McpServerOptions;

    fn signal(id: &str, dirty: bool, consumers: &[&str]) -> SignalDescriptor {
        SignalDescriptor {
            id: id.to_string(),
            dirty,
            consumers: consumers.iter().map(|s| s.to_string()).collect(),
            ..SignalDescriptor::default()
        }
    }

    #[test]
    fn parses_bare_array_and_envelope() {
        let arr = serde_json::json!([{"id": "s1", "dirty": true}]);
        let parsed = parse_signal_list(arr).expect("array");
        assert_eq!(parsed.len(), 1);
        assert_eq!(parsed[0].id, "s1");
        assert!(parsed[0].dirty);

        let env = serde_json::json!({"signals": [{"id": "s2"}], "ignored": 4});
        let parsed = parse_signal_list(env).expect("envelope");
        assert_eq!(parsed[0].id, "s2");
    }

    #[test]
    fn rejects_malformed_signal_list() {
        let bad = serde_json::json!({"signals": {"not": "a list"}});
        assert!(matches!(parse_signal_list(bad), Err(McpError::Ipc(_))));
    }

    #[test]
    fn consumer_match_encodings() {
        assert!(consumer_matches("42", "42"));
        assert!(consumer_matches("widget:42", "42"));
        assert!(consumer_matches("widget#42", "42"));
        assert!(consumer_matches("widget(42)", "42"));
        assert!(!consumer_matches("widget:421", "42"));
        assert!(!consumer_matches("signal:7", "42"));
    }

    #[test]
    fn trigger_output_parses_object_envelope() {
        let resp = serde_json::json!({
            "applied": false,
            "invalidated_widgets": [4, "7"],
            "reason": "type mismatch",
        });
        let out = trigger_output("s", resp, "live");
        assert!(!out.applied);
        assert_eq!(out.invalidated_widgets, vec![4, 7]);
        assert_eq!(out.reason.as_deref(), Some("type mismatch"));
        assert_eq!(out.mode, "live");
    }

    #[test]
    fn trigger_output_parses_wire_object() {
        let out = trigger_output(
            "s",
            serde_json::json!({"applied": true, "invalidated_widgets": [9]}),
            "live",
        );
        assert!(out.applied);
        assert_eq!(out.invalidated_widgets, vec![9]);
    }

    #[tokio::test]
    async fn inspect_signals_requires_live_session() {
        // Without `cargo martensite dev` the tool must fail with the
        // offline IPC hint rather than fabricating a signal graph (D9).
        let server = MartensiteMcp::new(McpServerOptions::offline());
        let res = InspectSignalsTool::invoke(&server, InspectSignalsParams::default()).await;
        match res {
            Err(McpError::Ipc(msg)) => {
                assert!(
                    msg.contains("requires a live Martensite dev session"),
                    "{msg}"
                );
            }
            other => panic!("expected offline IPC hint, got {other:?}"),
        }
    }

    #[tokio::test]
    async fn trigger_signal_validates_before_ipc() {
        let server = MartensiteMcp::new(McpServerOptions::offline());
        // Blank signal id → InvalidParameter without any socket traffic.
        let res = TriggerSignalTool::invoke(
            &server,
            TriggerSignalParams {
                signal_id: "  ".to_string(),
                new_value_json: "1".to_string(),
            },
        )
        .await;
        assert!(matches!(res, Err(McpError::InvalidParameter(_))));

        // Malformed JSON payload → InvalidParameter without any socket traffic.
        let res = TriggerSignalTool::invoke(
            &server,
            TriggerSignalParams {
                signal_id: "s".to_string(),
                new_value_json: "{nope".to_string(),
            },
        )
        .await;
        assert!(matches!(res, Err(McpError::InvalidParameter(_))));
    }

    #[test]
    fn signal_helper_smoke() {
        // Keep the local helper used so dead-code warnings stay silent in
        // test builds where no app answers.
        let s = signal("s1", true, &["widget:3"]);
        assert!(consumer_matches(&s.consumers[0], "3"));
    }

    #[test]
    fn set_signal_output_parses_object_and_bool() {
        let out = set_signal_output(
            "counter",
            &serde_json::json!({"applied": false, "reason": "type mismatch"}),
            "live",
        );
        assert!(!out.applied);
        assert_eq!(out.reason.as_deref(), Some("type mismatch"));
        assert_eq!(out.signal_id, "counter");

        let out = set_signal_output(
            "s",
            &serde_json::json!({"applied": true, "reason": "wrote"}),
            "live",
        );
        assert!(out.applied);
        assert_eq!(out.reason.as_deref(), Some("wrote"));

        // A bare status object counts as applied.
        let out = set_signal_output("s", &serde_json::json!({"status": "ok"}), "live");
        assert!(out.applied);
    }

    #[tokio::test]
    async fn set_signal_validates_before_ipc() {
        let server = MartensiteMcp::new(McpServerOptions::offline());
        let res = SetSignalTool::invoke(
            &server,
            SetSignalParams {
                signal_id: "   ".to_string(),
                value: serde_json::json!(1),
            },
        )
        .await;
        assert!(matches!(res, Err(McpError::InvalidParameter(_))));
    }

    #[tokio::test]
    async fn set_signal_requires_live_session() {
        let server = MartensiteMcp::new(McpServerOptions::offline());
        let res = SetSignalTool::invoke(
            &server,
            SetSignalParams {
                signal_id: "counter".to_string(),
                value: serde_json::json!(7),
            },
        )
        .await;
        match res {
            Err(McpError::Ipc(msg)) => {
                assert!(
                    msg.contains("requires a live Martensite dev session"),
                    "{msg}"
                );
            }
            other => panic!("expected offline IPC hint, got {other:?}"),
        }
    }

    #[test]
    fn tool_annotations_classify_reactive_tools() {
        let ro = InspectSignalsTool::annotations().expect("annotations");
        assert_eq!(ro.read_only_hint, Some(true));

        for ann in [
            TriggerSignalTool::annotations(),
            SetSignalTool::annotations(),
        ] {
            let ann = ann.expect("annotations");
            assert_eq!(ann.read_only_hint, Some(false));
            assert_eq!(ann.destructive_hint, Some(true));
            assert_eq!(ann.idempotent_hint, Some(false));
        }
    }
}
