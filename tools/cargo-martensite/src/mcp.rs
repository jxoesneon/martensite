//! Martensite MCP server entry point (ADR-0039, `docs/dx/MCP.md` §2).
//!
//! Provides the `cargo martensite mcp` subcommand: hosts the `martensite-mcp`
//! server on stdio JSON-RPC so AI agents (Devin, Claude Code, Cursor,
//! Windsurf) can inspect the live widget arena, Taffy layout chains, reactive
//! signals, AccessKit tree, and design lint findings.
//!
//! **stdout is the MCP wire.** This module never writes to stdout; all
//! diagnostics are emitted on stderr.
//!
//! # Examples
//!
//! ```no_run
//! use cargo_martensite::mcp::run_mcp;
//!
//! // Blocks until the host agent closes the stdio transport.
//! let _ = run_mcp(None, None, None, false);
//! ```

use std::path::PathBuf;

use martensite_mcp::{serve_stdio, McpError, McpServerOptions};

use crate::cli::CliError;

/// Executes the `cargo martensite mcp` subcommand.
///
/// Builds a [`McpServerOptions`], spins up a multi-threaded Tokio runtime,
/// and blocks on [`serve_stdio`] until the host agent disconnects or the
/// transport fails.
///
/// # Errors
///
/// * [`CliError::InfrastructureFailure`] for socket/IPC transport failures
///   ([`McpError::Ipc`], [`McpError::Io`]).
/// * [`CliError::VersionMismatch`] when the dev app rejects the D1 handshake
///   and `--allow-version-mismatch` was not supplied.
/// * [`CliError::ExecutionFailed`] for runtime startup or internal server
///   failures.
pub fn run_mcp(
    socket: Option<PathBuf>,
    scene: Option<PathBuf>,
    workspace: Option<PathBuf>,
    allow_version_mismatch: bool,
    offline: bool,
) -> Result<(), CliError> {
    let opts = McpServerOptions {
        socket,
        scene,
        workspace_root: workspace,
        allow_version_mismatch,
        offline,
    };

    eprintln!("cargo-martensite mcp: serving MCP over stdio");

    let runtime = tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()
        .map_err(|e| CliError::ExecutionFailed(format!("tokio runtime init failed: {e}")))?;

    runtime.block_on(serve_stdio(opts)).map_err(map_mcp_error)
}

/// Maps an [`McpError`] onto the CLI error taxonomy (uniform exit codes).
fn map_mcp_error(err: McpError) -> CliError {
    match err {
        McpError::Ipc(msg) => CliError::InfrastructureFailure(msg),
        McpError::Io(err) => CliError::InfrastructureFailure(err.to_string()),
        McpError::VersionMismatch { expected, actual } => CliError::VersionMismatch {
            app_version: actual,
            cli_version: expected,
        },
        other => CliError::ExecutionFailed(other.to_string()),
    }
}
