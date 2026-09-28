//! Error types for the Martensite Model Context Protocol (MCP) server.

use thiserror::Error;

/// Error variants encountered during Martensite MCP server operations.
///
/// # Examples
///
/// ```
/// use martensite_mcp::error::McpError;
///
/// let err = McpError::NodeNotFound("node-42".to_string());
/// assert_eq!(err.to_string(), "widget node not found: node-42");
/// ```
#[derive(Debug, Error)]
pub enum McpError {
    /// Dev channel IPC transport failure.
    #[error("IPC communication error: {0}")]
    Ipc(String),

    /// Handshake protocol version mismatch.
    #[error("protocol version mismatch: expected {expected}, actual {actual}")]
    ProtocolMismatch {
        /// Expected protocol version.
        expected: u32,
        /// Actual protocol version received.
        actual: u32,
    },

    /// Handshake crate / framework version mismatch.
    #[error("version mismatch: expected {expected}, actual {actual}")]
    VersionMismatch {
        /// Expected crate version string.
        expected: String,
        /// Actual crate version string received.
        actual: String,
    },

    /// Requested widget node was not found in the arena.
    #[error("widget node not found: {0}")]
    NodeNotFound(String),

    /// Target reactive signal was not found in the reactive registry.
    #[error("signal not found: {0}")]
    SignalNotFound(String),

    /// Target live tweak parameter was not found in the tweak registry.
    #[error("tweak not found: {0}")]
    TweakNotFound(String),

    /// Target dynamic MCP resource was not found.
    #[error("resource not found: {0}")]
    ResourceNotFound(String),

    /// Target design lint finding was not found.
    #[error("lint finding not found: {0}")]
    FindingNotFound(String),

    /// Parameter passed to an MCP tool call failed validation.
    #[error("invalid parameter: {0}")]
    InvalidParameter(String),

    /// Mutation operation target was invalid.
    #[error("invalid mutation target: {0}")]
    InvalidTarget(String),

    /// TimeMachine debugger action was invalid.
    #[error("invalid timemachine action: {0}")]
    InvalidAction(String),

    /// Attempted file write outside Cargo workspace boundaries.
    #[error("workspace confinement violation: {0}")]
    WorkspaceConfinementViolation(String),

    /// Attempted live mutation or disk write without required confirmation.
    #[error("unconfirmed mutation: {0}")]
    UnconfirmedMutation(String),

    /// Optimistic concurrency token mismatch during live tweak synchronization.
    #[error("revision conflict: expected {expected}, actual {actual}")]
    RevisionConflict {
        /// Expected revision token.
        expected: String,
        /// Actual revision token found in registry.
        actual: String,
    },

    /// Tweak or operation denied because the server or session is read-only.
    #[error("read-only mode violation: {0}")]
    ReadOnlyViolation(String),

    /// Taffy layout engine diagnostic failure.
    #[error("layout engine error: {0}")]
    LayoutEngine(String),

    /// Design lint engine or fix application error.
    #[error("design lint error: {0}")]
    LintEngine(String),

    /// Accessibility tree inspection or APG evaluation error.
    #[error("accessibility error: {0}")]
    A11yError(String),

    /// Headless or offscreen render capture error.
    #[error("render error: {0}")]
    RenderError(String),

    /// External command or test execution failure.
    #[error("execution failed: {0}")]
    ExecutionFailed(String),

    /// Underlying standard I/O error.
    #[error("I/O error: {0}")]
    Io(#[from] std::io::Error),

    /// JSON serialization or deserialization failure.
    #[error("JSON serialization error: {0}")]
    Json(#[from] serde_json::Error),

    /// Internal server error.
    #[error("internal server error: {0}")]
    Internal(String),
}

impl McpError {
    /// Returns the JSON-RPC error code corresponding to this error variant.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite_mcp::error::McpError;
    ///
    /// let err = McpError::NodeNotFound("foo".to_string());
    /// assert_eq!(err.code(), -32004);
    /// ```
    #[must_use]
    pub fn code(&self) -> i32 {
        match self {
            Self::InvalidParameter(_) => -32602, // Invalid params (JSON-RPC standard)
            Self::Internal(_) => -32603,         // Internal error (JSON-RPC standard)
            Self::ProtocolMismatch { .. } => -32001,
            Self::VersionMismatch { .. } => -32002,
            Self::Ipc(_) => -32003,
            Self::NodeNotFound(_) => -32004,
            Self::SignalNotFound(_) => -32005,
            Self::TweakNotFound(_) => -32006,
            Self::ResourceNotFound(_) => -32007,
            Self::FindingNotFound(_) => -32008,
            Self::WorkspaceConfinementViolation(_) => -32009,
            Self::UnconfirmedMutation(_) => -32010,
            Self::RevisionConflict { .. } => -32011,
            Self::ReadOnlyViolation(_) => -32012,
            Self::LayoutEngine(_) => -32013,
            Self::LintEngine(_) => -32014,
            Self::A11yError(_) => -32015,
            Self::RenderError(_) => -32016,
            Self::ExecutionFailed(_) => -32017,
            Self::InvalidTarget(_) => -32018,
            Self::InvalidAction(_) => -32019,
            Self::Io(_) => -32020,
            Self::Json(_) => -32021,
        }
    }
}
