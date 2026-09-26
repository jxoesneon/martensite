//! Command-line interface parsing and dispatch for `cargo-martensite`.
//!
//! The CLI is invoked through Cargo's custom-subcommand mechanism as
//! `cargo martensite <subcommand>`. When Cargo forwards the invocation to this
//! binary it inserts the literal subcommand name (`martensite`) as the first
//! positional argument, so [`parse_args`] transparently strips it when present.
//!
//! # Examples
//!
//! ```
//! use cargo_martensite::cli::{parse_args, Command};
//!
//! // Cargo forwards `cargo martensite dev` as `["martensite", "dev"]`.
//! let args = vec!["martensite".to_string(), "dev".to_string()];
//! let cmd = parse_args(&args).unwrap();
//! assert!(matches!(cmd, Command::Dev { watch: true, .. }));
//! ```

use crate::hot_reload::{HotReloadConfig, HotReloadState, ReloadError};
use std::fmt;
use std::process;

/// The default development server port used when none is supplied on the CLI.
pub const DEFAULT_DEV_PORT: u16 = 8765;

/// A parsed top-level command for the `cargo-martensite` toolchain.
///
/// Each variant corresponds to a user-facing subcommand. The data carried by a
/// variant captures the flags that alter its behaviour.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Command {
    /// `cargo martensite dev` — launch the hot-reload development loop.
    Dev {
        /// Whether to watch source files and reload on change.
        watch: bool,
        /// The development server port to bind.
        port: u16,
        /// The guest crate name to build (defaults to current package).
        package: Option<String>,
    },
    /// `cargo martensite build` — compile the guest crate as a cdylib.
    Build {
        /// Whether to build in release mode.
        release: bool,
        /// The guest crate name to build (defaults to current package).
        package: Option<String>,
    },
    /// `cargo martensite new <name>` — create a new Martensite project from a template.
    New {
        /// Project name or path to create.
        name: String,
        /// Template to use (`app`, `bare`, `dashboard`). Defaults to `app`.
        template: Option<String>,
    },
    /// `cargo martensite init` — generate missing DX context into an existing project.
    Init {
        /// Target directory (defaults to current directory).
        path: Option<String>,
        /// Only write AGENTS.md.
        agents_only: bool,
        /// Only write design-lint.toml.
        lint_only: bool,
        /// Template to draw defaults from (`app`, `dashboard`). Defaults to `app`.
        template: Option<String>,
    },
    /// `cargo martensite doctor` — diagnose the development environment.
    Doctor {
        /// Whether to apply safe remediations automatically.
        fix: bool,
        /// Target project directory (defaults to current directory).
        path: Option<String>,
    },
    /// `cargo martensite check` — composite format, clippy, and design lint checks.
    Check {
        /// Whether to check with `--all-features`.
        all_features: bool,
        /// Specific package to check.
        package: Option<String>,
        /// Whether to apply automatic fixes.
        fix: bool,
        /// Target project directory (defaults to current directory).
        path: Option<String>,
    },
    /// `cargo martensite lint` — evaluate design standards in offline or attach mode.
    Lint {
        /// Optional path to serialized scene or dump file (`--scene <path>`).
        scene: Option<String>,
        /// Whether to apply Safe autofix ops.
        fix: bool,
        /// Whether to force Risky autofixes.
        force: bool,
        /// Disable recursive autofix convergence.
        no_recursive: bool,
        /// Maximum recursion passes for autofix.
        max_recursiveness: usize,
        /// Standards to enable (e.g. `wcag`, `isa101`).
        standards: Vec<String>,
        /// Minimum severity to report (e.g. `warn`, `error`, `info`).
        severity: Option<String>,
        /// Substring filter for node paths.
        filter: Option<String>,
        /// Output format (`text` or `json`).
        format: String,
        /// Socket path override for dev-channel IPC.
        socket: Option<String>,
        /// Allow connecting when dev app version mismatches CLI.
        allow_version_mismatch: bool,
    },
    /// `cargo martensite inspect` — headless widget inspector attached to running dev app.
    Inspect {
        /// Stream updates / follow changes in the app.
        follow: bool,
        /// Wait for user click in app and inspect selected node.
        pick: bool,
        /// Output format (`text` or `json`).
        format: String,
        /// Socket path override for dev-channel IPC.
        socket: Option<String>,
        /// Allow connecting when dev app version mismatches CLI.
        allow_version_mismatch: bool,
    },
    /// `cargo martensite help` — print usage information.
    Help,
    /// `cargo martensite --version` — print the toolchain version.
    Version,
}

/// Errors that can arise while parsing or executing a CLI command.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CliError {
    /// An unknown or malformed subcommand was supplied.
    UnknownCommand(String),
    /// A flag was provided with an invalid or missing value.
    InvalidArgument {
        /// The flag name that produced the error.
        flag: String,
        /// A human-readable explanation.
        reason: String,
    },
    /// No subcommand was supplied at all.
    MissingCommand,
    /// An underlying system or build process failure occurred while executing
    /// a command.
    ExecutionFailed(String),
    /// Lint findings at Warn or above were detected (exit code 1).
    LintFindings(String),
    /// Version mismatch between CLI and running dev app (exit code 3).
    VersionMismatch {
        /// App version reported.
        app_version: String,
        /// CLI version.
        cli_version: String,
    },
    /// Infrastructure failure (e.g. cannot connect to socket, dev session not found) (exit code 3).
    InfrastructureFailure(String),
}

impl CliError {
    /// Returns the uniform process exit code for this error:
    /// - 1 for lint/doctor/check findings or execution failures
    /// - 2 for usage / argument parsing errors
    /// - 3 for infrastructure failures and version mismatches
    pub fn exit_code(&self) -> i32 {
        match self {
            CliError::UnknownCommand(_)
            | CliError::InvalidArgument { .. }
            | CliError::MissingCommand => 2,
            CliError::LintFindings(_) | CliError::ExecutionFailed(_) => 1,
            CliError::VersionMismatch { .. } | CliError::InfrastructureFailure(_) => 3,
        }
    }
}

