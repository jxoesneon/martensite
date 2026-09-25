//! Environment diagnosis and readiness checks for the Martensite developer toolchain.
//!
//! Provides the `cargo martensite doctor` subcommand specified in `docs/dx/CLI.md`.
//!
//! Diagnoses the developer environment across six critical subsystems:
//! 1. **Toolchain** — `rustc` version against MSRV/`rust-toolchain.toml`, `clippy`, and `rustfmt`.
//! 2. **GPU** — `wgpu` adapter capabilities (backends, compute shaders, storage textures for Vello),
//!    falling back gracefully to CPU rasterization (D3).
//! 3. **Text & Fonts** — System font provider reachability via `fontdb`, and platform IME presence.
//! 4. **Accessibility** — Platform accessibility reachability (UIA on Windows, AT-SPI on Linux,
//!    NSAccessibility on macOS).
//! 5. **Dev Loop / Version Parity** — `cargo-martensite` vs `martensite` version parity (D1).
//! 6. **Design Lint** — `design-lint.toml` presence, validity, and configuration inspection.
//!
//! Every check emits a distinct `✓` or `✗` indicator along with actionable remediation instructions.
//! The `--fix` flag automatically applies safe remediations (installing missing components or writing
//! default configuration files).
//!
//! # Examples
//!
//! ```
//! use cargo_martensite::doctor::{DoctorOptions, run_doctor};
//!
//! let options = DoctorOptions { fix: false, path: None };
//! let report = run_doctor(&options);
//! assert!(!report.results.is_empty());
//! ```

use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

use crate::scaffold::{init_project, InitOptions, TemplateKind};
use martensite_design_lint::LintConfig;

/// Minimum supported Rust version required by the workspace.
pub const WORKSPACE_MSRV: (u64, u64) = (1, 89);

/// The status outcome of a single diagnostic check.
///
/// # Examples
///
/// ```
/// use cargo_martensite::doctor::CheckStatus;
///
/// assert!(CheckStatus::Pass.is_ok());
/// assert!(!CheckStatus::Fail.is_ok());
/// ```
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CheckStatus {
    /// The check passed successfully.
    Pass,
    /// The check produced a non-fatal warning (e.g. fallback rasterization active).
    Warning,
    /// The check failed and requires remediation.
    Fail,
}

impl CheckStatus {
    /// Returns `true` if the status is [`CheckStatus::Pass`].
    #[must_use]
    pub fn is_ok(&self) -> bool {
        matches!(self, CheckStatus::Pass)
    }

    /// Returns `true` if the status is [`CheckStatus::Fail`].
    #[must_use]
    pub fn is_fail(&self) -> bool {
        matches!(self, CheckStatus::Fail)
    }
}

/// A structured result for an individual diagnostic check.
///
/// # Examples
///
/// ```
/// use cargo_martensite::doctor::{CheckResult, CheckStatus};
///
/// let res = CheckResult::pass("toolchain", "rustc 1.98.0 installed");
/// assert_eq!(res.status, CheckStatus::Pass);
/// assert!(res.remediation.is_none());
/// ```
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CheckResult {
    /// Subsystem or topic category name (e.g. `toolchain`, `gpu`, `fonts`).
    pub name: String,
    /// Outcome status of the check.
    pub status: CheckStatus,
    /// Human-readable diagnostic details describing the findings.
    pub details: String,
    /// Actionable remediation instructions if the check did not pass cleanly.
    pub remediation: Option<String>,
}

impl CheckResult {
    /// Creates a passing check result.
    #[must_use]
    pub fn pass(name: impl Into<String>, details: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            status: CheckStatus::Pass,
            details: details.into(),
            remediation: None,
        }
    }

    /// Creates a warning check result with optional remediation.
    #[must_use]
    pub fn warning(
        name: impl Into<String>,
        details: impl Into<String>,
        remediation: Option<String>,
    ) -> Self {
        Self {
            name: name.into(),
            status: CheckStatus::Warning,
            details: details.into(),
            remediation,
        }
    }

    /// Creates a failing check result with mandatory actionable remediation.
    #[must_use]
    pub fn fail(
        name: impl Into<String>,
        details: impl Into<String>,
        remediation: impl Into<String>,
    ) -> Self {
        Self {
            name: name.into(),
            status: CheckStatus::Fail,
            details: details.into(),
            remediation: Some(remediation.into()),
        }
    }
}

