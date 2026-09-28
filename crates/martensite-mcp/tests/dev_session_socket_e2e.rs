//! Real-socket end-to-end coverage for the Martensite MCP server.
//!
//! Unlike `mcp_stdio_smoke.rs` (which drives a hand-rolled mock socket),
//! these tests bind a real [`DevChannelServer`] hosting
//! [`SessionDevChannelHandler`] over a real [`DevSession`], then run MCP
//! `tools/call` requests through the full stack:
//! MCP tool → dev-channel client → Unix socket → JSON-RPC dispatcher →
//! session handler → `DevSession`/probe.

#![cfg(unix)]

use std::path::PathBuf;
use std::sync::Arc;

use martensite_devtools::dev_session::{
    ArenaProbe, DevSession, NullProbe, SessionResult, WidgetArenaProbe,
};
use martensite_host::dev_channel::{DevChannelConfig, DevChannelServer};
use martensite_host::session_handler::SessionDevChannelHandler;
use martensite_mcp::{MartensiteMcp, McpServerOptions};
use rmcp::model::CallToolRequestParams;
use rmcp::service::{serve_client, serve_server, RoleClient, RunningService};
use serde_json::{json, Value};

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

fn call(name: &str, arguments: serde_json::Value) -> CallToolRequestParams {
    let mut params = CallToolRequestParams::new(name.to_string());
    params.arguments = Some(rmcp::model::object(arguments));
    params
}

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
    let dir = std::env::temp_dir().join(format!(
        "martensite-mcp-socket-e2e-{tag}-{}",
        std::process::id()
    ));
    std::fs::create_dir_all(&dir).expect("create temp dir");
    dir
}

/// Starts a dev-channel server on `sock` serving `session`.
fn serve_session_at(sock: &std::path::Path, session: Arc<DevSession>) -> DevChannelServer {
    DevChannelConfig::new()
        .with_socket_path(sock)
        .with_handler(Arc::new(SessionDevChannelHandler::new(session)))
        .start()
        .expect("dev channel server binds")
}

#[test]
fn raw_client_handshake_against_session_server() {
    let dir = temp_dir("raw");
    let sock = dir.join("dev.sock");
    let server = serve_session_at(&sock, Arc::new(DevSession::new(Box::new(NullProbe))));
    assert!(server.is_running());

    let mut client = martensite_mcp::client::DevChannelClient::connect(&sock, false)
        .expect("mcp dev-channel client handshakes with real server");
    assert!(client.handshake_done());
    let tweaks = client
        .call("tweaks_list", serde_json::json!({}))
        .expect("tweaks_list over real socket");
    assert_eq!(tweaks["revision"], 0);
    drop(server);
}

/// Probe answering `tree_snapshot` with a canned two-node tree.
struct TreeProbe;