impl fmt::Display for CliError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            CliError::UnknownCommand(cmd) => {
                write!(f, "unknown command: `{cmd}`")
            }
            CliError::InvalidArgument { flag, reason } => {
                write!(f, "invalid argument for `{flag}`: {reason}")
            }
            CliError::MissingCommand => {
                write!(
                    f,
                    "no subcommand supplied (expected `dev`, `build`, `lint`, `inspect`, `doctor`, `check`, or `help`)"
                )
            }
            CliError::ExecutionFailed(msg) => {
                write!(f, "command execution failed: {msg}")
            }
            CliError::LintFindings(msg) => {
                write!(f, "design-lint: {msg}")
            }
            CliError::VersionMismatch {
                app_version,
                cli_version,
            } => {
                write!(
                    f,
                    "version mismatch: app version `{app_version}` does not match CLI version `{cli_version}` (pass `--allow-version-mismatch` to override)"
                )
            }
            CliError::InfrastructureFailure(msg) => {
                write!(f, "infrastructure failure: {msg}")
            }
        }
    }
}

impl std::error::Error for CliError {}

impl From<ReloadError> for CliError {
    fn from(err: ReloadError) -> Self {
        CliError::ExecutionFailed(err.to_string())
    }
}

impl From<crate::scaffold::ScaffoldError> for CliError {
    fn from(err: crate::scaffold::ScaffoldError) -> Self {
        CliError::ExecutionFailed(err.to_string())
    }
}

impl From<crate::lint::LintError> for CliError {
    fn from(err: crate::lint::LintError) -> Self {
        match err {
            crate::lint::LintError::Findings(summary) => CliError::LintFindings(format!(
                "{} warning(s), {} error(s) found",
                summary.warnings, summary.errors
            )),
            crate::lint::LintError::DevChannel(
                crate::dev_channel::DevChannelError::VersionMismatch {
                    app_version,
                    cli_version,
                },
            ) => CliError::VersionMismatch {
                app_version,
                cli_version,
            },
            crate::lint::LintError::DevChannel(err) => {
                CliError::InfrastructureFailure(err.to_string())
            }
            other => CliError::ExecutionFailed(other.to_string()),
        }
    }
}

impl From<crate::inspect::InspectError> for CliError {
    fn from(err: crate::inspect::InspectError) -> Self {
        match err {
            crate::inspect::InspectError::DevChannel(
                crate::dev_channel::DevChannelError::VersionMismatch {
                    app_version,
                    cli_version,
                },
            ) => CliError::VersionMismatch {
                app_version,
                cli_version,
            },
            crate::inspect::InspectError::DevChannel(err) => {
                CliError::InfrastructureFailure(err.to_string())
            }
            other => CliError::ExecutionFailed(other.to_string()),
        }
    }
}

/// Parses the raw argument vector into a [`Command`].
///
/// The leading program name (if present) is ignored, and the Cargo-injected
/// `martensite` subcommand name is stripped when it appears as the first
/// positional token.
///
/// # Examples
///
/// ```
/// use cargo_martensite::cli::{parse_args, Command};
///
/// // argv[0] is the program name and is ignored.
/// let args = vec!["cargo-martensite".to_string(), "build".to_string(), "--release".to_string()];
/// let cmd = parse_args(&args).unwrap();
/// assert!(matches!(cmd, Command::Build { release: true, package: None }));
/// ```
pub fn parse_args(args: &[String]) -> Result<Command, CliError> {
    // Drop the program name when present (argv[0]).
    let mut tokens: Vec<&str> = if args.is_empty() {
        Vec::new()
    } else {
        args[1..].iter().map(|s| s.as_str()).collect()
    };

    // Cargo forwards `cargo martensite ...` as `martensite ...`; strip it.
    if tokens.first().map(|s| *s == "martensite").unwrap_or(false) {
        tokens.remove(0);
    }

    if tokens.is_empty() {
        return Err(CliError::MissingCommand);
    }

    let sub = tokens[0];
    let rest = &tokens[1..];

    match sub {
        "new" => parse_new(rest),
        "init" => parse_init(rest),
        "doctor" => parse_doctor(rest),
        "check" => parse_check(rest),
        "lint" => parse_lint(rest),
        "inspect" => parse_inspect(rest),
        "dev" => parse_dev(rest),
        "build" => parse_build(rest),
        "help" | "--help" | "-h" => Ok(Command::Help),
        "--version" | "-V" | "version" => Ok(Command::Version),
        other => Err(CliError::UnknownCommand(other.to_string())),
    }
}

/// Parses flags for the `dev` subcommand.
fn parse_dev(rest: &[&str]) -> Result<Command, CliError> {
    let mut watch = true;
    let mut port = DEFAULT_DEV_PORT;
    let mut package: Option<String> = None;

    let mut i = 0;
    while i < rest.len() {
        match rest[i] {
            "--watch" => watch = true,
            "--no-watch" => watch = false,
            "--port" | "-p" => {
                i += 1;
                let raw = rest.get(i).ok_or_else(|| CliError::InvalidArgument {
                    flag: "--port".to_string(),
                    reason: "missing value".to_string(),
                })?;
                port = raw.parse().map_err(|_| CliError::InvalidArgument {
                    flag: "--port".to_string(),
                    reason: format!("`{raw}` is not a valid port number"),
                })?;
            }
            "--package" => {
                i += 1;
                let raw = rest.get(i).ok_or_else(|| CliError::InvalidArgument {
                    flag: "--package".to_string(),
                    reason: "missing value".to_string(),
                })?;
                package = Some((*raw).to_string());
            }
            other => {
                return Err(CliError::InvalidArgument {
                    flag: other.to_string(),
                    reason: "unknown flag for `dev`".to_string(),
                });
            }
        }
        i += 1;
    }

    Ok(Command::Dev {
        watch,
        port,
        package,
    })
}

/// Parses flags for the `build` subcommand.
fn parse_build(rest: &[&str]) -> Result<Command, CliError> {
    let mut release = false;
    let mut package: Option<String> = None;

    let mut i = 0;
    while i < rest.len() {
        match rest[i] {
            "--release" => release = true,
            "--debug" => release = false,
            "--package" => {
                i += 1;
                let raw = rest.get(i).ok_or_else(|| CliError::InvalidArgument {
                    flag: "--package".to_string(),
                    reason: "missing value".to_string(),
                })?;
                package = Some((*raw).to_string());
            }
            other => {
                return Err(CliError::InvalidArgument {
                    flag: other.to_string(),
                    reason: "unknown flag for `build`".to_string(),
                });
            }
        }
        i += 1;
    }

    Ok(Command::Build { release, package })
}

