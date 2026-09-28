//! End-to-end tests for the ADR-0038 dev-channel → `DevSession` bridge
//! (`session_handler` module). Exercises the real socket transport: a
//! `DevSession` built on `NullProbe` is wrapped in
//! `SessionDevChannelHandler`, served by `DevChannelServer`, and queried by
//! `DevChannelClient` after the mandatory `hello` handshake.
//!
//! Method coverage note: extension methods without typed dispatcher arms
//! (`reload_status`, `tree_node`, `tweaks_list`, `theme_get`, …) are
//! bridged through `DevChannelHandler::handle_custom`, so they are
//! exercised here over the wire today. `not_implemented:` session errors
//! surface as `-32601` on that path; for the typed dispatcher arms the
//! same mapping is applied inside `dev_channel::process_request_line`.

#![cfg(feature = "dev-channel")]
#![forbid(unsafe_code)]

use std::sync::Arc;

use martensite_devtools::dev_session::log_ring::{LogRecord, LogRing};
use martensite_devtools::dev_session::{DevSession, NullProbe, SignalAdapter};
use martensite_host::dev_channel::{
    socket_path_for_session, DevChannelClient, DevChannelConfig, DevChannelServer,
    EventLedgerParams, JsonRpcRequest, JsonRpcResponse, TreeSnapshotParams,
    DEV_CHANNEL_PROTOCOL_VERSION, ERR_HANDSHAKE_REQUIRED, ERR_INTERNAL, ERR_METHOD_NOT_FOUND,
    MARTENSITE_VERSION,
};
use martensite_host::session_handler::{serve_dev_session, SessionDevChannelHandler};
use tempfile::tempdir;

/// Builds a session handler around a fresh `NullProbe`-backed session.
fn null_probe_handler() -> SessionDevChannelHandler {
    SessionDevChannelHandler::new(Arc::new(DevSession::new(Box::new(NullProbe))))
}

/// Starts a dev-channel server on an explicit socket path inside `dir`.
fn start_handler_server(sock_path: &std::path::Path) -> DevChannelServer {
    DevChannelConfig::new()
        .with_socket_path(sock_path)
        .with_handler(Arc::new(null_probe_handler()))
        .start()
        .expect("server starts")
}

/// Connects a client and performs the mandatory `hello` handshake.
fn connect_handshook(sock_path: &std::path::Path) -> DevChannelClient {
    let mut client = DevChannelClient::connect(sock_path).expect("client connects");
    let hello = client
        .hello(MARTENSITE_VERSION, DEV_CHANNEL_PROTOCOL_VERSION)
        .expect("hello io")
        .expect("handshake succeeds");
    assert_eq!(hello.server_version, MARTENSITE_VERSION);
    assert_eq!(hello.protocol_version, DEV_CHANNEL_PROTOCOL_VERSION);
    client
}

/// Sends `method` with `params` as a raw request and returns the response.
fn call(client: &mut DevChannelClient, method: &str, params: serde_json::Value) -> JsonRpcResponse {
    let req = JsonRpcRequest::new(Some(100), method, params);
    client.send_raw(&req).expect("raw call io")
}

/// Asserts a `result`-carrying response and returns the payload.
fn ok_result(resp: JsonRpcResponse, method: &str) -> serde_json::Value {
    assert!(
        resp.error.is_none(),
        "{method} errored: {:?}",
        resp.error.as_ref().map(|e| &e.message)
    );
    resp.result.expect("{method} must return a result")
}

/// Asserts an `error`-carrying response with `code` and returns the message.
fn err_with_code(resp: JsonRpcResponse, method: &str, code: i32) -> String {
    let err = resp
        .error
        .unwrap_or_else(|| panic!("{method} must return an error"));
    assert_eq!(err.code, code, "{method} error code");
    err.message
}

/// Monotonic-ish unique suffix for socket names without extra deps.
fn unique_nanos() -> u128 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or(0)
}

