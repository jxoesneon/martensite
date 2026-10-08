//! Browser-side dev-channel transport leg (ADR-0042).
//!
//! `martensite-host`'s `DevChannelServer` binds a Unix domain socket, which
//! a page running under `wasm32-unknown-unknown` can neither create nor
//! accept. This module provides the reversed transport: the wasm app dials
//! **out** to the `cargo martensite dev-web` relay over
//! `ws://127.0.0.1:<port>/dev-channel?token=<bearer>` and then *serves* the
//! dev-channel protocol — the same newline-delimited JSON-RPC lines the
//! Unix socket carries — over the client-initiated WebSocket.
//!
//! The leg owns the connection lifecycle and the mandatory `Hello` gate
//! (ordering, version negotiation, and the optional `auth_token` bearer).
//! Every post-handshake request line is handed to the consumer's
//! dispatcher, which returns the response line to send back.
//! `martensite-host` is host-only, so wiring the dispatcher to a
//! `DevSession`-backed `DevChannelHandler` equivalent is the consumer's
//! responsibility — the leg only speaks the framed protocol.
//!
//! Compiled only for `wasm32-unknown-unknown` with the `web-dev-channel`
//! feature; release and gh-pages builds without the feature contain none
//! of this code.

use std::cell::{Cell, RefCell};
use std::rc::Rc;

use wasm_bindgen::prelude::Closure;
use wasm_bindgen::JsCast;

/// Default TCP port the `cargo martensite dev-web` relay binds on
/// `127.0.0.1` when `--port` is not supplied.
pub const DEFAULT_RELAY_PORT: u16 = 8788;

/// WebSocket path the relay serves the dev channel on.
pub const RELAY_PATH: &str = "/dev-channel";

/// `Hello` rejected because a request arrived before the handshake —
/// mirrors `martensite-host`'s `ERR_HANDSHAKE_REQUIRED`.
pub const ERR_HANDSHAKE_REQUIRED: i32 = -32000;
/// `Hello` rejected on version/protocol mismatch — mirrors
/// `martensite-host`'s `ERR_VERSION_MISMATCH`.
pub const ERR_VERSION_MISMATCH: i32 = -32001;
/// `Hello` rejected for a missing or wrong bearer token — mirrors
/// `martensite-host`'s `ERR_AUTH_FAILED`.
pub const ERR_AUTH_FAILED: i32 = -32002;
/// Request line could not be parsed as JSON-RPC — mirrors JSON-RPC
/// `-32700`.
pub const ERR_PARSE: i32 = -32700;

/// The normalized method names a `WebDevChannel` consumer's
/// post-handshake dispatcher serves — the canonical parity list for the
/// wasm leg's method table.
///
/// Method names are normalized exactly like the host dispatcher:
/// lowercased with `_`, `-`, and space stripped (`"TreeSnapshot"` →
/// `"treesnapshot"`). This is the wasm-side mirror of
/// `martensite-host`'s `METHOD_CLASSES` key set minus `"hello"`, which
/// the leg itself owns at the handshake gate. `METHOD_CLASSES` is
/// private and this module is wasm-gated, so set parity is enforced
/// where both surfaces are visible: the
/// `wasm_dev_channel_method_table_matches_host` test in
/// `cargo-martensite`'s `web_relay` module (host side) extracts both
/// tables from source and asserts they are equal — adding an RPC to the
/// host without updating this list fails that test.
///
/// # Examples
///
/// ```
/// use martensite_devtools::web_channel::DEV_CHANNEL_METHODS;
///
/// assert!(DEV_CHANNEL_METHODS.contains(&"treesnapshot"));
/// assert!(!DEV_CHANNEL_METHODS.contains(&"hello"));
/// ```
pub const DEV_CHANNEL_METHODS: &[&str] = &[
    "a11yaction",
    "a11ytree",
    "auditpaint",
    "capturenode",
    "eventdispatch",
    "eventledger",
    "hotreload",
    "inspectorselect",
    "layoutchain",
    "lintapply",
    "lintpull",
    "lintscene",
    "logs",
    "nodesetloading",
    "overflowscan",
    "reloadstatus",
    "runtimeerrors",
    "signalset",
    "signalslist",
    "signaltrigger",
    "themeget",
    "themeset",
    "timemachinestep",
    "treenode",
    "treesnapshot",
    "tweakset",
    "tweakslist",
    "tweakssync",
];