impl ArenaProbe for TreeProbe {
    fn probe_tree_snapshot(&mut self, _params: &Value) -> SessionResult {
        Ok(json!({
            "root": {
                "id": 1,
                "debug_name": "SocketRoot",
                "kind": "Container",
                "screen_bounds": [0.0, 0.0, 800.0, 600.0],
                "local_bounds": [0.0, 0.0, 800.0, 600.0],
                "depth": 0,
                "child_count": 1,
                "badges": [],
                "active_signal_count": 0,
                "children": [{
                    "id": 2,
                    "debug_name": "SocketChild",
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
        }))
    }
}

#[tokio::test]
async fn mcp_tools_reach_dev_session_over_real_socket() {
    let dir = temp_dir("live");
    let sock = dir.join("dev.sock");
    let session = Arc::new(DevSession::new(Box::new(TreeProbe)));
    session.record_reload("e2e-build");
    let server = serve_session_at(&sock, session);
    assert!(server.is_running());

    let client = spawn_server(McpServerOptions {
        socket: Some(sock.clone()),
        ..Default::default()
    })
    .await;

    // The server should report live mode against the real socket.
    let mode = client
        .call_tool(call("martensite_reload_status", json!({})))
        .await
        .expect("reload_status call");
    assert!(
        mode.is_error != Some(true),
        "reload_status must succeed live: {mode:?}"
    );
    let text = format!("{:?}", mode.content);
    assert!(
        text.contains("e2e-build"),
        "reload status must reflect the real session: {text}"
    );

    // Probe-backed inspection reaches the real `DevSession` + probe.
    let tree = client
        .call_tool(call("martensite_inspect_tree", json!({})))
        .await
        .expect("inspect_tree call");
    assert!(
        tree.is_error != Some(true),
        "inspect_tree must succeed against session probe: {tree:?}"
    );
    let text = format!("{:?}", tree.content);
    assert!(
        text.contains("SocketRoot"),
        "probe tree should reach the MCP surface: {text}"
    );

    // Session-state tools work with no probe at all.
    let tweaks = client
        .call_tool(call("martensite_list_tweaks", json!({})))
        .await
        .expect("list_tweaks call");
    assert!(
        tweaks.is_error != Some(true),
        "list_tweaks must succeed live: {tweaks:?}"
    );

    client.cancel().await.expect("shutdown");
    drop(server);
    assert!(
        !sock.exists(),
        "socket file must be cleaned up on server drop"
    );
}

#[tokio::test]
async fn event_dispatch_records_ledger_through_real_socket() {
    let dir = temp_dir("events");
    let sock = dir.join("dev.sock");
    // NullProbe cannot deliver events to a real arena, but the session
    // still records every dispatch attempt into the event ledger.
    let server = serve_session_at(&sock, Arc::new(DevSession::new(Box::new(NullProbe))));

    let client = spawn_server(McpServerOptions {
        socket: Some(sock),
        ..Default::default()
    })
    .await;

    // The probe is unimplemented, so the dispatch surfaces as an error —
    // but the attempt must still land in the session ledger.
    let err = call_expect_error(
        &client,
        "martensite_dispatch_event",
        json!({ "event_type": "pointer_click", "position": [10.0, 20.0] }),
    )
    .await;
    assert!(
        err.contains("not_implemented") || err.contains("event_dispatch"),
        "dispatch error should report the missing probe: {err}"
    );

    let ledger = client
        .call_tool(call("martensite_get_event_ledger", json!({})))
        .await
        .expect("event_ledger call");
    assert!(
        ledger.is_error != Some(true),
        "event_ledger must succeed live: {ledger:?}"
    );
    let text = format!("{:?}", ledger.content);
    assert!(
        text.contains("pointer") && text.contains("total\\\":1"),
        "dispatched event should be recorded in the ledger: {text}"
    );

    client.cancel().await.expect("shutdown");
    drop(server);
}

#[tokio::test]
async fn absent_capabilities_surface_as_structured_errors_over_socket() {
    let dir = temp_dir("null-probe");
    let sock = dir.join("dev.sock");
    let server = serve_session_at(&sock, Arc::new(DevSession::new(Box::new(NullProbe))));

    let client = spawn_server(McpServerOptions {
        socket: Some(sock.clone()),
        ..Default::default()
    })
    .await;

    // Probe-only capabilities honestly report the gap — never fabricate.
    let err = call_expect_error(&client, "martensite_inspect_tree", json!({})).await;
    assert!(
        err.contains("not_implemented") || err.contains("tree_snapshot"),
        "missing capability should surface as a structured error: {err}"
    );

    let err = call_expect_error(
        &client,
        "martensite_capture_node",
        json!({ "node_id": "7" }),
    )
    .await;
    assert!(
        err.contains("not_implemented") || err.contains("capture_node"),
        "capture without probe should error honestly: {err}"
    );

    // Non-probe session state still answers.
    let status = client
        .call_tool(call("martensite_reload_status", json!({})))
        .await
        .expect("reload_status call");
    assert!(
        status.is_error != Some(true),
        "reload_status works without a probe: {status:?}"
    );

    client.cancel().await.expect("shutdown");
    drop(server);
    assert!(!sock.exists());
}

/// Builds a real two-node arena: root (800×600) with a named child.
fn real_arena() -> Arc<std::sync::Mutex<martensite_core::WidgetArena>> {
    use martensite_core::{DummyWidget, HotNode, NodeFlags, Rect, WidgetArena};

    let mut arena = WidgetArena::new();
    let root_hot = HotNode {
        bounds: Rect::new(0.0, 0.0, 800.0, 600.0),
        flags: NodeFlags::VISIBLE | NodeFlags::HIT_TEST_ENABLED,
        ..Default::default()
    };
    let root = arena.insert_with_widget(root_hot, Box::new(DummyWidget));

    let child_hot = HotNode {
        bounds: Rect::new(8.0, 8.0, 120.0, 24.0),
        flags: NodeFlags::VISIBLE | NodeFlags::FOCUSABLE,
        ..Default::default()
    };
    let child = arena.insert_with_widget(child_hot, Box::new(DummyWidget));
    arena
        .get_cold_mut(child)
        .expect("child cold node")
        .debug_name = Some("LiveChild");
    arena.append_child(root, child).expect("append child");

    Arc::new(std::sync::Mutex::new(arena))
}

/// Full-path check against a real `WidgetArena` — not a canned probe.
/// `WidgetArenaProbe` reads live node data out of the shared arena, so
/// the MCP-visible tree must reflect actual bounds, flags, and names.
#[tokio::test]
async fn mcp_tools_reach_real_widget_arena_over_socket() {
    let dir = temp_dir("arena");
    let sock = dir.join("dev.sock");
    let arena = real_arena();
    let session = Arc::new(DevSession::new(Box::new(WidgetArenaProbe::new(
        Arc::clone(&arena),
    ))));
    let server = serve_session_at(&sock, session);

    let client = spawn_server(McpServerOptions {
        socket: Some(sock),
        ..Default::default()
    })
    .await;

    let tree = client
        .call_tool(call("martensite_inspect_tree", json!({})))
        .await
        .expect("inspect_tree call");
    assert!(
        tree.is_error != Some(true),
        "inspect_tree must succeed against a real arena: {tree:?}"
    );
    let text = format!("{:?}", tree.content);
    assert!(
        text.contains("LiveChild"),
        "real arena child name must reach the MCP surface: {text}"
    );

    // Mutating the arena while the session is live must be visible to
    // the next MCP call — the probe reads current state, not a snapshot.
    let child_id = {
        let mut g = arena.lock().expect("arena lock");
        let root = g.iter_depth_first().next().expect("root");
        let child = g.first_child(root).expect("child");
        g.get_hot_mut(child)
            .expect("child hot")
            .flags
            .insert(martensite_core::NodeFlags::DIRTY_PAINT);
        child.to_u64()
    };

    let detail = client
        .call_tool(call(
            "martensite_get_node",
            json!({ "node_id": child_id.to_string() }),
        ))
        .await
        .expect("get_node call");
    assert!(
        detail.is_error != Some(true),
        "get_node must succeed against a real arena: {detail:?}"
    );
    let text = format!("{:?}", detail.content);
    assert!(
        text.contains("LiveChild") && text.contains("dirty_paint"),
        "node detail must reflect live flags: {text}"
    );

    client.cancel().await.expect("shutdown");
    drop(server);
}

/// Workspace confinement must run BEFORE the app's write request: a tweak
/// whose recorded source span points outside the MCP workspace root is
/// rejected at the preflight (dry_run) stage, and the target file stays
/// untouched — proving no `tweaks_sync` write was ever dispatched.
#[tokio::test]
async fn sync_tweaks_confines_before_write() {
    use martensite_devtools::tweak::SourceSpan;

    let dir = temp_dir("confine");
    let sock = dir.join("dev.sock");
    // A workspace the MCP confines to, and a source file OUTSIDE it.
    let workspace = dir.join("ws");
    std::fs::create_dir_all(&workspace).expect("mkdir workspace");
    let outside = dir.join("outside.rs");
    let original = "fn view() {\n    .padding(12.0)\n}\n";
    std::fs::write(&outside, original).expect("write outside source");

    let session = Arc::new(DevSession::new(Box::new(NullProbe)));
    {
        let span = SourceSpan::new(outside.to_string_lossy().into_owned(), 2, 3);
        session
            .tweaks
            .lock()
            .expect("tweaks mutex")
            .register_or_get_with_span("ui/pad", 12.0f32, span, "padding");
    }
    session
        .tweak_set(&json!({"name": "ui/pad", "value": 16.0}))
        .expect("dirty the tweak");
    let server = serve_session_at(&sock, session);

    let client = spawn_server(McpServerOptions {
        socket: Some(sock),
        workspace_root: Some(workspace),
        ..Default::default()
    })
    .await;

    let msg = call_expect_error(
        &client,
        "martensite_sync_tweaks_to_source",
        json!({
            "tweak_names": ["ui/pad"],
            "dry_run": false,
            "confirmed": true,
        }),
    )
    .await;
    assert!(
        msg.contains("outside the workspace") || msg.contains("confinement"),
        "out-of-workspace target must be a confinement violation: {msg}"
    );
    assert_eq!(
        std::fs::read_to_string(&outside).expect("read outside source"),
        original,
        "rejected sync must never reach the app's write path"
    );

    client.cancel().await.expect("shutdown");
    drop(server);
}

/// Confirmed sync against an in-workspace file: preflight pins the write to
/// the previewed revision, the file is spliced in place, and an audit record
/// is emitted. A caller-supplied numeric `expected_revision` matching the
/// live revision is honored; a non-numeric token is rejected client-side.
#[tokio::test]
async fn sync_tweaks_writes_confined_source() {
    use martensite_devtools::tweak::SourceSpan;

    let dir = temp_dir("sync-ok");
    let sock = dir.join("dev.sock");
    let workspace = dir.join("ws");
    let src_dir = workspace.join("src");
    std::fs::create_dir_all(&src_dir).expect("mkdir workspace src");
    let target = src_dir.join("ui.rs");
    let original = "fn view() {\n    .padding(12.0)\n}\n";
    std::fs::write(&target, original).expect("write source");

    let session = Arc::new(DevSession::new(Box::new(NullProbe)));
    {
        let span = SourceSpan::new(target.to_string_lossy().into_owned(), 2, 5);
        session
            .tweaks
            .lock()
            .expect("tweaks mutex")
            .register_or_get_with_span("ui/pad", 12.0f32, span, "padding");
    }
    session
        .tweak_set(&json!({"name": "ui/pad", "value": 16.0}))
        .expect("dirty the tweak");
    let server = serve_session_at(&sock, session);

    let client = spawn_server(McpServerOptions {
        socket: Some(sock),
        workspace_root: Some(workspace),
        ..Default::default()
    })
    .await;

    // Non-numeric revision token is a client-side parameter error.
    let msg = call_expect_error(
        &client,
        "martensite_sync_tweaks_to_source",
        json!({
            "tweak_names": ["ui/pad"],
            "dry_run": false,
            "confirmed": true,
            "expected_revision": "not-a-number",
        }),
    )
    .await;
    assert!(
        msg.contains("expected_revision"),
        "non-numeric revision must be an invalid parameter: {msg}"
    );

    // Matching numeric revision + confirmed write commits the splice.
    let res = client
        .call_tool(call(
            "martensite_sync_tweaks_to_source",
            json!({
                "tweak_names": ["ui/pad"],
                "dry_run": false,
                "confirmed": true,
                "expected_revision": "1",
            }),
        ))
        .await
        .expect("sync call");
    assert!(res.is_error != Some(true), "sync must succeed: {res:?}");
    let structured = res.structured_content.clone().expect("structured output");
    assert_eq!(structured["applied"], json!(true));
    assert_eq!(structured["audited"], json!(true), "content: {res:?}");

    let written = std::fs::read_to_string(&target).expect("read back");
    assert_eq!(written, "fn view() {\n    .padding(16.0)\n}\n");

    client.cancel().await.expect("shutdown");
    drop(server);
}