#[test]
fn serve_dev_session_binds_and_handshakes() {
    let build_id = format!("e2e_serve_{}_{}", std::process::id(), unique_nanos());
    let sock_path = socket_path_for_session(&build_id);

    let session = Arc::new(DevSession::new(Box::new(NullProbe)));
    let mut server = serve_dev_session(session, &build_id).expect("serve_dev_session binds");
    assert!(server.is_running());
    assert_eq!(server.socket_path(), sock_path.as_path());

    let mut client = DevChannelClient::connect(&sock_path).expect("client connects");
    let hello = client
        .hello(MARTENSITE_VERSION, DEV_CHANNEL_PROTOCOL_VERSION)
        .expect("hello io")
        .expect("handshake succeeds");
    assert_eq!(hello.server_version, MARTENSITE_VERSION);
    assert_eq!(hello.build_id.as_deref(), Some(build_id.as_str()));

    server.stop();
    assert!(!server.is_running());
    assert!(
        !sock_path.exists(),
        "socket file must be unlinked on stop: {}",
        sock_path.display()
    );
}

#[test]
fn handshake_required_before_requests() {
    let tmp = tempdir().expect("tempdir");
    let sock_path = tmp.path().join("e2e_handshake.sock");
    let _server = start_handler_server(&sock_path);

    let mut client = DevChannelClient::connect(&sock_path).expect("client connects");
    let resp = call(&mut client, "reload_status", serde_json::json!({}));
    err_with_code(resp, "reload_status", ERR_HANDSHAKE_REQUIRED);
}

#[test]
fn reload_status_reports_inactive_session() {
    let tmp = tempdir().expect("tempdir");
    let sock_path = tmp.path().join("e2e_reload.sock");
    let _server = start_handler_server(&sock_path);
    let mut client = connect_handshook(&sock_path);

    let result = ok_result(
        call(&mut client, "reload_status", serde_json::json!({})),
        "reload_status",
    );
    assert_eq!(result["active"], false);
    assert_eq!(result["total_reloads"], 0);
    assert!(result["active_build_id"].is_null());
}

#[test]
fn reload_status_reflects_recorded_reload() {
    let tmp = tempdir().expect("tempdir");
    let sock_path = tmp.path().join("e2e_reload_live.sock");

    // Keep a handle to the session so the test can mutate shared state.
    let session = Arc::new(DevSession::new(Box::new(NullProbe)));
    let handler = SessionDevChannelHandler::new(Arc::clone(&session));
    let _server = DevChannelConfig::new()
        .with_socket_path(&sock_path)
        .with_handler(Arc::new(handler))
        .start()
        .expect("server starts");
    let mut client = connect_handshook(&sock_path);

    session.record_reload("build_99");

    let result = ok_result(
        call(&mut client, "reload_status", serde_json::json!({})),
        "reload_status",
    );
    assert_eq!(result["active"], true);
    assert_eq!(result["active_build_id"], "build_99");
    assert_eq!(result["total_reloads"], 1);
    assert!(result["last_reload_timestamp"].is_string());
}

#[test]
fn event_ledger_returns_empty_tail() {
    let tmp = tempdir().expect("tempdir");
    let sock_path = tmp.path().join("e2e_ledger.sock");
    let _server = start_handler_server(&sock_path);
    let mut client = connect_handshook(&sock_path);

    let result = ok_result(
        call(
            &mut client,
            "event_ledger",
            serde_json::json!({ "tail_count": 50 }),
        ),
        "event_ledger",
    );
    assert_eq!(result["events"], serde_json::json!([]));
    assert_eq!(result["total"], 0);
}

#[test]
fn event_dispatch_records_ledger_even_when_probe_unimplemented() {
    let tmp = tempdir().expect("tempdir");
    let sock_path = tmp.path().join("e2e_dispatch.sock");
    let _server = start_handler_server(&sock_path);
    let mut client = connect_handshook(&sock_path);

    // NullProbe cannot route input: the dispatch itself reports -32601.
    let resp = call(
        &mut client,
        "event_dispatch",
        serde_json::json!({ "event_type": "pointer_click" }),
    );
    err_with_code(resp, "event_dispatch", ERR_METHOD_NOT_FOUND);

    // The attempt is still journaled into the session ledger (spec §3.15).
    let result = ok_result(
        call(&mut client, "event_ledger", serde_json::json!({})),
        "event_ledger",
    );
    assert_eq!(result["total"], 1);
    let events = result["events"].as_array().expect("events array");
    assert_eq!(events.len(), 1);
}

