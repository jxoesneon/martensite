//! Composite pre-commit check command executing format, clippy, and design lint.
//!
//! Provides the `cargo martensite check` subcommand specified in `docs/dx/CLI.md`.
//!
//! Executes the standard three-command contributor verification workflow in a single invocation:
//! 1. `cargo fmt --check` (or `cargo fmt` with `--fix`)
//! 2. `cargo clippy --workspace --all-targets -- -D warnings` (plus `--all-features` when requested)
//! 3. `martensite-design-lint` configuration and rule verification
//!
//! The exit code is the logical AND of all executed legs, and compiler/tool outputs are
//! preserved verbatim without buffering or reformatting to guarantee editor jump-to-error
//! navigation continues working.
//!
//! # Examples
//!
//! ```
//! use cargo_martensite::check::{CheckOptions, CheckReport};
//!
//! let options = CheckOptions::default();
//! assert!(options.workspace);
//! ```

use std::fmt;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

use martensite_design_lint::LintConfig;

/// Options configuring the composite `check` run.
///
/// # Examples
///
/// ```
/// use cargo_martensite::check::CheckOptions;
///
/// let opts = CheckOptions {
///     all_features: true,
///     workspace: true,
///     package: None,
///     fix: false,
///     path: None,
/// };
/// assert!(opts.all_features);
/// ```
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CheckOptions {
    /// Whether to check with `--all-features` in addition to default features.
    pub all_features: bool,
    /// Whether to check all workspace members (defaults to true).
    pub workspace: bool,
    /// Specific package to check (if restricting scope).
    pub package: Option<String>,
    /// Apply automatic fixes (`cargo fmt`, `clippy --fix`).
    pub fix: bool,
    /// Working directory for command execution.
    pub path: Option<PathBuf>,
}

impl Default for CheckOptions {
    fn default() -> Self {
        Self {
            all_features: false,
            workspace: true,
            package: None,
            fix: false,
            path: None,
        }
    }
}

/// The result of an individual verification leg.
///
/// # Examples
///
/// ```
/// use cargo_martensite::check::LegResult;
///
/// let leg = LegResult::pass("cargo fmt", "cargo fmt --all -- --check");
/// assert!(leg.success);
/// assert_eq!(leg.exit_code, 0);
/// ```
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LegResult {
    /// Friendly leg name (e.g. `cargo fmt`, `cargo clippy (default)`).
    pub name: String,
    /// Exact command line executed.
    pub command: String,
    /// Process exit code (0 for success).
    pub exit_code: i32,
    /// Whether the leg completed successfully.
    pub success: bool,
    /// Optional failure details or notes.
    pub notes: Option<String>,
}

impl LegResult {
    /// Creates a successful leg result with exit code 0.
    #[must_use]
    pub fn pass(name: impl Into<String>, command: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            command: command.into(),
            exit_code: 0,
            success: true,
            notes: None,
        }
    }

    /// Creates a failed leg result with non-zero exit code.
    #[must_use]
    pub fn fail(
        name: impl Into<String>,
        command: impl Into<String>,
        exit_code: i32,
        notes: Option<String>,
    ) -> Self {
        Self {
            name: name.into(),
            command: command.into(),
            exit_code,
            success: false,
            notes,
        }
    }
}

/// Aggregated report from a composite check execution.
///
/// # Examples
///
/// ```
/// use cargo_martensite::check::{CheckReport, LegResult};
///
/// let mut report = CheckReport::new();
/// report.add(LegResult::pass("fmt", "cargo fmt"));
/// assert!(report.is_success());
/// ```
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct CheckReport {
    /// Results for each executed leg.
    pub legs: Vec<LegResult>,
}

impl CheckReport {
    /// Creates a new empty [`CheckReport`].
    #[must_use]
    pub fn new() -> Self {
        Self { legs: Vec::new() }
    }

    /// Records a completed leg result.
    pub fn add(&mut self, result: LegResult) {
        self.legs.push(result);
    }

    /// Returns `true` if all legs succeeded (exit code 0).
    #[must_use]
    pub fn is_success(&self) -> bool {
        self.legs.iter().all(|l| l.success)
    }

    /// Returns the suggested process exit code (0 if all pass, 1 otherwise).
    #[must_use]
    pub fn exit_code(&self) -> i32 {
        if self.is_success() {
            0
        } else {
            1
        }
    }

