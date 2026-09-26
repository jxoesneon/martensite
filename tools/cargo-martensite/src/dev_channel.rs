//! Dev Channel IPC client and protocol for `cargo martensite`.
//!
//! Implements ADR-0038: a narrow, local-only, read-mostly, version-handshook
//! Unix domain socket channel connecting the CLI to a running Martensite
//! development session.
//!
//! # Protocol
//!
//! - **Transport**: Unix domain socket at `$XDG_RUNTIME_DIR/martensite/<id>.sock`
//!   (or `/tmp/martensite/<id>.sock`).
//! - **Framing**: Newline-delimited JSON-RPC-lite requests and responses.
//! - **Handshake**: Mandatory `hello` call negotiating `MARTENSITE_VERSION` and
//!   `protocol_version`. Mismatch fails fast with exit code 3 (D1 audit constraint).
//! - **Methods**:
//!   - `hello`: Handshake carrying versions.
//!   - `lint_pull`: Retrieve current frame's [`LintScene`] and [`LintReport`].
//!   - `lint_apply`: Apply [`FixOp`]s to a copy of the scene and report converged state.
//!   - `tree_snapshot`: Retrieve the current [`WidgetArena`] hierarchy for headless inspection.
//!   - `inspector_select`: Arm select-mode, wait for user click, and resolve selected node.

use std::collections::BTreeMap;
use std::fmt;
use std::io::{BufRead, BufReader, Write};
use std::path::{Path, PathBuf};

#[cfg(unix)]
use std::os::unix::net::{UnixListener, UnixStream};

#[cfg(unix)]
type TransportStream = UnixStream;

#[cfg(windows)]
type TransportStream = std::fs::File;

use martensite_design_lint::{LintReport, LintScene};
use martensite_devtools::lint_bridge::{LintDump, SerializedLintReport, SerializedLintScene};
use serde::{Deserialize, Serialize};

/// Current dev channel wire protocol version.
pub const PROTOCOL_VERSION: u32 = 1;

/// Embedded Martensite crate version for version-parity verification (D1).
pub const MARTENSITE_VERSION: &str = env!("CARGO_PKG_VERSION");

/// JSON-RPC-lite request sent from CLI to the running dev app.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DevRequest {
    /// Request identifier matching response.
    pub id: u64,
    /// RPC method name (`hello`, `lint_pull`, `lint_apply`, `tree_snapshot`, `inspector_select`).
    pub method: String,
    /// Request parameters payload.
    #[serde(default)]
    pub params: serde_json::Value,
}

/// JSON-RPC-lite response returned from dev app to CLI.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DevResponse {
    /// Request identifier matching request.
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

/// Errors occurring during dev channel IPC communication.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DevChannelError {
    /// No active Martensite dev session found.
    NoSessionFound(String),
    /// Failed to connect to the dev socket.
    ConnectionFailed(String),
    /// Version parity handshake failed (app version != CLI version).
    VersionMismatch {
        /// Version reported by the running app.
        app_version: String,
        /// Version compiled into this CLI.
        cli_version: String,
    },
    /// Wire protocol framing or message error.
    ProtocolError(String),
    /// The dev app returned an RPC error.
    RpcError {
        /// Error code.
        code: i32,
        /// Error message.
        message: String,
    },
    /// Transport I/O failure.
    Io(String),
    /// JSON serialization or deserialization failure.
    Json(String),
}

impl fmt::Display for DevChannelError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            DevChannelError::NoSessionFound(reason) => {
                write!(
                    f,
                    "no active Martensite dev session found: {reason}\n\
                     Run `cargo martensite dev` or specify `--scene <path>` for offline linting."
                )
            }
            DevChannelError::ConnectionFailed(msg) => {
                write!(f, "failed to connect to dev channel socket: {msg}")
            }
            DevChannelError::VersionMismatch {
                app_version,
                cli_version,
            } => {
                write!(
                    f,
                    "version mismatch: app version `{app_version}` does not match CLI version `{cli_version}` \
                     (pass `--allow-version-mismatch` to override)"
                )
            }
            DevChannelError::ProtocolError(msg) => write!(f, "protocol error: {msg}"),
            DevChannelError::RpcError { code, message } => {
                write!(f, "dev app RPC error (code {code}): {message}")
            }
            DevChannelError::Io(msg) => write!(f, "I/O error: {msg}"),
            DevChannelError::Json(msg) => write!(f, "JSON error: {msg}"),
        }
    }
}