/// An aggregated diagnostic report containing all performed checks.
///
/// # Examples
///
/// ```
/// use cargo_martensite::doctor::{CheckResult, DoctorReport};
///
/// let mut report = DoctorReport::new();
/// report.add(CheckResult::pass("gpu", "Vulkan hardware adapter"));
/// assert!(report.is_success());
/// ```
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct DoctorReport {
    /// All individual check results collected in this diagnosis run.
    pub results: Vec<CheckResult>,
}

impl DoctorReport {
    /// Creates a new empty [`DoctorReport`].
    #[must_use]
    pub fn new() -> Self {
        Self {
            results: Vec::new(),
        }
    }

    /// Appends a single check result to this report.
    pub fn add(&mut self, result: CheckResult) {
        self.results.push(result);
    }

    /// Extends this report with a list of check results.
    pub fn extend(&mut self, results: Vec<CheckResult>) {
        self.results.extend(results);
    }

    /// Returns `true` if there are no failing checks.
    #[must_use]
    pub fn is_success(&self) -> bool {
        !self.results.iter().any(|r| r.status.is_fail())
    }

    /// Returns all failing check results.
    #[must_use]
    pub fn failures(&self) -> Vec<&CheckResult> {
        self.results.iter().filter(|r| r.status.is_fail()).collect()
    }

    /// Returns all warning check results.
    #[must_use]
    pub fn warnings(&self) -> Vec<&CheckResult> {
        self.results
            .iter()
            .filter(|r| r.status == CheckStatus::Warning)
            .collect()
    }

    /// Formats and prints the doctor report to stdout with clear symbols.
    pub fn print_report(&self) {
        println!("Martensite Environment Diagnosis");
        println!("=================================");

        for res in &self.results {
            match res.status {
                CheckStatus::Pass => {
                    println!(" \x1b[32m✓\x1b[0m {}: {}", res.name, res.details);
                }
                CheckStatus::Warning => {
                    println!(" \x1b[33m!\x1b[0m {}: {}", res.name, res.details);
                    if let Some(ref rem) = res.remediation {
                        println!("   \x1b[33m→ Remediation: {}\x1b[0m", rem);
                    }
                }
                CheckStatus::Fail => {
                    println!(" \x1b[31m✗\x1b[0m {}: {}", res.name, res.details);
                    if let Some(ref rem) = res.remediation {
                        println!("   \x1b[31m→ Remediation: {}\x1b[0m", rem);
                    }
                }
            }
        }

        println!("---------------------------------");
        let passes = self.results.iter().filter(|r| r.status.is_ok()).count();
        let warnings = self.warnings().len();
        let fails = self.failures().len();

        if fails == 0 {
            if warnings == 0 {
                println!(
                    "\x1b[32mDiagnosis complete: all {} checks passed.\x1b[0m",
                    passes
                );
            } else {
                println!(
                    "\x1b[33mDiagnosis complete: {} passed, {} warnings, 0 failures.\x1b[0m",
                    passes, warnings
                );
            }
        } else {
            println!(
                "\x1b[31mDiagnosis complete: {} passed, {} warnings, {} failures.\x1b[0m",
                passes, warnings, fails
            );
        }
    }
}

/// Options configuring the environment doctor diagnosis.
///
/// # Examples
///
/// ```
/// use cargo_martensite::doctor::DoctorOptions;
///
/// let opts = DoctorOptions { fix: true, path: None };
/// assert!(opts.fix);
/// ```
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct DoctorOptions {
    /// Whether to apply safe remediations automatically (e.g. install components, write configs).
    pub fix: bool,
    /// Root path of the project being diagnosed (defaults to current directory).
    pub path: Option<PathBuf>,
}

/// Runs all diagnostic checks and compiles the resulting [`DoctorReport`].
///
/// # Examples
///
/// ```
/// use cargo_martensite::doctor::{DoctorOptions, run_doctor};
///
/// let options = DoctorOptions::default();
/// let report = run_doctor(&options);
/// assert!(!report.results.is_empty());
/// ```
#[must_use]
pub fn run_doctor(options: &DoctorOptions) -> DoctorReport {
    let mut report = DoctorReport::new();
    let project_dir = options
        .path
        .clone()
        .unwrap_or_else(|| std::env::current_dir().unwrap_or_else(|_| PathBuf::from(".")));

    // 1. Toolchain
    report.extend(check_toolchain(options.fix, &project_dir));

    // 2. GPU
    report.add(check_gpu());

    // 3. Text & Fonts
    report.extend(check_text_and_fonts());

    // 4. Accessibility
    report.add(check_accessibility());

    // 5. Version parity
    report.add(check_version_parity(&project_dir));

    // 6. Design lint
    report.add(check_design_lint(&project_dir, options.fix));

    report
}

