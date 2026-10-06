//! Category H — Toolchain, Lifecycle & Scaffolding tools (spec §3.20–3.22).
//!
//! - `martensite_doctor` — fully offline host diagnosis: Rust toolchain,
//!   linkers, display server, GPU presence, workspace pin, and the
//!   `cargo-martensite` dev-loop binary.
//! - `martensite_reload_status` — live hot-reload cdylib lifecycle over the
//!   ADR-0038 dev channel; degrades to `active: false` offline (invariant 5).
//! - `martensite_hot_reload` — requests a guest-code rebuild+reload through
//!   the dev coordinator's reload-request marker.
//! - `martensite_scaffold_widget` — workspace-confined idiomatic widget code
//!   generation with audit logging (spec §6.4).

use std::borrow::Cow;
use std::path::Path;
use std::process::Command;

use rmcp::handler::server::router::tool::{AsyncTool, ToolBase};
use rmcp::model::ToolAnnotations;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::error::McpError;
use crate::server::MartensiteMcp;
use crate::types::AuditRecord;

// ---------------------------------------------------------------------------
// martensite_doctor
// ---------------------------------------------------------------------------

/// Parameters for `martensite_doctor` (no parameters).
#[derive(Debug, Clone, Default, Deserialize, JsonSchema)]
#[serde(default)]
pub struct DoctorParams {}

/// Outcome status of a single `martensite_doctor` check.
///
/// # Examples
///
/// ```
/// use martensite_mcp::tools::lifecycle::DoctorStatus;
///
/// assert_ne!(DoctorStatus::Pass, DoctorStatus::Fail);
/// ```
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "lowercase")]
pub enum DoctorStatus {
    /// The check passed.
    Pass,
    /// Non-fatal degradation or missing optional capability.
    Warn,
    /// A required capability is missing or broken.
    Fail,
}

/// One diagnostic check result emitted by `martensite_doctor`.
///
/// # Examples
///
/// ```
/// use martensite_mcp::tools::lifecycle::{DoctorCheck, DoctorStatus};
///
/// let check = DoctorCheck::pass("toolchain/rustc", "rustc 1.89.0");
/// assert_eq!(check.status, DoctorStatus::Pass);
/// ```
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct DoctorCheck {
    /// Check identifier (`toolchain/rustc`, `linker/mold`, ...).
    pub name: String,
    /// Outcome (`pass`, `warn`, `fail`).
    pub status: DoctorStatus,
    /// Human-readable finding detail.
    pub detail: String,
}

impl DoctorCheck {
    /// A passing check.
    pub fn pass(name: impl Into<String>, detail: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            status: DoctorStatus::Pass,
            detail: detail.into(),
        }
    }

    /// A warning check.
    pub fn warn(name: impl Into<String>, detail: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            status: DoctorStatus::Warn,
            detail: detail.into(),
        }
    }

    /// A failing check.
    pub fn fail(name: impl Into<String>, detail: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            status: DoctorStatus::Fail,
            detail: detail.into(),
        }
    }
}

/// Aggregated `martensite_doctor` report.
///
/// # Examples
///
/// ```
/// use martensite_mcp::tools::lifecycle::{DoctorCheck, DoctorOutput};
///
/// let out = DoctorOutput {
///     checks: vec![DoctorCheck::pass("a", "ok")],
///     healthy: true,
/// };
/// assert!(out.healthy);
/// ```
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct DoctorOutput {
    /// All checks performed, in probe order.
    pub checks: Vec<DoctorCheck>,
    /// `true` when no check reported `fail`.
    pub healthy: bool,
}

/// `martensite_doctor`: host toolchain, GPU adapters, display-server and
/// feature-flag diagnostics. Works fully offline.
pub struct DoctorTool;

impl ToolBase for DoctorTool {
    type Parameter = DoctorParams;
    type Output = DoctorOutput;
    type Error = McpError;

    fn name() -> Cow<'static, str> {
        "martensite_doctor".into()
    }

    fn description() -> Option<Cow<'static, str>> {
        Some(
            "Diagnose host toolchains (rustc, LLD/mold linkers), GPU adapters \
             (Vulkan/wgpu), display servers (Wayland/X11/macOS), and required \
             dev tools. Available offline."
                .into(),
        )
    }

    fn annotations() -> Option<ToolAnnotations> {
        crate::tools::read_only_annotations()
    }
}

impl AsyncTool<MartensiteMcp> for DoctorTool {
    async fn invoke(
        service: &MartensiteMcp,
        _param: Self::Parameter,
    ) -> Result<Self::Output, Self::Error> {
        Ok(run_doctor(service.offline().workspace_root()))
    }
}

/// Runs all doctor probes against the given workspace root.
fn run_doctor(workspace_root: &Path) -> DoctorOutput {
    let mut checks = vec![
        check_binary("toolchain/rustc", "rustc", &["--version"]),
        check_binary("toolchain/cargo", "cargo", &["--version"]),
        check_rustup(),
        check_toolchain_pin(workspace_root),
    ];
    checks.extend(check_linkers());
    checks.push(check_display());
    checks.push(check_gpu());
    checks.push(check_cargo_martensite());

    let healthy = !checks.iter().any(|c| c.status == DoctorStatus::Fail);
    DoctorOutput { checks, healthy }
}

/// Runs `bin` with `args`, returning the first stdout line on success.
fn command_output_line(bin: &str, args: &[&str]) -> Option<String> {
    let out = Command::new(bin).args(args).output().ok()?;
    if !out.status.success() {
        return None;
    }
    let stdout = String::from_utf8_lossy(&out.stdout);
    stdout.lines().next().map(|l| l.trim().to_string())
}

/// Whether `bin` resolves and executes at all (no panic on missing binaries).
fn binary_on_path(bin: &str) -> bool {
    Command::new(bin).arg("--version").output().is_ok()
}

/// Generic `tool --version` probe: `fail` when the binary is absent.
fn check_binary(name: &str, bin: &str, args: &[&str]) -> DoctorCheck {
    match command_output_line(bin, args) {
        Some(line) => DoctorCheck::pass(name, line),
        None => DoctorCheck::fail(
            name,
            format!("`{bin}` not found on PATH or returned a non-zero status"),
        ),
    }
}

/// `rustup` presence — a warning rather than a failure because distros and
/// rustup-less toolchains (e.g. system packages) are valid setups.
fn check_rustup() -> DoctorCheck {
    match command_output_line("rustup", &["--version"]) {
        Some(line) => DoctorCheck::pass("toolchain/rustup", line),
        None => DoctorCheck::warn(
            "toolchain/rustup",
            "`rustup` not found on PATH — component management unavailable",
        ),
    }
}

