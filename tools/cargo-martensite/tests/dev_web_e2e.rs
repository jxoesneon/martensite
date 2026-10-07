//! Loopback integration test for the authenticated `dev-web` relay
//! (ADR-0042). A real WebSocket client stands in for the wasm app's
//! dev-channel server leg, and a real Unix-socket client stands in for
//! the local toolchain — the same wire path a browser session takes.
#![cfg(all(unix, feature = "web-dev-channel"))]

use std::io::{BufRead, BufReader, Write};
use std::path::Path;
use std::time::Duration;

use cargo_martensite::web_relay::{
    serve_dev_web, RelayEndpoints, WebRelayConfig, ERR_METHOD_BLOCKED, ERR_NO_UPSTREAM,
    RELAY_WS_PATH,
};
use futures_util::{SinkExt, StreamExt};
use serde_json::{json, Value};
use tokio::net::UnixStream;
use tokio::sync::{mpsc, oneshot};
use tokio_tungstenite::connect_async;
use tokio_tungstenite::tungstenite::client::IntoClientRequest;
use tokio_tungstenite::tungstenite::http::HeaderValue;
use tokio_tungstenite::tungstenite::Message;

/// Sends one dev-channel request line over the relay's Unix socket and
/// reads the single response line back.
async fn uds_request(socket: &Path, line: &str) -> Value {
    use tokio::io::{AsyncBufReadExt, AsyncWriteExt};
    let stream = UnixStream::connect(socket).await.expect("uds connect");
    let (read, mut write) = stream.into_split();
    write
        .write_all(format!("{line}\n").as_bytes())
        .await
        .expect("uds write");
    write.flush().await.expect("uds flush");
    let mut lines = tokio::io::BufReader::new(read).lines();
    let resp = lines
        .next_line()
        .await
        .expect("uds read")
        .expect("uds response line");
    serde_json::from_str(&resp).expect("response is json")
}

/// Spawns the relay on an ephemeral port + tempdir socket and returns its
/// endpoints plus a shutdown handle.
async fn spawn_relay(
    dir: &tempfile::TempDir,
    allow_mutations: bool,
    token: &str,
) -> (RelayEndpoints, oneshot::Sender<()>) {
    let (tx, rx) = oneshot::channel();
    let (shutdown_tx, shutdown_rx) = oneshot::channel();
    let config = WebRelayConfig {
        port: 0,
        socket_path: dir.path().join("dev-web.sock"),
        allow_mutations,
        token: Some(token.to_string()),
    };
    tokio::spawn(serve_dev_web(
        config,
        move |ep| {
            let _ = tx.send(ep.clone());
        },
        async move {
            let _ = shutdown_rx.await;
        },
    ));
    let ep = tokio::time::timeout(Duration::from_secs(10), rx)
        .await
        .expect("relay ready timeout")
        .expect("relay reported endpoints");
    (ep, shutdown_tx)
}

/// Polls a `Read` request until the relay has registered the attached web
/// session — `connect_async` returning only means the WS handshake
/// completed; the relay-side registration runs on a spawned task, so a
/// client racing it can transiently see `ERR_NO_UPSTREAM`. Drains the
/// poll's forwarded request from `seen` before returning.
async fn wait_attached(socket: &Path, seen: &mut mpsc::UnboundedReceiver<Value>) {
    for _ in 0..200 {
        let resp = uds_request(
            socket,
            r#"{"jsonrpc":"2.0","id":0,"method":"tree_snapshot","params":{}}"#,
        )
        .await;
        if resp.get("result").is_some() {
            while seen.try_recv().is_ok() {}
            return;
        }
        tokio::time::sleep(Duration::from_millis(25)).await;
    }
    panic!("relay never registered the attached web session");
}

