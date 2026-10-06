//! Category I — Runtime Diagnostics tools: `martensite_runtime_errors` and
//! `martensite_logs`.
//!
//! Both tools are live-only (ADR-0039, invariant D9 — semantic honesty):
//! captured panics, `ErrorSurface` diagnostics, reactive evaluation errors,
//! and the tracing `LogRing` exist only inside a running application's dev
//! session, so each tool forwards over the ADR-0038 dev channel and
//! surfaces the offline-mode IPC error when no `cargo martensite dev`
//! session answers. The server never fabricates error or log state offline.

use std::borrow::Cow;

use rmcp::handler::server::router::tool::{AsyncTool, ToolBase};
use rmcp::model::ToolAnnotations;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

use crate::error::McpError;
use crate::server::{MartensiteMcp, ServerMode};
use crate::types::{LogRecordDescriptor, RuntimeErrorDescriptor};

/// Severity filters accepted by `martensite_runtime_errors`.
const RUNTIME_ERROR_SEVERITIES: &[&str] =
    &["info", "warning", "warn", "error", "critical", "fatal"];

/// Level filters accepted by `martensite_logs` (minimum severity).
const LOG_LEVELS: &[&str] = &["trace", "debug", "info", "warn", "warning", "error"];

/// Parameters for `martensite_runtime_errors`.
///
/// # Examples
///
/// ```
/// use martensite_mcp::tools::runtime::RuntimeErrorsParams;
///
/// let p = RuntimeErrorsParams {
///     severity: Some("error".to_string()),
///     ..RuntimeErrorsParams::default()
/// };
/// assert!(p.validate().is_ok());
/// assert_eq!(p.limit(), 50);
/// ```
#[derive(Debug, Clone, Default, Deserialize, JsonSchema)]
#[serde(default)]
pub struct RuntimeErrorsParams {
    /// Maximum error records to return (default 50, max 500).
    #[schemars(schema_with = "crate::types::schema_strip::opt_u32")]
    pub limit: Option<u32>,
    /// Severity filter: `info`, `warning`, `error`, `critical`, `fatal`.
    pub severity: Option<String>,
    /// When `true`, clears the captured panic/error state after reading
    /// (default false).
    pub clear: Option<bool>,
}

impl RuntimeErrorsParams {
    /// Effective result limit, clamped to the wire maximum.
    #[must_use]
    pub fn limit(&self) -> u32 {
        self.limit.unwrap_or(50).min(500)
    }

    /// Effective clear-after-read flag.
    #[must_use]
    pub fn clear(&self) -> bool {
        self.clear.unwrap_or(false)
    }

    /// Validates parameters client-side before the dev-channel round trip.
    ///
    /// # Errors
    ///
    /// [`McpError::InvalidParameter`] when `severity` is not a known level.
    pub fn validate(&self) -> Result<(), McpError> {
        if let Some(severity) = &self.severity {
            let norm = severity.trim().to_ascii_lowercase();
            if !RUNTIME_ERROR_SEVERITIES.contains(&norm.as_str()) {
                return Err(McpError::InvalidParameter(format!(
                    "`severity` `{severity}` is invalid; expected one of: {}",
                    RUNTIME_ERROR_SEVERITIES.join(", ")
                )));
            }
        }
        Ok(())
    }
}

/// Result payload of `martensite_runtime_errors`.
///
/// # Examples
///
/// ```
/// use martensite_mcp::tools::runtime::RuntimeErrorsOutput;
///
/// let out = RuntimeErrorsOutput {
///     errors: Vec::new(),
///     total: 0,
///     panic_active: false,
///     surface_attached: true,
///     cleared: false,
///     mode: "live".to_string(),
/// };
/// assert!(out.surface_attached);
/// ```
#[derive(Debug, Clone, Serialize, JsonSchema)]
pub struct RuntimeErrorsOutput {
    /// Structured error records (most recent first as reported by the app).
    pub errors: Vec<RuntimeErrorDescriptor>,
    /// Total error records available before `limit` clamping.
    #[schemars(schema_with = "crate::types::schema_strip::usize_s")]
    pub total: usize,
    /// Whether a panic is currently captured/active in the session.
    pub panic_active: bool,
    /// Whether an `ErrorSurface` is attached and feeding diagnostics.
    pub surface_attached: bool,
    /// Whether the session cleared captured error state after this read.
    pub cleared: bool,
    /// Server mode that produced the answer (`live` on success).
    pub mode: String,
}

