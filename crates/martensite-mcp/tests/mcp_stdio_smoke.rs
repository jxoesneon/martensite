//! End-to-end smoke tests for the Martensite MCP server.
//!
//! Drives the real `rmcp` server handler over an in-process duplex transport
//! (the same `serve_server` path used by `serve_stdio`) and, on Unix, against
//! a mock ADR-0038 dev-channel socket answering `hello`, `tree_snapshot`, and
//! friends — exercising the exact version-locked handshake a live dev app
//! performs.

use std::collections::BTreeSet;
use std::path::PathBuf;

use martensite_mcp::{MartensiteMcp, McpServerOptions, ServerMode};
use rmcp::model::{CallToolRequestParams, GetPromptRequestParams, ReadResourceRequestParams};
use rmcp::service::{serve_client, serve_server, RoleClient, RunningService};

/// Expected tool surface per spec §3 — 28 `martensite_*` tools.
const EXPECTED_TOOLS: [&str; 28] = [
    "martensite_inspect_tree",
    "martensite_get_node",
    "martensite_set_loading",
    "martensite_diagnose_layout",
    "martensite_explain_overflow",
    "martensite_inspect_signals",
    "martensite_trigger_signal",
    "martensite_set_signal",
    "martensite_inspect_a11y_tree",
    "martensite_invoke_accessibility_action",
    "martensite_lint_scene",
    "martensite_apply_lint_fix",
    "martensite_audit_paint",
    "martensite_list_tweaks",
    "martensite_set_tweak",
    "martensite_set_theme",
    "martensite_sync_tweaks_to_source",
    "martensite_dispatch_event",
    "martensite_get_event_ledger",
    "martensite_step_timemachine",
    "martensite_capture_node",
    "martensite_render_headless",
    "martensite_doctor",
    "martensite_reload_status",
    "martensite_hot_reload",
    "martensite_scaffold_widget",
    "martensite_runtime_errors",
    "martensite_logs",
];

/// Spawns the MCP server on one end of a duplex stream and returns a
/// connected MCP client on the other (full `initialize` handshake done).
async fn spawn_server(opts: McpServerOptions) -> RunningService<RoleClient, ()> {
    let (server_io, client_io) = tokio::io::duplex(64 * 1024);
    let server = MartensiteMcp::new(opts);
    tokio::spawn(async move {
        let Ok(running) = serve_server(server, server_io).await else {
            return;
        };
        let _ = running.waiting().await;
    });
    serve_client((), client_io)
        .await
        .expect("MCP initialize handshake")
}

/// Builds a call-tool params object from a tool name and JSON arguments.
fn call(name: &str, arguments: serde_json::Value) -> CallToolRequestParams {
    let mut params = CallToolRequestParams::new(name.to_string());
    params.arguments = Some(rmcp::model::object(arguments));
    params
}

/// Calls a tool expecting a safety-gate rejection, returning the rendered
/// error text. Rejections surface either as a protocol-level `McpError`
/// (`Err`) or as an `is_error` tool result — both are valid refusals.
async fn call_expect_error(
    client: &RunningService<RoleClient, ()>,
    name: &str,
    arguments: serde_json::Value,
) -> String {
    match client.call_tool(call(name, arguments)).await {
        Err(err) => format!("{err:?}"),
        Ok(res) => {
            assert_eq!(
                res.is_error,
                Some(true),
                "`{name}` should be rejected: {res:?}"
            );
            format!("{:?}", res.content)
        }
    }
}

fn temp_dir(tag: &str) -> PathBuf {
    let dir =
        std::env::temp_dir().join(format!("martensite-mcp-test-{tag}-{}", std::process::id()));
    std::fs::create_dir_all(&dir).expect("create temp dir");
    dir
}

