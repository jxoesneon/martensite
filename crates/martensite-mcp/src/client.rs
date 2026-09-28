//! ADR-0038 dev-channel client for the Martensite MCP server.
//!
//! Implements the same newline-delimited JSON-RPC-lite protocol used by
//! `cargo-martensite`'s dev channel: a local-only, version-handshook Unix
//! domain socket (Windows named pipe) connecting the MCP server to a running
//! Martensite development session.
//!
//! # Protocol
//!
//! - **Transport**: Unix domain socket at `$XDG_RUNTIME_DIR/martensite/<id>.sock`
//!   (or `/tmp/martensite/<id>.sock`); named pipe on Windows.
//! - **Framing**: newline-delimited [`DevRequest`]/[`DevResponse`] JSON.
//! - **Handshake**: mandatory `hello` call enforcing `MARTENSITE_VERSION` and
//!   `protocol` parity (D1). Mismatches fail fast.
//! - **Core methods**: `hello`, `lint_pull`, `lint_apply`, `tree_snapshot`,
//!   `inspector_select`.
//! - **Extension methods**: the remaining tool domains forward over the same
//!   channel (`tree_node`, `layout_chain`, `overflow_scan`, `signals_list`,
//!   `signal_trigger`, `signal_set`, `a11y_tree`, `a11y_action`,
//!   `tweaks_list`, `tweak_set`, `theme_set`, `tweaks_sync`, `event_dispatch`,
//!   `event_ledger`, `timemachine_step`, `capture_node`, `reload_status`,
//!   `runtime_errors`, `logs`, `hot_reload`). A dev app built against an
//!   older `martensite-devtools` answers `-32601` (method not found), which
//!   surfaces as [`McpError::Ipc`]

use std::io::{BufRead, BufReader, Write};
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::error::McpError;
use crate::types::{DEV_CHANNEL_PROTOCOL_VERSION, MARTENSITE_VERSION};

#[cfg(unix)]
use std::os::unix::net::UnixStream;

#[cfg(unix)]
type TransportStream = UnixStream;

#[cfg(windows)]
type TransportStream = std::fs::File;

/// JSON-RPC-lite request sent to the running dev app.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DevRequest {
    /// Request identifier matching the response.
    pub id: u64,
    /// RPC method name.
    pub method: String,
    /// Request parameters payload.
    #[serde(default)]
    pub params: serde_json::Value,
}

/// JSON-RPC-lite response returned by the dev app.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DevResponse {
    /// Request identifier matching the request.
    pub id: u64,
    /// Successful result payload, if any.
    #[serde(default)]
    pub result: Option<serde_json::Value>,
    /// Error payload, if any.
    #[serde(default)]
    pub error: Option<DevError>,
}

/// Error details returned in a [`DevResponse`].
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct DevError {
    /// Numeric error code (standard JSON-RPC or application-specific).
    pub code: i32,
    /// Human-readable error description.
    pub message: String,
    /// Structured error data, if available.
    #[serde(default)]
    pub data: Option<serde_json::Value>,
}

/// Wire representation of one widget tree node returned by `tree_snapshot`.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct WireTreeNode {
    /// Widget arena identifier integer.
    pub id: u64,
    /// Optional debug name (`ColdNode::debug_name`).
    #[serde(default)]
    pub debug_name: Option<String>,
    /// Node kind classification (e.g. `Container`, `Text`, `Flex`).
    #[serde(default)]
    pub kind: String,
    /// Bounding rectangle in screen coordinates `[x, y, width, height]`.
    pub screen_bounds: [f32; 4],
    /// Bounding rectangle relative to parent `[x, y, width, height]`.
    #[serde(default)]
    pub local_bounds: [f32; 4],
    /// Depth rank in hierarchy (0 = root).
    #[serde(default)]
    pub depth: usize,
    /// Total direct children.
    #[serde(default)]
    pub child_count: usize,
    /// Active badges (warnings, dirty flags, etc.).
    #[serde(default)]
    pub badges: Vec<String>,
    /// Number of active reactive signals bound to the widget.
    #[serde(default)]
    pub active_signal_count: usize,
    /// Materialized child nodes.
    #[serde(default)]
    pub children: Vec<WireTreeNode>,
}

/// A step in a widget's layout constraint chain (wire format).
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct LayoutStep {
    /// Step description (widget name or constraint origin).
    pub name: String,
    /// Textual constraint summary.
    #[serde(default)]
    pub constraints: String,
    /// Resulting dimensions `[width, height]`.
    pub result_size: [f32; 2],
    /// Constraint violations or warnings at this step.
    #[serde(default)]
    pub violations: Vec<String>,
}