/// Detects `rust-toolchain.toml` / `rust-toolchain` pins at the workspace root.
fn check_toolchain_pin(workspace_root: &Path) -> DoctorCheck {
    for candidate in ["rust-toolchain.toml", "rust-toolchain"] {
        let path = workspace_root.join(candidate);
        if path.is_file() {
            let detail = std::fs::read_to_string(&path)
                .ok()
                .and_then(|text| {
                    text.lines()
                        .map(str::trim)
                        .find(|l| l.starts_with("channel"))
                        .map(str::to_string)
                })
                .unwrap_or_else(|| "pinned toolchain".to_string());
            return DoctorCheck::pass("toolchain/pin", format!("`{candidate}` present ({detail})"));
        }
    }
    DoctorCheck::warn(
        "toolchain/pin",
        "no rust-toolchain.toml at the workspace root — version parity \
         with the running app is not pinned",
    )
}

/// Probes the linker set used by the CI/verification profile (mold, LLD,
/// clang, cc). Fails only when *no* usable linker is found.
fn check_linkers() -> Vec<DoctorCheck> {
    // (check name, binary, version args)
    let probes: &[(&str, &str, &[&str])] = &[
        ("linker/mold", "mold", &["--version"]),
        ("linker/lld", "ld.lld", &["--version"]),
        ("linker/clang", "clang", &["--version"]),
        ("linker/cc", "cc", &["--version"]),
    ];
    let mut checks = Vec::with_capacity(probes.len() + 1);
    let mut any = false;
    for &(name, bin, args) in probes {
        match command_output_line(bin, args) {
            Some(line) => {
                any = true;
                checks.push(DoctorCheck::pass(name, line));
            }
            None => checks.push(DoctorCheck::warn(
                name,
                format!("`{bin}` not found on PATH"),
            )),
        }
    }
    // `rust-lld` ships with the toolchain — probe it separately so the
    // aggregate still recognizes an otherwise bare host.
    if let Some(line) = command_output_line("rust-lld", &["-flavor", "gnu", "--version"]) {
        any = true;
        checks.push(DoctorCheck::pass("linker/rust-lld", line));
    }
    checks.push(if any {
        DoctorCheck::pass("linker", "at least one linker driver is available")
    } else {
        DoctorCheck::fail(
            "linker",
            "no linker driver found (mold, lld, clang, cc) — `cargo build` cannot link",
        )
    });
    checks
}

/// Display-server environment probe: Wayland/X11 variables on Unix, a static
/// pass on macOS/Windows.
fn check_display() -> DoctorCheck {
    #[cfg(all(unix, not(target_os = "macos")))]
    {
        let runtime_dir = std::env::var("XDG_RUNTIME_DIR").ok();
        let wayland = std::env::var("WAYLAND_DISPLAY").ok();
        let x11 = std::env::var("DISPLAY").ok();
        let session = std::env::var("XDG_SESSION_TYPE").ok();

        let mut parts = Vec::new();
        if let Some(rt) = &runtime_dir {
            parts.push(format!("XDG_RUNTIME_DIR={rt}"));
        }
        if let Some(w) = &wayland {
            parts.push(format!("WAYLAND_DISPLAY={w}"));
        }
        if let Some(x) = &x11 {
            parts.push(format!("DISPLAY={x}"));
        }
        if let Some(s) = &session {
            parts.push(format!("XDG_SESSION_TYPE={s}"));
        }

        if wayland.is_none() && x11.is_none() {
            return DoctorCheck::warn(
                "display",
                format!(
                    "no display server detected (headless){}",
                    if parts.is_empty() {
                        String::new()
                    } else {
                        format!("; {}", parts.join(", "))
                    }
                ),
            );
        }
        if runtime_dir.is_none() {
            return DoctorCheck::warn(
                "display",
                format!(
                    "display server present but XDG_RUNTIME_DIR unset — the \
                     dev-channel socket directory falls back to a tmp dir ({})",
                    parts.join(", ")
                ),
            );
        }
        DoctorCheck::pass("display", parts.join(", "))
    }
    #[cfg(target_os = "macos")]
    {
        DoctorCheck::pass("display", "macOS window server (Aqua) available")
    }
    #[cfg(target_os = "windows")]
    {
        DoctorCheck::pass("display", "Windows desktop window manager available")
    }
    #[cfg(not(any(unix, target_os = "windows")))]
    {
        DoctorCheck::warn("display", "unknown platform — assuming headless")
    }
}

/// GPU presence probe: `/dev/dri` render nodes on Linux (with an optional
/// `vulkaninfo --summary` fallback when DRI is absent), static passes on
/// macOS/Windows where the platform guarantees a graphics stack.
fn check_gpu() -> DoctorCheck {
    #[cfg(target_os = "linux")]
    {
        let dri = Path::new("/dev/dri");
        if dri.is_dir() {
            let mut nodes: Vec<String> = std::fs::read_dir(dri)
                .map(|rd| {
                    rd.filter_map(|e| e.ok())
                        .filter_map(|e| {
                            e.file_name()
                                .to_str()
                                .filter(|n| n.starts_with("renderD") || n.starts_with("card"))
                                .map(str::to_string)
                        })
                        .collect()
                })
                .unwrap_or_default();
            nodes.sort();
            if !nodes.is_empty() {
                return DoctorCheck::pass(
                    "gpu/adapter",
                    format!("DRM/KMS render nodes present: {}", nodes.join(", ")),
                );
            }
        }
        // Fall back to `vulkaninfo --summary` — cheap and only reached when
        // the kernel DRM interface is absent (VMs, containers).
        if binary_on_path("vulkaninfo") {
            if let Some(summary) = command_output_line("vulkaninfo", &["--summary"]) {
                return DoctorCheck::pass(
                    "gpu/adapter",
                    format!("vulkaninfo reports devices ({summary})"),
                );
            }
        }
        DoctorCheck::warn(
            "gpu/adapter",
            "no /dev/dri render nodes and no vulkaninfo output — CPU raster \
             fallback (TinySkia) will be used",
        )
    }
    #[cfg(target_os = "macos")]
    {
        DoctorCheck::pass("gpu/adapter", "Metal-capable GPU guaranteed on macOS")
    }
    #[cfg(target_os = "windows")]
    {
        DoctorCheck::pass(
            "gpu/adapter",
            "DXGI adapter enumeration available (WARP fallback guaranteed)",
        )
    }
    #[cfg(not(any(target_os = "linux", target_os = "macos", target_os = "windows")))]
    {
        DoctorCheck::warn(
            "gpu/adapter",
            "unrecognized platform — assuming CPU raster fallback",
        )
    }
}

