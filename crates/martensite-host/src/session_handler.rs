//! ADR-0038 dev-channel → [`DevSession`] bridge.
//!
//! `SessionDevChannelHandler` adapts the dev-channel wire surface
//! ([`DevChannelHandler`]) onto a
//! [`martensite_devtools::dev_session::DevSession`] so a running
//! application can serve inspector, lint, tweak, signal, event, and
//! hot-reload queries to local developer tools over the
//! version-handshook JSON-RPC-lite socket.
//!
//! NOTE: this module is declared in `lib.rs` as
//! `#[cfg(feature = "dev-channel")] pub mod session_handler;` (the module
//! contents assume the `dev-channel` feature is enabled — it is only
//! compiled under that feature). `SessionDevChannelHandler` and
//! `serve_dev_session` are re-exported flat at the crate root next to the
//! `dev_channel` re-exports.
//!
//! # Bridging contract
//!
//! Every [`DevChannelHandler`] method serializes its typed params with
//! [`serde_json::to_value`] and invokes the matching [`DevSession`]
//! handler, which returns wire-ready JSON or an error string. Error
//! strings propagate unchanged; `dev_channel::process_request_line` maps
//! a `not_implemented:*` error to `-32601` and any other failure to
//! `-32603`, so capability gaps are reported honestly (ADR-0039 D9).
//! Unknown methods fall through to [`DevChannelHandler::handle_custom`],
//! which reports `-32601` (method not found).

use std::io;
use std::path::PathBuf;
use std::sync::Arc;

use martensite_devtools::dev_session::{DevSession, SessionResult};
use serde::Serialize;
use serde_json::Value;

use crate::dev_channel::{
    A11yActionParams, A11yTreeParams, AuditPaintParams, CaptureNodeParams, DevChannelConfig,
    DevChannelHandler, DevChannelServer, EventDispatchParams, EventLedgerParams, HotReloadParams,
    InspectorSelectParams, LayoutChainParams, LintApplyParams, LintPullParams, LogsParams,
    NodeSetLoadingParams, OverflowScanParams, ReloadStatusParams, RuntimeErrorsParams,
    SignalSetParams, SignalTriggerParams, SignalsListParams, ThemeGetParams, ThemeSetParams,
    TimemachineStepParams, TreeNodeParams, TreeSnapshotParams, TweakSetParams, TweaksListParams,
    TweaksSyncParams, ERR_METHOD_NOT_FOUND,
};

/// A [`DevChannelHandler`] that forwards dev-channel requests to a
/// [`DevSession`].
///
/// The handler is cheap to construct and safe to share across the
/// server's per-connection threads: it only holds an [`Arc`] to the
/// session, and all session mutation happens through the session's
/// interior locks.
///
/// # Examples
///
/// ```
/// use std::sync::Arc;
///
/// use martensite_devtools::dev_session::{DevSession, NullProbe};
/// use martensite_host::dev_channel::{DevChannelHandler, ReloadStatusParams};
/// use martensite_host::session_handler::SessionDevChannelHandler;
///
/// let session = Arc::new(DevSession::new(Box::new(NullProbe)));
/// let handler = SessionDevChannelHandler::new(session);
///
/// // Fresh session: hot reload has never run.
/// let status = handler.handle_reload_status(ReloadStatusParams {}).unwrap();
/// assert_eq!(status["active"], false);
/// ```
pub struct SessionDevChannelHandler {
    session: Arc<DevSession>,
}

impl SessionDevChannelHandler {
    /// Creates a handler bridging the dev channel to `session`.
    ///
    /// # Examples
    ///
    /// ```
    /// use std::sync::Arc;
    ///
    /// use martensite_devtools::dev_session::{DevSession, NullProbe};
    /// use martensite_host::session_handler::SessionDevChannelHandler;
    ///
    /// let handler =
    ///     SessionDevChannelHandler::new(Arc::new(DevSession::new(Box::new(NullProbe))));
    /// ```
    pub fn new(session: Arc<DevSession>) -> Self {
        Self { session }
    }

    /// Returns the shared [`DevSession`] backing this handler.
    ///
    /// # Examples
    ///
    /// ```
    /// use std::sync::Arc;
    ///
    /// use martensite_devtools::dev_session::{DevSession, NullProbe};
    /// use martensite_host::session_handler::SessionDevChannelHandler;
    ///
    /// let session = Arc::new(DevSession::new(Box::new(NullProbe)));
    /// let handler = SessionDevChannelHandler::new(Arc::clone(&session));
    /// session.record_reload("build_7");
    /// let status = handler
    ///     .session()
    ///     .reload_status(&serde_json::json!({}))
    ///     .unwrap();
    /// assert_eq!(status["total_reloads"], 1);
    /// ```
    pub fn session(&self) -> &Arc<DevSession> {
        &self.session
    }

