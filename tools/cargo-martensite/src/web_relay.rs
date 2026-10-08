//! Authenticated loopback WebSocket bridge for the dev channel (ADR-0042).
//!
//! `cargo martensite dev-web` exposes a running *browser-hosted* Martensite
//! app's dev channel to the existing local toolchain. The wasm app cannot
//! open a listening socket, so the direction is reversed: the app dials
//! **out** to this relay over `ws://127.0.0.1:<port>/dev-channel?token=<b>`
//! and acts as the dev-channel *server* over that client-initiated socket.
//! The relay then binds the ordinary dev-channel Unix socket and pipes
//! newline-delimited JSON-RPC lines between local tools and the app.
//!
//! # Auth model
//!
//! - The WS listener binds `127.0.0.1` only — never a wildcard or bare
//!   `localhost` (an IPv6/IPv4 split would bypass the loopback intent).
//! - The upgrade request must carry the per-run bearer `token` query
//!   parameter, printed to the console when the relay starts.
//! - The `Origin` header, when present, is checked against a loopback
//!   allowlist — a cross-origin browser page cannot drive the channel.
//! - Only [`MethodClass::Read`](martensite_host::dev_channel::MethodClass::Read)
//!   requests are forwarded by default;
//!   `--allow-mutations` unlocks the full table.
//! - The token is injected into forwarded `hello` requests as
//!   `auth_token`, so a wasm app that requires the bearer (see
//!   `WebChannelOptions::required_token`) keeps end-to-end authentication
//!   without the local tools knowing the token.
//!
//! The whole module is behind the `web-dev-channel` feature and is never
//! enabled by default.

use std::fmt;
use std::future::Future;
use std::io;
use std::path::PathBuf;

use martensite_host::dev_channel::socket_path_for_session;

#[cfg(unix)]
use std::collections::HashMap;
#[cfg(unix)]
use std::net::{Ipv4Addr, SocketAddr, SocketAddrV4};
#[cfg(unix)]
use std::os::unix::fs::PermissionsExt;
#[cfg(unix)]
use std::path::Path;
#[cfg(unix)]
use std::sync::atomic::{AtomicU64, Ordering};
#[cfg(unix)]
use std::sync::{Arc, Mutex};
#[cfg(unix)]
use std::time::Duration;

#[cfg(unix)]
use futures_util::{SinkExt, StreamExt};
#[cfg(unix)]
use martensite_host::dev_channel::{method_class, MethodClass};
#[cfg(unix)]
use rand_core::{OsRng, RngCore};
#[cfg(unix)]
use serde_json::Value;
#[cfg(unix)]
use tokio::net::TcpListener;
#[cfg(unix)]
use tokio::sync::{mpsc, oneshot};
#[cfg(unix)]
use tokio_tungstenite::accept_hdr_async;
#[cfg(unix)]
use tokio_tungstenite::tungstenite::handshake::server::{ErrorResponse, Request, Response};
#[cfg(unix)]
use tokio_tungstenite::tungstenite::http;
#[cfg(unix)]
use tokio_tungstenite::tungstenite::Message;

/// WebSocket path the relay serves the dev channel on.
pub const RELAY_WS_PATH: &str = "/dev-channel";

/// JSON-RPC error returned when a `Mutate`-classified method reaches a
/// read-only relay.
pub const ERR_METHOD_BLOCKED: i32 = -32010;
/// JSON-RPC error returned when no web session is attached to the relay.
pub const ERR_NO_UPSTREAM: i32 = -32011;
/// JSON-RPC error returned when the attached session stops answering.
pub const ERR_UPSTREAM_TIMEOUT: i32 = -32012;

/// How long a forwarded request may wait for the web session before the
/// relay synthesizes [`ERR_UPSTREAM_TIMEOUT`].
const UPSTREAM_TIMEOUT: Duration = Duration::from_secs(30);

/// Capacity of the wasm-bound frame queue per attached session. Local
/// tools can burst requests faster than a browser tab drains them; the
/// bound keeps that back-pressure honest — a full queue fails the
/// request (or drops the notification) instead of growing memory.
#[cfg(unix)]
const UPSTREAM_TX_CAPACITY: usize = 256;