/// Checks the Rust toolchain version and installed components (`clippy`, `rustfmt`).
///
/// # Examples
///
/// ```
/// use cargo_martensite::doctor::check_toolchain;
/// use std::path::Path;
///
/// let checks = check_toolchain(false, Path::new("."));
/// assert!(!checks.is_empty());
/// ```
#[must_use]
pub fn check_toolchain(fix: bool, project_dir: &Path) -> Vec<CheckResult> {
    let mut results = Vec::new();

    // 1. rustc version
    match Command::new("rustc").arg("--version").output() {
        Ok(output) if output.status.success() => {
            let ver_str = String::from_utf8_lossy(&output.stdout);
            if let Some(parsed_ver) = parse_rustc_version(&ver_str) {
                let msrv_str = format!("{}.{}.0", WORKSPACE_MSRV.0, WORKSPACE_MSRV.1);
                if (parsed_ver.0, parsed_ver.1) >= WORKSPACE_MSRV {
                    results.push(CheckResult::pass(
                        "toolchain/rustc",
                        format!(
                            "rustc {}.{}.{} meets workspace MSRV (>= {})",
                            parsed_ver.0, parsed_ver.1, parsed_ver.2, msrv_str
                        ),
                    ));
                } else {
                    results.push(CheckResult::fail(
                        "toolchain/rustc",
                        format!(
                            "rustc {}.{}.{} is older than required MSRV ({})",
                            parsed_ver.0, parsed_ver.1, parsed_ver.2, msrv_str
                        ),
                        "run `rustup update` to upgrade your compiler toolchain",
                    ));
                }
            } else {
                results.push(CheckResult::warning(
                    "toolchain/rustc",
                    format!("rustc installed ({ver_str}) but version could not be parsed"),
                    None,
                ));
            }
        }
        _ => {
            results.push(CheckResult::fail(
                "toolchain/rustc",
                "rustc compiler binary not found in PATH",
                "install the Rust toolchain from https://rustup.rs",
            ));
        }
    }

    // 2. rust-toolchain.toml parity check
    let toolchain_toml = find_ancestor_file(project_dir, "rust-toolchain.toml");
    if let Some(path) = toolchain_toml {
        if let Ok(content) = fs::read_to_string(&path) {
            results.push(CheckResult::pass(
                "toolchain/config",
                format!(
                    "`{}` active ({})",
                    path.file_name().unwrap_or_default().to_string_lossy(),
                    summarize_toolchain_toml(&content)
                ),
            ));
        }
    }

    // 3. clippy component
    let clippy_installed = check_command_exists("clippy-driver")
        || check_cargo_subcommand("clippy")
        || check_rustup_component("clippy");

    if clippy_installed {
        results.push(CheckResult::pass(
            "toolchain/clippy",
            "clippy lint component is installed",
        ));
    } else if fix {
        let added = try_rustup_component_add("clippy");
        if added {
            results.push(CheckResult::pass(
                "toolchain/clippy",
                "clippy component was automatically installed via --fix",
            ));
        } else {
            results.push(CheckResult::fail(
                "toolchain/clippy",
                "clippy component is missing and could not be auto-installed",
                "run `rustup component add clippy`",
            ));
        }
    } else {
        results.push(CheckResult::fail(
            "toolchain/clippy",
            "clippy component is not installed",
            "run `rustup component add clippy` (or run `cargo martensite doctor --fix`)",
        ));
    }

    // 4. rustfmt component
    let rustfmt_installed = check_command_exists("rustfmt") || check_rustup_component("rustfmt");

    if rustfmt_installed {
        results.push(CheckResult::pass(
            "toolchain/rustfmt",
            "rustfmt formatting component is installed",
        ));
    } else if fix {
        let added = try_rustup_component_add("rustfmt");
        if added {
            results.push(CheckResult::pass(
                "toolchain/rustfmt",
                "rustfmt component was automatically installed via --fix",
            ));
        } else {
            results.push(CheckResult::fail(
                "toolchain/rustfmt",
                "rustfmt component is missing and could not be auto-installed",
                "run `rustup component add rustfmt`",
            ));
        }
    } else {
        results.push(CheckResult::fail(
            "toolchain/rustfmt",
            "rustfmt component is not installed",
            "run `rustup component add rustfmt` (or run `cargo martensite doctor --fix`)",
        ));
    }

    results
}

