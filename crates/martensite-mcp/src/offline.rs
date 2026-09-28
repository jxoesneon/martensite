//! Offline-mode context for the Martensite MCP server.
//!
//! When no live dev session is reachable the server degrades per spec §2 /
//! invariant 5 ("Fail-Safe Degradation"): tools backed by static analysis keep
//! working — scene-file linting, AST scaffolding, toolchain diagnostics, and
//! headless rendering — while live-only tools surface a structured error.
//!
//! This module owns the shared offline machinery: workspace discovery, scene
//! dump loading, workspace confinement for disk mutation, the mutation audit
//! log, and the capture-artifact directory.

use std::path::{Path, PathBuf};

use martensite_design_lint::LintScene;
use martensite_devtools::lint_bridge::{LintDump, SerializedLintScene};

use crate::error::McpError;
use crate::types::AuditRecord;

/// Magic bytes of a binary `LintDump` file (matches `cargo-martensite lint`).
const DUMP_MAGIC: &[u8; 4] = b"MLNT";

/// Offline execution context discovered once at server start.
///
/// # Examples
///
/// ```no_run
/// use martensite_mcp::offline::OfflineContext;
/// use martensite_mcp::server::McpServerOptions;
///
/// let ctx = OfflineContext::discover(&McpServerOptions::default());
/// assert!(ctx.workspace_root().is_absolute() || ctx.workspace_root() == std::path::Path::new("."));
/// ```
#[derive(Debug)]
pub struct OfflineContext {
    workspace_root: PathBuf,
    scene_path: Option<PathBuf>,
    runtime_dir: PathBuf,
}

impl OfflineContext {
    /// Builds an offline context, discovering the Cargo workspace root by
    /// walking up from the current directory (or `opts.workspace_root`).
    #[must_use]
    pub fn discover(opts: &crate::server::McpServerOptions) -> Self {
        let workspace_root = opts
            .workspace_root
            .clone()
            .or_else(find_workspace_root)
            .unwrap_or_else(|| PathBuf::from("."));
        Self {
            workspace_root,
            scene_path: opts.scene.clone(),
            runtime_dir: crate::client::default_socket_dir(),
        }
    }

    /// Root of the Cargo workspace the server was launched in.
    pub fn workspace_root(&self) -> &Path {
        &self.workspace_root
    }

    /// Scene dump path supplied via `--scene`, if any.
    pub fn scene_path(&self) -> Option<&Path> {
        self.scene_path.as_deref()
    }

    /// Runtime directory for artifacts (`$XDG_RUNTIME_DIR/martensite` or tmp).
    pub fn runtime_dir(&self) -> &Path {
        &self.runtime_dir
    }

    /// Directory for `martensite_capture_node` / headless render artifacts.
    pub fn captures_dir(&self) -> PathBuf {
        self.runtime_dir.join("captures")
    }

    /// Append-only JSON-lines audit log for guarded disk mutations.
    pub fn audit_log_path(&self) -> PathBuf {
        self.runtime_dir.join("mcp-audit.jsonl")
    }

    /// Loads the `--scene` dump file as a [`LintScene`], accepting binary
    /// `MLNT` dumps, `LintDump` JSON, or bare `SerializedLintScene` JSON.
    ///
    /// Returns `Ok(None)` when no scene path was configured.
    pub fn load_scene(&self) -> Result<Option<LintScene>, McpError> {
        let Some(path) = self.scene_path.clone() else {
            return Ok(None);
        };
        let bytes = std::fs::read(&path).map_err(|e| {
            McpError::Io(std::io::Error::new(
                e.kind(),
                format!("failed to read scene `{}`: {e}", path.display()),
            ))
        })?;

        if bytes.starts_with(DUMP_MAGIC) {
            let dump = LintDump::from_binary(&bytes).map_err(|e| {
                McpError::LintEngine(format!(
                    "corrupted binary scene dump `{}`: {e}",
                    path.display()
                ))
            })?;
            return Ok(Some(dump.to_scene()));
        }

        let text = std::str::from_utf8(&bytes).map_err(|_| {
            McpError::LintEngine(format!(
                "`{}` is neither valid binary MLNT nor UTF-8 JSON",
                path.display()
            ))
        })?;

        if let Ok(dump) = LintDump::from_json(text) {
            return Ok(Some(dump.to_scene()));
        }
        if let Ok(ser) = serde_json::from_str::<SerializedLintScene>(text) {
            return Ok(Some(ser.to_scene()));
        }

        Err(McpError::LintEngine(format!(
            "unrecognized scene dump structure in `{}`",
            path.display()
        )))
    }