/// Configuration for the `dev-web` relay.
///
/// # Examples
///
/// ```
/// use cargo_martensite::web_relay::WebRelayConfig;
///
/// let config = WebRelayConfig::default();
/// assert!(!config.allow_mutations);
/// assert!(config.token.is_none());
/// ```
#[derive(Clone)]
pub struct WebRelayConfig {
    /// WebSocket port bound on `127.0.0.1` (`0` asks the OS for a free
    /// port — used by tests).
    pub port: u16,
    /// Dev-channel Unix socket path the relay binds for local tools.
    pub socket_path: PathBuf,
    /// Forward [`Mutate`](martensite_host::dev_channel::MethodClass::Mutate)
    /// requests as well as `Read` ones. Off by default — the relay is
    /// read-only.
    pub allow_mutations: bool,
    /// Bearer token required on the WS upgrade. `None` generates a fresh
    /// per-run token — the supported operator flow.
    pub token: Option<String>,
}

// `token` is the WS-upgrade bearer — Debug redacts it so a logged
// config dump cannot leak the credential.
impl fmt::Debug for WebRelayConfig {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("WebRelayConfig")
            .field("port", &self.port)
            .field("socket_path", &self.socket_path)
            .field("allow_mutations", &self.allow_mutations)
            .field("token", &self.token.as_ref().map(|_| "[redacted]"))
            .finish()
    }
}

impl Default for WebRelayConfig {
    fn default() -> Self {
        Self {
            port: crate::cli::DEFAULT_WEB_RELAY_PORT,
            socket_path: socket_path_for_session("web"),
            allow_mutations: false,
            token: None,
        }
    }
}

/// Bound endpoints reported once the relay is listening.
///
/// # Examples
///
/// ```
/// use cargo_martensite::web_relay::RelayEndpoints;
/// use std::path::PathBuf;
///
/// let ep = RelayEndpoints {
///     port: 8788,
///     socket_path: PathBuf::from("/tmp/martensite/web.sock"),
///     token: "deadbeef".to_string(),
/// };
/// assert!(ep.ws_url().ends_with("token=deadbeef"));
/// ```
#[derive(Clone)]
pub struct RelayEndpoints {
    /// TCP port the WS listener actually bound (post-`0` resolution).
    pub port: u16,
    /// Dev-channel socket path tools connect to.
    pub socket_path: PathBuf,
    /// The bearer token a wasm app must present as `?token=`.
    pub token: String,
}

// `token` is the WS-upgrade bearer — Debug redacts it so a logged
// endpoints dump cannot leak the credential. `ws_url()` still returns
// the real dial URL; only the Debug surface is scrubbed.
impl fmt::Debug for RelayEndpoints {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("RelayEndpoints")
            .field("port", &self.port)
            .field("socket_path", &self.socket_path)
            .field("token", &"[redacted]")
            .finish()
    }
}

impl RelayEndpoints {
    /// The full `ws://` URL a wasm app should dial (e.g. via
    /// `martensite_devtools::web_channel`).
    pub fn ws_url(&self) -> String {
        format!(
            "ws://127.0.0.1:{}{}?token={}",
            self.port, RELAY_WS_PATH, self.token
        )
    }
}

/// Errors produced while setting up or running the relay.
///
/// # Examples
///
/// ```
/// use cargo_martensite::web_relay::RelayError;
///
/// let err = RelayError::UnsupportedPlatform;
/// assert!(err.to_string().contains("unix"));
/// ```
#[derive(Debug)]
pub enum RelayError {
    /// Underlying I/O failure (listener bind, socket setup, …).
    Io(io::Error),
    /// The tool-facing socket needs a Unix domain socket; other platforms
    /// are not yet bridged.
    UnsupportedPlatform,
}

