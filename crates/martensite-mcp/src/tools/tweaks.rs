//! Category F — Live Tweaks & Source Synchronization tools (spec §3.11–3.14).

use std::borrow::Cow;
use std::path::Path;
use std::time::Instant;

use rmcp::handler::server::router::tool::{AsyncTool, ToolBase};
use rmcp::model::ToolAnnotations;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use serde_json::{json, Map, Value};

use crate::error::McpError;
use crate::server::MartensiteMcp;
use crate::types::{AuditRecord, TweakDescriptor};

/// Parameters for `martensite_list_tweaks` (no parameters).
#[derive(Debug, Clone, Default, Deserialize, JsonSchema)]
#[serde(default)]
pub struct ListTweaksParams {}

/// Structured result of `martensite_list_tweaks`.
///
/// # Examples
///
/// ```
/// use martensite_mcp::tools::tweaks::ListTweaksOutput;
///
/// let out = ListTweaksOutput {
///     tweaks: Vec::new(),
///     mode: "live".to_string(),
/// };
/// assert!(out.tweaks.is_empty());
/// ```
#[derive(Debug, Clone, Serialize, JsonSchema)]
pub struct ListTweaksOutput {
    /// Registered tweakable parameters with values, defaults, source
    /// spans, and optimistic revision tokens.
    pub tweaks: Vec<TweakDescriptor>,
    /// Execution mode — always `live` (tweak state exists only in the
    /// running app).
    pub mode: String,
}

/// `martensite_list_tweaks`: enumerate registered `#[tweak]`/`TweakRegistry`
/// parameters with values, defaults, source spans, and revision tokens.
pub struct ListTweaksTool;

impl ToolBase for ListTweaksTool {
    type Parameter = ListTweaksParams;
    type Output = ListTweaksOutput;
    type Error = McpError;

    fn name() -> Cow<'static, str> {
        "martensite_list_tweaks".into()
    }

    fn description() -> Option<Cow<'static, str>> {
        Some(
            "List all active runtime tweakable parameters with current and \
             default values, source file/line, and optimistic revision tokens."
                .into(),
        )
    }

    fn annotations() -> Option<ToolAnnotations> {
        crate::tools::read_only_annotations()
    }
}

impl AsyncTool<MartensiteMcp> for ListTweaksTool {
    async fn invoke(
        service: &MartensiteMcp,
        _param: Self::Parameter,
    ) -> Result<Self::Output, Self::Error> {
        let val = service.live_call("tweaks_list", json!({}))?;
        // Accept both a bare array and a `{tweaks: [...]}` envelope.
        let payload = val.get("tweaks").cloned().unwrap_or(val);
        let tweaks: Vec<TweakDescriptor> = serde_json::from_value(payload)
            .map_err(|e| McpError::Ipc(format!("failed to decode `tweaks_list` payload: {e}")))?;
        Ok(ListTweaksOutput {
            tweaks,
            mode: "live".to_string(),
        })
    }
}

/// Parameters for `martensite_set_tweak`.
#[derive(Debug, Clone, Default, Deserialize, JsonSchema)]
#[serde(default)]
pub struct SetTweakParams {
    /// Tweak identifier.
    pub name: String,
    /// New value to assign (JSON scalar or string).
    pub value: serde_json::Value,
}

/// Structured result of `martensite_set_tweak`.
///
/// # Examples
///
/// ```
/// use martensite_mcp::tools::tweaks::SetTweakOutput;
///
/// let out = SetTweakOutput {
///     name: "ui/padding".to_string(),
///     applied: true,
///     latency_us: 420,
///     latency_ms: 0.42,
/// };
/// assert!(out.latency_ms < 1.0);
/// ```
#[derive(Debug, Clone, Serialize, JsonSchema)]
pub struct SetTweakOutput {
    /// Tweak identifier that was updated.
    pub name: String,
    /// Whether the running app confirmed the value application.
    pub applied: bool,
    /// Dev-channel round-trip latency in microseconds.
    #[schemars(schema_with = "crate::types::schema_strip::u64s")]
    pub latency_us: u64,
    /// Dev-channel round-trip latency in milliseconds (target < 1.0ms).
    pub latency_ms: f64,
}