/// `martensite_runtime_errors`: structured runtime error records — captured
/// panics, error-surface diagnostics, and reactive evaluation errors.
pub struct RuntimeErrorsTool;

impl ToolBase for RuntimeErrorsTool {
    type Parameter = RuntimeErrorsParams;
    type Output = RuntimeErrorsOutput;
    type Error = McpError;

    fn name() -> Cow<'static, str> {
        "martensite_runtime_errors".into()
    }

    fn description() -> Option<Cow<'static, str>> {
        Some(
            "Read structured runtime error records from the dev session: \
             captured panics, ErrorSurface diagnostics, and reactive \
             evaluation errors — with severity filtering and an optional \
             clear-after-read. Requires a live `cargo martensite dev` \
             session."
                .into(),
        )
    }

    fn annotations() -> Option<ToolAnnotations> {
        crate::tools::read_only_annotations()
    }
}

impl AsyncTool<MartensiteMcp> for RuntimeErrorsTool {
    async fn invoke(
        service: &MartensiteMcp,
        param: Self::Parameter,
    ) -> Result<Self::Output, Self::Error> {
        param.validate()?;
        let limit = param.limit();
        let severity = param
            .severity
            .as_deref()
            .map(|s| s.trim().to_ascii_lowercase())
            .filter(|s| !s.is_empty());

        // Live-only per spec: `live_call` produces the offline IPC error.
        let resp = service.live_call(
            "runtime_errors",
            json!({
                "limit": limit,
                "severity": severity,
                "clear": param.clear(),
            }),
        )?;

        // `{errors: [...], total, panic_active, surface_attached}` envelope.
        let records_value = resp
            .get("errors")
            .cloned()
            .unwrap_or(Value::Array(Vec::new()));
        let total = resp.get("total").and_then(wire_usize);
        let panic_active = resp
            .get("panic_active")
            .and_then(Value::as_bool)
            .unwrap_or(false);
        let surface_attached = resp
            .get("surface_attached")
            .and_then(Value::as_bool)
            .unwrap_or(false);

        let errors = parse_error_records(records_value)?;
        Ok(RuntimeErrorsOutput {
            total: total.unwrap_or(errors.len()),
            errors,
            panic_active,
            surface_attached,
            cleared: param.clear(),
            mode: mode_str(service).to_string(),
        })
    }
}

/// Parameters for `martensite_logs`.
///
/// # Examples
///
/// ```
/// use martensite_mcp::tools::runtime::LogsParams;
///
/// let p = LogsParams {
///     level: Some("warn".to_string()),
///     ..LogsParams::default()
/// };
/// assert!(p.validate().is_ok());
/// assert_eq!(p.limit(), 100);
/// ```
#[derive(Debug, Clone, Default, Deserialize, JsonSchema)]
#[serde(default)]
pub struct LogsParams {
    /// Maximum log records to return (default 100, max 1000).
    #[schemars(schema_with = "crate::types::schema_strip::opt_u32")]
    pub limit: Option<u32>,
    /// Minimum severity: `trace`, `debug`, `info`, `warn`, `error`.
    pub level: Option<String>,
    /// Only records whose `target` starts with this prefix.
    pub target_prefix: Option<String>,
    /// Case-insensitive substring filter on the formatted message.
    pub contains: Option<String>,
}

impl LogsParams {
    /// Effective result limit, clamped to the wire maximum.
    #[must_use]
    pub fn limit(&self) -> u32 {
        self.limit.unwrap_or(100).min(1000)
    }

    /// Validates parameters client-side before the dev-channel round trip.
    ///
    /// # Errors
    ///
    /// [`McpError::InvalidParameter`] when `level` is not a known tracing
    /// level.
    pub fn validate(&self) -> Result<(), McpError> {
        if let Some(level) = &self.level {
            let norm = level.trim().to_ascii_lowercase();
            if !LOG_LEVELS.contains(&norm.as_str()) {
                return Err(McpError::InvalidParameter(format!(
                    "`level` `{level}` is invalid; expected one of: {}",
                    LOG_LEVELS.join(", ")
                )));
            }
        }
        Ok(())
    }
}