impl fmt::Display for RelayError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            RelayError::Io(e) => write!(f, "dev-web relay I/O error: {e}"),
            RelayError::UnsupportedPlatform => write!(
                f,
                "the dev-web relay requires a unix domain socket; this platform is unsupported"
            ),
        }
    }
}

impl std::error::Error for RelayError {}

impl From<io::Error> for RelayError {
    fn from(e: io::Error) -> Self {
        RelayError::Io(e)
    }
}

/// Generates the per-run bearer token: 256 bits of OS entropy, hex encoded.
#[cfg(unix)]
fn generate_token() -> String {
    let mut bytes = [0u8; 32];
    OsRng.fill_bytes(&mut bytes);
    hex::encode(bytes)
}

/// Extracts the bearer token from a WS upgrade request query string.
///
/// Both `token` and `dev_token` keys are accepted.
#[cfg(unix)]
fn token_from_query(query: Option<&str>) -> Option<&str> {
    query?.split('&').find_map(|pair| {
        pair.strip_prefix("token=")
            .or_else(|| pair.strip_prefix("dev_token="))
    })
}

/// Validates the `Origin` header against the loopback allowlist.
///
/// Allowed: absent (non-browser local clients — the bearer token still
/// gates them), `null` (file:// and sandboxed local pages), and
/// `http(s)://127.0.0.1:*` / `http(s)://localhost:*` / `http(s)://[::1]:*`.
/// Anything else is rejected before the upgrade completes.
#[cfg(unix)]
fn origin_allowed(origin: Option<&str>) -> bool {
    let Some(origin) = origin else {
        return true;
    };
    let origin = origin.trim();
    if origin == "null" {
        return true;
    }
    let Some(rest) = origin
        .strip_prefix("http://")
        .or_else(|| origin.strip_prefix("https://"))
    else {
        return false;
    };
    let host_port = rest.split('/').next().unwrap_or("");
    let host = if let Some(bracketed) = host_port.strip_prefix('[') {
        bracketed.split(']').next().unwrap_or("")
    } else {
        host_port.split(':').next().unwrap_or("")
    };
    matches!(host, "127.0.0.1" | "localhost" | "::1")
}

/// One attached web session: the WebSocket the wasm app dialed in on,
/// plus the request-correlation table local tools' requests wait on.
#[cfg(unix)]
struct Upstream {
    /// Lines to transmit as WS text frames. Bounded: a flooded queue
    /// fails the waiter rather than buffering unboundedly.
    tx: mpsc::Sender<Message>,
    /// Relay-assigned request id → waiter for the response line.
    pending: Mutex<HashMap<u64, oneshot::Sender<String>>>,
    /// Relay-assigned request id counter (clients' ids collide across
    /// connections, so requests are renumbered on the way out and the
    /// original id restored on the way back).
    next_id: AtomicU64,
}

#[cfg(unix)]
impl Upstream {
    /// Fails every waiter, e.g. when the web session disconnects.
    fn fail_all_pending(&self) {
        self.pending
            .lock()
            .expect("upstream pending mutex")
            .drain()
            .for_each(|(_, tx)| drop(tx));
    }
}

/// The currently attached web session, if any. A newer connection
/// replaces a stale one (page reloads churn sockets).
#[cfg(unix)]
type SharedUpstream = Arc<Mutex<Option<Arc<Upstream>>>>;

/// Serializes a JSON-RPC error response line the relay itself answers.
#[cfg(unix)]
fn relay_error_line(id: Option<Value>, code: i32, message: String) -> String {
    serde_json::json!({
        "jsonrpc": "2.0",
        "id": id.unwrap_or(Value::Null),
        "error": { "code": code, "message": message },
    })
    .to_string()
}

/// Binds the tool-facing Unix socket with dev-channel permissions
/// (`0700` parent dir, `0600` socket).
#[cfg(unix)]
fn bind_dev_socket(path: &Path) -> io::Result<tokio::net::UnixListener> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
        std::fs::set_permissions(parent, std::fs::Permissions::from_mode(0o700))?;
    }
    if path.exists() {
        std::fs::remove_file(path)?;
    }
    let listener = tokio::net::UnixListener::bind(path)?;
    std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o600))?;
    Ok(listener)
}