/// `cargo-martensite` dev-loop binary presence on PATH.
fn check_cargo_martensite() -> DoctorCheck {
    match command_output_line("cargo-martensite", &["--version"]) {
        Some(line) => DoctorCheck::pass("tools/cargo-martensite", line),
        None => DoctorCheck::warn(
            "tools/cargo-martensite",
            "`cargo-martensite` not on PATH — `cargo martensite dev` / hot \
             reload unavailable (cargo install --path tools/cargo-martensite)",
        ),
    }
}

// ---------------------------------------------------------------------------
// martensite_reload_status
// ---------------------------------------------------------------------------

/// Parameters for `martensite_reload_status` (no parameters).
#[derive(Debug, Clone, Default, Deserialize, JsonSchema)]
#[serde(default)]
pub struct ReloadStatusParams {}

/// `martensite_reload_status` output: the live hot-reload cdylib lifecycle
/// state, or `active: false` when no dev session is reachable.
///
/// # Examples
///
/// ```
/// use martensite_mcp::tools::lifecycle::ReloadStatusOutput;
///
/// let out = ReloadStatusOutput::inactive("no live session");
/// assert!(!out.active);
/// ```
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct ReloadStatusOutput {
    /// Whether a live hot-reload session is currently attached.
    pub active: bool,
    /// Unique identifier of the currently mapped cdylib build.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub active_build_id: Option<String>,
    /// ISO 8601 timestamp of the last successful reload.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub last_reload_timestamp: Option<String>,
    /// Total successful hot swaps in the active session.
    #[schemars(schema_with = "crate::types::schema_strip::u64s")]
    pub total_reloads: u64,
    /// Active rustc warnings/errors reported by background file watching.
    #[serde(default)]
    pub compiler_diagnostics: Vec<Value>,
    /// Status of `on_hot_reload` state restoration hooks.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub state_preservation_report: Option<String>,
    /// Explanatory detail (session source, or why reload is inactive).
    pub detail: String,
}

impl ReloadStatusOutput {
    /// An inactive report for offline mode.
    pub fn inactive(detail: impl Into<String>) -> Self {
        Self {
            active: false,
            active_build_id: None,
            last_reload_timestamp: None,
            total_reloads: 0,
            compiler_diagnostics: Vec::new(),
            state_preservation_report: None,
            detail: detail.into(),
        }
    }

    /// Builds an active report from the dev-channel `reload_status` payload,
    /// tolerating missing fields (older devtools builds).
    fn from_live(v: &Value) -> Self {
        let get_str = |key: &str| v.get(key).and_then(Value::as_str).map(str::to_string);
        Self {
            active: true,
            active_build_id: get_str("active_build_id"),
            last_reload_timestamp: get_str("last_reload_timestamp"),
            total_reloads: v.get("total_reloads").and_then(Value::as_u64).unwrap_or(0),
            compiler_diagnostics: v
                .get("compiler_diagnostics")
                .and_then(Value::as_array)
                .cloned()
                .unwrap_or_default(),
            state_preservation_report: get_str("state_preservation_report"),
            detail: "live dev session attached".to_string(),
        }
    }
}

/// `martensite_reload_status`: live hot-reload cdylib lifecycle status.
pub struct ReloadStatusTool;

impl ToolBase for ReloadStatusTool {
    type Parameter = ReloadStatusParams;
    type Output = ReloadStatusOutput;
    type Error = McpError;

    fn name() -> Cow<'static, str> {
        "martensite_reload_status".into()
    }

    fn description() -> Option<Cow<'static, str>> {
        Some(
            "Query the live hot-reload cdylib lifecycle: active build id, last \
             reload timestamp, total reloads, compiler diagnostics, and state \
             preservation report. Returns `active: false` in offline mode."
                .into(),
        )
    }

    fn annotations() -> Option<ToolAnnotations> {
        crate::tools::read_only_annotations()
    }
}

impl AsyncTool<MartensiteMcp> for ReloadStatusTool {
    async fn invoke(
        service: &MartensiteMcp,
        _param: Self::Parameter,
    ) -> Result<Self::Output, Self::Error> {
        match service.try_live_call("reload_status", serde_json::json!({}))? {
            Some(v) => Ok(ReloadStatusOutput::from_live(&v)),
            None => Ok(ReloadStatusOutput::inactive(
                "no live dev session — hot reload inactive; start \
                 `cargo martensite dev` or pass --socket <path>",
            )),
        }
    }
}

// ---------------------------------------------------------------------------
// martensite_hot_reload
// ---------------------------------------------------------------------------

/// Parameters for `martensite_hot_reload`.
///
/// # Examples
///
/// ```
/// use martensite_mcp::tools::lifecycle::HotReloadParams;
///
/// let p = HotReloadParams { reason: Some("fix button".to_string()) };
/// assert!(p.reason.is_some());
/// ```
#[derive(Debug, Clone, Default, Deserialize, JsonSchema)]
#[serde(default)]
pub struct HotReloadParams {
    /// Free-form reason recorded in the reload-request marker.
    pub reason: Option<String>,
}

/// Structured result of `martensite_hot_reload`.
///
/// # Examples
///
/// ```
/// use martensite_mcp::tools::lifecycle::HotReloadOutput;
///
/// let out = HotReloadOutput {
///     requested: true,
///     mechanism: Some("marker-file".to_string()),
///     marker: Some("/run/martensite/app.reload".to_string()),
///     mode: "live".to_string(),
/// };
/// assert!(out.requested);
/// ```
#[derive(Debug, Clone, Serialize, JsonSchema)]
pub struct HotReloadOutput {
    /// Whether the dev session accepted the reload request.
    pub requested: bool,
    /// Reload mechanism engaged (`marker-file`, `in-process`, ...), when
    /// reported.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub mechanism: Option<String>,
    /// Path of the reload-request marker the session dropped, when
    /// reported.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub marker: Option<String>,
    /// Server mode that produced the answer (`live` on success).
    pub mode: String,
}

/// `martensite_hot_reload`: ask the running app's dev coordinator to rebuild
/// and reload guest code.
pub struct HotReloadTool;

impl ToolBase for HotReloadTool {
    type Parameter = HotReloadParams;
    type Output = HotReloadOutput;
    type Error = McpError;