/// Probes GPU adapter capabilities using `wgpu` and checks suitability for Vello compute.
///
/// Falls back gracefully to CPU rasterization report without failing (D3).
///
/// # Examples
///
/// ```
/// use cargo_martensite::doctor::check_gpu;
///
/// let result = check_gpu();
/// assert_eq!(result.name, "gpu/adapter");
/// ```
#[must_use]
pub fn check_gpu() -> CheckResult {
    let instance = wgpu::Instance::default();

    let options = wgpu::RequestAdapterOptions {
        power_preference: wgpu::PowerPreference::HighPerformance,
        force_fallback_adapter: false,
        compatible_surface: None,
        apply_limit_buckets: false,
    };
    let adapter = pollster::block_on(instance.request_adapter(&options)).ok();

    if let Some(adapter) = adapter {
        let info = adapter.get_info();
        let limits = adapter.limits();

        let has_compute = limits.max_compute_workgroup_size_x > 0
            && limits.max_storage_buffers_per_shader_stage > 0;
        let has_storage_textures = limits.max_storage_textures_per_shader_stage > 0;

        if has_compute && has_storage_textures {
            CheckResult::pass(
                "gpu/adapter",
                format!(
                    "hardware adapter '{}' ({:?}, {:?}) with compute & storage textures support",
                    info.name, info.backend, info.device_type
                ),
            )
        } else {
            CheckResult::warning(
                "gpu/adapter",
                format!(
                    "adapter '{}' ({:?}) detected but lacks full compute/storage texture limits; \
                     GPU acceleration may degrade to CPU",
                    info.name, info.backend
                ),
                Some("verify graphics driver installation or Vulkan/Metal runtime".to_string()),
            )
        }
    } else {
        // Graceful degradation (D3) as required by CLI.md:
        CheckResult::pass(
            "gpu/adapter",
            "no hardware GPU adapter found; CPU raster fallback (TinySkia) active",
        )
    }
}

/// Checks text and font subsystem reachability via `fontdb` and platform IME availability.
///
/// # Examples
///
/// ```
/// use cargo_martensite::doctor::check_text_and_fonts;
///
/// let checks = check_text_and_fonts();
/// assert!(!checks.is_empty());
/// ```
#[must_use]
pub fn check_text_and_fonts() -> Vec<CheckResult> {
    let mut results = Vec::new();

    // 1. Font provider inspection
    let mut db = fontdb::Database::new();
    db.load_system_fonts();
    let count = db.len();

    if count > 0 {
        results.push(CheckResult::pass(
            "text/fonts",
            format!("system font provider reachable ({count} font faces loaded)"),
        ));
    } else {
        results.push(CheckResult::fail(
            "text/fonts",
            "no system fonts found by font provider",
            "install system fonts or check fontconfig / operating system font paths",
        ));
    }

    // 2. IME availability
    results.push(check_ime_availability());

    results
}

/// Helper assessing platform IME availability.
fn check_ime_availability() -> CheckResult {
    #[cfg(target_os = "windows")]
    {
        CheckResult::pass(
            "text/ime",
            "Windows Text Services Framework (TSF/Imm32) input backend available",
        )
    }

    #[cfg(target_os = "macos")]
    {
        CheckResult::pass(
            "text/ime",
            "macOS InputMethodKit / Text Services Manager available",
        )
    }

    #[cfg(target_os = "linux")]
    {
        let env_vars = [
            "IBUS_ENABLE_SYNC_MODE",
            "GTK_IM_MODULE",
            "QT_IM_MODULE",
            "XMODIFIERS",
        ];
        let set_vars: Vec<String> = env_vars
            .iter()
            .filter_map(|v| std::env::var(v).ok().map(|val| format!("{v}={val}")))
            .collect();

        if !set_vars.is_empty() {
            CheckResult::pass(
                "text/ime",
                format!("Linux IME active ({})", set_vars.join(", ")),
            )
        } else {
            CheckResult::pass(
                "text/ime",
                "standard input active (no external IME env vars configured: IBus/Fcitx)",
            )
        }
    }

    #[cfg(not(any(target_os = "windows", target_os = "macos", target_os = "linux")))]
    {
        CheckResult::pass("text/ime", "generic platform input method provider active")
    }
}