/// Serves the authenticated loopback bridge until `shutdown` resolves.
///
/// `on_ready` fires once both listeners are bound — the CLI prints the
/// bearer token and endpoints there, tests capture them.
///
/// On Unix the tool-facing side is a Unix domain socket at
/// `config.socket_path`; other platforms are rejected with
/// [`RelayError::UnsupportedPlatform`].
///
/// # Examples
///
/// ```no_run
/// use cargo_martensite::web_relay::{serve_dev_web, WebRelayConfig};
///
/// # async fn demo() {
/// serve_dev_web(
///     WebRelayConfig::default(),
///     |ep| println!("relay token: {}", ep.token),
///     async { let _ = tokio::signal::ctrl_c().await; },
/// )
/// .await
/// .unwrap();
/// # }
/// ```
#[cfg(unix)]
pub async fn serve_dev_web<F, S>(
    config: WebRelayConfig,
    on_ready: F,
    shutdown: S,
) -> Result<(), RelayError>
where
    F: FnOnce(&RelayEndpoints),
    S: Future<Output = ()>,
{
    let token = config.token.unwrap_or_else(generate_token);

    // Explicit v4 loopback — never bare "localhost": a dual-stack bind
    // would silently expose ::1 to origins the allowlist never saw.
    let ws_listener = TcpListener::bind(SocketAddr::V4(SocketAddrV4::new(
        Ipv4Addr::LOCALHOST,
        config.port,
    )))
    .await?;
    let port = ws_listener.local_addr()?.port();

    let uds_listener = bind_dev_socket(&config.socket_path)?;

    let upstream: SharedUpstream = Arc::new(Mutex::new(None));
    let ws_task = tokio::spawn(ws_accept_loop(
        ws_listener,
        Arc::clone(&upstream),
        token.clone(),
    ));
    let uds_task = tokio::spawn(uds_accept_loop(
        uds_listener,
        Arc::clone(&upstream),
        config.allow_mutations,
        token.clone(),
    ));

    on_ready(&RelayEndpoints {
        port,
        socket_path: config.socket_path.clone(),
        token,
    });

    shutdown.await;

    ws_task.abort();
    uds_task.abort();
    let _ = std::fs::remove_file(&config.socket_path);
    Ok(())
}

/// Non-unix stub: the tool-facing side of the bridge needs a Unix domain
/// socket, so the relay reports an explicit unsupported-platform error.
#[cfg(not(unix))]
pub async fn serve_dev_web<F, S>(
    _config: WebRelayConfig,
    _on_ready: F,
    _shutdown: S,
) -> Result<(), RelayError>
where
    F: FnOnce(&RelayEndpoints),
    S: Future<Output = ()>,
{
    Err(RelayError::UnsupportedPlatform)
}

/// Accepts WS upgrade requests on `listener`; each authenticated
/// connection becomes the current upstream web session.
#[cfg(unix)]
async fn ws_accept_loop(listener: TcpListener, shared: SharedUpstream, token: String) {
    loop {
        match listener.accept().await {
            Ok((stream, _)) => {
                let shared = Arc::clone(&shared);
                let token = token.clone();
                tokio::spawn(async move {
                    let _ = accept_ws_session(stream, shared, &token).await;
                });
            }
            Err(_) => tokio::time::sleep(Duration::from_millis(5)).await,
        }
    }
}

