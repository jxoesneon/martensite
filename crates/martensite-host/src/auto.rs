//! Env-gated dev-channel auto-enable (ADR-0039 D8).
//!
//! [`serve_dev_session_from_env`] is the app-side one-liner: when the
//! `MARTENSITE_DEV_CHANNEL` environment variable is set to a truthy value
//! in a debug build, it constructs a
//! [`DevSession`](martensite_devtools::dev_session::DevSession) over the
//! app's widget arena and binds the ADR-0038 dev-channel socket so
//! `cargo-martensite` attach mode and MCP tooling can inspect the running
//! process.
//!
//! In release builds (`debug_assertions` off) the function is a no-op that
//! always returns `Ok(None)` — dev-channel machinery never ships in
//! production binaries (ADR-0039 D8).

use std::fmt;
use std::io;
use std::sync::{Arc, Mutex};

use crate::dev_channel::DevChannelServer;

/// Errors produced by [`serve_dev_session_from_env`].
///
/// # Examples
///
/// ```
/// use martensite_host::auto::DevChannelError;
///
/// let err = DevChannelError::Bind(std::io::Error::new(
///     std::io::ErrorKind::AddrInUse,
///     "address in use",
/// ));
/// assert!(err.to_string().contains("dev channel"));
/// ```
#[derive(Debug)]
pub enum DevChannelError {
    /// The dev-channel socket could not be bound or the accept loop could
    /// not be started.
    Bind(io::Error),
}

impl fmt::Display for DevChannelError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            DevChannelError::Bind(err) => {
                write!(f, "failed to bind dev channel socket: {err}")
            }
        }
    }
}

impl std::error::Error for DevChannelError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            DevChannelError::Bind(err) => Some(err),
        }
    }
}

impl From<io::Error> for DevChannelError {
    fn from(err: io::Error) -> Self {
        DevChannelError::Bind(err)
    }
}

/// Returns `true` when `value` opts the process into the dev channel.
///
/// Accepted truthy spellings (case-insensitive): `1`, `true`, `on`, `yes`.
/// Everything else — including the empty string — leaves the channel off.
#[cfg(debug_assertions)]
fn dev_channel_enabled(value: &str) -> bool {
    matches!(
        value.trim().to_ascii_lowercase().as_str(),
        "1" | "true" | "on" | "yes"
    )
}

/// Serves the ADR-0038 dev channel for `arena` when the environment opts in.
///
/// # Environment contract
///
/// - `MARTENSITE_DEV_CHANNEL` — set to `1`, `true`, `on`, or `yes`
///   (case-insensitive) to bind the dev-channel socket. Unset, empty, or
///   any other value leaves the channel off and returns `Ok(None)`.
/// - `MARTENSITE_BUILD_ID` — optional build/session identifier used for the
///   socket filename (`<build_id>.sock`); defaults to `dev-<pid>` so
///   parallel debug sessions never collide.
///
/// The socket path itself is derived from the build id via
/// [`socket_path_for_session`](crate::dev_channel::socket_path_for_session)
/// (`$XDG_RUNTIME_DIR/martensite/` on Unix, `\\.\pipe\` on Windows);
/// `MARTENSITE_DEV_SOCKET` is honored by dev-channel *clients* during
/// discovery, not by the server.
///
/// Release builds (`debug_assertions` disabled) always return `Ok(None)`
/// regardless of the environment — the channel compiles to a no-op per
/// ADR-0039 D8.
///
/// The returned [`DevChannelServer`] owns the accept-loop thread; keep it
/// alive for the app's lifetime (dropping it unlinks the socket). The
/// accompanying [`DevSession`](martensite_devtools::dev_session::DevSession)
/// handle is what the app feeds per frame — `on_frame(&paint_list)`,
/// `absorb_events(router.event_ledger())`, and `record_reload(build_id)`
/// after each hot reload — to populate the lint/event/reload surfaces.
///
/// # Examples
///
/// ```no_run
/// use std::sync::{Arc, Mutex};
///
/// use martensite_core::WidgetArena;
/// use martensite_host::auto::serve_dev_session_from_env;
///
/// let arena = Arc::new(Mutex::new(WidgetArena::new()));
/// // Returns Ok(None) unless MARTENSITE_DEV_CHANNEL is truthy.
/// let served = serve_dev_session_from_env(arena).expect("dev channel");
/// if let Some((server, session)) = served {
///     // Hold `server` for the app's lifetime; feed `session` per frame.
///     drop(server);
/// }
/// ```
pub fn serve_dev_session_from_env(
    arena: Arc<Mutex<martensite_core::WidgetArena>>,
) -> Result<
    Option<(
        DevChannelServer,
        Arc<martensite_devtools::dev_session::DevSession>,
    )>,
    DevChannelError,
> {
    #[cfg(not(debug_assertions))]
    {
        let _ = arena;
        Ok(None)
    }
    #[cfg(debug_assertions)]
    {
        let enabled = std::env::var("MARTENSITE_DEV_CHANNEL")
            .map(|value| dev_channel_enabled(&value))
            .unwrap_or(false);
        if !enabled {
            return Ok(None);
        }
        let build_id = std::env::var("MARTENSITE_BUILD_ID")
            .ok()
            .filter(|value| !value.trim().is_empty())
            .unwrap_or_else(|| format!("dev-{}", std::process::id()));
        let session = Arc::new(martensite_devtools::dev_session::DevSession::with_arena(
            arena,
        ));
        crate::session_handler::serve_dev_session(Arc::clone(&session), &build_id)
            .map(|server| Some((server, session)))
            .map_err(DevChannelError::Bind)
    }
}
