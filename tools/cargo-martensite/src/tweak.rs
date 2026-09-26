//! Live Property Tweaks CLI subcommand implementation (W5).
//!
//! Provides the command-line interface for inspecting live property tweaks
//! and applying emitted source patches back into Rust source files.
//!
//! # Commands
//!
//! - `cargo martensite tweak` / `cargo martensite tweak dump`:
//!   Connects to the running dev session over the dev channel (or reads a patch file)
//!   and prints active tweaks and source patches.
//! - `cargo martensite tweak apply [--dry-run]`:
//!   Reads emitted source patches and writes the updated values directly into
//!   the corresponding source files, triggering hot reload.
//!
//! # Honesty Contract
//!
//! All source patches are written in canonical format:
//! `file:line: .method(old_value) -> .method(new_value)`
//! or `file:line: old_value -> new_value`.
//!
//! Applying patches is idempotent and verifiable using `--dry-run`.

#![forbid(unsafe_code)]

use std::fmt;
use std::fs;
use std::path::PathBuf;

use crate::dev_channel::{discover_socket, DevChannelError, DevClient};

/// Target sub-action for `cargo martensite tweak`.
///
/// # Examples
///
/// ```
/// use cargo_martensite::tweak::TweakAction;
///
/// assert_eq!(TweakAction::Dump.as_str(), "dump");
/// assert_eq!(TweakAction::Apply.as_str(), "apply");
/// ```
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum TweakAction {
    /// Inspect active live tweaks and print source patches.
    #[default]
    Dump,
    /// Apply source patches directly into source code files.
    Apply,
}

impl TweakAction {
    /// Returns the string representation of this action.
    ///
    /// # Examples
    ///
    /// ```
    /// use cargo_martensite::tweak::TweakAction;
    ///
    /// assert_eq!(TweakAction::Dump.as_str(), "dump");
    /// ```
    pub const fn as_str(&self) -> &'static str {
        match self {
            Self::Dump => "dump",
            Self::Apply => "apply",
        }
    }
}

/// Options configuring the `cargo martensite tweak` command.
///
/// # Examples
///
/// ```
/// use cargo_martensite::tweak::{TweakAction, TweakOptions};
///
/// let opts = TweakOptions::default();
/// assert_eq!(opts.action, TweakAction::Dump);
/// assert!(!opts.dry_run);
/// ```
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct TweakOptions {
    /// Action to execute (`dump` or `apply`).
    pub action: TweakAction,
    /// Optional dev channel socket path override.
    pub socket: Option<PathBuf>,
    /// Allow version mismatch when connecting to dev app.
    pub allow_version_mismatch: bool,
    /// Path to a file containing patches (for offline apply or dump).
    pub file: Option<PathBuf>,
    /// Dry-run mode for apply (preview changes without modifying files).
    pub dry_run: bool,
}

impl TweakOptions {
    /// Creates default options for `tweak dump`.
    ///
    /// # Examples
    ///
    /// ```
    /// use cargo_martensite::tweak::{TweakAction, TweakOptions};
    ///
    /// let opts = TweakOptions::dump();
    /// assert_eq!(opts.action, TweakAction::Dump);
    /// ```
    pub fn dump() -> Self {
        Self {
            action: TweakAction::Dump,
            ..Default::default()
        }
    }

    /// Creates default options for `tweak apply`.
    ///
    /// # Examples
    ///
    /// ```
    /// use cargo_martensite::tweak::{TweakAction, TweakOptions};
    ///
    /// let opts = TweakOptions::apply(false);
    /// assert_eq!(opts.action, TweakAction::Apply);
    /// assert!(!opts.dry_run);
    /// ```
    pub fn apply(dry_run: bool) -> Self {
        Self {
            action: TweakAction::Apply,
            dry_run,
            ..Default::default()
        }
    }
}

/// A parsed live tweak source patch hunk.
///
/// Matches formats:
/// - `file:line: .prop(old) -> .prop(new)`
/// - `file:line:col: .prop(old) -> .prop(new)`
/// - `file:line: old -> new`
///
/// # Examples
///
/// ```
/// use std::path::PathBuf;
/// use cargo_martensite::tweak::ParsedPatch;
///
/// let patch = ParsedPatch {
///     file: PathBuf::from("src/ui.rs"),
///     line: 142,
///     old_text: ".padding(12.0)".to_string(),
///     new_text: ".padding(16.0)".to_string(),
/// };
/// assert_eq!(patch.line, 142);
/// ```
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct ParsedPatch {
    /// Path to target source file.
    pub file: PathBuf,
    /// 1-indexed target line number.
    pub line: usize,
    /// Exact text or invocation to replace.
    pub old_text: String,
    /// Replacement text or invocation.
    pub new_text: String,
}