/// Validates the upgrade request (path, bearer token, Origin allowlist)
/// then runs the WebSocket as the dev-channel server endpoint until close.
#[cfg(unix)]
// The handshake callback's Err variant is `ErrorResponse` (an `http`
// response) — the size is dictated by `accept_hdr_async`'s signature.
#[allow(clippy::result_large_err)]
async fn accept_ws_session(
    stream: tokio::net::TcpStream,
    shared: SharedUpstream,
    token: &str,
) -> Result<(), ()> {
    let ws = accept_hdr_async(stream, |req: &Request, resp: Response| {
        let reject = |status: u16, body: &str| -> ErrorResponse {
            http::Response::builder()
                .status(status)
                .body(Some(body.to_string()))
                .expect("static rejection response builds")
        };
        if req.uri().path() != RELAY_WS_PATH {
            return Err(reject(404, "not found"));
        }
        if token_from_query(req.uri().query()) != Some(token) {
            return Err(reject(403, "missing or invalid dev channel token"));
        }
        let origin = req
            .headers()
            .get(http::header::ORIGIN)
            .and_then(|v| v.to_str().ok());
        if !origin_allowed(origin) {
            return Err(reject(403, "origin not allowed"));
        }
        Ok(resp)
    })
    .await
    .map_err(|_| ())?;

    let (mut sink, mut read) = ws.split();
    let (tx, mut rx) = mpsc::channel::<Message>(UPSTREAM_TX_CAPACITY);
    let upstream = Arc::new(Upstream {
        tx,
        pending: Mutex::new(HashMap::new()),
        next_id: AtomicU64::new(1),
    });

    // Latest wins: a page reload dials a fresh socket; retire the old one.
    if let Some(old) = shared
        .lock()
        .expect("upstream mutex")
        .replace(Arc::clone(&upstream))
    {
        let _ = old.tx.try_send(Message::Close(None));
        old.fail_all_pending();
    }

    let writer = tokio::spawn(async move {
        while let Some(msg) = rx.recv().await {
            if sink.send(msg).await.is_err() {
                break;
            }
        }
    });

    while let Some(Ok(msg)) = read.next().await {
        if let Message::Text(text) = msg {
            for line in text.split('\n').map(str::trim).filter(|l| !l.is_empty()) {
                if let Some(id) = serde_json::from_str::<Value>(line)
                    .ok()
                    .and_then(|v| v.get("id").and_then(Value::as_u64))
                {
                    if let Some(waiter) = upstream
                        .pending
                        .lock()
                        .expect("upstream pending mutex")
                        .remove(&id)
                    {
                        let _ = waiter.send(line.to_string());
                    }
                }
            }
        }
    }

    // Disconnect: detach ourselves (if still current) and fail waiters.
    let mut guard = shared.lock().expect("upstream mutex");
    if guard
        .as_ref()
        .map(|u| Arc::ptr_eq(u, &upstream))
        .unwrap_or(false)
    {
        *guard = None;
    }
    drop(guard);
    upstream.fail_all_pending();
    writer.abort();
    Ok(())
}

/// Accepts local dev-channel clients and serves them from the attached
/// web session.
#[cfg(unix)]
async fn uds_accept_loop(
    listener: tokio::net::UnixListener,
    shared: SharedUpstream,
    allow_mutations: bool,
    token: String,
) {
    loop {
        match listener.accept().await {
            Ok((stream, _)) => {
                let shared = Arc::clone(&shared);
                let token = token.clone();
                tokio::spawn(async move {
                    let _ = bridge_uds_client(stream, shared, allow_mutations, &token).await;
                });
            }
            Err(_) => tokio::time::sleep(Duration::from_millis(5)).await,
        }
    }
}

/// Bridges one dev-channel client connection: request lines in, response
/// lines out, with the read-only method filter applied per line.
#[cfg(unix)]
async fn bridge_uds_client(
    stream: tokio::net::UnixStream,
    shared: SharedUpstream,
    allow_mutations: bool,
    token: &str,
) -> io::Result<()> {
    use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};

    let (read, mut write) = stream.into_split();
    let mut lines = BufReader::new(read).lines();

    while let Some(line) = lines.next_line().await? {
        let line = line.trim();
        if line.is_empty() {
            continue;
        }
        if let Some(resp) = route_line(line, &shared, allow_mutations, token).await {
            let mut out = resp;
            out.push('\n');
            write.write_all(out.as_bytes()).await?;
            write.flush().await?;
        }
    }
    Ok(())
}

