//! Dev Channel IPC Transport (ADR-0038).
//!
//! Provides a narrow, read-mostly, version-handshook local IPC transport
//! between running Martensite applications and developer tools (such as
//! `cargo-martensite` CLI or headless test runners).
//!
//! # Transport and Framing
//!
//! - **Transport:** Local Unix domain socket on Unix platforms (AF_UNIX socket on Windows).
//!   The socket is bound with permissions `0600` (user-only) inside `$XDG_RUNTIME_DIR/martensite/`
//!   or `/tmp/martensite-<user>/`.
//! - **Framing:** Newline-delimited JSON-RPC-lite (`\n` separated JSON objects).
//! - **Version Handshake (Constraint D1):** Mandatory `Hello` handshake carrying
//!   [`MARTENSITE_VERSION`] and [`DEV_CHANNEL_PROTOCOL_VERSION`]. Version mismatch produces
//!   an explicit `version_mismatch` error with both client and server versions.
//! - **Lifecycle:** The server creates the socket on startup, creates the parent directory
//!   with restricted permissions, and unlinks the socket file on shutdown.
//!
//! # Example
//!
//! ```no_run
//! use martensite_host::dev_channel::{
//!     DevChannelServer, DevChannelClient, TreeSnapshotParams,
//!     MARTENSITE_VERSION, DEV_CHANNEL_PROTOCOL_VERSION,
//! };
//!
//! // Start the dev channel server for a session.
//! let server = DevChannelServer::start("session_123").expect("failed to start server");
//!
//! // Connect a dev channel client.
//! let mut client = DevChannelClient::connect(server.socket_path())
//!     .expect("failed to connect");
//!
//! // Perform the mandatory hello handshake.
//! let hello = client.hello(MARTENSITE_VERSION, DEV_CHANNEL_PROTOCOL_VERSION)
//!     .expect("io error")
//!     .expect("handshake failed");
//! assert_eq!(hello.server_version, MARTENSITE_VERSION);
//!
//! // Query the tree snapshot.
//! let snapshot = client.tree_snapshot(TreeSnapshotParams::default())
//!     .expect("io error")
//!     .expect("rpc error");
//! ```

use std::fmt;
use std::io::{self, BufRead, BufReader, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::Arc;
use std::thread::JoinHandle;
use std::time::Duration;

#[cfg(unix)]
use std::os::unix::net::{UnixListener, UnixStream};

#[cfg(windows)]
use std::os::windows::net::{UnixListener, UnixStream};

use serde::{Deserialize, Serialize};

/// The version of the Martensite host runtime, derived from package metadata.
///
/// # Examples
///
/// ```
/// use martensite_host::dev_channel::MARTENSITE_VERSION;
///
/// assert!(!MARTENSITE_VERSION.is_empty());
/// ```
pub const MARTENSITE_VERSION: &str = env!("CARGO_PKG_VERSION");

/// Current protocol version of the Dev Channel IPC transport.
///
/// # Examples
///
/// ```
/// use martensite_host::dev_channel::DEV_CHANNEL_PROTOCOL_VERSION;
///
/// assert_eq!(DEV_CHANNEL_PROTOCOL_VERSION, 1);
/// ```
pub const DEV_CHANNEL_PROTOCOL_VERSION: u32 = 1;

/// JSON-RPC 2.0 parse error code.
pub const ERR_PARSE: i32 = -32700;
/// JSON-RPC 2.0 invalid request error code.
pub const ERR_INVALID_REQUEST: i32 = -32600;
/// JSON-RPC 2.0 method not found error code.
pub const ERR_METHOD_NOT_FOUND: i32 = -32601;
/// JSON-RPC 2.0 invalid params error code.
pub const ERR_INVALID_PARAMS: i32 = -32602;
/// JSON-RPC 2.0 internal server error code.
pub const ERR_INTERNAL: i32 = -32603;

/// Custom error code indicating that the mandatory `Hello` handshake has not been performed.
pub const ERR_HANDSHAKE_REQUIRED: i32 = -32000;
/// Custom error code indicating that the client and server versions do not match (Constraint D1).
pub const ERR_VERSION_MISMATCH: i32 = -32001;

fn default_jsonrpc_version() -> String {
    "2.0".to_string()
}

/// A JSON-RPC 2.0 request envelope.
///
/// # Examples
///
/// ```
/// use martensite_host::dev_channel::JsonRpcRequest;
///
/// let req = JsonRpcRequest::new(Some(1), "Hello", serde_json::json!({
///     "client_version": "0.19.0",
///     "protocol_version": 1
/// }));
/// assert_eq!(req.method, "Hello");
/// assert_eq!(req.id, Some(1));
/// ```
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct JsonRpcRequest {
    /// JSON-RPC protocol specification version (default `"2.0"`).
    #[serde(default = "default_jsonrpc_version")]
    pub jsonrpc: String,
    /// Request identifier, or `None` for notifications.
    #[serde(default)]
    pub id: Option<u64>,
    /// Method name to invoke.
    pub method: String,
    /// Parameters payload for the method.
    #[serde(default)]
    pub params: serde_json::Value,
}

impl JsonRpcRequest {
    /// Creates a new JSON-RPC request.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite_host::dev_channel::JsonRpcRequest;
    ///
    /// let req = JsonRpcRequest::new(Some(42), "TreeSnapshot", serde_json::Value::Null);
    /// assert_eq!(req.id, Some(42));
    /// assert_eq!(req.method, "TreeSnapshot");
    /// ```
    pub fn new(id: Option<u64>, method: impl Into<String>, params: serde_json::Value) -> Self {
        Self {
            jsonrpc: default_jsonrpc_version(),
            id,
            method: method.into(),
            params,
        }
    }
}

/// A JSON-RPC 2.0 error object.
///
/// # Examples
///
/// ```
/// use martensite_host::dev_channel::{JsonRpcError, ERR_METHOD_NOT_FOUND};
///
/// let err = JsonRpcError::new(ERR_METHOD_NOT_FOUND, "method not found", None);
/// assert_eq!(err.code, ERR_METHOD_NOT_FOUND);
/// assert_eq!(err.message, "method not found");
/// ```
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct JsonRpcError {
    /// Error code.
    pub code: i32,
    /// Human-readable explanation.
    pub message: String,
    /// Optional structured data describing the error.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub data: Option<serde_json::Value>,
}

impl JsonRpcError {
    /// Creates a new JSON-RPC error.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite_host::dev_channel::JsonRpcError;
    ///
    /// let err = JsonRpcError::new(-32600, "invalid request", None);
    /// assert_eq!(err.code, -32600);
    /// ```
    pub fn new(code: i32, message: impl Into<String>, data: Option<serde_json::Value>) -> Self {
        Self {
            code,
            message: message.into(),
            data,
        }
    }
}

impl fmt::Display for JsonRpcError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "JSON-RPC error ({}): {}", self.code, self.message)
    }
}

