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
mod win32 {
    pub const PIPE_ACCESS_DUPLEX: u32 = 0x00000003;
    pub const PIPE_TYPE_BYTE: u32 = 0x00000000;
    pub const PIPE_WAIT: u32 = 0x00000000;
    pub const PIPE_UNLIMITED_INSTANCES: u32 = 255;
    pub const INVALID_HANDLE_VALUE: isize = -1;
    pub const ERROR_PIPE_CONNECTED: u32 = 535;

    #[link(name = "kernel32")]
    extern "system" {
        pub fn CreateNamedPipeW(
            lpName: *const u16,
            dwOpenMode: u32,
            dwPipeMode: u32,
            nMaxInstances: u32,
            nOutBufferSize: u32,
            nInBufferSize: u32,
            nDefaultTimeOut: u32,
            lpSecurityAttributes: *mut std::ffi::c_void,
        ) -> isize;

        pub fn ConnectNamedPipe(hNamedPipe: isize, lpOverlapped: *mut std::ffi::c_void) -> i32;

        pub fn CloseHandle(hObject: isize) -> i32;

        pub fn GetLastError() -> u32;
    }
}

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
/// Custom error code indicating that the `Hello` handshake did not present the
/// bearer token the session requires (ADR-0042 authenticated forwarders).
pub const ERR_AUTH_FAILED: i32 = -32002;

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
///     auth_token: None,
/// };
/// assert_eq!(params.protocol_version, 1);
/// ```
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct HelloParams {
    /// The client application's Martensite framework version.
    #[serde(alias = "version")]
    pub client_version: String,
    /// Protocol version supported by the client.
    pub protocol_version: u32,
    /// Optional bearer token presented during the handshake (ADR-0042).
    ///
    /// Only inspected when the server was configured with a required token —
    /// see [`DevChannelConfig::with_auth_token`]. Local Unix-socket sessions
    /// that configure no token ignore this field entirely.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub auth_token: Option<String>,
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
    #[serde(alias = "version")]
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
///     ..Default::default()
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
    /// Maximum number of children emitted per node.
    #[serde(default)]
    pub child_limit: Option<usize>,
    /// Child/index offset for paginated snapshots.
    #[serde(default)]
    pub offset: Option<usize>,
    /// Optional marker filter; matching nodes retain ancestors as context.
    #[serde(default)]
    pub filter_marker: Option<String>,
    /// Whether widget-internal children should be included in the snapshot.
    #[serde(default)]
    pub include_internal: Option<bool>,
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
///     ..Default::default()
/// };
/// assert_eq!(params.tail_count, Some(50));
/// ```
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Default)]
pub struct EventLedgerParams {
    /// Number of most recent event records to return from the tail of the ledger.
    #[serde(default)]
    pub tail_count: Option<usize>,
    /// Alias used by the MCP client (`martensite_get_event_ledger`).
    #[serde(default)]
    pub limit: Option<usize>,
}

/// Parameters for the `InspectorSelect` request.
///
/// # Examples
///
/// ```
/// use martensite_host::dev_channel::InspectorSelectParams;
///
/// let params = InspectorSelectParams {
///     arm: true,
///     ..Default::default()
/// };
/// assert!(params.arm);
/// ```
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Default)]
pub struct InspectorSelectParams {
    /// Whether to arm (`true`) or disarm (`false`) Chrome DevTools-style pixel inspection mode.
    pub arm: bool,
    /// Whether the caller wants the handler to wait for a user click before responding.
    #[serde(default)]
    pub wait: bool,
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
///     ..Default::default()
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
    /// Fingerprint of the lint finding to fix (`martensite_apply_lint_fix`).
    #[serde(default)]
    pub finding_id: Option<String>,
    /// Fix output target: `"scene"` (post-fix dump) or `"patch"` (unified diff).
    #[serde(default)]
    pub target: Option<String>,
    /// Whether the fix engine may apply dependent fixes recursively.
    #[serde(default)]
    pub recursive: bool,
    /// Maximum recursion depth for dependent fixes.
    #[serde(default)]
    pub max_recursiveness: Option<u32>,
}

/// Deserializes a `u64` that may arrive on the wire either as a JSON number
/// or as a decimal string.
///
/// Dev-channel clients such as `martensite-mcp` encode node, checkpoint, and
/// revision identifiers as plain strings on some code paths; this adapter
/// accepts both representations so handlers see a single canonical type.
/// Non-numeric strings and non-integer values fail deserialization, which
/// `process_request_line` reports as `ERR_INVALID_PARAMS`.
fn deserialize_u64_wire<'de, D>(deserializer: D) -> Result<u64, D::Error>
where
    D: serde::Deserializer<'de>,
{
    match serde_json::Value::deserialize(deserializer)? {
        serde_json::Value::Number(n) => n
            .as_u64()
            .ok_or_else(|| serde::de::Error::custom("expected a non-negative integer")),
        serde_json::Value::String(s) => s
            .trim()
            .parse::<u64>()
            .map_err(|_| serde::de::Error::custom("expected a decimal integer string")),
        other => Err(serde::de::Error::custom(format!(
            "expected an integer or decimal string, got {other}"
        ))),
    }
}

/// Optional variant of [`deserialize_u64_wire`] for `Option<u64>` fields:
/// `null` and absent values map to `None`.
fn deserialize_opt_u64_wire<'de, D>(deserializer: D) -> Result<Option<u64>, D::Error>
where
    D: serde::Deserializer<'de>,
{
    match Option::<serde_json::Value>::deserialize(deserializer)? {
        None | Some(serde_json::Value::Null) => Ok(None),
        Some(serde_json::Value::Number(n)) => n
            .as_u64()
            .map(Some)
            .ok_or_else(|| serde::de::Error::custom("expected a non-negative integer")),
        Some(serde_json::Value::String(s)) => s
            .trim()
            .parse::<u64>()
            .map(Some)
            .map_err(|_| serde::de::Error::custom("expected a decimal integer string")),
        Some(other) => Err(serde::de::Error::custom(format!(
            "expected an integer or decimal string, got {other}"
        ))),
    }
}

/// Builds the sentinel `not_implemented:<method>` error string returned by the
/// default [`DevChannelHandler`] method implementations.
///
/// `process_request_line` maps errors carrying this prefix to
/// [`ERR_METHOD_NOT_FOUND`] (-32601) instead of [`ERR_INTERNAL`], preserving
/// semantic honesty for methods a dev app does not yet implement (D9).
fn not_implemented_err(method: &str) -> String {
    format!("not_implemented:{method}")
}

/// Parameters for the `TreeNode` request (`martensite_get_node`, MCP spec §3.2).
///
/// Identifies a single widget node whose complete record — generational
/// index, kind, computed bounds, markers, ARIA role, AccessKit properties,
/// and optional source span — is requested.
///
/// # Examples
///
/// ```
/// use martensite_host::dev_channel::TreeNodeParams;
///
/// let params = TreeNodeParams { node_id: 42 };
/// assert_eq!(params.node_id, 42);
///
/// // Numeric identifiers may also arrive as decimal strings on the wire.
/// let parsed: TreeNodeParams =
///     serde_json::from_str(r#"{ "node_id": "42" }"#).unwrap();
/// assert_eq!(parsed, params);
/// ```
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Default)]
pub struct TreeNodeParams {
    /// Arena identifier of the widget node to inspect (accepts a JSON number
    /// or a decimal string).
    #[serde(default, deserialize_with = "deserialize_u64_wire")]
    pub node_id: u64,
}

/// Parameters for the `LayoutChain` request (`martensite_diagnose_layout`, MCP spec §3.3).
///
/// Selects the widget whose Taffy constraint-resolution chain — inbound
/// constraints, style definitions, intrinsic measure output, and resolved
/// bounds with ancestry propagation — is requested.
///
/// # Examples
///
/// ```
/// use martensite_host::dev_channel::LayoutChainParams;
///
/// let params = LayoutChainParams { node_id: 7 };
/// assert_eq!(params.node_id, 7);
/// ```
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Default)]
pub struct LayoutChainParams {
    /// Arena identifier of the widget node whose layout chain is requested
    /// (accepts a JSON number or a decimal string).
    #[serde(default, deserialize_with = "deserialize_u64_wire")]
    pub node_id: u64,
}

/// Parameters for the `OverflowScan` request (`martensite_explain_overflow`, MCP spec §3.4).
///
/// When `node_id` is omitted, the handler scans the entire scene for active
/// layout overflows; otherwise it pinpoints the clipping lineage of the
/// given subtree.
///
/// # Examples
///
/// ```
/// use martensite_host::dev_channel::OverflowScanParams;
///
/// // Scene-wide scan.
/// assert_eq!(OverflowScanParams::default().node_id, None);
///
/// let params = OverflowScanParams { node_id: Some(9) };
/// assert_eq!(params.node_id, Some(9));
/// ```
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Default)]
pub struct OverflowScanParams {
    /// Optional widget node anchoring the overflow diagnosis (accepts a JSON
    /// number or a decimal string).
    #[serde(default, deserialize_with = "deserialize_opt_u64_wire")]
    pub node_id: Option<u64>,
}

/// Parameters for the `SignalsList` request (`martensite_inspect_signals`, MCP spec §3.5).
///
/// Introspects the push-pull reactive signal DAG with token-safe pagination
/// and optional dirty-only filtering.
///
/// # Examples
///
/// ```
/// use martensite_host::dev_channel::SignalsListParams;
///
/// let params = SignalsListParams {
///     signal_id: None,
///     node_id: Some(3),
///     only_dirty: true,
///     limit: Some(50),
///     offset: 0,
/// };
/// assert!(params.only_dirty);
/// assert_eq!(params.limit, Some(50));
/// ```
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Default)]
pub struct SignalsListParams {
    /// Optional signal identifier narrowing the result to a single signal.
    #[serde(default)]
    pub signal_id: Option<String>,
    /// Optional widget node anchor returning only signals this node
    /// subscribes to (accepts a JSON number or a decimal string).
    #[serde(default, deserialize_with = "deserialize_opt_u64_wire")]
    pub node_id: Option<u64>,
    /// When `true`, only signals flagged dirty in the current evaluation
    /// tick are returned.
    #[serde(default)]
    pub only_dirty: bool,
    /// Maximum number of signal descriptors to return (spec default `50`,
    /// max `200`).
    #[serde(default)]
    pub limit: Option<usize>,
    /// Pagination offset into the signal list.
    #[serde(default)]
    pub offset: usize,
}

/// Parameters for the `SignalTrigger` request (`martensite_trigger_signal`, MCP spec §3.6).
///
/// Mutates a registered tweakable reactive signal value in dev mode. Writes
/// are gated to signals registered in `TweakRegistry` (or annotated
/// `#[tweak]`) and are type-checked against the registered schema.
///
/// # Examples
///
/// ```
/// use martensite_host::dev_channel::SignalTriggerParams;
///
/// let params = SignalTriggerParams {
///     signal_id: "counter".to_string(),
///     value: serde_json::json!(7),
/// };
/// assert_eq!(params.signal_id, "counter");
/// assert_eq!(params.value, serde_json::json!(7));
/// ```
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Default)]
pub struct SignalTriggerParams {
    /// Identifier of the signal to update.
    #[serde(default)]
    pub signal_id: String,
    /// Serialized JSON value to assign to the signal.
    #[serde(default)]
    pub value: serde_json::Value,
}

