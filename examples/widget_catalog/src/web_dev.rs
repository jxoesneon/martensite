//! Web dev channel (ADR-0042) — optional browser-side dev tooling.
//!
//! Compiled only for `wasm32-unknown-unknown` with `web-dev`:
//!
//! ```toml
//! [features]
//! web-dev = ["martensite-devtools/web-dev-channel"]
//! ```
//!
//! ## Direction matters
//!
//! A browser page cannot *bind* a socket — the host-side
//! `serve_dev_session` listener is unreachable from wasm. The web leg
//! therefore **dials out**: [`WebDevChannel`] opens a WebSocket to
//! `ws://127.0.0.1:8788/dev-channel`, where
//! `cargo martensite dev-web` bridges back onto the local dev-channel
//! protocol and forward-declares clients that connect through it.
//!
//! ## Responsibility split
//!
//! [`WebDevChannel`] already owns the connection lifecycle **and** the
//! mandatory `Hello` gate — ordering (`handshake_required` until a
//! handshake completes), `client_version`/`protocol_version`
//! verification, and the `auth_token` bearer check. The bearer is also
//! part of the dial URL (`relay_url(port, token)`), so the relay only
//! accepts a page presenting the same per-run token it printed.
//!
//! What the transport deliberately does *not* do is dispatch: every
//! post-handshake request line is handed to [`process_request_line`]
//! here, which routes it onto the real [`DevSession`] — the same
//! session (same arena probe, focus manager, log ring, signal
//! adapters) the host socket would serve natively. Method
//! normalization and error-code mapping mirror
//! `martensite_host::dev_channel::process_request_line`: method names
//! are lowercased with `_`/`-`/space stripped, `not_implemented:*`
//! errors map to `-32601` so capability gaps degrade honestly
//! (ADR-0039 D9), and unknown methods report `-32601` method-not-found.
//!
//! ## Opt-in token
//!
//! The channel is intentionally unreachable unless the developer asks
//! for it: the page URL must carry `?dev_token=<relay-token>`. A
//! public `web-dev` build served without the parameter never dials a
//! loopback relay.

use std::sync::{Arc, Mutex};

use martensite::core::WidgetArena;
use martensite::devtools::dev_session::{DevSession, SessionResult, SignalAdapter};
use martensite::devtools::web_channel::{
    relay_url, WebChannelOptions, WebDevChannel, DEFAULT_RELAY_PORT, DEV_CHANNEL_METHODS,
};
use martensite::focus::FocusManager;
use wasm_bindgen::JsValue;

/// Reads `dev_token` from `location.search`. `None` disables the web
/// dev channel — without the per-run token the relay would refuse the
/// WebSocket anyway, and a dev build served publicly must not dial a
/// stranger's loopback on load.
fn dev_token() -> Option<String> {
    web_sys::window()
        .and_then(|window| window.location().search().ok())
        .filter(|search| search.len() > 1)
        .and_then(|search| {
            web_sys::UrlSearchParams::new_with_str(&search)
                .ok()
                .and_then(|params| params.get("dev_token"))
        })
        .filter(|token| !token.is_empty())
}

/// Strips `dev_token` from the address bar after [`dev_token`] has read
/// it — the bearer must not linger in `location.href` or the history
/// entry, where a copied or shared URL would leak it. Other query
/// parameters and the fragment are preserved. Best-effort: any DOM/JS
/// failure leaves the URL untouched rather than panicking.
fn scrub_dev_token_param() {
    let Some(window) = web_sys::window() else {
        return;
    };
    let location = window.location();
    let (Ok(search), Ok(pathname), Ok(hash)) =
        (location.search(), location.pathname(), location.hash())
    else {
        return;
    };
    let Ok(params) = web_sys::UrlSearchParams::new_with_str(&search) else {
        return;
    };
    params.delete("dev_token");
    let query: String = params.to_string().into();
    // A relative URL keeps `replaceState` same-origin by construction.
    let scrubbed = if query.is_empty() {
        format!("{pathname}{hash}")
    } else {
        format!("{pathname}?{query}{hash}")
    };
    if let Ok(history) = window.history() {
        let _ = history.replace_state_with_url(&JsValue::NULL, "", Some(&scrubbed));
    }
}

/// Opens the WebSocket leg to `cargo martensite dev-web` and wires the
/// dev-session dispatcher to it.
///
/// Returns `None` when the page URL carries no `?dev_token=` — the
/// feature stays compiled-in but inert. The channel itself is returned
/// so the caller can hold it for its lifetime (dropping it closes the
/// socket).
pub(crate) fn serve(
    arena: Arc<Mutex<WidgetArena>>,
    focus: Arc<Mutex<FocusManager>>,
    log_ring: Arc<martensite::devtools::dev_session::log_ring::LogRing>,
    signal_adapters: Vec<SignalAdapter>,
) -> Option<(Arc<DevSession>, WebDevChannel)> {
    let token = dev_token()?;
    scrub_dev_token_param();
    debug_assert!(
        dispatch_covers_method_table(),
        "web dev-channel DISPATCH drifted from DEV_CHANNEL_METHODS"
    );
    let session = Arc::new(DevSession::with_arena(arena));
    session.attach_focus_manager(focus);
    session.set_log_ring(log_ring);
    for adapter in signal_adapters {
        session.register_signal_adapter(adapter);
    }

    // Bearer enforcement on both layers (ADR-0042): the token in the
    // dial URL authenticates the page to the relay; `required_token`
    // makes the leg re-verify the relay-forwarded `Hello.auth_token`,
    // restoring end-to-end authentication on a client-opened socket.
    let options = WebChannelOptions::default()
        .with_required_token(token.clone())
        .with_build_id("widget_catalog-web");
    let session_for_handler = Arc::clone(&session);
    match WebDevChannel::serve_with_options(
        &relay_url(DEFAULT_RELAY_PORT, &token),
        options,
        move |line| process_request_line(&session_for_handler, line),
    ) {
        Ok(channel) => {
            crate::web::log(&format!(
                "web dev channel dialing relay on 127.0.0.1:{DEFAULT_RELAY_PORT}"
            ));
            Some((session, channel))
        }
        Err(err) => {
            crate::web::log(&format!("web dev channel unavailable: {err}"));
            None
        }
    }
}