/// Builds the relay WebSocket URL for `port`, embedding the operator's
/// per-run bearer `token` as the `token` query parameter.
///
/// The relay prints the token to the console when it starts; the operator
/// hands it to the page (e.g. a `?dev_token=` location parameter the app
/// reads and passes here).
///
/// # Examples
///
/// ```
/// use martensite_devtools::web_channel::relay_url;
///
/// assert_eq!(
///     relay_url(8788, "deadbeef"),
///     "ws://127.0.0.1:8788/dev-channel?token=deadbeef"
/// );
/// ```
pub fn relay_url(port: u16, token: &str) -> String {
    format!("ws://127.0.0.1:{port}{RELAY_PATH}?token={token}")
}

/// Errors produced while opening or driving the relay WebSocket.
///
/// # Examples
///
/// ```
/// use martensite_devtools::web_channel::WebChannelError;
///
/// let err = WebChannelError::Closed;
/// assert!(err.to_string().contains("closed"));
/// ```
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum WebChannelError {
    /// `WebSocket::new` rejected the URL or the constructor threw.
    Connect(String),
    /// `WebSocket.send` failed.
    Send(String),
    /// The socket reached a terminal state (close or error event).
    Closed,
}

impl std::fmt::Display for WebChannelError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            WebChannelError::Connect(msg) => write!(f, "websocket connect failed: {msg}"),
            WebChannelError::Send(msg) => write!(f, "websocket send failed: {msg}"),
            WebChannelError::Closed => write!(f, "dev channel websocket closed"),
        }
    }
}

impl std::error::Error for WebChannelError {}

/// Options controlling the dev-channel `Hello` gate served by
/// [`WebDevChannel`].
///
/// # Examples
///
/// ```
/// use martensite_devtools::web_channel::WebChannelOptions;
///
/// let opts = WebChannelOptions::default()
///     .with_required_token("per-run-bearer");
/// assert_eq!(opts.required_token.as_deref(), Some("per-run-bearer"));
/// ```
#[derive(Clone)]
pub struct WebChannelOptions {
    /// Version string echoed in `HelloResult.server_version` and enforced
    /// against the client's `client_version` (Constraint D1). Defaults to
    /// this crate's version — correct whenever the app and
    /// `cargo-martensite` are built from the same workspace release.
    pub server_version: String,
    /// Protocol version echoed in `HelloResult.protocol_version` and
    /// enforced against the client's `protocol_version`.
    pub protocol_version: u32,
    /// Optional bearer token the `Hello` must carry as
    /// `params.auth_token`. The relay injects the operator token into
    /// forwarded handshakes, so setting this to the relay token restores
    /// end-to-end authentication even though the socket is client-opened.
    pub required_token: Option<String>,
    /// Optional build/session identifier echoed in `HelloResult.build_id`.
    pub build_id: Option<String>,
}

// `required_token` is the bearer — Debug redacts it so a logged options
// dump cannot leak the credential.
impl std::fmt::Debug for WebChannelOptions {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("WebChannelOptions")
            .field("server_version", &self.server_version)
            .field("protocol_version", &self.protocol_version)
            .field(
                "required_token",
                &self.required_token.as_ref().map(|_| "[redacted]"),
            )
            .field("build_id", &self.build_id)
            .finish()
    }
}

impl Default for WebChannelOptions {
    fn default() -> Self {
        Self {
            server_version: env!("CARGO_PKG_VERSION").to_string(),
            protocol_version: 1,
            required_token: None,
            build_id: None,
        }
    }
}