#[tokio::test]
async fn registers_all_28_tools() {
    let client = spawn_server(McpServerOptions::offline()).await;
    let tools = client.list_tools(None).await.expect("tools/list");
    let names: BTreeSet<String> = tools.tools.iter().map(|t| t.name.to_string()).collect();
    assert_eq!(names.len(), 28, "expected exactly 28 tools, got {names:?}");
    for expected in EXPECTED_TOOLS {
        assert!(names.contains(expected), "missing tool `{expected}`");
    }
    // Every advertised tool carries MCP ToolAnnotations hints.
    for tool in &tools.tools {
        assert!(
            tool.annotations.is_some(),
            "tool `{}` must advertise annotations",
            tool.name
        );
    }
    let by_name = |n: &str| {
        tools
            .tools
            .iter()
            .find(|t| t.name == n)
            .and_then(|t| t.annotations.clone())
            .unwrap_or_else(|| panic!("missing tool `{n}`"))
    };
    assert_eq!(
        by_name("martensite_inspect_tree").read_only_hint,
        Some(true)
    );
    assert_eq!(by_name("martensite_hot_reload").read_only_hint, Some(false));
    assert_eq!(
        by_name("martensite_set_loading").read_only_hint,
        Some(false)
    );
    assert_eq!(
        by_name("martensite_set_loading").destructive_hint,
        Some(true)
    );
    client.cancel().await.expect("shutdown");
}

#[tokio::test]
async fn lists_resources_and_prompts() {
    let client = spawn_server(McpServerOptions::offline()).await;

    let resources = client.list_resources(None).await.expect("resources/list");
    let uris: Vec<String> = resources
        .resources
        .iter()
        .map(|r| r.uri.to_string())
        .collect();
    for required in [
        "martensite://app/tree/summary",
        "martensite://app/a11y",
        "martensite://app/lint",
        "martensite://app/signals",
        "martensite://app/events/ledger",
        "martensite://app/tweaks",
        "martensite://theme/tokens",
    ] {
        assert!(uris.iter().any(|u| u == required), "missing {required}");
    }

    let templates = client
        .list_resource_templates(None)
        .await
        .expect("resources/templates/list");
    let tpl: Vec<String> = templates
        .resource_templates
        .iter()
        .map(|r| r.uri_template.to_string())
        .collect();
    assert!(tpl.iter().any(|u| u == "martensite://app/tree/{root_id}"));
    assert!(tpl.iter().any(|u| u == "martensite://standards/{standard}"));

    let prompts = client.list_prompts(None).await.expect("prompts/list");
    let names: BTreeSet<String> = prompts.prompts.iter().map(|p| p.name.clone()).collect();
    for expected in [
        "audit_screen",
        "fix_overflow",
        "refactor_reactive_widget",
        "implement_accessible_pattern",
    ] {
        assert!(names.contains(expected), "missing prompt `{expected}`");
    }
    client.cancel().await.expect("shutdown");
}

#[tokio::test]
async fn offline_doctor_succeeds() {
    let client = spawn_server(McpServerOptions::offline()).await;
    let res = client
        .call_tool(call("martensite_doctor", serde_json::json!({})))
        .await
        .expect("call martensite_doctor");
    assert!(
        res.is_error != Some(true),
        "doctor must succeed offline: {res:?}"
    );
    assert!(!res.content.is_empty(), "doctor should report checks");
    client.cancel().await.expect("shutdown");
}

#[tokio::test]
async fn live_only_tool_reports_structured_offline_error() {
    let client = spawn_server(McpServerOptions::offline()).await;
    let msg = call_expect_error(
        &client,
        "martensite_trigger_signal",
        serde_json::json!({"signal_id": "sig-1", "new_value_json": "42"}),
    )
    .await;
    assert!(
        msg.contains("live"),
        "live-only tool must surface a structured error offline: {msg}"
    );
    client.cancel().await.expect("shutdown");
}