impl ParsedPatch {
    /// Constructs a new `ParsedPatch`.
    ///
    /// # Examples
    ///
    /// ```
    /// use cargo_martensite::tweak::ParsedPatch;
    ///
    /// let patch = ParsedPatch::new("src/main.rs", 42, "10", "20");
    /// assert_eq!(patch.line, 42);
    /// ```
    pub fn new(
        file: impl Into<PathBuf>,
        line: usize,
        old_text: impl Into<String>,
        new_text: impl Into<String>,
    ) -> Self {
        Self {
            file: file.into(),
            line,
            old_text: old_text.into(),
            new_text: new_text.into(),
        }
    }

    /// Formats the patch in canonical human-readable syntax.
    ///
    /// # Examples
    ///
    /// ```
    /// use cargo_martensite::tweak::ParsedPatch;
    ///
    /// let patch = ParsedPatch::new("src/main.rs", 42, ".gap(4.0)", ".gap(8.0)");
    /// assert_eq!(patch.format_canonical(), "src/main.rs:42: .gap(4.0) -> .gap(8.0)");
    /// ```
    pub fn format_canonical(&self) -> String {
        format!(
            "{}:{}: {} -> {}",
            self.file.display(),
            self.line,
            self.old_text,
            self.new_text
        )
    }
}

/// Result of evaluating or applying a single patch.
///
/// # Examples
///
/// ```
/// use std::path::PathBuf;
/// use cargo_martensite::tweak::PatchResult;
///
/// let res = PatchResult {
///     file: PathBuf::from("src/ui.rs"),
///     line: 142,
///     old_text: "12.0".to_string(),
///     new_text: "16.0".to_string(),
///     applied: true,
///     dry_run: false,
///     actual_line: 142,
/// };
/// assert!(res.applied);
/// ```
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PatchResult {
    /// File containing the patch target.
    pub file: PathBuf,
    /// Target line originally referenced.
    pub line: usize,
    /// Old text replaced.
    pub old_text: String,
    /// New text inserted.
    pub new_text: String,
    /// Whether the patch was successfully applied or matched.
    pub applied: bool,
    /// Whether this was a dry-run preview.
    pub dry_run: bool,
    /// Actual line index where replacement occurred.
    pub actual_line: usize,
}

/// Summary report produced by running `tweak dump` or `tweak apply`.
///
/// # Examples
///
/// ```
/// use cargo_martensite::tweak::{TweakAction, TweakReport};
///
/// let report = TweakReport {
///     action: TweakAction::Apply,
///     patches_processed: 2,
///     patches_applied: 2,
///     files_modified: 1,
///     dry_run: false,
///     details: vec!["Applied src/ui.rs:142".to_string()],
/// };
/// assert_eq!(report.patches_applied, 2);
/// ```
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TweakReport {
    /// Action executed (`Dump` or `Apply`).
    pub action: TweakAction,
    /// Total patches parsed or retrieved.
    pub patches_processed: usize,
    /// Patches successfully applied or previewed.
    pub patches_applied: usize,
    /// Number of distinct files modified.
    pub files_modified: usize,
    /// Whether execution was dry-run.
    pub dry_run: bool,
    /// User-facing log and preview details.
    pub details: Vec<String>,
}

/// Errors occurring during `cargo martensite tweak` execution.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TweakError {
    /// File I/O failure.
    Io(String),
    /// Patch syntax parsing failure.
    PatchParse(String),
    /// Failed to apply patch at target location.
    PatchApply {
        /// Target file.
        file: String,
        /// Target line.
        line: usize,
        /// Detailed failure reason.
        reason: String,
    },
    /// Dev channel IPC failure.
    DevChannel(DevChannelError),
    /// No patches found to dump or apply.
    NoPatchesFound(String),
}

impl fmt::Display for TweakError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Io(msg) => write!(f, "I/O error: {msg}"),
            Self::PatchParse(msg) => write!(f, "patch parse error: {msg}"),
            Self::PatchApply { file, line, reason } => {
                write!(f, "failed to apply patch at {file}:{line}: {reason}")
            }
            Self::DevChannel(err) => write!(f, "dev channel error: {err}"),
            Self::NoPatchesFound(msg) => write!(f, "no patches found: {msg}"),
        }
    }
}

