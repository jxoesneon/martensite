//! Wayland clipboard backend using `wl-clipboard` as a subprocess.
//!
//! This module provides clipboard support for Wayland compositors by
//! delegating to the `wl-clipboard` command-line utility, which is the
//! standard approach for applications that do not implement the full
//! Wayland data-device protocol inline. This avoids linking against
//! `libwayland-client` and the `wl_data_device_manager` protocol boilerplate.
//!
//! # Requirements
//!
//! The `wl-clipboard` binary must be installed and on `$PATH`. On most
//! Linux distributions it is available as `wl-clipboard` or `wlclipboard`.
//!
//! # Detection
//!
//! The backend is selected when the `WAYLAND_DISPLAY` environment variable
//! is set *and* the compositor socket it names actually exists, guarding
//! against a stale `WAYLAND_DISPLAY` left over from a dead session. Both
//! halves of `wl-clipboard` (`wl-copy` and `wl-paste`) must be installed;
//! if either check fails, [`WaylandBackend::new`] returns an error and the
//! factory falls back to the X11 backend.
//!
//! # Timeouts
//!
//! Every subprocess invocation is bounded: `wl-paste` asks the current
//! selection owner for its data over the Wayland wire, and a hung owner
//! would otherwise block `read()` forever. Children that outlive
//! `COMMAND_TIMEOUT` are killed. This mirrors the X11 backend, which
//! bounds its `SelectionNotify` wait to a ~100 ms poll budget.
//!
//! # Safety
//!
//! This module contains no `unsafe` code. All clipboard operations are
//! performed via subprocess invocations of `wl-clipboard`.

use std::process::{Child, Command, ExitStatus};
use std::time::{Duration, Instant};

use crate::ClipboardBackend;

/// Maximum time to wait for a `wl-clipboard` subprocess to exit before
/// killing it.
///
/// A hung selection owner can leave `wl-paste` blocked on `read()`
/// forever, so every wait goes through [`wait_with_timeout`]. The payload
/// itself is drained concurrently (see [`capture_stdout`]), so the
/// deadline only guards against a stuck peer — not against large but
/// well-behaved transfers.
const COMMAND_TIMEOUT: Duration = Duration::from_millis(500);

/// Interval between `try_wait` polls while waiting on a subprocess.
const POLL_INTERVAL: Duration = Duration::from_millis(10);

/// Errors that can occur when initializing the Wayland clipboard backend.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum WaylandError {
    /// The `WAYLAND_DISPLAY` environment variable is not set, or the
    /// compositor socket it names does not exist (stale display).
    NoDisplay,
    /// The `wl-clipboard` binaries (`wl-copy`/`wl-paste`) were not found
    /// on `$PATH` or failed to run.
    NoClipboard,
}

impl std::fmt::Display for WaylandError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            WaylandError::NoDisplay => {
                write!(f, "WAYLAND_DISPLAY is not set or its socket is missing")
            }
            WaylandError::NoClipboard => {
                write!(
                    f,
                    "wl-clipboard binaries (wl-copy/wl-paste) not found on PATH"
                )
            }
        }
    }
}

impl std::error::Error for WaylandError {}

/// Resolves the Wayland display socket path for `WAYLAND_DISPLAY`.
///
/// An absolute `WAYLAND_DISPLAY` is used as-is; a relative socket name is
/// resolved against `XDG_RUNTIME_DIR`, matching `libwayland-client`
/// behavior. Returns `None` when `WAYLAND_DISPLAY` is unset or a relative
/// name cannot be resolved because `XDG_RUNTIME_DIR` is unset.
fn display_socket() -> Option<std::path::PathBuf> {
    let display = std::path::PathBuf::from(std::env::var_os("WAYLAND_DISPLAY")?);
    // An empty WAYLAND_DISPLAY is a dead display, not a relative name —
    // joining it would yield XDG_RUNTIME_DIR itself (which exists).
    if display.as_os_str().is_empty() {
        return None;
    }
    if display.is_absolute() {
        Some(display)
    } else {
        let runtime_dir = std::path::PathBuf::from(std::env::var_os("XDG_RUNTIME_DIR")?);
        Some(runtime_dir.join(display))
    }
}