/// Snapshot payload returned by `tree_snapshot`.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct TreeSnapshotData {
    /// Root node of the widget tree.
    pub root: WireTreeNode,
    /// Currently selected widget id, if any.
    #[serde(default)]
    pub selected_id: Option<u64>,
    /// Constraint layout chain for the selected widget.
    #[serde(default)]
    pub layout_chain: Vec<LayoutStep>,
    /// Active signal summaries.
    #[serde(default)]
    pub signals: Vec<String>,
}

/// Inspection payload returned by `inspector_select`.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct InspectSelectData {
    /// The clicked/selected tree node.
    pub selected_node: WireTreeNode,
    /// Constraint layout chain for the selected node.
    #[serde(default)]
    pub layout_chain: Vec<LayoutStep>,
    /// Extracted properties and markers for the selected node.
    #[serde(default)]
    pub properties: std::collections::BTreeMap<String, String>,
}

/// Default runtime directory hosting Martensite dev-channel sockets.
#[must_use]
pub fn default_socket_dir() -> PathBuf {
    if let Ok(runtime_dir) = std::env::var("XDG_RUNTIME_DIR") {
        if !runtime_dir.is_empty() {
            return PathBuf::from(runtime_dir).join("martensite");
        }
    }
    std::env::temp_dir().join("martensite")
}

/// Discovers all available dev session sockets or named pipes.
pub fn discover_dev_sessions() -> Result<Vec<PathBuf>, McpError> {
    let mut sessions = Vec::new();

    #[cfg(windows)]
    {
        if let Ok(entries) = std::fs::read_dir(r"\\.\pipe\") {
            for entry in entries.flatten() {
                let name = entry.file_name().to_string_lossy().to_string();
                if name.starts_with("martensite-") {
                    sessions.push(PathBuf::from(format!(r"\\.\pipe\{name}")));
                }
            }
        }
    }

    let dir = default_socket_dir();
    if dir.exists() {
        if let Ok(entries) = std::fs::read_dir(&dir) {
            for entry in entries.flatten() {
                let p = entry.path();
                if p.extension().and_then(|s| s.to_str()) == Some("sock") && !sessions.contains(&p)
                {
                    sessions.push(p);
                }
            }
        }
    }

    Ok(sessions)
}