impl std::error::Error for DevChannelError {}

/// Serialized tree node representation for headless inspection over dev channel.
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
    /// Bounding rectangle in screen coordinates: `[x, y, width, height]`.
    pub screen_bounds: [f32; 4],
    /// Bounding rectangle relative to parent: `[x, y, width, height]`.
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
    /// Number of active reactive signals bound to widget.
    #[serde(default)]
    pub active_signal_count: usize,
    /// Materialized child nodes.
    #[serde(default)]
    pub children: Vec<WireTreeNode>,
}

impl WireTreeNode {
    /// User-facing display label for the node.
    pub fn display_label(&self) -> &str {
        if let Some(name) = &self.debug_name {
            name.as_str()
        } else if !self.kind.is_empty() {
            &self.kind
        } else {
            "Widget"
        }
    }

    /// Format node and its subtree as an ASCII tree diagram.
    pub fn to_tree_string(&self) -> String {
        let mut out = String::new();
        self.render_tree_ascii(&mut out, "", true);
        out
    }

    fn render_tree_ascii(&self, out: &mut String, prefix: &str, is_last: bool) {
        let marker = if prefix.is_empty() {
            ""
        } else if is_last {
            "└── "
        } else {
            "├── "
        };

        let badges = if self.badges.is_empty() {
            String::new()
        } else {
            format!(" [{}]", self.badges.join(", "))
        };

        let signals = if self.active_signal_count > 0 {
            format!(" ({} signals)", self.active_signal_count)
        } else {
            String::new()
        };

        out.push_str(&format!(
            "{prefix}{marker}{name} [{:.0}, {:.0} {:.0}×{:.0}] (id: {}){badges}{signals}\n",
            self.screen_bounds[0],
            self.screen_bounds[1],
            self.screen_bounds[2],
            self.screen_bounds[3],
            self.id,
            name = self.display_label(),
        ));

        let new_prefix = if prefix.is_empty() {
            ""
        } else if is_last {
            "    "
        } else {
            "│   "
        };
        let child_prefix = format!("{prefix}{new_prefix}");

        for (i, child) in self.children.iter().enumerate() {
            let last_child = i + 1 == self.children.len();
            child.render_tree_ascii(out, &child_prefix, last_child);
        }
    }
}

/// A step in a widget's layout constraint chain.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct LayoutStep {
    /// Step description (e.g. widget name or constraint origin).
    pub name: String,
    /// Textual constraint summary (e.g. `min: [0, 0], max: [800, 600]`).
    pub constraints: String,
    /// Resulting dimensions `[width, height]`.
    pub result_size: [f32; 2],
    /// Any constraint violations or warnings at this step.
    #[serde(default)]
    pub violations: Vec<String>,
}

/// Snapshot data returned by `tree_snapshot`.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct TreeSnapshotData {
    /// Root node of the widget tree.
    pub root: WireTreeNode,
    /// Currently selected widget ID, if any.
    #[serde(default)]
    pub selected_id: Option<u64>,
    /// Constraint layout chain for the selected widget.
    #[serde(default)]
    pub layout_chain: Vec<LayoutStep>,
    /// Active signal summaries.
    #[serde(default)]
    pub signals: Vec<String>,
}

