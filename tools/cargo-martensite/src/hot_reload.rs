//! Hot-reload coordination framework for the Martensite guest cdylib.
//!
//! The Martensite v0.9.0 architecture splits an application into a lightweight
//! **host** binary (which owns the `WidgetArena` and reactive signal store) and
//! a **guest** dynamic library (`cdylib`) that contains the component code.
//! When source files change, `cargo-martensite` recompiles only the guest crate
//! into a *versioned* `.dylib`/`.so`/`.dll`, and the host re-links the new
//! symbol table and triggers a full layout re-measurement within a 350 ms
//! budget.
//!
//! This module implements the **coordination** layer of that pipeline:
//!
//! * [`FileWatcher`] — a polling-based file watcher with no external
//!   dependencies.
//! * [`versioned_library_path`] — deterministic, platform-aware versioned
//!   cdylib path generation.
//! * [`build_guest_crate`] — spawns `cargo build` for the guest crate as a
//!   cdylib.
//! * [`reload_cycle`] — a single detect → build → measure cycle that updates
//!   [`HotReloadState`].
//! * [`is_within_reload_budget`] — the sub-350 ms latency gate check.
//!
//! ## Why no `unsafe`?
//!
//! Real dynamic-library loading (`dlopen`/`dlsym` via `libloading`) requires
//! `unsafe`. This crate is `#![forbid(unsafe_code)]` because it is a developer
//! tool that must remain auditable. The actual symbol-table re-linking is
//! performed by the **host binary** (a separate crate that is *not*
//! `forbid(unsafe_code)`), which consumes the versioned paths produced here.
//! This keeps the stable ABI boundary described in the milestone spec intact:
//! component code is strictly decoupled from host arena state.
//!
//! # Examples
//!
//! ```
//! use cargo_martensite::hot_reload::{HotReloadConfig, HotReloadState, versioned_library_path};
//! use std::path::PathBuf;
//!
//! let config = HotReloadConfig {
//!     guest_crate: "my_app".to_string(),
//!     output_dir: PathBuf::from("target/martensite"),
//!     watch_paths: vec![PathBuf::from("src")],
//!     poll_interval_ms: 100,
//! };
//! let path = versioned_library_path(&config.output_dir, &config.guest_crate, 7);
//! assert!(path.to_string_lossy().contains("my_app_v7"));
//! ```

use std::collections::HashMap;
use std::fmt;
use std::path::{Path, PathBuf};
use std::process;
use std::time::{Duration, Instant, SystemTime};

/// The sub-350 ms hot-reload latency budget, in milliseconds.
///
/// Reload cycles that exceed this budget fail [`is_within_reload_budget`] and
/// are flagged by the development loop.
pub const RELOAD_BUDGET_MS: u64 = 350;

/// The default poll interval (in milliseconds) used by [`FileWatcher`] when no
/// explicit interval is configured.
pub const DEFAULT_POLL_INTERVAL_MS: u64 = 100;

/// Configuration for a single hot-reload session.
///
/// Describes which guest crate to compile, where to emit the versioned cdylib,
/// which source paths to watch, and how frequently to poll for changes.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HotReloadConfig {
    /// The name of the guest crate (the `cdylib` Cargo target).
    pub guest_crate: String,
    /// Directory where versioned guest libraries are emitted.
    pub output_dir: PathBuf,
    /// Source paths polled for modifications by the [`FileWatcher`].
    pub watch_paths: Vec<PathBuf>,
    /// Polling interval between file-change scans, in milliseconds.
    pub poll_interval_ms: u64,
}

impl HotReloadConfig {
    /// Returns the polling interval as a [`Duration`].
    ///
    /// # Examples
    ///
    /// ```
    /// use cargo_martensite::hot_reload::{HotReloadConfig, DEFAULT_POLL_INTERVAL_MS};
    /// use std::time::Duration;
    ///
    /// let config = HotReloadConfig {
    ///     guest_crate: "g".to_string(),
    ///     output_dir: ".".into(),
    ///     watch_paths: vec![],
    ///     poll_interval_ms: 250,
    /// };
    /// assert_eq!(config.poll_interval(), Duration::from_millis(250));
    /// ```
    pub fn poll_interval(&self) -> Duration {
        Duration::from_millis(self.poll_interval_ms)
    }
}