    /// Serializes `params` and invokes `call` on the wrapped session,
    /// propagating the session's result (or error string) unchanged.
    fn forward<P: Serialize>(
        &self,
        params: &P,
        call: impl FnOnce(&DevSession, &Value) -> SessionResult,
    ) -> SessionResult {
        let params = serde_json::to_value(params).unwrap_or_default();
        call(&self.session, &params)
    }
}

impl DevChannelHandler for SessionDevChannelHandler {
    /// Bridges `TreeSnapshot` to [`DevSession::tree_snapshot`].
    fn handle_tree_snapshot(&self, params: TreeSnapshotParams) -> Result<Value, String> {
        self.forward(&params, DevSession::tree_snapshot)
    }

    /// Bridges `LintPull`/`LintScene` to [`DevSession::lint_pull`].
    fn handle_lint_pull(&self, params: LintPullParams) -> Result<Value, String> {
        self.forward(&params, DevSession::lint_pull)
    }

    /// Bridges `EventLedger` to [`DevSession::event_ledger`].
    fn handle_event_ledger(&self, params: EventLedgerParams) -> Result<Value, String> {
        self.forward(&params, DevSession::event_ledger)
    }

    /// Bridges `InspectorSelect` to [`DevSession::inspector_select`].
    fn handle_inspector_select(&self, params: InspectorSelectParams) -> Result<Value, String> {
        self.forward(&params, DevSession::inspector_select)
    }

    /// Bridges `LintApply` to [`DevSession::lint_apply`].
    ///
    /// Fix evaluation runs against a copy of the scene model inside the
    /// session; the live widget tree is never mutated (ADR-0038).
    fn handle_lint_apply(&self, params: LintApplyParams) -> Result<Value, String> {
        self.forward(&params, DevSession::lint_apply)
    }

    /// Bridges `TreeNode` to [`DevSession::tree_node`].
    fn handle_tree_node(&self, params: TreeNodeParams) -> Result<Value, String> {
        self.forward(&params, DevSession::tree_node)
    }

    /// Bridges `LayoutChain` to [`DevSession::layout_chain`].
    fn handle_layout_chain(&self, params: LayoutChainParams) -> Result<Value, String> {
        self.forward(&params, DevSession::layout_chain)
    }

    /// Bridges `OverflowScan` to [`DevSession::overflow_scan`].
    fn handle_overflow_scan(&self, params: OverflowScanParams) -> Result<Value, String> {
        self.forward(&params, DevSession::overflow_scan)
    }

    /// Bridges `SignalsList` to [`DevSession::signals_list`].
    fn handle_signals_list(&self, params: SignalsListParams) -> Result<Value, String> {
        self.forward(&params, DevSession::signals_list)
    }

    /// Bridges `SignalTrigger` to [`DevSession::signal_trigger`].
    fn handle_signal_trigger(&self, params: SignalTriggerParams) -> Result<Value, String> {
        self.forward(&params, DevSession::signal_trigger)
    }

    /// Bridges `A11yTree` to [`DevSession::a11y_tree`].
    fn handle_a11y_tree(&self, params: A11yTreeParams) -> Result<Value, String> {
        self.forward(&params, DevSession::a11y_tree)
    }

    /// Bridges `TweaksList` to [`DevSession::tweaks_list`].
    fn handle_tweaks_list(&self, params: TweaksListParams) -> Result<Value, String> {
        self.forward(&params, DevSession::tweaks_list)
    }

    /// Bridges `TweakSet` to [`DevSession::tweak_set`].
    fn handle_tweak_set(&self, params: TweakSetParams) -> Result<Value, String> {
        self.forward(&params, DevSession::tweak_set)
    }

    /// Bridges `ThemeSet` to [`DevSession::theme_set`].
    fn handle_theme_set(&self, params: ThemeSetParams) -> Result<Value, String> {
        self.forward(&params, DevSession::theme_set)
    }

    /// Bridges `ThemeGet` to [`DevSession::theme_get`].
    fn handle_theme_get(&self, params: ThemeGetParams) -> Result<Value, String> {
        self.forward(&params, DevSession::theme_get)
    }

    /// Bridges `TweaksSync` to [`DevSession::tweaks_sync`].
    fn handle_tweaks_sync(&self, params: TweaksSyncParams) -> Result<Value, String> {
        self.forward(&params, DevSession::tweaks_sync)
    }

    /// Bridges `EventDispatch` to [`DevSession::event_dispatch`].
    fn handle_event_dispatch(&self, params: EventDispatchParams) -> Result<Value, String> {
        self.forward(&params, DevSession::event_dispatch)
    }

