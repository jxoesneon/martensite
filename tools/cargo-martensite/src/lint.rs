//! Design standard linting command for `cargo martensite lint`.
//!
//! Provides the `cargo martensite lint` subcommand specified in `docs/dx/CLI.md`
//! and `docs/dx/DEV_LINT.md`.
//!
//! Supports two modes of operation:
//! 1. **Offline mode** (`--scene <path>`): reads a serialized [`LintDump`] or
//!    [`LintScene`] snapshot file and runs [`martensite_design_lint::lint`].
//! 2. **Attach mode** (default when dev session is live): connects to the app's
//!    ADR-0038 dev-channel Unix domain socket, pulls the current scene/report,
//!    and outputs findings.
//!
//! Exit codes:
//! - `0`: Clean (no active findings at `Warn` or above).
//! - `1`: Violations detected at `Warn` or `Error`/`Forbid`.
//! - `2`: Usage / CLI argument parsing error.
//! - `3`: Infrastructure failure (dev session not running or version mismatch).

use std::fmt;
use std::path::{Path, PathBuf};

use martensite_design_lint::{
    autofix, lint, FixOptions, LintConfig, LintReport, LintScene, Severity, Standard,
};
use martensite_devtools::lint_bridge::{
    LintDump, SerializedLintReport, SerializedLintScene, DUMP_MAGIC,
};

use crate::dev_channel::{discover_socket, DevChannelError, DevClient};

/// Output serialization format for lint findings.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum OutputFormat {
    /// Formatted human-readable text for terminal display.
    #[default]
    Text,
    /// Machine-readable pretty-printed JSON for CI annotations and tooling.
    Json,
}

impl OutputFormat {
    /// Parses an output format from a string (`text` or `json`).
    pub fn parse(s: &str) -> Option<Self> {
        match s.to_ascii_lowercase().as_str() {
            "text" => Some(Self::Text),
            "json" => Some(Self::Json),
            _ => None,
        }
    }
}

/// Options configuring the `lint` subcommand run.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LintOptions {
    /// Optional path to a serialized scene or dump file for offline linting.
    pub scene: Option<PathBuf>,
    /// Whether to apply Safe autofix ops to the scene before evaluation.
    pub fix: bool,
    /// Whether to force Risky autofixes in addition to Safe ones.
    pub force: bool,
    /// Disable recursive autofix convergence (single-pass only).
    pub no_recursive: bool,
    /// Maximum recursive passes for autofix convergence (default: 8).
    pub max_recursiveness: usize,
    /// Standards filter (e.g. `wcag`, `isa101`, `hick-fitts`).
    pub standards: Vec<String>,
    /// Minimum severity threshold to report (e.g. `warn`, `error`, `info`).
    pub severity: Option<String>,
    /// Substring filter for matching node paths.
    pub filter: Option<String>,
    /// Output presentation format (`text` or `json`).
    pub format: OutputFormat,
    /// Socket path override for dev channel attach mode.
    pub socket: Option<PathBuf>,
    /// Whether to ignore version handshake mismatches.
    pub allow_version_mismatch: bool,
}

impl Default for LintOptions {
    fn default() -> Self {
        Self {
            scene: None,
            fix: false,
            force: false,
            no_recursive: false,
            max_recursiveness: 8,
            standards: Vec::new(),
            severity: None,
            filter: None,
            format: OutputFormat::Text,
            socket: None,
            allow_version_mismatch: false,
        }
    }
}

/// Summary of lint findings returned by [`run_lint`].
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct LintSummary {
    /// Active errors and fatal forbidden violations.
    pub errors: usize,
    /// Active warnings.
    pub warnings: usize,
    /// Active informational review prompts.
    pub infos: usize,
    /// Suppressed findings allowed by configuration or inline markers.
    pub suppressed: usize,
    /// Stale allow entries that matched no findings.
    pub unused_allows: usize,
    /// Total gating findings at `Warn` or above (`errors + warnings`).
    pub gating_findings: usize,
}

impl LintSummary {
    /// Whether the report passed with zero gating violations.
    pub fn is_clean(&self) -> bool {
        self.gating_findings == 0
    }
}

/// Errors produced during lint execution.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LintError {
    /// Gating lint findings were detected (exit code 1).
    Findings(LintSummary),
    /// Failed to load scene or dump file.
    InvalidDump(String),
    /// Dev channel communication failure (exit code 3).
    DevChannel(DevChannelError),
    /// Invalid configuration file or standard name.
    Config(String),
    /// I/O error during execution.
    Io(String),
}

impl fmt::Display for LintError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            LintError::Findings(summary) => {
                write!(
                    f,
                    "design-lint: {} warning(s), {} error(s) found",
                    summary.warnings, summary.errors
                )
            }
            LintError::InvalidDump(msg) => write!(f, "failed to load scene dump: {msg}"),
            LintError::DevChannel(err) => write!(f, "{err}"),
            LintError::Config(msg) => write!(f, "lint configuration error: {msg}"),
            LintError::Io(msg) => write!(f, "I/O error: {msg}"),
        }
    }
}

impl std::error::Error for LintError {}

impl From<DevChannelError> for LintError {
    fn from(err: DevChannelError) -> Self {
        LintError::DevChannel(err)
    }
}