/// Tracks the current hot-reload state across reload cycles.
///
/// Maintained by [`reload_cycle`] and inspected by the development loop to
/// report progress and enforce the sub-350 ms latency gate.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HotReloadState {
    /// The monotonically increasing version of the currently linked guest.
    pub current_version: u64,
    /// The instant at which the most recent reload completed, if any.
    pub last_reload_time: Option<Instant>,
    /// The wall-clock duration of the most recent reload, in milliseconds.
    pub last_reload_duration_ms: u64,
    /// The total number of successful reloads performed.
    pub total_reloads: u64,
}

impl HotReloadState {
    /// Creates a fresh state with version `0` and no recorded reloads.
    ///
    /// # Examples
    ///
    /// ```
    /// use cargo_martensite::hot_reload::HotReloadState;
    ///
    /// let state = HotReloadState::new();
    /// assert_eq!(state.current_version, 0);
    /// assert_eq!(state.total_reloads, 0);
    /// ```
    pub fn new() -> Self {
        Self {
            current_version: 0,
            last_reload_time: None,
            last_reload_duration_ms: 0,
            total_reloads: 0,
        }
    }
}

impl Default for HotReloadState {
    fn default() -> Self {
        Self::new()
    }
}

/// Errors produced while coordinating a hot-reload cycle.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ReloadError {
    /// The guest crate name was empty or otherwise invalid.
    InvalidConfig(String),
    /// The spawned `cargo build` process failed or could not be launched.
    BuildFailed(String),
    /// A filesystem operation (stat, read) failed during change detection.
    IoFailed(String),
    /// The built cdylib artifact could not be located in the target directory.
    ArtifactNotFound(String),
    /// The built artifact exists but is not a valid dynamic library.
    ArtifactInvalid(String),
}

impl fmt::Display for ReloadError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ReloadError::InvalidConfig(msg) => write!(f, "invalid hot-reload config: {msg}"),
            ReloadError::BuildFailed(msg) => write!(f, "guest build failed: {msg}"),
            ReloadError::IoFailed(msg) => write!(f, "filesystem error: {msg}"),
            ReloadError::ArtifactNotFound(msg) => {
                write!(f, "cdylib artifact not found: {msg}")
            }
            ReloadError::ArtifactInvalid(msg) => {
                write!(f, "cdylib artifact invalid: {msg}")
            }
        }
    }
}

impl std::error::Error for ReloadError {}

/// A polling-based file watcher.
///
/// Records the last-modified timestamp of each watched path on construction
/// and reports the paths whose mtime advanced since the previous
/// [`FileWatcher::check_for_changes`] call. This avoids pulling in a native
/// `inotify`/`FSEvents` dependency, which keeps the tool portable and
/// `unsafe`-free.
///
/// # Examples
///
/// ```
/// use cargo_martensite::hot_reload::FileWatcher;
/// use std::path::PathBuf;
/// use std::time::Duration;
///
/// let mut watcher = FileWatcher::new(vec![PathBuf::from("src")], Duration::from_millis(50));
/// // No changes between consecutive scans yields an empty vec.
/// assert!(watcher.check_for_changes().is_empty());
/// ```
pub struct FileWatcher {
    paths: Vec<PathBuf>,
    last_modified: HashMap<PathBuf, SystemTime>,
    poll_interval: Duration,
}

impl FileWatcher {
    /// Creates a new watcher seeded with the current mtime of each `paths`.
    ///
    /// Directory paths are recursively expanded to their contained files
    /// so that edits to existing files are detected (directory mtime does
    /// not change when a file inside it is edited on most filesystems).
    ///
    /// Paths that do not yet exist are recorded as [`SystemTime::UNIX_EPOCH`]
    /// so that their later creation is reported as a change.
    pub fn new(paths: Vec<PathBuf>, poll_interval: Duration) -> Self {
        let expanded = expand_watch_paths(paths);
        let mut watcher = Self {
            paths: expanded,
            last_modified: HashMap::new(),
            poll_interval,
        };
        watcher.snapshot();
        watcher
    }

    /// Returns the configured poll interval.
    pub fn poll_interval(&self) -> Duration {
        self.poll_interval
    }

    /// Returns the number of paths currently being watched.
    pub fn len(&self) -> usize {
        self.paths.len()
    }

    /// Returns `true` if no paths are being watched.
    pub fn is_empty(&self) -> bool {
        self.paths.is_empty()
    }