/// Result payload of `martensite_logs`.
///
/// # Examples
///
/// ```
/// use martensite_mcp::tools::runtime::LogsOutput;
///
/// let out = LogsOutput {
///     logs: Vec::new(),
///     total_buffered: 0,
///     mode: "live".to_string(),
/// };
/// assert!(out.logs.is_empty());
/// ```
#[derive(Debug, Clone, Serialize, JsonSchema)]
pub struct LogsOutput {
    /// Log records matching the filters (capture order, oldest first).
    pub logs: Vec<LogRecordDescriptor>,
    /// Total records held in the session ring buffer.
    #[schemars(schema_with = "crate::types::schema_strip::usize_s")]
    pub total_buffered: usize,
    /// Server mode that produced the answer (`live` on success).
    pub mode: String,
}

/// `martensite_logs`: drain the dev session's bounded tracing ring buffer
/// with level/target/substring filters.
pub struct LogsTool;

impl ToolBase for LogsTool {
    type Parameter = LogsParams;
    type Output = LogsOutput;
    type Error = McpError;

    fn name() -> Cow<'static, str> {
        "martensite_logs".into()
    }

    fn description() -> Option<Cow<'static, str>> {
        Some(
            "Drain the dev session's bounded tracing ring buffer: seq, level, \
             target, message, source file/line, and capture timestamp — with \
             minimum-level, target-prefix, and message-substring filters. \
             Requires a live `cargo martensite dev` session."
                .into(),
        )
    }

    fn annotations() -> Option<ToolAnnotations> {
        crate::tools::read_only_annotations()
    }
}

impl AsyncTool<MartensiteMcp> for LogsTool {
    async fn invoke(
        service: &MartensiteMcp,
        param: Self::Parameter,
    ) -> Result<Self::Output, Self::Error> {
        param.validate()?;
        let level = param
            .level
            .as_deref()
            .map(|s| s.trim().to_ascii_lowercase())
            .filter(|s| !s.is_empty());

        // Live-only per spec: `live_call` produces the offline IPC error.
        let resp = service.live_call(
            "logs",
            json!({
                "limit": param.limit(),
                "level": level,
                "target_prefix": param.target_prefix,
                "contains": param.contains,
            }),
        )?;

        // `{logs: [...], total_buffered}` envelope.
        let records_value = resp
            .get("logs")
            .cloned()
            .unwrap_or(Value::Array(Vec::new()));
        let total_buffered = resp.get("total_buffered").and_then(wire_usize);

        let logs = parse_log_records(records_value)?;
        Ok(LogsOutput {
            total_buffered: total_buffered.unwrap_or(logs.len()),
            logs,
            mode: mode_str(service).to_string(),
        })
    }
}

/// Deserializes a wire array of runtime error records, tolerating plain
/// message strings, missing `seq` (assigned the array index), alias fields
/// (`level`/`msg`/`kind`/`time`), and stringly-typed `node_id`/`seq`.
fn parse_error_records(value: Value) -> Result<Vec<RuntimeErrorDescriptor>, McpError> {
    let Value::Array(items) = value else {
        return Err(McpError::Ipc(
            "runtime_errors payload field `errors` was not an array".to_string(),
        ));
    };
    items
        .into_iter()
        .enumerate()
        .map(|(idx, item)| match item {
            Value::String(message) => Ok(RuntimeErrorDescriptor {
                seq: idx as u64,
                severity: String::new(),
                source: String::new(),
                message,
                node_id: None,
                timestamp: None,
            }),
            Value::Object(mut map) => {
                normalize_record_numbers(&mut map, &["seq", "node_id"]);
                // `kind` is a `source` alias on the descriptor — panic
                // records carry both keys, which serde would reject as a
                // duplicate field, so keep the more specific `source`.
                if map.contains_key("source") {
                    map.remove("kind");
                }
                stringify_timestamp(&mut map, &["timestamp", "time", "ts"]);
                map.entry("seq").or_insert_with(|| Value::from(idx as u64));
                serde_json::from_value::<RuntimeErrorDescriptor>(Value::Object(map)).map_err(|e| {
                    McpError::Ipc(format!("malformed runtime_errors record #{idx}: {e}"))
                })
            }
            other => Err(McpError::Ipc(format!(
                "malformed runtime_errors record #{idx}: expected object or string, got {other}"
            ))),
        })
        .collect()
}