/// Discovers an active dev-channel socket path.
///
/// Priority:
/// 1. Explicit path from `McpServerOptions::socket`.
/// 2. `MARTENSITE_DEV_SOCKET` environment variable.
/// 3. Search [`discover_dev_sessions`], probing live sessions.
///
/// Returns `Ok(None)` when no session exists (offline mode is valid).
pub fn discover_socket(explicit: Option<&Path>) -> Result<Option<PathBuf>, McpError> {
    if let Some(path) = explicit {
        let usable = {
            #[cfg(windows)]
            {
                path.to_string_lossy().starts_with(r"\\.\pipe\") || path.exists()
            }
            #[cfg(not(windows))]
            {
                path.exists()
            }
        };
        return if usable {
            Ok(Some(path.to_path_buf()))
        } else {
            Err(McpError::Ipc(format!(
                "specified socket `{}` does not exist",
                path.display()
            )))
        };
    }

    if let Ok(env_path) = std::env::var("MARTENSITE_DEV_SOCKET") {
        let p = PathBuf::from(env_path);
        let usable = {
            #[cfg(windows)]
            {
                p.to_string_lossy().starts_with(r"\\.\pipe\") || p.exists()
            }
            #[cfg(not(windows))]
            {
                p.exists()
            }
        };
        if usable {
            return Ok(Some(p));
        }
    }

    let mut sessions = discover_dev_sessions()?;
    if sessions.is_empty() {
        return Ok(None);
    }

    // Most recently written socket first.
    sessions.sort_by_key(|p| {
        std::fs::metadata(p)
            .and_then(|m| m.modified())
            .unwrap_or(std::time::SystemTime::UNIX_EPOCH)
    });
    sessions.reverse();

    for sock in &sessions {
        if let Ok(client) = DevChannelClient::connect(sock, true) {
            if client.handshake_done() {
                return Ok(Some(sock.clone()));
            }
        }
    }

    // All candidate sockets are stale (no handshake) — offline, not a guess.
    Ok(None)
}

/// Blocking client for the ADR-0038 dev channel.
///
/// All calls are synchronous and cheap (local IPC, small payloads); callers
/// running inside the tokio runtime should hold it behind a `Mutex` and keep
/// critical sections short.
pub struct DevChannelClient {
    stream: TransportStream,
    reader: BufReader<TransportStream>,
    next_id: u64,
    handshake_done: bool,
    app_version: String,
    protocol_version: u32,
}

impl DevChannelClient {
    /// Connects to a dev channel socket and performs the mandatory `hello`
    /// handshake (D1 version lock).
    ///
    /// If `allow_version_mismatch` is false and the app's version differs from
    /// [`MARTENSITE_VERSION`], returns [`McpError::VersionMismatch`].
    pub fn connect(socket_path: &Path, allow_version_mismatch: bool) -> Result<Self, McpError> {
        #[cfg(unix)]
        let stream = UnixStream::connect(socket_path).map_err(|e| {
            McpError::Ipc(format!(
                "cannot connect to `{}`: {e}",
                socket_path.display()
            ))
        })?;

        #[cfg(windows)]
        let stream = connect_windows_pipe(socket_path)?;

        let reader_stream = stream.try_clone().map_err(McpError::Io)?;
        let reader = BufReader::new(reader_stream);

        let mut client = Self {
            stream,
            reader,
            next_id: 1,
            handshake_done: false,
            app_version: String::new(),
            protocol_version: 0,
        };

        let hello_params = serde_json::json!({
            "client_version": MARTENSITE_VERSION,
            "protocol_version": DEV_CHANNEL_PROTOCOL_VERSION,
        });
        let res = client.call("hello", hello_params)?;

        let app_version = res
            .get("server_version")
            .and_then(|v| v.as_str())
            .unwrap_or("unknown")
            .to_string();
        let proto_ver = res
            .get("protocol_version")
            .and_then(serde_json::Value::as_u64)
            .unwrap_or(0) as u32;

        client.app_version = app_version.clone();
        client.protocol_version = proto_ver;
        client.handshake_done = true;

        if proto_ver != DEV_CHANNEL_PROTOCOL_VERSION {
            return Err(McpError::ProtocolMismatch {
                expected: DEV_CHANNEL_PROTOCOL_VERSION,
                actual: proto_ver,
            });
        }
        if !allow_version_mismatch && app_version != MARTENSITE_VERSION {
            return Err(McpError::VersionMismatch {
                expected: MARTENSITE_VERSION.to_string(),
                actual: app_version,
            });
        }

        Ok(client)
    }

    /// Whether the mandatory `hello` handshake completed.
    pub fn handshake_done(&self) -> bool {
        self.handshake_done
    }

    /// Application version reported by the handshake.
    pub fn app_version(&self) -> &str {
        &self.app_version
    }

    /// Wire protocol version negotiated during the handshake.
    pub fn protocol_version(&self) -> u32 {
        self.protocol_version
    }

    /// Dispatches an RPC request and waits for the response line.
    pub fn call(
        &mut self,
        method: &str,
        params: serde_json::Value,
    ) -> Result<serde_json::Value, McpError> {
        let id = self.next_id;
        self.next_id += 1;

        let req = DevRequest {
            id,
            method: method.to_string(),
            params,
        };
        let req_json = serde_json::to_string(&req)?;

        self.stream.write_all(req_json.as_bytes())?;
        self.stream.write_all(b"\n")?;
        self.stream.flush()?;

        let mut line = String::new();
        let n = self.reader.read_line(&mut line)?;
        if n == 0 {
            return Err(McpError::Ipc(
                "unexpected EOF from dev channel socket".to_string(),
            ));
        }

        let resp: DevResponse = serde_json::from_str(line.trim_end())
            .map_err(|e| McpError::Ipc(format!("invalid response: {e}; raw: `{line}`")))?;

        if let Some(err) = resp.error {
            if err.code == -32601 {
                return Err(McpError::Ipc(format!(
                    "dev app does not implement `{method}` (method not found); \
                     the running build may predate this MCP tool surface"
                )));
            }
            return Err(McpError::Ipc(format!(
                "dev app RPC error (code {}): {}",
                err.code, err.message
            )));
        }

        resp.result
            .ok_or_else(|| McpError::Ipc("response missing result payload".to_string()))
    }

    /// Requests a tree snapshot of the widget arena (`tree_snapshot`).
    pub fn tree_snapshot(&mut self) -> Result<TreeSnapshotData, McpError> {
        let val = self.call("tree_snapshot", serde_json::json!({}))?;
        serde_json::from_value(val)
            .map_err(|e| McpError::Ipc(format!("failed to deserialize TreeSnapshotData: {e}")))
    }

    /// Pulls the latest frame's lint dump (`lint_pull`) as raw JSON.
    pub fn lint_pull_raw(&mut self) -> Result<serde_json::Value, McpError> {
        self.call("lint_pull", serde_json::json!({}))
    }

    /// Applies autofix operations on the dev app (`lint_apply`) as raw JSON.
    pub fn lint_apply_raw(
        &mut self,
        force: bool,
        recursive: bool,
        max_depth: usize,
    ) -> Result<serde_json::Value, McpError> {
        self.call(
            "lint_apply",
            serde_json::json!({
                "force": force,
                "recursive": recursive,
                "max_recursiveness": max_depth,
            }),
        )
    }

    /// Arms select mode and waits for a user click (`inspector_select`).
    pub fn inspector_select(&mut self) -> Result<InspectSelectData, McpError> {
        let val = self.call(
            "inspector_select",
            serde_json::json!({ "arm": true, "wait": true }),
        )?;
        serde_json::from_value(val)
            .map_err(|e| McpError::Ipc(format!("failed to deserialize InspectSelectData: {e}")))
    }
}

#[cfg(windows)]
fn resolve_windows_pipe_path(socket_path: &Path) -> PathBuf {
    let s = socket_path.to_string_lossy();
    if s.starts_with(r"\\.\pipe\") {
        socket_path.to_path_buf()
    } else if socket_path.is_file() {
        if let Ok(content) = std::fs::read_to_string(socket_path) {
            let trimmed = content.trim();
            if trimmed.starts_with(r"\\.\pipe\") {
                PathBuf::from(trimmed)
            } else if !trimmed.is_empty() {
                PathBuf::from(format!(r"\\.\pipe\martensite-{trimmed}"))
            } else {
                fallback_pipe_name(socket_path)
            }
        } else {
            fallback_pipe_name(socket_path)
        }
    } else {
        fallback_pipe_name(socket_path)
    }
}

#[cfg(windows)]
fn fallback_pipe_name(path: &Path) -> PathBuf {
    let s = path.to_string_lossy();
    if s.starts_with(r"\\.\pipe\") {
        return path.to_path_buf();
    }
    let stem = path.file_stem().and_then(|s| s.to_str()).unwrap_or("dev");
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    std::hash::Hash::hash(&s, &mut hasher);
    let h = std::hash::Hasher::finish(&hasher);
    PathBuf::from(format!(r"\\.\pipe\martensite-{stem}-{h:016x}"))
}

#[cfg(windows)]
fn connect_windows_pipe(socket_path: &Path) -> Result<std::fs::File, McpError> {
    let target_pipe = resolve_windows_pipe_path(socket_path);
    let start = std::time::Instant::now();
    loop {
        match std::fs::OpenOptions::new()
            .read(true)
            .write(true)
            .open(&target_pipe)
        {
            Ok(f) => return Ok(f),
            Err(e) => {
                let code = e.raw_os_error();
                if (code == Some(231)
                    || code == Some(2)
                    || e.kind() == std::io::ErrorKind::WouldBlock)
                    && start.elapsed() < std::time::Duration::from_millis(1500)
                {
                    std::thread::sleep(std::time::Duration::from_millis(10));
                    continue;
                }
                return Err(McpError::Ipc(format!(
                    "cannot connect to `{}`: {e}",
                    target_pipe.display()
                )));
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dev_request_round_trip() {
        let req = DevRequest {
            id: 7,
            method: "hello".to_string(),
            params: serde_json::json!({"protocol_version": 1}),
        };
        let s = serde_json::to_string(&req).expect("serialize");
        assert!(s.contains("\"method\":\"hello\""));
        let back: DevRequest = serde_json::from_str(&s).expect("deserialize");
        assert_eq!(back.id, 7);
    }

    #[test]
    fn socket_discovery_no_panic() {
        // In a bare environment discovery must return Ok rather than panic.
        let sessions = discover_dev_sessions().expect("discover");
        for s in sessions {
            assert!(
                s.extension().and_then(|e| e.to_str()) == Some("sock")
                    || s.to_string_lossy().starts_with(r"\\.\pipe\")
            );
        }
    }
}