/// Waits for `child` to exit, returning its [`ExitStatus`].
///
/// If the child is still running after `timeout`, it is killed and reaped
/// and `None` is returned. `Err` from `try_wait` is treated as a failed
/// wait (`None`).
fn wait_with_timeout(child: &mut Child, timeout: Duration) -> Option<ExitStatus> {
    let deadline = Instant::now() + timeout;
    loop {
        match child.try_wait() {
            Ok(Some(status)) => return Some(status),
            Ok(None) => {
                if Instant::now() >= deadline {
                    let _ = child.kill();
                    let _ = child.wait();
                    return None;
                }
                std::thread::sleep(POLL_INTERVAL);
            }
            Err(_) => {
                // OS-level error polling the child — still attempt to reap
                // it so we don't leave a zombie.
                let _ = child.kill();
                let _ = child.wait();
                return None;
            }
        }
    }
}

/// Runs `command` with a bounded wait, returning the captured stdout when
/// the child exits successfully.
///
/// stdout is drained on a helper thread so that a payload larger than the
/// OS pipe buffer cannot deadlock the child while we poll `try_wait`. On
/// spawn failure, non-zero exit, or timeout (in which case the child is
/// killed) `None` is returned.
fn capture_stdout(command: &mut Command, timeout: Duration) -> Option<Vec<u8>> {
    use std::io::Read;
    let mut child = command
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::null())
        .spawn()
        .ok()?;
    let Some(mut stdout) = child.stdout.take() else {
        let _ = child.kill();
        let _ = child.wait();
        return None;
    };
    let (tx, rx) = std::sync::mpsc::channel();
    let _reader = std::thread::spawn(move || {
        let mut buf = Vec::new();
        // The pipe hits EOF when the child exits (or is killed on
        // timeout), so this thread always winds down on its own even
        // if we do not join it below.
        let _ = stdout.read_to_end(&mut buf);
        let _ = tx.send(buf);
    });
    let status = wait_with_timeout(&mut child, timeout)?;
    if !status.success() {
        return None;
    }
    // recv_timeout bounds the join: a grandchild that inherited the
    // stdout write fd would otherwise hold the pipe past EOF and block
    // `join` forever. On timeout the thread is detached; it exits when
    // the inherited fd finally closes.
    rx.recv_timeout(timeout).ok()
}

/// Returns `true` if `binary` runs `--version` successfully within
/// [`COMMAND_TIMEOUT`].
fn probe(binary: &str) -> bool {
    Command::new(binary)
        .arg("--version")
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .spawn()
        .map(|mut child| {
            wait_with_timeout(&mut child, COMMAND_TIMEOUT)
                .map(|status| status.success())
                .unwrap_or(false)
        })
        .unwrap_or(false)
}

/// Returns `true` if both halves of `wl-clipboard` (`wl-copy` and
/// `wl-paste`) are installed and runnable.
fn wl_clipboard_installed() -> bool {
    probe("wl-copy") && probe("wl-paste")
}

/// Wayland clipboard backend using `wl-clipboard` as a subprocess.
///
/// # Examples
///
/// ```no_run
/// use martensite_clipboard_platform::ClipboardBackend;
/// use martensite_clipboard_platform::wayland::WaylandBackend;
///
/// if std::env::var("WAYLAND_DISPLAY").is_ok() {
///     if let Ok(mut backend) = WaylandBackend::new() {
///         backend.write("text/plain;charset=utf-8", b"hello");
///         let read = backend.read("text/plain;charset=utf-8");
///         assert_eq!(read, Some(b"hello".to_vec()));
///     }
/// }
/// ```
pub struct WaylandBackend;

impl WaylandBackend {
    /// Creates a new Wayland clipboard backend.
    ///
    /// Returns an error if `WAYLAND_DISPLAY` is not set, if the compositor
    /// socket it names does not exist (stale display), or if `wl-copy` /
    /// `wl-paste` are not installed.
    ///
    /// # Examples
    ///
    /// ```no_run
    /// use martensite_clipboard_platform::wayland::WaylandBackend;
    ///
    /// let backend = WaylandBackend::new();
    /// // On Wayland: Ok(WaylandBackend { ... })
    /// // On X11 or without wl-clipboard: Err(WaylandError)
    /// ```
    pub fn new() -> Result<Self, WaylandError> {
        // Reject a stale `WAYLAND_DISPLAY`: when the compositor socket is
        // gone every wl-clipboard call would block on a dead connection.
        match display_socket() {
            Some(socket) if socket.exists() => {}
            _ => return Err(WaylandError::NoDisplay),
        }
        // Verify both halves of wl-clipboard are installed.
        if !wl_clipboard_installed() {
            return Err(WaylandError::NoClipboard);
        }
        Ok(Self)
    }