    fn name() -> Cow<'static, str> {
        "martensite_hot_reload".into()
    }

    fn description() -> Option<Cow<'static, str>> {
        Some(
            "Request a guest-code rebuild+reload from the running app's dev \
             coordinator: the session drops a reload-request marker that \
             `cargo martensite dev` polls for and acts on. Verify the cycle \
             afterwards with `martensite_reload_status`. Requires a live \
             `cargo martensite dev` session."
                .into(),
        )
    }

    fn annotations() -> Option<ToolAnnotations> {
        crate::tools::mutation_annotations()
    }
}

impl AsyncTool<MartensiteMcp> for HotReloadTool {
    async fn invoke(
        service: &MartensiteMcp,
        param: Self::Parameter,
    ) -> Result<Self::Output, Self::Error> {
        // Live-only per spec: `live_call` produces the offline IPC error.
        let resp = service.live_call(
            "hot_reload",
            serde_json::json!({ "reason": param.reason.as_deref() }),
        )?;
        Ok(HotReloadOutput::from_wire(&resp, mode_str(service)))
    }
}

impl HotReloadOutput {
    /// Builds the output from the raw `hot_reload` dev-channel payload,
    /// tolerating `requested`/`ok`/`accepted`, `mechanism`/`via`, and
    /// `marker`/`marker_path`/`marker_file` spellings.
    fn from_wire(resp: &Value, mode: &str) -> Self {
        let get_str = |keys: &[&str]| {
            keys.iter()
                .find_map(|k| resp.get(*k).and_then(Value::as_str))
                .map(str::to_string)
        };
        Self {
            requested: ["requested", "ok", "accepted"]
                .iter()
                .find_map(|k| resp.get(*k).and_then(Value::as_bool))
                .unwrap_or(true),
            mechanism: get_str(&["mechanism", "via"]),
            marker: get_str(&["marker", "marker_path", "marker_file"]),
            mode: mode.to_string(),
        }
    }
}

/// Current server mode as a stable wire string (`live` / `offline`).
fn mode_str(service: &MartensiteMcp) -> &'static str {
    match service.mode() {
        crate::server::ServerMode::Live { .. } => "live",
        crate::server::ServerMode::Offline => "offline",
    }
}

// ---------------------------------------------------------------------------
// martensite_scaffold_widget
// ---------------------------------------------------------------------------

/// Parameters for `martensite_scaffold_widget`.
#[derive(Debug, Clone, Default, Deserialize, JsonSchema)]
#[serde(default)]
pub struct ScaffoldWidgetParams {
    /// PascalCase widget name.
    pub widget_name: String,
    /// Target crate directory (canonicalized; must resolve inside the
    /// Cargo workspace root — spec §3.22 safety gate).
    pub parent_crate_path: String,
    /// `"leaf"` (custom painting), `"container"` (internal children
    /// protocol), or `"composite"` (macro-based component).
    pub widget_type: String,
    /// Include `Signal` bindings.
    pub has_reactive_state: Option<bool>,
    /// Include the AccessKit contract implementation.
    pub has_a11y: Option<bool>,
}

/// `martensite_scaffold_widget` output: files written and module registration
/// status.
///
/// # Examples
///
/// ```
/// use martensite_mcp::tools::lifecycle::ScaffoldWidgetOutput;
///
/// let out = ScaffoldWidgetOutput {
///     files: vec!["src/widgets/foo.rs".into()],
///     module_updated: true,
///     registered: true,
/// };
/// assert!(out.registered);
/// ```
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct ScaffoldWidgetOutput {
    /// Workspace-relative paths of every file written (or already identical).
    pub files: Vec<String>,
    /// Whether the widget module's `mod.rs` was created or modified.
    pub module_updated: bool,
    /// Whether the widget is reachable via `pub mod` + `pub use` after this
    /// call.
    pub registered: bool,
}

/// `martensite_scaffold_widget`: generate idiomatic widget source code,
/// strictly workspace-path confined.
pub struct ScaffoldWidgetTool;

impl ToolBase for ScaffoldWidgetTool {
    type Parameter = ScaffoldWidgetParams;
    type Output = ScaffoldWidgetOutput;
    type Error = McpError;

    fn name() -> Cow<'static, str> {
        "martensite_scaffold_widget".into()
    }

    fn description() -> Option<Cow<'static, str>> {
        Some(
            "Generate idiomatic, standards-compliant Martensite widget source \
             (leaf / container / composite) with optional Signal bindings and \
             AccessKit contract; strictly workspace-path confined."
                .into(),
        )
    }

    fn annotations() -> Option<ToolAnnotations> {
        crate::tools::artifact_annotations()
    }
}

impl AsyncTool<MartensiteMcp> for ScaffoldWidgetTool {
    async fn invoke(
        service: &MartensiteMcp,
        param: Self::Parameter,
    ) -> Result<Self::Output, Self::Error> {
        scaffold_widget(service, &param)
    }
}

/// Validates the `widget_name` contract: PascalCase, `^[A-Z][a-zA-Z0-9]*$`.
fn validate_widget_name(name: &str) -> Result<(), McpError> {
    let mut chars = name.chars();
    let valid = match chars.next() {
        Some(c) if c.is_ascii_uppercase() => chars.all(|c| c.is_ascii_alphanumeric()),
        _ => false,
    };
    if valid {
        Ok(())
    } else {
        Err(McpError::InvalidParameter(format!(
            "widget_name `{name}` must be PascalCase matching \
             `^[A-Z][a-zA-Z0-9]*$`"
        )))
    }
}

/// Converts a PascalCase widget name to a snake_case file stem
/// (`KpiBanner` → `kpi_banner`).
fn to_snake_case(name: &str) -> String {
    let mut out = String::with_capacity(name.len() + 4);
    for (i, c) in name.chars().enumerate() {
        if c.is_ascii_uppercase() {
            if i > 0 {
                out.push('_');
            }
            out.push(c.to_ascii_lowercase());
        } else {
            out.push(c);
        }
    }
    out
}

/// Writes `content` to `path`, refusing to clobber differing content.
/// Returns `true` when the file was (re)written.
fn write_guarded(path: &Path, content: &str) -> Result<bool, McpError> {
    if path.exists() {
        let existing = std::fs::read_to_string(path).map_err(McpError::Io)?;
        if existing == content {
            return Ok(false);
        }
        return Err(McpError::InvalidTarget(format!(
            "`{}` already exists with different content; refusing to \
             overwrite — remove it or choose another widget name",
            path.display()
        )));
    }
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(McpError::Io)?;
    }
    std::fs::write(path, content).map_err(McpError::Io)?;
    Ok(true)
}