impl std::error::Error for TweakError {}

impl From<DevChannelError> for TweakError {
    fn from(err: DevChannelError) -> Self {
        Self::DevChannel(err)
    }
}

/// Parses a single patch hunk line in canonical format:
/// `file:line: .prop(old) -> .prop(new)` or `file:line:col: .prop(old) -> .prop(new)`
///
/// Returns `None` if the line is empty, a comment (`#` or `//`), or unparseable.
///
/// # Examples
///
/// ```
/// use std::path::PathBuf;
/// use cargo_martensite::tweak::parse_patch_line;
///
/// let line = "src/ui.rs:142: .padding(12.0) -> .padding(16.0)";
/// let patch = parse_patch_line(line).expect("valid patch");
/// assert_eq!(patch.file, PathBuf::from("src/ui.rs"));
/// assert_eq!(patch.line, 142);
/// assert_eq!(patch.old_text, ".padding(12.0)");
/// assert_eq!(patch.new_text, ".padding(16.0)");
/// ```
pub fn parse_patch_line(line: &str) -> Option<ParsedPatch> {
    let trimmed = line.trim();
    if trimmed.is_empty() || trimmed.starts_with('#') || trimmed.starts_with("//") {
        return None;
    }

    // Split on arrow " -> "
    let (left, right) = trimmed.split_once(" -> ")?;
    let new_text = right.trim().to_string();

    // In left side: look for `: ` separating location from old_text
    let (loc_part, old_text) = left.split_once(": ")?;
    let old_text = old_text.trim().to_string();

    // Split loc_part into file and line
    // Note: On Windows paths might have drive prefix like `C:\...`, so split from the right
    let parts: Vec<&str> = loc_part.split(':').collect();
    if parts.len() < 2 {
        return None;
    }

    // Handle `file:line:col` or `file:line`
    let (file_str, line_str) = if parts.len() >= 3
        && parts[parts.len() - 1].parse::<usize>().is_ok()
        && parts[parts.len() - 2].parse::<usize>().is_ok()
    {
        // e.g. path:line:col
        let file_part = parts[..parts.len() - 2].join(":");
        let line_part = parts[parts.len() - 2];
        (file_part, line_part)
    } else {
        // e.g. path:line
        let file_part = parts[..parts.len() - 1].join(":");
        let line_part = parts[parts.len() - 1];
        (file_part, line_part)
    };

    let line_num: usize = line_str.parse().ok()?;
    if line_num == 0 {
        return None;
    }

    Some(ParsedPatch {
        file: PathBuf::from(file_str),
        line: line_num,
        old_text,
        new_text,
    })
}

/// Parses multiple patch hunks from a multi-line string.
///
/// # Examples
///
/// ```
/// use cargo_martensite::tweak::parse_patches;
///
/// let input = "src/a.rs:10: .gap(4.0) -> .gap(8.0)\nsrc/b.rs:20: 100 -> 200";
/// let patches = parse_patches(input);
/// assert_eq!(patches.len(), 2);
/// ```
pub fn parse_patches(content: &str) -> Vec<ParsedPatch> {
    content.lines().filter_map(parse_patch_line).collect()
}