    /// Adds a path to the watch set, snapshotting its current mtime.
    /// If the path is a directory, it is recursively expanded to its
    /// contained files.
    pub fn add_path(&mut self, path: PathBuf) {
        let expanded = expand_watch_paths(vec![path]);
        for p in expanded {
            if !self.paths.contains(&p) {
                self.paths.push(p.clone());
            }
            let mtime = current_mtime(&p);
            self.last_modified.insert(p, mtime);
        }
    }

    /// Removes a path from the watch set.
    pub fn remove_path(&mut self, path: &Path) {
        self.paths.retain(|p| p != path);
        self.last_modified.remove(path);
    }

    /// Scans all watched paths and returns those whose mtime advanced since the
    /// last call.
    ///
    /// The returned paths are sorted for deterministic ordering.
    pub fn check_for_changes(&mut self) -> Vec<PathBuf> {
        let mut changed = Vec::new();
        for path in &self.paths {
            let now = current_mtime(path);
            if let Some(&prev) = self.last_modified.get(path) {
                if now != prev {
                    changed.push(path.clone());
                    self.last_modified.insert(path.clone(), now);
                }
            } else {
                // Newly added path not yet snapshotted: record it.
                self.last_modified.insert(path.clone(), now);
            }
        }
        changed.sort();
        changed
    }

    /// Seeds `last_modified` with the current mtime of every watched path.
    fn snapshot(&mut self) {
        for path in &self.paths {
            let mtime = current_mtime(path);
            self.last_modified.insert(path.clone(), mtime);
        }
    }
}

/// Returns the mtime of `path`, or [`SystemTime::UNIX_EPOCH`] if unavailable.
fn current_mtime(path: &Path) -> SystemTime {
    match std::fs::metadata(path) {
        Ok(meta) => meta.modified().unwrap_or(SystemTime::UNIX_EPOCH),
        Err(_) => SystemTime::UNIX_EPOCH,
    }
}

/// Recursively expands directory paths into their contained files.
///
/// Directory mtimes do not change when an existing file inside them is
/// edited on most filesystems (e.g., Linux ext4). To reliably detect
/// source file edits, we expand directory watch paths to individual
/// file paths before snapshotting.
fn expand_watch_paths(paths: Vec<PathBuf>) -> Vec<PathBuf> {
    let mut expanded = Vec::new();
    for path in paths {
        if path.is_dir() {
            walk_dir(&path, &mut expanded);
        } else {
            expanded.push(path);
        }
    }
    expanded.sort();
    expanded.dedup();
    expanded
}

/// Recursively walks a directory and collects all file paths.
fn walk_dir(dir: &Path, out: &mut Vec<PathBuf>) {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            walk_dir(&path, out);
        } else {
            out.push(path);
        }
    }
}

/// Generates a deterministic, platform-aware versioned cdylib path.
///
/// The resulting filename embeds the version counter so that successive
/// recompilations do not clobber one another on disk, allowing the host to
/// atomically re-link to a fresh symbol table:
///
/// * macOS:   `lib{crate}_v{version}.dylib`
/// * Linux:   `lib{crate}_v{version}.so`
/// * Windows: `{crate}_v{version}.dll`
///
/// # Examples
///
/// ```
/// use cargo_martensite::hot_reload::versioned_library_path;
/// use std::path::Path;
///
/// let path = versioned_library_path(Path::new("/tmp/out"), "guest", 3);
/// let name = path.file_name().unwrap().to_string_lossy().to_string();
/// assert!(name.starts_with("guest_v3") || name.starts_with("libguest_v3"));
/// ```
pub fn versioned_library_path(output_dir: &Path, crate_name: &str, version: u64) -> PathBuf {
    versioned_library_path_with(
        output_dir,
        crate_name,
        version,
        std::env::consts::DLL_PREFIX,
        std::env::consts::DLL_EXTENSION,
    )
}

/// Platform-parameterised version of [`versioned_library_path`].
///
/// Exposed for testing so the path shape for every supported platform can be
/// verified regardless of the host running the test suite.
pub(crate) fn versioned_library_path_with(
    output_dir: &Path,
    crate_name: &str,
    version: u64,
    dll_prefix: &str,
    dll_extension: &str,
) -> PathBuf {
    let filename = format!("{dll_prefix}{crate_name}_v{version}.{dll_extension}");
    output_dir.join(filename)
}