/// Writes an audit record for a generated/appended file.
fn audit_file(
    service: &MartensiteMcp,
    abs_path: &Path,
    span_start: u32,
    span_end: u32,
    diff: &str,
) -> Result<(), McpError> {
    let rel = abs_path
        .strip_prefix(service.offline().workspace_root())
        .unwrap_or(abs_path)
        .to_string_lossy()
        .replace('\\', "/");
    let mut record = AuditRecord::new("scaffold_widget", rel, span_start, span_end);
    record.diff = diff.to_string();
    service.offline().write_audit(&record)
}

/// `martensite_scaffold_widget` implementation (offline only).
fn scaffold_widget(
    service: &MartensiteMcp,
    param: &ScaffoldWidgetParams,
) -> Result<ScaffoldWidgetOutput, McpError> {
    validate_widget_name(&param.widget_name)?;

    let widget_type = match param.widget_type.as_str() {
        "leaf" => WidgetKind::Leaf,
        "container" => WidgetKind::Container,
        "composite" => WidgetKind::Composite,
        other => {
            return Err(McpError::InvalidParameter(format!(
                "widget_type `{other}` is invalid — expected `leaf`, \
                 `container`, or `composite`"
            )))
        }
    };

    let crate_dir = service
        .offline()
        .confine_to_workspace(Path::new(&param.parent_crate_path))?;
    if !crate_dir.is_dir() {
        return Err(McpError::InvalidParameter(format!(
            "parent_crate_path `{}` is not an existing directory inside the \
             workspace",
            param.parent_crate_path
        )));
    }

    let snake = to_snake_case(&param.widget_name);
    let reactive = param.has_reactive_state.unwrap_or(false);
    let a11y = param.has_a11y.unwrap_or(true);
    let source = widget_source(widget_type, &param.widget_name, &snake, reactive, a11y);

    let widgets_dir = crate_dir.join("src").join("widgets");
    let widget_file = widgets_dir.join(format!("{snake}.rs"));

    let mut files = Vec::new();
    let line_count = u32::try_from(source.lines().count()).unwrap_or(u32::MAX);
    if write_guarded(&widget_file, &source)? {
        files.push(widget_file.clone());
        audit_file(service, &widget_file, 1, line_count, &source)?;
    } else {
        files.push(widget_file.clone());
    }

    // Module registration: ensure `pub mod <snake>;` and
    // `pub use <snake>::<Name>;` exist in `src/widgets/mod.rs`.
    let mod_file = widgets_dir.join("mod.rs");
    let mod_decl = format!("pub mod {snake};");
    let use_decl = format!("pub use {snake}::{};", param.widget_name);
    let existing_mod = std::fs::read_to_string(&mod_file)
        .map_err(McpError::Io)
        .ok();

    let mut registered;
    let mut module_updated = false;
    match existing_mod {
        Some(text) => {
            let has_mod = text.lines().any(|l| l.trim() == mod_decl);
            let has_use = text.lines().any(|l| l.trim() == use_decl);
            registered = has_mod;
            let mut additions = String::new();
            if !has_mod {
                additions.push_str(&mod_decl);
                additions.push('\n');
            }
            if !has_use {
                additions.push_str(&use_decl);
                additions.push('\n');
            }
            if !additions.is_empty() {
                let mut updated = text;
                if !updated.ends_with('\n') {
                    updated.push('\n');
                }
                let first_new_line = u32::try_from(updated.lines().count()).unwrap_or(u32::MAX) + 1;
                updated.push_str(&additions);
                std::fs::write(&mod_file, &updated).map_err(McpError::Io)?;
                files.push(mod_file.clone());
                let last_new_line = u32::try_from(updated.lines().count()).unwrap_or(u32::MAX);
                audit_file(
                    service,
                    &mod_file,
                    first_new_line,
                    last_new_line,
                    &additions,
                )?;
                module_updated = true;
                registered = true;
            }
        }
        None => {
            let content = format!(
                "//! Widget module — generated module index.\n\n{mod_decl}\n\n{use_decl}\n"
            );
            write_guarded(&mod_file, &content)?;
            files.push(mod_file.clone());
            audit_file(service, &mod_file, 1, 5, &content)?;
            module_updated = true;
            registered = true;
        }
    }

    let files: Vec<String> = files
        .iter()
        .map(|p| {
            p.strip_prefix(service.offline().workspace_root())
                .unwrap_or(p)
                .to_string_lossy()
                .replace('\\', "/")
        })
        .collect();

    Ok(ScaffoldWidgetOutput {
        files,
        module_updated,
        registered,
    })
}

/// Widget skeleton kind selected by `widget_type`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum WidgetKind {
    /// Custom-painted leaf widget.
    Leaf,
    /// Parent widget using the internal-children protocol.
    Container,
    /// `widget!`-macro declared state + composed stock widgets.
    Composite,
}

/// Renders the widget source file for the requested kind.
fn widget_source(kind: WidgetKind, name: &str, snake: &str, reactive: bool, a11y: bool) -> String {
    match kind {
        WidgetKind::Leaf => leaf_source(name, snake, reactive, a11y),
        WidgetKind::Container => container_source(name, snake, reactive, a11y),
        WidgetKind::Composite => composite_source(name, snake, reactive, a11y),
    }
}

/// Common doc header for generated widget files.
fn widget_doc_header(name: &str, snake: &str, blurb: &str) -> String {
    format!(
        "//! `{name}` widget — {blurb}.\n\
         //!\n\
         //! Scaffolded skeleton: requires `martensite` plus the `glam`,\n\
         //! `kurbo`, and `accesskit` crates as direct dependencies.\n\
         //!\n\
         //! # Examples\n\
         //!\n\
         //! ```\n\
         //! use crate::widgets::{snake}::{name};\n\
         //!\n\
         //! let widget = {name}::new();\n\
         //! ```\n\n"
    )
}

/// Accessibility impl block lines shared by all widget kinds.
fn a11y_impl(name: &str, role: &str) -> String {
    format!(
        "    fn accessibility(&self, node: &mut AccessKitNode) {{\n\
         \x20       node.set_role(accesskit::Role::{role});\n\
         \x20       node.set_label(\"{name}\");\n\
         \x20   }}\n"
    )
}

/// Signal-binding field + constructor lines for `has_reactive_state`.
fn reactive_field() -> &'static str {
    "    /// Reactive value mirrored into paint/measure.\n    pub value: martensite::reactive::Signal<f32>,\n"
}

