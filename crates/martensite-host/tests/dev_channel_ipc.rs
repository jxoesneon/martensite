//! Integration tests for ADR-0038 Dev Channel IPC Transport.

#![cfg(feature = "dev-channel")]
#![forbid(unsafe_code)]

use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;

use martensite_host::dev_channel::{
    socket_path_for_session, A11yActionParams, DevChannelClient, DevChannelConfig,
    DevChannelHandler, DevChannelServer, DevStream, EventLedgerParams, HotReloadParams,
    InspectorSelectParams, JsonRpcRequest, LintApplyParams, LintPullParams, LogsParams,
    RuntimeErrorsParams, SignalSetParams, TreeSnapshotParams, DEV_CHANNEL_PROTOCOL_VERSION,
    ERR_HANDSHAKE_REQUIRED, ERR_INTERNAL, ERR_METHOD_NOT_FOUND, ERR_PARSE, ERR_VERSION_MISMATCH,
    MARTENSITE_VERSION,
};
use tempfile::tempdir;

#[test]
fn test_socket_path_derivation() {
    let path = socket_path_for_session("session_abc123");
    #[cfg(unix)]
    {
        assert!(path.to_string_lossy().ends_with("session_abc123.sock"));

        // Verify parent directory component is either XDG_RUNTIME_DIR or tmp
        let parent = path.parent().expect("parent directory exists");
        assert!(parent.to_string_lossy().contains("martensite"));
    }
    #[cfg(windows)]
    {
        assert!(path.to_string_lossy().contains("martensite-session_abc123"));
        assert!(path.to_string_lossy().starts_with(r"\\.\pipe\"));
    }
}

#[test]
fn test_server_creation_binding_and_cleanup() {
    let tmp = tempdir().expect("tempdir created");
    let sock_path = tmp.path().join("dev_test.sock");

    let mut server = DevChannelServer::bind(&sock_path).expect("server binds");
    assert!(server.is_running());
    assert!(sock_path.exists());

    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let metadata = std::fs::metadata(&sock_path).expect("socket metadata");
        let mode = metadata.permissions().mode() & 0o777;
        assert_eq!(mode, 0o600, "socket permissions must be user-only 0600");
    }

    server.stop();
    assert!(!server.is_running());
    assert!(!sock_path.exists(), "socket file must be unlinked on stop");
}

#[test]
fn test_server_cleanup_on_drop() {
    let tmp = tempdir().expect("tempdir created");
    let sock_path = tmp.path().join("dev_drop_test.sock");

    {
        let server = DevChannelServer::bind(&sock_path).expect("server binds");
        assert!(server.is_running());
        assert!(sock_path.exists());
    }

    assert!(
        !sock_path.exists(),
        "socket file must be cleaned up on server drop"
    );
}

#[test]
fn test_stale_socket_file_replacement() {
    let tmp = tempdir().expect("tempdir created");
    let sock_path = tmp.path().join("stale.sock");

    // Create a dummy stale file
    std::fs::write(&sock_path, b"stale socket content").expect("write dummy file");
    assert!(sock_path.exists());

    // Server should remove stale file and bind successfully
    let server = DevChannelServer::bind(&sock_path).expect("bind over stale socket succeeds");
    assert!(server.is_running());
    assert!(sock_path.exists());
}

#[test]
fn test_handshake_required_before_requests() {
    let tmp = tempdir().expect("tempdir created");
    let sock_path = tmp.path().join("handshake_req.sock");
    let _server = DevChannelServer::bind(&sock_path).expect("server binds");

    let mut client = DevChannelClient::connect(&sock_path).expect("client connects");

    // Attempt TreeSnapshot without calling hello first
    let res = client
        .tree_snapshot(TreeSnapshotParams::default())
        .expect("io succeeds");
    assert!(res.is_err(), "request without handshake must fail");
    let err = res.unwrap_err();
    assert_eq!(err.code, ERR_HANDSHAKE_REQUIRED);
    assert!(err
        .message
        .to_ascii_lowercase()
        .contains("handshake required"));

    // Attempt LintPull without calling hello
    let res = client
        .lint_pull(LintPullParams::default())
        .expect("io succeeds");
    assert!(res.is_err());
    assert_eq!(res.unwrap_err().code, ERR_HANDSHAKE_REQUIRED);
}

#[test]
fn test_successful_hello_handshake() {
    let tmp = tempdir().expect("tempdir created");
    let sock_path = tmp.path().join("hello_ok.sock");

    let config = DevChannelConfig::new()
        .with_socket_path(&sock_path)
        .with_build_id("session_build_999");
    let _server = config.start().expect("server starts");

    let mut client = DevChannelClient::connect(&sock_path).expect("client connects");
    let res = client
        .hello(MARTENSITE_VERSION, DEV_CHANNEL_PROTOCOL_VERSION)
        .expect("io succeeds")
        .expect("handshake succeeds");

    assert_eq!(res.server_version, MARTENSITE_VERSION);
    assert_eq!(res.protocol_version, DEV_CHANNEL_PROTOCOL_VERSION);
    assert_eq!(res.build_id.as_deref(), Some("session_build_999"));
}

#[test]
fn test_version_mismatch_loud_rejection() {
    let tmp = tempdir().expect("tempdir created");
    let sock_path = tmp.path().join("version_mismatch.sock");
    let _server = DevChannelServer::bind(&sock_path).expect("server binds");

    let mut client = DevChannelClient::connect(&sock_path).expect("client connects");

    // Intentionally pass an older client version to trigger Constraint D1
    let skewed_version = "0.18.0";
    let handshake_res = client
        .hello(skewed_version, DEV_CHANNEL_PROTOCOL_VERSION)
        .expect("transport succeeds");

    assert!(handshake_res.is_err(), "version mismatch must be rejected");
    let err = handshake_res.unwrap_err();

    assert_eq!(
        err.code, ERR_VERSION_MISMATCH,
        "error code must be ERR_VERSION_MISMATCH (-32001)"
    );
    assert!(
        err.message.contains("version_mismatch"),
        "error message must contain 'version_mismatch'"
    );
    assert!(
        err.message.contains(skewed_version),
        "error message must explicitly print client version"
    );
    assert!(
        err.message.contains(MARTENSITE_VERSION),
        "error message must explicitly print server version"
    );

    // Verify structured error data
    let data = err.data.expect("error data present");
    assert_eq!(data["error_type"], "version_mismatch");
    assert_eq!(data["client_version"], skewed_version);
    assert_eq!(data["server_version"], MARTENSITE_VERSION);

    // After failed handshake, subsequent requests must still be rejected
    let tree_res = client
        .tree_snapshot(TreeSnapshotParams::default())
        .expect("transport succeeds");
    assert!(tree_res.is_err());
    assert_eq!(tree_res.unwrap_err().code, ERR_HANDSHAKE_REQUIRED);
}

#[test]
fn test_protocol_version_mismatch_loud_rejection() {
    let tmp = tempdir().expect("tempdir created");
    let sock_path = tmp.path().join("proto_mismatch.sock");
    let _server = DevChannelServer::bind(&sock_path).expect("server binds");

    let mut client = DevChannelClient::connect(&sock_path).expect("client connects");

    let skewed_protocol = 999;
    let handshake_res = client
        .hello(MARTENSITE_VERSION, skewed_protocol)
        .expect("transport succeeds");

    assert!(handshake_res.is_err(), "protocol mismatch must be rejected");
    let err = handshake_res.unwrap_err();
    assert_eq!(err.code, ERR_VERSION_MISMATCH);
    assert!(err.message.contains("999"));
    assert!(err
        .message
        .contains(&DEV_CHANNEL_PROTOCOL_VERSION.to_string()));
}

/// Custom mock handler for validating request forwarding.
struct TestMockHandler {
    tree_calls: AtomicUsize,
    lint_calls: AtomicUsize,
    event_calls: AtomicUsize,
    select_calls: AtomicUsize,
    apply_calls: AtomicUsize,
}

impl TestMockHandler {
    fn new() -> Self {
        Self {
            tree_calls: AtomicUsize::new(0),
            lint_calls: AtomicUsize::new(0),
            event_calls: AtomicUsize::new(0),
            select_calls: AtomicUsize::new(0),
            apply_calls: AtomicUsize::new(0),
        }
    }
}

impl DevChannelHandler for TestMockHandler {
    fn handle_tree_snapshot(
        &self,
        params: TreeSnapshotParams,
    ) -> Result<serde_json::Value, String> {
        self.tree_calls.fetch_add(1, Ordering::SeqCst);
        Ok(serde_json::json!({
            "root_id": params.root_id,
            "max_depth": params.max_depth,
            "nodes": [
                { "id": 1, "name": "AppRoot" },
                { "id": 2, "name": "Header" }
            ]
        }))
    }

    fn handle_lint_pull(&self, params: LintPullParams) -> Result<serde_json::Value, String> {
        self.lint_calls.fetch_add(1, Ordering::SeqCst);
        Ok(serde_json::json!({
            "window_id": params.window_id,
            "report": { "findings_count": 3 }
        }))
    }

    fn handle_event_ledger(&self, params: EventLedgerParams) -> Result<serde_json::Value, String> {
        self.event_calls.fetch_add(1, Ordering::SeqCst);
        Ok(serde_json::json!({
            "tail_count": params.tail_count,
            "events": ["PointerClick", "KeyDown"]
        }))
    }

    fn handle_inspector_select(
        &self,
        params: InspectorSelectParams,
    ) -> Result<serde_json::Value, String> {
        self.select_calls.fetch_add(1, Ordering::SeqCst);
        Ok(serde_json::json!({
            "armed": params.arm,
            "selected_node_id": if params.arm { Some(42) } else { None }
        }))
    }

    fn handle_lint_apply(&self, params: LintApplyParams) -> Result<serde_json::Value, String> {
        self.apply_calls.fetch_add(1, Ordering::SeqCst);
        Ok(serde_json::json!({
            "converged": true,
            "applied_ops_count": params.ops.len(),
            "force": params.force
        }))
    }
}

#[test]
fn test_all_supported_requests_end_to_end() {
    let tmp = tempdir().expect("tempdir created");
    let sock_path = tmp.path().join("full_requests.sock");

    let mock_handler = Arc::new(TestMockHandler::new());
    let config = DevChannelConfig::new()
        .with_socket_path(&sock_path)
        .with_handler(Arc::clone(&mock_handler) as Arc<dyn DevChannelHandler>);
    let _server = config.start().expect("server starts");

    let mut client = DevChannelClient::connect(&sock_path).expect("client connects");

    // 1. Hello handshake
    let hello = client
        .hello(MARTENSITE_VERSION, DEV_CHANNEL_PROTOCOL_VERSION)
        .expect("io")
        .expect("hello ok");
    assert_eq!(hello.server_version, MARTENSITE_VERSION);

    // 2. TreeSnapshot
    let tree = client
        .tree_snapshot(TreeSnapshotParams {
            max_depth: Some(4),
            root_id: Some(100),
            ..Default::default()
        })
        .expect("io")
        .expect("tree ok");
    assert_eq!(tree["max_depth"], 4);
    assert_eq!(tree["root_id"], 100);
    assert_eq!(tree["nodes"].as_array().unwrap().len(), 2);
    assert_eq!(mock_handler.tree_calls.load(Ordering::SeqCst), 1);

    // 3. LintPull
    let lint = client
        .lint_pull(LintPullParams { window_id: Some(1) })
        .expect("io")
        .expect("lint ok");
    assert_eq!(lint["window_id"], 1);
    assert_eq!(lint["report"]["findings_count"], 3);
    assert_eq!(mock_handler.lint_calls.load(Ordering::SeqCst), 1);

    // 4. EventLedger
    let ledger = client
        .event_ledger(EventLedgerParams {
            tail_count: Some(25),
            ..Default::default()
        })
        .expect("io")
        .expect("ledger ok");
    assert_eq!(ledger["tail_count"], 25);
    assert_eq!(ledger["events"].as_array().unwrap().len(), 2);
    assert_eq!(mock_handler.event_calls.load(Ordering::SeqCst), 1);

    // 5. InspectorSelect
    let select = client
        .inspector_select(true)
        .expect("io")
        .expect("select ok");
    assert_eq!(select["armed"], true);
    assert_eq!(select["selected_node_id"], 42);
    assert_eq!(mock_handler.select_calls.load(Ordering::SeqCst), 1);

    // 6. LintApply
    let apply = client
        .lint_apply(LintApplyParams {
            ops: vec![serde_json::json!({ "op": "set_padding", "value": 12 })],
            force: true,
            ..Default::default()
        })
        .expect("io")
        .expect("apply ok");
    assert_eq!(apply["converged"], true);
    assert_eq!(apply["applied_ops_count"], 1);
    assert_eq!(apply["force"], true);
    assert_eq!(mock_handler.apply_calls.load(Ordering::SeqCst), 1);
}

#[test]
fn test_lint_scene_alias_dispatches_to_lint_pull() {
    let tmp = tempdir().expect("tempdir created");
    let sock_path = tmp.path().join("lint_scene_alias.sock");

    let mock_handler = Arc::new(TestMockHandler::new());
    let config = DevChannelConfig::new()
        .with_socket_path(&sock_path)
        .with_handler(Arc::clone(&mock_handler) as Arc<dyn DevChannelHandler>);
    let _server = config.start().expect("server starts");

    let mut client = DevChannelClient::connect(&sock_path).expect("client connects");
    client
        .hello(MARTENSITE_VERSION, DEV_CHANNEL_PROTOCOL_VERSION)
        .expect("io")
        .expect("hello ok");

    // Send using raw request with method = "LintScene"
    let req = JsonRpcRequest::new(Some(10), "LintScene", serde_json::json!({ "window_id": 5 }));
    let resp = client.send_raw(&req).expect("send raw succeeds");
    assert!(resp.error.is_none());
    assert_eq!(resp.result.unwrap()["window_id"], 5);
    assert_eq!(mock_handler.lint_calls.load(Ordering::SeqCst), 1);
}

#[test]
fn test_parse_error_on_malformed_json() {
    let tmp = tempdir().expect("tempdir created");
    let sock_path = tmp.path().join("malformed.sock");
    let _server = DevChannelServer::bind(&sock_path).expect("server binds");

    use std::io::{BufRead, BufReader, Write};

    let mut stream = DevStream::connect(&sock_path).expect("connect");
    stream
        .write_all(b"this is not valid json\n")
        .expect("write malformed line");
    stream.flush().expect("flush");

    let mut reader = BufReader::new(stream);
    let mut line = String::new();
    reader.read_line(&mut line).expect("read response");

    let resp: serde_json::Value = serde_json::from_str(&line).expect("valid json response");
    assert_eq!(resp["error"]["code"], ERR_PARSE);
}

#[test]
fn test_unknown_method_error() {
    let tmp = tempdir().expect("tempdir created");
    let sock_path = tmp.path().join("unknown_method.sock");
    let _server = DevChannelServer::bind(&sock_path).expect("server binds");

    let mut client = DevChannelClient::connect(&sock_path).expect("client connects");
    client
        .hello(MARTENSITE_VERSION, DEV_CHANNEL_PROTOCOL_VERSION)
        .expect("io")
        .expect("hello ok");

    let req = JsonRpcRequest::new(Some(1), "NonExistentMethod", serde_json::Value::Null);
    let resp = client.send_raw(&req).expect("send succeeds");
    assert!(resp.error.is_some());
    assert_eq!(resp.error.unwrap().code, ERR_METHOD_NOT_FOUND);
}

#[test]
fn test_concurrent_clients() {
    let tmp = tempdir().expect("tempdir created");
    let sock_path = tmp.path().join("concurrent.sock");
    let _server = DevChannelServer::bind(&sock_path).expect("server binds");

    let client_count = 5;
    let mut handles = Vec::new();

    for i in 0..client_count {
        let path = sock_path.clone();
        handles.push(std::thread::spawn(move || {
            let mut client = DevChannelClient::connect(&path).expect("connect");
            let hello = client
                .hello(MARTENSITE_VERSION, DEV_CHANNEL_PROTOCOL_VERSION)
                .expect("io")
                .expect("hello ok");
            assert_eq!(hello.server_version, MARTENSITE_VERSION);

            let res = client
                .tree_snapshot(TreeSnapshotParams {
                    max_depth: Some(i),
                    root_id: None,
                    ..Default::default()
                })
                .expect("io")
                .expect("tree ok");
            assert_eq!(res["max_depth"], i);
        }));
    }

    for h in handles {
        h.join().expect("thread finished cleanly");
    }
}

/// Mock handler for the dev-session typed dispatch arms
/// (`signal_set`, `runtime_errors`, `logs`, `a11y_action`, `hot_reload`)
/// added for ADR-0038/ADR-0039: proves each wire method reaches its typed
/// `DevChannelHandler` override with deserialized params rather than
/// falling through to `handle_custom`.
struct TypedDevMethodsHandler;

impl DevChannelHandler for TypedDevMethodsHandler {
    fn handle_signal_set(&self, params: SignalSetParams) -> Result<serde_json::Value, String> {
        Ok(serde_json::json!({
            "handler": "signal_set",
            "signal_id": params.signal_id,
            "value": params.value,
        }))
    }

    fn handle_runtime_errors(
        &self,
        params: RuntimeErrorsParams,
    ) -> Result<serde_json::Value, String> {
        Ok(serde_json::json!({
            "handler": "runtime_errors",
            "limit": params.limit,
            "severity": params.severity,
            "clear": params.clear,
        }))
    }

    fn handle_logs(&self, params: LogsParams) -> Result<serde_json::Value, String> {
        Ok(serde_json::json!({
            "handler": "logs",
            "limit": params.limit,
            "level": params.level,
            "target_prefix": params.target_prefix,
            "contains": params.contains,
        }))
    }

    fn handle_a11y_action(&self, params: A11yActionParams) -> Result<serde_json::Value, String> {
        Ok(serde_json::json!({
            "handler": "a11y_action",
            "node_id": params.node_id,
            "action": params.action,
            "value": params.value,
            "point": params.point,
        }))
    }

    fn handle_hot_reload(&self, params: HotReloadParams) -> Result<serde_json::Value, String> {
        Ok(serde_json::json!({
            "handler": "hot_reload",
            "reason": params.reason,
        }))
    }
}

#[test]
fn test_new_dev_methods_typed_dispatch() {
    let tmp = tempdir().expect("tempdir created");
    let sock_path = tmp.path().join("new_methods_dispatch.sock");

    let config = DevChannelConfig::new()
        .with_socket_path(&sock_path)
        .with_handler(Arc::new(TypedDevMethodsHandler));
    let _server = config.start().expect("server starts");

    let mut client = DevChannelClient::connect(&sock_path).expect("client connects");
    client
        .hello(MARTENSITE_VERSION, DEV_CHANNEL_PROTOCOL_VERSION)
        .expect("io")
        .expect("hello ok");

    let cases: [(&str, serde_json::Value, &str); 5] = [
        (
            "signal_set",
            serde_json::json!({ "signal_id": "counter", "value": 3 }),
            "signal_set",
        ),
        (
            "runtime_errors",
            serde_json::json!({ "limit": 10, "severity": "error", "clear": true }),
            "runtime_errors",
        ),
        (
            "logs",
            serde_json::json!({ "limit": 20, "level": "warn", "target_prefix": "app::" }),
            "logs",
        ),
        (
            "a11y_action",
            serde_json::json!({ "node_id": "7", "action": "click" }),
            "a11y_action",
        ),
        (
            "hot_reload",
            serde_json::json!({ "reason": "fix button" }),
            "hot_reload",
        ),
    ];

    for (i, (method, params, want)) in cases.iter().enumerate() {
        let req = JsonRpcRequest::new(Some(i as u64 + 1), *method, params.clone());
        let resp = client.send_raw(&req).expect("send succeeds");
        assert!(
            resp.error.is_none(),
            "{method} must not error: {:?}",
            resp.error
        );
        let result = resp.result.expect("{method} must return a result");
        assert_eq!(
            result["handler"], *want,
            "{method} must reach its typed handler"
        );
        // Spot-check param deserialization survived the round trip.
        for (key, value) in params.as_object().expect("params object") {
            assert_eq!(&result[key], value, "{method} param {key} must echo");
        }
    }

    // Method names normalize: mixed case and separators hit the same arm.
    let req = JsonRpcRequest::new(
        Some(90),
        "SignalSet",
        serde_json::json!({ "signal_id": "s" }),
    );
    let resp = client.send_raw(&req).expect("send succeeds");
    assert_eq!(
        resp.result.expect("SignalSet result")["handler"],
        "signal_set"
    );

    let req = JsonRpcRequest::new(Some(91), "Runtime-Errors", serde_json::json!({}));
    let resp = client.send_raw(&req).expect("send succeeds");
    assert_eq!(
        resp.result.expect("runtime_errors result")["handler"],
        "runtime_errors"
    );
}

#[test]
fn test_new_dev_methods_default_handler_reports_not_implemented() {
    let tmp = tempdir().expect("tempdir created");
    let sock_path = tmp.path().join("new_methods_default.sock");
    // DefaultDevChannelHandler: every new typed arm returns
    // `not_implemented:*`, which the dispatcher maps to -32601.
    let _server = DevChannelServer::bind(&sock_path).expect("server binds");

    let mut client = DevChannelClient::connect(&sock_path).expect("client connects");
    client
        .hello(MARTENSITE_VERSION, DEV_CHANNEL_PROTOCOL_VERSION)
        .expect("io")
        .expect("hello ok");

    for (i, method) in [
        "signal_set",
        "runtime_errors",
        "logs",
        "a11y_action",
        "hot_reload",
    ]
    .iter()
    .enumerate()
    {
        let req = JsonRpcRequest::new(Some(i as u64 + 1), *method, serde_json::json!({}));
        let resp = client.send_raw(&req).expect("send succeeds");
        let err = resp
            .error
            .unwrap_or_else(|| panic!("{method} must error under the default handler"));
        assert_eq!(err.code, ERR_METHOD_NOT_FOUND, "{method} error code");
        assert!(
            err.message.starts_with("not_implemented:"),
            "{method} must carry the not_implemented marker: {}",
            err.message
        );
    }
}

#[test]
fn test_new_dev_methods_error_mapping() {
    let tmp = tempdir().expect("tempdir created");
    let sock_path = tmp.path().join("new_methods_errors.sock");

    /// Handler whose typed arms fail with a non-`not_implemented` error —
    /// the dispatcher must map it to `ERR_INTERNAL`, not -32601.
    struct FailingHandler;
    impl DevChannelHandler for FailingHandler {
        fn handle_logs(&self, _params: LogsParams) -> Result<serde_json::Value, String> {
            Err("ring buffer corrupted".to_string())
        }
        fn handle_hot_reload(&self, _params: HotReloadParams) -> Result<serde_json::Value, String> {
            Err("unavailable: no coordinator".to_string())
        }
    }

    let config = DevChannelConfig::new()
        .with_socket_path(&sock_path)
        .with_handler(Arc::new(FailingHandler));
    let _server = config.start().expect("server starts");

    let mut client = DevChannelClient::connect(&sock_path).expect("client connects");
    client
        .hello(MARTENSITE_VERSION, DEV_CHANNEL_PROTOCOL_VERSION)
        .expect("io")
        .expect("hello ok");

    for (i, method) in ["logs", "hot_reload"].iter().enumerate() {
        let req = JsonRpcRequest::new(Some(i as u64 + 1), *method, serde_json::json!({}));
        let resp = client.send_raw(&req).expect("send succeeds");
        let err = resp.error.expect("error response");
        assert_eq!(err.code, ERR_INTERNAL, "{method} error code");
    }
}