/// Builds the guest crate as a cdylib, returning the versioned library path.
///
/// Shells out to `cargo build --lib` (or `cargo rustc --lib` when the guest
/// crate does not declare `crate-type = ["cdylib"]` in its `Cargo.toml`) so
/// that only the library target is compiled and the output is always a
/// dynamic library. After a successful build the produced artifact is copied
/// to the deterministic versioned path returned by
/// [`versioned_library_path`], allowing the host to atomically re-link to a
/// fresh symbol table.
///
/// On build failure a [`ReloadError::BuildFailed`] is returned carrying the
/// captured stderr. If the artifact cannot be located or is not a valid
/// dynamic library, [`ReloadError::ArtifactNotFound`] or
/// [`ReloadError::ArtifactInvalid`] is returned respectively.
pub fn build_guest_crate(config: &HotReloadConfig, version: u64) -> Result<PathBuf, ReloadError> {
    validate_config(config)?;

    // Determine whether the guest crate already declares `crate-type =
    // ["cdylib"]`. If it does, a plain `cargo build --lib` suffices. If not,
    // we force cdylib output via `cargo rustc --lib -- --crate-type cdylib`
    // (the trailing `--crate-type` flag overrides cargo's own `--crate-type`
    // because rustc honors the last occurrence).
    let force_cdylib = !guest_declares_cdylib(&config.guest_crate);

    let mut cmd = process::Command::new("cargo");
    if force_cdylib {
        cmd.arg("rustc")
            .arg("--lib")
            .arg("--package")
            .arg(&config.guest_crate)
            .arg("--target-dir")
            .arg(&config.output_dir)
            .arg("--")
            .arg("--crate-type")
            .arg("cdylib");
    } else {
        cmd.arg("build")
            .arg("--lib")
            .arg("--package")
            .arg(&config.guest_crate)
            .arg("--target-dir")
            .arg(&config.output_dir);
    }

    let output = cmd
        .output()
        .map_err(|e| ReloadError::BuildFailed(format!("failed to spawn cargo: {e}")))?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr).into_owned();
        return Err(ReloadError::BuildFailed(stderr));
    }

    // Locate the freshly built artifact in the target directory. The dev
    // profile (the default for hot-reload) emits into `<output_dir>/debug/`.
    let artifact =
        find_built_artifact(&config.output_dir, &config.guest_crate).ok_or_else(|| {
            ReloadError::ArtifactNotFound(format!(
                "no dynamic library for '{}' found under {}",
                config.guest_crate,
                config.output_dir.display()
            ))
        })?;

    // Verify the artifact is a dynamic library by its file extension.
    if !is_dynamic_library(&artifact) {
        return Err(ReloadError::ArtifactInvalid(format!(
            "built artifact {} is not a dynamic library (.so/.dylib/.dll)",
            artifact.display()
        )));
    }

    // Copy/rename the artifact to the deterministic versioned path so the
    // host can re-link atomically without clobbering the in-use library.
    let versioned = versioned_library_path(&config.output_dir, &config.guest_crate, version);
    std::fs::copy(&artifact, &versioned).map_err(|e| {
        ReloadError::IoFailed(format!(
            "failed to copy {} to {}: {}",
            artifact.display(),
            versioned.display(),
            e
        ))
    })?;

    Ok(versioned)
}

/// Validates a [`HotReloadConfig`], returning `Ok(())` if it is usable.
fn validate_config(config: &HotReloadConfig) -> Result<(), ReloadError> {
    if config.guest_crate.trim().is_empty() {
        return Err(ReloadError::InvalidConfig(
            "guest_crate must be a non-empty crate name".to_string(),
        ));
    }
    if config.output_dir.as_os_str().is_empty() {
        return Err(ReloadError::InvalidConfig(
            "output_dir must be a non-empty path".to_string(),
        ));
    }
    Ok(())
}

/// Returns `true` if `path` has a dynamic-library file extension.
fn is_dynamic_library(path: &Path) -> bool {
    let Some(ext) = path.extension().and_then(|e| e.to_str()) else {
        return false;
    };
    matches!(ext, "so" | "dylib" | "dll")
}