impl WebChannelOptions {
    /// Requires `Hello` handshakes to present `token` as
    /// `params.auth_token`.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite_devtools::web_channel::WebChannelOptions;
    ///
    /// let opts = WebChannelOptions::default().with_required_token("tok");
    /// assert!(opts.required_token.is_some());
    /// ```
    pub fn with_required_token(mut self, token: impl Into<String>) -> Self {
        self.required_token = Some(token.into());
        self
    }

    /// Sets the build/session identifier echoed in `HelloResult.build_id`.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite_devtools::web_channel::WebChannelOptions;
    ///
    /// let opts = WebChannelOptions::default().with_build_id("build_7");
    /// assert_eq!(opts.build_id.as_deref(), Some("build_7"));
    /// ```
    pub fn with_build_id(mut self, build_id: impl Into<String>) -> Self {
        self.build_id = Some(build_id.into());
        self
    }
}

/// Evaluates a `Hello` request line against `opts`, mirroring
/// `martensite-host`'s handshake arm.
///
/// Returns `Some(line)` with the JSON-RPC response the leg must send back,
/// plus whether the handshake now counts as established. `None` means the
/// line was not valid JSON-RPC at all and is answered with a parse error.
fn evaluate_hello(line: &str, opts: &WebChannelOptions) -> (Option<String>, bool) {
    let Ok(req) = serde_json::from_str::<serde_json::Value>(line) else {
        return (
            Some(error_line(
                None,
                ERR_PARSE,
                "parse error: invalid JSON-RPC payload",
            )),
            false,
        );
    };
    let id = req.get("id").cloned().unwrap_or(serde_json::Value::Null);
    let method = req
        .get("method")
        .and_then(serde_json::Value::as_str)
        .unwrap_or("")
        .to_ascii_lowercase()
        .replace(['_', '-', ' '], "");

    if method != "hello" {
        return (
            Some(error_line(
                Some(id),
                ERR_HANDSHAKE_REQUIRED,
                "handshake required: the first request on a dev channel \
                 connection must be 'Hello'",
            )),
            false,
        );
    }

    let params = req
        .get("params")
        .cloned()
        .unwrap_or(serde_json::Value::Null);
    let client_version = params
        .get("client_version")
        .or_else(|| params.get("version"))
        .and_then(serde_json::Value::as_str)
        .unwrap_or("");
    let protocol_version = params
        .get("protocol_version")
        .and_then(serde_json::Value::as_u64)
        .unwrap_or(0);
    let auth_token = params.get("auth_token").and_then(serde_json::Value::as_str);

    if let Some(expected) = opts.required_token.as_deref() {
        if auth_token != Some(expected) {
            return (
                Some(error_line(
                    Some(id),
                    ERR_AUTH_FAILED,
                    "auth_failed: this dev channel session requires a bearer token",
                )),
                false,
            );
        }
    }

    if client_version != opts.server_version || protocol_version != opts.protocol_version as u64 {
        return (
            Some(error_line(
                Some(id),
                ERR_VERSION_MISMATCH,
                &format!(
                    "version_mismatch: client is v{client_version} (protocol \
                     {protocol_version}) but server is v{} (protocol {})",
                    opts.server_version, opts.protocol_version
                ),
            )),
            false,
        );
    }

    let result = serde_json::json!({
        "jsonrpc": "2.0",
        "id": id,
        "result": {
            "server_version": opts.server_version,
            "protocol_version": opts.protocol_version,
            "build_id": opts.build_id,
        },
    });
    (Some(result.to_string()), true)
}

/// Serializes a JSON-RPC error response line.
fn error_line(id: Option<serde_json::Value>, code: i32, message: &str) -> String {
    serde_json::json!({
        "jsonrpc": "2.0",
        "id": id.unwrap_or(serde_json::Value::Null),
        "error": { "code": code, "message": message },
    })
    .to_string()
}