/// Parameters for the `A11yTree` request (`martensite_inspect_a11y_tree`, MCP spec §3.7).
///
/// Evaluates the hierarchical AccessKit accessibility tree mirror
/// independently of the widget layout hierarchy.
///
/// # Examples
///
/// ```
/// use martensite_host::dev_channel::A11yTreeParams;
///
/// let params = A11yTreeParams {
///     root_id: None,
///     role_filter: Some("button".to_string()),
///     include_ignored: false,
/// };
/// assert_eq!(params.role_filter.as_deref(), Some("button"));
/// assert!(!params.include_ignored);
/// ```
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Default)]
pub struct A11yTreeParams {
    /// Optional `NodeId` or widget anchor to scope the subtree (accepts a
    /// JSON number or a decimal string).
    #[serde(default, deserialize_with = "deserialize_opt_u64_wire")]
    pub root_id: Option<u64>,
    /// Optional ARIA role filter (`"button"`, `"heading"`, `"list"`,
    /// `"dialog"`, etc.).
    #[serde(default)]
    pub role_filter: Option<String>,
    /// When `true`, includes presentation-only / unmapped accessibility
    /// nodes (default `false`).
    #[serde(default)]
    pub include_ignored: bool,
}

/// Parameters for the `TweaksList` request (`martensite_list_tweaks`, MCP spec §3.11).
///
/// Carries no fields: the request enumerates all active runtime tweakable
/// parameters registered via `#[tweak]` or `TweakRegistry`.
///
/// # Examples
///
/// ```
/// use martensite_host::dev_channel::TweaksListParams;
///
/// let params = TweaksListParams::default();
/// assert_eq!(serde_json::to_value(&params).unwrap(), serde_json::json!({}));
/// ```
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Default)]
pub struct TweaksListParams {}

/// Parameters for the `TweakSet` request (`martensite_set_tweak`, MCP spec §3.12).
///
/// Updates a single tweak parameter in the running application in real time.
///
/// # Examples
///
/// ```
/// use martensite_host::dev_channel::TweakSetParams;
///
/// let params = TweakSetParams {
///     name: "card_gap".to_string(),
///     value: serde_json::json!(12.5),
/// };
/// assert_eq!(params.name, "card_gap");
/// ```
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Default)]
pub struct TweakSetParams {
    /// Tweak identifier to update.
    #[serde(default)]
    pub name: String,
    /// New value to assign (typed per the registered tweak schema).
    #[serde(default)]
    pub value: serde_json::Value,
}

/// Parameters for the `ThemeSet` request (`martensite_set_theme`, MCP spec §3.13).
///
/// Toggles the dark/light theme mode and/or applies dynamic design token
/// palette overrides.
///
/// # Examples
///
/// ```
/// use martensite_host::dev_channel::ThemeSetParams;
///
/// let params = ThemeSetParams {
///     mode: Some("dark".to_string()),
///     token_overrides: Some(serde_json::json!({ "accent": "#ff8800" })),
/// };
/// assert_eq!(params.mode.as_deref(), Some("dark"));
/// ```
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Default)]
pub struct ThemeSetParams {
    /// Optional theme mode: `"light"`, `"dark"`, or `"system"`.
    #[serde(default)]
    pub mode: Option<String>,
    /// Optional map of token names to Oklab/hex color values.
    #[serde(default)]
    pub token_overrides: Option<serde_json::Value>,
}

/// Parameters for the `ThemeGet` request (theme token resource read).
///
/// Carries no fields: the request returns the current theme mode and active
/// design token values.
///
/// # Examples
///
/// ```
/// use martensite_host::dev_channel::ThemeGetParams;
///
/// let params = ThemeGetParams::default();
/// assert_eq!(serde_json::to_value(&params).unwrap(), serde_json::json!({}));
/// ```
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Default)]
pub struct ThemeGetParams {}

/// Parameters for the `TweaksSync` request (`martensite_sync_tweaks_to_source`, MCP spec §3.14).
///
/// Commits converged live tweak values back to the underlying Rust source
/// files under strict confirmation gates: disk writes proceed only when
/// `dry_run` is `false` AND `confirmed` is `true`, and targets must resolve
/// inside the Cargo workspace `src/` tree.
///
/// # Examples
///
/// ```
/// use martensite_host::dev_channel::TweaksSyncParams;
///
/// let params = TweaksSyncParams {
///     names: vec!["card_gap".to_string()],
///     dry_run: false,
///     confirmed: true,
///     expected_revision: Some(3),
/// };
/// assert_eq!(params.names.len(), 1);
/// assert!(!params.dry_run);
/// ```
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Default)]
pub struct TweaksSyncParams {
    /// Names of tweaks to commit; an empty list targets all modified tweaks.
    #[serde(default)]
    pub names: Vec<String>,
    /// Preview-only mode (spec default `true`); disk writes require `false`.
    #[serde(default)]
    pub dry_run: bool,
    /// Mandatory confirmation gate; disk writes require `true`.
    #[serde(default)]
    pub confirmed: bool,
    /// Optional optimistic-concurrency revision token preventing multi-agent
    /// write races (accepts a JSON number or a decimal string).
    #[serde(
        default,
        deserialize_with = "deserialize_opt_u64_wire",
        skip_serializing_if = "Option::is_none"
    )]
    pub expected_revision: Option<u64>,
}

/// Parameters for the `EventDispatch` request (`martensite_dispatch_event`, MCP spec §3.15).
///
/// Injects a synthetic input event into the running application's
/// `EventRouter` and reports the routing outcome and hit-test path.
///
/// # Examples
///
/// ```
/// use martensite_host::dev_channel::EventDispatchParams;
///
/// let params = EventDispatchParams {
///     event_type: "pointer_click".to_string(),
///     position: Some([12.0, 34.0]),
///     key: None,
///     text: None,
///     delta: None,
/// };
/// assert_eq!(params.event_type, "pointer_click");
/// assert_eq!(params.position, Some([12.0, 34.0]));
/// ```
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Default)]
pub struct EventDispatchParams {
    /// Event kind: `"pointer_click"`, `"pointer_move"`, `"scroll"`,
    /// `"key_press"`, or `"text_input"`.
    #[serde(default)]
    pub event_type: String,
    /// Optional `[x, y]` logical coordinates for pointer events.
    #[serde(default)]
    pub position: Option<[f32; 2]>,
    /// Optional virtual key code for `key_press` events.
    #[serde(default)]
    pub key: Option<String>,
    /// Optional committed text for `text_input` events.
    #[serde(default)]
    pub text: Option<String>,
    /// Optional `[dx, dy]` scroll delta for scroll events.
    #[serde(default)]
    pub delta: Option<[f32; 2]>,
}

/// Parameters for the `TimemachineStep` request (`martensite_step_timemachine`, MCP spec §3.17).
///
/// Controls the deterministic TimeMachine debugger: pause, resume, step
/// through recorded frames, or restore a specific checkpoint.
///
/// # Examples
///
/// ```
/// use martensite_host::dev_channel::TimemachineStepParams;
///
/// let params = TimemachineStepParams {
///     action: "restore_checkpoint".to_string(),
///     checkpoint_id: Some(12),
/// };
/// assert_eq!(params.action, "restore_checkpoint");
/// assert_eq!(params.checkpoint_id, Some(12));
/// ```
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Default)]
pub struct TimemachineStepParams {
    /// TimeMachine action: `"pause"`, `"resume"`, `"step_forward"`,
    /// `"step_backward"`, or `"restore_checkpoint"`.
    #[serde(default)]
    pub action: String,
    /// Optional frame checkpoint to restore (required for
    /// `"restore_checkpoint"`; accepts a JSON number or a decimal string).
    #[serde(default, deserialize_with = "deserialize_opt_u64_wire")]
    pub checkpoint_id: Option<u64>,
}

/// Parameters for the `CaptureNode` request (`martensite_capture_node`, MCP spec §3.18).
///
/// Renders an individual widget subtree directly to an image artifact
/// without desktop window chrome.
///
/// # Examples
///
/// ```
/// use martensite_host::dev_channel::CaptureNodeParams;
///
/// let params = CaptureNodeParams {
///     node_id: 5,
///     format: Some("png".to_string()),
///     scale: Some(2.0),
/// };
/// assert_eq!(params.format.as_deref(), Some("png"));
/// assert_eq!(params.scale, Some(2.0));
/// ```
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Default)]
pub struct CaptureNodeParams {
    /// Arena identifier of the widget node to rasterize (accepts a JSON
    /// number or a decimal string).
    #[serde(default, deserialize_with = "deserialize_u64_wire")]
    pub node_id: u64,
    /// Image encoding: `"png"` (spec default) or `"jpeg"`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub format: Option<String>,
    /// Optional DPI scale multiplier (spec default `1.0`).
    #[serde(default)]
    pub scale: Option<f64>,
}

/// Parameters for the `AuditPaint` request (`martensite_audit_paint`, MCP spec §3.10).
///
/// Performs a deep visual audit on text rendering, WCAG contrast, and
/// non-text keylines. When `node_id` is omitted, the whole scene is audited.
///
/// # Examples
///
/// ```
/// use martensite_host::dev_channel::AuditPaintParams;
///
/// let params = AuditPaintParams { node_id: Some(2) };
/// assert_eq!(params.node_id, Some(2));
/// ```
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Default)]
pub struct AuditPaintParams {
    /// Optional widget node scoping the audit to a subtree (accepts a JSON
    /// number or a decimal string).
    #[serde(default, deserialize_with = "deserialize_opt_u64_wire")]
    pub node_id: Option<u64>,
}

/// Parameters for the `ReloadStatus` request (`martensite_reload_status`, MCP spec §3.21).
///
/// Carries no fields: the request reports the live hot-reload cdylib
/// lifecycle — active build id, last reload timestamp, total reloads,
/// compiler diagnostics, and state-preservation status.
///
/// # Examples
///
/// ```
/// use martensite_host::dev_channel::ReloadStatusParams;
///
/// let params = ReloadStatusParams::default();
/// assert_eq!(serde_json::to_value(&params).unwrap(), serde_json::json!({}));
/// ```
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Default)]
pub struct ReloadStatusParams {}

/// Parameters for the `SignalSet` request (`martensite_set_signal`).
///
/// Writes a JSON value into a registered reactive signal adapter. Signals
/// are only mutable when the app registered a
/// `SignalAdapter` for the id — arbitrary type-erased signals are
/// inspectable but not writable.
///
/// # Examples
///
/// ```
/// use martensite_host::dev_channel::SignalSetParams;
///
/// let params = SignalSetParams {
///     signal_id: "counter".to_string(),
///     value: serde_json::json!(42),
/// };
/// assert_eq!(params.signal_id, "counter");
/// ```
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Default)]
pub struct SignalSetParams {
    /// Registered adapter name or numeric signal id.
    #[serde(default)]
    pub signal_id: String,
    /// JSON value to assign; validated by the adapter.
    #[serde(default)]
    pub value: serde_json::Value,
}

/// Parameters for the `RuntimeErrors` request (`martensite_runtime_errors`).
///
/// Returns structured runtime error records: captured panics
/// (`install_dev_panic_hook`), active `ErrorSurface` diagnostics, and
/// reactive-runtime evaluation errors.
///
/// # Examples
///
/// ```
/// use martensite_host::dev_channel::RuntimeErrorsParams;
///
/// let params = RuntimeErrorsParams {
///     limit: Some(10),
///     severity: Some("error".to_string()),
///     clear: false,
/// };
/// assert_eq!(params.limit, Some(10));
/// ```
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Default)]
pub struct RuntimeErrorsParams {
    /// Maximum records to return (default `50`, max `500`).
    #[serde(default)]
    pub limit: Option<usize>,
    /// Severity filter: `"info"`, `"warning"`, `"error"`, `"critical"`,
    /// `"fatal"`.
    #[serde(default)]
    pub severity: Option<String>,
    /// When `true`, clears the captured panic/error state after reading.
    #[serde(default)]
    pub clear: bool,
}

