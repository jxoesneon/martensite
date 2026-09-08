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
                    "no subcommand supplied (expected `dev`, `build`, or `help`)"
                )
            }
            CliError::ExecutionFailed(msg) => {
                write!(f, "command execution failed: {msg}")
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
        Command::Build { release, package } => run_build(release, package),
        Command::Dev {
            watch,
            port,
            package,
        } => run_dev(watch, port, package),
    }
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
         dev      Launch the hot-reload development loop [default: --watch]\n    \
         build    Compile the guest crate as a cdylib\n    \
         help     Print this message\n    \
         version  Print the toolchain version\n\
         \n\
         OPTIONS:\n    \
         --watch / --no-watch   Toggle file watching (dev)\n    \
         --port <N>, -p <N>     Development server port (dev, default {port})\n    \
         --release              Build in release mode (build)\n",
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
}