/// Deserializes a wire array of log records, tolerating missing `seq`
/// (assigned the array index) and stringly-typed `seq`/`line`.
fn parse_log_records(value: Value) -> Result<Vec<LogRecordDescriptor>, McpError> {
    let Value::Array(items) = value else {
        return Err(McpError::Ipc(
            "logs payload field `logs` was not an array".to_string(),
        ));
    };
    items
        .into_iter()
        .enumerate()
        .map(|(idx, item)| match item {
            Value::String(message) => Ok(LogRecordDescriptor {
                seq: idx as u64,
                message,
                ..LogRecordDescriptor::default()
            }),
            Value::Object(mut map) => {
                normalize_record_numbers(&mut map, &["seq", "line"]);
                stringify_timestamp(&mut map, &["timestamp"]);
                map.entry("seq").or_insert_with(|| Value::from(idx as u64));
                serde_json::from_value::<LogRecordDescriptor>(Value::Object(map))
                    .map_err(|e| McpError::Ipc(format!("malformed logs record #{idx}: {e}")))
            }
            other => Err(McpError::Ipc(format!(
                "malformed logs record #{idx}: expected object or string, got {other}"
            ))),
        })
        .collect()
}

/// Rewrites decimal-string entries under `keys` of `map` into JSON numbers
/// so strict `u64`/`u32` fields tolerate `"42"` encodings.
fn normalize_record_numbers(map: &mut serde_json::Map<String, Value>, keys: &[&str]) {
    for key in keys {
        if let Some(Value::String(s)) = map.get(*key) {
            if let Ok(n) = s.trim().parse::<u64>() {
                map.insert((*key).to_string(), Value::from(n));
            }
        }
    }
}

/// Rewrites numeric entries under `keys` of `map` into JSON strings so
/// strict `String`/`Option<String>` timestamp fields tolerate epoch or
/// nanosecond encodings.
fn stringify_timestamp(map: &mut serde_json::Map<String, Value>, keys: &[&str]) {
    for key in keys {
        if let Some(v @ (Value::Number(_) | Value::Bool(_))) = map.get_mut(*key) {
            *v = Value::String(v.to_string());
        }
    }
}

