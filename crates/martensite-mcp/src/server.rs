//! `MartensiteMcp` server type: runtime mode management and `rmcp` wiring.
//!
//! The server owns a [`ToolRouter`] assembled from [`crate::tools`], the
//! offline [`OfflineContext`], and a lazily connected dev-channel client.
//! Live calls are version-locked (D1); when no dev session is reachable the
//! server degrades to offline mode without aborting the host agent session
//! (invariant 5).

use std::path::PathBuf;
use std::sync::Mutex;

use rmcp::handler::server::router::tool::ToolRouter;
use rmcp::handler::server::tool::ToolCallContext;
use rmcp::model::{
    CallToolRequestParams, CallToolResponse, GetPromptRequestMethod, GetPromptRequestParams,
    GetPromptResponse, Implementation, ListPromptsResult, ListResourceTemplatesResult,
    ListResourcesResult, ListToolsResult, PaginatedRequestParams, ReadResourceRequestParams,
    ReadResourceResponse, ServerCapabilities, ServerConfig, Tool,
};
use rmcp::service::RequestContext;
use rmcp::{ErrorData, RoleServer, ServerHandler};

use crate::client::{discover_socket, DevChannelClient};
use crate::error::McpError;
use crate::offline::OfflineContext;
use crate::tools;

/// Options controlling `cargo martensite mcp` server startup.
///
/// # Examples
///
/// ```
/// use martensite_mcp::server::McpServerOptions;
///
/// let opts = McpServerOptions::default();
/// assert!(opts.socket.is_none());
/// ```
#[derive(Debug, Clone, Default)]
pub struct McpServerOptions {
    /// Explicit dev-channel socket path (`--socket <path>`).
    pub socket: Option<PathBuf>,
    /// Offline scene dump to evaluate (`--scene <path>`).
    pub scene: Option<PathBuf>,
    /// Cargo workspace root override (`--workspace <path>`).
    pub workspace_root: Option<PathBuf>,
    /// Permit connecting to a dev app whose version differs from this build.
    pub allow_version_mismatch: bool,
    /// Never attach to a dev session (`--offline`); live-only tools report
    /// the offline hint.
    pub offline: bool,
}

impl McpServerOptions {
    /// Options that never attach to a dev session (`--offline`).
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite_mcp::server::McpServerOptions;
    ///
    /// let opts = McpServerOptions::offline();
    /// assert!(opts.offline);
    /// ```
    #[must_use]
    pub fn offline() -> Self {
        Self {
            offline: true,
            ..Self::default()
        }
    }
}

/// Whether the server currently observes a live dev session.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ServerMode {
    /// Attached to a running Martensite dev app.
    Live {
        /// Dev-channel socket in use.
        socket: PathBuf,
        /// Application version reported by the handshake.
        app_version: String,
    },
    /// No dev session reachable; static-analysis toolset only.
    Offline,
}

/// Internal lazy-connection state for the dev channel.
enum LiveState {
    /// No connection attempt yet.
    NotTried,
    /// Attached to a running app.
    Connected {
        /// Connected dev-channel client.
        client: DevChannelClient,
        /// Socket path the client connected through.
        socket: PathBuf,
    },
    /// Discovery/connect failed; keep retrying on next call (fail-safe
    /// degradation: a later-launched dev app can still be picked up).
    Unavailable,
}

/// The Martensite MCP server: 28 semantic tools, dynamic resources, and
/// prompt templates over stdio, bridging to the ADR-0038 dev channel.
///
/// # Examples
///
/// ```
/// use martensite_mcp::server::{MartensiteMcp, McpServerOptions, ServerMode};
///
/// let server = MartensiteMcp::new(McpServerOptions::default());
/// // With no running dev app the server starts in offline mode.
/// assert!(matches!(server.mode(), ServerMode::Offline));
/// ```
pub struct MartensiteMcp {
    tool_router: ToolRouter<Self>,
    opts: McpServerOptions,
    offline: OfflineContext,
    live: Mutex<LiveState>,
}

