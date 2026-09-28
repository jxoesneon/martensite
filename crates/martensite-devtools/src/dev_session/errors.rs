//! `runtime_errors`, `logs`, `a11y_action`, and `hot_reload` handlers.
//!
//! - `runtime_errors` merges every in-process error source the session
//!   can see without probe cooperation — the global dev-panic hook
//!   bundle, the attached [`ErrorSurface`](crate::error_surface::ErrorSurface)
//!   (tier-2 diagnostics + captured crash), and the ambient
//!   [`ReactiveRuntime`](martensite_reactive::ReactiveRuntime) error log —
//!   into one severity-tagged record list. It always succeeds: an empty
//!   list is an honest answer, not a capability gap.
//! - `logs` tails the attached [`LogRing`](super::log_ring::LogRing);
//!   with no ring attached the capability is genuinely absent and the
//!   handler reports `not_implemented`.
//! - `hot_reload` drops the coordinator marker file that
//!   `cargo martensite dev` polls for; a session not served through
//!   `serve_dev_session` has no marker path and reports `unavailable`.

use serde_json::{json, Value};

use martensite_core::WidgetId;

use crate::error_surface::{CrashBundle, DiagnosticSeverity};

use super::log_ring::truncate_message;
use super::{not_impl, DevSession, SessionResult};

/// Default `runtime_errors` page size.
const ERRORS_DEFAULT_LIMIT: usize = 50;
/// Maximum `runtime_errors` page size.
const ERRORS_MAX_LIMIT: u64 = 500;
/// Default `logs` page size.
const LOGS_DEFAULT_LIMIT: usize = 100;
/// Maximum `logs` page size.
const LOGS_MAX_LIMIT: u64 = 1000;

/// `hot_reload` failure when the session was not served with a
/// coordinator marker path — a plain error, not `not_implemented`: the
/// handler exists, the runtime wiring does not.
const HOT_RELOAD_UNAVAILABLE: &str = "unavailable: hot_reload requires the session to be served \
     via serve_dev_session (no coordinator marker path)";

impl DevSession {
    /// `runtime_errors` — structured error records (panic bundle, error
    /// surface diagnostics, reactive evaluation errors).
    ///
    /// `limit` defaults to 50 and clamps to 500. `severity` is an exact
    /// case-insensitive match on the record's severity string
    /// (`"warn"` is accepted as `"warning"`). `clear: true` drains every
    /// source after the snapshot is taken — the returned records are the
    /// pre-clear state.
    pub fn runtime_errors(&self, params: &Value) -> SessionResult {
        let limit = params
            .get("limit")
            .and_then(Value::as_u64)
            .map_or(ERRORS_DEFAULT_LIMIT, |v| v.min(ERRORS_MAX_LIMIT) as usize);
        let severity = params
            .get("severity")
            .and_then(Value::as_str)
            .map(normalize_severity);
        let clear = params
            .get("clear")
            .and_then(Value::as_bool)
            .unwrap_or(false);

        let mut records: Vec<Value> = Vec::new();
        let mut panic_active = false;

        // Source 1: the global dev-panic hook bundle (works unattached).
        let global_panic = if clear {
            crate::error_surface::take_last_panic()
        } else {
            crate::error_surface::peek_last_panic()
        };
        if let Some(bundle) = global_panic {
            panic_active = true;
            records.push(panic_record(&bundle, "panic_hook"));
        }

        // Source 2: the attached error surface (crash + tier-2 entries).
        let mut surface_attached = false;
        let mut surface_diagnostic_count = 0usize;
        {
            let mut guard = self.error_surface();
            if let Some(surface) = guard.as_mut() {
                surface_attached = true;
                surface_diagnostic_count = surface.diagnostic_count();
                if let Some(bundle) = surface.last_crash() {
                    panic_active = true;
                    records.push(panic_record(bundle, "error_surface"));
                }
                for entry in surface.tier2_entries() {
                    records.push(json!({
                        "kind": "diagnostic",
                        "severity": severity_str(entry.severity),
                        "message": truncated(&entry.one_line_cause),
                        "id": entry.id,
                        "node_id": entry.node_id.map(WidgetId::to_u64),
                        "node_path": entry.node_path,
                        "doc_link": entry.doc_link,
                        "first_seen_frame": entry.first_seen_frame,
                        "last_seen_frame": entry.last_seen_frame,
                        "occurrences": entry.occurrences,
                    }));
                }
                if clear {
                    surface.clear_crash();
                }
            }
        }

        // Source 3: ambient reactive-runtime evaluation errors.
        let runtime = martensite_reactive::ReactiveRuntime::current();
        for err in runtime.errors() {
            records.push(json!({
                "kind": "reactive",
                "severity": "error",
                "message": truncated(&err.to_string()),
            }));
        }
        if clear {
            runtime.clear_errors();
        }

        if let Some(want) = severity.as_deref() {
            records.retain(|r| r["severity"].as_str() == Some(want));
        }
        let total = records.len();
        records.truncate(limit);

        Ok(json!({
            "errors": records,
            "total": total,
            "panic_active": panic_active,
            "surface_attached": surface_attached,
            "surface_diagnostic_count": surface_diagnostic_count,
        }))
    }