/// Parameters for the `Logs` request (`martensite_logs`).
///
/// Reads the session's bounded tracing ring buffer. The app installs the
/// `LogRing` layer once at startup; this request drains the tail with
/// level/target filters.
///
/// # Examples
///
/// ```
/// use martensite_host::dev_channel::LogsParams;
///
/// let params = LogsParams {
///     limit: Some(20),
///     level: Some("warn".to_string()),
///     target_prefix: Some("martensite".to_string()),
///     contains: None,
/// };
/// assert_eq!(params.limit, Some(20));
/// ```
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Default)]
pub struct LogsParams {
    /// Maximum log records to return (default `100`, max `1000`).
    #[serde(default)]
    pub limit: Option<usize>,
    /// Minimum severity: `"trace"`/`"debug"`/`"info"`/`"warn"`/`"error"`.
    #[serde(default)]
    pub level: Option<String>,
    /// Only records whose `target` starts with this prefix.
    #[serde(default)]
    pub target_prefix: Option<String>,
    /// Case-insensitive substring filter on the formatted message.
    #[serde(default)]
    pub contains: Option<String>,
}

/// Parameters for the `A11yAction` request (`martensite_invoke_accessibility_action`).
///
/// Dispatches a [`martensite_core::SemanticAction`] to a widget node —
/// the same event the AccessKit platform adapter emits for assistive
/// technology actions.
///
/// # Examples
///
/// ```
/// use martensite_host::dev_channel::A11yActionParams;
///
/// let params = A11yActionParams {
///     node_id: Some("7".to_string()),
///     action: "click".to_string(),
///     value: None,
///     point: None,
/// };
/// assert_eq!(params.action, "click");
/// ```
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Default)]
pub struct A11yActionParams {
    /// Target widget id (JSON number or decimal string). When absent the
    /// session dispatches to the currently focused node.
    #[serde(default)]
    pub node_id: Option<String>,
    /// `SemanticAction` name, snake_case: `click`, `focus`, `blur`,
    /// `set_value`, `increment`, `decrement`, `expand`, `collapse`,
    /// `show_tooltip`, `hide_tooltip`, `show_context_menu`, `scroll_up`,
    /// `scroll_down`, `scroll_left`, `scroll_right`, `scroll_into_view`,
    /// `scroll_to_point`, `set_scroll_offset`.
    #[serde(default)]
    pub action: String,
    /// Payload for `set_value` (string) — ignored for other actions.
    #[serde(default)]
    pub value: Option<String>,
    /// `[x, y]` logical point for `scroll_to_point` / `set_scroll_offset`.
    #[serde(default)]
    pub point: Option<[f32; 2]>,
}

/// Parameters for the `HotReload` request (`martensite_hot_reload`).
///
/// Asks the running app's dev coordinator to rebuild and reload the guest
/// code. The session drops a reload-request marker next to its socket;
/// `cargo martensite dev` polls for markers and runs a reload cycle.
///
/// # Examples
///
/// ```
/// use martensite_host::dev_channel::HotReloadParams;
///
/// let params = HotReloadParams { reason: Some("fix button".to_string()) };
/// assert!(params.reason.is_some());
/// ```
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Default)]
pub struct HotReloadParams {
    /// Free-form reason recorded in the reload-request marker.
    #[serde(default)]
    pub reason: Option<String>,
}

/// Parameters for the `NodeSetLoading` request (`martensite_set_loading`,
/// ADR-0040 phase 3).
///
/// Forces or clears the instance-level `NodeFlags::LOADING` override on a
/// widget node via `WidgetArena::set_loading` — the arena-side half of the
/// loading protocol that skeletonizes *any* node, including widgets that
/// never implemented `Widget::is_loading`.
///
/// # Examples
///
/// ```
/// use martensite_host::dev_channel::NodeSetLoadingParams;
///
/// let params = NodeSetLoadingParams {
///     node_id: 7,
///     loading: true,
/// };
/// assert!(params.loading);
///
/// // `node` is an accepted alias; ids may arrive as decimal strings.
/// let parsed: NodeSetLoadingParams =
///     serde_json::from_str(r#"{ "node": "7", "loading": true }"#).unwrap();
/// assert_eq!(parsed, params);
/// ```
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Default)]
pub struct NodeSetLoadingParams {
    /// Target widget id (JSON number or decimal string); the `node` key
    /// is an accepted alias.
    #[serde(default, alias = "node", deserialize_with = "deserialize_u64_wire")]
    pub node_id: u64,
    /// New loading state: `true` forces the skeleton placeholder, `false`
    /// clears the override (a widget's own `is_loading` declaration still
    /// applies). Required — its absence fails deserialization.
    pub loading: bool,
}

/// Read/write classification of a dev-channel RPC method.
///
/// Used by authenticated forwarders (ADR-0042) that expose the channel
/// beyond the raw user-only socket to offer a read-only default mode:
/// only [`MethodClass::Read`] methods are forwarded unless the operator
/// explicitly opts in to mutation.
///
/// # Examples
///
/// ```
/// use martensite_host::dev_channel::MethodClass;
///
/// assert_eq!(MethodClass::Read, MethodClass::Read);
/// assert_ne!(MethodClass::Read, MethodClass::Mutate);
/// ```
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum MethodClass {
    /// The method only observes session state. It cannot mutate the widget
    /// tree, reactive signals, theme, inspector state, event routing, source
    /// files, or the filesystem.
    Read,
    /// The method can mutate app/session state or external resources:
    /// property tweaks, signal writes, dispatched events, accessibility
    /// actions, theme changes, source sync, artifact files, marker files,
    /// inspector arming, or clearing captured diagnostics.
    Mutate,
}

/// Classification table over the normalized method names recognized by
/// `process_request_line` (lowercase, with `_`, `-`, and spaces stripped).
///
/// Fail-closed rule: any method absent from this table — including every
/// `handle_custom` extension point — classifies as [`MethodClass::Mutate`].
const METHOD_CLASSES: &[(&str, MethodClass)] = &[
    ("hello", MethodClass::Read),
    ("treesnapshot", MethodClass::Read),
    ("lintpull", MethodClass::Read),
    ("lintscene", MethodClass::Read),
    ("eventledger", MethodClass::Read),
    // `lint_apply` evaluates fix ops against a *copy* of the scene model;
    // the live arena is never touched (ADR-0038).
    ("lintapply", MethodClass::Read),
    ("treenode", MethodClass::Read),
    ("layoutchain", MethodClass::Read),
    ("overflowscan", MethodClass::Read),
    ("signalslist", MethodClass::Read),
    ("a11ytree", MethodClass::Read),
    ("tweakslist", MethodClass::Read),
    ("themeget", MethodClass::Read),
    ("auditpaint", MethodClass::Read),
    ("reloadstatus", MethodClass::Read),
    ("logs", MethodClass::Read),
    ("inspectorselect", MethodClass::Mutate),
    ("signaltrigger", MethodClass::Mutate),
    ("tweakset", MethodClass::Mutate),
    ("themeset", MethodClass::Mutate),
    ("tweakssync", MethodClass::Mutate),
    ("eventdispatch", MethodClass::Mutate),
    ("timemachinestep", MethodClass::Mutate),
    ("capturenode", MethodClass::Mutate),
    ("signalset", MethodClass::Mutate),
    // `runtime_errors` accepts `clear: true`, draining captured panic state.
    ("runtimeerrors", MethodClass::Mutate),
    ("a11yaction", MethodClass::Mutate),
    // `hot_reload` drops a `<socket>.reload-request` marker file the dev
    // coordinator polls for.
    ("hotreload", MethodClass::Mutate),
    ("nodesetloading", MethodClass::Mutate),
];