/// `martensite_set_tweak`: sub-millisecond live parameter update.
pub struct SetTweakTool;

impl ToolBase for SetTweakTool {
    type Parameter = SetTweakParams;
    type Output = SetTweakOutput;
    type Error = McpError;

    fn name() -> Cow<'static, str> {
        "martensite_set_tweak".into()
    }

    fn description() -> Option<Cow<'static, str>> {
        Some(
            "Update a tweak parameter in the running application in real time \
             (target latency < 1.0ms); reports live application confirmation."
                .into(),
        )
    }

    fn annotations() -> Option<ToolAnnotations> {
        crate::tools::mutation_annotations()
    }
}

impl AsyncTool<MartensiteMcp> for SetTweakTool {
    async fn invoke(
        service: &MartensiteMcp,
        param: Self::Parameter,
    ) -> Result<Self::Output, Self::Error> {
        if param.name.trim().is_empty() {
            return Err(McpError::InvalidParameter(
                "`name` must not be empty".to_string(),
            ));
        }
        let started = Instant::now();
        let res = service.live_call(
            "tweak_set",
            json!({ "name": &param.name, "value": &param.value }),
        )?;
        let elapsed = started.elapsed();
        // The app may flag a rejected write explicitly; a clean response
        // with no `applied` field counts as applied.
        let applied = res.get("applied").and_then(Value::as_bool).unwrap_or(true);
        Ok(SetTweakOutput {
            name: param.name,
            applied,
            latency_us: elapsed.as_micros() as u64,
            latency_ms: elapsed.as_secs_f64() * 1000.0,
        })
    }
}

/// Parameters for `martensite_set_theme`.
#[derive(Debug, Clone, Default, Deserialize, JsonSchema)]
#[serde(default)]
pub struct SetThemeParams {
    /// `"light"`, `"dark"`, or `"system"`.
    pub mode: Option<String>,
    /// Token name → Oklab/hex color overrides.
    pub token_overrides: Option<serde_json::Map<String, serde_json::Value>>,
}

/// Structured result of `martensite_set_theme`.
///
/// # Examples
///
/// ```
/// use martensite_mcp::tools::tweaks::SetThemeOutput;
///
/// let out = SetThemeOutput {
///     mode: Some("dark".to_string()),
///     active_tokens: vec!["surface".to_string()],
///     repaint_queued: true,
/// };
/// assert!(out.repaint_queued);
/// ```
#[derive(Debug, Clone, Serialize, JsonSchema)]
pub struct SetThemeOutput {
    /// Effective theme mode after the call, when reported by the app.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub mode: Option<String>,
    /// Active design-token names after applying overrides.
    pub active_tokens: Vec<String>,
    /// Whether the app queued a repaint for the new theme state.
    pub repaint_queued: bool,
}

/// `martensite_set_theme`: dark/light toggle + design-token palette overrides.
pub struct SetThemeTool;

impl ToolBase for SetThemeTool {
    type Parameter = SetThemeParams;
    type Output = SetThemeOutput;
    type Error = McpError;

    fn name() -> Cow<'static, str> {
        "martensite_set_theme".into()
    }

    fn description() -> Option<Cow<'static, str>> {
        Some(
            "Toggle dark/light/system theme mode or apply dynamic design-token \
             palette overrides; returns active tokens and repaint confirmation."
                .into(),
        )
    }

    fn annotations() -> Option<ToolAnnotations> {
        crate::tools::mutation_annotations()
    }
}