/// Routes one client request line: applies the method-classification
/// filter, injects the bearer into `hello`, forwards to the attached web
/// session, and restores the client's original request id on the reply.
#[cfg(unix)]
async fn route_line(
    line: &str,
    shared: &SharedUpstream,
    allow_mutations: bool,
    token: &str,
) -> Option<String> {
    let Ok(mut req) = serde_json::from_str::<Value>(line) else {
        return Some(relay_error_line(
            None,
            martensite_host::dev_channel::ERR_PARSE,
            "parse error: invalid JSON-RPC payload".to_string(),
        ));
    };
    let has_id = req.get("id").is_some();
    let orig_id = req.get("id").cloned();
    let method = req
        .get("method")
        .and_then(Value::as_str)
        .unwrap_or("")
        .to_string();

    // Read-only default: mutation-classified methods never leave the relay.
    if !allow_mutations && method_class(&method) == MethodClass::Mutate {
        return has_id.then(|| {
            relay_error_line(
                orig_id,
                ERR_METHOD_BLOCKED,
                format!(
                    "method_blocked: `{method}` is classified Mutate; \
                     restart `cargo martensite dev-web` with --allow-mutations to enable it"
                ),
            )
        });
    }

    let upstream = shared.lock().expect("upstream mutex").clone();
    let Some(up) = upstream else {
        return has_id.then(|| {
            relay_error_line(
                orig_id,
                ERR_NO_UPSTREAM,
                "no web dev session is attached to the relay".to_string(),
            )
        });
    };

    // The bearer survives the bridge inside the protocol: the wasm leg can
    // require it on `hello` even though the socket is client-initiated.
    if method.to_ascii_lowercase().replace(['_', '-', ' '], "") == "hello" {
        if !req.get("params").is_some_and(Value::is_object) {
            req["params"] = serde_json::json!({});
        }
        if let Some(params) = req.get_mut("params").and_then(Value::as_object_mut) {
            params.insert("auth_token".to_string(), Value::String(token.to_string()));
        }
    }

    if has_id {
        // Renumber so concurrently bridged clients cannot collide, then
        // correlate the response and restore the client's id.
        let up_id = up.next_id.fetch_add(1, Ordering::SeqCst);
        req["id"] = Value::from(up_id);
        let (tx, rx) = oneshot::channel();
        up.pending
            .lock()
            .expect("upstream pending mutex")
            .insert(up_id, tx);
        if up.tx.try_send(Message::text(req.to_string())).is_err() {
            // Queue full or session gone: fail this request and drop its
            // waiter instead of buffering unboundedly.
            eprintln!(
                "dev-web relay: dropping `{method}` — wasm-bound queue is full \
                 or the session detached"
            );
            up.pending
                .lock()
                .expect("upstream pending mutex")
                .remove(&up_id);
            return Some(relay_error_line(
                orig_id,
                ERR_NO_UPSTREAM,
                "the web dev session detached from the relay".to_string(),
            ));
        }
        match tokio::time::timeout(UPSTREAM_TIMEOUT, rx).await {
            Ok(Ok(resp_line)) => match serde_json::from_str::<Value>(&resp_line) {
                Ok(mut resp) => {
                    if let Some(obj) = resp.as_object_mut() {
                        obj.insert("id".to_string(), orig_id.unwrap_or(Value::Null));
                    }
                    Some(resp.to_string())
                }
                Err(_) => Some(resp_line),
            },
            Ok(Err(_)) => Some(relay_error_line(
                orig_id,
                ERR_NO_UPSTREAM,
                "the web dev session detached from the relay".to_string(),
            )),
            Err(_) => {
                up.pending
                    .lock()
                    .expect("upstream pending mutex")
                    .remove(&up_id);
                Some(relay_error_line(
                    orig_id,
                    ERR_UPSTREAM_TIMEOUT,
                    format!("web dev session did not answer `{method}` within 30s"),
                ))
            }
        }
    } else {
        // Notification: forwarded without a waiter — a full queue drops
        // it (no `id` exists to answer with an error).
        if up.tx.try_send(Message::text(req.to_string())).is_err() {
            eprintln!(
                "dev-web relay: dropping `{method}` notification — wasm-bound queue \
                 is full or the session detached"
            );
        }
        None
    }
}