/// Executes the `cargo martensite lint` command according to [`LintOptions`].
pub fn run_lint(options: &LintOptions) -> Result<LintSummary, LintError> {
    let (_scene, mut report) = if let Some(dump_path) = &options.scene {
        // Mode 1: Offline mode reading serialized scene or dump.
        let loaded_scene = load_scene_from_file(dump_path)?;
        let config = resolve_lint_config(options)?;

        let mut scene = loaded_scene;
        if options.fix || options.force {
            let fix_opts = FixOptions {
                force: options.force,
                recursive: !options.no_recursive,
                max_depth: options.max_recursiveness,
            };
            let _fix_report = autofix(&mut scene, &config, &fix_opts);
        }

        let evaluated_report = lint(&scene, &config);
        (scene, evaluated_report)
    } else {
        // Mode 2: Attach mode connecting to dev-channel socket.
        let sock_path = discover_socket(options.socket.as_deref())?;
        let mut client = DevClient::connect(&sock_path, options.allow_version_mismatch)?;

        if options.fix || options.force {
            client.lint_apply(
                options.force,
                !options.no_recursive,
                options.max_recursiveness,
            )?
        } else {
            client.lint_pull()?
        }
    };

    // Apply CLI filtering overrides.
    apply_filters(&mut report, options);

    // Render report output according to requested format.
    match options.format {
        OutputFormat::Json => {
            let serialized = SerializedLintReport::from(&report);
            let json = serde_json::to_string_pretty(&serialized)
                .map_err(|e| LintError::Io(e.to_string()))?;
            println!("{json}");
        }
        OutputFormat::Text => {
            let text = report.to_text();
            if text.trim().is_empty() {
                println!("design-lint: clean (0 findings)");
            } else {
                print!("{text}");
            }
        }
    }

    let summary = compute_summary(&report);

    if summary.is_clean() {
        Ok(summary)
    } else {
        Err(LintError::Findings(summary))
    }
}

/// Reads a [`LintScene`] from a disk path (supports binary [`LintDump`], JSON [`LintDump`],
/// and raw [`SerializedLintScene`]).
pub fn load_scene_from_file(path: &Path) -> Result<LintScene, LintError> {
    let bytes = std::fs::read(path)
        .map_err(|e| LintError::Io(format!("failed to read `{}`: {e}", path.display())))?;

    if bytes.starts_with(DUMP_MAGIC) {
        let dump = LintDump::from_binary(&bytes).map_err(|e| {
            LintError::InvalidDump(format!("corrupted binary dump `{}`: {e}", path.display()))
        })?;
        return Ok(dump.to_scene());
    }

    let text = match std::str::from_utf8(&bytes) {
        Ok(s) => s,
        Err(_) => {
            return Err(LintError::InvalidDump(format!(
                "`{}` is neither valid binary MLNT nor UTF-8 JSON",
                path.display()
            )));
        }
    };

    if let Ok(dump) = LintDump::from_json(text) {
        return Ok(dump.to_scene());
    }

    if let Ok(ser_scene) = serde_json::from_str::<SerializedLintScene>(text) {
        return Ok(ser_scene.to_scene());
    }

    Err(LintError::InvalidDump(format!(
        "unrecognized dump structure in `{}`",
        path.display()
    )))
}

/// Resolves the [`LintConfig`] to use for evaluation, incorporating project `design-lint.toml`
/// and CLI flag overrides.
fn resolve_lint_config(options: &LintOptions) -> Result<LintConfig, LintError> {
    let mut config = if let Ok(Some(cfg)) = LintConfig::from_file(Path::new("design-lint.toml")) {
        cfg
    } else {
        LintConfig::default()
    };

    if !options.standards.is_empty() {
        let mut selected = Vec::new();
        for key in &options.standards {
            let std = Standard::from_key(key)
                .ok_or_else(|| LintError::Config(format!("unknown design standard key `{key}`")))?;
            selected.push(std);
        }
        config = config.only_standards(&selected);
    }

    Ok(config)
}

/// Filters findings in `report` in-place according to `--filter` and `--severity` options.
fn apply_filters(report: &mut LintReport, options: &LintOptions) {
    if let Some(pattern) = &options.filter {
        report.findings.retain(|f| f.path.contains(pattern));
        report.suppressed.retain(|f| f.path.contains(pattern));
    }

    if let Some(sev_str) = &options.severity {
        if let Some(min_sev) = Severity::from_key(sev_str) {
            report.findings.retain(|f| f.severity >= min_sev);
            report.suppressed.retain(|f| f.severity >= min_sev);
        }
    }
}

/// Aggregates counts into a [`LintSummary`].
fn compute_summary(report: &LintReport) -> LintSummary {
    let mut errors = 0;
    let mut warnings = 0;
    let mut infos = 0;

    for f in &report.findings {
        match f.severity {
            Severity::Error | Severity::Forbid => errors += 1,
            Severity::Warn => warnings += 1,
            Severity::Info => infos += 1,
            Severity::Off => {}
        }
    }

    let gating_findings = errors + warnings;

    LintSummary {
        errors,
        warnings,
        infos,
        suppressed: report.suppressed.len(),
        unused_allows: report.unused_allows.len(),
        gating_findings,
    }
}