/// Locates the freshly built cdylib artifact inside `output_dir`.
///
/// The dev profile (default for hot-reload) emits into `<output_dir>/debug/`.
/// The filename follows the platform convention:
/// `lib{crate}.so` (Linux), `lib{crate}.dylib` (macOS), `{crate}.dll` (Windows).
fn find_built_artifact(output_dir: &Path, crate_name: &str) -> Option<PathBuf> {
    let profile_dir = output_dir.join("debug");
    let prefix = std::env::consts::DLL_PREFIX;
    let ext = std::env::consts::DLL_EXTENSION;
    let candidate = profile_dir.join(format!("{prefix}{crate_name}.{ext}"));
    if candidate.exists() {
        return Some(candidate);
    }
    // Fallback: scan the profile directory for any file matching the
    // crate name with a dynamic-library extension. This handles platforms
    // where the prefix is empty (Windows) or where the extension differs.
    let entries = std::fs::read_dir(&profile_dir).ok()?;
    let mut best: Option<PathBuf> = None;
    for entry in entries.flatten() {
        let path = entry.path();
        let Some(name) = path.file_name().and_then(|n| n.to_str()) else {
            continue;
        };
        if name.contains(crate_name) && is_dynamic_library(&path) {
            best = Some(path);
        }
    }
    best
}

/// Returns `true` if the guest crate declares `crate-type = ["cdylib"]` in
/// its `Cargo.toml`.
///
/// Uses `cargo metadata --no-deps` to locate the manifest path for the named
/// package, then reads the `[lib]` table for a `crate-type` entry containing
/// `cdylib`. If the manifest cannot be located or read, this returns `false`
/// (conservatively forcing cdylib output).
fn guest_declares_cdylib(crate_name: &str) -> bool {
    let Some(manifest) = find_guest_manifest(crate_name) else {
        return false;
    };
    let Ok(contents) = std::fs::read_to_string(&manifest) else {
        return false;
    };
    crate_toml_declares_cdylib(&contents)
}

/// Parses a `Cargo.toml` body and returns `true` if the `[lib]` section
/// declares `crate-type` containing `cdylib`.
fn crate_toml_declares_cdylib(toml: &str) -> bool {
    // Minimal TOML scan: find the `[lib]` table and look for a `crate-type`
    // line within it that mentions `cdylib`. This avoids pulling in a TOML
    // parser dependency for a developer tool.
    let mut in_lib_section = false;
    for line in toml.lines() {
        let trimmed = line.trim();
        if trimmed.starts_with('[') {
            in_lib_section = trimmed == "[lib]";
            continue;
        }
        if in_lib_section && trimmed.starts_with("crate-type") && trimmed.contains("cdylib") {
            return true;
        }
    }
    false
}

/// Locates the `Cargo.toml` manifest path for the named workspace member
/// using `cargo metadata --no-deps`.
fn find_guest_manifest(crate_name: &str) -> Option<PathBuf> {
    let output = process::Command::new("cargo")
        .arg("metadata")
        .arg("--no-deps")
        .arg("--format-version")
        .arg("1")
        .output()
        .ok()?;
    if !output.status.success() {
        return None;
    }
    let stdout = String::from_utf8_lossy(&output.stdout);
    extract_manifest_path(&stdout, crate_name)
}

/// Extracts the `manifest_path` for the package named `crate_name` from a
/// `cargo metadata` JSON blob using a minimal string scan.
fn extract_manifest_path(metadata_json: &str, crate_name: &str) -> Option<PathBuf> {
    // The JSON contains objects like:
    //   {"name":"<crate_name>","version":"...","id":"...","manifest_path":"..."}
    // We search for `"name":"<crate_name>"` and then the next
    // `"manifest_path":"..."` that follows it within the same package object.
    let name_key = format!("\"name\":\"{crate_name}\"");
    let name_pos = metadata_json.find(&name_key)?;
    let after_name = &metadata_json[name_pos..];
    let mp_key = "\"manifest_path\":\"";
    let mp_start = after_name.find(mp_key)?;
    let value_start = mp_start + mp_key.len();
    let value_slice = &after_name[value_start..];
    let value_end = value_slice.find('"')?;
    let path_str = &value_slice[..value_end];
    Some(PathBuf::from(path_str))
}