    /// `logs` — tail the attached [`super::log_ring::LogRing`].
    ///
    /// `limit` defaults to 100 and clamps to 1000; `level` is a minimum
    /// severity, `target_prefix` and `contains` narrow the tail. Without
    /// a ring the capability is genuinely absent (the app never
    /// installed the layer), so the handler reports `not_implemented`
    /// rather than an empty list.
    pub fn logs(&self, params: &Value) -> SessionResult {
        let Some(ring) = self.log_ring() else {
            return Err(not_impl("logs"));
        };
        let limit = params
            .get("limit")
            .and_then(Value::as_u64)
            .map_or(LOGS_DEFAULT_LIMIT, |v| v.min(LOGS_MAX_LIMIT) as usize);
        let level = params.get("level").and_then(Value::as_str);
        let target_prefix = params.get("target_prefix").and_then(Value::as_str);
        let contains = params.get("contains").and_then(Value::as_str);

        let logs: Vec<Value> = ring
            .tail(limit, level, target_prefix, contains)
            .iter()
            .map(|r| {
                json!({
                    "seq": r.seq,
                    "level": r.level,
                    "target": r.target,
                    "message": r.message,
                    "file": r.file,
                    "line": r.line,
                    "timestamp": r.timestamp,
                })
            })
            .collect();

        Ok(json!({
            "logs": logs,
            "total_buffered": ring.len(),
            "capacity": ring.capacity(),
        }))
    }

    /// `a11y_action` — dispatches a `SemanticAction` through the probe.
    pub fn a11y_action(&self, params: &Value) -> SessionResult {
        self.probe
            .lock()
            .expect("probe mutex")
            .probe_a11y_action(params)
    }

    /// `hot_reload` — writes the reload-request marker for the dev
    /// coordinator to pick up.
    ///
    /// `serve_dev_session` registers a marker path next to the session
    /// socket; `cargo martensite dev` polls for it and runs a reload
    /// cycle. A session without a path was never wired for coordinator
    /// reload and reports `unavailable` (not `not_implemented`).
    pub fn hot_reload(&self, params: &Value) -> SessionResult {
        let Some(path) = self.reload_request_path() else {
            return Err(HOT_RELOAD_UNAVAILABLE.to_string());
        };
        let marker = json!({
            "requested_at": super::iso_now(),
            "reason": params.get("reason").and_then(Value::as_str),
            "pid": std::process::id(),
        });
        std::fs::write(&path, marker.to_string())
            .map_err(|e| format!("hot_reload: cannot write marker {}: {e}", path.display()))?;
        Ok(json!({
            "requested": true,
            "mechanism": "coordinator_marker",
            "marker": path,
        }))
    }
}

