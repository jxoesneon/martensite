//! Per-OS application config/data directory resolution.
//!
//! Resolves the conventional per-user settings directory for `org`/`app`
//! without pulling in a `dirs` dependency:
//!
//! | Target | Resolution |
//! |--------|-----------|
//! | Windows | `%APPDATA%\<org>\<app>` (roaming) |
//! | macOS | `~/Library/Application Support/<org>/<app>` |
//! | Linux/BSD | `$XDG_CONFIG_HOME/<org>/<app>` or `~/.config/<org>/<app>` |
//! | other | `$HOME/.config/<org>/<app>` |
//!
//! # Examples
//!
//! ```
//! use martensite_persist::paths::app_config_dir;
//!
//! // `Some` on desktop targets with a resolvable home directory.
//! let _dir = app_config_dir("example", "demo");
//! ```

use std::path::PathBuf;

/// Returns the conventional per-user config directory for
/// `org`/`app`, or `None` when no home/config root is resolvable.
///
/// The directory is **not** created; callers that write should
/// `std::fs::create_dir_all` the returned path first.
///
/// # Examples
///
/// ```
/// use martensite_persist::paths::app_config_dir;
///
/// if let Some(dir) = app_config_dir("acme", "app") {
///     assert!(dir.ends_with("app"));
///     assert!(dir.to_string_lossy().contains("acme"));
/// }
/// ```
pub fn app_config_dir(org: &str, app: &str) -> Option<PathBuf> {
    let root = config_root()?;
    Some(root.join(org).join(app))
}

/// The per-OS config root (no org/app suffix).
fn config_root() -> Option<PathBuf> {
    #[cfg(target_os = "windows")]
    {
        std::env::var_os("APPDATA").map(PathBuf::from)
    }
    #[cfg(target_os = "macos")]
    {
        std::env::var_os("HOME")
            .map(PathBuf::from)
            .map(|h| h.join("Library").join("Application Support"))
    }
    // Linux/BSD (and as a usable fallback, other unix-likes including
    // iOS/Android): XDG_CONFIG_HOME or ~/.config.
    #[cfg(all(unix, not(target_os = "macos")))]
    {
        std::env::var_os("XDG_CONFIG_HOME")
            .map(PathBuf::from)
            .or_else(|| std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".config")))
    }
    // wasm and other non-unix/non-windows targets.
    #[cfg(not(any(unix, target_os = "windows")))]
    {
        std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".config"))
    }
}

/// Returns the default settings file path (`settings.json`) inside
/// [`app_config_dir`].
///
/// # Examples
///
/// ```
/// use martensite_persist::paths::default_store_path;
///
/// if let Some(p) = default_store_path("acme", "app") {
///     assert_eq!(p.file_name().unwrap(), "settings.json");
/// }
/// ```
pub fn default_store_path(org: &str, app: &str) -> Option<PathBuf> {
    app_config_dir(org, app).map(|d| d.join("settings.json"))
}