impl AsyncTool<MartensiteMcp> for SetThemeTool {
    async fn invoke(
        service: &MartensiteMcp,
        param: Self::Parameter,
    ) -> Result<Self::Output, Self::Error> {
        if let Some(mode) = param
            .mode
            .as_deref()
            .map(str::trim)
            .filter(|s| !s.is_empty())
        {
            if !matches!(mode, "light" | "dark" | "system") {
                return Err(McpError::InvalidParameter(format!(
                    "unknown theme mode `{mode}`; expected light|dark|system"
                )));
            }
        }
        let mut wire = Map::new();
        if let Some(mode) = param
            .mode
            .as_deref()
            .map(str::trim)
            .filter(|s| !s.is_empty())
        {
            wire.insert("mode".to_string(), json!(mode));
        }
        if let Some(overrides) = &param.token_overrides {
            wire.insert(
                "token_overrides".to_string(),
                Value::Object(overrides.clone()),
            );
        }
        let res = service.live_call("theme_set", Value::Object(wire))?;

        let mode = res
            .get("mode")
            .and_then(Value::as_str)
            .map(str::to_string)
            .or(param.mode.clone());
        // `active_tokens` arrives either as a token→value map or as a
        // flat list of names; normalize to the name list.
        let active_tokens: Vec<String> = match res.get("active_tokens") {
            Some(Value::Object(map)) => map.keys().cloned().collect(),
            Some(Value::Array(items)) => items
                .iter()
                .filter_map(|v| v.as_str().map(str::to_string))
                .collect(),
            _ => Vec::new(),
        };
        let repaint_queued = res
            .get("repaint_queued")
            .and_then(Value::as_bool)
            .unwrap_or(true);
        Ok(SetThemeOutput {
            mode,
            active_tokens,
            repaint_queued,
        })
    }
}

/// Parameters for `martensite_sync_tweaks_to_source`.
///
/// Guarded disk mutation: `dry_run` defaults to `true`; disk writes require
/// `dry_run: false` AND `confirmed: true`, are workspace-confined, and emit
/// audit records (spec §6.4).
#[derive(Debug, Clone, Default, Deserialize, JsonSchema)]
#[serde(default)]
pub struct SyncTweaksParams {
    /// Tweaks to commit; omitted targets all modified tweaks.
    pub tweak_names: Option<Vec<String>>,
    /// Preview-only mode. Defaults to `true`; disk writes require `false`.
    pub dry_run: Option<bool>,
    /// Mandatory confirmation gate: writes proceed only when `true`.
    pub confirmed: Option<bool>,
    /// Optimistic concurrency token preventing multi-agent races.
    pub expected_revision: Option<String>,
}

impl SyncTweaksParams {
    /// Effective dry-run flag (preview-by-default per spec §3.14).
    #[must_use]
    pub fn dry_run(&self) -> bool {
        self.dry_run.unwrap_or(true)
    }

    /// Whether the caller explicitly confirmed the disk write.
    #[must_use]
    pub fn confirmed(&self) -> bool {
        self.confirmed.unwrap_or(false)
    }
}

/// One source file the dev app reports writing (or previewing) for a
/// tweaks-to-source sync.
#[derive(Debug)]
struct WrittenFile {
    /// Workspace-relative file path.
    path: String,
    /// First modified line (1-based); `0` when the app did not report a span.
    span_start: u32,
    /// Last modified line (1-based, inclusive).
    span_end: u32,
    /// Unified diff applied to the file.
    diff: String,
}

/// Structured result of `martensite_sync_tweaks_to_source`.
///
/// # Examples
///
/// ```
/// use martensite_mcp::tools::tweaks::SyncTweaksOutput;
///
/// let out = SyncTweaksOutput {
///     dry_run: true,
///     applied: false,
///     diff: "src/ui.rs:142: .padding(12.0) -> .padding(16.0)".to_string(),
///     files: vec!["src/ui.rs".to_string()],
///     audited: false,
/// };
/// assert!(out.dry_run && !out.applied);
/// ```
#[derive(Debug, Clone, Serialize, JsonSchema)]
pub struct SyncTweaksOutput {
    /// Whether this call ran in preview mode (no disk writes).
    pub dry_run: bool,
    /// Whether the dev app committed values to source files.
    pub applied: bool,
    /// Unified diff / patch text of the changes (or previewed changes).
    pub diff: String,
    /// Workspace-relative file paths touched (or that would be touched).
    pub files: Vec<String>,
    /// Whether every written file produced an audit-log record.
    pub audited: bool,
}