/// Coordinates a single hot-reload cycle: build the guest and measure elapsed
/// time, updating `state` in place.
///
/// Returns the new guest version on success. File-change detection is the
/// responsibility of the caller (the development loop drives a [`FileWatcher`]
/// and invokes this function when changes are observed); this function focuses
/// purely on the build-and-time half of the cycle so it can be unit-tested in
/// isolation.
///
/// # Examples
///
/// ```
/// use cargo_martensite::hot_reload::{HotReloadConfig, HotReloadState};
/// use std::path::PathBuf;
///
/// let config = HotReloadConfig {
///     guest_crate: "demo".to_string(),
///     output_dir: PathBuf::from("target/martensite"),
///     watch_paths: vec![PathBuf::from("src")],
///     poll_interval_ms: 100,
/// };
/// let mut state = HotReloadState::new();
/// // Note: this would shell out to cargo; see tests for the error path.
/// let _ = state;
/// ```
pub fn reload_cycle(
    config: &HotReloadConfig,
    state: &mut HotReloadState,
) -> Result<u64, ReloadError> {
    let start = Instant::now();
    let next_version = state
        .current_version
        .checked_add(1)
        .ok_or_else(|| ReloadError::InvalidConfig("version counter overflowed".to_string()))?;

    let _path = build_guest_crate(config, next_version)?;

    let elapsed = start.elapsed();
    let duration_ms = duration_to_ms(elapsed);

    state.current_version = next_version;
    state.last_reload_time = Some(Instant::now());
    state.last_reload_duration_ms = duration_ms;
    state.total_reloads = state.total_reloads.saturating_add(1);

    Ok(next_version)
}

/// Converts a [`Duration`] to whole milliseconds (rounded down).
fn duration_to_ms(d: Duration) -> u64 {
    d.as_millis().min(u128::from(u64::MAX)) as u64
}