/// Connects the "wasm app" side and spawns a responder that answers every
/// request with `{ok: true}` while recording the forwarded request lines.
async fn attach_web_session(url: &str, origin: &str) -> mpsc::UnboundedReceiver<Value> {
    let mut req = url.into_client_request().expect("ws request");
    req.headers_mut().insert(
        "Origin",
        HeaderValue::from_str(origin).expect("origin header"),
    );
    let (ws, _resp) = connect_async(req).await.expect("ws upgrade");
    let (seen_tx, seen_rx) = mpsc::unbounded_channel();
    tokio::spawn(async move {
        let (mut write, mut read) = ws.split();
        while let Some(Ok(msg)) = read.next().await {
            if let Message::Text(text) = msg {
                for line in text.split('\n').map(str::trim).filter(|l| !l.is_empty()) {
                    let v: Value = serde_json::from_str(line).expect("forwarded line is json");
                    if let Some(id) = v.get("id").cloned() {
                        seen_tx.send(v).expect("record seen request");
                        let resp = json!({"jsonrpc": "2.0", "id": id, "result": {"ok": true}});
                        write
                            .send(Message::text(resp.to_string()))
                            .await
                            .expect("ws respond");
                    }
                }
            }
        }
    });
    seen_rx
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn relay_requires_token_origin_and_filters_mutations() {
    let dir = tempfile::tempdir().expect("tempdir");
    let (ep, shutdown) = spawn_relay(&dir, false, "test-token").await;
    let socket = ep.socket_path.clone();
    let url = format!(
        "ws://127.0.0.1:{}{}?token=test-token",
        ep.port, RELAY_WS_PATH
    );

    // Unauthenticated upgrade: wrong token and missing token are refused.
    let bad = format!("ws://127.0.0.1:{}{}?token=wrong", ep.port, RELAY_WS_PATH);
    assert!(connect_async(&bad).await.is_err(), "wrong token accepted");
    let bare = format!("ws://127.0.0.1:{}{}", ep.port, RELAY_WS_PATH);
    assert!(
        connect_async(&bare).await.is_err(),
        "missing token accepted"
    );

    // Cross-origin browser page: rejected even with the right token.
    let mut evil_req = url.clone().into_client_request().expect("ws request");
    evil_req.headers_mut().insert(
        "Origin",
        HeaderValue::from_static("https://evil.example.com"),
    );
    assert!(
        connect_async(evil_req).await.is_err(),
        "evil origin accepted"
    );

    // No upstream attached yet: read requests get a structured error.
    let resp = uds_request(
        &socket,
        r#"{"jsonrpc":"2.0","id":1,"method":"tree_snapshot","params":{}}"#,
    )
    .await;
    assert_eq!(resp["error"]["code"], json!(ERR_NO_UPSTREAM));

    // Attach the wasm dev-channel server leg from an allowed loopback
    // origin, then wait for the relay to register it.
    let mut seen = attach_web_session(&url, "http://localhost:8000").await;
    wait_attached(&socket, &mut seen).await;

    // `hello` gets the bearer injected into params.
    let resp = uds_request(
        &socket,
        r#"{"jsonrpc":"2.0","id":42,"method":"hello","params":{"client_version":"0.21.0","protocol_version":1}}"#,
    )
    .await;
    let hello = seen.recv().await.expect("hello forwarded");
    assert_eq!(
        hello["params"]["auth_token"],
        json!("test-token"),
        "relay must inject the bearer token into hello"
    );
    assert_eq!(resp["id"], json!(42), "original request id restored");
    assert_eq!(resp["result"]["ok"], json!(true));

    // Read-classified methods forward and correlate.
    let resp = uds_request(
        &socket,
        r#"{"jsonrpc":"2.0","id":7,"method":"tree_snapshot","params":{"max_depth":2}}"#,
    )
    .await;
    let fwd = seen.recv().await.expect("tree_snapshot forwarded");
    assert_eq!(fwd["method"], json!("tree_snapshot"));
    assert_eq!(resp["id"], json!(7));
    assert_eq!(resp["result"]["ok"], json!(true));

    // Mutate-classified methods are blocked before reaching the app.
    let resp = uds_request(
        &socket,
        r#"{"jsonrpc":"2.0","id":9,"method":"theme.set","params":{"theme":"dark"}}"#,
    )
    .await;
    assert_eq!(resp["error"]["code"], json!(ERR_METHOD_BLOCKED));
    let resp = uds_request(
        &socket,
        r#"{"jsonrpc":"2.0","id":10,"method":"event_dispatch","params":{}}"#,
    )
    .await;
    assert_eq!(resp["error"]["code"], json!(ERR_METHOD_BLOCKED));
    // Unknown methods are conservatively treated as Mutate too.
    let resp = uds_request(
        &socket,
        r#"{"jsonrpc":"2.0","id":11,"method":"totally.unknown","params":{}}"#,
    )
    .await;
    assert_eq!(resp["error"]["code"], json!(ERR_METHOD_BLOCKED));

    let _ = shutdown.send(());
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn relay_allow_mutations_forwards_mutate_methods() {
    let dir = tempfile::tempdir().expect("tempdir");
    let (ep, shutdown) = spawn_relay(&dir, true, "test-token").await;
    let socket = ep.socket_path.clone();

    let mut seen = attach_web_session(&ep.ws_url(), "http://127.0.0.1:8000").await;
    wait_attached(&socket, &mut seen).await;

    let resp = uds_request(
        &socket,
        r#"{"jsonrpc":"2.0","id":5,"method":"theme.set","params":{"theme":"dark"}}"#,
    )
    .await;
    let fwd = seen.recv().await.expect("theme.set forwarded");
    assert_eq!(fwd["method"], json!("theme.set"));
    assert_eq!(resp["id"], json!(5));
    assert_eq!(resp["result"]["ok"], json!(true));

    let _ = shutdown.send(());
}

/// The relay binds the dev socket with dev-channel permissions (0600).
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn relay_socket_is_owner_only() {
    use std::os::unix::fs::PermissionsExt;
    let dir = tempfile::tempdir().expect("tempdir");
    let (ep, shutdown) = spawn_relay(&dir, false, "t").await;
    let mode = std::fs::metadata(&ep.socket_path)
        .expect("socket exists")
        .permissions()
        .mode()
        & 0o777;
    assert_eq!(mode, 0o600);
    let _ = shutdown.send(());
}

/// Sanity check that an ordinary blocking std client (what the existing
/// toolchain uses) interoperates with the relay socket.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn std_unix_client_interops() {
    let dir = tempfile::tempdir().expect("tempdir");
    let (ep, shutdown) = spawn_relay(&dir, false, "test-token").await;
    let socket = ep.socket_path.clone();
    let mut seen = attach_web_session(&ep.ws_url(), "http://localhost").await;
    wait_attached(&socket, &mut seen).await;

    let socket2 = socket.clone();
    let resp = tokio::task::spawn_blocking(move || {
        let mut stream = std::os::unix::net::UnixStream::connect(&socket2).expect("connect");
        stream
            .write_all(br#"{"jsonrpc":"2.0","id":3,"method":"reload_status","params":{}}"#)
            .expect("write");
        stream.write_all(b"\n").expect("nl");
        let mut line = String::new();
        BufReader::new(stream).read_line(&mut line).expect("read");
        serde_json::from_str::<Value>(&line).expect("json")
    })
    .await
    .expect("blocking join");
    assert_eq!(resp["id"], json!(3));
    assert_eq!(resp["result"]["ok"], json!(true));

    let _ = shutdown.send(());
}