/// Classifies a dev-channel RPC method name as [`MethodClass::Read`] or
/// [`MethodClass::Mutate`].
///
/// `method` is normalized exactly like the request dispatcher —
/// lowercased with `_`, `-`, and space removed — so `"TreeSnapshot"`,
/// `"tree_snapshot"`, and `"tree snapshot"` classify identically.
/// Unknown methods classify as [`MethodClass::Mutate`] (fail closed).
///
/// # Examples
///
/// ```
/// use martensite_host::dev_channel::{method_class, MethodClass};
///
/// assert_eq!(method_class("TreeSnapshot"), MethodClass::Read);
/// assert_eq!(method_class("tree_snapshot"), MethodClass::Read);
/// assert_eq!(method_class("ThemeSet"), MethodClass::Mutate);
/// // Unknown/extension methods fail closed.
/// assert_eq!(method_class("ExecuteArbitraryCode"), MethodClass::Mutate);
/// ```
pub fn method_class(method: &str) -> MethodClass {
    let norm = method.to_ascii_lowercase().replace(['_', '-', ' '], "");
    METHOD_CLASSES
        .iter()
        .find(|(name, _)| *name == norm)
        .map(|(_, class)| *class)
        .unwrap_or(MethodClass::Mutate)
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
    /// let res = handler
    ///     .handle_inspector_select(InspectorSelectParams {
    ///         arm: true,
    ///         ..Default::default()
    ///     })
    ///     .unwrap();
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

    /// Handles a `TreeNode` request (`martensite_get_node`, MCP spec §3.2).
    ///
    /// Returns the complete record for a single widget node: generational
    /// index, kind, computed bounds, markers, and AccessKit properties.
    ///
    /// The default implementation returns `Err("not_implemented:tree_node")`,
    /// which the dispatcher maps to [`ERR_METHOD_NOT_FOUND`].
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite_host::dev_channel::{DevChannelHandler, DefaultDevChannelHandler, TreeNodeParams};
    ///
    /// let handler = DefaultDevChannelHandler;
    /// let err = handler.handle_tree_node(TreeNodeParams { node_id: 7 }).unwrap_err();
    /// assert_eq!(err, "not_implemented:tree_node");
    /// ```
    fn handle_tree_node(&self, params: TreeNodeParams) -> Result<serde_json::Value, String> {
        let _ = params;
        Err(not_implemented_err("tree_node"))
    }

    /// Handles a `LayoutChain` request (`martensite_diagnose_layout`, MCP spec §3.3).
    ///
    /// Returns the exact Taffy layout chain and constraint resolution history
    /// for a widget: inbound constraints, style definitions, intrinsic
    /// measure output, and resolved bounds.
    ///
    /// The default implementation returns `Err("not_implemented:layout_chain")`,
    /// which the dispatcher maps to [`ERR_METHOD_NOT_FOUND`].
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite_host::dev_channel::{DevChannelHandler, DefaultDevChannelHandler, LayoutChainParams};
    ///
    /// let handler = DefaultDevChannelHandler;
    /// let err = handler
    ///     .handle_layout_chain(LayoutChainParams { node_id: 3 })
    ///     .unwrap_err();
    /// assert_eq!(err, "not_implemented:layout_chain");
    /// ```
    fn handle_layout_chain(&self, params: LayoutChainParams) -> Result<serde_json::Value, String> {
        let _ = params;
        Err(not_implemented_err("layout_chain"))
    }

    /// Handles an `OverflowScan` request (`martensite_explain_overflow`, MCP spec §3.4).
    ///
    /// Pinpoints the mathematical cause and clipping lineage of a layout
    /// overflow, or scans the entire scene when `node_id` is `None`.
    ///
    /// The default implementation returns `Err("not_implemented:overflow_scan")`,
    /// which the dispatcher maps to [`ERR_METHOD_NOT_FOUND`].
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite_host::dev_channel::{DevChannelHandler, DefaultDevChannelHandler, OverflowScanParams};
    ///
    /// let handler = DefaultDevChannelHandler;
    /// let err = handler
    ///     .handle_overflow_scan(OverflowScanParams::default())
    ///     .unwrap_err();
    /// assert_eq!(err, "not_implemented:overflow_scan");
    /// ```
    fn handle_overflow_scan(
        &self,
        params: OverflowScanParams,
    ) -> Result<serde_json::Value, String> {
        let _ = params;
        Err(not_implemented_err("overflow_scan"))
    }

    /// Handles a `SignalsList` request (`martensite_inspect_signals`, MCP spec §3.5).
    ///
    /// Introspects the push-pull reactive signal DAG with token-safe
    /// pagination and optional dirty-only filtering.
    ///
    /// The default implementation returns `Err("not_implemented:signals_list")`,
    /// which the dispatcher maps to [`ERR_METHOD_NOT_FOUND`].
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite_host::dev_channel::{DevChannelHandler, DefaultDevChannelHandler, SignalsListParams};
    ///
    /// let handler = DefaultDevChannelHandler;
    /// let err = handler
    ///     .handle_signals_list(SignalsListParams::default())
    ///     .unwrap_err();
    /// assert_eq!(err, "not_implemented:signals_list");
    /// ```
    fn handle_signals_list(&self, params: SignalsListParams) -> Result<serde_json::Value, String> {
        let _ = params;
        Err(not_implemented_err("signals_list"))
    }

    /// Handles a `SignalTrigger` request (`martensite_trigger_signal`, MCP spec §3.6).
    ///
    /// Mutates a registered tweakable reactive signal value in dev mode;
    /// deserialization is type-checked against the registered schema.
    ///
    /// The default implementation returns `Err("not_implemented:signal_trigger")`,
    /// which the dispatcher maps to [`ERR_METHOD_NOT_FOUND`].
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite_host::dev_channel::{DevChannelHandler, DefaultDevChannelHandler, SignalTriggerParams};
    ///
    /// let handler = DefaultDevChannelHandler;
    /// let err = handler
    ///     .handle_signal_trigger(SignalTriggerParams {
    ///         signal_id: "counter".to_string(),
    ///         value: serde_json::json!(1),
    ///     })
    ///     .unwrap_err();
    /// assert_eq!(err, "not_implemented:signal_trigger");
    /// ```
    fn handle_signal_trigger(
        &self,
        params: SignalTriggerParams,
    ) -> Result<serde_json::Value, String> {
        let _ = params;
        Err(not_implemented_err("signal_trigger"))
    }

    /// Handles an `A11yTree` request (`martensite_inspect_a11y_tree`, MCP spec §3.7).
    ///
    /// Evaluates the hierarchical AccessKit accessibility tree mirror
    /// independently of the widget layout hierarchy.
    ///
    /// The default implementation returns `Err("not_implemented:a11y_tree")`,
    /// which the dispatcher maps to [`ERR_METHOD_NOT_FOUND`].
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite_host::dev_channel::{DevChannelHandler, DefaultDevChannelHandler, A11yTreeParams};
    ///
    /// let handler = DefaultDevChannelHandler;
    /// let err = handler
    ///     .handle_a11y_tree(A11yTreeParams::default())
    ///     .unwrap_err();
    /// assert_eq!(err, "not_implemented:a11y_tree");
    /// ```
    fn handle_a11y_tree(&self, params: A11yTreeParams) -> Result<serde_json::Value, String> {
        let _ = params;
        Err(not_implemented_err("a11y_tree"))
    }

    /// Handles a `TweaksList` request (`martensite_list_tweaks`, MCP spec §3.11).
    ///
    /// Enumerates all active runtime tweakable parameters registered via
    /// `#[tweak]` or `TweakRegistry`.
    ///
    /// The default implementation returns `Err("not_implemented:tweaks_list")`,
    /// which the dispatcher maps to [`ERR_METHOD_NOT_FOUND`].
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite_host::dev_channel::{DevChannelHandler, DefaultDevChannelHandler, TweaksListParams};
    ///
    /// let handler = DefaultDevChannelHandler;
    /// let err = handler
    ///     .handle_tweaks_list(TweaksListParams::default())
    ///     .unwrap_err();
    /// assert_eq!(err, "not_implemented:tweaks_list");
    /// ```
    fn handle_tweaks_list(&self, params: TweaksListParams) -> Result<serde_json::Value, String> {
        let _ = params;
        Err(not_implemented_err("tweaks_list"))
    }

    /// Handles a `TweakSet` request (`martensite_set_tweak`, MCP spec §3.12).
    ///
    /// Updates a tweak parameter in the running application in real time.
    ///
    /// The default implementation returns `Err("not_implemented:tweak_set")`,
    /// which the dispatcher maps to [`ERR_METHOD_NOT_FOUND`].
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite_host::dev_channel::{DevChannelHandler, DefaultDevChannelHandler, TweakSetParams};
    ///
    /// let handler = DefaultDevChannelHandler;
    /// let err = handler
    ///     .handle_tweak_set(TweakSetParams {
    ///         name: "card_gap".to_string(),
    ///         value: serde_json::json!(8.0),
    ///     })
    ///     .unwrap_err();
    /// assert_eq!(err, "not_implemented:tweak_set");
    /// ```
    fn handle_tweak_set(&self, params: TweakSetParams) -> Result<serde_json::Value, String> {
        let _ = params;
        Err(not_implemented_err("tweak_set"))
    }

    /// Handles a `ThemeSet` request (`martensite_set_theme`, MCP spec §3.13).
    ///
    /// Toggles the dark/light theme mode or applies dynamic design token
    /// palette overrides.
    ///
    /// The default implementation returns `Err("not_implemented:theme_set")`,
    /// which the dispatcher maps to [`ERR_METHOD_NOT_FOUND`].
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite_host::dev_channel::{DevChannelHandler, DefaultDevChannelHandler, ThemeSetParams};
    ///
    /// let handler = DefaultDevChannelHandler;
    /// let err = handler
    ///     .handle_theme_set(ThemeSetParams::default())
    ///     .unwrap_err();
    /// assert_eq!(err, "not_implemented:theme_set");
    /// ```
    fn handle_theme_set(&self, params: ThemeSetParams) -> Result<serde_json::Value, String> {
        let _ = params;
        Err(not_implemented_err("theme_set"))
    }

    /// Handles a `ThemeGet` request (theme token resource read).
    ///
    /// Returns the current theme mode and active design token values.
    ///
    /// The default implementation returns `Err("not_implemented:theme_get")`,
    /// which the dispatcher maps to [`ERR_METHOD_NOT_FOUND`].
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite_host::dev_channel::{DevChannelHandler, DefaultDevChannelHandler, ThemeGetParams};
    ///
    /// let handler = DefaultDevChannelHandler;
    /// let err = handler
    ///     .handle_theme_get(ThemeGetParams::default())
    ///     .unwrap_err();
    /// assert_eq!(err, "not_implemented:theme_get");
    /// ```
    fn handle_theme_get(&self, params: ThemeGetParams) -> Result<serde_json::Value, String> {
        let _ = params;
        Err(not_implemented_err("theme_get"))
    }

    /// Handles a `TweaksSync` request (`martensite_sync_tweaks_to_source`, MCP spec §3.14).
    ///
    /// Commits converged live tweak values back to Rust source files under
    /// strict confirmation gates and workspace confinement.
    ///
    /// The default implementation returns `Err("not_implemented:tweaks_sync")`,
    /// which the dispatcher maps to [`ERR_METHOD_NOT_FOUND`].
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite_host::dev_channel::{DevChannelHandler, DefaultDevChannelHandler, TweaksSyncParams};
    ///
    /// let handler = DefaultDevChannelHandler;
    /// let err = handler
    ///     .handle_tweaks_sync(TweaksSyncParams::default())
    ///     .unwrap_err();
    /// assert_eq!(err, "not_implemented:tweaks_sync");
    /// ```
    fn handle_tweaks_sync(&self, params: TweaksSyncParams) -> Result<serde_json::Value, String> {
        let _ = params;
        Err(not_implemented_err("tweaks_sync"))
    }

    /// Handles an `EventDispatch` request (`martensite_dispatch_event`, MCP spec §3.15).
    ///
    /// Injects a synthetic input event into the running application's
    /// `EventRouter` and reports the routing outcome and hit-test path.
    ///
    /// The default implementation returns `Err("not_implemented:event_dispatch")`,
    /// which the dispatcher maps to [`ERR_METHOD_NOT_FOUND`].
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite_host::dev_channel::{DevChannelHandler, DefaultDevChannelHandler, EventDispatchParams};
    ///
    /// let handler = DefaultDevChannelHandler;
    /// let err = handler
    ///     .handle_event_dispatch(EventDispatchParams {
    ///         event_type: "key_press".to_string(),
    ///         ..EventDispatchParams::default()
    ///     })
    ///     .unwrap_err();
    /// assert_eq!(err, "not_implemented:event_dispatch");
    /// ```
    fn handle_event_dispatch(
        &self,
        params: EventDispatchParams,
    ) -> Result<serde_json::Value, String> {
        let _ = params;
        Err(not_implemented_err("event_dispatch"))
    }

    /// Handles a `TimemachineStep` request (`martensite_step_timemachine`, MCP spec §3.17).
    ///
    /// Controls the deterministic TimeMachine debugger (pause, resume, step,
    /// restore checkpoint).
    ///
    /// The default implementation returns `Err("not_implemented:timemachine_step")`,
    /// which the dispatcher maps to [`ERR_METHOD_NOT_FOUND`].
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite_host::dev_channel::{DevChannelHandler, DefaultDevChannelHandler, TimemachineStepParams};
    ///
    /// let handler = DefaultDevChannelHandler;
    /// let err = handler
    ///     .handle_timemachine_step(TimemachineStepParams {
    ///         action: "pause".to_string(),
    ///         checkpoint_id: None,
    ///     })
    ///     .unwrap_err();
    /// assert_eq!(err, "not_implemented:timemachine_step");
    /// ```
    fn handle_timemachine_step(
        &self,
        params: TimemachineStepParams,
    ) -> Result<serde_json::Value, String> {
        let _ = params;
        Err(not_implemented_err("timemachine_step"))
    }

    /// Handles a `CaptureNode` request (`martensite_capture_node`, MCP spec §3.18).
    ///
    /// Renders an individual widget subtree to an image artifact without
    /// desktop window chrome.
    ///
    /// The default implementation returns `Err("not_implemented:capture_node")`,
    /// which the dispatcher maps to [`ERR_METHOD_NOT_FOUND`].
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite_host::dev_channel::{DevChannelHandler, DefaultDevChannelHandler, CaptureNodeParams};
    ///
    /// let handler = DefaultDevChannelHandler;
    /// let err = handler
    ///     .handle_capture_node(CaptureNodeParams {
    ///         node_id: 4,
    ///         format: Some("png".to_string()),
    ///         scale: None,
    ///     })
    ///     .unwrap_err();
    /// assert_eq!(err, "not_implemented:capture_node");
    /// ```
    fn handle_capture_node(&self, params: CaptureNodeParams) -> Result<serde_json::Value, String> {
        let _ = params;
        Err(not_implemented_err("capture_node"))
    }

    /// Handles an `AuditPaint` request (`martensite_audit_paint`, MCP spec §3.10).
    ///
    /// Performs a deep visual audit on text rendering, WCAG contrast, and
    /// non-text keylines for the scene or a subtree.
    ///
    /// The default implementation returns `Err("not_implemented:audit_paint")`,
    /// which the dispatcher maps to [`ERR_METHOD_NOT_FOUND`].
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite_host::dev_channel::{DevChannelHandler, DefaultDevChannelHandler, AuditPaintParams};
    ///
    /// let handler = DefaultDevChannelHandler;
    /// let err = handler
    ///     .handle_audit_paint(AuditPaintParams::default())
    ///     .unwrap_err();
    /// assert_eq!(err, "not_implemented:audit_paint");
    /// ```
    fn handle_audit_paint(&self, params: AuditPaintParams) -> Result<serde_json::Value, String> {
        let _ = params;
        Err(not_implemented_err("audit_paint"))
    }

    /// Handles a `ReloadStatus` request (`martensite_reload_status`, MCP spec §3.21).
    ///
    /// Reports the live hot-reload cdylib lifecycle: active build id, last
    /// reload timestamp, total reloads, compiler diagnostics, and
    /// state-preservation status.
    ///
    /// The default implementation returns `Err("not_implemented:reload_status")`,
    /// which the dispatcher maps to [`ERR_METHOD_NOT_FOUND`].
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite_host::dev_channel::{DevChannelHandler, DefaultDevChannelHandler, ReloadStatusParams};
    ///
    /// let handler = DefaultDevChannelHandler;
    /// let err = handler
    ///     .handle_reload_status(ReloadStatusParams::default())
    ///     .unwrap_err();
    /// assert_eq!(err, "not_implemented:reload_status");
    /// ```
    fn handle_reload_status(
        &self,
        params: ReloadStatusParams,
    ) -> Result<serde_json::Value, String> {
        let _ = params;
        Err(not_implemented_err("reload_status"))
    }

    /// Handles a `SignalSet` request (`martensite_set_signal`).
    ///
    /// Writes a JSON value into a registered reactive signal adapter. The
    /// default implementation returns `Err("not_implemented:signal_set")`,
    /// which the dispatcher maps to [`ERR_METHOD_NOT_FOUND`].
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite_host::dev_channel::{DevChannelHandler, DefaultDevChannelHandler, SignalSetParams};
    ///
    /// let handler = DefaultDevChannelHandler;
    /// let err = handler
    ///     .handle_signal_set(SignalSetParams::default())
    ///     .unwrap_err();
    /// assert_eq!(err, "not_implemented:signal_set");
    /// ```
    fn handle_signal_set(&self, params: SignalSetParams) -> Result<serde_json::Value, String> {
        let _ = params;
        Err(not_implemented_err("signal_set"))
    }

    /// Handles a `RuntimeErrors` request (`martensite_runtime_errors`).
    ///
    /// Returns structured runtime error records (captured panics, active
    /// error-surface diagnostics, reactive evaluation errors). The default
    /// implementation returns `Err("not_implemented:runtime_errors")`.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite_host::dev_channel::{DevChannelHandler, DefaultDevChannelHandler, RuntimeErrorsParams};
    ///
    /// let handler = DefaultDevChannelHandler;
    /// let err = handler
    ///     .handle_runtime_errors(RuntimeErrorsParams::default())
    ///     .unwrap_err();
    /// assert_eq!(err, "not_implemented:runtime_errors");
    /// ```
    fn handle_runtime_errors(
        &self,
        params: RuntimeErrorsParams,
    ) -> Result<serde_json::Value, String> {
        let _ = params;
        Err(not_implemented_err("runtime_errors"))
    }

    /// Handles a `Logs` request (`martensite_logs`).
    ///
    /// Drains the session's tracing ring buffer with level/target filters.
    /// The default implementation returns `Err("not_implemented:logs")`.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite_host::dev_channel::{DevChannelHandler, DefaultDevChannelHandler, LogsParams};
    ///
    /// let handler = DefaultDevChannelHandler;
    /// let err = handler.handle_logs(LogsParams::default()).unwrap_err();
    /// assert_eq!(err, "not_implemented:logs");
    /// ```
    fn handle_logs(&self, params: LogsParams) -> Result<serde_json::Value, String> {
        let _ = params;
        Err(not_implemented_err("logs"))
    }

    /// Handles an `A11yAction` request (`martensite_invoke_accessibility_action`).
    ///
    /// Dispatches a `SemanticAction` to the target (or focused) widget.
    /// The default implementation returns
    /// `Err("not_implemented:a11y_action")`.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite_host::dev_channel::{DevChannelHandler, DefaultDevChannelHandler, A11yActionParams};
    ///
    /// let handler = DefaultDevChannelHandler;
    /// let err = handler
    ///     .handle_a11y_action(A11yActionParams::default())
    ///     .unwrap_err();
    /// assert_eq!(err, "not_implemented:a11y_action");
    /// ```
    fn handle_a11y_action(&self, params: A11yActionParams) -> Result<serde_json::Value, String> {
        let _ = params;
        Err(not_implemented_err("a11y_action"))
    }

    /// Handles a `HotReload` request (`martensite_hot_reload`).
    ///
    /// Requests a guest-code rebuild+reload from the dev coordinator. The
    /// default implementation returns `Err("not_implemented:hot_reload")`.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite_host::dev_channel::{DevChannelHandler, DefaultDevChannelHandler, HotReloadParams};
    ///
    /// let handler = DefaultDevChannelHandler;
    /// let err = handler
    ///     .handle_hot_reload(HotReloadParams::default())
    ///     .unwrap_err();
    /// assert_eq!(err, "not_implemented:hot_reload");
    /// ```
    fn handle_hot_reload(&self, params: HotReloadParams) -> Result<serde_json::Value, String> {
        let _ = params;
        Err(not_implemented_err("hot_reload"))
    }

    /// Handles a `NodeSetLoading` request (`martensite_set_loading`,
    /// ADR-0040 phase 3).
    ///
    /// Forces or clears the `NodeFlags::LOADING` override on a widget node
    /// so agents can drive the skeleton-placeholder path and verify it
    /// through `a11y_tree` (`busy`) and `capture_node`. The default
    /// implementation returns `Err("not_implemented:node_set_loading")`,
    /// which the dispatcher maps to [`ERR_METHOD_NOT_FOUND`].
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite_host::dev_channel::{DevChannelHandler, DefaultDevChannelHandler, NodeSetLoadingParams};
    ///
    /// let handler = DefaultDevChannelHandler;
    /// let err = handler
    ///     .handle_node_set_loading(NodeSetLoadingParams {
    ///         node_id: 4,
    ///         loading: true,
    ///     })
    ///     .unwrap_err();
    /// assert_eq!(err, "not_implemented:node_set_loading");
    /// ```
    fn handle_node_set_loading(
        &self,
        params: NodeSetLoadingParams,
    ) -> Result<serde_json::Value, String> {
        let _ = params;
        Err(not_implemented_err("node_set_loading"))
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

/// Computes the default Unix domain socket or Windows Named Pipe path for a dev session.
///
/// Follows ADR-0038 path resolution:
/// - On Windows: `\\.\pipe\martensite-<build_id>`
/// - On Unix: If `$XDG_RUNTIME_DIR` is set and non-empty: `$XDG_RUNTIME_DIR/martensite/<build_id>.sock`
/// - Otherwise on Unix: `/tmp/martensite-<user>/<build_id>.sock`, where `<user>` is determined from
///   the `USER` or `LOGNAME` environment variables (defaulting to `"user"`).
///
/// # Examples
///
/// ```
/// use martensite_host::dev_channel::socket_path_for_session;
///
/// let path = socket_path_for_session("test_session");
/// assert!(path.to_string_lossy().contains("test_session"));
/// ```
#[cfg(unix)]
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

/// Computes the default Unix domain socket or Windows Named Pipe path for a dev session.
///
/// Follows ADR-0038 path resolution:
/// - On Windows: `\\.\pipe\martensite-<build_id>`
/// - On Unix: If `$XDG_RUNTIME_DIR` is set and non-empty: `$XDG_RUNTIME_DIR/martensite/<build_id>.sock`
/// - Otherwise on Unix: `/tmp/martensite-<user>/<build_id>.sock`, where `<user>` is determined from
///   the `USER` or `LOGNAME` environment variables (defaulting to `"user"`).
///
/// # Examples
///
/// ```
/// use martensite_host::dev_channel::socket_path_for_session;
///
/// let path = socket_path_for_session("test_session");
/// assert!(path.to_string_lossy().contains("test_session"));
/// ```
#[cfg(windows)]
pub fn socket_path_for_session(build_id: &str) -> PathBuf {
    PathBuf::from(format!(r"\\.\pipe\martensite-{build_id}"))
}

#[cfg(unix)]
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

/// Cross-platform communication stream abstraction for the Dev Channel.
///
/// Wraps a Unix domain socket on Unix platforms, and a Windows Named Pipe on Windows.
///
/// # Examples
///
/// ```no_run
/// use martensite_host::dev_channel::DevStream;
/// use std::path::Path;
///
/// # #[cfg(unix)]
/// let _stream = DevStream::connect(Path::new("/tmp/test.sock"));
/// # #[cfg(windows)]
/// let _stream = DevStream::connect(Path::new(r"\\.\pipe\test_pipe"));
/// ```
pub struct DevStream {
    #[cfg(unix)]
    inner: UnixStream,
    #[cfg(windows)]
    inner: std::fs::File,
}

impl fmt::Debug for DevStream {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("DevStream").finish_non_exhaustive()
    }
}

impl io::Read for DevStream {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        self.inner.read(buf)
    }
}