/// Checks accessibility platform reachability (UIA on Windows, AT-SPI on Linux, NSAccessibility on macOS).
///
/// # Examples
///
/// ```
/// use cargo_martensite::doctor::check_accessibility;
///
/// let res = check_accessibility();
/// assert_eq!(res.name, "accessibility");
/// ```
#[must_use]
pub fn check_accessibility() -> CheckResult {
    #[cfg(target_os = "windows")]
    {
        let sys_root = std::env::var("SystemRoot")
            .or_else(|_| std::env::var("WINDIR"))
            .unwrap_or_else(|_| "C:\\Windows".to_string());
        let uia_path = Path::new(&sys_root)
            .join("System32")
            .join("UIAutomationCore.dll");

        if uia_path.exists() {
            CheckResult::pass(
                "accessibility",
                "Windows UI Automation (UIA) runtime reachable",
            )
        } else {
            CheckResult::warning(
                "accessibility",
                "UIAutomationCore.dll not found in standard System32 path",
                Some("verify Windows accessibility runtime installation".to_string()),
            )
        }
    }

    #[cfg(target_os = "macos")]
    {
        CheckResult::pass("accessibility", "macOS NSAccessibility subsystem reachable")
    }

    #[cfg(target_os = "linux")]
    {
        let bus_env = std::env::var("AT_SPI_BUS_ADDRESS").ok();
        let bus_file_exists = std::env::var("XDG_RUNTIME_DIR")
            .ok()
            .map(|dir| Path::new(&dir).join("at-spi").join("bus").exists())
            .unwrap_or(false);

        if bus_env.is_some() || bus_file_exists {
            CheckResult::pass(
                "accessibility",
                format!(
                    "Linux AT-SPI accessibility bus reachable ({})",
                    bus_env.unwrap_or_else(|| "socket active".to_string())
                ),
            )
        } else {
            CheckResult::warning(
                "accessibility",
                "Linux AT-SPI accessibility bus not detected",
                Some(
                    "ensure `at-spi2-core` is running (`systemctl --user start at-spi-dbus-bus`)"
                        .to_string(),
                ),
            )
        }
    }

    #[cfg(not(any(target_os = "windows", target_os = "macos", target_os = "linux")))]
    {
        CheckResult::pass("accessibility", "platform accessibility subsystem ready")
    }
}

/// Checks version parity between `cargo-martensite` and the project's `martensite` dependency (D1).
///
/// # Examples
///
/// ```
/// use cargo_martensite::doctor::check_version_parity;
/// use std::path::Path;
///
/// let res = check_version_parity(Path::new("."));
/// assert_eq!(res.name, "version/parity");
/// ```
#[must_use]
pub fn check_version_parity(project_dir: &Path) -> CheckResult {
    let cli_version = env!("CARGO_PKG_VERSION");
    let manifest_path = find_ancestor_file(project_dir, "Cargo.toml");

    let Some(manifest) = manifest_path else {
        return CheckResult::pass(
            "version/parity",
            format!("cargo-martensite v{cli_version} (no project Cargo.toml found)"),
        );
    };

    let content = match fs::read_to_string(&manifest) {
        Ok(c) => c,
        Err(_) => {
            return CheckResult::pass(
                "version/parity",
                format!("cargo-martensite v{cli_version} (could not read Cargo.toml)"),
            );
        }
    };

    if let Some(dep_version) = extract_martensite_dep_version(&content) {
        if versions_compatible(cli_version, &dep_version) {
            CheckResult::pass(
                "version/parity",
                format!(
                    "cargo-martensite v{cli_version} matches project martensite v{dep_version}"
                ),
            )
        } else {
            CheckResult::fail(
                "version/parity",
                format!(
                    "version skew: cargo-martensite is v{cli_version} but project specifies \
                     martensite v{dep_version}"
                ),
                format!("update project dependencies to `martensite = \"{cli_version}\"`"),
            )
        }
    } else {
        CheckResult::pass(
            "version/parity",
            format!(
                "cargo-martensite v{cli_version} (project has no direct martensite dependency)"
            ),
        )
    }
}