    /// Checks if the Wayland clipboard backend is available.
    ///
    /// Returns `true` if `WAYLAND_DISPLAY` is set, its compositor socket
    /// exists, and `wl-clipboard` is installed.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite_clipboard_platform::wayland::WaylandBackend;
    ///
    /// let available = WaylandBackend::is_available();
    /// // On Wayland with wl-clipboard: true
    /// // On X11 or without wl-clipboard: false
    /// ```
    pub fn is_available() -> bool {
        display_socket()
            .map(|socket| socket.exists())
            .unwrap_or(false)
            && wl_clipboard_installed()
    }
}

impl ClipboardBackend for WaylandBackend {
    fn write(&mut self, mime: &str, bytes: &[u8]) {
        // wl-copy reads from stdin and copies to the Wayland clipboard.
        let _ = Command::new("wl-copy")
            .arg("--type")
            .arg(mime)
            .stdin(std::process::Stdio::piped())
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .spawn()
            .map(|mut child| {
                use std::io::Write;
                // `take` moves stdin out of the child; the writer thread
                // drops it after writing so wl-copy observes EOF. Without
                // this, stdin stays open in `child` while `wait` blocks
                // on a wl-copy that is still waiting for more input.
                //
                // The write runs on a helper thread because `write_all`
                // can block past `COMMAND_TIMEOUT` if a wedged wl-copy
                // never drains its pipe (payload larger than the pipe
                // buffer). If the child is killed on timeout, its pipe
                // read end closes and the blocked writer fails with
                // EPIPE; the `recv_timeout` join bounds even the case
                // where a grandchild inherited the read end.
                let (tx, rx) = std::sync::mpsc::channel();
                if let Some(mut stdin) = child.stdin.take() {
                    let payload = bytes.to_vec();
                    std::thread::spawn(move || {
                        let _ = stdin.write_all(&payload);
                        let _ = tx.send(());
                    });
                } else {
                    drop(tx);
                }
                let status = wait_with_timeout(&mut child, COMMAND_TIMEOUT);
                let _ = rx.recv_timeout(COMMAND_TIMEOUT);
                status
            });
    }

    fn read(&self, mime: &str) -> Option<Vec<u8>> {
        // wl-paste outputs the clipboard contents to stdout.
        capture_stdout(
            Command::new("wl-paste").arg("--type").arg(mime),
            COMMAND_TIMEOUT,
        )
    }

    fn available_types(&self) -> Vec<String> {
        // wl-paste --list-types lists available MIME types.
        capture_stdout(
            Command::new("wl-paste").arg("--list-types"),
            COMMAND_TIMEOUT,
        )
        .map(|out| {
            String::from_utf8_lossy(&out)
                .lines()
                .map(String::from)
                .collect()
        })
        .unwrap_or_default()
    }

    fn clear(&mut self) {
        if let Ok(mut child) = Command::new("wl-copy")
            .arg("--clear")
            .stdin(std::process::Stdio::null())
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .spawn()
        {
            let _ = wait_with_timeout(&mut child, COMMAND_TIMEOUT);
        }
    }

    fn platform_name(&self) -> &str {
        "wayland-wl-clipboard"
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Mutex;

    /// Serializes tests that mutate process environment (`PATH`,
    /// `WAYLAND_DISPLAY`, `XDG_RUNTIME_DIR`).
    static ENV_LOCK: Mutex<()> = Mutex::new(());

    fn env_lock() -> std::sync::MutexGuard<'static, ()> {
        ENV_LOCK.lock().unwrap_or_else(|e| e.into_inner())
    }

    /// A unique scratch directory for fake binaries and sockets.
    struct ScratchDir(std::path::PathBuf);

    impl ScratchDir {
        fn new(tag: &str) -> Self {
            let dir = std::env::temp_dir().join(format!(
                "martensite-wayland-{tag}-{}-{}",
                std::process::id(),
                std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .unwrap()
                    .as_nanos()
            ));
            std::fs::create_dir_all(&dir).unwrap();
            Self(dir)
        }

        /// Writes an executable stub that exits 0 for any invocation
        /// (satisfies the `--version` probe).
        fn write_stub(&self, name: &str) {
            use std::os::unix::fs::PermissionsExt;
            let path = self.0.join(name);
            std::fs::write(&path, "#!/bin/sh\nexit 0\n").unwrap();
            let mut perms = std::fs::metadata(&path).unwrap().permissions();
            perms.set_mode(0o755);
            std::fs::set_permissions(&path, perms).unwrap();
        }
    }