/// `martensite_sync_tweaks_to_source`: commit converged tweak values to Rust
/// source under confirmation, confinement, and audit gates.
pub struct SyncTweaksTool;

impl ToolBase for SyncTweaksTool {
    type Parameter = SyncTweaksParams;
    type Output = SyncTweaksOutput;
    type Error = McpError;

    fn name() -> Cow<'static, str> {
        "martensite_sync_tweaks_to_source".into()
    }

    fn description() -> Option<Cow<'static, str>> {
        Some(
            "Safely commit converged live tweak values back to Rust source \
             files. Preview-only by default (dry_run), requires confirmed=true \
             for disk writes, workspace-src confined, audit-logged, with \
             optimistic revision locking."
                .into(),
        )
    }

    fn annotations() -> Option<ToolAnnotations> {
        crate::tools::mutation_annotations()
    }
}

impl AsyncTool<MartensiteMcp> for SyncTweaksTool {
    async fn invoke(
        service: &MartensiteMcp,
        param: Self::Parameter,
    ) -> Result<Self::Output, Self::Error> {
        let dry_run = param.dry_run();
        let confirmed = param.confirmed();

        // Spec §6.4 guarded-mutation gate — enforced server-side before any
        // transport, in every mode.
        if !dry_run && !confirmed {
            return Err(McpError::UnconfirmedMutation(
                "martensite_sync_tweaks_to_source writes to disk only with \
                 `dry_run: false` AND `confirmed: true`"
                    .to_string(),
            ));
        }

        // The wire token is numeric (u64); a non-numeric caller string is a
        // parameter error, not a protocol failure.
        let expected_revision_num = match &param.expected_revision {
            Some(s) => Some(s.trim().parse::<u64>().map_err(|_| {
                McpError::InvalidParameter(format!(
                    "`expected_revision` `{s}` is not a non-negative integer"
                ))
            })?),
            None => None,
        };

        let mut wire = Map::new();
        if let Some(names) = &param.tweak_names {
            wire.insert("names".to_string(), json!(names));
        }
        wire.insert("dry_run".to_string(), Value::Bool(dry_run));
        wire.insert("confirmed".to_string(), Value::Bool(confirmed));
        if let Some(rev) = expected_revision_num {
            wire.insert("expected_revision".to_string(), json!(rev));
        }

        // Confirmed write: preview first. The preflight `tweaks_sync`
        // (dry_run) reports the exact `files` the app would splice; every
        // path is confined to the workspace BEFORE any write request is
        // sent, and the write is pinned to the preflight's `revision` so a
        // concurrent edit cannot slip between preview and commit.
        let mut expected_revision = param.expected_revision.clone();
        if !dry_run {
            let mut preflight = wire.clone();
            preflight.insert("dry_run".to_string(), Value::Bool(true));
            preflight.insert("confirmed".to_string(), Value::Bool(false));
            let preview = service.live_call("tweaks_sync", Value::Object(preflight))?;
            check_revision_conflict(&preview, param.expected_revision.as_deref())?;
            for f in parse_written_files(&preview) {
                service.offline().confine_to_workspace(Path::new(&f.path))?;
            }
            let Some(rev) = preview.get("revision").and_then(Value::as_u64) else {
                return Err(McpError::Ipc(
                    "`tweaks_sync` preflight response lacks a numeric `revision`; \
                     refusing to send an unpinned write"
                        .to_string(),
                ));
            };
            wire.insert("expected_revision".to_string(), json!(rev));
            expected_revision = Some(rev.to_string());
        }

        // Offline: `LintScene` dumps carry no tweak registry state, so there
        // is nothing to patch — the live session is required.
        let res = service.live_call("tweaks_sync", Value::Object(wire))?;

        // Optimistic-concurrency gate: surface an app-reported revision
        // mismatch as a structured conflict rather than a generic IPC error.
        check_revision_conflict(&res, expected_revision.as_deref())?;

        let files = parse_written_files(&res);
        let applied = res
            .get("applied")
            .and_then(Value::as_bool)
            .unwrap_or(!dry_run);
        let diff = res
            .get("diff")
            .and_then(Value::as_str)
            .map(str::to_string)
            .unwrap_or_else(|| {
                files
                    .iter()
                    .map(|f| f.diff.as_str())
                    .filter(|d| !d.is_empty())
                    .collect::<Vec<_>>()
                    .join("\n")
            });

        // Spec §6.4: every committed write emits an audit record.
        let mut audited = false;
        if applied && !dry_run && !files.is_empty() {
            for f in &files {
                let mut record = AuditRecord::new(
                    "sync_tweaks_to_source",
                    f.path.clone(),
                    f.span_start,
                    f.span_end,
                );
                record.diff = f.diff.clone();
                record.revision_token = expected_revision.clone();
                service.offline().write_audit(&record)?;
            }
            audited = true;
        }

        Ok(SyncTweaksOutput {
            dry_run,
            applied,
            diff,
            files: files.iter().map(|f| f.path.clone()).collect(),
            audited,
        })
    }
}