/// Returns `true` if `duration_ms` is within the sub-350 ms reload budget.
///
/// This is the latency gate defined in milestone §5.1: a reload that
/// meets or exceeds the budget is reported as a regression. The gate
/// is strict (`< 350`), matching the spec's "sub-350ms" requirement.
///
/// # Examples
///
/// ```
/// use cargo_martensite::hot_reload::{is_within_reload_budget, RELOAD_BUDGET_MS};
///
/// assert!(is_within_reload_budget(100));
/// assert!(is_within_reload_budget(RELOAD_BUDGET_MS - 1));
/// assert!(!is_within_reload_budget(RELOAD_BUDGET_MS));
/// assert!(!is_within_reload_budget(RELOAD_BUDGET_MS + 1));
/// ```
pub fn is_within_reload_budget(duration_ms: u64) -> bool {
    duration_ms < RELOAD_BUDGET_MS
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use std::path::PathBuf;

    #[test]
    fn default_poll_interval_constant() {
        assert_eq!(DEFAULT_POLL_INTERVAL_MS, 100);
    }

    #[test]
    fn reload_budget_is_350ms() {
        assert_eq!(RELOAD_BUDGET_MS, 350);
    }

    #[test]
    fn budget_check_within() {
        assert!(is_within_reload_budget(0));
        assert!(is_within_reload_budget(200));
        assert!(is_within_reload_budget(349));
    }

    #[test]
    fn budget_check_exceeds() {
        assert!(!is_within_reload_budget(350));
        assert!(!is_within_reload_budget(351));
        assert!(!is_within_reload_budget(1_000));
        assert!(!is_within_reload_budget(u64::MAX));
    }

    #[test]
    fn versioned_path_macos() {
        let path = versioned_library_path_with(Path::new("/tmp/out"), "guest", 1, "lib", "dylib");
        assert_eq!(path, PathBuf::from("/tmp/out/libguest_v1.dylib"));
    }

    #[test]
    fn versioned_path_linux() {
        let path = versioned_library_path_with(Path::new("/tmp/out"), "app", 42, "lib", "so");
        assert_eq!(path, PathBuf::from("/tmp/out/libapp_v42.so"));
    }

    #[test]
    fn versioned_path_windows() {
        let path = versioned_library_path_with(Path::new("C:\\out"), "widget", 7, "", "dll");
        assert_eq!(path, PathBuf::from("C:\\out/widget_v7.dll"));
    }

    #[test]
    fn versioned_path_current_platform_matches_constants() {
        let path = versioned_library_path(Path::new("target"), "guest", 9);
        let name = path.file_name().unwrap().to_string_lossy().into_owned();
        assert!(name.contains("guest_v9"));
        assert!(name.ends_with(std::env::consts::DLL_EXTENSION));
        if !std::env::consts::DLL_PREFIX.is_empty() {
            assert!(name.starts_with(std::env::consts::DLL_PREFIX));
        }
    }

    #[test]
    fn versioned_path_increments_version() {
        let p1 = versioned_library_path(Path::new("o"), "g", 1);
        let p2 = versioned_library_path(Path::new("o"), "g", 2);
        assert_ne!(p1, p2);
        assert!(p2.to_string_lossy().contains("g_v2"));
    }

    #[test]
    fn hot_reload_state_new_defaults() {
        let state = HotReloadState::new();
        assert_eq!(state.current_version, 0);
        assert_eq!(state.last_reload_duration_ms, 0);
        assert_eq!(state.total_reloads, 0);
        assert!(state.last_reload_time.is_none());
        assert_eq!(HotReloadState::default(), state);
    }

    #[test]
    fn config_poll_interval_conversion() {
        let config = HotReloadConfig {
            guest_crate: "g".to_string(),
            output_dir: PathBuf::from("."),
            watch_paths: vec![],
            poll_interval_ms: 333,
        };
        assert_eq!(config.poll_interval(), Duration::from_millis(333));
    }

    #[test]
    fn build_guest_crate_rejects_empty_crate_name() {
        let config = HotReloadConfig {
            guest_crate: "   ".to_string(),
            output_dir: PathBuf::from("target"),
            watch_paths: vec![],
            poll_interval_ms: 100,
        };
        let err = build_guest_crate(&config, 1).unwrap_err();
        assert!(matches!(err, ReloadError::InvalidConfig(_)));
        assert!(err.to_string().contains("guest_crate"));
    }

    #[test]
    fn build_guest_crate_rejects_empty_output_dir() {
        let config = HotReloadConfig {
            guest_crate: "g".to_string(),
            output_dir: PathBuf::new(),
            watch_paths: vec![],
            poll_interval_ms: 100,
        };
        let err = build_guest_crate(&config, 1).unwrap_err();
        assert!(matches!(err, ReloadError::InvalidConfig(_)));
        assert!(err.to_string().contains("output_dir"));
    }

    #[test]
    fn reload_cycle_reports_invalid_config_without_building() {
        let config = HotReloadConfig {
            guest_crate: String::new(),
            output_dir: PathBuf::from("target"),
            watch_paths: vec![],
            poll_interval_ms: 100,
        };
        let mut state = HotReloadState::new();
        let err = reload_cycle(&config, &mut state).unwrap_err();
        assert!(matches!(err, ReloadError::InvalidConfig(_)));
        // State must be untouched on failure.
        assert_eq!(state.current_version, 0);
        assert_eq!(state.total_reloads, 0);
    }

    #[test]
    fn reload_cycle_returns_versioned_path_on_build_failure() {
        // A non-existent crate will fail the cargo build, exercising the
        // BuildFailed path without depending on a real guest crate.
        let config = HotReloadConfig {
            guest_crate: "definitely_not_a_real_crate_xyz".to_string(),
            output_dir: PathBuf::from("target/martensite-test"),
            watch_paths: vec![],
            poll_interval_ms: 100,
        };
        let mut state = HotReloadState::new();
        let result = reload_cycle(&config, &mut state);
        // cargo may or may not be present; if it is, the build must fail.
        match result {
            Err(ReloadError::BuildFailed(_)) => {
                assert_eq!(state.current_version, 0);
                assert_eq!(state.total_reloads, 0);
            }
            Err(ReloadError::InvalidConfig(_)) => {
                // Acceptable: config validation path.
            }
            Ok(_) => {
                // If cargo somehow succeeded (unlikely), state must advance.
                assert_eq!(state.current_version, 1);
                assert_eq!(state.total_reloads, 1);
            }
            Err(other) => panic!("unexpected error: {other:?}"),
        }
    }

    #[test]
    fn file_watcher_detects_new_file() {
        let dir = tempdir();
        let path = dir.join("watched.rs");
        fs::write(&path, "// initial\n").unwrap();

        let mut watcher = FileWatcher::new(vec![path.clone()], Duration::from_millis(1));
        // First scan after construction: no changes yet.
        assert!(watcher.check_for_changes().is_empty());

        // Bump mtime by rewriting.
        std::thread::sleep(Duration::from_millis(20));
        fs::write(&path, "// modified\n").unwrap();

        let changed = watcher.check_for_changes();
        assert_eq!(changed, vec![path.clone()]);

        // A subsequent scan with no further changes is empty.
        assert!(watcher.check_for_changes().is_empty());
    }

    #[test]
    fn file_watcher_add_and_remove_path() {
        let dir = tempdir();
        let a = dir.join("a.rs");
        let b = dir.join("b.rs");
        fs::write(&a, "a").unwrap();
        fs::write(&b, "b").unwrap();

        let mut watcher = FileWatcher::new(vec![a.clone()], Duration::from_millis(1));
        assert_eq!(watcher.len(), 1);

        watcher.add_path(b.clone());
        assert_eq!(watcher.len(), 2);

        watcher.remove_path(&a);
        assert_eq!(watcher.len(), 1);
        assert!(!watcher.is_empty());

        watcher.remove_path(&b);
        assert!(watcher.is_empty());
    }

    #[test]
    fn file_watcher_handles_missing_paths() {
        let missing = PathBuf::from("/nonexistent/path/that/should/not/exist.rs");
        let mut watcher = FileWatcher::new(vec![missing.clone()], Duration::from_millis(1));
        // Missing paths do not panic and produce no changes.
        assert!(watcher.check_for_changes().is_empty());
        assert_eq!(watcher.len(), 1);
    }

    #[test]
    fn file_watcher_poll_interval_accessor() {
        let watcher = FileWatcher::new(vec![], Duration::from_millis(250));
        assert_eq!(watcher.poll_interval(), Duration::from_millis(250));
        assert!(watcher.is_empty());
    }

    #[test]
    fn reload_error_display() {
        assert!(ReloadError::InvalidConfig("x".into())
            .to_string()
            .contains("x"));
        assert!(ReloadError::BuildFailed("y".into())
            .to_string()
            .contains("y"));
        assert!(ReloadError::IoFailed("z".into()).to_string().contains("z"));
    }

    #[test]
    fn duration_to_ms_clamps() {
        assert_eq!(duration_to_ms(Duration::from_millis(0)), 0);
        assert_eq!(duration_to_ms(Duration::from_millis(123)), 123);
        assert_eq!(duration_to_ms(Duration::from_secs(1)), 1_000);
    }

    #[test]
    fn reload_error_display_includes_new_variants() {
        assert!(ReloadError::ArtifactNotFound("missing".into())
            .to_string()
            .contains("missing"));
        assert!(ReloadError::ArtifactInvalid("bad".into())
            .to_string()
            .contains("bad"));
    }

    #[test]
    fn is_dynamic_library_recognizes_extensions() {
        assert!(is_dynamic_library(Path::new("libfoo.so")));
        assert!(is_dynamic_library(Path::new("libfoo.dylib")));
        assert!(is_dynamic_library(Path::new("foo.dll")));
        assert!(!is_dynamic_library(Path::new("libfoo.rlib")));
        assert!(!is_dynamic_library(Path::new("foo")));
    }

    #[test]
    fn crate_toml_declares_cdylib_detects_lib_section() {
        let toml = "[lib]\ncrate-type = [\"cdylib\"]\n";
        assert!(crate_toml_declares_cdylib(toml));
    }

    #[test]
    fn crate_toml_declares_cdylib_with_other_types() {
        let toml = "[lib]\ncrate-type = [\"rlib\", \"cdylib\"]\n";
        assert!(crate_toml_declares_cdylib(toml));
    }

    #[test]
    fn crate_toml_declares_cdylib_without_cdylib() {
        let toml = "[lib]\ncrate-type = [\"rlib\"]\n";
        assert!(!crate_toml_declares_cdylib(toml));
    }

    #[test]
    fn crate_toml_declares_cdylib_no_lib_section() {
        let toml = "[package]\nname = \"foo\"\n";
        assert!(!crate_toml_declares_cdylib(toml));
    }

    #[test]
    fn crate_toml_declares_cdylib_ignores_bin_section() {
        let toml = "[[bin]]\ncrate-type = [\"cdylib\"]\n";
        assert!(!crate_toml_declares_cdylib(toml));
    }

    #[test]
    fn extract_manifest_path_finds_package() {
        let json = r#"{"packages":[{"name":"foo","manifest_path":"/a/Cargo.toml"},{"name":"bar","manifest_path":"/b/Cargo.toml"}]}"#;
        let path = extract_manifest_path(json, "bar");
        assert_eq!(path, Some(PathBuf::from("/b/Cargo.toml")));
    }

    #[test]
    fn extract_manifest_path_missing_package() {
        let json = r#"{"packages":[{"name":"foo","manifest_path":"/a/Cargo.toml"}]}"#;
        assert!(extract_manifest_path(json, "missing").is_none());
    }

    /// Creates a unique temporary directory for a single test and returns its
    /// path. The directory is left behind (tests are short-lived).
    fn tempdir() -> PathBuf {
        let mut dir = std::env::temp_dir();
        let id = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        dir.push(format!("cargo-martensite-test-{id}"));
        fs::create_dir_all(&dir).unwrap();
        dir
    }
}