/// Parses flags for the `new` subcommand.
fn parse_new(rest: &[&str]) -> Result<Command, CliError> {
    let mut name: Option<String> = None;
    let mut template: Option<String> = None;

    let mut i = 0;
    while i < rest.len() {
        match rest[i] {
            "--template" | "-t" => {
                i += 1;
                let raw = rest.get(i).ok_or_else(|| CliError::InvalidArgument {
                    flag: "--template".to_string(),
                    reason: "missing value".to_string(),
                })?;
                template = Some((*raw).to_string());
            }
            other if other.starts_with('-') => {
                return Err(CliError::InvalidArgument {
                    flag: other.to_string(),
                    reason: "unknown flag for `new`".to_string(),
                });
            }
            pos => {
                if name.is_none() {
                    name = Some(pos.to_string());
                } else {
                    return Err(CliError::InvalidArgument {
                        flag: pos.to_string(),
                        reason: "unexpected multiple positional arguments for `new`".to_string(),
                    });
                }
            }
        }
        i += 1;
    }

    let name = name.ok_or_else(|| CliError::InvalidArgument {
        flag: "<name>".to_string(),
        reason: "missing project name for `new`".to_string(),
    })?;

    Ok(Command::New { name, template })
}

/// Parses flags for the `init` subcommand.
fn parse_init(rest: &[&str]) -> Result<Command, CliError> {
    let mut path: Option<String> = None;
    let mut agents_only = false;
    let mut lint_only = false;
    let mut template: Option<String> = None;

    let mut i = 0;
    while i < rest.len() {
        match rest[i] {
            "--agents" => agents_only = true,
            "--lint" => lint_only = true,
            "--template" | "-t" => {
                i += 1;
                let raw = rest.get(i).ok_or_else(|| CliError::InvalidArgument {
                    flag: "--template".to_string(),
                    reason: "missing value".to_string(),
                })?;
                template = Some((*raw).to_string());
            }
            other if other.starts_with('-') => {
                return Err(CliError::InvalidArgument {
                    flag: other.to_string(),
                    reason: "unknown flag for `init`".to_string(),
                });
            }
            pos => {
                if path.is_none() {
                    path = Some(pos.to_string());
                } else {
                    return Err(CliError::InvalidArgument {
                        flag: pos.to_string(),
                        reason: "unexpected multiple positional arguments for `init`".to_string(),
                    });
                }
            }
        }
        i += 1;
    }

    Ok(Command::Init {
        path,
        agents_only,
        lint_only,
        template,
    })
}

/// Parses flags for the `doctor` subcommand.
fn parse_doctor(rest: &[&str]) -> Result<Command, CliError> {
    let mut fix = false;
    let mut path: Option<String> = None;

    let mut i = 0;
    while i < rest.len() {
        match rest[i] {
            "--fix" => fix = true,
            "--path" => {
                i += 1;
                let raw = rest.get(i).ok_or_else(|| CliError::InvalidArgument {
                    flag: "--path".to_string(),
                    reason: "missing value".to_string(),
                })?;
                path = Some((*raw).to_string());
            }
            other if other.starts_with('-') => {
                return Err(CliError::InvalidArgument {
                    flag: other.to_string(),
                    reason: "unknown flag for `doctor`".to_string(),
                });
            }
            pos => {
                if path.is_none() {
                    path = Some(pos.to_string());
                } else {
                    return Err(CliError::InvalidArgument {
                        flag: pos.to_string(),
                        reason: "unexpected multiple positional arguments for `doctor`".to_string(),
                    });
                }
            }
        }
        i += 1;
    }

    Ok(Command::Doctor { fix, path })
}

/// Parses flags for the `check` subcommand.
fn parse_check(rest: &[&str]) -> Result<Command, CliError> {
    let mut all_features = false;
    let mut package: Option<String> = None;
    let mut fix = false;
    let mut path: Option<String> = None;

    let mut i = 0;
    while i < rest.len() {
        match rest[i] {
            "--all-features" => all_features = true,
            "--package" | "-p" => {
                i += 1;
                let raw = rest.get(i).ok_or_else(|| CliError::InvalidArgument {
                    flag: "--package".to_string(),
                    reason: "missing value".to_string(),
                })?;
                package = Some((*raw).to_string());
            }
            "--fix" => fix = true,
            "--path" => {
                i += 1;
                let raw = rest.get(i).ok_or_else(|| CliError::InvalidArgument {
                    flag: "--path".to_string(),
                    reason: "missing value".to_string(),
                })?;
                path = Some((*raw).to_string());
            }
            other if other.starts_with('-') => {
                return Err(CliError::InvalidArgument {
                    flag: other.to_string(),
                    reason: "unknown flag for `check`".to_string(),
                });
            }
            pos => {
                if path.is_none() {
                    path = Some(pos.to_string());
                } else {
                    return Err(CliError::InvalidArgument {
                        flag: pos.to_string(),
                        reason: "unexpected multiple positional arguments for `check`".to_string(),
                    });
                }
            }
        }
        i += 1;
    }

    Ok(Command::Check {
        all_features,
        package,
        fix,
        path,
    })
}