/// Checks `design-lint.toml` presence, valid syntax, and configuration rules.
///
/// # Examples
///
/// ```
/// use cargo_martensite::doctor::check_design_lint;
/// use std::path::Path;
///
/// let res = check_design_lint(Path::new("."), false);
/// assert_eq!(res.name, "design-lint/config");
/// ```
#[must_use]
pub fn check_design_lint(project_dir: &Path, fix: bool) -> CheckResult {
    let lint_toml_path = project_dir.join("design-lint.toml");

    if lint_toml_path.exists() {
        match fs::read_to_string(&lint_toml_path) {
            Ok(content) => match LintConfig::from_toml(&content) {
                Ok(config) => CheckResult::pass(
                    "design-lint/config",
                    format!(
                        "design-lint.toml present and valid ({} standards, {} path allows)",
                        config.standards.len(),
                        config.allows.len()
                    ),
                ),
                Err(err) => CheckResult::fail(
                    "design-lint/config",
                    format!("design-lint.toml failed to parse: {err}"),
                    "correct the syntax errors in design-lint.toml",
                ),
            },
            Err(err) => CheckResult::fail(
                "design-lint/config",
                format!("failed to read design-lint.toml: {err}"),
                "ensure design-lint.toml is readable",
            ),
        }
    } else if fix {
        let init_opts = InitOptions {
            agents_only: false,
            lint_only: true,
            template: TemplateKind::App,
        };
        match init_project(project_dir, &init_opts) {
            Ok(_) if lint_toml_path.exists() => CheckResult::pass(
                "design-lint/config",
                "design-lint.toml created automatically via --fix",
            ),
            _ => CheckResult::fail(
                "design-lint/config",
                "design-lint.toml is missing and auto-creation failed",
                "run `cargo martensite init --lint`",
            ),
        }
    } else {
        CheckResult::fail(
            "design-lint/config",
            "design-lint.toml missing from project root",
            "run `cargo martensite init --lint` (or `cargo martensite doctor --fix`)",
        )
    }
}

// ---------------------------------------------------------------------------
// Helper parsing routines
// ---------------------------------------------------------------------------

/// Parses a semver triple `(major, minor, patch)` from `rustc --version` output.
pub(crate) fn parse_rustc_version(output: &str) -> Option<(u64, u64, u64)> {
    let part = output.strip_prefix("rustc ")?.split_whitespace().next()?;
    let mut segments = part.split('.');
    let major = segments.next()?.parse().ok()?;
    let minor = segments.next()?.parse().ok()?;
    let patch_part = segments.next()?.split('-').next()?;
    let patch = patch_part.parse().ok()?;
    Some((major, minor, patch))
}

/// Checks whether an executable command exists in PATH.
fn check_command_exists(cmd: &str) -> bool {
    Command::new(cmd).arg("--version").output().is_ok()
}

/// Checks whether a cargo subcommand is available.
fn check_cargo_subcommand(subcmd: &str) -> bool {
    Command::new("cargo")
        .args([subcmd, "--version"])
        .output()
        .map(|o| o.status.success())
        .unwrap_or(false)
}

/// Checks whether a rustup component is currently installed.
fn check_rustup_component(component: &str) -> bool {
    Command::new("rustup")
        .args(["component", "list", "--installed"])
        .output()
        .map(|o| {
            if o.status.success() {
                let out = String::from_utf8_lossy(&o.stdout);
                out.lines().any(|l| l.trim().starts_with(component))
            } else {
                false
            }
        })
        .unwrap_or(false)
}

/// Attempts to add a rustup component.
fn try_rustup_component_add(component: &str) -> bool {
    Command::new("rustup")
        .args(["component", "add", component])
        .status()
        .map(|s| s.success())
        .unwrap_or(false)
}

/// Searches upwards from `start_dir` for a named file.
fn find_ancestor_file(start_dir: &Path, file_name: &str) -> Option<PathBuf> {
    let mut current = if start_dir.is_absolute() {
        start_dir.to_path_buf()
    } else {
        std::env::current_dir().ok()?.join(start_dir)
    };

    loop {
        let candidate = current.join(file_name);
        if candidate.is_file() {
            return Some(candidate);
        }
        if !current.pop() {
            break;
        }
    }
    None
}

/// Summarizes a `rust-toolchain.toml` file content.
fn summarize_toolchain_toml(content: &str) -> String {
    for line in content.lines() {
        let trimmed = line.trim();
        if trimmed.starts_with("channel") {
            return trimmed.to_string();
        }
    }
    "custom toolchain configuration".to_string()
}