fn reactive_init() -> &'static str {
    "            value: martensite::reactive::Signal::new(0.0),\n"
}

/// Leaf-widget skeleton (custom painting, no internal children).
fn leaf_source(name: &str, snake: &str, reactive: bool, a11y: bool) -> String {
    let mut s = widget_doc_header(name, snake, "custom-painted leaf widget");

    s.push_str("use glam::Vec2;\n");
    if a11y {
        s.push_str("use accesskit::Node as AccessKitNode;\n");
    }
    s.push_str(
        "use martensite::core::widget::{LayoutConstraints, LayoutContext, PaintContext, Widget};\n\
         use martensite::core::{Rect, TokenKey};\n\n",
    );
    s.push_str("/// Ink fallback when the theme lacks an accent token.\n");
    s.push_str("const FALLBACK: [u8; 4] = [90, 120, 200, 255];\n\n");

    s.push_str(&format!(
        "/// {name} — a leaf widget with custom painting.\n"
    ));
    s.push_str("#[derive(Debug)]\n");
    s.push_str(&format!("pub struct {name} {{\n"));
    if reactive {
        s.push_str(reactive_field());
    }
    s.push_str("    /// Cached bounds from the last layout pass.\n    cached_bounds: Rect,\n}\n\n");

    s.push_str(&format!("impl {name} {{\n"));
    s.push_str(&format!(
        "    /// Creates a new `{name}`.\n    pub fn new() -> Self {{\n        Self {{\n"
    ));
    if reactive {
        s.push_str(reactive_init());
    }
    s.push_str("            cached_bounds: Rect::default(),\n        }\n    }\n}\n\n");
    s.push_str(&format!("impl Default for {name} {{\n    fn default() -> Self {{\n        Self::new()\n    }}\n}}\n\n"));

    s.push_str(&format!("impl Widget for {name} {{\n"));
    s.push_str(
        "    fn measure(&mut self, cx: &mut LayoutContext, constraints: LayoutConstraints) -> Vec2 {\n\
         \x20       // Intrinsic size: fill bounded offers, otherwise a sensible default.\n\
         \x20       Vec2::new(\n\
         \x20           if constraints.max_size.x < f32::MAX {\n\
         \x20               constraints.max_size.x.max(0.0)\n\
         \x20           } else {\n\
         \x20               cx.pt(64.0)\n\
         \x20           },\n\
         \x20           if constraints.max_size.y < f32::MAX {\n\
         \x20               constraints.max_size.y.max(0.0)\n\
         \x20           } else {\n\
         \x20               cx.pt(32.0)\n\
         \x20           },\n\
         \x20       )\n\
         \x20   }\n\n",
    );
    s.push_str(
        "    fn layout(&mut self, _cx: &mut LayoutContext, bounds: Rect) {\n\
         \x20       self.cached_bounds = bounds;\n\
         \x20   }\n\n",
    );
    if a11y {
        s.push_str(&a11y_impl(name, "GenericContainer"));
        s.push('\n');
    }
    s.push_str(
        "    fn paint(&self, cx: &mut PaintContext) {\n\
         \x20       let b = cx.bounds;\n\
         \x20       let color = cx.color(TokenKey::AccentColor, FALLBACK);\n\
         \x20       cx.list.push_fill_rect(\n\
         \x20           kurbo::Rect::new(\n\
         \x20               f64::from(b.origin.x),\n\
         \x20               f64::from(b.origin.y),\n\
         \x20               f64::from(b.max_x()),\n\
         \x20               f64::from(b.max_y()),\n\
         \x20           ),\n\
         \x20           color,\n\
         \x20       );\n\
         \x20   }\n\
         }\n",
    );
    s
}

/// Container-widget skeleton (internal children protocol, spec architecture
/// notes in `crates/martensite/src/widgets/mod.rs`).
fn container_source(name: &str, snake: &str, reactive: bool, a11y: bool) -> String {
    let mut s = widget_doc_header(name, snake, "container widget with internal children");

    s.push_str("use glam::Vec2;\n");
    if a11y {
        s.push_str("use accesskit::Node as AccessKitNode;\n");
    }
    s.push_str(
        "use martensite::core::widget::{LayoutConstraints, LayoutContext, PaintContext, Widget};\n\
         use martensite::core::Rect;\n\n",
    );

    s.push_str(&format!(
        "/// {name} — a container managing its own children via the\n\
         /// `child_count`/`child`/`child_mut`/`child_bounds` protocol.\n"
    ));
    s.push_str(&format!("pub struct {name} {{\n"));
    if reactive {
        s.push_str(reactive_field());
    }
    s.push_str(
        "    /// Internal children (not registered in the arena).\n\
         \x20   pub children: Vec<Box<dyn Widget>>,\n\
         \x20   /// Per-child bounds resolved during `layout`.\n\
         \x20   child_rects: Vec<Rect>,\n\
         \x20   /// Cached bounds from the last layout pass.\n\
         \x20   cached_bounds: Rect,\n\
         }\n\n",
    );

    s.push_str(&format!("impl {name} {{\n"));
    s.push_str(&format!(
        "    /// Creates an empty `{name}`.\n    pub fn new() -> Self {{\n        Self {{\n"
    ));
    if reactive {
        s.push_str(reactive_init());
    }
    s.push_str(
        "            children: Vec::new(),\n\
         \x20           child_rects: Vec::new(),\n\
         \x20           cached_bounds: Rect::default(),\n\
         \x20       }\n\
         \x20   }\n\n\
         \x20   /// Appends a child widget.\n\
         \x20   #[must_use]\n\
         \x20   pub fn child(mut self, child: impl Widget + 'static) -> Self {\n\
         \x20       self.children.push(Box::new(child));\n\
         \x20       self\n\
         \x20   }\n\
         }\n\n",
    );
    s.push_str(&format!(
        "impl Default for {name} {{\n    fn default() -> Self {{\n        Self::new()\n    }}\n}}\n\n"
    ));
    s.push_str(&format!(
        "impl std::fmt::Debug for {name} {{\n\
         \x20   fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {{\n\
         \x20       f.debug_struct(\"{name}\")\n\
         \x20           .field(\"children\", &self.children.len())\n\
         \x20           .finish()\n\
         \x20   }}\n\
         }}\n\n"
    ));

    s.push_str(&format!("impl Widget for {name} {{\n"));
    s.push_str(
        "    fn measure(&mut self, cx: &mut LayoutContext, constraints: LayoutConstraints) -> Vec2 {\n\
         \x20       // Stack children vertically; widest child wins.\n\
         \x20       let mut size = Vec2::ZERO;\n\
         \x20       for child in &mut self.children {\n\
         \x20           let child_size = child.measure(cx, constraints);\n\
         \x20           size.x = size.x.max(child_size.x);\n\
         \x20           size.y += child_size.y;\n\
         \x20       }\n\
         \x20       size\n\
         \x20   }\n\n",
    );
    s.push_str(
        "    fn layout(&mut self, cx: &mut LayoutContext, bounds: Rect) {\n\
         \x20       self.cached_bounds = bounds;\n\
         \x20       self.child_rects.clear();\n\
         \x20       let mut y = bounds.origin.y;\n\
         \x20       for child in &mut self.children {\n\
         \x20           // A fair scaffold shares height evenly; refine per child.\n\
         \x20           let h = bounds.size.y / self.children.len().max(1) as f32;\n\
         \x20           let rect = Rect::new(bounds.origin.x, y, bounds.size.x, h);\n\
         \x20           cx.layout_child(child.as_mut(), rect);\n\
         \x20           self.child_rects.push(rect);\n\
         \x20           y += h;\n\
         \x20       }\n\
         \x20   }\n\n",
    );
    if a11y {
        s.push_str(&a11y_impl(name, "GenericContainer"));
        s.push('\n');
    }
    s.push_str(
        "    fn paint(&self, _cx: &mut PaintContext) {\n\
         \x20       // Chrome-free container; children paint themselves.\n\
         \x20   }\n\n",
    );
    s.push_str(
        "    fn child_count(&self) -> usize {\n\
         \x20       self.children.len()\n\
         \x20   }\n\n\
         \x20   fn child(&self, index: usize) -> Option<&dyn Widget> {\n\
         \x20       self.children.get(index).map(|c| c.as_ref())\n\
         \x20   }\n\n\
         \x20   fn child_mut(&mut self, index: usize) -> Option<&mut dyn Widget> {\n\
         \x20       self.children.get_mut(index).map(|c| c.as_mut())\n\
         \x20   }\n\n\
         \x20   fn child_bounds(&self, index: usize) -> Option<Rect> {\n\
         \x20       self.child_rects.get(index).copied()\n\
         \x20   }\n\
         }\n",
    );
    s
}