/// Lowercase wire severity for a [`DiagnosticSeverity`].
fn severity_str(severity: DiagnosticSeverity) -> &'static str {
    match severity {
        DiagnosticSeverity::Info => "info",
        DiagnosticSeverity::Warn => "warning",
        DiagnosticSeverity::Error => "error",
        DiagnosticSeverity::Fatal => "fatal",
    }
}

/// Normalizes a `severity` filter: lowercase, `warn` → `warning`.
fn normalize_severity(filter: &str) -> String {
    match filter.to_ascii_lowercase().as_str() {
        "warn" => "warning".to_string(),
        other => other.to_string(),
    }
}

/// Serializes a [`CrashBundle`] as a `kind:"panic"` error record.
/// `source` distinguishes the global panic hook from a session-attached
/// [`ErrorSurface`](crate::error_surface::ErrorSurface) capture.
fn panic_record(bundle: &CrashBundle, source: &str) -> Value {
    json!({
        "kind": "panic",
        "severity": "fatal",
        "source": source,
        "message": truncated(&bundle.panic_message),
        "in_flight_node": bundle.in_flight_node.map(WidgetId::to_u64),
        "in_flight_path": bundle.in_flight_path,
        "phase": bundle.phase.as_str(),
        "policy": bundle.policy.as_str(),
        "backtrace": bundle.backtrace,
        "recent_events": bundle.recent_events,
        "timestamp_ns": bundle.timestamp_ns,
    })
}

/// Copies `message` capped at the log-ring's 1024-char budget.
fn truncated(message: &str) -> String {
    let mut owned = message.to_string();
    truncate_message(&mut owned);
    owned
}

#[cfg(test)]
mod tests {
    use std::sync::atomic::{AtomicU64, Ordering};
    use std::sync::Arc;

    use serde_json::json;

    use super::super::log_ring::{LogRecord, LogRing};
    use super::super::NullProbe;
    use super::*;
    use crate::error_surface::{
        DiagnosticSeverity as Severity, ErrorSurface, LintDiagnostic, PanicPhase,
    };

    fn session() -> DevSession {
        DevSession::new(Box::new(NullProbe))
    }

    fn record(level: &str, target: &str, message: &str) -> LogRecord {
        LogRecord {
            seq: 0,
            level: level.to_string(),
            target: target.to_string(),
            message: message.to_string(),
            file: None,
            line: None,
            timestamp: "2026-01-01T00:00:00Z".to_string(),
        }
    }

    #[test]
    fn runtime_errors_empty_when_nothing_recorded() {
        let s = session();
        // Isolate from other tests sharing the process-wide sources.
        let _ = crate::error_surface::take_last_panic();
        martensite_reactive::ReactiveRuntime::current().clear_errors();

        let v = s.runtime_errors(&json!({})).expect("runtime_errors");
        assert_eq!(v["errors"], json!([]));
        assert_eq!(v["total"], 0);
        assert_eq!(v["panic_active"], false);
        assert_eq!(v["surface_attached"], false);
    }

    #[test]
    fn runtime_errors_reports_surface_crash_and_diagnostics() {
        let s = session();
        let mut surface = ErrorSurface::new();
        surface.capture_panic(
            "render target unbound",
            None,
            "App/Viewport",
            PanicPhase::Paint,
            None,
            0,
        );
        surface.record_lint_diagnostic(LintDiagnostic::new(
            None,
            "App/Button",
            "min-size",
            "height below minimum",
            Severity::Warn,
            kurbo::Rect::ZERO,
        ));
        s.set_error_surface(surface);

        let v = s.runtime_errors(&json!({})).expect("runtime_errors");
        assert_eq!(v["surface_attached"], true);
        assert_eq!(v["panic_active"], true);
        let errors = v["errors"].as_array().expect("errors array");

        let panic = errors.iter().find(|e| e["kind"] == "panic").expect("panic");
        assert_eq!(panic["severity"], "fatal");
        assert_eq!(panic["source"], "error_surface");
        assert_eq!(panic["message"], "render target unbound");
        assert_eq!(panic["in_flight_path"], "App/Viewport");
        assert_eq!(panic["phase"], "Paint");

        let diag = errors
            .iter()
            .find(|e| e["kind"] == "diagnostic")
            .expect("diagnostic");
        assert_eq!(diag["severity"], "warning");
        assert_eq!(diag["node_path"], "App/Button");
        assert!(diag["message"]
            .as_str()
            .is_some_and(|m| m.contains("height below minimum")));
    }