/// A dev-channel server endpoint hosted in the browser over a
/// client-initiated WebSocket to the `cargo martensite dev-web` relay
/// (ADR-0042).
///
/// The WebSocket is opened by [`serve`](Self::serve); incoming text frames
/// are split on newlines into dev-channel request lines. `Hello` lines are
/// answered by the leg itself per [`WebChannelOptions`]; all other lines —
/// once the handshake has completed — are passed to the consumer's
/// dispatcher closure, which returns the response line (or `None` for
/// notifications and unsolicited output).
///
/// Dropping the value closes the socket.
///
/// # Examples
///
/// ```no_run
/// use martensite_devtools::web_channel::{relay_url, WebDevChannel};
///
/// // `handler` turns a request line into a response line — wire it to the
/// // app's dev-channel dispatcher.
/// let channel = WebDevChannel::serve(&relay_url(8788, "token"), |_line| None);
/// ```
pub struct WebDevChannel {
    ws: web_sys::WebSocket,
    closed: Rc<Cell<bool>>,
    // Closures are retained so their `Rc` targets outlive the registration.
    _on_message: Closure<dyn FnMut(web_sys::MessageEvent)>,
    _on_close: Closure<dyn FnMut(web_sys::CloseEvent)>,
    _on_error: Closure<dyn FnMut(web_sys::Event)>,
}

impl std::fmt::Debug for WebDevChannel {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        // `ws.url()` embeds `?token=<bearer>` — print host/path only so a
        // Debug dump cannot leak the credential.
        let url = self.ws.url();
        let safe_url = url.split(['?', '#']).next().unwrap_or(&url);
        f.debug_struct("WebDevChannel")
            .field("url", &safe_url)
            .field("closed", &self.closed.get())
            .finish()
    }
}

impl WebDevChannel {
    /// Opens a WebSocket to `url` and serves dev-channel lines with default
    /// [`WebChannelOptions`].
    ///
    /// `handler` receives each post-handshake request line and returns the
    /// response line to transmit, or `None` to send nothing (e.g. for
    /// notifications).
    ///
    /// # Examples
    ///
    /// ```no_run
    /// use martensite_devtools::web_channel::{relay_url, WebDevChannel, DEFAULT_RELAY_PORT};
    ///
    /// let channel = WebDevChannel::serve(&relay_url(DEFAULT_RELAY_PORT, "tok"), |_| None);
    /// ```
    pub fn serve<F>(url: &str, handler: F) -> Result<Self, WebChannelError>
    where
        F: FnMut(&str) -> Option<String> + 'static,
    {
        Self::serve_with_options(url, WebChannelOptions::default(), handler)
    }