/// A [`DevSession`] method handler: takes the raw `params` JSON and
/// answers with the result value or an error string.
type SessionMethod = fn(&DevSession, &serde_json::Value) -> SessionResult;

/// Normalized method name → [`DevSession`] call. One entry per name in
/// [`DEV_CHANNEL_METHODS`] — the canonical method list the wasm leg
/// shares with the host dispatcher; `serve()` debug-asserts the two
/// tables stay the same set, and the
/// `wasm_dev_channel_method_table_matches_host` test in
/// `cargo-martensite`'s `web_relay` module fails a host-side RPC added
/// without updating the list (which is what would otherwise silently
/// `-32601` on the web).
const DISPATCH: &[(&str, SessionMethod)] = &[
    ("a11yaction", DevSession::a11y_action),
    ("a11ytree", DevSession::a11y_tree),
    ("auditpaint", DevSession::audit_paint),
    ("capturenode", DevSession::capture_node),
    ("eventdispatch", DevSession::event_dispatch),
    ("eventledger", DevSession::event_ledger),
    ("hotreload", DevSession::hot_reload),
    ("inspectorselect", DevSession::inspector_select),
    ("layoutchain", DevSession::layout_chain),
    ("lintapply", DevSession::lint_apply),
    ("lintpull", DevSession::lint_pull),
    ("lintscene", DevSession::lint_pull),
    ("logs", DevSession::logs),
    ("nodesetloading", DevSession::node_set_loading),
    ("overflowscan", DevSession::overflow_scan),
    ("reloadstatus", DevSession::reload_status),
    ("runtimeerrors", DevSession::runtime_errors),
    ("signalset", DevSession::signal_set),
    ("signalslist", DevSession::signals_list),
    ("signaltrigger", DevSession::signal_trigger),
    ("themeget", DevSession::theme_get),
    ("themeset", DevSession::theme_set),
    ("timemachinestep", DevSession::timemachine_step),
    ("treenode", DevSession::tree_node),
    ("treesnapshot", DevSession::tree_snapshot),
    ("tweakset", DevSession::tweak_set),
    ("tweakslist", DevSession::tweaks_list),
    ("tweakssync", DevSession::tweaks_sync),
];

/// `true` when [`DISPATCH`] and [`DEV_CHANNEL_METHODS`] name the same
/// method set — checked in [`serve`] under `debug_assertions` so a
/// debug wasm build fails loudly at startup on drift.
#[cfg(debug_assertions)]
fn dispatch_covers_method_table() -> bool {
    DEV_CHANNEL_METHODS
        .iter()
        .all(|m| DISPATCH.iter().any(|(name, _)| name == m))
        && DISPATCH
            .iter()
            .all(|(name, _)| DEV_CHANNEL_METHODS.contains(name))
}

/// Post-handshake JSON-RPC dispatch onto [`DevSession`].
///
/// Mirrors `martensite_host::dev_channel`'s dispatcher: the method is
/// normalized (lowercase, `_`/`-`/space stripped) and the `params`
/// payload passes straight to the session handler — session methods
/// already take `&serde_json::Value` and validate internally, so the
/// host path's typed-param round-trip is redundant here.
/// `not_implemented:*` maps to `-32601`; any other session error maps
/// to `-32603`.
fn process_request_line(session: &DevSession, line: &str) -> Option<String> {
    let req: serde_json::Value = match serde_json::from_str(line) {
        Ok(req) => req,
        Err(err) => {
            return Some(error_line(
                serde_json::Value::Null,
                -32700,
                &format!("parse error: {err}"),
            ));
        }
    };
    let id = req.get("id").cloned().unwrap_or(serde_json::Value::Null);
    let method = req
        .get("method")
        .and_then(serde_json::Value::as_str)
        .unwrap_or("")
        .to_ascii_lowercase()
        .replace(['_', '-', ' '], "");
    let params = req
        .get("params")
        .cloned()
        .unwrap_or(serde_json::Value::Null);

    // The transport leg owns `hello` (it stays reachable after the
    // handshake, matching the host dispatcher); it never reaches this
    // table — `DEV_CHANNEL_METHODS` deliberately omits it.
    let result = DISPATCH
        .iter()
        .find(|(name, _)| *name == method)
        .map(|(_, handler)| handler(session, &params));

    match result {
        // Notifications carry no `id`; method-not-found keeps the
        // `-32601` contract `handle_custom` applies on the host path.
        None => Some(error_line(
            id,
            -32601,
            &format!("method not found: {method}"),
        )),
        Some(Ok(value)) => Some(
            serde_json::json!({
                "jsonrpc": "2.0",
                "id": id,
                "result": value,
            })
            .to_string(),
        ),
        Some(Err(message)) => {
            let code = if message.starts_with("not_implemented:") {
                -32601
            } else {
                -32603
            };
            Some(error_line(id, code, &message))
        }
    }
}

/// Serializes a JSON-RPC error response line in the same shape the
/// host `JsonRpcResponse::error` produces.
fn error_line(id: serde_json::Value, code: i32, message: &str) -> String {
    serde_json::json!({
        "jsonrpc": "2.0",
        "id": id,
        "error": { "code": code, "message": message },
    })
    .to_string()
}