/// Parses flags for the `lint` subcommand.
fn parse_lint(rest: &[&str]) -> Result<Command, CliError> {
    let mut scene: Option<String> = None;
    let mut fix = false;
    let mut force = false;
    let mut no_recursive = false;
    let mut max_recursiveness = 8usize;
    let mut standards = Vec::new();
    let mut severity = None;
    let mut filter = None;
    let mut format = "text".to_string();
    let mut socket = None;
    let mut allow_version_mismatch = false;

    let mut i = 0;
    while i < rest.len() {
        match rest[i] {
            "--scene" => {
                i += 1;
                let raw = rest.get(i).ok_or_else(|| CliError::InvalidArgument {
                    flag: "--scene".to_string(),
                    reason: "missing path value".to_string(),
                })?;
                scene = Some((*raw).to_string());
            }
            "--fix" => fix = true,
            "--force" => force = true,
            "--no-recursive" => no_recursive = true,
            "--max-recursiveness" => {
                i += 1;
                let raw = rest.get(i).ok_or_else(|| CliError::InvalidArgument {
                    flag: "--max-recursiveness".to_string(),
                    reason: "missing numeric value".to_string(),
                })?;
                max_recursiveness = raw.parse().map_err(|_| CliError::InvalidArgument {
                    flag: "--max-recursiveness".to_string(),
                    reason: format!("`{raw}` is not a valid number"),
                })?;
            }
            "--standard" | "-s" => {
                i += 1;
                let raw = rest.get(i).ok_or_else(|| CliError::InvalidArgument {
                    flag: "--standard".to_string(),
                    reason: "missing standard name".to_string(),
                })?;
                for part in raw.split(',') {
                    let trimmed = part.trim();
                    if !trimmed.is_empty() {
                        standards.push(trimmed.to_string());
                    }
                }
            }
            "--severity" => {
                i += 1;
                let raw = rest.get(i).ok_or_else(|| CliError::InvalidArgument {
                    flag: "--severity".to_string(),
                    reason: "missing severity level".to_string(),
                })?;
                severity = Some((*raw).to_string());
            }
            "--filter" => {
                i += 1;
                let raw = rest.get(i).ok_or_else(|| CliError::InvalidArgument {
                    flag: "--filter".to_string(),
                    reason: "missing filter pattern".to_string(),
                })?;
                filter = Some((*raw).to_string());
            }
            "--format" => {
                i += 1;
                let raw = rest.get(i).ok_or_else(|| CliError::InvalidArgument {
                    flag: "--format".to_string(),
                    reason: "missing format (`text` or `json`)".to_string(),
                })?;
                if *raw != "text" && *raw != "json" {
                    return Err(CliError::InvalidArgument {
                        flag: "--format".to_string(),
                        reason: format!("unknown format `{raw}` (expected `text` or `json`)"),
                    });
                }
                format = (*raw).to_string();
            }
            "--socket" => {
                i += 1;
                let raw = rest.get(i).ok_or_else(|| CliError::InvalidArgument {
                    flag: "--socket".to_string(),
                    reason: "missing socket path".to_string(),
                })?;
                socket = Some((*raw).to_string());
            }
            "--allow-version-mismatch" => allow_version_mismatch = true,
            other if other.starts_with('-') => {
                return Err(CliError::InvalidArgument {
                    flag: other.to_string(),
                    reason: "unknown flag for `lint`".to_string(),
                });
            }
            pos => {
                if scene.is_none() {
                    scene = Some(pos.to_string());
                } else {
                    return Err(CliError::InvalidArgument {
                        flag: pos.to_string(),
                        reason: "unexpected positional argument for `lint`".to_string(),
                    });
                }
            }
        }
        i += 1;
    }

    if force && !fix {
        fix = true;
    }

    Ok(Command::Lint {
        scene,
        fix,
        force,
        no_recursive,
        max_recursiveness,
        standards,
        severity,
        filter,
        format,
        socket,
        allow_version_mismatch,
    })
}

/// Parses flags for the `inspect` subcommand.
fn parse_inspect(rest: &[&str]) -> Result<Command, CliError> {
    let mut follow = false;
    let mut pick = false;
    let mut format = "text".to_string();
    let mut socket = None;
    let mut allow_version_mismatch = false;

    let mut i = 0;
    while i < rest.len() {
        match rest[i] {
            "--follow" | "-f" => follow = true,
            "--pick" | "-p" => pick = true,
            "--format" => {
                i += 1;
                let raw = rest.get(i).ok_or_else(|| CliError::InvalidArgument {
                    flag: "--format".to_string(),
                    reason: "missing format (`text` or `json`)".to_string(),
                })?;
                if *raw != "text" && *raw != "json" {
                    return Err(CliError::InvalidArgument {
                        flag: "--format".to_string(),
                        reason: format!("unknown format `{raw}` (expected `text` or `json`)"),
                    });
                }
                format = (*raw).to_string();
            }
            "--socket" => {
                i += 1;
                let raw = rest.get(i).ok_or_else(|| CliError::InvalidArgument {
                    flag: "--socket".to_string(),
                    reason: "missing socket path".to_string(),
                })?;
                socket = Some((*raw).to_string());
            }
            "--allow-version-mismatch" => allow_version_mismatch = true,
            other => {
                return Err(CliError::InvalidArgument {
                    flag: other.to_string(),
                    reason: "unknown flag for `inspect`".to_string(),
                });
            }
        }
        i += 1;
    }

    Ok(Command::Inspect {
        follow,
        pick,
        format,
        socket,
        allow_version_mismatch,
    })
}

/// Executes a parsed [`Command`], performing any side effects.
///
/// Returns `Ok(())` on success or a [`CliError`] describing the failure. The
/// `help` and `version` variants print to stdout and exit the process; the
/// `dev` and `build` variants shell out to Cargo.
pub fn run_command(cmd: Command) -> Result<(), CliError> {
    match cmd {
        Command::Help => {
            print_help();
            Ok(())
        }
        Command::Version => {
            println!("cargo-martensite {}", env!("CARGO_PKG_VERSION"));
            Ok(())
        }
        Command::New { name, template } => run_new(&name, template.as_deref()),
        Command::Init {
            path,
            agents_only,
            lint_only,
            template,
        } => run_init(path.as_deref(), agents_only, lint_only, template.as_deref()),
        Command::Doctor { fix, path } => run_doctor_cmd(fix, path.as_deref()),
        Command::Check {
            all_features,
            package,
            fix,
            path,
        } => run_check_cmd(all_features, package, fix, path.as_deref()),
        Command::Lint {
            scene,
            fix,
            force,
            no_recursive,
            max_recursiveness,
            standards,
            severity,
            filter,
            format,
            socket,
            allow_version_mismatch,
        } => {
            let opts = crate::lint::LintOptions {
                scene: scene.map(std::path::PathBuf::from),
                fix,
                force,
                no_recursive,
                max_recursiveness,
                standards,
                severity,
                filter,
                format: crate::lint::OutputFormat::parse(&format).unwrap_or_default(),
                socket: socket.map(std::path::PathBuf::from),
                allow_version_mismatch,
            };
            run_lint_cmd(&opts)
        }
        Command::Inspect {
            follow,
            pick,
            format,
            socket,
            allow_version_mismatch,
        } => {
            let opts = crate::inspect::InspectOptions {
                follow,
                pick,
                format: crate::lint::OutputFormat::parse(&format).unwrap_or_default(),
                socket: socket.map(std::path::PathBuf::from),
                allow_version_mismatch,
            };
            run_inspect_cmd(&opts)
        }
        Command::Build { release, package } => run_build(release, package),
        Command::Dev {
            watch,
            port,
            package,
        } => run_dev(watch, port, package),
    }
}

/// Runs the `lint` subcommand.
fn run_lint_cmd(opts: &crate::lint::LintOptions) -> Result<(), CliError> {
    crate::lint::run_lint(opts)?;
    Ok(())
}

/// Runs the `inspect` subcommand.
fn run_inspect_cmd(opts: &crate::inspect::InspectOptions) -> Result<(), CliError> {
    crate::inspect::run_inspect(opts)?;
    Ok(())
}