/// Composite-widget skeleton (`martensite::macros::widget!` state declaration
/// + a `Widget` impl composing stock widgets).
fn composite_source(name: &str, snake: &str, reactive: bool, a11y: bool) -> String {
    let mut s = widget_doc_header(name, snake, "macro-based composite component");

    s.push_str("use glam::Vec2;\n");
    if a11y {
        s.push_str("use accesskit::Node as AccessKitNode;\n");
    }
    s.push_str(
        "use martensite::core::widget::{LayoutConstraints, LayoutContext, PaintContext, Widget};\n\
         use martensite::core::Rect;\n\
         use martensite::macros::widget;\n\
         use martensite::widgets::{Flex, FlexDirection};\n\n",
    );

    s.push_str(&format!(
        "widget! {{\n    /// {name} — composite component state (getters/setters generated).\n    {name} {{\n        title: String = String::new(),\n    }}\n}}\n\n"
    ));

    s.push_str(&format!("impl {name} {{\n"));
    s.push_str(
        "    /// Builds the internal composition: a column of stock widgets.\n\
         \x20   /// Children live inside the widget (two-level layout) — they\n\
         \x20   /// are not registered in the arena.\n\
         \x20   fn build_children(&self) -> Vec<Box<dyn Widget>> {\n\
         \x20       vec![Box::new(Flex::new(FlexDirection::Column))]\n\
         \x20   }\n",
    );
    if reactive {
        s.push_str(
            "\n    /// Binds a reactive value into this component's state.\n\
             \x20   pub fn bind_value(&self, signal: martensite::reactive::Signal<f32>) -> martensite::reactive::Signal<f32> {\n\
             \x20       signal\n\
             \x20   }\n",
        );
    }
    s.push_str("}\n\n");

    s.push_str(&format!("impl Widget for {name} {{\n"));
    s.push_str(
        "    fn measure(&mut self, _cx: &mut LayoutContext, constraints: LayoutConstraints) -> Vec2 {\n\
         \x20       // Composite fallback: honor bounded offers, else a default card.\n\
         \x20       Vec2::new(\n\
         \x20           if constraints.max_size.x < f32::MAX {\n\
         \x20               constraints.max_size.x.max(0.0).min(320.0)\n\
         \x20           } else {\n\
         \x20               320.0\n\
         \x20           },\n\
         \x20           if constraints.max_size.y < f32::MAX {\n\
         \x20               constraints.max_size.y.max(0.0).min(120.0)\n\
         \x20           } else {\n\
         \x20               120.0\n\
         \x20           },\n\
         \x20       )\n\
         \x20   }\n\n",
    );
    s.push_str("    fn layout(&mut self, _cx: &mut LayoutContext, _bounds: Rect) {}\n\n");
    if a11y {
        s.push_str(&a11y_impl(name, "Group"));
        s.push('\n');
    }
    s.push_str(
        "    fn paint(&self, _cx: &mut PaintContext) {\n\
         \x20       // Stock children paint themselves; add chrome here.\n\
         \x20   }\n\
         }\n",
    );
    s
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::server::McpServerOptions;

    #[test]
    fn widget_name_validation() {
        assert!(validate_widget_name("KpiBanner").is_ok());
        assert!(validate_widget_name("A").is_ok());
        assert!(validate_widget_name("Widget2x").is_ok());
        assert!(validate_widget_name("kpiBanner").is_err());
        assert!(validate_widget_name("").is_err());
        assert!(validate_widget_name("Kpi_Banner").is_err());
        assert!(validate_widget_name("2Widget").is_err());
    }

    #[test]
    fn snake_case_conversion() {
        assert_eq!(to_snake_case("KpiBanner"), "kpi_banner");
        assert_eq!(to_snake_case("Text"), "text");
        assert_eq!(to_snake_case("MyWidget2"), "my_widget2");
    }

    #[test]
    fn doctor_runs_offline() {
        let server = MartensiteMcp::new(McpServerOptions::offline());
        let out = run_doctor(server.offline().workspace_root());
        assert!(!out.checks.is_empty());
        assert!(out.checks.iter().any(|c| c.name == "toolchain/rustc"));
        assert!(out.checks.iter().any(|c| c.name.starts_with("linker/")));
    }

    #[tokio::test]
    async fn reload_status_offline_is_inactive() {
        let server = MartensiteMcp::new(McpServerOptions::offline());
        let out = ReloadStatusTool::invoke(&server, ReloadStatusParams {})
            .await
            .expect("offline reload status never errors");
        assert!(!out.active);
        assert!(out.detail.contains("no live dev session"));
    }

    #[test]
    fn reload_status_from_live_fields() {
        let v = serde_json::json!({
            "active_build_id": "b-42",
            "last_reload_timestamp": "2026-09-01T10:00:00Z",
            "total_reloads": 7,
            "compiler_diagnostics": ["warning: unused"],
            "state_preservation_report": "ok",
        });
        let out = ReloadStatusOutput::from_live(&v);
        assert!(out.active);
        assert_eq!(out.active_build_id.as_deref(), Some("b-42"));
        assert_eq!(out.total_reloads, 7);
        assert_eq!(out.compiler_diagnostics.len(), 1);
    }

    #[test]
    fn scaffold_rejects_bad_params() {
        let server = MartensiteMcp::new(McpServerOptions::offline());
        let bad_name = ScaffoldWidgetParams {
            widget_name: "lowercase".to_string(),
            parent_crate_path: ".".to_string(),
            widget_type: "leaf".to_string(),
            ..Default::default()
        };
        let err = scaffold_widget(&server, &bad_name).unwrap_err();
        assert!(matches!(err, McpError::InvalidParameter(_)));

        let bad_type = ScaffoldWidgetParams {
            widget_name: "Good".to_string(),
            parent_crate_path: ".".to_string(),
            widget_type: "weird".to_string(),
            ..Default::default()
        };
        let err = scaffold_widget(&server, &bad_type).unwrap_err();
        assert!(matches!(err, McpError::InvalidParameter(_)));

        let escape = ScaffoldWidgetParams {
            widget_name: "Good".to_string(),
            parent_crate_path: "/etc".to_string(),
            widget_type: "leaf".to_string(),
            ..Default::default()
        };
        let err = scaffold_widget(&server, &escape).unwrap_err();
        assert!(matches!(err, McpError::WorkspaceConfinementViolation(_)));
    }

    #[test]
    fn scaffold_leaf_writes_files() {
        let root = std::env::temp_dir().join(format!("mcp-scaffold-{}", std::process::id()));
        let crate_dir = root.join("my_crate");
        std::fs::create_dir_all(crate_dir.join("src")).expect("mkdir");
        std::fs::write(
            crate_dir.join("Cargo.toml"),
            "[package]\nname = \"my_crate\"\n",
        )
        .expect("manifest");

        let server = MartensiteMcp::new(McpServerOptions {
            workspace_root: Some(root.clone()),
            ..McpServerOptions::offline()
        });
        let out = scaffold_widget(
            &server,
            &ScaffoldWidgetParams {
                widget_name: "KpiTile".to_string(),
                parent_crate_path: "my_crate".to_string(),
                widget_type: "leaf".to_string(),
                has_a11y: Some(false),
                ..Default::default()
            },
        )
        .expect("scaffold");

        let widget_file = crate_dir.join("src/widgets/kpi_tile.rs");
        assert!(widget_file.is_file());
        let src = std::fs::read_to_string(&widget_file).expect("read");
        assert!(src.contains("pub struct KpiTile"));
        assert!(src.contains("impl Widget for KpiTile"));
        assert!(out.registered);
        assert!(out.module_updated);
        assert_eq!(out.files.len(), 2);

        // Idempotent: a second identical call succeeds.
        scaffold_widget(
            &server,
            &ScaffoldWidgetParams {
                widget_name: "KpiTile".to_string(),
                parent_crate_path: "my_crate".to_string(),
                widget_type: "leaf".to_string(),
                has_a11y: Some(false),
                ..Default::default()
            },
        )
        .expect("idempotent");

        // Divergent content is refused.
        std::fs::write(&widget_file, "// tampered\n").expect("tamper");
        let err = scaffold_widget(
            &server,
            &ScaffoldWidgetParams {
                widget_name: "KpiTile".to_string(),
                parent_crate_path: "my_crate".to_string(),
                widget_type: "leaf".to_string(),
                ..Default::default()
            },
        )
        .unwrap_err();
        assert!(matches!(err, McpError::InvalidTarget(_)));

        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn hot_reload_output_parses_wire_payload() {
        let out = HotReloadOutput::from_wire(
            &serde_json::json!({
                "requested": true,
                "mechanism": "marker-file",
                "marker": "/run/martensite/dev.reload",
            }),
            "live",
        );
        assert!(out.requested);
        assert_eq!(out.mechanism.as_deref(), Some("marker-file"));
        assert_eq!(out.marker.as_deref(), Some("/run/martensite/dev.reload"));

        // Alias spellings.
        let out = HotReloadOutput::from_wire(
            &serde_json::json!({"accepted": true, "via": "in-process",
                "marker_path": "/tmp/m.marker"}),
            "live",
        );
        assert!(out.requested);
        assert_eq!(out.mechanism.as_deref(), Some("in-process"));
        assert_eq!(out.marker.as_deref(), Some("/tmp/m.marker"));

        // Minimal acknowledgement still counts as requested.
        let out = HotReloadOutput::from_wire(&serde_json::json!({}), "live");
        assert!(out.requested);
        assert!(out.mechanism.is_none());
    }

    #[tokio::test]
    async fn hot_reload_requires_live_session() {
        let server = MartensiteMcp::new(McpServerOptions::offline());
        let res = HotReloadTool::invoke(
            &server,
            HotReloadParams {
                reason: Some("iterate".to_string()),
            },
        )
        .await;
        match res {
            Err(McpError::Ipc(msg)) => {
                assert!(
                    msg.contains("requires a live Martensite dev session"),
                    "{msg}"
                );
            }
            other => panic!("expected offline IPC hint, got {other:?}"),
        }
    }

    #[test]
    fn tool_annotations_classify_lifecycle_tools() {
        for ann in [DoctorTool::annotations(), ReloadStatusTool::annotations()] {
            let ann = ann.expect("annotations");
            assert_eq!(ann.read_only_hint, Some(true));
        }
        let hot = HotReloadTool::annotations().expect("annotations");
        assert_eq!(hot.read_only_hint, Some(false));
        assert_eq!(hot.destructive_hint, Some(true));
        assert_eq!(hot.idempotent_hint, Some(false));
        let scaff = ScaffoldWidgetTool::annotations().expect("annotations");
        assert_eq!(scaff.read_only_hint, Some(false));
        assert_eq!(scaff.destructive_hint, Some(false));
    }
}