/// Applies a list of [`ParsedPatch`] hunks to source files on disk.
///
/// If `dry_run` is true, verifies that all target locations match and generates preview details
/// without modifying any files on disk.
///
/// # Examples
///
/// ```
/// use tempfile::tempdir;
/// use std::fs;
/// use cargo_martensite::tweak::{apply_patches, ParsedPatch};
///
/// let dir = tempdir().unwrap();
/// let file_path = dir.path().join("view.rs");
/// fs::write(&file_path, "let padding = 12.0;\n").unwrap();
///
/// let patch = ParsedPatch::new(&file_path, 1, "12.0", "16.0");
/// let report = apply_patches(&[patch], false).unwrap();
/// assert_eq!(report.patches_applied, 1);
/// assert_eq!(report.files_modified, 1);
/// assert_eq!(fs::read_to_string(&file_path).unwrap(), "let padding = 16.0;\n");
/// ```
pub fn apply_patches(patches: &[ParsedPatch], dry_run: bool) -> Result<TweakReport, TweakError> {
    if patches.is_empty() {
        return Ok(TweakReport {
            action: TweakAction::Apply,
            patches_processed: 0,
            patches_applied: 0,
            files_modified: 0,
            dry_run,
            details: vec!["No patches provided.".to_string()],
        });
    }

    // Group patches by file
    let mut by_file: std::collections::BTreeMap<PathBuf, Vec<ParsedPatch>> =
        std::collections::BTreeMap::new();
    for p in patches {
        by_file.entry(p.file.clone()).or_default().push(p.clone());
    }

    let mut total_applied = 0;
    let mut files_modified = 0;
    let mut details = Vec::new();

    for (file_path, file_patches) in &by_file {
        if !file_path.exists() {
            return Err(TweakError::PatchApply {
                file: file_path.display().to_string(),
                line: file_patches.first().map(|p| p.line).unwrap_or(0),
                reason: format!("file `{}` does not exist", file_path.display()),
            });
        }

        let content = fs::read_to_string(file_path).map_err(|e| {
            TweakError::Io(format!("failed to read `{}`: {e}", file_path.display()))
        })?;

        let mut lines: Vec<String> = content.lines().map(|s| s.to_string()).collect();
        let trailing_newline = content.ends_with('\n');
        let mut file_changed = false;

        for patch in file_patches {
            let target_line = patch.line;
            let mut matched_idx: Option<usize> = None;

            // 1. Check exact line (1-indexed -> 0-indexed)
            if target_line >= 1 && target_line <= lines.len() {
                let idx = target_line - 1;
                if lines[idx].contains(&patch.old_text) {
                    matched_idx = Some(idx);
                }
            }

            // 2. If not found at exact line, search nearby (+/- 10 lines window)
            if matched_idx.is_none() {
                let center = if target_line >= 1 && target_line <= lines.len() {
                    target_line - 1
                } else {
                    0
                };
                let start = center.saturating_sub(10);
                let end = (center + 10).min(lines.len());
                for (offset, line_str) in lines[start..end].iter().enumerate() {
                    if line_str.contains(&patch.old_text) {
                        matched_idx = Some(start + offset);
                        break;
                    }
                }
            }

            // 3. Fallback: full file scan
            if matched_idx.is_none() {
                for (i, line_str) in lines.iter().enumerate() {
                    if line_str.contains(&patch.old_text) {
                        matched_idx = Some(i);
                        break;
                    }
                }
            }

            match matched_idx {
                Some(idx) => {
                    let old_line_str = &lines[idx];
                    let new_line_str = old_line_str.replacen(&patch.old_text, &patch.new_text, 1);
                    let line_no = idx + 1;

                    if dry_run {
                        details.push(format!(
                            "[DRY RUN] {}:{}: `{}` -> `{}`",
                            file_path.display(),
                            line_no,
                            patch.old_text,
                            patch.new_text
                        ));
                    } else {
                        lines[idx] = new_line_str;
                        file_changed = true;
                        details.push(format!(
                            "Applied {}:{}: `{}` -> `{}`",
                            file_path.display(),
                            line_no,
                            patch.old_text,
                            patch.new_text
                        ));
                    }
                    total_applied += 1;
                }
                None => {
                    return Err(TweakError::PatchApply {
                        file: file_path.display().to_string(),
                        line: patch.line,
                        reason: format!(
                            "could not find old value `{}` near line {}",
                            patch.old_text, patch.line
                        ),
                    });
                }
            }
        }

        if file_changed && !dry_run {
            let mut new_content = lines.join("\n");
            if trailing_newline {
                new_content.push('\n');
            }
            fs::write(file_path, new_content).map_err(|e| {
                TweakError::Io(format!("failed to write `{}`: {e}", file_path.display()))
            })?;
            files_modified += 1;
        } else if dry_run {
            files_modified += 1;
        }
    }

    Ok(TweakReport {
        action: TweakAction::Apply,
        patches_processed: patches.len(),
        patches_applied: total_applied,
        files_modified,
        dry_run,
        details,
    })
}

/// Executes the `cargo martensite tweak` command with provided [`TweakOptions`].
///
/// # Examples
///
/// ```
/// use cargo_martensite::tweak::{run_tweak, TweakAction, TweakOptions};
///
/// let opts = TweakOptions {
///     action: TweakAction::Dump,
///     socket: None,
///     allow_version_mismatch: false,
///     file: None,
///     dry_run: false,
/// };
/// // Will attempt connection or report no dev session
/// let _ = run_tweak(&opts);
/// ```
pub fn run_tweak(opts: &TweakOptions) -> Result<TweakReport, TweakError> {
    match opts.action {
        TweakAction::Dump => run_dump(opts),
        TweakAction::Apply => run_apply(opts),
    }
}