/// Runs the `doctor` subcommand to diagnose development environment readiness.
fn run_doctor_cmd(fix: bool, path: Option<&str>) -> Result<(), CliError> {
    let opts = crate::doctor::DoctorOptions {
        fix,
        path: path.map(std::path::PathBuf::from),
    };
    let report = crate::doctor::run_doctor(&opts);
    report.print_report();

    if report.is_success() {
        Ok(())
    } else {
        Err(CliError::ExecutionFailed(format!(
            "doctor reported {} failure(s)",
            report.failures().len()
        )))
    }
}

/// Runs the `check` subcommand composite check (fmt + clippy + design-lint).
fn run_check_cmd(
    all_features: bool,
    package: Option<String>,
    fix: bool,
    path: Option<&str>,
) -> Result<(), CliError> {
    let opts = crate::check::CheckOptions {
        all_features,
        workspace: package.is_none(),
        package,
        fix,
        path: path.map(std::path::PathBuf::from),
    };
    let report =
        crate::check::run_check(&opts).map_err(|e| CliError::ExecutionFailed(e.to_string()))?;

    if report.is_success() {
        Ok(())
    } else {
        Err(CliError::ExecutionFailed(
            "one or more check legs failed".to_string(),
        ))
    }
}

/// Runs the `new` subcommand to scaffold a project from a template.
fn run_new(name: &str, template: Option<&str>) -> Result<(), CliError> {
    let kind = if let Some(t) = template {
        crate::scaffold::TemplateKind::parse(t).map_err(|e| CliError::InvalidArgument {
            flag: "--template".to_string(),
            reason: e.to_string(),
        })?
    } else {
        crate::scaffold::TemplateKind::App
    };

    let opts = crate::scaffold::ScaffoldOptions::new(name).with_template(kind);
    let _path = crate::scaffold::scaffold_project(&opts)
        .map_err(|e| CliError::ExecutionFailed(e.to_string()))?;

    println!("Scaffolded new Martensite project `{name}` (template: {kind}).\n");
    println!("Next steps:");
    println!("  cd {name}");
    println!("  cargo martensite doctor");
    println!("  cargo martensite dev");

    Ok(())
}

/// Runs the `init` subcommand to generate missing DX context files into an existing project.
fn run_init(
    path: Option<&str>,
    agents_only: bool,
    lint_only: bool,
    template: Option<&str>,
) -> Result<(), CliError> {
    let kind = if let Some(t) = template {
        crate::scaffold::TemplateKind::parse(t).map_err(|e| CliError::InvalidArgument {
            flag: "--template".to_string(),
            reason: e.to_string(),
        })?
    } else {
        crate::scaffold::TemplateKind::App
    };

    let target_dir = path
        .map(std::path::PathBuf::from)
        .unwrap_or_else(|| std::path::PathBuf::from("."));

    let opts = crate::scaffold::InitOptions::new()
        .with_template(kind)
        .with_agents_only(agents_only)
        .with_lint_only(lint_only);

    let results = crate::scaffold::init_project(&target_dir, &opts)
        .map_err(|e| CliError::ExecutionFailed(e.to_string()))?;

    for status in results {
        match status {
            crate::scaffold::FileInitStatus::Created(p) => {
                println!("Created: {}", p.display());
            }
            crate::scaffold::FileInitStatus::Unchanged(p) => {
                println!("Unchanged: {}", p.display());
            }
            crate::scaffold::FileInitStatus::DiffersNotOverwritten(p) => {
                println!("Differs (preserved, not overwritten): {}", p.display());
            }
        }
    }

    Ok(())
}

/// Prints the help text to stdout.
fn print_help() {
    println!(
        "cargo-martensite {version} — Martensite developer toolchain\n\
         \n\
         USAGE:\n    \
         cargo martensite <SUBCOMMAND> [OPTIONS]\n\
         \n\
         SUBCOMMANDS:\n    \
         new      Create a new Martensite project from a template\n    \
         init     Generate missing DX context into an existing project\n    \
         doctor   Diagnose development environment and toolchain readiness\n    \
         check    Composite pre-commit check (fmt + clippy + design lint)\n    \
         lint     Evaluate design standards in offline or dev-channel attach mode\n    \
         inspect  Headless widget inspector attached to running dev app\n    \
         dev      Launch the hot-reload development loop [default: --watch]\n    \
         build    Compile the guest crate as a cdylib\n    \
         help     Print this message\n    \
         version  Print the toolchain version\n\
         \n\
         OPTIONS:\n    \
         --template <T>, -t <T>  Template kind: app, bare, dashboard (new, init)\n    \
         --agents                Only generate AGENTS.md (init)\n    \
         --lint                  Only generate design-lint.toml (init)\n    \
         --fix                   Apply safe fixes/remediations (doctor, check, lint)\n    \
         --force                 Also apply risky autofixes (lint)\n    \
         --no-recursive          Single-pass autofix (lint)\n    \
         --max-recursiveness <N> Maximum recursion passes for autofix (lint)\n    \
         --scene <path>          Serialized scene or dump file for offline mode (lint)\n    \
         --standard <S>, -s <S>  Select design standard filter (lint)\n    \
         --severity <S>          Minimum severity threshold (lint)\n    \
         --filter <STR>          Path substring filter (lint)\n    \
         --follow, -f            Stream live widget tree updates (inspect)\n    \
         --pick, -p              Wait for click in app to inspect node (inspect)\n    \
         --format <F>            Output presentation format: text, json (lint, inspect)\n    \
         --socket <P>            Dev channel socket path override (lint, inspect)\n    \
         --allow-version-mismatch Allow connecting on version mismatch (lint, inspect)\n    \
         --all-features          Check with all feature flags enabled (check)\n    \
         --package <P>, -p <P>   Target a specific workspace package (dev, build, check)\n    \
         --watch / --no-watch    Toggle file watching (dev)\n    \
         --port <N>, -p <N>      Development server port (dev, default {port})\n    \
         --release               Build in release mode (build)\n",
        version = env!("CARGO_PKG_VERSION"),
        port = DEFAULT_DEV_PORT,
    );
}

