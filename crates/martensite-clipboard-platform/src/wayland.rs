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
//! is set, indicating a running Wayland compositor. If `wl-clipboard` is
//! not found, [`WaylandBackend::new`] returns an error and the factory
//! falls back to the X11 backend.
//!
//! # Safety
//!
//! This module contains no `unsafe` code. All clipboard operations are
//! performed via subprocess invocations of `wl-clipboard`.

use std::process::Command;

use crate::ClipboardBackend;

/// Errors that can occur when initializing the Wayland clipboard backend.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum WaylandError {
    /// The `WAYLAND_DISPLAY` environment variable is not set.
    NoDisplay,
    /// The `wl-clipboard` binary was not found on `$PATH`.
    NoClipboard,
}

impl std::fmt::Display for WaylandError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            WaylandError::NoDisplay => write!(f, "WAYLAND_DISPLAY is not set"),
            WaylandError::NoClipboard => write!(f, "wl-clipboard binary not found on PATH"),
        }
    }
}

impl std::error::Error for WaylandError {}

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
pub struct WaylandBackend {
    /// Cached available types from the last `available_types` call.
    cached_types: Vec<String>,
}

impl WaylandBackend {
    /// Creates a new Wayland clipboard backend.
    ///
    /// Returns an error if `WAYLAND_DISPLAY` is not set or `wl-clipboard`
    /// is not installed.
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
        if std::env::var("WAYLAND_DISPLAY").is_err() {
            return Err(WaylandError::NoDisplay);
        }
        // Verify wl-clipboard is available.
        let result = Command::new("wl-copy")
            .arg("--version")
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .spawn();
        if result.is_err() {
            return Err(WaylandError::NoClipboard);
        }
        Ok(Self {
            cached_types: Vec::new(),
        })
    }

    /// Checks if the Wayland clipboard backend is available.
    ///
    /// Returns `true` if `WAYLAND_DISPLAY` is set and `wl-clipboard` is
    /// installed.
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
        std::env::var("WAYLAND_DISPLAY").is_ok()
            && Command::new("wl-copy")
                .arg("--version")
                .stdout(std::process::Stdio::null())
                .stderr(std::process::Stdio::null())
                .spawn()
                .is_ok()
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
            .and_then(|mut child| {
                use std::io::Write;
                if let Some(stdin) = child.stdin.as_mut() {
                    let _ = stdin.write_all(bytes);
                }
                child.wait()
            });
    }

    fn read(&self, mime: &str) -> Option<Vec<u8>> {
        // wl-paste outputs the clipboard contents to stdout.
        let output = Command::new("wl-paste")
            .arg("--type")
            .arg(mime)
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::null())
            .output()
            .ok()?;
        if !output.status.success() {
            return None;
        }
        Some(output.stdout)
    }

    fn available_types(&self) -> Vec<String> {
        // wl-paste --list-types lists available MIME types.
        if let Ok(output) = Command::new("wl-paste")
            .arg("--list-types")
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::null())
            .output()
        {
            if output.status.success() {
                let types = String::from_utf8_lossy(&output.stdout)
                    .lines()
                    .map(String::from)
                    .collect();
                return types;
            }
        }
        Vec::new()
    }

    fn clear(&mut self) {
        let _ = Command::new("wl-copy")
            .arg("--clear")
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .status();
        self.cached_types.clear();
    }

    fn platform_name(&self) -> &str {
        "wayland-wl-clipboard"
    }
}