/// Executes the `dump` action.
fn run_dump(opts: &TweakOptions) -> Result<TweakReport, TweakError> {
    // 1. If an explicit patch file was provided, dump from it
    if let Some(ref file_path) = opts.file {
        let content = fs::read_to_string(file_path).map_err(|e| {
            TweakError::Io(format!("failed to read `{}`: {e}", file_path.display()))
        })?;
        let patches = parse_patches(&content);
        let mut details = Vec::new();
        details.push(format!("--- Tweaks from file: {} ---", file_path.display()));
        for p in &patches {
            details.push(p.format_canonical());
        }

        println!("{}", details.join("\n"));

        return Ok(TweakReport {
            action: TweakAction::Dump,
            patches_processed: patches.len(),
            patches_applied: 0,
            files_modified: 0,
            dry_run: false,
            details,
        });
    }

    // 2. Query running dev app over dev channel socket
    let sock = discover_socket(opts.socket.as_deref())?;
    let mut client = DevClient::connect(&sock, opts.allow_version_mismatch)?;

    // Call tweak_dump RPC
    let dump_val = client.call("tweak_dump", serde_json::json!({}))?;

    let mut details = Vec::new();
    details.push(format!(
        "Connected to Martensite dev session via {}",
        sock.display()
    ));

    let patches_val = dump_val.get("patches").and_then(|v| v.as_array());
    let mut patch_count = 0;

    if let Some(entries) = dump_val.get("entries").and_then(|v| v.as_array()) {
        details.push(format!("\nActive Live Tweaks ({} total):", entries.len()));
        for entry in entries {
            let name = entry
                .get("name")
                .and_then(|v| v.as_str())
                .unwrap_or("unnamed");
            let cur = entry
                .get("current_value")
                .and_then(|v| v.as_str())
                .unwrap_or("?");
            let def = entry
                .get("default_value")
                .and_then(|v| v.as_str())
                .unwrap_or("?");
            let is_mod = entry
                .get("is_modified")
                .and_then(|v| v.as_bool())
                .unwrap_or(false);
            let badge = if is_mod { " [~]" } else { "" };
            let span = entry
                .get("source_span")
                .and_then(|v| v.as_str())
                .unwrap_or("");
            details.push(format!("  • {name}{badge} = {cur} (default: {def}) {span}"));
        }
    }

    if let Some(patches) = patches_val {
        patch_count = patches.len();
        if !patches.is_empty() {
            details.push("\nEmitted Source Patches:".to_string());
            for p in patches {
                if let Some(s) = p.as_str() {
                    details.push(format!("  {s}"));
                }
            }
        }
    }

    println!("{}", details.join("\n"));

    Ok(TweakReport {
        action: TweakAction::Dump,
        patches_processed: patch_count,
        patches_applied: 0,
        files_modified: 0,
        dry_run: false,
        details,
    })
}

/// Executes the `apply` action.
fn run_apply(opts: &TweakOptions) -> Result<TweakReport, TweakError> {
    let patches = if let Some(ref file_path) = opts.file {
        // Read patches from specified file
        let content = fs::read_to_string(file_path).map_err(|e| {
            TweakError::Io(format!("failed to read `{}`: {e}", file_path.display()))
        })?;
        parse_patches(&content)
    } else {
        // Query running dev app over dev channel
        let sock = discover_socket(opts.socket.as_deref())?;
        let mut client = DevClient::connect(&sock, opts.allow_version_mismatch)?;

        let dump_val = client.call("tweak_dump", serde_json::json!({}))?;
        let mut patch_list = Vec::new();

        if let Some(patches) = dump_val.get("patches").and_then(|v| v.as_array()) {
            for p in patches {
                if let Some(s) = p.as_str() {
                    if let Some(parsed) = parse_patch_line(s) {
                        patch_list.push(parsed);
                    }
                }
            }
        }
        patch_list
    };

    if patches.is_empty() {
        return Err(TweakError::NoPatchesFound(
            "no modified tweaks or patches available to apply (specify --file <path> or tweak values in running app)".to_string(),
        ));
    }

    let report = apply_patches(&patches, opts.dry_run)?;

    println!("{}", report.details.join("\n"));
    if report.dry_run {
        println!(
            "\n[DRY RUN] Would apply {} patch(es) across {} file(s). No changes written.",
            report.patches_applied, report.files_modified
        );
    } else {
        println!(
            "\nSuccessfully applied {} patch(es) across {} file(s).",
            report.patches_applied, report.files_modified
        );
    }

    Ok(report)
}