/// Maps an app-reported `revision_conflict` onto the structured
/// [`McpError::RevisionConflict`] error.
fn check_revision_conflict(res: &Value, expected: Option<&str>) -> Result<(), McpError> {
    if res
        .get("revision_conflict")
        .and_then(Value::as_bool)
        .unwrap_or(false)
    {
        let actual = res
            .get("actual_revision")
            .and_then(Value::as_str)
            .unwrap_or("unknown")
            .to_string();
        return Err(McpError::RevisionConflict {
            expected: expected.unwrap_or("unspecified").to_string(),
            actual,
        });
    }
    Ok(())
}

/// Normalizes the `files` array of a `tweaks_sync` response — entries are
/// `{path, span_start, span_end, diff}` objects.
fn parse_written_files(res: &Value) -> Vec<WrittenFile> {
    let Some(arr) = res.get("files").and_then(Value::as_array) else {
        return Vec::new();
    };
    arr.iter()
        .filter_map(|f| {
            let map = f.as_object()?;
            Some(WrittenFile {
                path: map.get("path")?.as_str()?.to_string(),
                span_start: map.get("span_start").and_then(Value::as_u64).unwrap_or(0) as u32,
                span_end: map.get("span_end").and_then(Value::as_u64).unwrap_or(0) as u32,
                diff: map
                    .get("diff")
                    .and_then(Value::as_str)
                    .unwrap_or_default()
                    .to_string(),
            })
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sync_params_gate_defaults() {
        let p = SyncTweaksParams::default();
        assert!(p.dry_run(), "dry_run defaults to preview-only");
        assert!(!p.confirmed());
        let p = SyncTweaksParams {
            dry_run: Some(false),
            confirmed: Some(true),
            ..SyncTweaksParams::default()
        };
        assert!(!p.dry_run() && p.confirmed());
    }

    #[test]
    fn parse_files_accepts_canonical_objects() {
        let res = json!({
            "files": [
                {"path": "src/ui.rs", "span_start": 10, "span_end": 14, "diff": "@@ -10 +10 @@"},
                {"path": "src/app.rs", "span_start": 42, "span_end": 42}
            ]
        });
        let files = parse_written_files(&res);
        assert_eq!(files.len(), 2);
        assert_eq!(files[0].path, "src/ui.rs");
        assert_eq!(files[0].span_start, 10);
        assert_eq!(files[1].path, "src/app.rs");
        assert_eq!(files[1].span_end, 42);
    }

    #[test]
    fn parse_files_empty_when_missing() {
        assert!(parse_written_files(&json!({})).is_empty());
        assert!(parse_written_files(&json!({"files": null})).is_empty());
    }
}