impl std::error::Error for JsonRpcError {}

/// A JSON-RPC 2.0 response envelope.
///
/// # Examples
///
/// ```
/// use martensite_host::dev_channel::JsonRpcResponse;
///
/// let resp = JsonRpcResponse::success(Some(1), serde_json::json!({ "status": "ok" }));
/// assert!(resp.error.is_none());
/// assert!(resp.result.is_some());
/// ```
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct JsonRpcResponse {
    /// JSON-RPC protocol specification version (default `"2.0"`).
    #[serde(default = "default_jsonrpc_version")]
    pub jsonrpc: String,
    /// Correlated request identifier.
    #[serde(default)]
    pub id: Option<u64>,
    /// Result payload if successful.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub result: Option<serde_json::Value>,
    /// Error details if the request failed.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub error: Option<JsonRpcError>,
}

impl JsonRpcResponse {
    /// Creates a success response.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite_host::dev_channel::JsonRpcResponse;
    ///
    /// let resp = JsonRpcResponse::success(Some(1), serde_json::json!({ "answer": 42 }));
    /// assert_eq!(resp.id, Some(1));
    /// assert_eq!(resp.result, Some(serde_json::json!({ "answer": 42 })));
    /// ```
    pub fn success(id: Option<u64>, result: serde_json::Value) -> Self {
        Self {
            jsonrpc: default_jsonrpc_version(),
            id,
            result: Some(result),
            error: None,
        }
    }

    /// Creates an error response.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite_host::dev_channel::{JsonRpcResponse, ERR_INVALID_REQUEST};
    ///
    /// let resp = JsonRpcResponse::error(Some(1), ERR_INVALID_REQUEST, "invalid", None);
    /// assert!(resp.error.is_some());
    /// assert_eq!(resp.error.unwrap().code, ERR_INVALID_REQUEST);
    /// ```
    pub fn error(
        id: Option<u64>,
        code: i32,
        message: impl Into<String>,
        data: Option<serde_json::Value>,
    ) -> Self {
        Self {
            jsonrpc: default_jsonrpc_version(),
            id,
            result: None,
            error: Some(JsonRpcError::new(code, message, data)),
        }
    }
}

/// Parameters for the mandatory `Hello` handshake request.
///
/// # Examples
///
/// ```
/// use martensite_host::dev_channel::HelloParams;
///
/// let params = HelloParams {
///     client_version: "0.19.0".to_string(),
///     protocol_version: 1,
/// };
/// assert_eq!(params.protocol_version, 1);
/// ```
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct HelloParams {
    /// The client application's Martensite framework version.
    pub client_version: String,
    /// Protocol version supported by the client.
    pub protocol_version: u32,
}

/// Result returned from a successful `Hello` handshake.
///
/// # Examples
///
/// ```
/// use martensite_host::dev_channel::HelloResult;
///
/// let result = HelloResult {
///     server_version: "0.19.0".to_string(),
///     protocol_version: 1,
///     build_id: Some("build_123".to_string()),
/// };
/// assert_eq!(result.server_version, "0.19.0");
/// ```
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct HelloResult {
    /// Martensite framework version of the server.
    pub server_version: String,
    /// Dev Channel protocol version negotiated with the server.
    pub protocol_version: u32,
    /// Optional build or session identifier of the server process.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub build_id: Option<String>,
}

/// Parameters for the `TreeSnapshot` request.
///
/// # Examples
///
/// ```
/// use martensite_host::dev_channel::TreeSnapshotParams;
///
/// let params = TreeSnapshotParams {
///     max_depth: Some(5),
///     root_id: None,
/// };
/// assert_eq!(params.max_depth, Some(5));
/// ```
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Default)]
pub struct TreeSnapshotParams {
    /// Optional maximum depth for widget tree traversal.
    #[serde(default)]
    pub max_depth: Option<usize>,
    /// Optional root widget ID to start the snapshot from (defaults to scene root).
    #[serde(default)]
    pub root_id: Option<u64>,
}