#[cfg(all(test, unix))]
mod tests {
    use super::*;

    #[test]
    fn token_from_query_finds_token_and_dev_token() {
        assert_eq!(token_from_query(Some("token=abc")), Some("abc"));
        assert_eq!(token_from_query(Some("dev_token=xyz")), Some("xyz"));
        assert_eq!(token_from_query(Some("a=1&token=t2&b=3")), Some("t2"));
        assert_eq!(token_from_query(Some("a=1")), None);
        assert_eq!(token_from_query(None), None);
        assert_eq!(token_from_query(Some("token=")), Some(""));
    }

    #[test]
    fn origin_allowlist() {
        assert!(origin_allowed(None));
        assert!(origin_allowed(Some("null")));
        assert!(origin_allowed(Some("http://127.0.0.1:8000")));
        assert!(origin_allowed(Some("http://localhost:8000")));
        assert!(origin_allowed(Some("https://127.0.0.1")));
        assert!(origin_allowed(Some("http://[::1]:9000")));
        assert!(!origin_allowed(Some("http://evil.example.com")));
        assert!(!origin_allowed(Some("https://attacker.io:443")));
        assert!(!origin_allowed(Some("http://127.0.0.1.evil.com")));
        assert!(!origin_allowed(Some("file:///etc/passwd")));
        assert!(!origin_allowed(Some("ftp://localhost")));
        assert!(!origin_allowed(Some("localhost:8000")));
        assert!(!origin_allowed(Some("")));
    }

    #[test]
    fn generate_token_is_64_hex_chars_and_unique() {
        let a = generate_token();
        let b = generate_token();
        assert_eq!(a.len(), 64);
        assert!(a.chars().all(|c| c.is_ascii_hexdigit()));
        assert_ne!(a, b);
    }

    /// Extracts the quoted entries of a `&[..]` table from `src` between
    /// `marker` (the table's declaration text) and the closing `];`.
    fn table_entries(src: &str, marker: &str) -> Vec<String> {
        let start = src
            .find(marker)
            .unwrap_or_else(|| panic!("`{marker}` not found in source"));
        let end = src[start..]
            .find("];")
            .map(|i| start + i)
            .expect("table terminator");
        src[start..end]
            .split('"')
            .skip(1)
            .step_by(2)
            .map(str::to_string)
            .collect()
    }

    /// Method-table parity gate for the wasm leg (ADR-0042): the served
    /// method set a `WebDevChannel` consumer dispatches —
    /// `martensite_devtools::web_channel::DEV_CHANNEL_METHODS`, consumed
    /// by `examples/widget_catalog::web_dev` — must equal the host
    /// dispatcher's `METHOD_CLASSES` key set minus `hello` (which the
    /// leg owns at the handshake gate). Neither table is visible to the
    /// other side as code (`METHOD_CLASSES` is private, `web_channel` is
    /// wasm-gated), so the comparison runs over source text — the one
    /// surface where both compile together. A host-side RPC added
    /// without updating the wasm list fails here, which is the signal
    /// that the web leg would silently `-32601` the new method.
    #[test]
    fn wasm_dev_channel_method_table_matches_host() {
        const HOST_SRC: &str = include_str!("../../../crates/martensite-host/src/dev_channel.rs");
        const WASM_SRC: &str =
            include_str!("../../../crates/martensite-devtools/src/web_channel.rs");
        let host: std::collections::BTreeSet<String> =
            table_entries(HOST_SRC, "const METHOD_CLASSES")
                .into_iter()
                .filter(|m| m != "hello")
                .collect();
        let wasm: std::collections::BTreeSet<String> =
            table_entries(WASM_SRC, "pub const DEV_CHANNEL_METHODS")
                .into_iter()
                .collect();
        assert_eq!(
            wasm, host,
            "wasm dev-channel method table drifted from the host METHOD_CLASSES key set"
        );
    }
}