impl MartensiteMcp {
    /// Creates the server: discovers the workspace, builds the tool router,
    /// and defers the dev-channel handshake until the first live call.
    #[must_use]
    pub fn new(opts: McpServerOptions) -> Self {
        let offline = OfflineContext::discover(&opts);
        Self {
            tool_router: tools::tool_router(),
            opts,
            offline,
            live: Mutex::new(LiveState::NotTried),
        }
    }

    /// Server startup options.
    pub fn options(&self) -> &McpServerOptions {
        &self.opts
    }

    /// Offline execution context (workspace root, scene file, audit log).
    pub fn offline(&self) -> &OfflineContext {
        &self.offline
    }

    /// Current connectivity mode observed by the last live attempt.
    pub fn mode(&self) -> ServerMode {
        let live = self.live.lock().unwrap_or_else(|e| e.into_inner());
        match &*live {
            LiveState::Connected { client, socket } => ServerMode::Live {
                socket: socket.clone(),
                app_version: client.app_version().to_string(),
            },
            LiveState::NotTried | LiveState::Unavailable => ServerMode::Offline,
        }
    }

    /// Calls a dev-channel method, transparently attaching on first use.
    ///
    /// Returns `Ok(None)` when no dev session is reachable, letting tools fall
    /// back to offline behavior. Hard protocol failures (version mismatch,
    /// RPC errors from the app) surface as `Err`.
    ///
    /// # Errors
    ///
    /// * [`McpError::VersionMismatch`] / [`McpError::ProtocolMismatch`] — D1
    ///   version-lock violations.
    /// * [`McpError::Ipc`] — the app returned an RPC-level error.
    pub fn try_live_call(
        &self,
        method: &str,
        params: serde_json::Value,
    ) -> Result<Option<serde_json::Value>, McpError> {
        if self.opts.offline {
            return Ok(None);
        }
        let mut live = self.live.lock().unwrap_or_else(|e| e.into_inner());
        match &mut *live {
            LiveState::Connected { client, .. } => match client.call(method, params) {
                Ok(v) => Ok(Some(v)),
                Err(e) => {
                    // A dead socket demotes to Unavailable so a restarted app
                    // is re-discovered on the next call.
                    if matches!(e, McpError::Io(_) | McpError::Ipc(_)) {
                        *live = LiveState::Unavailable;
                    }
                    Err(e)
                }
            },
            LiveState::NotTried | LiveState::Unavailable => {
                match discover_socket(self.opts.socket.as_deref()) {
                    Ok(Some(path)) => {
                        match DevChannelClient::connect(&path, self.opts.allow_version_mismatch) {
                            Ok(mut client) => {
                                let result = client.call(method, params).map(Some);
                                if result.is_ok() {
                                    *live = LiveState::Connected {
                                        client,
                                        socket: path,
                                    };
                                }
                                result
                            }
                            // Version-lock violations are hard errors.
                            Err(
                                e @ (McpError::VersionMismatch { .. }
                                | McpError::ProtocolMismatch { .. }),
                            ) => Err(e),
                            // Everything else → offline degradation.
                            Err(_) => {
                                *live = LiveState::Unavailable;
                                Ok(None)
                            }
                        }
                    }
                    Ok(None) => {
                        *live = LiveState::Unavailable;
                        Ok(None)
                    }
                    Err(e @ McpError::Ipc(_)) if self.opts.socket.is_some() => Err(e),
                    Err(_) => {
                        *live = LiveState::Unavailable;
                        Ok(None)
                    }
                }
            }
        }
    }

    /// Calls a dev-channel method for tools that have no offline equivalent.
    ///
    /// # Errors
    ///
    /// [`McpError::Ipc`] when no live dev session is reachable, plus the
    /// errors documented on [`Self::try_live_call`].
    pub fn live_call(
        &self,
        method: &str,
        params: serde_json::Value,
    ) -> Result<serde_json::Value, McpError> {
        self.try_live_call(method, params)?.ok_or_else(|| {
            McpError::Ipc(format!(
                "`{method}` requires a live Martensite dev session; \
                 start `cargo martensite dev` or pass --socket <path>"
            ))
        })
    }
}