/// Parameters for the `LintPull` (or `LintScene`) request.
///
/// # Examples
///
/// ```
/// use martensite_host::dev_channel::LintPullParams;
///
/// let params = LintPullParams {
///     window_id: Some(1),
/// };
/// assert_eq!(params.window_id, Some(1));
/// ```
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Default)]
pub struct LintPullParams {
    /// Optional window identifier to query lint findings for.
    #[serde(default)]
    pub window_id: Option<u64>,
}

/// Parameters for the `EventLedger` request.
///
/// # Examples
///
/// ```
/// use martensite_host::dev_channel::EventLedgerParams;
///
/// let params = EventLedgerParams {
///     tail_count: Some(50),
/// };
/// assert_eq!(params.tail_count, Some(50));
/// ```
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Default)]
pub struct EventLedgerParams {
    /// Number of most recent event records to return from the tail of the ledger.
    #[serde(default)]
    pub tail_count: Option<usize>,
}

/// Parameters for the `InspectorSelect` request.
///
/// # Examples
///
/// ```
/// use martensite_host::dev_channel::InspectorSelectParams;
///
/// let params = InspectorSelectParams { arm: true };
/// assert!(params.arm);
/// ```
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Default)]
pub struct InspectorSelectParams {
    /// Whether to arm (`true`) or disarm (`false`) Chrome DevTools-style pixel inspection mode.
    pub arm: bool,
}

/// Parameters for the `LintApply` request.
///
/// # Examples
///
/// ```
/// use martensite_host::dev_channel::LintApplyParams;
///
/// let params = LintApplyParams {
///     ops: vec![serde_json::json!({ "op": "set_gap", "amount": 8.0 })],
///     force: false,
/// };
/// assert_eq!(params.ops.len(), 1);
/// ```
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Default)]
pub struct LintApplyParams {
    /// Serialized fix operations to evaluate against a copy of the scene.
    #[serde(default)]
    pub ops: Vec<serde_json::Value>,
    /// Whether to allow risky operations during autofix evaluation.
    #[serde(default)]
    pub force: bool,
}

/// Trait implemented by application dev tools handlers to service Dev Channel requests.
///
/// All methods provide default no-op responses so that handlers can selectively implement
/// only the subsystems they support.
pub trait DevChannelHandler: Send + Sync {
    /// Handles a `TreeSnapshot` request.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite_host::dev_channel::{DevChannelHandler, DefaultDevChannelHandler, TreeSnapshotParams};
    ///
    /// let handler = DefaultDevChannelHandler;
    /// let res = handler.handle_tree_snapshot(TreeSnapshotParams::default()).unwrap();
    /// assert!(res.is_object());
    /// ```
    fn handle_tree_snapshot(
        &self,
        params: TreeSnapshotParams,
    ) -> Result<serde_json::Value, String> {
        let _ = params;
        Ok(serde_json::json!({
            "status": "ok",
            "root_id": params.root_id,
            "max_depth": params.max_depth,
            "nodes": []
        }))
    }

    /// Handles a `LintPull` (or `LintScene`) request.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite_host::dev_channel::{DevChannelHandler, DefaultDevChannelHandler, LintPullParams};
    ///
    /// let handler = DefaultDevChannelHandler;
    /// let res = handler.handle_lint_pull(LintPullParams::default()).unwrap();
    /// assert!(res.is_object());
    /// ```
    fn handle_lint_pull(&self, params: LintPullParams) -> Result<serde_json::Value, String> {
        let _ = params;
        Ok(serde_json::json!({
            "window_id": params.window_id,
            "report": null,
            "scene": null
        }))
    }

    /// Handles an `EventLedger` request.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite_host::dev_channel::{DevChannelHandler, DefaultDevChannelHandler, EventLedgerParams};
    ///
    /// let handler = DefaultDevChannelHandler;
    /// let res = handler.handle_event_ledger(EventLedgerParams::default()).unwrap();
    /// assert!(res.is_object());
    /// ```
    fn handle_event_ledger(&self, params: EventLedgerParams) -> Result<serde_json::Value, String> {
        let _ = params;
        Ok(serde_json::json!({
            "tail_count": params.tail_count.unwrap_or(0),
            "events": []
        }))
    }

    /// Handles an `InspectorSelect` request.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite_host::dev_channel::{DevChannelHandler, DefaultDevChannelHandler, InspectorSelectParams};
    ///
    /// let handler = DefaultDevChannelHandler;
    /// let res = handler.handle_inspector_select(InspectorSelectParams { arm: true }).unwrap();
    /// assert_eq!(res["armed"], true);
    /// ```
    fn handle_inspector_select(
        &self,
        params: InspectorSelectParams,
    ) -> Result<serde_json::Value, String> {
        Ok(serde_json::json!({
            "armed": params.arm,
            "selected_node_id": None::<u64>
        }))
    }

    /// Handles a `LintApply` request.
    ///
    /// Note: Per ADR-0038, `LintApply` fixes must be evaluated against a *copy* of the
    /// scene model and report the converged result; fixes never mutate the live widget tree.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite_host::dev_channel::{DevChannelHandler, DefaultDevChannelHandler, LintApplyParams};
    ///
    /// let handler = DefaultDevChannelHandler;
    /// let res = handler.handle_lint_apply(LintApplyParams::default()).unwrap();
    /// assert_eq!(res["converged"], true);
    /// ```
    fn handle_lint_apply(&self, params: LintApplyParams) -> Result<serde_json::Value, String> {
        Ok(serde_json::json!({
            "converged": true,
            "applied_ops_count": params.ops.len(),
            "report": null
        }))
    }