    #[test]
    fn runtime_errors_severity_filter_matches_with_warn_alias() {
        let s = session();
        let mut surface = ErrorSurface::new();
        surface.capture_panic("boom", None, "Root", PanicPhase::Unknown, None, 0);
        surface.record_lint_diagnostic(LintDiagnostic::new(
            None,
            "Card",
            "contrast",
            "fails",
            Severity::Warn,
            kurbo::Rect::ZERO,
        ));
        s.set_error_surface(surface);

        let v = s
            .runtime_errors(&json!({"severity": "fatal"}))
            .expect("filtered");
        let errors = v["errors"].as_array().expect("array");
        assert!(!errors.is_empty());
        assert!(errors.iter().all(|e| e["severity"] == "fatal"));

        // `warn` is an alias for the wire's `warning`.
        let v = s
            .runtime_errors(&json!({"severity": "warn"}))
            .expect("filtered");
        let errors = v["errors"].as_array().expect("array");
        assert_eq!(errors.len(), 1);
        assert_eq!(errors[0]["severity"], "warning");

        // Case-insensitive.
        let v = s
            .runtime_errors(&json!({"severity": "WARNING"}))
            .expect("filtered");
        assert_eq!(v["errors"].as_array().expect("array").len(), 1);
    }

    #[test]
    fn runtime_errors_clear_drains_sources() {
        let s = session();
        let mut surface = ErrorSurface::new();
        surface.capture_panic("boom", None, "Root", PanicPhase::Unknown, None, 0);
        s.set_error_surface(surface);

        let v = s
            .runtime_errors(&json!({"clear": true}))
            .expect("runtime_errors");
        assert!(v["errors"]
            .as_array()
            .expect("array")
            .iter()
            .any(|e| e["kind"] == "panic"));

        let v = s.runtime_errors(&json!({})).expect("post-clear");
        assert_eq!(v["panic_active"], false);
        assert!(s
            .error_surface()
            .as_ref()
            .expect("surface")
            .last_crash()
            .is_none());
    }

    #[test]
    fn runtime_errors_limit_clamps() {
        let s = session();
        let v = s
            .runtime_errors(&json!({"limit": 99999}))
            .expect("runtime_errors");
        assert_eq!(v["total"], v["errors"].as_array().expect("array").len());
    }

    #[test]
    fn logs_without_ring_is_not_implemented() {
        let s = session();
        let err = s.logs(&json!({})).expect_err("no ring attached");
        assert_eq!(err, "not_implemented:logs");
    }

    #[test]
    fn logs_tails_ring_with_filters() {
        let s = session();
        let ring = Arc::new(LogRing::new(16));
        ring.push(record("INFO", "app::boot", "boot complete"));
        ring.push(record("WARN", "app::mem", "low memory"));
        ring.push(record("ERROR", "net::io", "socket closed"));
        s.set_log_ring(Arc::clone(&ring));

        let v = s.logs(&json!({})).expect("logs");
        assert_eq!(v["total_buffered"], 3);
        assert_eq!(v["capacity"], 16);
        let arr = v["logs"].as_array().expect("array");
        assert_eq!(arr.len(), 3);
        assert_eq!(arr[0]["seq"], 0);
        assert_eq!(arr[0]["target"], "app::boot");

        // `level` is a minimum severity.
        let v = s.logs(&json!({"level": "warn"})).expect("logs");
        let arr = v["logs"].as_array().expect("array");
        assert_eq!(arr.len(), 2);
        assert!(arr.iter().all(|r| r["level"] != "INFO"));

        let v = s.logs(&json!({"target_prefix": "app::"})).expect("logs");
        assert_eq!(v["logs"].as_array().expect("array").len(), 2);

        // `contains` is case-insensitive.
        let v = s.logs(&json!({"contains": "MEMORY"})).expect("logs");
        let arr = v["logs"].as_array().expect("array");
        assert_eq!(arr.len(), 1);
        assert_eq!(arr[0]["message"], "low memory");

        // `limit` keeps the newest records.
        let v = s.logs(&json!({"limit": 1})).expect("logs");
        let arr = v["logs"].as_array().expect("array");
        assert_eq!(arr.len(), 1);
        assert_eq!(arr[0]["message"], "socket closed");
    }