impl ServerHandler for MartensiteMcp {
    fn get_info(&self) -> ServerConfig {
        ServerConfig::new(
            ServerCapabilities::builder()
                .enable_tools()
                .enable_tool_list_changed()
                .enable_resources()
                .enable_resources_subscribe()
                .enable_resources_list_changed()
                .enable_prompts()
                .enable_prompts_list_changed()
                .build(),
        )
        .with_server_info(Implementation::new(
            "martensite-mcp",
            env!("CARGO_PKG_VERSION"),
        ))
        .with_instructions(
            "Martensite semantic development server (ADR-0039). Recommended \
             workflow:\n\
             1. Check connectivity first: live tools require a running \
             `cargo martensite dev` session; outputs report `mode`/`source` \
             (`live` vs `offline`), and offline mode degrades to static \
             analysis (lint, scaffold, doctor) only.\n\
             2. Locate widgets with `martensite_inspect_tree` or \
             `martensite_explain_overflow`; the returned `id`/`ref` handles \
             address nodes across calls.\n\
             3. Pull detail via `martensite_get_node` and \
             `martensite_inspect_a11y_tree`; inspect reactive state via \
             `martensite_inspect_signals`.\n\
             4. Mutate the live app via `martensite_dispatch_event`, \
             `martensite_invoke_accessibility_action`, \
             `martensite_set_tweak`/`martensite_set_theme`, or \
             `martensite_set_signal`/`martensite_trigger_signal` — mutation \
             tools are annotated `readOnlyHint=false`.\n\
             5. Verify every mutation with `martensite_capture_node`, \
             `martensite_runtime_errors`, and `martensite_logs`.\n\
             6. After source edits, call `martensite_hot_reload`, then \
             confirm the cycle via `martensite_reload_status`.",
        )
    }

    async fn call_tool(
        &self,
        request: CallToolRequestParams,
        context: RequestContext<RoleServer>,
    ) -> Result<CallToolResponse, ErrorData> {
        let ctx = ToolCallContext::new(self, request, context);
        self.tool_router.call(ctx).await
    }

    async fn list_tools(
        &self,
        _request: Option<PaginatedRequestParams>,
        _context: RequestContext<RoleServer>,
    ) -> Result<ListToolsResult, ErrorData> {
        Ok(ListToolsResult::with_all_items(self.tool_router.list_all()))
    }

    fn get_tool(&self, name: &str) -> Option<Tool> {
        self.tool_router.get(name).cloned()
    }

    async fn list_resources(
        &self,
        _request: Option<PaginatedRequestParams>,
        _context: RequestContext<RoleServer>,
    ) -> Result<ListResourcesResult, ErrorData> {
        Ok(ListResourcesResult::with_all_items(
            crate::resources::list_resources(),
        ))
    }

    async fn list_resource_templates(
        &self,
        _request: Option<PaginatedRequestParams>,
        _context: RequestContext<RoleServer>,
    ) -> Result<ListResourceTemplatesResult, ErrorData> {
        Ok(ListResourceTemplatesResult::with_all_items(
            crate::resources::list_resource_templates(),
        ))
    }

    async fn read_resource(
        &self,
        request: ReadResourceRequestParams,
        _context: RequestContext<RoleServer>,
    ) -> Result<ReadResourceResponse, ErrorData> {
        crate::resources::read_resource(self, &request.uri)
            .map(ReadResourceResponse::Complete)
            .map_err(|e| {
                if matches!(e, McpError::ResourceNotFound(_)) {
                    ErrorData::resource_not_found(e.to_string(), None)
                } else {
                    e.into()
                }
            })
    }

    async fn list_prompts(
        &self,
        _request: Option<PaginatedRequestParams>,
        _context: RequestContext<RoleServer>,
    ) -> Result<ListPromptsResult, ErrorData> {
        Ok(ListPromptsResult::with_all_items(
            crate::prompts::list_prompts(),
        ))
    }

    async fn get_prompt(
        &self,
        request: GetPromptRequestParams,
        _context: RequestContext<RoleServer>,
    ) -> Result<GetPromptResponse, ErrorData> {
        crate::prompts::get_prompt(self, &request.name, request.arguments.as_ref())
            .map(GetPromptResponse::Complete)
            .map_err(|e| {
                if matches!(e, McpError::ResourceNotFound(_)) {
                    ErrorData::method_not_found::<GetPromptRequestMethod>()
                } else {
                    e.into()
                }
            })
    }
}