    /// Handles an unrecognized or custom request method.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite_host::dev_channel::{DevChannelHandler, DefaultDevChannelHandler};
    ///
    /// let handler = DefaultDevChannelHandler;
    /// let res = handler.handle_custom("ping", serde_json::Value::Null);
    /// assert!(res.is_err());
    /// ```
    fn handle_custom(
        &self,
        method: &str,
        params: serde_json::Value,
    ) -> Result<serde_json::Value, (i32, String)> {
        let _ = (method, params);
        Err((ERR_METHOD_NOT_FOUND, format!("method not found: {method}")))
    }
}

/// Default implementation of [`DevChannelHandler`] returning empty or no-op responses.
///
/// # Examples
///
/// ```
/// use martensite_host::dev_channel::DefaultDevChannelHandler;
///
/// let handler = DefaultDevChannelHandler;
/// ```
#[derive(Debug, Default, Clone, Copy)]
pub struct DefaultDevChannelHandler;

impl DevChannelHandler for DefaultDevChannelHandler {}

/// Computes the default Unix domain socket path for a dev session.
///
/// Follows ADR-0038 path resolution:
/// - If `$XDG_RUNTIME_DIR` is set and non-empty: `$XDG_RUNTIME_DIR/martensite/<build_id>.sock`
/// - Otherwise: `/tmp/martensite-<user>/<build_id>.sock`, where `<user>` is determined from
///   the `USER` or `LOGNAME` environment variables (defaulting to `"user"`).
///
/// # Examples
///
/// ```
/// use martensite_host::dev_channel::socket_path_for_session;
///
/// let path = socket_path_for_session("test_session");
/// assert!(path.to_string_lossy().contains("test_session.sock"));
/// ```
pub fn socket_path_for_session(build_id: &str) -> PathBuf {
    let dir = if let Ok(xdg) = std::env::var("XDG_RUNTIME_DIR") {
        if !xdg.trim().is_empty() {
            PathBuf::from(xdg).join("martensite")
        } else {
            fallback_tmp_dir()
        }
    } else {
        fallback_tmp_dir()
    };
    dir.join(format!("{build_id}.sock"))
}

fn fallback_tmp_dir() -> PathBuf {
    let user = std::env::var("USER")
        .or_else(|_| std::env::var("LOGNAME"))
        .unwrap_or_else(|_| "user".to_string());
    PathBuf::from(format!("/tmp/martensite-{user}"))
}

/// Prepares the parent directory for a Unix socket with restricted permissions (`0700`).
fn prepare_socket_parent_dir(path: &Path) -> io::Result<()> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let _ = std::fs::set_permissions(parent, std::fs::Permissions::from_mode(0o700));
        }
    }
    Ok(())
}

/// Configuration builder for launching a [`DevChannelServer`].
///
/// # Examples
///
/// ```no_run
/// use martensite_host::dev_channel::DevChannelConfig;
///
/// let config = DevChannelConfig::new()
///     .with_build_id("my_build");
/// let _server = config.start().expect("failed to start");
/// ```
#[derive(Clone)]
pub struct DevChannelConfig {
    /// Build or session identifier.
    pub build_id: Option<String>,
    /// Explicit socket path (overrides build_id if set).
    pub socket_path: Option<PathBuf>,
    /// Request handler implementation.
    pub handler: Arc<dyn DevChannelHandler>,
}

impl Default for DevChannelConfig {
    fn default() -> Self {
        Self::new()
    }
}

impl DevChannelConfig {
    /// Creates a new default [`DevChannelConfig`].
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite_host::dev_channel::DevChannelConfig;
    ///
    /// let config = DevChannelConfig::new();
    /// assert!(config.build_id.is_none());
    /// ```
    pub fn new() -> Self {
        Self {
            build_id: None,
            socket_path: None,
            handler: Arc::new(DefaultDevChannelHandler),
        }
    }

    /// Sets the build or session identifier for socket path derivation.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite_host::dev_channel::DevChannelConfig;
    ///
    /// let config = DevChannelConfig::new().with_build_id("build_42");
    /// assert_eq!(config.build_id.as_deref(), Some("build_42"));
    /// ```
    pub fn with_build_id(mut self, build_id: impl Into<String>) -> Self {
        self.build_id = Some(build_id.into());
        self
    }

    /// Sets an explicit socket path.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite_host::dev_channel::DevChannelConfig;
    /// use std::path::Path;
    ///
    /// let config = DevChannelConfig::new().with_socket_path(Path::new("/tmp/test.sock"));
    /// assert!(config.socket_path.is_some());
    /// ```
    pub fn with_socket_path(mut self, path: impl Into<PathBuf>) -> Self {
        self.socket_path = Some(path.into());
        self
    }

    /// Sets the request handler.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite_host::dev_channel::{DevChannelConfig, DefaultDevChannelHandler};
    /// use std::sync::Arc;
    ///
    /// let config = DevChannelConfig::new().with_handler(Arc::new(DefaultDevChannelHandler));
    /// ```
    pub fn with_handler(mut self, handler: Arc<dyn DevChannelHandler>) -> Self {
        self.handler = handler;
        self
    }

    /// Launches the [`DevChannelServer`] using this configuration.
    ///
    /// # Examples
    ///
    /// ```no_run
    /// use martensite_host::dev_channel::DevChannelConfig;
    ///
    /// let server = DevChannelConfig::new()
    ///     .with_build_id("session_abc")
    ///     .start()
    ///     .expect("failed to start");
    /// ```
    pub fn start(self) -> io::Result<DevChannelServer> {
        DevChannelServer::start_with_config(self)
    }
}