    /// Opens a WebSocket to `url` and serves dev-channel lines gated by
    /// `opts`.
    ///
    /// Until a `Hello` has completed (version match, and bearer-token match
    /// when [`WebChannelOptions::required_token`] is set), non-`Hello`
    /// lines are rejected with a `handshake_required` error and `handler`
    /// is never invoked.
    ///
    /// # Examples
    ///
    /// ```no_run
    /// use martensite_devtools::web_channel::{
    ///     relay_url, WebChannelOptions, WebDevChannel,
    /// };
    ///
    /// let channel = WebDevChannel::serve_with_options(
    ///     &relay_url(8788, "tok"),
    ///     WebChannelOptions::default().with_required_token("tok"),
    ///     |_| None,
    /// );
    /// ```
    pub fn serve_with_options<F>(
        url: &str,
        opts: WebChannelOptions,
        handler: F,
    ) -> Result<Self, WebChannelError>
    where
        F: FnMut(&str) -> Option<String> + 'static,
    {
        let ws =
            web_sys::WebSocket::new(url).map_err(|e| WebChannelError::Connect(format!("{e:?}")))?;

        let closed = Rc::new(Cell::new(false));
        let handshook = Rc::new(Cell::new(false));
        let handler: Rc<RefCell<F>> = Rc::new(RefCell::new(handler));
        let opts = Rc::new(opts);

        let on_message = {
            let ws = ws.clone();
            let handshook = Rc::clone(&handshook);
            let handler = Rc::clone(&handler);
            let opts = Rc::clone(&opts);
            Closure::new(move |ev: web_sys::MessageEvent| {
                let Some(text) = ev.data().as_string() else {
                    return;
                };
                for raw in text.split('\n') {
                    let line = raw.trim();
                    if line.is_empty() {
                        continue;
                    }
                    // `Hello` lines are always owned by the leg's handshake
                    // gate — including repeats after a completed handshake,
                    // matching the host dispatcher where `hello` stays
                    // reachable.
                    let is_hello = !handshook.get() || line_is_hello(line);
                    let reply = if is_hello {
                        let (resp, ok) = evaluate_hello(line, &opts);
                        // Host parity (dev_channel.rs): a *failed* re-`Hello`
                        // — bad bearer or version mismatch on an already-
                        // handshook leg — revokes the handshake, so the next
                        // non-`Hello` line answers `handshake_required` again.
                        handshook.set(ok);
                        resp
                    } else {
                        (handler.borrow_mut())(line)
                    };
                    if let Some(out) = reply {
                        let _ = ws.send_with_str(&out);
                    }
                }
            })
        };
        ws.set_onmessage(Some(on_message.as_ref().unchecked_ref()));

        let on_close = {
            let closed = Rc::clone(&closed);
            Closure::new(move |_: web_sys::CloseEvent| closed.set(true))
        };
        ws.set_onclose(Some(on_close.as_ref().unchecked_ref()));

        let on_error = {
            let closed = Rc::clone(&closed);
            Closure::new(move |_: web_sys::Event| closed.set(true))
        };
        ws.set_onerror(Some(on_error.as_ref().unchecked_ref()));

        Ok(Self {
            ws,
            closed,
            _on_message: on_message,
            _on_close: on_close,
            _on_error: on_error,
        })
    }

    /// Sends one framed dev-channel line back over the socket.
    ///
    /// Most consumers never need this — [`serve`](Self::serve) replies
    /// automatically. It exists for server-initiated traffic the protocol
    /// may grow (e.g. notifications).
    ///
    /// # Examples
    ///
    /// ```no_run
    /// use martensite_devtools::web_channel::{relay_url, WebDevChannel};
    ///
    /// let channel = WebDevChannel::serve(&relay_url(8788, "tok"), |_| None).unwrap();
    /// channel.send_line("{\"jsonrpc\":\"2.0\",\"method\":\"note\"}");
    /// ```
    pub fn send_line(&self, line: &str) -> Result<(), WebChannelError> {
        if self.closed.get() {
            return Err(WebChannelError::Closed);
        }
        self.ws
            .send_with_str(line)
            .map_err(|e| WebChannelError::Send(format!("{e:?}")))
    }

    /// Whether the underlying socket has reached a terminal state.
    ///
    /// # Examples
    ///
    /// ```no_run
    /// use martensite_devtools::web_channel::{relay_url, WebDevChannel};
    ///
    /// let channel = WebDevChannel::serve(&relay_url(8788, "tok"), |_| None).unwrap();
    /// let _ = channel.is_closed();
    /// ```
    pub fn is_closed(&self) -> bool {
        self.closed.get()
    }
}

/// Reports whether `line` parses as a JSON-RPC request whose method
/// normalizes to `hello` — used to keep the leg-owned handshake reachable
/// after the first handshake completed.
fn line_is_hello(line: &str) -> bool {
    serde_json::from_str::<serde_json::Value>(line)
        .ok()
        .and_then(|v| v.get("method").and_then(|m| m.as_str()).map(String::from))
        .map(|m| m.to_ascii_lowercase().replace(['_', '-', ' '], "") == "hello")
        .unwrap_or(false)
}

impl Drop for WebDevChannel {
    fn drop(&mut self) {
        let _ = self.ws.close();
    }
}