#[tokio::test]
async fn sync_tweaks_requires_explicit_confirmation() {
    let client = spawn_server(McpServerOptions::offline()).await;
    let msg = call_expect_error(
        &client,
        "martensite_sync_tweaks_to_source",
        serde_json::json!({"dry_run": false, "confirmed": false}),
    )
    .await;
    assert!(
        msg.contains("unconfirmed") || msg.contains("confirmed"),
        "unconfirmed mutation must be rejected: {msg}"
    );
    client.cancel().await.expect("shutdown");
}

#[tokio::test]
async fn scaffold_rejects_workspace_escape() {
    let ws = temp_dir("ws");
    let client = spawn_server(McpServerOptions {
        workspace_root: Some(ws),
        ..McpServerOptions::offline()
    })
    .await;
    let msg = call_expect_error(
        &client,
        "martensite_scaffold_widget",
        serde_json::json!({
            "widget_name": "BadWidget",
            "widget_type": "leaf",
            "parent_crate_path": "/etc",
        }),
    )
    .await;
    assert!(
        msg.contains("confinement") || msg.contains("workspace"),
        "paths outside the workspace must be confined: {msg}"
    );
    client.cancel().await.expect("shutdown");
}

#[tokio::test]
async fn render_headless_rejects_unsafe_target() {
    let client = spawn_server(McpServerOptions::offline()).await;
    let msg = call_expect_error(
        &client,
        "martensite_render_headless",
        serde_json::json!({"crate_target": "evil; rm -rf /"}),
    )
    .await;
    assert!(
        msg.contains("crate_target") || msg.contains("invalid"),
        "crate_target must pass ^[a-zA-Z0-9_-]+$: {msg}"
    );
    client.cancel().await.expect("shutdown");
}

#[tokio::test]
async fn standards_resource_is_statically_served() {
    let client = spawn_server(McpServerOptions::offline()).await;
    let res = client
        .read_resource(ReadResourceRequestParams::new(
            "martensite://standards/wcag".to_string(),
        ))
        .await
        .expect("read standards/wcag");
    let text = serde_json::to_string(&res.contents).expect("serialize contents");
    assert!(text.to_lowercase().contains("wcag"), "got: {text}");
    client.cancel().await.expect("shutdown");
}

#[tokio::test]
async fn get_prompt_returns_workflow() {
    let client = spawn_server(McpServerOptions::offline()).await;
    let mut req = GetPromptRequestParams::new("fix_overflow".to_string());
    req.arguments = Some(rmcp::model::object(serde_json::json!({"node_id": "42"})));
    let res = client
        .get_prompt(req)
        .await
        .expect("prompts/get fix_overflow");
    assert!(
        !res.messages.is_empty(),
        "fix_overflow should return a multi-step workflow"
    );
    client.cancel().await.expect("shutdown");
}

#[tokio::test]
async fn offline_lint_scene_from_dump_file() {
    let dir = temp_dir("scene");
    let scene_path = dir.join("scene.json");

    // Build a minimal LintScene and serialize it as a LintDump JSON fixture.
    let scene = martensite_design_lint::LintScene::default();
    let report =
        martensite_design_lint::lint(&scene, &martensite_design_lint::LintConfig::default());
    let dump = martensite_devtools::lint_bridge::LintDump::new(&scene, &report);
    std::fs::write(
        &scene_path,
        serde_json::to_string(&dump).expect("dump json"),
    )
    .expect("write scene");

    let client = spawn_server(McpServerOptions {
        scene: Some(scene_path),
        ..McpServerOptions::offline()
    })
    .await;
    let res = client
        .call_tool(call("martensite_lint_scene", serde_json::json!({})))
        .await
        .expect("call martensite_lint_scene");
    assert!(
        res.is_error != Some(true),
        "offline lint must succeed with --scene: {res:?}"
    );
    client.cancel().await.expect("shutdown");
}

/// Minimal mock dev channel: a Unix listener speaking the ADR-0038
/// newline-delimited JSON-RPC-lite protocol with a versioned `hello`.
#[cfg(unix)]
mod mock_dev {
    use std::io::{BufRead, BufReader, Write};
    use std::os::unix::net::UnixListener;
    use std::path::PathBuf;