impl From<McpError> for ErrorData {
    fn from(err: McpError) -> Self {
        let code = rmcp::model::ErrorCode(err.code());
        ErrorData::new(code, err.to_string(), None)
    }
}

/// Runs the MCP server on stdio until the host agent disconnects.
///
/// This is the entry point invoked by `cargo martensite mcp`.
///
/// # Errors
///
/// Returns [`McpError::Ipc`] when the stdio transport fails to initialize and
/// [`McpError::Internal`] if the serving task aborts.
pub async fn serve_stdio(opts: McpServerOptions) -> Result<(), McpError> {
    let server = MartensiteMcp::new(opts);
    let service = rmcp::serve_server(server, rmcp::transport::stdio())
        .await
        .map_err(|e| McpError::Ipc(format!("stdio transport init failed: {e}")))?;
    service
        .waiting()
        .await
        .map_err(|e| McpError::Internal(format!("server task failed: {e}")))?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn server_constructs_offline() {
        let server = MartensiteMcp::new(McpServerOptions::offline());
        let res = server.try_live_call("tree_snapshot", serde_json::json!({}));
        assert!(matches!(res, Ok(None)), "expected Ok(None), got {res:?}");
    }

    #[test]
    fn offline_flag_wins_over_explicit_socket() {
        let dir = std::env::temp_dir().join(format!("martensite-mcp-test-{}", std::process::id()));
        std::fs::create_dir_all(&dir).expect("create temp dir");
        let sock = dir.join("exists.sock");
        std::fs::write(&sock, b"").expect("create placeholder socket file");
        let server = MartensiteMcp::new(McpServerOptions {
            socket: Some(sock.clone()),
            ..McpServerOptions::offline()
        });
        let res = server.try_live_call("tree_snapshot", serde_json::json!({}));
        assert!(matches!(res, Ok(None)), "expected Ok(None), got {res:?}");
        let _ = std::fs::remove_file(&sock);
        let _ = std::fs::remove_dir(&dir);
    }

    #[test]
    fn tool_router_registers_all() {
        let server = MartensiteMcp::new(McpServerOptions::default());
        let names: Vec<String> = server
            .tool_router
            .list_all()
            .iter()
            .map(|t| t.name.to_string())
            .collect();
        assert_eq!(names.len(), 28, "expected 28 tools, got {names:?}");
    }

    #[test]
    fn every_tool_carries_annotations() {
        let server = MartensiteMcp::new(McpServerOptions::default());
        for tool in server.tool_router.list_all() {
            let ann = tool
                .annotations
                .as_ref()
                .unwrap_or_else(|| panic!("tool `{}` lacks ToolAnnotations", tool.name));
            assert_eq!(
                ann.open_world_hint,
                Some(false),
                "`{}` only talks to the local dev session",
                tool.name
            );
        }
        // Spot-check the two classes.
        let get = |name: &str| {
            server
                .tool_router
                .list_all()
                .iter()
                .find(|t| t.name == name)
                .and_then(|t| t.annotations.clone())
                .unwrap_or_else(|| panic!("`{name}` missing"))
        };
        assert_eq!(get("martensite_inspect_tree").read_only_hint, Some(true));
        assert_eq!(get("martensite_runtime_errors").read_only_hint, Some(true));
        assert_eq!(get("martensite_logs").read_only_hint, Some(true));
        assert_eq!(get("martensite_hot_reload").read_only_hint, Some(false));
        assert_eq!(get("martensite_hot_reload").destructive_hint, Some(true));
        assert_eq!(get("martensite_set_signal").read_only_hint, Some(false));
        assert_eq!(
            get("martensite_invoke_accessibility_action").read_only_hint,
            Some(false)
        );
    }

    #[test]
    fn error_data_mapping() {
        let err: ErrorData = McpError::NodeNotFound("n".into()).into();
        assert_eq!(err.code.0, -32004);
    }
}