/// Runs the `build` subcommand by building the guest crate as a cdylib.
fn run_build(release: bool, package: Option<String>) -> Result<(), CliError> {
    let guest_crate = resolve_guest_crate(package);
    let config = HotReloadConfig {
        guest_crate: guest_crate.clone(),
        output_dir: std::path::PathBuf::from("target/martensite"),
        watch_paths: vec![],
        poll_interval_ms: 100,
    };

    let state = HotReloadState::new();
    let version = state.current_version + 1;

    // Build the guest crate as a cdylib.
    let _path = crate::hot_reload::build_guest_crate(&config, version)
        .map_err(|e| CliError::ExecutionFailed(e.to_string()))?;

    if release {
        // For release builds, also run cargo build --release.
        let status = process::Command::new("cargo")
            .args(["build", "--release", "--package"])
            .arg(&guest_crate)
            .status()
            .map_err(|e| CliError::ExecutionFailed(format!("failed to spawn cargo: {e}")))?;
        if !status.success() {
            return Err(CliError::ExecutionFailed(format!(
                "cargo build --release exited with {status}"
            )));
        }
    }

    println!("built guest crate `{guest_crate}` as cdylib (version {version})");
    Ok(())
}

/// Runs the `dev` subcommand, optionally entering the hot-reload watch loop.
fn run_dev(watch: bool, port: u16, package: Option<String>) -> Result<(), CliError> {
    let guest_crate = resolve_guest_crate(package);
    let config = HotReloadConfig {
        guest_crate: guest_crate.clone(),
        output_dir: std::path::PathBuf::from("target/martensite"),
        watch_paths: vec![std::path::PathBuf::from("src")],
        poll_interval_ms: 100,
    };

    println!("cargo-martensite dev: guest=`{guest_crate}` port={port} watch={watch}");

    if !watch {
        // One-shot build of the guest cdylib.
        let mut state = HotReloadState::new();
        let _ = build_and_record(&config, &mut state)?;
        return Ok(());
    }

    let mut state = HotReloadState::new();
    let mut watcher =
        crate::hot_reload::FileWatcher::new(config.watch_paths.clone(), config.poll_interval());
    println!("watching {} path(s) for changes", config.watch_paths.len());

    loop {
        let changed = watcher.check_for_changes();
        if !changed.is_empty() {
            for path in &changed {
                println!("change detected: {}", path.display());
            }
            match build_and_record(&config, &mut state) {
                Ok(version) => {
                    println!(
                        "reloaded guest v{version} in {} ms",
                        state.last_reload_duration_ms
                    );
                    if !crate::hot_reload::is_within_reload_budget(state.last_reload_duration_ms) {
                        eprintln!(
                            "warning: reload exceeded 350 ms budget ({} ms)",
                            state.last_reload_duration_ms
                        );
                    }
                }
                Err(ReloadError::BuildFailed(msg)) => {
                    eprintln!("build failed: {msg}");
                }
                Err(other) => {
                    eprintln!("reload error: {other}");
                }
            }
        }
        std::thread::sleep(config.poll_interval());
    }
}

/// Resolves the guest crate name from CLI argument or Cargo.toml.
///
/// If `package` is provided, it is used directly. Otherwise, reads the
/// package name from the current directory's `Cargo.toml`. If that
/// fails, defaults to `"guest"`.
fn resolve_guest_crate(package: Option<String>) -> String {
    if let Some(name) = package {
        return name;
    }

    // Try to read the package name from Cargo.toml.
    if let Ok(content) = std::fs::read_to_string("Cargo.toml") {
        for line in content.lines() {
            let trimmed = line.trim();
            if let Some(name) = trimmed.strip_prefix("name = ") {
                let name = name.trim_matches('"');
                if !name.is_empty() {
                    return name.to_string();
                }
            }
        }
    }

    "guest".to_string()
}