/// The Dev Channel server managing the Unix domain socket IPC transport.
///
/// Automatically creates the socket with user-only permissions (`0600`) and
/// removes the socket file upon drop or explicit [`stop`](Self::stop).
///
/// # Examples
///
/// ```no_run
/// use martensite_host::dev_channel::DevChannelServer;
///
/// let server = DevChannelServer::start("session_demo").expect("failed to bind");
/// assert!(server.is_running());
/// ```
pub struct DevChannelServer {
    socket_path: Option<PathBuf>,
    running: Arc<AtomicBool>,
    accept_thread: Option<JoinHandle<()>>,
}

impl fmt::Debug for DevChannelServer {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("DevChannelServer")
            .field("socket_path", &self.socket_path)
            .field("is_running", &self.is_running())
            .finish()
    }
}

impl DevChannelServer {
    /// Starts the Dev Channel server for the given `build_id`.
    ///
    /// # Examples
    ///
    /// ```no_run
    /// use martensite_host::dev_channel::DevChannelServer;
    ///
    /// let server = DevChannelServer::start("session_123").expect("failed to start");
    /// ```
    pub fn start(build_id: &str) -> io::Result<Self> {
        DevChannelConfig::new().with_build_id(build_id).start()
    }

    /// Starts the Dev Channel server bound to an explicit socket path.
    ///
    /// # Examples
    ///
    /// ```no_run
    /// use martensite_host::dev_channel::DevChannelServer;
    /// use std::path::Path;
    ///
    /// let server = DevChannelServer::bind(Path::new("/tmp/test.sock")).expect("failed to bind");
    /// ```
    pub fn bind(path: impl AsRef<Path>) -> io::Result<Self> {
        DevChannelConfig::new()
            .with_socket_path(path.as_ref())
            .start()
    }

    /// Starts the Dev Channel server with custom configuration.
    ///
    /// # Examples
    ///
    /// ```no_run
    /// use martensite_host::dev_channel::{DevChannelConfig, DevChannelServer};
    ///
    /// let config = DevChannelConfig::new().with_build_id("test");
    /// let server = DevChannelServer::start_with_config(config).expect("failed to start");
    /// ```
    pub fn start_with_config(config: DevChannelConfig) -> io::Result<Self> {
        let socket_path = if let Some(path) = config.socket_path {
            path
        } else if let Some(ref build_id) = config.build_id {
            socket_path_for_session(build_id)
        } else {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "DevChannelConfig requires either build_id or socket_path",
            ));
        };

        prepare_socket_parent_dir(&socket_path)?;

        // Remove any stale socket file at the target path before binding.
        if socket_path.exists() {
            let _ = std::fs::remove_file(&socket_path);
        }

        let listener = UnixListener::bind(&socket_path)?;

        // Enforce user-only socket permissions (0600) per ADR-0038.
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let _ = std::fs::set_permissions(&socket_path, std::fs::Permissions::from_mode(0o600));
        }

        let running = Arc::new(AtomicBool::new(true));
        let running_listener = Arc::clone(&running);
        let handler = config.handler;
        let build_id = config.build_id.clone();

        let accept_thread = std::thread::Builder::new()
            .name("martensite-dev-channel-listener".to_string())
            .spawn(move || {
                run_listener_loop(listener, handler, running_listener, build_id);
            })?;

        Ok(Self {
            socket_path: Some(socket_path),
            running,
            accept_thread: Some(accept_thread),
        })
    }

    /// Returns the filesystem path of the active Unix domain socket.
    ///
    /// # Examples
    ///
    /// ```no_run
    /// use martensite_host::dev_channel::DevChannelServer;
    ///
    /// let server = DevChannelServer::start("session_123").unwrap();
    /// assert!(server.socket_path().exists());
    /// ```
    pub fn socket_path(&self) -> &Path {
        self.socket_path.as_deref().unwrap_or_else(|| Path::new(""))
    }

    /// Returns `true` if the server's listener thread is actively running.
    ///
    /// # Examples
    ///
    /// ```no_run
    /// use martensite_host::dev_channel::DevChannelServer;
    ///
    /// let server = DevChannelServer::start("session_123").unwrap();
    /// assert!(server.is_running());
    /// ```
    pub fn is_running(&self) -> bool {
        self.running.load(Ordering::SeqCst)
    }

    /// Gracefully stops the Dev Channel server and removes the socket file.
    ///
    /// # Examples
    ///
    /// ```no_run
    /// use martensite_host::dev_channel::DevChannelServer;
    ///
    /// let mut server = DevChannelServer::start("session_123").unwrap();
    /// server.stop();
    /// assert!(!server.is_running());
    /// ```
    pub fn stop(&mut self) {
        if !self.running.swap(false, Ordering::SeqCst) {
            return;
        }

        // Unblock accept() by connecting to the socket if it still exists.
        if let Some(ref path) = self.socket_path {
            let _ = UnixStream::connect(path);
        }

        if let Some(handle) = self.accept_thread.take() {
            let _ = handle.join();
        }

        if let Some(ref path) = self.socket_path {
            let _ = std::fs::remove_file(path);
        }
    }
}

impl Drop for DevChannelServer {
    fn drop(&mut self) {
        self.stop();
    }
}