#[test]
fn event_dispatch_rejects_unknown_event_type() {
    let tmp = tempdir().expect("tempdir");
    let sock_path = tmp.path().join("e2e_dispatch_bad.sock");
    let _server = start_handler_server(&sock_path);
    let mut client = connect_handshook(&sock_path);

    // Param validation happens in the session before the probe is touched:
    // it is an internal/invalid error, not a missing capability.
    let resp = call(
        &mut client,
        "event_dispatch",
        serde_json::json!({ "event_type": "teleport" }),
    );
    let msg = err_with_code(resp, "event_dispatch", ERR_INTERNAL);
    assert!(msg.contains("event_type"), "{msg}");
}

#[test]
fn tree_snapshot_reports_missing_capability_as_method_not_found() {
    let tmp = tempdir().expect("tempdir");
    let sock_path = tmp.path().join("e2e_tree.sock");
    let _server = start_handler_server(&sock_path);
    let mut client = connect_handshook(&sock_path);

    let res = client
        .tree_snapshot(TreeSnapshotParams::default())
        .expect("tree_snapshot io");
    let err = res.expect_err("NullProbe cannot snapshot a tree");
    assert_eq!(err.code, ERR_METHOD_NOT_FOUND);
    assert!(
        err.message.starts_with("not_implemented:"),
        "error must carry the not_implemented marker: {}",
        err.message
    );
}

#[test]
fn extension_methods_bridge_through_handle_custom() {
    let tmp = tempdir().expect("tempdir");
    let sock_path = tmp.path().join("e2e_ext.sock");
    let _server = start_handler_server(&sock_path);
    let mut client = connect_handshook(&sock_path);

    // `tweaks_list` has real session state even without an app probe.
    let result = ok_result(
        call(&mut client, "tweaks_list", serde_json::json!({})),
        "tweaks_list",
    );
    assert_eq!(result["tweaks"], serde_json::json!([]));
    assert_eq!(result["revision"], 0);

    // Probe-only capabilities honestly report `-32601` under NullProbe.
    // `signals_list` is excluded: the session has a reactive-runtime
    // fallback that succeeds whenever the `devtools-timemachine`
    // feature is compiled in (feature unification toggles it), so its
    // contract is "Ok with a signals array, or -32601 without one".
    for method in [
        "tree_node",
        "layout_chain",
        "overflow_scan",
        "a11y_tree",
        "theme_get",
        "audit_paint",
    ] {
        let resp = call(&mut client, method, serde_json::json!({}));
        err_with_code(resp, method, ERR_METHOD_NOT_FOUND);
    }
    let resp = call(&mut client, "signals_list", serde_json::json!({}));
    if let Some(result) = &resp.result {
        assert!(
            result["signals"].is_array(),
            "signals_list must carry a signals array: {result}"
        );
    } else {
        err_with_code(resp, "signals_list", ERR_METHOD_NOT_FOUND);
    }

    // `capture_node` needs a valid `format` to clear session-side param
    // validation and reach the probe (`""` from the typed params' default
    // is rejected as unsupported before the probe is consulted).
    let resp = call(
        &mut client,
        "capture_node",
        serde_json::json!({ "node_id": 1, "format": "png" }),
    );
    err_with_code(resp, "capture_node", ERR_METHOD_NOT_FOUND);

    // `timemachine_step` validates `action`, then reports the feature gap
    // (devtools-timemachine is off) as a plain error, not `not_implemented`.
    let resp = call(
        &mut client,
        "timemachine_step",
        serde_json::json!({ "action": "pause" }),
    );
    let msg = err_with_code(resp, "timemachine_step", ERR_INTERNAL);
    assert!(msg.contains("timemachine"), "{msg}");

    // `signal_trigger` validates `signal_id` before touching the probe.
    let resp = call(&mut client, "signal_trigger", serde_json::json!({}));
    err_with_code(resp, "signal_trigger", ERR_INTERNAL);

    let resp = call(
        &mut client,
        "signal_trigger",
        serde_json::json!({ "signal_id": "sig-1", "value": 1 }),
    );
    err_with_code(resp, "signal_trigger", ERR_METHOD_NOT_FOUND);
}