    impl Drop for ScratchDir {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }

    /// Restores mutated environment variables on drop so a failing test
    /// cannot poison the process environment for its siblings.
    struct EnvGuard {
        path: Option<std::ffi::OsString>,
        wayland_display: Option<std::ffi::OsString>,
        xdg_runtime_dir: Option<std::ffi::OsString>,
    }

    impl EnvGuard {
        fn capture() -> Self {
            Self {
                path: std::env::var_os("PATH"),
                wayland_display: std::env::var_os("WAYLAND_DISPLAY"),
                xdg_runtime_dir: std::env::var_os("XDG_RUNTIME_DIR"),
            }
        }

        fn restore(&mut self) {
            restore_var("PATH", self.path.take());
            restore_var("WAYLAND_DISPLAY", self.wayland_display.take());
            restore_var("XDG_RUNTIME_DIR", self.xdg_runtime_dir.take());
        }
    }

    impl Drop for EnvGuard {
        fn drop(&mut self) {
            self.restore();
        }
    }

    fn restore_var(key: &str, value: Option<std::ffi::OsString>) {
        match value {
            Some(value) => std::env::set_var(key, value),
            None => std::env::remove_var(key),
        }
    }

    #[test]
    fn platform_name_is_wayland_wl_clipboard() {
        let _lock = env_lock();
        assert_eq!(WaylandBackend.platform_name(), "wayland-wl-clipboard");
    }

    #[test]
    fn new_fails_without_wayland_display() {
        let _lock = env_lock();
        let _guard = EnvGuard::capture();
        std::env::remove_var("WAYLAND_DISPLAY");
        assert!(matches!(
            WaylandBackend::new(),
            Err(WaylandError::NoDisplay)
        ));
        assert!(!WaylandBackend::is_available());
    }

    #[test]
    fn new_rejects_stale_display_socket() {
        let _lock = env_lock();
        let _guard = EnvGuard::capture();
        let scratch = ScratchDir::new("stale");
        // WAYLAND_DISPLAY is set but the socket does not exist, and the
        // wl-clipboard stubs are present — the socket check must still
        // reject the backend before the binary probe runs.
        scratch.write_stub("wl-copy");
        scratch.write_stub("wl-paste");
        std::env::set_var("PATH", &scratch.0);
        std::env::set_var("XDG_RUNTIME_DIR", &scratch.0);
        std::env::set_var("WAYLAND_DISPLAY", "wayland-definitely-missing");
        assert!(matches!(
            WaylandBackend::new(),
            Err(WaylandError::NoDisplay)
        ));
        assert!(!WaylandBackend::is_available());
        // The factory falls back to the X11 backend (a headless no-op
        // when DISPLAY is unset) rather than selecting dead Wayland.
        let backend = crate::native_backend().expect("X11 fallback is always available");
        assert_eq!(backend.platform_name(), "x11");
    }

    #[test]
    fn native_backend_prefers_wayland_under_wayland() {
        let _lock = env_lock();
        let _guard = EnvGuard::capture();
        let scratch = ScratchDir::new("live");
        scratch.write_stub("wl-copy");
        scratch.write_stub("wl-paste");
        // A stand-in for the compositor socket: the backend only checks
        // for existence, not the file type.
        std::fs::write(scratch.0.join("wayland-test"), "").unwrap();
        std::env::set_var("PATH", &scratch.0);
        std::env::set_var("XDG_RUNTIME_DIR", &scratch.0);
        std::env::set_var("WAYLAND_DISPLAY", "wayland-test");

        assert!(WaylandBackend::is_available());
        let backend =
            crate::native_backend().expect("expected a backend under a fake Wayland session");
        assert_eq!(backend.platform_name(), "wayland-wl-clipboard");
    }

    #[test]
    fn new_fails_without_wl_clipboard() {
        let _lock = env_lock();
        let _guard = EnvGuard::capture();
        let scratch = ScratchDir::new("nobin");
        // Live socket but an empty PATH: the binary probe must reject.
        std::fs::write(scratch.0.join("wayland-test"), "").unwrap();
        std::env::set_var("PATH", &scratch.0);
        std::env::set_var("XDG_RUNTIME_DIR", &scratch.0);
        std::env::set_var("WAYLAND_DISPLAY", "wayland-test");
        assert!(matches!(
            WaylandBackend::new(),
            Err(WaylandError::NoClipboard)
        ));
        assert!(!WaylandBackend::is_available());
    }
}