    /// Formats and displays the composite check summary.
    pub fn print_summary(&self) {
        println!("\n==========================================");
        println!("Martensite Pre-Commit Check Summary");
        println!("==========================================");

        for leg in &self.legs {
            if leg.success {
                println!(" \x1b[32m✓\x1b[0m {} (exit: 0)", leg.name);
            } else {
                println!(" \x1b[31m✗\x1b[0m {} (exit: {})", leg.name, leg.exit_code);
                if let Some(ref note) = leg.notes {
                    println!("   \x1b[31m→ {}\x1b[0m", note);
                }
            }
        }

        println!("------------------------------------------");
        let passed = self.legs.iter().filter(|l| l.success).count();
        let total = self.legs.len();

        if self.is_success() {
            println!("\x1b[32mAll {} check legs passed cleanly.\x1b[0m\n", total);
        } else {
            println!(
                "\x1b[31mCheck failed: {} of {} legs failed.\x1b[0m\n",
                total - passed,
                total
            );
        }
    }
}

/// Errors that can occur when dispatching the check command.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CheckError {
    /// An underlying system command could not be spawned.
    SpawnFailed {
        /// Command name.
        command: String,
        /// Reason description.
        reason: String,
    },
}

impl fmt::Display for CheckError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            CheckError::SpawnFailed { command, reason } => {
                write!(f, "failed to spawn `{command}`: {reason}")
            }
        }
    }
}

impl std::error::Error for CheckError {}

/// Executes the full pre-commit check composite (fmt + clippy + lint).
///
/// # Examples
///
/// ```no_run
/// use cargo_martensite::check::{CheckOptions, run_check};
///
/// let report = run_check(&CheckOptions::default()).unwrap();
/// if !report.is_success() {
///     std::process::exit(report.exit_code());
/// }
/// ```
pub fn run_check(options: &CheckOptions) -> Result<CheckReport, CheckError> {
    let mut report = CheckReport::new();
    let working_dir = options.path.as_deref();

    // -----------------------------------------------------------------------
    // Leg 1: cargo fmt
    // -----------------------------------------------------------------------
    println!("\n\x1b[1;34m>>> [1/3] Checking formatting (cargo fmt)...\x1b[0m");
    let fmt_res = run_fmt_leg(options, working_dir)?;
    report.add(fmt_res);

    // -----------------------------------------------------------------------
    // Leg 2: cargo clippy (default features)
    // -----------------------------------------------------------------------
    println!("\n\x1b[1;34m>>> [2/3] Running Clippy lints (default features)...\x1b[0m");
    let clippy_default = run_clippy_leg(options, false, working_dir)?;
    report.add(clippy_default);

    // If requested, also run --all-features (matching CI dual-pass verification)
    if options.all_features {
        println!("\n\x1b[1;34m>>> [2b/3] Running Clippy lints (--all-features)...\x1b[0m");
        let clippy_all = run_clippy_leg(options, true, working_dir)?;
        report.add(clippy_all);
    }

    // -----------------------------------------------------------------------
    // Leg 3: design-lint verification
    // -----------------------------------------------------------------------
    println!("\n\x1b[1;34m>>> [3/3] Checking design-lint configuration...\x1b[0m");
    let lint_res = run_lint_leg(options, working_dir);
    report.add(lint_res);

    report.print_summary();

    Ok(report)
}

/// Executes the formatting check leg (`cargo fmt`).
pub(crate) fn run_fmt_leg(
    options: &CheckOptions,
    working_dir: Option<&Path>,
) -> Result<LegResult, CheckError> {
    let mut cmd = Command::new("cargo");
    cmd.arg("fmt");

    if let Some(ref pkg) = options.package {
        cmd.args(["-p", pkg]);
    } else if options.workspace {
        cmd.arg("--all");
    }

    if !options.fix {
        cmd.args(["--", "--check"]);
    }

    if let Some(dir) = working_dir {
        cmd.current_dir(dir);
    }

    // Stream directly to console to maintain colored diffs & warnings
    cmd.stdin(Stdio::inherit());
    cmd.stdout(Stdio::inherit());
    cmd.stderr(Stdio::inherit());

    let cmd_str = format!("{:?}", cmd);
    match cmd.status() {
        Ok(status) => {
            let code = status.code().unwrap_or(1);
            if status.success() {
                Ok(LegResult::pass("cargo fmt", cmd_str))
            } else {
                Ok(LegResult::fail(
                    "cargo fmt",
                    cmd_str,
                    code,
                    Some("formatting discrepancies found; run `cargo fmt` to apply".to_string()),
                ))
            }
        }
        Err(e) => Err(CheckError::SpawnFailed {
            command: "cargo fmt".to_string(),
            reason: e.to_string(),
        }),
    }
}