/// Inspection data returned by `inspector_select` (`--pick`).
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct InspectSelectData {
    /// The clicked/selected tree node.
    pub selected_node: WireTreeNode,
    /// Constraint layout chain for the selected node.
    #[serde(default)]
    pub layout_chain: Vec<LayoutStep>,
    /// Extracted properties and markers for the selected node.
    #[serde(default)]
    pub properties: BTreeMap<String, String>,
}

/// Default runtime directory hosting Martensite dev-channel sockets.
pub fn default_socket_dir() -> PathBuf {
    if let Ok(runtime_dir) = std::env::var("XDG_RUNTIME_DIR") {
        if !runtime_dir.is_empty() {
            return PathBuf::from(runtime_dir).join("martensite");
        }
    }
    std::env::temp_dir().join("martensite")
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
fn connect_windows_pipe(socket_path: &Path) -> Result<std::fs::File, DevChannelError> {
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
                return Err(DevChannelError::ConnectionFailed(format!(
                    "cannot connect to `{}`: {e}",
                    target_pipe.display()
                )));
            }
        }
    }
}

/// Discovers all available dev session sockets or named pipes.
pub fn discover_dev_sessions() -> Result<Vec<PathBuf>, DevChannelError> {
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

/// Finds an active dev socket or named pipe, probing candidates for responsiveness.
pub fn find_dev_socket(explicit: Option<&Path>) -> Result<PathBuf, DevChannelError> {
    discover_socket(explicit)
}

/// Discovers an active dev-channel socket path.
///
/// Priority:
/// 1. Explicit path from `--socket <path>`.
/// 2. `MARTENSITE_DEV_SOCKET` environment variable.
/// 3. Search `discover_dev_sessions()`, probing live sessions.
pub fn discover_socket(explicit: Option<&Path>) -> Result<PathBuf, DevChannelError> {
    if let Some(path) = explicit {
        #[cfg(windows)]
        {
            if path.to_string_lossy().starts_with(r"\\.\pipe\") || path.exists() {
                return Ok(path.to_path_buf());
            }
        }
        #[cfg(not(windows))]
        {
            if path.exists() {
                return Ok(path.to_path_buf());
            }
        }
        return Err(DevChannelError::NoSessionFound(format!(
            "specified socket `{}` does not exist",
            path.display()
        )));
    }

    if let Ok(env_path) = std::env::var("MARTENSITE_DEV_SOCKET") {
        let p = PathBuf::from(env_path);
        #[cfg(windows)]
        {
            if p.to_string_lossy().starts_with(r"\\.\pipe\") || p.exists() {
                return Ok(p);
            }
        }
        #[cfg(not(windows))]
        {
            if p.exists() {
                return Ok(p);
            }
        }
    }

    let mut sessions = discover_dev_sessions()?;

    if sessions.is_empty() {
        return Err(DevChannelError::NoSessionFound(
            "no dev sessions found".to_string(),
        ));
    }

    // Sort by modification time (most recent first).
    sessions.sort_by_key(|p| {
        std::fs::metadata(p)
            .and_then(|m| m.modified())
            .unwrap_or(std::time::SystemTime::UNIX_EPOCH)
    });
    sessions.reverse();

    for sock_path in &sessions {
        // Quick probe to verify the socket is alive and responsive.
        if let Ok(client) = DevClient::connect(sock_path, true) {
            if client.handshake_done {
                return Ok(sock_path.clone());
            }
        }
    }

    // If probing failed to find a responsive socket, return the most recent socket
    // so connection error surfaces the detailed failure.
    if let Some(first) = sessions.first() {
        return Ok(first.clone());
    }

    Err(DevChannelError::NoSessionFound(
        "no responsive dev session socket found".to_string(),
    ))
}

/// Client for communicating with a running Martensite dev session over the dev channel.
pub struct DevClient {
    stream: TransportStream,
    reader: BufReader<TransportStream>,
    next_id: u64,
    handshake_done: bool,
    app_version: String,
    protocol_version: u32,
}

impl DevClient {
    /// Connects to a dev channel Unix domain socket and performs the mandatory `hello` handshake.
    ///
    /// If `allow_version_mismatch` is false and the app's version differs from
    /// [`MARTENSITE_VERSION`], returns [`DevChannelError::VersionMismatch`].
    pub fn connect(
        socket_path: &Path,
        allow_version_mismatch: bool,
    ) -> Result<Self, DevChannelError> {
        #[cfg(unix)]
        let stream = UnixStream::connect(socket_path).map_err(|e| {
            DevChannelError::ConnectionFailed(format!(
                "cannot connect to `{}`: {e}",
                socket_path.display()
            ))
        })?;

        #[cfg(windows)]
        let stream = connect_windows_pipe(socket_path)?;

        let reader_stream = stream
            .try_clone()
            .map_err(|e| DevChannelError::Io(format!("failed to clone socket: {e}")))?;
        let reader = BufReader::new(reader_stream);

        let mut client = Self {
            stream,
            reader,
            next_id: 1,
            handshake_done: false,
            app_version: String::new(),
            protocol_version: 0,
        };

        // Perform mandatory hello handshake.
        let hello_params = serde_json::json!({
            "client_version": MARTENSITE_VERSION,
            "version": MARTENSITE_VERSION,
            "protocol_version": PROTOCOL_VERSION,
        });

        let res = client.call("hello", hello_params)?;

        let app_version = res
            .get("server_version")
            .or_else(|| res.get("version"))
            .and_then(|v| v.as_str())
            .unwrap_or("unknown")
            .to_string();
        let proto_ver = res
            .get("protocol_version")
            .and_then(|v| v.as_u64())
            .unwrap_or(0) as u32;

        client.app_version = app_version.clone();
        client.protocol_version = proto_ver;
        client.handshake_done = true;

        if !allow_version_mismatch && app_version != MARTENSITE_VERSION {
            return Err(DevChannelError::VersionMismatch {
                app_version,
                cli_version: MARTENSITE_VERSION.to_string(),
            });
        }

        Ok(client)
    }

    /// Dispatches an RPC request and waits for the response line.
    pub fn call(
        &mut self,
        method: &str,
        params: serde_json::Value,
    ) -> Result<serde_json::Value, DevChannelError> {
        let id = self.next_id;
        self.next_id += 1;

        let req = DevRequest {
            id,
            method: method.to_string(),
            params,
        };

        let req_json =
            serde_json::to_string(&req).map_err(|e| DevChannelError::Json(e.to_string()))?;

        self.stream
            .write_all(req_json.as_bytes())
            .map_err(|e| DevChannelError::Io(e.to_string()))?;
        self.stream
            .write_all(b"\n")
            .map_err(|e| DevChannelError::Io(e.to_string()))?;
        self.stream
            .flush()
            .map_err(|e| DevChannelError::Io(e.to_string()))?;

        let mut line = String::new();
        let n = self
            .reader
            .read_line(&mut line)
            .map_err(|e| DevChannelError::Io(e.to_string()))?;

        if n == 0 {
            return Err(DevChannelError::ProtocolError(
                "unexpected EOF from dev channel socket".to_string(),
            ));
        }

        let resp: DevResponse = serde_json::from_str(line.trim_end())
            .map_err(|e| DevChannelError::Json(format!("invalid response: {e}; raw: `{line}`")))?;

        if let Some(err) = resp.error {
            return Err(DevChannelError::RpcError {
                code: err.code,
                message: err.message,
            });
        }

        resp.result.ok_or_else(|| {
            DevChannelError::ProtocolError("response missing result payload".to_string())
        })
    }

    /// Pulls the latest frame's lint scene and report from the dev app (`LintPull`).
    pub fn lint_pull(&mut self) -> Result<(LintScene, LintReport), DevChannelError> {
        let val = self.call("lint_pull", serde_json::json!({}))?;
        self.decode_lint_result(val)
    }

    /// Applies autofix operations to a copy of the scene on the dev app (`LintApply`).
    pub fn lint_apply(
        &mut self,
        force: bool,
        recursive: bool,
        max_depth: usize,
    ) -> Result<(LintScene, LintReport), DevChannelError> {
        let val = self.call(
            "lint_apply",
            serde_json::json!({
                "force": force,
                "recursive": recursive,
                "max_recursiveness": max_depth,
            }),
        )?;
        self.decode_lint_result(val)
    }

    /// Requests a tree snapshot of the widget arena from the running app.
    pub fn tree_snapshot(&mut self) -> Result<TreeSnapshotData, DevChannelError> {
        let val = self.call("tree_snapshot", serde_json::json!({}))?;
        serde_json::from_value(val).map_err(|e| {
            DevChannelError::Json(format!("failed to deserialize TreeSnapshotData: {e}"))
        })
    }

    /// Arms select mode and waits for user click in app (`--pick`).
    pub fn inspector_select(&mut self) -> Result<InspectSelectData, DevChannelError> {
        let val = self.call(
            "inspector_select",
            serde_json::json!({
                "arm": true,
                "wait": true,
            }),
        )?;
        serde_json::from_value(val).map_err(|e| {
            DevChannelError::Json(format!("failed to deserialize InspectSelectData: {e}"))
        })
    }

    /// App version returned during handshake.
    pub fn app_version(&self) -> &str {
        &self.app_version
    }

    /// Protocol version negotiated during handshake.
    pub fn protocol_version(&self) -> u32 {
        self.protocol_version
    }

    fn decode_lint_result(
        &self,
        val: serde_json::Value,
    ) -> Result<(LintScene, LintReport), DevChannelError> {
        if let Ok(dump) = serde_json::from_value::<LintDump>(val.clone()) {
            return Ok((dump.to_scene(), dump.to_report()));
        }

        if let (Some(scene_val), Some(report_val)) = (val.get("scene"), val.get("report")) {
            let scene_ser: SerializedLintScene = serde_json::from_value(scene_val.clone())
                .map_err(|e| DevChannelError::Json(e.to_string()))?;
            let report_ser: SerializedLintReport = serde_json::from_value(report_val.clone())
                .map_err(|e| DevChannelError::Json(e.to_string()))?;
            return Ok((scene_ser.to_scene(), report_ser.to_report()));
        }

        Err(DevChannelError::ProtocolError(format!(
            "unexpected lint result format: {val}"
        )))
    }
}

/// A lightweight mock dev-channel server for testing CLI attach mode and inspector.
#[cfg(unix)]
pub struct DevServer {
    socket_path: PathBuf,
    shutdown_flag: std::sync::Arc<std::sync::atomic::AtomicBool>,
    handle: Option<std::thread::JoinHandle<()>>,
}

#[cfg(unix)]
impl DevServer {
    /// Binds a Unix domain socket listener and serves requests on a background thread.
    pub fn bind(
        socket_path: &Path,
        version: Option<String>,
        dump: Option<LintDump>,
        tree: Option<TreeSnapshotData>,
        pick: Option<InspectSelectData>,
    ) -> Result<Self, DevChannelError> {
        let _ = std::fs::remove_file(socket_path);
        if let Some(parent) = socket_path.parent() {
            let _ = std::fs::create_dir_all(parent);
        }

        let listener = UnixListener::bind(socket_path).map_err(|e| {
            DevChannelError::Io(format!("bind failed for `{}`: {e}", socket_path.display()))
        })?;

        let shutdown_flag = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
        let flag_clone = shutdown_flag.clone();
        let sock_clone = socket_path.to_path_buf();

        let handle = std::thread::spawn(move || {
            let _ = listener.set_nonblocking(true);
            while !flag_clone.load(std::sync::atomic::Ordering::Relaxed) {
                match listener.accept() {
                    Ok((mut stream, _)) => {
                        let mut reader = BufReader::new(stream.try_clone().unwrap());
                        let mut line = String::new();
                        while let Ok(n) = reader.read_line(&mut line) {
                            if n == 0 {
                                break;
                            }
                            let req: Result<DevRequest, _> = serde_json::from_str(line.trim_end());
                            if let Ok(req) = req {
                                let resp = match req.method.as_str() {
                                    "hello" => {
                                        let ver = version
                                            .clone()
                                            .unwrap_or_else(|| MARTENSITE_VERSION.to_string());
                                        serde_json::json!({
                                            "version": ver,
                                            "protocol_version": PROTOCOL_VERSION,
                                        })
                                    }
                                    "lint_pull" | "lint_apply" => {
                                        if let Some(d) = &dump {
                                            serde_json::to_value(d).unwrap()
                                        } else {
                                            let scene = LintScene::default();
                                            let report = LintReport::default();
                                            let d = LintDump::new(&scene, &report);
                                            serde_json::to_value(&d).unwrap()
                                        }
                                    }
                                    "tree_snapshot" => {
                                        if let Some(t) = &tree {
                                            serde_json::to_value(t).unwrap()
                                        } else {
                                            serde_json::json!({
                                                "root": {
                                                    "id": 1,
                                                    "debug_name": "RootContainer",
                                                    "kind": "Container",
                                                    "screen_bounds": [0.0, 0.0, 800.0, 600.0],
                                                    "children": []
                                                }
                                            })
                                        }
                                    }
                                    "inspector_select" => {
                                        if let Some(p) = &pick {
                                            serde_json::to_value(p).unwrap()
                                        } else {
                                            serde_json::json!({
                                                "selected_node": {
                                                    "id": 42,
                                                    "debug_name": "ClickedButton",
                                                    "kind": "Button",
                                                    "screen_bounds": [10.0, 20.0, 100.0, 30.0],
                                                    "children": []
                                                },
                                                "layout_chain": [],
                                                "properties": {}
                                            })
                                        }
                                    }
                                    other => {
                                        let err_resp = DevResponse {
                                            id: req.id,
                                            result: None,
                                            error: Some(DevError {
                                                code: -32601,
                                                message: format!("method not found: {other}"),
                                                data: None,
                                            }),
                                        };
                                        let _ = stream.write_all(
                                            serde_json::to_string(&err_resp).unwrap().as_bytes(),
                                        );
                                        let _ = stream.write_all(b"\n");
                                        line.clear();
                                        continue;
                                    }
                                };

                                let success_resp = DevResponse {
                                    id: req.id,
                                    result: Some(resp),
                                    error: None,
                                };
                                let _ = stream.write_all(
                                    serde_json::to_string(&success_resp).unwrap().as_bytes(),
                                );
                                let _ = stream.write_all(b"\n");
                                let _ = stream.flush();
                            }
                            line.clear();
                        }
                    }
                    Err(ref e) if e.kind() == std::io::ErrorKind::WouldBlock => {
                        std::thread::sleep(std::time::Duration::from_millis(10));
                    }
                    Err(_) => break,
                }
            }
            let _ = std::fs::remove_file(&sock_clone);
        });

        Ok(Self {
            socket_path: socket_path.to_path_buf(),
            shutdown_flag,
            handle: Some(handle),
        })
    }

    /// Path to the bound socket file.
    pub fn socket_path(&self) -> &Path {
        &self.socket_path
    }

    /// Shut down the dev server and remove the socket file.
    pub fn shutdown(mut self) {
        self.shutdown_flag
            .store(true, std::sync::atomic::Ordering::Relaxed);
        if let Some(handle) = self.handle.take() {
            let _ = handle.join();
        }
        let _ = std::fs::remove_file(&self.socket_path);
    }
}

#[cfg(unix)]
impl Drop for DevServer {
    fn drop(&mut self) {
        self.shutdown_flag
            .store(true, std::sync::atomic::Ordering::Relaxed);
        let _ = std::fs::remove_file(&self.socket_path);
    }
}