    pub fn spawn(sock_path: PathBuf) -> std::thread::JoinHandle<()> {
        std::thread::spawn(move || {
            let _ = std::fs::remove_file(&sock_path);
            let listener = UnixListener::bind(&sock_path).expect("bind mock socket");
            for stream in listener.incoming() {
                let Ok(stream) = stream else { continue };
                std::thread::spawn(move || serve_conn(stream));
            }
        })
    }

    fn respond(id: serde_json::Value, result: serde_json::Value) -> String {
        serde_json::json!({"id": id, "result": result}).to_string()
    }

    fn serve_conn(stream: std::os::unix::net::UnixStream) {
        let write_side = match stream.try_clone() {
            Ok(s) => s,
            Err(_) => return,
        };
        let mut writer = std::io::BufWriter::new(write_side);
        let mut reader = BufReader::new(stream);
        let mut line = String::new();
        while reader.read_line(&mut line).unwrap_or(0) > 0 {
            let req: serde_json::Value = match serde_json::from_str(line.trim()) {
                Ok(v) => v,
                Err(_) => break,
            };
            let id = req.get("id").cloned().unwrap_or(serde_json::json!(0));
            let method = req.get("method").and_then(|m| m.as_str()).unwrap_or("");
            let payload = match method {
                "hello" => respond(
                    id,
                    serde_json::json!({
                        "server": "martensite-mock",
                        "server_version": env!("CARGO_PKG_VERSION"),
                        "protocol_version": 1,
                        "stateful": true,
                    }),
                ),
                "tree_snapshot" => respond(
                    id,
                    serde_json::json!({
                        "root": {
                            "id": 1,
                            "debug_name": "MockRoot",
                            "kind": "Container",
                            "screen_bounds": [0.0, 0.0, 800.0, 600.0],
                            "local_bounds": [0.0, 0.0, 800.0, 600.0],
                            "depth": 0,
                            "child_count": 1,
                            "badges": [],
                            "active_signal_count": 0,
                            "children": [{
                                "id": 2,
                                "debug_name": "MockChild",
                                "kind": "Text",
                                "screen_bounds": [8.0, 8.0, 120.0, 24.0],
                                "local_bounds": [8.0, 8.0, 120.0, 24.0],
                                "depth": 1,
                                "child_count": 0,
                                "badges": [],
                                "active_signal_count": 0,
                                "children": []
                            }]
                        },
                        "selected_id": null,
                        "layout_chain": [],
                        "signals": []
                    }),
                ),
                "event_ledger" => respond(id, serde_json::json!({"events": [], "total": 0})),
                "tweaks_list" => respond(id, serde_json::json!({"tweaks": []})),
                "signal_set" => respond(id, serde_json::json!({"applied": true})),
                "runtime_errors" => respond(
                    id,
                    serde_json::json!({
                        "errors": [{
                            "seq": 1,
                            "severity": "error",
                            "source": "panic",
                            "message": "mock panic: layout cycle",
                            "node_id": 2,
                            "timestamp": "2025-01-01T00:00:00Z"
                        }],
                        "total": 1,
                        "panic_active": true,
                        "surface_attached": true
                    }),
                ),
                "logs" => respond(
                    id,
                    serde_json::json!({
                        "logs": [{
                            "seq": 7,
                            "level": "WARN",
                            "target": "martensite::mock",
                            "message": "mock warning",
                            "file": "src/mock.rs",
                            "line": 12,
                            "timestamp": "2025-01-01T00:00:00Z"
                        }],
                        "total_buffered": 7
                    }),
                ),
                "a11y_action" => respond(
                    id,
                    serde_json::json!({
                        "response": "Handled",
                        "target": "2",
                        "hit_path": [1, 2]
                    }),
                ),
                "hot_reload" => respond(
                    id,
                    serde_json::json!({
                        "requested": true,
                        "mechanism": "marker-file",
                        "marker": "/tmp/martensite/mock.reload"
                    }),
                ),
                "reload_status" => respond(
                    id,
                    serde_json::json!({
                        "active_build_id": "mock-build",
                        "total_reloads": 0
                    }),
                ),
                other => serde_json::json!({
                    "id": id,
                    "error": {"code": -32601, "message": format!("method `{other}` not found")},
                })
                .to_string(),
            };
            if writer.write_all(payload.as_bytes()).is_err()
                || writer.write_all(b"\n").is_err()
                || writer.flush().is_err()
            {
                break;
            }
            line.clear();
        }
    }
}