/// Executes the Clippy lint check leg.
pub(crate) fn run_clippy_leg(
    options: &CheckOptions,
    all_features: bool,
    working_dir: Option<&Path>,
) -> Result<LegResult, CheckError> {
    let mut cmd = Command::new("cargo");
    cmd.arg("clippy");

    if let Some(ref pkg) = options.package {
        cmd.args(["-p", pkg]);
    } else if options.workspace {
        cmd.arg("--workspace");
    }

    cmd.arg("--all-targets");

    if all_features {
        cmd.arg("--all-features");
    }

    if options.fix {
        cmd.args(["--fix", "--allow-dirty"]);
    }

    // Enforce zero warnings per CI rules
    cmd.args(["--", "-D", "warnings"]);

    if let Some(dir) = working_dir {
        cmd.current_dir(dir);
    }

    // Stream directly to console to maintain jump-to-error compiler diagnostic format
    cmd.stdin(Stdio::inherit());
    cmd.stdout(Stdio::inherit());
    cmd.stderr(Stdio::inherit());

    let name = if all_features {
        "cargo clippy (--all-features)"
    } else {
        "cargo clippy (default)"
    };
    let cmd_str = format!("{:?}", cmd);

    match cmd.status() {
        Ok(status) => {
            let code = status.code().unwrap_or(1);
            if status.success() {
                Ok(LegResult::pass(name, cmd_str))
            } else {
                Ok(LegResult::fail(
                    name,
                    cmd_str,
                    code,
                    Some("clippy warnings or errors reported".to_string()),
                ))
            }
        }
        Err(e) => Err(CheckError::SpawnFailed {
            command: "cargo clippy".to_string(),
            reason: e.to_string(),
        }),
    }
}

/// Validates `design-lint.toml` presence, syntax, and standards validity.
pub(crate) fn run_lint_leg(_options: &CheckOptions, working_dir: Option<&Path>) -> LegResult {
    let base_dir = working_dir.unwrap_or_else(|| Path::new("."));
    let config_path = base_dir.join("design-lint.toml");

    let cmd_str = format!("verify {}", config_path.display());

    if !config_path.exists() {
        return LegResult::fail(
            "design-lint",
            cmd_str,
            1,
            Some("design-lint.toml not found; run `cargo martensite init --lint`".to_string()),
        );
    }

    match fs::read_to_string(&config_path) {
        Ok(content) => match LintConfig::from_toml(&content) {
            Ok(cfg) => {
                println!(
                    " \x1b[32m✓\x1b[0m design-lint config valid: {} standards, {} path allows",
                    cfg.standards.len(),
                    cfg.allows.len()
                );
                LegResult::pass("design-lint", cmd_str)
            }
            Err(err) => LegResult::fail(
                "design-lint",
                cmd_str,
                1,
                Some(format!("syntax error in design-lint.toml: {err}")),
            ),
        },
        Err(err) => LegResult::fail(
            "design-lint",
            cmd_str,
            1,
            Some(format!("failed to read design-lint.toml: {err}")),
        ),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_check_options_defaults() {
        let opts = CheckOptions::default();
        assert!(opts.workspace);
        assert!(!opts.all_features);
        assert!(!opts.fix);
        assert!(opts.package.is_none());
    }

    #[test]
    fn test_check_report_aggregation() {
        let mut report = CheckReport::new();
        assert!(report.is_success());
        assert_eq!(report.exit_code(), 0);

        report.add(LegResult::pass("leg1", "cmd1"));
        assert!(report.is_success());

        report.add(LegResult::fail(
            "leg2",
            "cmd2",
            1,
            Some("failed".to_string()),
        ));
        assert!(!report.is_success());
        assert_eq!(report.exit_code(), 1);
    }

    #[test]
    fn test_leg_result_constructors() {
        let p = LegResult::pass("fmt", "cargo fmt");
        assert!(p.success);
        assert_eq!(p.exit_code, 0);
        assert!(p.notes.is_none());

        let f = LegResult::fail("clippy", "cargo clippy", 101, Some("warn".to_string()));
        assert!(!f.success);
        assert_eq!(f.exit_code, 101);
        assert_eq!(f.notes.as_deref(), Some("warn"));
    }
}