#[test]
fn event_ledger_typed_client_helper_round_trips() {
    let tmp = tempdir().expect("tempdir");
    let sock_path = tmp.path().join("e2e_typed.sock");
    let _server = start_handler_server(&sock_path);
    let mut client = connect_handshook(&sock_path);

    // The typed `DevChannelClient::event_ledger` helper hits the same
    // session bridge as the raw `event_ledger` method name.
    let res = client
        .event_ledger(EventLedgerParams {
            tail_count: Some(10),
            ..Default::default()
        })
        .expect("event_ledger io");
    let value = res.expect("event_ledger must succeed for a fresh session");
    assert_eq!(value["events"], serde_json::json!([]));
    assert_eq!(value["total"], 0);
}

#[test]
fn runtime_errors_returns_empty_set_on_fresh_session() {
    let tmp = tempdir().expect("tempdir");
    let sock_path = tmp.path().join("e2e_runtime_errors.sock");
    let _server = start_handler_server(&sock_path);
    let mut client = connect_handshook(&sock_path);

    // `runtime_errors` merges process-global sources (dev-panic hook
    // bundle, ambient reactive-runtime errors) that sibling tests in
    // this binary could have populated; drain them via `clear` first so
    // the fresh-session shape is asserted deterministically.
    let _ = ok_result(
        call(
            &mut client,
            "runtime_errors",
            serde_json::json!({ "clear": true }),
        ),
        "runtime_errors",
    );

    let result = ok_result(
        call(&mut client, "runtime_errors", serde_json::json!({})),
        "runtime_errors",
    );
    assert_eq!(result["errors"], serde_json::json!([]));
    assert_eq!(result["total"], 0);
    assert_eq!(result["panic_active"], false);
    assert_eq!(result["surface_attached"], false);
}

#[test]
fn logs_reports_not_implemented_without_attached_ring() {
    let tmp = tempdir().expect("tempdir");
    let sock_path = tmp.path().join("e2e_logs_no_ring.sock");
    let _server = start_handler_server(&sock_path);
    let mut client = connect_handshook(&sock_path);

    // No `LogRing` was attached to the session: the capability is
    // genuinely absent and degrades honestly to -32601, not an empty
    // list that would pretend the app simply logged nothing.
    let resp = call(&mut client, "logs", serde_json::json!({}));
    let msg = err_with_code(resp, "logs", ERR_METHOD_NOT_FOUND);
    assert_eq!(msg, "not_implemented:logs");
}

#[test]
fn logs_tails_attached_ring_over_the_wire() {
    let tmp = tempdir().expect("tempdir");
    let sock_path = tmp.path().join("e2e_logs_ring.sock");

    let session = Arc::new(DevSession::new(Box::new(NullProbe)));
    let ring = Arc::new(LogRing::new(16));
    let record = |level: &str, target: &str, message: &str| LogRecord {
        seq: 0,
        level: level.to_string(),
        target: target.to_string(),
        message: message.to_string(),
        file: None,
        line: None,
        timestamp: "2026-01-01T00:00:00Z".to_string(),
    };
    ring.push(record("INFO", "e2e::boot", "boot complete"));
    ring.push(record("WARN", "e2e::mem", "low memory"));
    ring.push(record("ERROR", "e2e::io", "socket closed"));
    session.set_log_ring(Arc::clone(&ring));

    let handler = SessionDevChannelHandler::new(Arc::clone(&session));
    let _server = DevChannelConfig::new()
        .with_socket_path(&sock_path)
        .with_handler(Arc::new(handler))
        .start()
        .expect("server starts");
    let mut client = connect_handshook(&sock_path);

    let result = ok_result(call(&mut client, "logs", serde_json::json!({})), "logs");
    assert_eq!(result["total_buffered"], 3);
    assert_eq!(result["capacity"], 16);
    let logs = result["logs"].as_array().expect("logs array");
    assert_eq!(logs.len(), 3);
    assert_eq!(logs[0]["target"], "e2e::boot");
    assert_eq!(logs[0]["message"], "boot complete");

    // `level` is a minimum severity filter applied inside the ring.
    let result = ok_result(
        call(&mut client, "logs", serde_json::json!({ "level": "warn" })),
        "logs",
    );
    let logs = result["logs"].as_array().expect("logs array");
    assert_eq!(logs.len(), 2);
    assert!(logs.iter().all(|r| r["level"] != "INFO"));
}