#[cfg(unix)]
#[tokio::test]
async fn live_mode_handshakes_and_serves_tree() {
    let dir = temp_dir("live");
    let sock = dir.join("dev.sock");
    let _server = mock_dev::spawn(sock.clone());
    for _ in 0..100 {
        if sock.exists() {
            break;
        }
        std::thread::sleep(std::time::Duration::from_millis(10));
    }

    let client = spawn_server(McpServerOptions {
        socket: Some(sock),
        ..Default::default()
    })
    .await;

    let res = client
        .call_tool(call("martensite_inspect_tree", serde_json::json!({})))
        .await
        .expect("call martensite_inspect_tree");
    assert!(
        res.is_error != Some(true),
        "live inspect_tree must succeed against mock: {res:?}"
    );
    let text = format!("{:?}", res.content);
    assert!(
        text.contains("MockRoot"),
        "mock tree should reach the MCP surface: {text}"
    );
    client.cancel().await.expect("shutdown");
}

#[cfg(unix)]
#[tokio::test]
async fn live_mode_serves_runtime_tools() {
    let dir = temp_dir("live-runtime");
    let sock = dir.join("dev.sock");
    let _server = mock_dev::spawn(sock.clone());
    for _ in 0..100 {
        if sock.exists() {
            break;
        }
        std::thread::sleep(std::time::Duration::from_millis(10));
    }

    let client = spawn_server(McpServerOptions {
        socket: Some(sock),
        ..Default::default()
    })
    .await;

    // Read-only runtime diagnostics decode the mock envelopes.
    let res = client
        .call_tool(call(
            "martensite_runtime_errors",
            serde_json::json!({"severity": "error"}),
        ))
        .await
        .expect("call martensite_runtime_errors");
    assert!(
        res.is_error != Some(true),
        "runtime_errors must succeed against mock: {res:?}"
    );
    let text = format!("{:?}", res.content);
    assert!(
        text.contains("mock panic") && text.contains("panic_active"),
        "decoded error bundle should reach the MCP surface: {text}"
    );

    let res = client
        .call_tool(call(
            "martensite_logs",
            serde_json::json!({"level": "warn"}),
        ))
        .await
        .expect("call martensite_logs");
    assert!(res.is_error != Some(true), "logs must succeed: {res:?}");
    let text = format!("{:?}", res.content);
    assert!(
        text.contains("mock warning") && text.contains("total_buffered"),
        "decoded log records should reach the MCP surface: {text}"
    );

    // Mutation tools decode their wire acknowledgements.
    let res = client
        .call_tool(call(
            "martensite_set_signal",
            serde_json::json!({"signal_id": "counter", "value": 42}),
        ))
        .await
        .expect("call martensite_set_signal");
    assert!(
        res.is_error != Some(true),
        "set_signal must succeed: {res:?}"
    );

    let res = client
        .call_tool(call(
            "martensite_invoke_accessibility_action",
            serde_json::json!({"node_id": "2", "action": "click"}),
        ))
        .await
        .expect("call martensite_invoke_accessibility_action");
    assert!(
        res.is_error != Some(true),
        "a11y action must succeed: {res:?}"
    );
    let text = format!("{:?}", res.content);
    assert!(text.contains("Handled"), "response should decode: {text}");

    let res = client
        .call_tool(call(
            "martensite_hot_reload",
            serde_json::json!({"reason": "test"}),
        ))
        .await
        .expect("call martensite_hot_reload");
    assert!(
        res.is_error != Some(true),
        "hot_reload must succeed: {res:?}"
    );
    let text = format!("{:?}", res.content);
    assert!(
        text.contains("marker-file"),
        "mechanism should decode: {text}"
    );

    client.cancel().await.expect("shutdown");
}