    /// Canonicalizes `path` and enforces workspace confinement (spec §6.4):
    /// disk-mutating tools may only touch files under the workspace root.
    ///
    /// For paths that do not exist yet (scaffold targets), the parent is
    /// canonicalized and the file name rejoined.
    pub fn confine_to_workspace(&self, path: &Path) -> Result<PathBuf, McpError> {
        let abs = if path.is_absolute() {
            path.to_path_buf()
        } else {
            self.workspace_root.join(path)
        };
        let canonical = if abs.exists() {
            abs.canonicalize().map_err(McpError::Io)?
        } else {
            let parent = abs.parent().unwrap_or_else(|| Path::new("."));
            let canon_parent = parent.canonicalize().map_err(McpError::Io)?;
            canon_parent.join(abs.file_name().ok_or_else(|| {
                McpError::InvalidParameter(format!("path `{}` has no file name", abs.display()))
            })?)
        };
        let root = self.workspace_root.canonicalize().map_err(McpError::Io)?;
        if !canonical.starts_with(&root) {
            return Err(McpError::WorkspaceConfinementViolation(format!(
                "`{}` resolves outside the workspace root `{}`",
                canonical.display(),
                root.display()
            )));
        }
        Ok(canonical)
    }

    /// Appends a structured [`AuditRecord`] to the JSONL audit log.
    pub fn write_audit(&self, record: &AuditRecord) -> Result<(), McpError> {
        if let Some(parent) = self.audit_log_path().parent() {
            std::fs::create_dir_all(parent).map_err(McpError::Io)?;
        }
        let mut line = serde_json::to_string(record)?;
        line.push('\n');
        use std::io::Write;
        let mut f = std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(self.audit_log_path())
            .map_err(McpError::Io)?;
        f.write_all(line.as_bytes()).map_err(McpError::Io)
    }
}

/// Walks up from `start` looking for a `Cargo.toml` that declares
/// `[workspace]`, returning that directory.
fn find_workspace_root() -> Option<PathBuf> {
    let start = std::env::current_dir().ok()?;
    for dir in start.ancestors() {
        let manifest = dir.join("Cargo.toml");
        if manifest.is_file() {
            if let Ok(text) = std::fs::read_to_string(&manifest) {
                if text.contains("[workspace]") {
                    return Some(dir.to_path_buf());
                }
            }
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::server::McpServerOptions;

    #[test]
    fn confinement_rejects_escape() {
        let ctx = OfflineContext {
            workspace_root: std::env::temp_dir().join("mcp-test-ws"),
            scene_path: None,
            runtime_dir: std::env::temp_dir().join("mcp-test-rt"),
        };
        std::fs::create_dir_all(&ctx.workspace_root).expect("mkdir");
        let err = ctx.confine_to_workspace(Path::new("/etc/passwd"));
        assert!(matches!(
            err,
            Err(McpError::WorkspaceConfinementViolation(_))
        ));
    }

    #[test]
    fn confinement_accepts_inside() {
        let root = std::env::temp_dir().join("mcp-test-ws2");
        std::fs::create_dir_all(root.join("src")).expect("mkdir");
        let ctx = OfflineContext {
            workspace_root: root.clone(),
            scene_path: None,
            runtime_dir: std::env::temp_dir().join("mcp-test-rt2"),
        };
        let ok = ctx
            .confine_to_workspace(Path::new("src/lib.rs"))
            .expect("inside path accepted");
        assert!(ok.starts_with(root.canonicalize().expect("canon")));
    }

    #[test]
    fn discover_returns_context() {
        let ctx = OfflineContext::discover(&McpServerOptions::default());
        assert!(!ctx.workspace_root().as_os_str().is_empty());
    }
}