/// Main accept loop running in a background thread.
fn run_listener_loop(
    listener: UnixListener,
    handler: Arc<dyn DevChannelHandler>,
    running: Arc<AtomicBool>,
    build_id: Option<String>,
) {
    while running.load(Ordering::SeqCst) {
        match listener.accept() {
            Ok((stream, _)) => {
                if !running.load(Ordering::SeqCst) {
                    break;
                }
                let handler = Arc::clone(&handler);
                let running_child = Arc::clone(&running);
                let build_id = build_id.clone();
                let _ = std::thread::Builder::new()
                    .name("martensite-dev-channel-client".to_string())
                    .spawn(move || {
                        handle_client(stream, handler, running_child, build_id);
                    });
            }
            Err(_) => {
                if !running.load(Ordering::SeqCst) {
                    break;
                }
                std::thread::sleep(Duration::from_millis(5));
            }
        }
    }
}

/// Handles incoming requests from an accepted client connection.
fn handle_client(
    mut stream: UnixStream,
    handler: Arc<dyn DevChannelHandler>,
    running: Arc<AtomicBool>,
    build_id: Option<String>,
) {
    let read_stream = match stream.try_clone() {
        Ok(s) => s,
        Err(_) => return,
    };
    let reader = BufReader::new(read_stream);
    let mut handshook = false;

    for line in reader.lines() {
        if !running.load(Ordering::SeqCst) {
            break;
        }
        let line = match line {
            Ok(l) => l,
            Err(_) => break,
        };
        let trimmed = line.trim();
        if trimmed.is_empty() {
            continue;
        }

        let response = process_request_line(
            trimmed,
            &mut handshook,
            handler.as_ref(),
            build_id.as_deref(),
        );

        let mut resp_json = match serde_json::to_string(&response) {
            Ok(s) => s,
            Err(e) => {
                let err_resp = JsonRpcResponse::error(
                    response.id,
                    ERR_INTERNAL,
                    format!("failed to serialize JSON-RPC response: {e}"),
                    None,
                );
                serde_json::to_string(&err_resp).unwrap_or_default()
            }
        };
        resp_json.push('\n');

        if stream.write_all(resp_json.as_bytes()).is_err() {
            break;
        }
        let _ = stream.flush();
    }
}

/// Processes a single request line and returns the appropriate JSON-RPC response.
pub(crate) fn process_request_line(
    line: &str,
    handshook: &mut bool,
    handler: &dyn DevChannelHandler,
    build_id: Option<&str>,
) -> JsonRpcResponse {
    let req: JsonRpcRequest = match serde_json::from_str(line) {
        Ok(r) => r,
        Err(_) => {
            return JsonRpcResponse::error(
                None,
                ERR_PARSE,
                "parse error: invalid JSON-RPC payload",
                None,
            );
        }
    };

    let norm_method = req.method.to_ascii_lowercase().replace(['_', '-', ' '], "");

    // Constraint D1: Mandatory Hello handshake must be the very first request.
    if !*handshook && norm_method != "hello" {
        return JsonRpcResponse::error(
            req.id,
            ERR_HANDSHAKE_REQUIRED,
            "handshake required: the first request on a dev channel connection must be 'Hello'",
            Some(serde_json::json!({
                "error_type": "handshake_required",
                "server_version": MARTENSITE_VERSION,
                "protocol_version": DEV_CHANNEL_PROTOCOL_VERSION
            })),
        );
    }

    match norm_method.as_str() {
        "hello" => {
            let params: HelloParams = match serde_json::from_value(req.params) {
                Ok(p) => p,
                Err(e) => {
                    return JsonRpcResponse::error(
                        req.id,
                        ERR_INVALID_PARAMS,
                        format!("invalid Hello parameters: {e}"),
                        None,
                    );
                }
            };

            // Version handshake verification (Constraint D1).
            let version_matches = params.client_version == MARTENSITE_VERSION;
            let protocol_matches = params.protocol_version == DEV_CHANNEL_PROTOCOL_VERSION;

            if !version_matches || !protocol_matches {
                *handshook = false;
                let message = format!(
                    "version_mismatch: client is v{} (protocol {}) but server is v{} (protocol {})",
                    params.client_version,
                    params.protocol_version,
                    MARTENSITE_VERSION,
                    DEV_CHANNEL_PROTOCOL_VERSION
                );
                return JsonRpcResponse::error(
                    req.id,
                    ERR_VERSION_MISMATCH,
                    message,
                    Some(serde_json::json!({
                        "error_type": "version_mismatch",
                        "client_version": params.client_version,
                        "server_version": MARTENSITE_VERSION,
                        "client_protocol_version": params.protocol_version,
                        "server_protocol_version": DEV_CHANNEL_PROTOCOL_VERSION,
                    })),
                );
            }

            *handshook = true;
            let result = HelloResult {
                server_version: MARTENSITE_VERSION.to_string(),
                protocol_version: DEV_CHANNEL_PROTOCOL_VERSION,
                build_id: build_id.map(ToString::to_string),
            };
            JsonRpcResponse::success(req.id, serde_json::to_value(result).unwrap_or_default())
        }
        "treesnapshot" => {
            let params: TreeSnapshotParams = if req.params.is_null() {
                TreeSnapshotParams::default()
            } else {
                match serde_json::from_value(req.params) {
                    Ok(p) => p,
                    Err(e) => {
                        return JsonRpcResponse::error(
                            req.id,
                            ERR_INVALID_PARAMS,
                            format!("invalid TreeSnapshot parameters: {e}"),
                            None,
                        );
                    }
                }
            };
            match handler.handle_tree_snapshot(params) {
                Ok(val) => JsonRpcResponse::success(req.id, val),
                Err(msg) => JsonRpcResponse::error(req.id, ERR_INTERNAL, msg, None),
            }
        }
        "lintpull" | "lintscene" => {
            let params: LintPullParams = if req.params.is_null() {
                LintPullParams::default()
            } else {
                match serde_json::from_value(req.params) {
                    Ok(p) => p,
                    Err(e) => {
                        return JsonRpcResponse::error(
                            req.id,
                            ERR_INVALID_PARAMS,
                            format!("invalid LintPull parameters: {e}"),
                            None,
                        );
                    }
                }
            };
            match handler.handle_lint_pull(params) {
                Ok(val) => JsonRpcResponse::success(req.id, val),
                Err(msg) => JsonRpcResponse::error(req.id, ERR_INTERNAL, msg, None),
            }
        }
        "eventledger" => {
            let params: EventLedgerParams = if req.params.is_null() {
                EventLedgerParams::default()
            } else {
                match serde_json::from_value(req.params) {
                    Ok(p) => p,
                    Err(e) => {
                        return JsonRpcResponse::error(
                            req.id,
                            ERR_INVALID_PARAMS,
                            format!("invalid EventLedger parameters: {e}"),
                            None,
                        );
                    }
                }
            };
            match handler.handle_event_ledger(params) {
                Ok(val) => JsonRpcResponse::success(req.id, val),
                Err(msg) => JsonRpcResponse::error(req.id, ERR_INTERNAL, msg, None),
            }
        }
        "inspectorselect" => {
            let params: InspectorSelectParams = match serde_json::from_value(req.params) {
                Ok(p) => p,
                Err(e) => {
                    return JsonRpcResponse::error(
                        req.id,
                        ERR_INVALID_PARAMS,
                        format!("invalid InspectorSelect parameters: {e}"),
                        None,
                    );
                }
            };
            match handler.handle_inspector_select(params) {
                Ok(val) => JsonRpcResponse::success(req.id, val),
                Err(msg) => JsonRpcResponse::error(req.id, ERR_INTERNAL, msg, None),
            }
        }
        "lintapply" => {
            let params: LintApplyParams = if req.params.is_null() {
                LintApplyParams::default()
            } else {
                match serde_json::from_value(req.params) {
                    Ok(p) => p,
                    Err(e) => {
                        return JsonRpcResponse::error(
                            req.id,
                            ERR_INVALID_PARAMS,
                            format!("invalid LintApply parameters: {e}"),
                            None,
                        );
                    }
                }
            };
            match handler.handle_lint_apply(params) {
                Ok(val) => JsonRpcResponse::success(req.id, val),
                Err(msg) => JsonRpcResponse::error(req.id, ERR_INTERNAL, msg, None),
            }
        }
        other => match handler.handle_custom(other, req.params) {
            Ok(val) => JsonRpcResponse::success(req.id, val),
            Err((code, msg)) => JsonRpcResponse::error(req.id, code, msg, None),
        },
    }
}