    /// Bridges `TimemachineStep` to [`DevSession::timemachine_step`].
    fn handle_timemachine_step(&self, params: TimemachineStepParams) -> Result<Value, String> {
        self.forward(&params, DevSession::timemachine_step)
    }

    /// Bridges `CaptureNode` to [`DevSession::capture_node`].
    fn handle_capture_node(&self, params: CaptureNodeParams) -> Result<Value, String> {
        self.forward(&params, DevSession::capture_node)
    }

    /// Bridges `AuditPaint` to [`DevSession::audit_paint`].
    fn handle_audit_paint(&self, params: AuditPaintParams) -> Result<Value, String> {
        self.forward(&params, DevSession::audit_paint)
    }

    /// Bridges `ReloadStatus` to [`DevSession::reload_status`].
    fn handle_reload_status(&self, params: ReloadStatusParams) -> Result<Value, String> {
        self.forward(&params, DevSession::reload_status)
    }

    /// Bridges `SignalSet` to [`DevSession::signal_set`].
    fn handle_signal_set(&self, params: SignalSetParams) -> Result<Value, String> {
        self.forward(&params, DevSession::signal_set)
    }

    /// Bridges `RuntimeErrors` to [`DevSession::runtime_errors`].
    fn handle_runtime_errors(&self, params: RuntimeErrorsParams) -> Result<Value, String> {
        self.forward(&params, DevSession::runtime_errors)
    }

    /// Bridges `Logs` to [`DevSession::logs`].
    fn handle_logs(&self, params: LogsParams) -> Result<Value, String> {
        self.forward(&params, DevSession::logs)
    }

    /// Bridges `A11yAction` to [`DevSession::a11y_action`].
    fn handle_a11y_action(&self, params: A11yActionParams) -> Result<Value, String> {
        self.forward(&params, DevSession::a11y_action)
    }

    /// Bridges `HotReload` to [`DevSession::hot_reload`].
    fn handle_hot_reload(&self, params: HotReloadParams) -> Result<Value, String> {
        self.forward(&params, DevSession::hot_reload)
    }

    /// Bridges `NodeSetLoading` to [`DevSession::node_set_loading`].
    fn handle_node_set_loading(&self, params: NodeSetLoadingParams) -> Result<Value, String> {
        self.forward(&params, DevSession::node_set_loading)
    }

    /// Reports unrecognized methods as `-32601` (method not found).
    ///
    /// `method` arrives normalized by the dispatcher (lowercase, with
    /// `_`, `-`, and spaces stripped). All known wire methods have typed
    /// dispatch arms, so reaching this point means the capability is
    /// genuinely absent.
    fn handle_custom(&self, method: &str, params: Value) -> Result<Value, (i32, String)> {
        let _ = params;
        Err((ERR_METHOD_NOT_FOUND, format!("method not found: {method}")))
    }
}

/// Spawns a dev-channel server that serves `session` for `build_id`.
///
/// The socket path follows ADR-0038 resolution
/// ([`socket_path_for_session`](crate::dev_channel::socket_path_for_session)):
/// `$XDG_RUNTIME_DIR/martensite/<build_id>.sock` on Unix (falling back to
/// `/tmp/martensite-<user>/`), or `\\.\pipe\martensite-<build_id>` on
/// Windows. Once the socket is bound, the session's reload-request marker
/// path is set to `<socket>.reload-request` so the `hot_reload` handler
/// can drop the marker file the dev coordinator (`cargo martensite dev`)
/// polls for. The returned [`DevChannelServer`] owns the accept-loop
/// thread — it is spawned internally by `DevChannelServer::start_with_config`
/// and cleanly stopped, socket file unlinked, when the server is
/// [`stop`](DevChannelServer::stop)ed or dropped.
///
/// # Examples
///
/// ```no_run
/// use std::sync::Arc;
///
/// use martensite_devtools::dev_session::{DevSession, NullProbe};
/// use martensite_host::session_handler::serve_dev_session;
///
/// let session = Arc::new(DevSession::new(Box::new(NullProbe)));
/// let server = serve_dev_session(session, "build_42").expect("dev channel bound");
/// assert!(server.is_running());
/// ```
pub fn serve_dev_session(session: Arc<DevSession>, build_id: &str) -> io::Result<DevChannelServer> {
    let server = DevChannelConfig::new()
        .with_build_id(build_id)
        .with_handler(Arc::new(SessionDevChannelHandler::new(Arc::clone(
            &session,
        ))))
        .start()?;
    // Register the coordinator-polled marker path now that the resolved
    // socket path is known; `DevSession::hot_reload` writes it.
    session.set_reload_request_path(PathBuf::from(format!(
        "{}.reload-request",
        server.socket_path().display()
    )));
    Ok(server)
}