#[cfg(unix)]
#[tokio::test]
async fn version_mismatch_is_rejected_unless_allowed() {
    let dir = temp_dir("ver");
    let sock = dir.join("dev.sock");

    // Mock that lies about its version.
    let sock2 = sock.clone();
    std::thread::spawn(move || {
        let _ = std::fs::remove_file(&sock2);
        let listener = std::os::unix::net::UnixListener::bind(&sock2).expect("bind");
        for stream in listener.incoming() {
            let Ok(stream) = stream else { continue };
            std::thread::spawn(move || {
                use std::io::{BufRead, BufReader, Write};
                let w = match stream.try_clone() {
                    Ok(s) => s,
                    Err(_) => return,
                };
                let mut writer = std::io::BufWriter::new(w);
                let mut reader = BufReader::new(stream);
                let mut line = String::new();
                while reader.read_line(&mut line).unwrap_or(0) > 0 {
                    let req: serde_json::Value =
                        serde_json::from_str(line.trim()).unwrap_or(serde_json::json!({}));
                    let id = req.get("id").cloned().unwrap_or(serde_json::json!(0));
                    let payload = serde_json::json!({
                        "id": id,
                        "result": {
                            "server": "stale-mock",
                            "server_version": "0.0.0-stale",
                            "protocol_version": 1,
                        }
                    })
                    .to_string();
                    let _ = writer.write_all(payload.as_bytes());
                    let _ = writer.write_all(b"\n");
                    let _ = writer.flush();
                    line.clear();
                }
            });
        }
    });
    for _ in 0..100 {
        if sock.exists() {
            break;
        }
        std::thread::sleep(std::time::Duration::from_millis(10));
    }

    // Strict mode: the mismatched app must produce a live-call error.
    let client = spawn_server(McpServerOptions {
        socket: Some(sock.clone()),
        allow_version_mismatch: false,
        ..Default::default()
    })
    .await;
    let msg = call_expect_error(&client, "martensite_inspect_tree", serde_json::json!({})).await;
    assert!(
        msg.contains("version") || msg.contains("mismatch"),
        "version mismatch must be rejected: {msg}"
    );
    client.cancel().await.expect("shutdown");

    // Relaxed mode: same app, but allowed — the tool may then fail on a
    // missing method (mock only answers hello) yet must NOT fail with a
    // version-mismatch error.
    let client = spawn_server(McpServerOptions {
        socket: Some(sock),
        allow_version_mismatch: true,
        ..Default::default()
    })
    .await;
    let msg = match client
        .call_tool(call("martensite_inspect_tree", serde_json::json!({})))
        .await
    {
        Ok(res) => format!("{:?}", res.content),
        Err(err) => format!("{err:?}"),
    };
    assert!(
        !msg.contains("version mismatch") && !msg.contains("VersionMismatch"),
        "allow_version_mismatch must bypass the version gate: {msg}"
    );
    client.cancel().await.expect("shutdown");
}

/// `ServerMode` reflects the discovered environment at construction time.
#[test]
fn server_mode_reporting() {
    let server = MartensiteMcp::new(McpServerOptions::offline());
    // No socket configured → the mode query is still safe and returns a
    // definite value (Offline unless a session was discovered).
    assert!(matches!(
        server.mode(),
        ServerMode::Offline | ServerMode::Live { .. }
    ));
}