/// A lightweight client connecting to a [`DevChannelServer`] over a local Unix domain socket.
///
/// # Examples
///
/// ```no_run
/// use martensite_host::dev_channel::{DevChannelClient, MARTENSITE_VERSION, DEV_CHANNEL_PROTOCOL_VERSION};
/// use std::path::Path;
///
/// let mut client = DevChannelClient::connect(Path::new("/tmp/martensite.sock")).unwrap();
/// let handshake = client.hello(MARTENSITE_VERSION, DEV_CHANNEL_PROTOCOL_VERSION).unwrap();
/// assert!(handshake.is_ok());
/// ```
pub struct DevChannelClient {
    stream: UnixStream,
    reader: BufReader<UnixStream>,
    next_id: AtomicU64,
}

impl fmt::Debug for DevChannelClient {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("DevChannelClient")
            .field("next_id", &self.next_id.load(Ordering::Relaxed))
            .finish_non_exhaustive()
    }
}

impl DevChannelClient {
    /// Connects to a Dev Channel Unix domain socket at `path`.
    ///
    /// # Examples
    ///
    /// ```no_run
    /// use martensite_host::dev_channel::DevChannelClient;
    /// use std::path::Path;
    ///
    /// let client = DevChannelClient::connect(Path::new("/tmp/test.sock"));
    /// ```
    pub fn connect(path: impl AsRef<Path>) -> io::Result<Self> {
        let stream = UnixStream::connect(path.as_ref())?;
        let read_stream = stream.try_clone()?;
        Ok(Self {
            stream,
            reader: BufReader::new(read_stream),
            next_id: AtomicU64::new(1),
        })
    }