impl io::Write for DevStream {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        self.inner.write(buf)
    }

    fn flush(&mut self) -> io::Result<()> {
        self.inner.flush()
    }
}

impl DevStream {
    /// Attempts to clone the underlying stream handle.
    ///
    /// # Examples
    ///
    /// ```no_run
    /// use martensite_host::dev_channel::DevStream;
    /// use std::path::Path;
    ///
    /// # #[cfg(unix)]
    /// {
    ///     let stream = DevStream::connect(Path::new("/tmp/test.sock")).unwrap();
    ///     let _cloned = stream.try_clone().unwrap();
    /// }
    /// # #[cfg(windows)]
    /// {
    ///     let stream = DevStream::connect(Path::new(r"\\.\pipe\test_pipe")).unwrap();
    ///     let _cloned = stream.try_clone().unwrap();
    /// }
    /// ```
    pub fn try_clone(&self) -> io::Result<Self> {
        Ok(Self {
            inner: self.inner.try_clone()?,
        })
    }

    /// Connects to a dev channel server at the given path or named pipe.
    ///
    /// # Examples
    ///
    /// ```no_run
    /// use martensite_host::dev_channel::DevStream;
    /// use std::path::Path;
    ///
    /// # #[cfg(unix)]
    /// let _stream = DevStream::connect(Path::new("/tmp/test.sock"));
    /// # #[cfg(windows)]
    /// let _stream = DevStream::connect(Path::new(r"\\.\pipe\test_pipe"));
    /// ```
    pub fn connect(path: impl AsRef<Path>) -> io::Result<Self> {
        let path = path.as_ref();
        #[cfg(unix)]
        {
            let stream = UnixStream::connect(path)?;
            Ok(Self { inner: stream })
        }
        #[cfg(windows)]
        {
            let pipe_path = resolve_windows_connect_path(path)?;
            let file = connect_named_pipe_with_retry(&pipe_path)?;
            Ok(Self { inner: file })
        }
    }
}