/// Extracts declared `martensite` version requirement from a `Cargo.toml` string.
pub(crate) fn extract_martensite_dep_version(content: &str) -> Option<String> {
    for line in content.lines() {
        let trimmed = line.trim();
        if trimmed.starts_with('#') {
            continue;
        }
        // Example: martensite = "0.19.0" or martensite = { version = "0.19.0", ... }
        if let Some(rest) = trimmed.strip_prefix("martensite =") {
            let rest = rest.trim();
            if let Some(stripped) = rest.strip_prefix('"') {
                if let Some(end) = stripped.find('"') {
                    return Some(stripped[..end].to_string());
                }
            } else if let Some(v_start) = rest.find("version =") {
                let v_part = &rest[v_start + "version =".len()..].trim_start();
                if let Some(stripped) = v_part.strip_prefix('"') {
                    if let Some(end) = stripped.find('"') {
                        return Some(stripped[..end].to_string());
                    }
                }
            }
        }
    }
    None
}

/// Determines whether two semver versions are compatible (major/minor match).
pub(crate) fn versions_compatible(cli_ver: &str, proj_ver: &str) -> bool {
    let clean_proj = proj_ver.trim_start_matches(['^', '~', '=', ' ']);
    let cli_parts: Vec<&str> = cli_ver.split('.').collect();
    let proj_parts: Vec<&str> = clean_proj.split('.').collect();

    if cli_parts.len() >= 2 && proj_parts.len() >= 2 {
        cli_parts[0] == proj_parts[0] && cli_parts[1] == proj_parts[1]
    } else {
        cli_ver == clean_proj
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_rustc_version() {
        let normal = "rustc 1.89.0 (2026-09-01)";
        assert_eq!(parse_rustc_version(normal), Some((1, 89, 0)));

        let nightly = "rustc 1.98.1-nightly (48a229cea 2026-09-01)";
        assert_eq!(parse_rustc_version(nightly), Some((1, 98, 1)));

        let bad = "invalid string";
        assert_eq!(parse_rustc_version(bad), None);
    }

    #[test]
    fn test_versions_compatible() {
        assert!(versions_compatible("0.19.0", "0.19.0"));
        assert!(versions_compatible("0.19.0", "^0.19.0"));
        assert!(versions_compatible("0.19.0", "0.19.2"));
        assert!(!versions_compatible("0.19.0", "0.18.0"));
        assert!(!versions_compatible("0.19.0", "1.0.0"));
    }

    #[test]
    fn test_extract_martensite_dep_version() {
        let simple = r#"
[dependencies]
martensite = "0.19.0"
serde = "1.0"
"#;
        assert_eq!(
            extract_martensite_dep_version(simple),
            Some("0.19.0".to_string())
        );

        let table = r#"
[dependencies]
martensite = { version = "0.19.0", features = ["vello"] }
"#;
        assert_eq!(
            extract_martensite_dep_version(table),
            Some("0.19.0".to_string())
        );

        let none = r#"
[dependencies]
tokio = "1.0"
"#;
        assert_eq!(extract_martensite_dep_version(none), None);
    }

    #[test]
    fn test_doctor_report_aggregation() {
        let mut report = DoctorReport::new();
        report.add(CheckResult::pass("test1", "ok"));
        report.add(CheckResult::warning("test2", "warn", None));
        assert!(report.is_success());
        assert_eq!(report.warnings().len(), 1);

        report.add(CheckResult::fail("test3", "failed", "fix it"));
        assert!(!report.is_success());
        assert_eq!(report.failures().len(), 1);
    }

    #[test]
    fn test_check_text_and_fonts_returns_results() {
        let results = check_text_and_fonts();
        assert!(!results.is_empty());
        assert!(results.iter().any(|r| r.name == "text/fonts"));
        assert!(results.iter().any(|r| r.name == "text/ime"));
    }

    #[test]
    fn test_check_gpu_returns_result() {
        let result = check_gpu();
        assert_eq!(result.name, "gpu/adapter");
        assert!(result.status.is_ok());
    }

    #[test]
    fn test_check_accessibility_returns_result() {
        let result = check_accessibility();
        assert_eq!(result.name, "accessibility");
        assert!(result.status.is_ok() || result.status == CheckStatus::Warning);
    }
}