#[test]
fn a11y_action_maps_null_probe_to_method_not_found() {
    let tmp = tempdir().expect("tempdir");
    let sock_path = tmp.path().join("e2e_a11y_action.sock");
    let _server = start_handler_server(&sock_path);
    let mut client = connect_handshook(&sock_path);

    let resp = call(
        &mut client,
        "a11y_action",
        serde_json::json!({ "node_id": "1", "action": "click" }),
    );
    let msg = err_with_code(resp, "a11y_action", ERR_METHOD_NOT_FOUND);
    assert!(
        msg.starts_with("not_implemented:"),
        "NullProbe must report a capability gap: {msg}"
    );
}

#[test]
fn hot_reload_writes_coordinator_marker_when_served() {
    let build_id = format!("e2e_reload_{}_{}", std::process::id(), unique_nanos());
    let sock_path = socket_path_for_session(&build_id);
    let marker_path = std::path::PathBuf::from(format!("{}.reload-request", sock_path.display()));
    // Clear a stale marker from an earlier run before serving.
    let _ = std::fs::remove_file(&marker_path);

    let session = Arc::new(DevSession::new(Box::new(NullProbe)));
    let mut server =
        serve_dev_session(Arc::clone(&session), &build_id).expect("serve_dev_session binds");

    // `serve_dev_session` wired the marker path next to the bound socket.
    assert_eq!(
        session.reload_request_path().as_deref(),
        Some(marker_path.as_path()),
        "reload-request marker path must sit next to the socket"
    );

    let mut client = connect_handshook(&sock_path);
    let result = ok_result(
        call(
            &mut client,
            "hot_reload",
            serde_json::json!({ "reason": "e2e probe" }),
        ),
        "hot_reload",
    );
    assert_eq!(result["requested"], true);
    assert_eq!(result["mechanism"], "coordinator_marker");

    assert!(
        marker_path.exists(),
        "marker file must be written: {}",
        marker_path.display()
    );
    let body: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string(&marker_path).expect("marker file readable"))
            .expect("marker file is JSON");
    assert_eq!(body["reason"], "e2e probe");
    assert!(
        body["requested_at"].is_string(),
        "marker carries an RFC 3339 requested_at: {body}"
    );

    let _ = std::fs::remove_file(&marker_path);
    server.stop();
}

#[test]
fn hot_reload_without_serve_wiring_reports_unavailable() {
    // A session served through a bare `DevChannelServer` never got a
    // coordinator marker path — `hot_reload` exists but cannot act, so
    // it reports `unavailable` as an internal error, not -32601.
    let tmp = tempdir().expect("tempdir");
    let sock_path = tmp.path().join("e2e_hot_reload_unserved.sock");
    let _server = start_handler_server(&sock_path);
    let mut client = connect_handshook(&sock_path);

    let resp = call(&mut client, "hot_reload", serde_json::json!({}));
    let msg = err_with_code(resp, "hot_reload", ERR_INTERNAL);
    assert!(msg.starts_with("unavailable:"), "{msg}");
}