/// Cross-platform listener abstraction for the Dev Channel.
///
/// Wraps a Unix domain socket listener on Unix platforms, and a Windows Named Pipe listener on Windows.
///
/// # Examples
///
/// ```no_run
/// use martensite_host::dev_channel::DevListener;
/// use std::path::Path;
///
/// # #[cfg(unix)]
/// let _listener = DevListener::bind(Path::new("/tmp/test.sock"));
/// # #[cfg(windows)]
/// let _listener = DevListener::bind(Path::new(r"\\.\pipe\test_pipe"));
/// ```
pub struct DevListener {
    #[cfg(unix)]
    inner: UnixListener,
    #[cfg(windows)]
    pipe_name_wide: Vec<u16>,
    #[cfg(windows)]
    #[allow(dead_code)]
    pipe_path: PathBuf,
    #[cfg(windows)]
    file_path: Option<PathBuf>,
}

impl fmt::Debug for DevListener {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("DevListener").finish_non_exhaustive()
    }
}

impl DevListener {
    /// Binds a new dev channel listener to the given path or named pipe.
    ///
    /// # Examples
    ///
    /// ```no_run
    /// use martensite_host::dev_channel::DevListener;
    /// use std::path::Path;
    ///
    /// # #[cfg(unix)]
    /// let _listener = DevListener::bind(Path::new("/tmp/test.sock")).unwrap();
    /// # #[cfg(windows)]
    /// let _listener = DevListener::bind(Path::new(r"\\.\pipe\test_pipe")).unwrap();
    /// ```
    pub fn bind(path: impl AsRef<Path>) -> io::Result<Self> {
        let path = path.as_ref();
        #[cfg(unix)]
        {
            prepare_socket_parent_dir(path)?;
            if path.exists() {
                let _ = std::fs::remove_file(path);
            }
            let inner = UnixListener::bind(path)?;
            use std::os::unix::fs::PermissionsExt;
            let _ = std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o600));
            Ok(Self { inner })
        }
        #[cfg(windows)]
        {
            let path_str = path.to_string_lossy();
            let is_pipe = path_str.starts_with(r"\\.\pipe\");
            let (pipe_path, file_path) = if is_pipe {
                (path.to_path_buf(), None)
            } else {
                prepare_socket_parent_dir(path)?;
                if path.exists() {
                    let _ = std::fs::remove_file(path);
                }
                let pipe = pipe_name_from_path(path);
                std::fs::write(path, pipe.to_string_lossy().as_bytes())?;
                (pipe, Some(path.to_path_buf()))
            };

            use std::ffi::OsStr;
            use std::os::windows::ffi::OsStrExt;
            let pipe_name_wide: Vec<u16> = OsStr::new(pipe_path.as_os_str())
                .encode_wide()
                .chain(std::iter::once(0))
                .collect();

            Ok(Self {
                pipe_name_wide,
                pipe_path,
                file_path,
            })
        }
    }

    /// Accepts a new incoming client connection.
    ///
    /// # Examples
    ///
    /// ```no_run
    /// use martensite_host::dev_channel::DevListener;
    /// use std::path::Path;
    ///
    /// # #[cfg(unix)]
    /// {
    ///     let listener = DevListener::bind(Path::new("/tmp/test.sock")).unwrap();
    ///     let (_stream, _) = listener.accept().unwrap();
    /// }
    /// # #[cfg(windows)]
    /// {
    ///     let listener = DevListener::bind(Path::new(r"\\.\pipe\test_pipe")).unwrap();
    ///     let (_stream, _) = listener.accept().unwrap();
    /// }
    /// ```
    pub fn accept(&self) -> io::Result<(DevStream, ())> {
        #[cfg(unix)]
        {
            let (stream, _) = self.inner.accept()?;
            Ok((DevStream { inner: stream }, ()))
        }
        #[cfg(windows)]
        {
            use win32::*;
            let handle = unsafe {
                CreateNamedPipeW(
                    self.pipe_name_wide.as_ptr(),
                    PIPE_ACCESS_DUPLEX,
                    PIPE_TYPE_BYTE | PIPE_WAIT,
                    PIPE_UNLIMITED_INSTANCES,
                    65536,
                    65536,
                    0,
                    std::ptr::null_mut(),
                )
            };
            if handle == INVALID_HANDLE_VALUE {
                return Err(io::Error::last_os_error());
            }

            let res = unsafe { ConnectNamedPipe(handle, std::ptr::null_mut()) };
            let last_err = unsafe { GetLastError() };

            if res != 0 || last_err == ERROR_PIPE_CONNECTED {
                use std::os::windows::io::FromRawHandle;
                let file = unsafe { std::fs::File::from_raw_handle(handle as _) };
                Ok((DevStream { inner: file }, ()))
            } else {
                unsafe { CloseHandle(handle) };
                Err(io::Error::from_raw_os_error(last_err as i32))
            }
        }
    }
}

#[cfg(windows)]
impl Drop for DevListener {
    fn drop(&mut self) {
        if let Some(ref path) = self.file_path {
            let _ = std::fs::remove_file(path);
        }
    }
}

#[cfg(windows)]
fn resolve_windows_connect_path(path: &Path) -> io::Result<PathBuf> {
    let s = path.to_string_lossy();
    if s.starts_with(r"\\.\pipe\") {
        return Ok(path.to_path_buf());
    }
    if path.is_file() {
        if let Ok(content) = std::fs::read_to_string(path) {
            let trimmed = content.trim();
            if trimmed.starts_with(r"\\.\pipe\") {
                return Ok(PathBuf::from(trimmed));
            }
            if !trimmed.is_empty() {
                return Ok(PathBuf::from(format!(r"\\.\pipe\martensite-{trimmed}")));
            }
        }
    }
    Ok(pipe_name_from_path(path))
}