    /// Sends a raw [`JsonRpcRequest`] and reads the corresponding [`JsonRpcResponse`].
    ///
    /// # Examples
    ///
    /// ```no_run
    /// use martensite_host::dev_channel::{DevChannelClient, JsonRpcRequest};
    /// use std::path::Path;
    ///
    /// let mut client = DevChannelClient::connect(Path::new("/tmp/test.sock")).unwrap();
    /// let req = JsonRpcRequest::new(Some(1), "Hello", serde_json::Value::Null);
    /// let resp = client.send_raw(&req).unwrap();
    /// ```
    pub fn send_raw(&mut self, request: &JsonRpcRequest) -> io::Result<JsonRpcResponse> {
        let mut json = serde_json::to_string(request)
            .map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e))?;
        json.push('\n');
        self.stream.write_all(json.as_bytes())?;
        self.stream.flush()?;

        let mut line = String::new();
        let bytes_read = self.reader.read_line(&mut line)?;
        if bytes_read == 0 {
            return Err(io::Error::new(
                io::ErrorKind::UnexpectedEof,
                "server closed connection prematurely",
            ));
        }

        serde_json::from_str(&line).map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e))
    }

    /// Performs the mandatory `Hello` handshake with the server.
    ///
    /// # Examples
    ///
    /// ```no_run
    /// use martensite_host::dev_channel::{DevChannelClient, MARTENSITE_VERSION, DEV_CHANNEL_PROTOCOL_VERSION};
    /// use std::path::Path;
    ///
    /// let mut client = DevChannelClient::connect(Path::new("/tmp/test.sock")).unwrap();
    /// let res = client.hello(MARTENSITE_VERSION, DEV_CHANNEL_PROTOCOL_VERSION).unwrap();
    /// ```
    pub fn hello(
        &mut self,
        client_version: &str,
        protocol_version: u32,
    ) -> io::Result<Result<HelloResult, JsonRpcError>> {
        let id = self.next_id.fetch_add(1, Ordering::SeqCst);
        let params = HelloParams {
            client_version: client_version.to_string(),
            protocol_version,
        };
        let req = JsonRpcRequest::new(
            Some(id),
            "Hello",
            serde_json::to_value(params).unwrap_or_default(),
        );
        let resp = self.send_raw(&req)?;
        if let Some(err) = resp.error {
            Ok(Err(err))
        } else if let Some(res) = resp.result {
            let hello: HelloResult = serde_json::from_value(res)
                .map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e))?;
            Ok(Ok(hello))
        } else {
            Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "response missing both result and error fields",
            ))
        }
    }

    /// Requests a widget tree snapshot from the server.
    ///
    /// # Examples
    ///
    /// ```no_run
    /// use martensite_host::dev_channel::{DevChannelClient, TreeSnapshotParams};
    /// use std::path::Path;
    ///
    /// let mut client = DevChannelClient::connect(Path::new("/tmp/test.sock")).unwrap();
    /// let res = client.tree_snapshot(TreeSnapshotParams::default()).unwrap();
    /// ```
    pub fn tree_snapshot(
        &mut self,
        params: TreeSnapshotParams,
    ) -> io::Result<Result<serde_json::Value, JsonRpcError>> {
        self.invoke_method(
            "TreeSnapshot",
            serde_json::to_value(params).unwrap_or_default(),
        )
    }

    /// Pulls current design lint findings from the server.
    ///
    /// # Examples
    ///
    /// ```no_run
    /// use martensite_host::dev_channel::{DevChannelClient, LintPullParams};
    /// use std::path::Path;
    ///
    /// let mut client = DevChannelClient::connect(Path::new("/tmp/test.sock")).unwrap();
    /// let res = client.lint_pull(LintPullParams::default()).unwrap();
    /// ```
    pub fn lint_pull(
        &mut self,
        params: LintPullParams,
    ) -> io::Result<Result<serde_json::Value, JsonRpcError>> {
        self.invoke_method("LintPull", serde_json::to_value(params).unwrap_or_default())
    }

    /// Queries the event ledger history tail from the server.
    ///
    /// # Examples
    ///
    /// ```no_run
    /// use martensite_host::dev_channel::{DevChannelClient, EventLedgerParams};
    /// use std::path::Path;
    ///
    /// let mut client = DevChannelClient::connect(Path::new("/tmp/test.sock")).unwrap();
    /// let res = client.event_ledger(EventLedgerParams::default()).unwrap();
    /// ```
    pub fn event_ledger(
        &mut self,
        params: EventLedgerParams,
    ) -> io::Result<Result<serde_json::Value, JsonRpcError>> {
        self.invoke_method(
            "EventLedger",
            serde_json::to_value(params).unwrap_or_default(),
        )
    }

    /// Arms or disarms DevTools inspector select mode.
    ///
    /// # Examples
    ///
    /// ```no_run
    /// use martensite_host::dev_channel::DevChannelClient;
    /// use std::path::Path;
    ///
    /// let mut client = DevChannelClient::connect(Path::new("/tmp/test.sock")).unwrap();
    /// let res = client.inspector_select(true).unwrap();
    /// ```
    pub fn inspector_select(
        &mut self,
        arm: bool,
    ) -> io::Result<Result<serde_json::Value, JsonRpcError>> {
        self.invoke_method(
            "InspectorSelect",
            serde_json::to_value(InspectorSelectParams { arm }).unwrap_or_default(),
        )
    }

    /// Applies design lint fixes against a copy of the scene model.
    ///
    /// # Examples
    ///
    /// ```no_run
    /// use martensite_host::dev_channel::{DevChannelClient, LintApplyParams};
    /// use std::path::Path;
    ///
    /// let mut client = DevChannelClient::connect(Path::new("/tmp/test.sock")).unwrap();
    /// let res = client.lint_apply(LintApplyParams::default()).unwrap();
    /// ```
    pub fn lint_apply(
        &mut self,
        params: LintApplyParams,
    ) -> io::Result<Result<serde_json::Value, JsonRpcError>> {
        self.invoke_method(
            "LintApply",
            serde_json::to_value(params).unwrap_or_default(),
        )
    }

    /// Internal helper for invoking a typed JSON-RPC method.
    fn invoke_method(
        &mut self,
        method: &str,
        params: serde_json::Value,
    ) -> io::Result<Result<serde_json::Value, JsonRpcError>> {
        let id = self.next_id.fetch_add(1, Ordering::SeqCst);
        let req = JsonRpcRequest::new(Some(id), method, params);
        let resp = self.send_raw(&req)?;
        if let Some(err) = resp.error {
            Ok(Err(err))
        } else if let Some(res) = resp.result {
            Ok(Ok(res))
        } else {
            Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "response missing both result and error fields",
            ))
        }
    }
}