    #[test]
    fn log_ring_layer_captures_events() {
        use tracing_subscriber::prelude::*;

        let ring = Arc::new(LogRing::new(8));
        let subscriber = tracing_subscriber::registry().with(ring.layer());
        tracing::subscriber::with_default(subscriber, || {
            tracing::info!(target: "martensite::test", answer = 42_u64, "hello world");
            tracing::warn!("fields only");
        });

        let recs = ring.tail(8, None, None, None);
        assert_eq!(recs.len(), 2);
        let first = &recs[0];
        assert_eq!(first.level, "INFO");
        assert_eq!(first.target, "martensite::test");
        assert!(first.message.contains("hello world"), "{}", first.message);
        assert!(first.message.contains("answer=42"), "{}", first.message);
        assert!(first.file.is_some());
        assert!(!first.timestamp.is_empty());
        assert_eq!(recs[1].level, "WARN");
        assert!(
            recs[1].message.contains("fields only"),
            "{}",
            recs[1].message
        );
    }

    #[test]
    fn log_ring_truncates_long_messages() {
        let ring = LogRing::new(4);
        ring.push(record("INFO", "t", &"x".repeat(2048)));
        let recs = ring.tail(1, None, None, None);
        let msg = &recs[0].message;
        assert!(msg.ends_with('…'));
        assert_eq!(msg.chars().count(), 1025);
    }

    #[test]
    fn hot_reload_without_marker_path_is_unavailable() {
        let s = session();
        let err = s.hot_reload(&json!({})).expect_err("no marker path");
        assert!(err.starts_with("unavailable:"), "{err}");
        assert!(!err.starts_with("not_implemented:"), "{err}");
    }

    #[test]
    fn hot_reload_writes_coordinator_marker() {
        static COUNTER: AtomicU64 = AtomicU64::new(0);
        let dir = std::env::temp_dir().join(format!(
            "martensite-devtools-test-{}-{}",
            std::process::id(),
            COUNTER.fetch_add(1, Ordering::Relaxed)
        ));
        std::fs::create_dir_all(&dir).expect("temp dir");
        let path = dir.join("session.reload-request");

        let s = session();
        s.set_reload_request_path(path.clone());
        let v = s
            .hot_reload(&json!({"reason": "fix button"}))
            .expect("hot_reload");
        assert_eq!(v["requested"], true);
        assert_eq!(v["mechanism"], "coordinator_marker");
        assert_eq!(v["marker"], json!(path));

        let body: Value =
            serde_json::from_str(&std::fs::read_to_string(&path).expect("marker written"))
                .expect("marker json");
        assert_eq!(body["reason"], "fix button");
        assert_eq!(body["pid"], std::process::id());
        assert!(body["requested_at"].is_string());
        let _ = std::fs::remove_file(&path);
        let _ = std::fs::remove_dir(&dir);
    }

    #[test]
    fn hot_reload_write_failure_reports_path() {
        let s = session();
        // A path inside a nonexistent directory cannot be written.
        let path = std::env::temp_dir()
            .join("martensite-no-such-dir-9e31")
            .join("marker");
        s.set_reload_request_path(path.clone());
        let err = s.hot_reload(&json!({})).expect_err("write must fail");
        assert!(err.starts_with("hot_reload: cannot write marker"), "{err}");
        assert!(err.contains("marker"), "{err}");
    }
}