#[cfg(windows)]
fn pipe_name_from_path(path: &Path) -> PathBuf {
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
fn connect_named_pipe_with_retry(pipe_path: &Path) -> io::Result<std::fs::File> {
    let start = std::time::Instant::now();
    loop {
        match std::fs::OpenOptions::new()
            .read(true)
            .write(true)
            .open(pipe_path)
        {
            Ok(file) => return Ok(file),
            Err(e) => {
                let code = e.raw_os_error();
                if (code == Some(231) || code == Some(2) || e.kind() == io::ErrorKind::WouldBlock)
                    && start.elapsed() < Duration::from_secs(5)
                {
                    std::thread::sleep(Duration::from_millis(5));
                    continue;
                }
                return Err(e);
            }
        }
    }
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
    /// Bearer token required in the `Hello` handshake (ADR-0042).
    ///
    /// When `Some`, a `Hello` whose [`HelloParams::auth_token`] does not
    /// match is rejected with [`ERR_AUTH_FAILED`]. `None` (the default)
    /// preserves the unauthenticated local-socket posture — existing
    /// clients are unaffected.
    pub auth_token: Option<String>,
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
            auth_token: None,
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

    /// Requires every `Hello` handshake on this server to present `token`
    /// as [`HelloParams::auth_token`] (ADR-0042).
    ///
    /// Intended for transports that forward the channel beyond the raw
    /// user-only socket — e.g. the loopback WebSocket bridge — so the
    /// bearer survives translation. Local sessions should leave this unset.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite_host::dev_channel::DevChannelConfig;
    ///
    /// let config = DevChannelConfig::new().with_auth_token("s3cret");
    /// assert_eq!(config.auth_token.as_deref(), Some("s3cret"));
    /// ```
    pub fn with_auth_token(mut self, token: impl Into<String>) -> Self {
        self.auth_token = Some(token.into());
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

        let listener = DevListener::bind(&socket_path)?;

        let running = Arc::new(AtomicBool::new(true));
        let running_listener = Arc::clone(&running);
        let handler = config.handler;
        let build_id = config.build_id.clone();
        let auth_token = config.auth_token.clone();

        let accept_thread = std::thread::Builder::new()
            .name("martensite-dev-channel-listener".to_string())
            .spawn(move || {
                run_listener_loop(listener, handler, running_listener, build_id, auth_token);
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
            let _ = DevStream::connect(path);
        }

        if let Some(handle) = self.accept_thread.take() {
            let _ = handle.join();
        }

        #[cfg(unix)]
        if let Some(ref path) = self.socket_path {
            let _ = std::fs::remove_file(path);
        }

        #[cfg(windows)]
        if let Some(ref path) = self.socket_path {
            if !path.to_string_lossy().starts_with(r"\\.\pipe\") {
                let _ = std::fs::remove_file(path);
            }
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
    listener: DevListener,
    handler: Arc<dyn DevChannelHandler>,
    running: Arc<AtomicBool>,
    build_id: Option<String>,
    auth_token: Option<String>,
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
                let auth_token = auth_token.clone();
                let _ = std::thread::Builder::new()
                    .name("martensite-dev-channel-client".to_string())
                    .spawn(move || {
                        handle_client(stream, handler, running_child, build_id, auth_token);
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
    mut stream: DevStream,
    handler: Arc<dyn DevChannelHandler>,
    running: Arc<AtomicBool>,
    build_id: Option<String>,
    auth_token: Option<String>,
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
            auth_token.as_deref(),
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
///
/// `required_token` is the optional bearer token configured on the server
/// (ADR-0042): when `Some`, the `Hello` handshake must carry a matching
/// [`HelloParams::auth_token`] or it is rejected with [`ERR_AUTH_FAILED`].
pub(crate) fn process_request_line(
    line: &str,
    handshook: &mut bool,
    handler: &dyn DevChannelHandler,
    build_id: Option<&str>,
    required_token: Option<&str>,
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

            // Bearer-token gate (ADR-0042): when the session was configured
            // with a required token, a `Hello` that does not carry a
            // matching `auth_token` is rejected before any version data is
            // exchanged. Sessions without a configured token are unaffected.
            if let Some(expected) = required_token {
                if params.auth_token.as_deref() != Some(expected) {
                    *handshook = false;
                    return JsonRpcResponse::error(
                        req.id,
                        ERR_AUTH_FAILED,
                        "auth_failed: this dev channel session requires a bearer token",
                        Some(serde_json::json!({
                            "error_type": "auth_failed",
                        })),
                    );
                }
            }

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
                Err(msg) if msg.starts_with("not_implemented:") => {
                    JsonRpcResponse::error(req.id, ERR_METHOD_NOT_FOUND, msg, None)
                }
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
                Err(msg) if msg.starts_with("not_implemented:") => {
                    JsonRpcResponse::error(req.id, ERR_METHOD_NOT_FOUND, msg, None)
                }
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
                Err(msg) if msg.starts_with("not_implemented:") => {
                    JsonRpcResponse::error(req.id, ERR_METHOD_NOT_FOUND, msg, None)
                }
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
                Err(msg) if msg.starts_with("not_implemented:") => {
                    JsonRpcResponse::error(req.id, ERR_METHOD_NOT_FOUND, msg, None)
                }
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
                Err(msg) if msg.starts_with("not_implemented:") => {
                    JsonRpcResponse::error(req.id, ERR_METHOD_NOT_FOUND, msg, None)
                }
                Err(msg) => JsonRpcResponse::error(req.id, ERR_INTERNAL, msg, None),
            }
        }
        "treenode" => {
            let params: TreeNodeParams = if req.params.is_null() {
                TreeNodeParams::default()
            } else {
                match serde_json::from_value(req.params) {
                    Ok(p) => p,
                    Err(e) => {
                        return JsonRpcResponse::error(
                            req.id,
                            ERR_INVALID_PARAMS,
                            format!("invalid TreeNode parameters: {e}"),
                            None,
                        );
                    }
                }
            };
            match handler.handle_tree_node(params) {
                Ok(val) => JsonRpcResponse::success(req.id, val),
                Err(msg) if msg.starts_with("not_implemented:") => {
                    JsonRpcResponse::error(req.id, ERR_METHOD_NOT_FOUND, msg, None)
                }
                Err(msg) => JsonRpcResponse::error(req.id, ERR_INTERNAL, msg, None),
            }
        }
        "layoutchain" => {
            let params: LayoutChainParams = if req.params.is_null() {
                LayoutChainParams::default()
            } else {
                match serde_json::from_value(req.params) {
                    Ok(p) => p,
                    Err(e) => {
                        return JsonRpcResponse::error(
                            req.id,
                            ERR_INVALID_PARAMS,
                            format!("invalid LayoutChain parameters: {e}"),
                            None,
                        );
                    }
                }
            };
            match handler.handle_layout_chain(params) {
                Ok(val) => JsonRpcResponse::success(req.id, val),
                Err(msg) if msg.starts_with("not_implemented:") => {
                    JsonRpcResponse::error(req.id, ERR_METHOD_NOT_FOUND, msg, None)
                }
                Err(msg) => JsonRpcResponse::error(req.id, ERR_INTERNAL, msg, None),
            }
        }
        "overflowscan" => {
            let params: OverflowScanParams = if req.params.is_null() {
                OverflowScanParams::default()
            } else {
                match serde_json::from_value(req.params) {
                    Ok(p) => p,
                    Err(e) => {
                        return JsonRpcResponse::error(
                            req.id,
                            ERR_INVALID_PARAMS,
                            format!("invalid OverflowScan parameters: {e}"),
                            None,
                        );
                    }
                }
            };
            match handler.handle_overflow_scan(params) {
                Ok(val) => JsonRpcResponse::success(req.id, val),
                Err(msg) if msg.starts_with("not_implemented:") => {
                    JsonRpcResponse::error(req.id, ERR_METHOD_NOT_FOUND, msg, None)
                }
                Err(msg) => JsonRpcResponse::error(req.id, ERR_INTERNAL, msg, None),
            }
        }
        "signalslist" => {
            let params: SignalsListParams = if req.params.is_null() {
                SignalsListParams::default()
            } else {
                match serde_json::from_value(req.params) {
                    Ok(p) => p,
                    Err(e) => {
                        return JsonRpcResponse::error(
                            req.id,
                            ERR_INVALID_PARAMS,
                            format!("invalid SignalsList parameters: {e}"),
                            None,
                        );
                    }
                }
            };
            match handler.handle_signals_list(params) {
                Ok(val) => JsonRpcResponse::success(req.id, val),
                Err(msg) if msg.starts_with("not_implemented:") => {
                    JsonRpcResponse::error(req.id, ERR_METHOD_NOT_FOUND, msg, None)
                }
                Err(msg) => JsonRpcResponse::error(req.id, ERR_INTERNAL, msg, None),
            }
        }
        "signaltrigger" => {
            let params: SignalTriggerParams = if req.params.is_null() {
                SignalTriggerParams::default()
            } else {
                match serde_json::from_value(req.params) {
                    Ok(p) => p,
                    Err(e) => {
                        return JsonRpcResponse::error(
                            req.id,
                            ERR_INVALID_PARAMS,
                            format!("invalid SignalTrigger parameters: {e}"),
                            None,
                        );
                    }
                }
            };
            match handler.handle_signal_trigger(params) {
                Ok(val) => JsonRpcResponse::success(req.id, val),
                Err(msg) if msg.starts_with("not_implemented:") => {
                    JsonRpcResponse::error(req.id, ERR_METHOD_NOT_FOUND, msg, None)
                }
                Err(msg) => JsonRpcResponse::error(req.id, ERR_INTERNAL, msg, None),
            }
        }
        "a11ytree" => {
            let params: A11yTreeParams = if req.params.is_null() {
                A11yTreeParams::default()
            } else {
                match serde_json::from_value(req.params) {
                    Ok(p) => p,
                    Err(e) => {
                        return JsonRpcResponse::error(
                            req.id,
                            ERR_INVALID_PARAMS,
                            format!("invalid A11yTree parameters: {e}"),
                            None,
                        );
                    }
                }
            };
            match handler.handle_a11y_tree(params) {
                Ok(val) => JsonRpcResponse::success(req.id, val),
                Err(msg) if msg.starts_with("not_implemented:") => {
                    JsonRpcResponse::error(req.id, ERR_METHOD_NOT_FOUND, msg, None)
                }
                Err(msg) => JsonRpcResponse::error(req.id, ERR_INTERNAL, msg, None),
            }
        }
        "tweakslist" => {
            let params: TweaksListParams = if req.params.is_null() {
                TweaksListParams::default()
            } else {
                match serde_json::from_value(req.params) {
                    Ok(p) => p,
                    Err(e) => {
                        return JsonRpcResponse::error(
                            req.id,
                            ERR_INVALID_PARAMS,
                            format!("invalid TweaksList parameters: {e}"),
                            None,
                        );
                    }
                }
            };
            match handler.handle_tweaks_list(params) {
                Ok(val) => JsonRpcResponse::success(req.id, val),
                Err(msg) if msg.starts_with("not_implemented:") => {
                    JsonRpcResponse::error(req.id, ERR_METHOD_NOT_FOUND, msg, None)
                }
                Err(msg) => JsonRpcResponse::error(req.id, ERR_INTERNAL, msg, None),
            }
        }
        "tweakset" => {
            let params: TweakSetParams = if req.params.is_null() {
                TweakSetParams::default()
            } else {
                match serde_json::from_value(req.params) {
                    Ok(p) => p,
                    Err(e) => {
                        return JsonRpcResponse::error(
                            req.id,
                            ERR_INVALID_PARAMS,
                            format!("invalid TweakSet parameters: {e}"),
                            None,
                        );
                    }
                }
            };
            match handler.handle_tweak_set(params) {
                Ok(val) => JsonRpcResponse::success(req.id, val),
                Err(msg) if msg.starts_with("not_implemented:") => {
                    JsonRpcResponse::error(req.id, ERR_METHOD_NOT_FOUND, msg, None)
                }
                Err(msg) => JsonRpcResponse::error(req.id, ERR_INTERNAL, msg, None),
            }
        }
        "themeset" => {
            let params: ThemeSetParams = if req.params.is_null() {
                ThemeSetParams::default()
            } else {
                match serde_json::from_value(req.params) {
                    Ok(p) => p,
                    Err(e) => {
                        return JsonRpcResponse::error(
                            req.id,
                            ERR_INVALID_PARAMS,
                            format!("invalid ThemeSet parameters: {e}"),
                            None,
                        );
                    }
                }
            };
            match handler.handle_theme_set(params) {
                Ok(val) => JsonRpcResponse::success(req.id, val),
                Err(msg) if msg.starts_with("not_implemented:") => {
                    JsonRpcResponse::error(req.id, ERR_METHOD_NOT_FOUND, msg, None)
                }
                Err(msg) => JsonRpcResponse::error(req.id, ERR_INTERNAL, msg, None),
            }
        }
        "themeget" => {
            let params: ThemeGetParams = if req.params.is_null() {
                ThemeGetParams::default()
            } else {
                match serde_json::from_value(req.params) {
                    Ok(p) => p,
                    Err(e) => {
                        return JsonRpcResponse::error(
                            req.id,
                            ERR_INVALID_PARAMS,
                            format!("invalid ThemeGet parameters: {e}"),
                            None,
                        );
                    }
                }
            };
            match handler.handle_theme_get(params) {
                Ok(val) => JsonRpcResponse::success(req.id, val),
                Err(msg) if msg.starts_with("not_implemented:") => {
                    JsonRpcResponse::error(req.id, ERR_METHOD_NOT_FOUND, msg, None)
                }
                Err(msg) => JsonRpcResponse::error(req.id, ERR_INTERNAL, msg, None),
            }
        }
        "tweakssync" => {
            let params: TweaksSyncParams = if req.params.is_null() {
                TweaksSyncParams::default()
            } else {
                match serde_json::from_value(req.params) {
                    Ok(p) => p,
                    Err(e) => {
                        return JsonRpcResponse::error(
                            req.id,
                            ERR_INVALID_PARAMS,
                            format!("invalid TweaksSync parameters: {e}"),
                            None,
                        );
                    }
                }
            };
            match handler.handle_tweaks_sync(params) {
                Ok(val) => JsonRpcResponse::success(req.id, val),
                Err(msg) if msg.starts_with("not_implemented:") => {
                    JsonRpcResponse::error(req.id, ERR_METHOD_NOT_FOUND, msg, None)
                }
                Err(msg) => JsonRpcResponse::error(req.id, ERR_INTERNAL, msg, None),
            }
        }
        "eventdispatch" => {
            let params: EventDispatchParams = if req.params.is_null() {
                EventDispatchParams::default()
            } else {
                match serde_json::from_value(req.params) {
                    Ok(p) => p,
                    Err(e) => {
                        return JsonRpcResponse::error(
                            req.id,
                            ERR_INVALID_PARAMS,
                            format!("invalid EventDispatch parameters: {e}"),
                            None,
                        );
                    }
                }
            };
            match handler.handle_event_dispatch(params) {
                Ok(val) => JsonRpcResponse::success(req.id, val),
                Err(msg) if msg.starts_with("not_implemented:") => {
                    JsonRpcResponse::error(req.id, ERR_METHOD_NOT_FOUND, msg, None)
                }
                Err(msg) => JsonRpcResponse::error(req.id, ERR_INTERNAL, msg, None),
            }
        }
        "timemachinestep" => {
            let params: TimemachineStepParams = if req.params.is_null() {
                TimemachineStepParams::default()
            } else {
                match serde_json::from_value(req.params) {
                    Ok(p) => p,
                    Err(e) => {
                        return JsonRpcResponse::error(
                            req.id,
                            ERR_INVALID_PARAMS,
                            format!("invalid TimemachineStep parameters: {e}"),
                            None,
                        );
                    }
                }
            };
            match handler.handle_timemachine_step(params) {
                Ok(val) => JsonRpcResponse::success(req.id, val),
                Err(msg) if msg.starts_with("not_implemented:") => {
                    JsonRpcResponse::error(req.id, ERR_METHOD_NOT_FOUND, msg, None)
                }
                Err(msg) => JsonRpcResponse::error(req.id, ERR_INTERNAL, msg, None),
            }
        }
        "capturenode" => {
            let params: CaptureNodeParams = if req.params.is_null() {
                CaptureNodeParams::default()
            } else {
                match serde_json::from_value(req.params) {
                    Ok(p) => p,
                    Err(e) => {
                        return JsonRpcResponse::error(
                            req.id,
                            ERR_INVALID_PARAMS,
                            format!("invalid CaptureNode parameters: {e}"),
                            None,
                        );
                    }
                }
            };
            match handler.handle_capture_node(params) {
                Ok(val) => JsonRpcResponse::success(req.id, val),
                Err(msg) if msg.starts_with("not_implemented:") => {
                    JsonRpcResponse::error(req.id, ERR_METHOD_NOT_FOUND, msg, None)
                }
                Err(msg) => JsonRpcResponse::error(req.id, ERR_INTERNAL, msg, None),
            }
        }
        "auditpaint" => {
            let params: AuditPaintParams = if req.params.is_null() {
                AuditPaintParams::default()
            } else {
                match serde_json::from_value(req.params) {
                    Ok(p) => p,
                    Err(e) => {
                        return JsonRpcResponse::error(
                            req.id,
                            ERR_INVALID_PARAMS,
                            format!("invalid AuditPaint parameters: {e}"),
                            None,
                        );
                    }
                }
            };
            match handler.handle_audit_paint(params) {
                Ok(val) => JsonRpcResponse::success(req.id, val),
                Err(msg) if msg.starts_with("not_implemented:") => {
                    JsonRpcResponse::error(req.id, ERR_METHOD_NOT_FOUND, msg, None)
                }
                Err(msg) => JsonRpcResponse::error(req.id, ERR_INTERNAL, msg, None),
            }
        }
        "reloadstatus" => {
            let params: ReloadStatusParams = if req.params.is_null() {
                ReloadStatusParams::default()
            } else {
                match serde_json::from_value(req.params) {
                    Ok(p) => p,
                    Err(e) => {
                        return JsonRpcResponse::error(
                            req.id,
                            ERR_INVALID_PARAMS,
                            format!("invalid ReloadStatus parameters: {e}"),
                            None,
                        );
                    }
                }
            };
            match handler.handle_reload_status(params) {
                Ok(val) => JsonRpcResponse::success(req.id, val),
                Err(msg) if msg.starts_with("not_implemented:") => {
                    JsonRpcResponse::error(req.id, ERR_METHOD_NOT_FOUND, msg, None)
                }
                Err(msg) => JsonRpcResponse::error(req.id, ERR_INTERNAL, msg, None),
            }
        }
        "signalset" => dispatch_typed(req, SignalSetParams::default, |p| {
            handler.handle_signal_set(p)
        }),
        "runtimeerrors" => dispatch_typed(req, RuntimeErrorsParams::default, |p| {
            handler.handle_runtime_errors(p)
        }),
        "logs" => dispatch_typed(req, LogsParams::default, |p| handler.handle_logs(p)),
        "a11yaction" => dispatch_typed(req, A11yActionParams::default, |p| {
            handler.handle_a11y_action(p)
        }),
        "hotreload" => dispatch_typed(req, HotReloadParams::default, |p| {
            handler.handle_hot_reload(p)
        }),
        "nodesetloading" => dispatch_typed(req, NodeSetLoadingParams::default, |p| {
            handler.handle_node_set_loading(p)
        }),
        other => match handler.handle_custom(other, req.params) {
            Ok(val) => JsonRpcResponse::success(req.id, val),
            Err((code, msg)) => JsonRpcResponse::error(req.id, code, msg, None),
        },
    }
}

/// Shared dispatch body for typed handler arms: parse params (or defaults
/// when `null`), run the handler, and map `not_implemented:` errors to
/// [`ERR_METHOD_NOT_FOUND`].
fn dispatch_typed<P, F>(req: JsonRpcRequest, make_default: fn() -> P, call: F) -> JsonRpcResponse
where
    P: serde::de::DeserializeOwned,
    F: FnOnce(P) -> Result<serde_json::Value, String>,
{
    let params: P = if req.params.is_null() {
        make_default()
    } else {
        match serde_json::from_value(req.params) {
            Ok(p) => p,
            Err(e) => {
                return JsonRpcResponse::error(
                    req.id,
                    ERR_INVALID_PARAMS,
                    format!("invalid parameters: {e}"),
                    None,
                );
            }
        }
    };
    match call(params) {
        Ok(val) => JsonRpcResponse::success(req.id, val),
        Err(msg) if msg.starts_with("not_implemented:") => {
            JsonRpcResponse::error(req.id, ERR_METHOD_NOT_FOUND, msg, None)
        }
        Err(msg) => JsonRpcResponse::error(req.id, ERR_INTERNAL, msg, None),
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
    stream: DevStream,
    reader: BufReader<DevStream>,
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
    /// Connects to a Dev Channel Unix domain socket or Windows Named Pipe at `path`.
    ///
    /// # Examples
    ///
    /// ```no_run
    /// use martensite_host::dev_channel::DevChannelClient;
    /// use std::path::Path;
    ///
    /// # #[cfg(unix)]
    /// let client = DevChannelClient::connect(Path::new("/tmp/test.sock"));
    /// # #[cfg(windows)]
    /// let client = DevChannelClient::connect(Path::new(r"\\.\pipe\test_pipe"));
    /// ```
    pub fn connect(path: impl AsRef<Path>) -> io::Result<Self> {
        let stream = DevStream::connect(path.as_ref())?;
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
        self.hello_inner(client_version, protocol_version, None)
    }

    /// Performs the mandatory `Hello` handshake, presenting `token` as the
    /// [`HelloParams::auth_token`] bearer credential (ADR-0042).
    ///
    /// Needed when connecting to a session configured with
    /// [`DevChannelConfig::with_auth_token`].
    ///
    /// # Examples
    ///
    /// ```no_run
    /// use martensite_host::dev_channel::{DevChannelClient, MARTENSITE_VERSION, DEV_CHANNEL_PROTOCOL_VERSION};
    /// use std::path::Path;
    ///
    /// let mut client = DevChannelClient::connect(Path::new("/tmp/test.sock")).unwrap();
    /// let res = client
    ///     .hello_with_token(MARTENSITE_VERSION, DEV_CHANNEL_PROTOCOL_VERSION, "s3cret")
    ///     .unwrap();
    /// ```
    pub fn hello_with_token(
        &mut self,
        client_version: &str,
        protocol_version: u32,
        token: &str,
    ) -> io::Result<Result<HelloResult, JsonRpcError>> {
        self.hello_inner(client_version, protocol_version, Some(token))
    }

    /// Shared `Hello` handshake body for [`hello`](Self::hello) and
    /// [`hello_with_token`](Self::hello_with_token).
    fn hello_inner(
        &mut self,
        client_version: &str,
        protocol_version: u32,
        auth_token: Option<&str>,
    ) -> io::Result<Result<HelloResult, JsonRpcError>> {
        let id = self.next_id.fetch_add(1, Ordering::SeqCst);
        let params = HelloParams {
            client_version: client_version.to_string(),
            protocol_version,
            auth_token: auth_token.map(ToString::to_string),
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
            serde_json::to_value(InspectorSelectParams {
                arm,
                ..Default::default()
            })
            .unwrap_or_default(),
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

#[cfg(test)]
mod tests {
    use super::*;

    /// Builds a `Hello` request line with an optional `auth_token` param.
    fn hello_line(token: Option<&str>) -> String {
        let params = match token {
            Some(t) => serde_json::json!({
                "client_version": MARTENSITE_VERSION,
                "protocol_version": DEV_CHANNEL_PROTOCOL_VERSION,
                "auth_token": t,
            }),
            None => serde_json::json!({
                "client_version": MARTENSITE_VERSION,
                "protocol_version": DEV_CHANNEL_PROTOCOL_VERSION,
            }),
        };
        serde_json::to_string(&JsonRpcRequest::new(Some(1), "Hello", params))
            .expect("hello request serializes")
    }

    #[test]
    fn method_class_covers_every_dispatch_arm() {
        // Every normalized method name the dispatcher matches must appear
        // in the classification table — an absent entry would fail closed
        // to Mutate, so also assert the intended class for each arm.
        for (name, expected) in [
            ("hello", MethodClass::Read),
            ("treesnapshot", MethodClass::Read),
            ("lintpull", MethodClass::Read),
            ("lintscene", MethodClass::Read),
            ("eventledger", MethodClass::Read),
            ("lintapply", MethodClass::Read),
            ("treenode", MethodClass::Read),
            ("layoutchain", MethodClass::Read),
            ("overflowscan", MethodClass::Read),
            ("signalslist", MethodClass::Read),
            ("a11ytree", MethodClass::Read),
            ("tweakslist", MethodClass::Read),
            ("themeget", MethodClass::Read),
            ("auditpaint", MethodClass::Read),
            ("reloadstatus", MethodClass::Read),
            ("logs", MethodClass::Read),
            ("inspectorselect", MethodClass::Mutate),
            ("signaltrigger", MethodClass::Mutate),
            ("tweakset", MethodClass::Mutate),
            ("themeset", MethodClass::Mutate),
            ("tweakssync", MethodClass::Mutate),
            ("eventdispatch", MethodClass::Mutate),
            ("timemachinestep", MethodClass::Mutate),
            ("capturenode", MethodClass::Mutate),
            ("signalset", MethodClass::Mutate),
            ("runtimeerrors", MethodClass::Mutate),
            ("a11yaction", MethodClass::Mutate),
            ("hotreload", MethodClass::Mutate),
            ("nodesetloading", MethodClass::Mutate),
        ] {
            assert_eq!(method_class(name), expected, "method `{name}`");
        }
    }

    #[test]
    fn method_class_normalizes_and_fails_closed() {
        assert_eq!(method_class("Tree_Snapshot"), MethodClass::Read);
        assert_eq!(method_class("TREE-SNAPSHOT"), MethodClass::Read);
        assert_eq!(method_class("signal set"), MethodClass::Mutate);
        assert_eq!(method_class("definitely_not_a_method"), MethodClass::Mutate);
        assert_eq!(method_class(""), MethodClass::Mutate);
    }

    #[test]
    fn hello_rejected_without_token_when_required() {
        let mut handshook = false;
        let resp = process_request_line(
            &hello_line(None),
            &mut handshook,
            &DefaultDevChannelHandler,
            None,
            Some("expected-token"),
        );
        let err = resp.error.expect("hello without token must fail");
        assert_eq!(err.code, ERR_AUTH_FAILED);
        assert_eq!(
            err.data.as_ref().and_then(|d| d.get("error_type")),
            Some(&serde_json::json!("auth_failed"))
        );
        assert!(!handshook, "rejected handshake must not mark handshook");
    }

    #[test]
    fn hello_rejected_with_wrong_token() {
        let mut handshook = false;
        let resp = process_request_line(
            &hello_line(Some("wrong")),
            &mut handshook,
            &DefaultDevChannelHandler,
            None,
            Some("expected-token"),
        );
        assert_eq!(
            resp.error.expect("wrong token must fail").code,
            ERR_AUTH_FAILED
        );
        assert!(!handshook);
    }

    #[test]
    fn hello_accepted_with_matching_token() {
        let mut handshook = false;
        let resp = process_request_line(
            &hello_line(Some("expected-token")),
            &mut handshook,
            &DefaultDevChannelHandler,
            Some("build_x"),
            Some("expected-token"),
        );
        assert!(resp.error.is_none(), "matching token must pass: {resp:?}");
        assert!(handshook);
        let result = resp.result.expect("hello result");
        assert_eq!(
            result["protocol_version"],
            serde_json::json!(DEV_CHANNEL_PROTOCOL_VERSION)
        );
        assert_eq!(result["build_id"], serde_json::json!("build_x"));
    }

    #[test]
    fn hello_unaffected_when_no_token_configured() {
        let mut handshook = false;
        let resp = process_request_line(
            &hello_line(None),
            &mut handshook,
            &DefaultDevChannelHandler,
            None,
            None,
        );
        assert!(resp.error.is_none(), "no token configured: {resp:?}");
        assert!(handshook);
    }
}