#[test]
fn signal_set_reports_missing_adapter_under_null_probe() {
    let tmp = tempdir().expect("tempdir");
    let sock_path = tmp.path().join("e2e_signal_set.sock");
    let _server = start_handler_server(&sock_path);
    let mut client = connect_handshook(&sock_path);

    // An empty `signal_id` fails session-side param validation before the
    // adapter lookup — a plain internal error, not a capability gap.
    let resp = call(&mut client, "signal_set", serde_json::json!({}));
    let msg = err_with_code(resp, "signal_set", ERR_INTERNAL);
    assert!(msg.contains("signal_id"), "{msg}");

    // A named signal with no registered `SignalAdapter` reports the
    // registration gap as an internal error (the method exists; the
    // per-signal write capability does not — ADR-0039 D9 honesty).
    let resp = call(
        &mut client,
        "signal_set",
        serde_json::json!({ "signal_id": "counter", "value": 42 }),
    );
    let msg = err_with_code(resp, "signal_set", ERR_INTERNAL);
    assert!(msg.contains("no signal adapter registered"), "{msg}");
    assert!(msg.contains("counter"), "{msg}");
}

#[test]
fn signal_set_writes_through_registered_adapter() {
    let tmp = tempdir().expect("tempdir");
    let sock_path = tmp.path().join("e2e_signal_set_ok.sock");

    let session = Arc::new(DevSession::new(Box::new(NullProbe)));
    session.register_signal_adapter(SignalAdapter {
        name: "counter".to_string(),
        signal_id: None,
        read_json: Box::new(|| Some(serde_json::json!(0))),
        write_json: Box::new(|value| {
            if value.is_number() {
                Ok(())
            } else {
                Err("counter expects a number".to_string())
            }
        }),
    });
    let handler = SessionDevChannelHandler::new(Arc::clone(&session));
    let _server = DevChannelConfig::new()
        .with_socket_path(&sock_path)
        .with_handler(Arc::new(handler))
        .start()
        .expect("server starts");
    let mut client = connect_handshook(&sock_path);

    let result = ok_result(
        call(
            &mut client,
            "signal_set",
            serde_json::json!({ "signal_id": "counter", "value": 7 }),
        ),
        "signal_set",
    );
    assert_eq!(result["applied"], true);
    assert_eq!(result["signal"], "counter");

    // Adapter codec rejections propagate as internal errors.
    let resp = call(
        &mut client,
        "signal_set",
        serde_json::json!({ "signal_id": "counter", "value": "not a number" }),
    );
    let msg = err_with_code(resp, "signal_set", ERR_INTERNAL);
    assert!(msg.contains("expects a number"), "{msg}");
}

#[test]
fn unknown_method_reports_method_not_found() {
    let tmp = tempdir().expect("tempdir");
    let sock_path = tmp.path().join("e2e_unknown.sock");
    let _server = start_handler_server(&sock_path);
    let mut client = connect_handshook(&sock_path);

    let resp = call(&mut client, "bogus_capability", serde_json::json!({}));
    let msg = err_with_code(resp, "bogus_capability", ERR_METHOD_NOT_FOUND);
    assert!(msg.contains("method not found"), "{msg}");
    // The dispatcher lowercases and strips separators before custom routing.
    assert!(msg.contains("boguscapability"), "{msg}");
}