/// Coerces a wire value (integer, float, or decimal string) into `usize`.
fn wire_usize(v: &Value) -> Option<usize> {
    match v {
        Value::Number(n) => n
            .as_u64()
            .or_else(|| n.as_f64().map(|f| f.max(0.0) as u64))
            .and_then(|n| usize::try_from(n).ok()),
        Value::String(s) => s.trim().parse().ok(),
        _ => None,
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

    #[test]
    fn runtime_errors_params_validate_severity() {
        let p = RuntimeErrorsParams {
            severity: Some("loud".to_string()),
            ..RuntimeErrorsParams::default()
        };
        assert!(matches!(p.validate(), Err(McpError::InvalidParameter(_))));

        for sev in ["info", "warning", "error", "critical", "fatal", "ERROR"] {
            let p = RuntimeErrorsParams {
                severity: Some(sev.to_string()),
                ..RuntimeErrorsParams::default()
            };
            // Uppercase is accepted — it is normalized before the wire call.
            assert!(p.validate().is_ok(), "severity {sev} should validate");
        }

        assert_eq!(RuntimeErrorsParams::default().limit(), 50);
        assert_eq!(
            RuntimeErrorsParams {
                limit: Some(9999),
                ..RuntimeErrorsParams::default()
            }
            .limit(),
            500
        );
        assert!(!RuntimeErrorsParams::default().clear());
    }

    #[test]
    fn logs_params_validate_level() {
        let p = LogsParams {
            level: Some("verbose".to_string()),
            ..LogsParams::default()
        };
        assert!(matches!(p.validate(), Err(McpError::InvalidParameter(_))));

        for lvl in ["trace", "debug", "info", "warn", "warning", "error"] {
            let p = LogsParams {
                level: Some(lvl.to_string()),
                ..LogsParams::default()
            };
            assert!(p.validate().is_ok(), "level {lvl} should validate");
        }

        assert_eq!(LogsParams::default().limit(), 100);
        assert_eq!(
            LogsParams {
                limit: Some(u32::MAX),
                ..LogsParams::default()
            }
            .limit(),
            1000
        );
    }

    #[test]
    fn parse_error_records_objects_strings_and_aliases() {
        let wire = serde_json::json!([
            {"seq": "3", "level": "critical", "kind": "panic",
             "msg": "unwrap failed", "node_id": "9", "time": "2025-01-01T00:00:00Z"},
            "bare failure text",
            {"severity": "warning", "message": "layout cycle"}
        ]);
        let errors = parse_error_records(wire).expect("parses");
        assert_eq!(errors.len(), 3);
        assert_eq!(errors[0].seq, 3);
        assert_eq!(errors[0].severity, "critical");
        assert_eq!(errors[0].source, "panic");
        assert_eq!(errors[0].message, "unwrap failed");
        assert_eq!(errors[0].node_id, Some(9));
        assert_eq!(errors[0].timestamp.as_deref(), Some("2025-01-01T00:00:00Z"));
        assert_eq!(errors[1].seq, 1);
        assert_eq!(errors[1].message, "bare failure text");
        assert_eq!(errors[2].seq, 2);
        assert_eq!(errors[2].severity, "warning");
    }

    #[test]
    fn parse_error_records_accepts_real_panic_bundle() {
        // `panic_record` on the wire carries both `kind` and `source`
        // (alias collision) plus a numeric `timestamp_ns`.
        let wire = serde_json::json!([{
            "kind": "panic",
            "severity": "fatal",
            "source": "panic_hook",
            "message": "index out of bounds",
            "in_flight_node": 12,
            "phase": "event_dispatch",
            "timestamp_ns": 1_700_000_000_000_000_000_u64,
        }]);
        let errors = parse_error_records(wire).expect("parses");
        assert_eq!(errors.len(), 1);
        assert_eq!(errors[0].severity, "fatal");
        assert_eq!(errors[0].source, "panic_hook");
        assert_eq!(errors[0].message, "index out of bounds");
    }

    #[test]
    fn parse_error_records_rejects_non_array() {
        assert!(matches!(
            parse_error_records(serde_json::json!({"errors": "nope"})),
            Err(McpError::Ipc(_))
        ));
    }

    #[test]
    fn parse_log_records_objects_and_strings() {
        let wire = serde_json::json!([
            {"seq": "7", "level": "WARN", "target": "martensite::layout",
             "message": "constraint cycle", "file": "src/layout.rs", "line": "42",
             "timestamp": "2025-01-01T00:00:00Z"},
            "plain log line"
        ]);
        let logs = parse_log_records(wire).expect("parses");
        assert_eq!(logs.len(), 2);
        assert_eq!(logs[0].seq, 7);
        assert_eq!(logs[0].level, "WARN");
        assert_eq!(logs[0].target, "martensite::layout");
        assert_eq!(logs[0].file.as_deref(), Some("src/layout.rs"));
        assert_eq!(logs[0].line, Some(42));
        assert_eq!(logs[1].seq, 1);
        assert_eq!(logs[1].message, "plain log line");
    }

    #[test]
    fn parse_log_records_rejects_non_array() {
        assert!(matches!(
            parse_log_records(serde_json::json!({"logs": 42})),
            Err(McpError::Ipc(_))
        ));
    }

    #[tokio::test]
    async fn runtime_errors_requires_live_session() {
        // Without `cargo martensite dev` the tool must fail with the
        // offline IPC hint rather than fabricating error state (D9).
        let server = MartensiteMcp::new(McpServerOptions::offline());
        let res = RuntimeErrorsTool::invoke(&server, RuntimeErrorsParams::default()).await;
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
    async fn logs_requires_live_session() {
        let server = MartensiteMcp::new(McpServerOptions::offline());
        let res = LogsTool::invoke(&server, LogsParams::default()).await;
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
    fn tool_annotations_classify_runtime_tools() {
        for ann in [RuntimeErrorsTool::annotations(), LogsTool::annotations()] {
            let ann = ann.expect("annotations");
            assert_eq!(ann.read_only_hint, Some(true));
            assert_eq!(ann.destructive_hint, Some(false));
            assert_eq!(ann.open_world_hint, Some(false));
        }
    }
}