/// Performs a single build cycle and records timing in `state`.
fn build_and_record(
    config: &HotReloadConfig,
    state: &mut HotReloadState,
) -> Result<u64, crate::hot_reload::ReloadError> {
    crate::hot_reload::reload_cycle(config, state)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn args(parts: &[&str]) -> Vec<String> {
        parts.iter().map(|s| s.to_string()).collect()
    }

    #[test]
    fn parse_dev_default_watches() {
        let cmd = parse_args(&args(&["martensite", "dev"])).unwrap();
        assert_eq!(
            cmd,
            Command::Dev {
                watch: true,
                package: None,
                port: DEFAULT_DEV_PORT,
            }
        );
    }

    #[test]
    fn parse_dev_no_watch() {
        let cmd = parse_args(&args(&["martensite", "dev", "--no-watch"])).unwrap();
        assert_eq!(
            cmd,
            Command::Dev {
                watch: false,
                package: None,
                port: DEFAULT_DEV_PORT,
            }
        );
    }

    #[test]
    fn parse_dev_custom_port() {
        let cmd = parse_args(&args(&["martensite", "dev", "--port", "9999"])).unwrap();
        assert_eq!(
            cmd,
            Command::Dev {
                watch: true,
                package: None,
                port: 9999,
            }
        );
    }

    #[test]
    fn parse_dev_short_port_flag() {
        let cmd = parse_args(&args(&["martensite", "dev", "-p", "4242"])).unwrap();
        assert_eq!(
            cmd,
            Command::Dev {
                watch: true,
                package: None,
                port: 4242,
            }
        );
    }

    #[test]
    fn parse_dev_missing_port_value() {
        let err = parse_args(&args(&["martensite", "dev", "--port"])).unwrap_err();
        assert!(matches!(err, CliError::InvalidArgument { ref flag, .. } if flag == "--port"));
    }

    #[test]
    fn parse_dev_invalid_port_value() {
        let err = parse_args(&args(&["martensite", "dev", "--port", "notanumber"])).unwrap_err();
        assert!(
            matches!(err, CliError::InvalidArgument { ref flag, ref reason }
            if flag == "--port" && reason.contains("not a valid port"))
        );
    }

    #[test]
    fn parse_build_default_debug() {
        let cmd = parse_args(&args(&["martensite", "build"])).unwrap();
        assert_eq!(
            cmd,
            Command::Build {
                release: false,
                package: None
            }
        );
    }

    #[test]
    fn parse_build_release() {
        let cmd = parse_args(&args(&["martensite", "build", "--release"])).unwrap();
        assert_eq!(
            cmd,
            Command::Build {
                release: true,
                package: None
            }
        );
    }

    #[test]
    fn parse_build_debug_overrides_release() {
        let cmd = parse_args(&args(&["martensite", "build", "--release", "--debug"])).unwrap();
        assert_eq!(
            cmd,
            Command::Build {
                release: false,
                package: None
            }
        );
    }

    #[test]
    fn parse_help_variants() {
        for variant in &["help", "--help", "-h"] {
            let cmd = parse_args(&args(&["martensite", variant])).unwrap();
            assert_eq!(cmd, Command::Help);
        }
    }

    #[test]
    fn parse_version_variants() {
        for variant in &["--version", "-V", "version"] {
            let cmd = parse_args(&args(&["martensite", variant])).unwrap();
            assert_eq!(cmd, Command::Version);
        }
    }

    #[test]
    fn parse_strips_cargo_subcommand_prefix() {
        let cmd = parse_args(&args(&["cargo-martensite", "martensite", "build"])).unwrap();
        assert_eq!(
            cmd,
            Command::Build {
                release: false,
                package: None
            }
        );
    }

    #[test]
    fn parse_without_cargo_prefix() {
        let cmd = parse_args(&args(&["cargo-martensite", "build"])).unwrap();
        assert_eq!(
            cmd,
            Command::Build {
                release: false,
                package: None
            }
        );
    }

    #[test]
    fn parse_missing_command_errors() {
        let err = parse_args(&args(&["cargo-martensite"])).unwrap_err();
        assert_eq!(err, CliError::MissingCommand);
    }

    #[test]
    fn parse_empty_args_errors() {
        let err = parse_args(&[]).unwrap_err();
        assert_eq!(err, CliError::MissingCommand);
    }

    #[test]
    fn parse_unknown_command_errors() {
        let err = parse_args(&args(&["martensite", "frobnicate"])).unwrap_err();
        assert!(matches!(err, CliError::UnknownCommand(ref s) if s == "frobnicate"));
    }

    #[test]
    fn parse_unknown_dev_flag_errors() {
        let err = parse_args(&args(&["martensite", "dev", "--bogus"])).unwrap_err();
        assert!(matches!(err, CliError::InvalidArgument { ref flag, .. } if flag == "--bogus"));
    }

    #[test]
    fn parse_unknown_build_flag_errors() {
        let err = parse_args(&args(&["martensite", "build", "--bogus"])).unwrap_err();
        assert!(matches!(err, CliError::InvalidArgument { ref flag, .. } if flag == "--bogus"));
    }

    #[test]
    fn cli_error_display_messages() {
        assert!(CliError::MissingCommand
            .to_string()
            .contains("no subcommand"));
        assert!(CliError::UnknownCommand("x".into())
            .to_string()
            .contains("x"));
        assert!(CliError::ExecutionFailed("boom".into())
            .to_string()
            .contains("boom"));
    }

    #[test]
    fn run_version_succeeds() {
        assert!(run_command(Command::Version).is_ok());
    }

    #[test]
    fn run_help_succeeds() {
        assert!(run_command(Command::Help).is_ok());
    }

    #[test]
    fn parse_new_default_template() {
        let cmd = parse_args(&args(&["martensite", "new", "my-app"])).unwrap();
        assert_eq!(
            cmd,
            Command::New {
                name: "my-app".to_string(),
                template: None,
            }
        );
    }

    #[test]
    fn parse_new_with_template() {
        let cmd = parse_args(&args(&[
            "martensite",
            "new",
            "my-app",
            "--template",
            "dashboard",
        ]))
        .unwrap();
        assert_eq!(
            cmd,
            Command::New {
                name: "my-app".to_string(),
                template: Some("dashboard".to_string()),
            }
        );
    }

    #[test]
    fn parse_new_short_template_flag() {
        let cmd = parse_args(&args(&["martensite", "new", "my-app", "-t", "bare"])).unwrap();
        assert_eq!(
            cmd,
            Command::New {
                name: "my-app".to_string(),
                template: Some("bare".to_string()),
            }
        );
    }

    #[test]
    fn parse_new_missing_name_errors() {
        let err = parse_args(&args(&["martensite", "new"])).unwrap_err();
        assert!(matches!(err, CliError::InvalidArgument { ref flag, .. } if flag == "<name>"));
    }

    #[test]
    fn parse_new_unknown_flag_errors() {
        let err = parse_args(&args(&["martensite", "new", "my-app", "--bogus"])).unwrap_err();
        assert!(matches!(err, CliError::InvalidArgument { ref flag, .. } if flag == "--bogus"));
    }

    #[test]
    fn parse_init_defaults() {
        let cmd = parse_args(&args(&["martensite", "init"])).unwrap();
        assert_eq!(
            cmd,
            Command::Init {
                path: None,
                agents_only: false,
                lint_only: false,
                template: None,
            }
        );
    }

    #[test]
    fn parse_init_with_flags_and_path() {
        let cmd = parse_args(&args(&[
            "martensite",
            "init",
            "some/path",
            "--agents",
            "-t",
            "dashboard",
        ]))
        .unwrap();
        assert_eq!(
            cmd,
            Command::Init {
                path: Some("some/path".to_string()),
                agents_only: true,
                lint_only: false,
                template: Some("dashboard".to_string()),
            }
        );
    }

    #[test]
    fn parse_init_lint_only() {
        let cmd = parse_args(&args(&["martensite", "init", "--lint"])).unwrap();
        assert_eq!(
            cmd,
            Command::Init {
                path: None,
                agents_only: false,
                lint_only: true,
                template: None,
            }
        );
    }

    #[test]
    fn parse_init_unknown_flag_errors() {
        let err = parse_args(&args(&["martensite", "init", "--bogus"])).unwrap_err();
        assert!(matches!(err, CliError::InvalidArgument { ref flag, .. } if flag == "--bogus"));
    }

    #[test]
    fn parse_doctor_default() {
        let cmd = parse_args(&args(&["martensite", "doctor"])).unwrap();
        assert_eq!(
            cmd,
            Command::Doctor {
                fix: false,
                path: None
            }
        );
    }

    #[test]
    fn parse_doctor_fix() {
        let cmd = parse_args(&args(&["martensite", "doctor", "--fix"])).unwrap();
        assert_eq!(
            cmd,
            Command::Doctor {
                fix: true,
                path: None
            }
        );
    }

    #[test]
    fn parse_doctor_with_path() {
        let cmd = parse_args(&args(&[
            "martensite",
            "doctor",
            "--fix",
            "--path",
            "custom/dir",
        ]))
        .unwrap();
        assert_eq!(
            cmd,
            Command::Doctor {
                fix: true,
                path: Some("custom/dir".to_string()),
            }
        );
    }

    #[test]
    fn parse_doctor_positional_path() {
        let cmd = parse_args(&args(&["martensite", "doctor", "custom/dir"])).unwrap();
        assert_eq!(
            cmd,
            Command::Doctor {
                fix: false,
                path: Some("custom/dir".to_string()),
            }
        );
    }

    #[test]
    fn parse_doctor_unknown_flag_errors() {
        let err = parse_args(&args(&["martensite", "doctor", "--bogus"])).unwrap_err();
        assert!(matches!(err, CliError::InvalidArgument { ref flag, .. } if flag == "--bogus"));
    }

    #[test]
    fn parse_check_default() {
        let cmd = parse_args(&args(&["martensite", "check"])).unwrap();
        assert_eq!(
            cmd,
            Command::Check {
                all_features: false,
                package: None,
                fix: false,
                path: None,
            }
        );
    }

    #[test]
    fn parse_check_options() {
        let cmd = parse_args(&args(&[
            "martensite",
            "check",
            "--all-features",
            "-p",
            "my_pkg",
            "--fix",
        ]))
        .unwrap();
        assert_eq!(
            cmd,
            Command::Check {
                all_features: true,
                package: Some("my_pkg".to_string()),
                fix: true,
                path: None,
            }
        );
    }

    #[test]
    fn parse_check_unknown_flag_errors() {
        let err = parse_args(&args(&["martensite", "check", "--unknown"])).unwrap_err();
        assert!(matches!(err, CliError::InvalidArgument { ref flag, .. } if flag == "--unknown"));
    }

    #[test]
    fn parse_lint_defaults() {
        let cmd = parse_args(&args(&["martensite", "lint"])).unwrap();
        assert_eq!(
            cmd,
            Command::Lint {
                scene: None,
                fix: false,
                force: false,
                no_recursive: false,
                max_recursiveness: 8,
                standards: Vec::new(),
                severity: None,
                filter: None,
                format: "text".to_string(),
                socket: None,
                allow_version_mismatch: false,
            }
        );
    }

    #[test]
    fn parse_lint_all_flags() {
        let cmd = parse_args(&args(&[
            "martensite",
            "lint",
            "--scene",
            "dump.bin",
            "--fix",
            "--force",
            "--no-recursive",
            "--max-recursiveness",
            "4",
            "-s",
            "wcag,isa101",
            "--severity",
            "warn",
            "--filter",
            "Panel",
            "--format",
            "json",
            "--socket",
            "/tmp/test.sock",
            "--allow-version-mismatch",
        ]))
        .unwrap();

        assert_eq!(
            cmd,
            Command::Lint {
                scene: Some("dump.bin".to_string()),
                fix: true,
                force: true,
                no_recursive: true,
                max_recursiveness: 4,
                standards: vec!["wcag".to_string(), "isa101".to_string()],
                severity: Some("warn".to_string()),
                filter: Some("Panel".to_string()),
                format: "json".to_string(),
                socket: Some("/tmp/test.sock".to_string()),
                allow_version_mismatch: true,
            }
        );
    }

    #[test]
    fn parse_lint_positional_scene() {
        let cmd = parse_args(&args(&["martensite", "lint", "my_scene.bin"])).unwrap();
        assert_eq!(
            cmd,
            Command::Lint {
                scene: Some("my_scene.bin".to_string()),
                fix: false,
                force: false,
                no_recursive: false,
                max_recursiveness: 8,
                standards: Vec::new(),
                severity: None,
                filter: None,
                format: "text".to_string(),
                socket: None,
                allow_version_mismatch: false,
            }
        );
    }

    #[test]
    fn parse_lint_invalid_format_errors() {
        let err = parse_args(&args(&["martensite", "lint", "--format", "yaml"])).unwrap_err();
        assert!(
            matches!(err, CliError::InvalidArgument { ref flag, ref reason } if flag == "--format" && reason.contains("yaml"))
        );
    }

    #[test]
    fn parse_lint_unknown_flag_errors() {
        let err = parse_args(&args(&["martensite", "lint", "--bogus"])).unwrap_err();
        assert!(matches!(err, CliError::InvalidArgument { ref flag, .. } if flag == "--bogus"));
    }

    #[test]
    fn parse_inspect_defaults() {
        let cmd = parse_args(&args(&["martensite", "inspect"])).unwrap();
        assert_eq!(
            cmd,
            Command::Inspect {
                follow: false,
                pick: false,
                format: "text".to_string(),
                socket: None,
                allow_version_mismatch: false,
            }
        );
    }

    #[test]
    fn parse_inspect_all_flags() {
        let cmd = parse_args(&args(&[
            "martensite",
            "inspect",
            "--follow",
            "--pick",
            "--format",
            "json",
            "--socket",
            "/tmp/app.sock",
            "--allow-version-mismatch",
        ]))
        .unwrap();

        assert_eq!(
            cmd,
            Command::Inspect {
                follow: true,
                pick: true,
                format: "json".to_string(),
                socket: Some("/tmp/app.sock".to_string()),
                allow_version_mismatch: true,
            }
        );
    }

    #[test]
    fn parse_inspect_short_flags() {
        let cmd = parse_args(&args(&["martensite", "inspect", "-f", "-p"])).unwrap();
        assert_eq!(
            cmd,
            Command::Inspect {
                follow: true,
                pick: true,
                format: "text".to_string(),
                socket: None,
                allow_version_mismatch: false,
            }
        );
    }

    #[test]
    fn parse_inspect_invalid_format_errors() {
        let err = parse_args(&args(&["martensite", "inspect", "--format", "csv"])).unwrap_err();
        assert!(
            matches!(err, CliError::InvalidArgument { ref flag, ref reason } if flag == "--format" && reason.contains("csv"))
        );
    }

    #[test]
    fn parse_inspect_unknown_flag_errors() {
        let err = parse_args(&args(&["martensite", "inspect", "--bogus"])).unwrap_err();
        assert!(matches!(err, CliError::InvalidArgument { ref flag, .. } if flag == "--bogus"));
    }

    #[test]
    fn exit_codes_conformance() {
        assert_eq!(CliError::MissingCommand.exit_code(), 2);
        assert_eq!(CliError::UnknownCommand("x".into()).exit_code(), 2);
        assert_eq!(
            CliError::InvalidArgument {
                flag: "-x".into(),
                reason: "r".into()
            }
            .exit_code(),
            2
        );
        assert_eq!(CliError::ExecutionFailed("fail".into()).exit_code(), 1);
        assert_eq!(CliError::LintFindings("findings".into()).exit_code(), 1);
        assert_eq!(
            CliError::VersionMismatch {
                app_version: "1.0".into(),
                cli_version: "2.0".into()
            }
            .exit_code(),
            3
        );
        assert_eq!(
            CliError::InfrastructureFailure("no socket".into()).exit_code(),
            3
        );
    }
}