/// `node_set_loading` (ADR-0040 phase 3) over the real socket: the request
/// mutates the live arena through `WidgetArenaProbe`, and the effects are
/// observable through the existing `a11y_tree`/`tree_node` reads.
#[test]
fn node_set_loading_round_trips_over_socket() {
    use std::sync::Mutex;

    use martensite_core::{DummyWidget, HotNode, NodeFlags, Rect, WidgetArena, WidgetId};

    let tmp = tempdir().expect("tempdir");
    let sock_path = tmp.path().join("e2e_node_set_loading.sock");

    // Real arena: a visible container root with a named focusable child.
    let mut arena = WidgetArena::new();
    let root = arena.insert_with_widget(
        HotNode {
            bounds: Rect::new(0.0, 0.0, 800.0, 600.0),
            flags: NodeFlags::VISIBLE | NodeFlags::HIT_TEST_ENABLED,
            ..Default::default()
        },
        Box::new(DummyWidget),
    );
    let child = arena.insert_with_widget(
        HotNode {
            bounds: Rect::new(8.0, 8.0, 120.0, 24.0),
            flags: NodeFlags::VISIBLE | NodeFlags::FOCUSABLE | NodeFlags::HIT_TEST_ENABLED,
            ..Default::default()
        },
        Box::new(DummyWidget),
    );
    arena.get_cold_mut(child).expect("child cold").debug_name = Some("PendingRow");
    arena.append_child(root, child).expect("append child");
    let arena = Arc::new(Mutex::new(arena));

    let session = Arc::new(DevSession::with_arena(Arc::clone(&arena)));
    let _server = DevChannelConfig::new()
        .with_socket_path(&sock_path)
        .with_handler(Arc::new(SessionDevChannelHandler::new(Arc::clone(
            &session,
        ))))
        .start()
        .expect("server starts");
    let mut client = connect_handshook(&sock_path);

    // Force loading over the wire — the mutation response echoes the
    // canonical id, the effective state, and a stamped revision.
    let result = ok_result(
        call(
            &mut client,
            "node_set_loading",
            serde_json::json!({ "node_id": child.to_u64(), "loading": true }),
        ),
        "node_set_loading",
    );
    assert_eq!(result["applied"], true);
    assert_eq!(result["node_id"], child.to_u64());
    assert_eq!(result["loading"], true);
    assert!(
        result["revision"].as_u64().is_some_and(|r| r > 0),
        "revision must be stamped: {result}"
    );
    assert!(arena.lock().expect("arena").node_loading(child));

    // `a11y_tree` shows the sanitized busy node the AccessKit adapter
    // would emit: label "Loading", busy + disabled, subtree pruned.
    let a11y = ok_result(
        call(&mut client, "a11y_tree", serde_json::json!({})),
        "a11y_tree",
    );
    let kids = a11y["root"]["children"].as_array().expect("a11y children");
    let want_id = child.to_u64().to_string();
    let node = kids
        .iter()
        .find(|n| n["id"].as_str() == Some(want_id.as_str()))
        .expect("child in a11y tree");
    assert_eq!(node["name"], "Loading");
    let states: Vec<&str> = node["states"]
        .as_array()
        .expect("states")
        .iter()
        .filter_map(serde_json::Value::as_str)
        .collect();
    assert!(states.contains(&"busy"), "{node}");
    assert!(states.contains(&"disabled"), "{node}");
    assert_eq!(node["children"], serde_json::json!([]));

    // `tree_node` reports the effective flag, badge, and busy prop.
    let detail = ok_result(
        call(
            &mut client,
            "tree_node",
            serde_json::json!({ "node_id": child.to_u64() }),
        ),
        "tree_node",
    );
    assert_eq!(detail["loading"], true);
    assert!(
        detail["badges"]
            .as_array()
            .is_some_and(|b| b.iter().any(|v| v == "loading")),
        "{detail}"
    );
    assert_eq!(detail["accesskit"]["busy"], "true");

    // Clearing (via the separator-normalized method spelling) restores
    // the unobscured node.
    let result = ok_result(
        call(
            &mut client,
            "node-set-loading",
            serde_json::json!({ "node_id": child.to_u64(), "loading": false }),
        ),
        "node-set-loading",
    );
    assert_eq!(result["loading"], false);
    assert!(!arena.lock().expect("arena").node_loading(child));

    // Invalid and dead ids are honest internal errors — never a fake ok.
    let resp = call(
        &mut client,
        "node_set_loading",
        serde_json::json!({ "node_id": 0, "loading": true }),
    );
    let msg = err_with_code(resp, "node_set_loading", ERR_INTERNAL);
    assert!(msg.contains("not a valid widget id"), "{msg}");

    let dead = WidgetId::from_parts(u32::MAX - 1, 9);
    let resp = call(
        &mut client,
        "node_set_loading",
        serde_json::json!({ "node_id": dead.to_u64(), "loading": true }),
    );
    let msg = err_with_code(resp, "node_set_loading", ERR_INTERNAL);
    assert!(msg.contains("not alive"), "{msg}");
}
