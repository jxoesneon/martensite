//! Category E — Design Standards & Lint Enforcement tools (spec §3.8–3.10).

use std::borrow::Cow;
use std::collections::HashMap;

use martensite_design_lint::{
    rule_catalog, AlignEdge, FillStat, Finding, FixOp, FixSafety, LintConfig, LintNode, LintReport,
    LintScene, Severity, Standard,
};
use martensite_devtools::lint_bridge::{LintDump, SerializedLintReport, SerializedLintScene};
use rmcp::handler::server::router::tool::{AsyncTool, ToolBase};
use rmcp::model::ToolAnnotations;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

use crate::error::McpError;
use crate::server::MartensiteMcp;
use crate::types::{LintFindingDescriptor, LintSummaryCounts, Pagination};

/// Parameters for `martensite_lint_scene`.
///
/// # Examples
///
/// ```
/// use martensite_mcp::tools::lint::LintSceneParams;
///
/// let p = LintSceneParams::default();
/// assert_eq!(p.limit(), 50);
/// ```
#[derive(Debug, Clone, Default, Deserialize, JsonSchema)]
#[serde(default)]
pub struct LintSceneParams {
    /// Filter by standard (`wcag`, `isa-101`, `isa-18.2`, `gestalt`, `fitts`,
    /// `tufte`, `all`).
    pub standard: Option<String>,
    /// Minimum severity threshold (`info`, `warn`, `forbid`).
    pub min_severity: Option<String>,
    /// Substring filter on widget `debug_name` lineage.
    pub node_path_filter: Option<String>,
    /// Maximum findings returned (default 50, max 250).
    pub limit: Option<u32>,
    /// Pagination offset (default 0).
    pub offset: Option<u32>,
}

impl LintSceneParams {
    /// Effective result limit, clamped to the spec maximum.
    #[must_use]
    pub fn limit(&self) -> u32 {
        self.limit.unwrap_or(50).min(250)
    }

    /// Effective pagination offset.
    #[must_use]
    pub fn offset(&self) -> u32 {
        self.offset.unwrap_or(0)
    }
}

/// Structured result of `martensite_lint_scene`.
///
/// # Examples
///
/// ```
/// use martensite_mcp::tools::lint::LintSceneOutput;
///
/// let out = LintSceneOutput {
///     findings: Vec::new(),
///     summary: Default::default(),
///     pagination: Default::default(),
///     mode: "offline".to_string(),
/// };
/// assert_eq!(out.mode, "offline");
/// ```
#[derive(Debug, Clone, Serialize, JsonSchema)]
pub struct LintSceneOutput {
    /// Paginated findings surviving the `standard` / `min_severity` /
    /// `node_path_filter` predicates.
    pub findings: Vec<LintFindingDescriptor>,
    /// Scene-wide severity ledger (computed before filtering — spec §3.8:
    /// "counts across the entire scene"). `forbid` aggregates `Error`- and
    /// `Forbid`-severity findings — the build-gating class.
    pub summary: LintSummaryCounts,
    /// Token-safe pagination envelope for the filtered result set.
    pub pagination: Pagination,
    /// Evaluation mode: `live` (dev-channel `lint_pull`) or `offline`
    /// (`--scene` dump evaluated in-process).
    pub mode: String,
}

/// `martensite_lint_scene`: 51-rule design-standards evaluation.
pub struct LintSceneTool;

impl ToolBase for LintSceneTool {
    type Parameter = LintSceneParams;
    type Output = LintSceneOutput;
    type Error = McpError;

    fn name() -> Cow<'static, str> {
        "martensite_lint_scene".into()
    }

    fn description() -> Option<Cow<'static, str>> {
        Some(
            "Evaluate the running frame or offline scene against Martensite's \
             design standards (WCAG 2.2, ISA-101, ISA-18.2, Gestalt, Fitts, \
             Tufte) with token-safe pagination."
                .into(),
        )
    }

    fn annotations() -> Option<ToolAnnotations> {
        crate::tools::read_only_annotations()
    }
}

impl AsyncTool<MartensiteMcp> for LintSceneTool {
    async fn invoke(
        service: &MartensiteMcp,
        param: Self::Parameter,
    ) -> Result<Self::Output, Self::Error> {
        let standard = standard_filter(param.standard.as_deref())?;
        let min_severity = severity_floor(param.min_severity.as_deref())?;

        // Live: pull the app's evaluated scene+report (the app may run a
        // project LintConfig — its report is authoritative). Offline: lint
        // the `--scene` dump in-process with the default configuration.
        let (scene, report, mode) = match service.try_live_call("lint_pull", json!({}))? {
            Some(val) => {
                let (scene, report) = decode_lint_payload(val)?;
                (scene, report, "live")
            }
            None => {
                let scene = service.offline().load_scene()?.ok_or_else(|| {
                    McpError::Ipc(
                        "martensite_lint_scene requires a live dev session or \
                         an offline scene (`--scene <path>`)"
                            .to_string(),
                    )
                })?;
                let report = martensite_design_lint::lint(&scene, &LintConfig::new());
                (scene, report, "offline")
            }
        };

        let catalog = rule_standards();
        let path_filter = param
            .node_path_filter
            .as_deref()
            .map(str::trim)
            .filter(|s| !s.is_empty());

        let filtered: Vec<&Finding> = report
            .findings
            .iter()
            .filter(|f| {
                if f.severity < min_severity {
                    return false;
                }
                if let Some(std) = standard {
                    // Deserialized (live) findings arrive with `standards`
                    // empty — recover the rule's standards from the catalog,
                    // which is the authoritative rule→standard mapping.
                    let standards: &[Standard] = if f.standards.is_empty() {
                        catalog.get(f.rule).copied().unwrap_or(&[])
                    } else {
                        f.standards
                    };
                    if !standards.contains(&std) {
                        return false;
                    }
                }
                if let Some(pf) = path_filter {
                    let lineage_hit = f.path.contains(pf)
                        || scene
                            .node(&f.path)
                            .is_some_and(|n| n.full_name.contains(pf) || n.name.contains(pf));
                    if !lineage_hit {
                        return false;
                    }
                }
                true
            })
            .collect();

        let total = filtered.len();
        let offset = param.offset() as usize;
        let limit = param.limit() as usize;
        let findings = filtered
            .iter()
            .copied()
            .skip(offset)
            .take(limit)
            .map(|f| finding_descriptor(f, &scene))
            .collect();

        Ok(LintSceneOutput {
            findings,
            summary: summarize(&report),
            pagination: Pagination::new(total, limit, offset),
            mode: mode.to_string(),
        })
    }
}

/// Parameters for `martensite_apply_lint_fix`.
///
/// # Examples
///
/// ```
/// use martensite_mcp::tools::lint::ApplyLintFixParams;
///
/// let p = ApplyLintFixParams::default();
/// assert_eq!(p.target(), "scene");
/// ```
#[derive(Debug, Clone, Default, Deserialize, JsonSchema)]
#[serde(default)]
pub struct ApplyLintFixParams {
    /// Finding identifier returned by `martensite_lint_scene`
    /// (a [`Finding::fingerprint`]).
    pub finding_id: String,
    /// Permit applying `Risky` fixes (recoloring, bounds restructuring).
    pub force: Option<bool>,
    /// `"scene"` (in-memory preview, never the live arena) or `"patch"`
    /// (unified git diff for source). Default `"scene"`.
    pub target: Option<String>,
}

impl ApplyLintFixParams {
    /// Effective fix target (`scene` | `patch`).
    #[must_use]
    pub fn target(&self) -> &str {
        self.target.as_deref().unwrap_or("scene")
    }

    /// Whether `Risky` fixes are permitted.
    #[must_use]
    pub fn force(&self) -> bool {
        self.force.unwrap_or(false)
    }
}

/// Structured result of `martensite_apply_lint_fix`.
///
/// # Examples
///
/// ```
/// use martensite_mcp::tools::lint::ApplyLintFixOutput;
///
/// let out = ApplyLintFixOutput {
///     applied: true,
///     target: "scene".to_string(),
///     finding_id: "0123456789abcdef".to_string(),
///     convergence_proof: Some("finding resolved".to_string()),
///     patch: None,
///     remaining_findings: 0,
///     mode: "offline".to_string(),
/// };
/// assert!(out.applied);
/// ```
#[derive(Debug, Clone, Serialize, JsonSchema)]
pub struct ApplyLintFixOutput {
    /// Whether at least one [`FixOp`] changed the preview scene (or the app
    /// confirmed application).
    pub applied: bool,
    /// The effective target (`scene` or `patch`).
    pub target: String,
    /// The finding the fix was applied for.
    pub finding_id: String,
    /// Post-fix convergence proof: the fix summary, whether the finding
    /// still fires, and the remaining active-finding count.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub convergence_proof: Option<String>,
    /// Unified diff text for `target: "patch"`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub patch: Option<String>,
    /// Active findings remaining on the post-fix scene.
    pub remaining_findings: usize,
    /// Execution mode (`live` or `offline`).
    pub mode: String,
}

/// `martensite_apply_lint_fix`: apply a lint fix to the preview scene or
/// produce a git patch. Never mutates the authoritative `WidgetArena`.
pub struct ApplyLintFixTool;

impl ToolBase for ApplyLintFixTool {
    type Parameter = ApplyLintFixParams;
    type Output = ApplyLintFixOutput;
    type Error = McpError;

    fn name() -> Cow<'static, str> {
        "martensite_apply_lint_fix".into()
    }

    fn description() -> Option<Cow<'static, str>> {
        Some(
            "Apply a lint autofix to the in-memory LintScene preview model \
             (convergence proof) or emit a unified git diff patch — never \
             mutates the live WidgetArena (ADR-0038)."
                .into(),
        )
    }

    fn annotations() -> Option<ToolAnnotations> {
        crate::tools::mutation_annotations()
    }
}

impl AsyncTool<MartensiteMcp> for ApplyLintFixTool {
    async fn invoke(
        service: &MartensiteMcp,
        param: Self::Parameter,
    ) -> Result<Self::Output, Self::Error> {
        if param.finding_id.trim().is_empty() {
            return Err(McpError::InvalidParameter(
                "`finding_id` must not be empty".to_string(),
            ));
        }
        let force = param.force();
        let target = param.target();
        if !matches!(target, "scene" | "patch") {
            return Err(McpError::InvalidTarget(format!(
                "unknown fix target `{target}`; expected `scene` or `patch`"
            )));
        }

        // Live path — the dev app applies the ops to a *copy* of its scene
        // and returns the post-fix dump (or patch text). The authoritative
        // WidgetArena is never touched (ADR-0038).
        if let Some(val) = service.try_live_call(
            "lint_apply",
            json!({
                "finding_id": param.finding_id,
                "force": force,
                "target": target,
                "recursive": true,
                "max_recursiveness": 8,
            }),
        )? {
            let (_scene_after, report_after) = decode_lint_payload(val)?;
            if target == "patch" {
                // The app answered with a scene dump rather than diff text —
                // LintScene nodes carry no source spans to anchor a patch.
                return Err(McpError::InvalidTarget(
                    "`patch` requires devtools-source-spans annotations on the \
                     offending nodes; the dev app returned a scene dump without \
                     source spans — apply with `target: \"scene\"` instead"
                        .to_string(),
                ));
            }
            let resolved = !report_after
                .findings
                .iter()
                .any(|f| f.fingerprint() == param.finding_id);
            let remaining = report_after.findings.len();
            let proof = format!(
                "lint_apply on preview scene copy: finding `{}` {} after fix; \
                 {} active finding(s) remain{}",
                param.finding_id,
                if resolved {
                    "resolved"
                } else {
                    "still present"
                },
                remaining,
                if report_after.is_clean() {
                    " — scene is clean"
                } else {
                    ""
                },
            );
            return Ok(ApplyLintFixOutput {
                applied: true,
                target: target.to_string(),
                finding_id: param.finding_id,
                convergence_proof: Some(proof),
                patch: None,
                remaining_findings: remaining,
                mode: "live".to_string(),
            });
        }

        // Offline path — apply the finding's own `FixOp`s to a clone of the
        // `--scene` dump, then re-lint to prove convergence.
        if target == "patch" {
            return Err(McpError::InvalidTarget(
                "`patch` requires source-annotated nodes (devtools-source-spans); \
                 offline LintScene dumps carry no source spans"
                    .to_string(),
            ));
        }
        let scene = service.offline().load_scene()?.ok_or_else(|| {
            McpError::Ipc(
                "martensite_apply_lint_fix requires a live dev session or an \
                 offline scene (`--scene <path>`)"
                    .to_string(),
            )
        })?;
        let config = LintConfig::new();
        let report = martensite_design_lint::lint(&scene, &config);
        let finding = report
            .findings
            .iter()
            .find(|f| f.fingerprint() == param.finding_id)
            .ok_or_else(|| McpError::FindingNotFound(param.finding_id.clone()))?;
        let fix = finding.fix.clone().ok_or_else(|| {
            McpError::LintEngine(format!(
                "finding `{}` ({} @ {}) carries no autofix payload",
                param.finding_id, finding.rule, finding.path
            ))
        })?;
        if fix.safety == FixSafety::Risky && !force {
            return Err(McpError::UnconfirmedMutation(format!(
                "fix for `{}` is risky ({}); pass `force: true` to apply it",
                param.finding_id, fix.summary
            )));
        }

        let mut preview = scene.clone();
        let mut ops_applied = 0usize;
        for op in &fix.ops {
            if apply_fix_op(&mut preview, op) {
                ops_applied += 1;
            }
        }
        let after = martensite_design_lint::lint(&preview, &config);
        let resolved = after
            .findings
            .iter()
            .all(|f| f.fingerprint() != param.finding_id);
        let proof = format!(
            "{} — {} ops applied to preview LintScene; finding {} post-fix; \
             {} active finding(s) remain{}",
            fix.summary,
            ops_applied,
            if resolved {
                "resolved"
            } else {
                "still present"
            },
            after.findings.len(),
            if after.is_clean() {
                " — scene is clean"
            } else {
                ""
            },
        );
        Ok(ApplyLintFixOutput {
            applied: ops_applied > 0,
            target: "scene".to_string(),
            finding_id: param.finding_id,
            convergence_proof: Some(proof),
            patch: None,
            remaining_findings: after.findings.len(),
            mode: "offline".to_string(),
        })
    }
}

/// Parameters for `martensite_audit_paint`.
#[derive(Debug, Clone, Default, Deserialize, JsonSchema)]
#[serde(default)]
pub struct AuditPaintParams {
    /// Subtree `WidgetId` to audit; omitted audits the full scene.
    pub node_id: Option<String>,
}

/// Structured result of `martensite_audit_paint`.
///
/// The three audit facets are forwarded verbatim from the dev app's payload
/// (semantic honesty, D9) — the wire shape of each section is owned by
/// `martensite-devtools`.
///
/// # Examples
///
/// ```
/// use martensite_mcp::tools::lint::AuditPaintOutput;
///
/// let out = AuditPaintOutput {
///     mode: "live".to_string(),
///     node_id: None,
///     text_clearance: serde_json::json!([]),
///     contrast: serde_json::json!([]),
///     keylines: serde_json::json!([]),
///     finding_count: 0,
///     raw: serde_json::json!({}),
/// };
/// assert_eq!(out.finding_count, 0);
/// ```
#[derive(Debug, Clone, Serialize, JsonSchema)]
pub struct AuditPaintOutput {
    /// Execution mode — always `live` (pixel audits need a rendered frame).
    pub mode: String,
    /// Subtree `WidgetId` audited; `None` = full scene.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub node_id: Option<String>,
    /// Text ink descender clearance findings (baseline-to-bottom overlaps),
    /// verbatim from the dev app.
    pub text_clearance: Value,
    /// Measured WCAG contrast findings for visible text against blended
    /// backgrounds, verbatim from the dev app.
    pub contrast: Value,
    /// Non-text keyline 3:1 stroke verification findings, verbatim from the
    /// dev app.
    pub keylines: Value,
    /// Total findings across all audit sections.
    pub finding_count: usize,
    /// Verbatim dev-app response payload.
    pub raw: Value,
}

/// `martensite_audit_paint`: deep visual audit of text, contrast, keylines.
pub struct AuditPaintTool;

impl ToolBase for AuditPaintTool {
    type Parameter = AuditPaintParams;
    type Output = AuditPaintOutput;
    type Error = McpError;

    fn name() -> Cow<'static, str> {
        "martensite_audit_paint".into()
    }

    fn description() -> Option<Cow<'static, str>> {
        Some(
            "Deep visual audit: text ink descender clearance, measured WCAG \
             contrast against blended backgrounds, and non-text keyline 3:1 \
             stroke verification."
                .into(),
        )
    }

    fn annotations() -> Option<ToolAnnotations> {
        crate::tools::read_only_annotations()
    }
}

impl AsyncTool<MartensiteMcp> for AuditPaintTool {
    async fn invoke(
        service: &MartensiteMcp,
        param: Self::Parameter,
    ) -> Result<Self::Output, Self::Error> {
        let mut wire = serde_json::Map::new();
        if let Some(id) = param
            .node_id
            .as_deref()
            .map(str::trim)
            .filter(|s| !s.is_empty())
        {
            wire.insert("node_id".to_string(), json!(id));
        }
        // Pixel audits measure a rendered frame — live-only by definition.
        let val = service.live_call("audit_paint", Value::Object(wire))?;
        let section = |key: &str| -> Value {
            val.get(key)
                .cloned()
                .unwrap_or_else(|| Value::Array(Vec::new()))
        };
        let text_clearance = section("text_clearance");
        let contrast = section("contrast");
        let keylines = section("keylines");
        let count = [&text_clearance, &contrast, &keylines]
            .iter()
            .map(|v| v.as_array().map_or(0, Vec::len))
            .sum();
        Ok(AuditPaintOutput {
            mode: "live".to_string(),
            node_id: param.node_id,
            text_clearance,
            contrast,
            keylines,
            finding_count: count,
            raw: val,
        })
    }
}

// ---------------------------------------------------------------------------
// Shared helpers
// ---------------------------------------------------------------------------

/// Decodes a `lint_pull`/`lint_apply` wire payload — either a full
/// [`LintDump`] or a bare `{scene, report}` pair — into runtime types.
/// Mirrors `decode_lint_result` in `tools/cargo-martensite/src/dev_channel.rs`.
fn decode_lint_payload(val: Value) -> Result<(LintScene, LintReport), McpError> {
    if let Ok(dump) = serde_json::from_value::<LintDump>(val.clone()) {
        return Ok((dump.to_scene(), dump.to_report()));
    }
    if let (Some(scene_val), Some(report_val)) = (val.get("scene"), val.get("report")) {
        let scene: SerializedLintScene = serde_json::from_value(scene_val.clone())?;
        let report: SerializedLintReport = serde_json::from_value(report_val.clone())?;
        return Ok((scene.to_scene(), report.to_report()));
    }
    Err(McpError::Ipc(format!(
        "unexpected lint payload shape from dev app: {val}"
    )))
}

/// Rule id → cited standards, from the engine's own catalog. Deserialized
/// (live-channel) findings arrive with `standards` empty, so filtering
/// consults the authoritative rule metadata instead.
fn rule_standards() -> HashMap<&'static str, &'static [Standard]> {
    rule_catalog()
        .into_iter()
        .map(|(id, _title, _sev, standards)| (id, standards))
        .collect()
}

/// Resolves the `standard` filter parameter into a catalog [`Standard`].
/// `"all"` and `None` disable filtering; the spec's friendly aliases
/// (`gestalt`, `fitts`, `tufte`, ...) map onto their catalog authority.
fn standard_filter(param: Option<&str>) -> Result<Option<Standard>, McpError> {
    let Some(key) = param.map(str::trim).filter(|s| !s.is_empty()) else {
        return Ok(None);
    };
    if key.eq_ignore_ascii_case("all") {
        return Ok(None);
    }
    let parsed = Standard::from_key(key).or_else(|| {
        match key.to_ascii_lowercase().as_str() {
            // Spec §3.8 lists friendly names that are facets of catalog
            // standards rather than standalone entries.
            "gestalt" | "nielsen" => Some(Standard::InfoDesign),
            "fitts" | "hick" | "hick-hyman" | "miller" => Some(Standard::HciLaws),
            _ => None,
        }
    });
    parsed.map(Some).ok_or_else(|| {
        McpError::InvalidParameter(format!(
            "unknown standard `{key}`; expected one of: wcag, isa-101, \
             isa-18.2, hci-laws, info-design, perception, consistency, \
             nureg-0700, faa-hfds (or aliases gestalt, fitts, tufte, all)"
        ))
    })
}

/// Parses the `min_severity` threshold (`info` | `warn` | `error` |
/// `forbid`); `off` is meaningless as a floor and maps to `info`.
fn severity_floor(param: Option<&str>) -> Result<Severity, McpError> {
    let Some(key) = param.map(str::trim).filter(|s| !s.is_empty()) else {
        return Ok(Severity::Info);
    };
    let sev = Severity::from_key(key).ok_or_else(|| {
        McpError::InvalidParameter(format!(
            "unknown min_severity `{key}`; expected info|warn|error|forbid"
        ))
    })?;
    Ok(if sev == Severity::Off {
        Severity::Info
    } else {
        sev
    })
}

/// Maps one engine [`Finding`] to its wire descriptor. `finding_id` is the
/// finding's stable fingerprint — `martensite_apply_lint_fix` resolves it
/// the same way.
fn finding_descriptor(f: &Finding, scene: &LintScene) -> LintFindingDescriptor {
    let node = scene.node(&f.path);
    LintFindingDescriptor {
        finding_id: f.fingerprint(),
        rule_id: f.rule.to_string(),
        standard: f
            .standards
            .first()
            .map(|s| s.config_key().to_string())
            .unwrap_or_default(),
        severity: f.severity.config_key().to_string(),
        node_id: node.and_then(|n| n.widget_id),
        node_path: Some(f.path.clone()),
        message: f.message.clone(),
        citation_url: if f.doc.is_empty() {
            None
        } else {
            Some(f.doc.clone())
        },
        has_autofix: f.fix.is_some(),
    }
}

/// Scene-wide severity ledger — computed on the *unfiltered* report per
/// spec §3.8 ("counts ... across the entire scene").
fn summarize(report: &LintReport) -> LintSummaryCounts {
    let mut counts = LintSummaryCounts {
        active: report.findings.len(),
        suppressed: report.suppressed.len(),
        ..LintSummaryCounts::default()
    };
    for f in &report.findings {
        match f.severity {
            // `forbid` aggregates the build-gating class (Error + Forbid).
            Severity::Error | Severity::Forbid => counts.forbid += 1,
            Severity::Warn => counts.warn += 1,
            Severity::Info => counts.info += 1,
            Severity::Off => {}
        }
    }
    counts
}

// ---------------------------------------------------------------------------
// Offline FixOp application — the preview-model convergence seam
//
// `martensite_design_lint` exposes `autofix` (a whole-report `cargo fix`
// loop) but its single-op application entry point is crate-private. The
// port below applies *one* finding's [`FixOp`]s to a cloned preview
// `LintScene` so `martensite_apply_lint_fix` reports per-finding
// convergence without touching unrelated findings — and never touches the
// authoritative `WidgetArena` (ADR-0038). Semantics mirror `fix.rs`:
// spacing fixes only spread, alignment works inside band clusters, and
// recolors keep shared colors honest.
// ---------------------------------------------------------------------------

/// Dominant layout axis of a sibling set.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Axis {
    /// Horizontal spread.
    X,
    /// Vertical spread.
    Y,
}

/// The (start, end) extent of `n` along `axis`.
fn axis_pos(n: &LintNode, axis: Axis) -> (f64, f64) {
    match axis {
        Axis::X => (n.bounds.x0, n.bounds.x1),
        Axis::Y => (n.bounds.y0, n.bounds.y1),
    }
}

/// Every node at `path` — same-name siblings share a path, so a fix
/// anchored by path may legitimately hit several nodes (the engine's
/// `nodes_mut` semantics: condition-gated ops only mutate actual
/// offenders).
fn scene_nodes_mut<'a>(scene: &'a mut LintScene, path: &str) -> Vec<&'a mut LintNode> {
    fn find<'a>(n: &'a mut LintNode, path: &str, out: &mut Vec<&'a mut LintNode>) {
        if n.path == path {
            out.push(n);
            return;
        }
        for c in &mut n.children {
            find(c, path, out);
        }
    }
    let mut out = Vec::new();
    for r in &mut scene.roots {
        find(r, path, &mut out);
    }
    out
}

/// Shifts `n` and its entire subtree by `d` device px along `axis` —
/// bounds, fill rects and their clips, and text origins move together
/// (equivalent to `LintNode::translate` for a single-axis delta).
fn shift_tree(n: &mut LintNode, axis: Axis, d: f64) {
    match axis {
        Axis::X => {
            n.bounds.x0 += d;
            n.bounds.x1 += d;
            for f in &mut n.fills {
                f.rect.x0 += d;
                f.rect.x1 += d;
                if let Some(c) = &mut f.clip {
                    c.x0 += d;
                    c.x1 += d;
                }
            }
            for t in &mut n.texts {
                t.origin.x += d;
            }
        }
        Axis::Y => {
            n.bounds.y0 += d;
            n.bounds.y1 += d;
            for f in &mut n.fills {
                f.rect.y0 += d;
                f.rect.y1 += d;
                if let Some(c) = &mut f.clip {
                    c.y0 += d;
                    c.y1 += d;
                }
            }
            for t in &mut n.texts {
                t.origin.y += d;
            }
        }
    }
    for c in &mut n.children {
        shift_tree(c, axis, d);
    }
}

/// Whether two siblings share the perpendicular band — sitting on the
/// same row (X-axis layout) or column (Y-axis layout), judged by >50%
/// overlap of the shorter extent.
fn band_adjacent(a: &LintNode, b: &LintNode, axis: Axis) -> bool {
    let (a0, a1, b0, b1) = match axis {
        Axis::X => (a.bounds.y0, a.bounds.y1, b.bounds.y0, b.bounds.y1),
        Axis::Y => (a.bounds.x0, a.bounds.x1, b.bounds.x0, b.bounds.x1),
    };
    let overlap = a1.min(b1) - a0.max(b0);
    overlap > 0.5 * (a1 - a0).min(b1 - b0).max(1.0)
}

/// Child indices sorted by dominant-axis position; `axis_out` receives
/// whichever axis the children spread along.
fn sorted_children(node: &LintNode, axis_out: &mut Axis) -> Vec<usize> {
    let xs: Vec<f64> = node.children.iter().map(|c| c.bounds.x0).collect();
    let ys: Vec<f64> = node.children.iter().map(|c| c.bounds.y0).collect();
    let spread = |v: &[f64]| {
        v.iter().copied().fold(f64::NEG_INFINITY, f64::max)
            - v.iter().copied().fold(f64::INFINITY, f64::min)
    };
    *axis_out = if spread(&xs) >= spread(&ys) {
        Axis::X
    } else {
        Axis::Y
    };
    let axis = *axis_out;
    let mut idx: Vec<usize> = (0..node.children.len()).collect();
    idx.sort_by(|&a, &b| {
        axis_pos(&node.children[a], axis)
            .0
            .partial_cmp(&axis_pos(&node.children[b], axis).0)
            .unwrap_or(std::cmp::Ordering::Equal)
    });
    idx
}

/// Transitive band clusters along `axis` — children whose
/// perpendicular-band overlaps chain together form one group.
fn band_clusters(node: &LintNode, axis: Axis) -> Vec<Vec<usize>> {
    let mut idx: Vec<usize> = (0..node.children.len()).collect();
    idx.sort_by(|&a, &b| {
        axis_pos(&node.children[a], axis)
            .0
            .partial_cmp(&axis_pos(&node.children[b], axis).0)
            .unwrap_or(std::cmp::Ordering::Equal)
    });
    let mut clusters: Vec<Vec<usize>> = Vec::new();
    let mut cur_end = f64::NEG_INFINITY;
    for i in idx {
        let (s, e) = axis_pos(&node.children[i], axis);
        match clusters.last_mut() {
            Some(cluster) if s <= cur_end + 0.5 => {
                cluster.push(i);
                cur_end = cur_end.max(e);
            }
            _ => {
                clusters.push(vec![i]);
                cur_end = e;
            }
        }
    }
    clusters
}

/// Enforces a minimum inter-sibling gap — children spaced wider than
/// `gap` keep their positions (fixes spread siblings apart, never pull
/// them together). Only band-adjacent pairs are constrained.
fn set_gap(node: &mut LintNode, gap: f64) -> bool {
    let mut axis = Axis::X;
    let kids = sorted_children(node, &mut axis);
    if kids.len() < 2 {
        return false;
    }
    let mut changed = false;
    let mut cursor: Option<(usize, f64)> = None;
    for &idx in &kids {
        let (pos, _) = axis_pos(&node.children[idx], axis);
        if let Some((prev_idx, prev_end)) = cursor {
            if band_adjacent(&node.children[prev_idx], &node.children[idx], axis) {
                let want = prev_end + gap;
                if pos < want - 0.01 {
                    shift_tree(&mut node.children[idx], axis, want - pos);
                    changed = true;
                }
            }
        }
        let (_, end) = axis_pos(&node.children[idx], axis);
        cursor = Some((idx, end));
    }
    changed
}

/// Snaps each band-adjacent sibling gap to the nearest multiple of
/// `grid` (minimum `grid` — never collapses to zero).
fn snap_gaps(node: &mut LintNode, grid: f64) -> bool {
    if grid <= 0.0 || !grid.is_finite() {
        return false;
    }
    let mut axis = Axis::X;
    let kids = sorted_children(node, &mut axis);
    if kids.len() < 2 {
        return false;
    }
    let mut changed = false;
    let mut cursor: Option<(usize, f64)> = None;
    for &idx in &kids {
        let (pos, _) = axis_pos(&node.children[idx], axis);
        if let Some((prev_idx, prev_end)) = cursor {
            if band_adjacent(&node.children[prev_idx], &node.children[idx], axis) {
                let cur = pos - prev_end;
                let snapped = (cur / grid).round().max(1.0) * grid;
                if (snapped - cur).abs() > 0.01 {
                    shift_tree(&mut node.children[idx], axis, prev_end + snapped - pos);
                    changed = true;
                }
            }
        }
        let (_, end) = axis_pos(&node.children[idx], axis);
        cursor = Some((idx, end));
    }
    changed
}

/// Aligns `node`'s children to the given `edge` — within perpendicular
/// band clusters only, so a grid's columns/rows never collapse onto one
/// global extreme.
fn align_siblings(node: &mut LintNode, edge: AlignEdge) -> bool {
    if node.children.len() < 2 {
        return false;
    }
    let axis = match edge {
        AlignEdge::Left | AlignEdge::Right => Axis::X,
        AlignEdge::Top | AlignEdge::Bottom => Axis::Y,
    };
    let edge_pos = |n: &LintNode| match edge {
        AlignEdge::Left => n.bounds.x0,
        AlignEdge::Top => n.bounds.y0,
        AlignEdge::Right => n.bounds.x1,
        AlignEdge::Bottom => n.bounds.y1,
    };
    let take_min = matches!(edge, AlignEdge::Left | AlignEdge::Top);
    let mut changed = false;
    for cluster in band_clusters(node, axis) {
        if cluster.len() < 2 {
            continue;
        }
        let target = cluster
            .iter()
            .map(|&i| edge_pos(&node.children[i]))
            .reduce(|a, b| if take_min { a.min(b) } else { a.max(b) })
            .unwrap_or(0.0);
        for &i in &cluster {
            let delta = target - edge_pos(&node.children[i]);
            if delta.abs() > 0.01 {
                shift_tree(&mut node.children[i], axis, delta);
                changed = true;
            }
        }
    }
    changed
}

/// Keeps `node.colors` honest after a recolor — `from` survives only
/// while some fill or text still paints it.
fn sync_colors(node: &mut LintNode, from: &[u8; 4], to: &[u8; 4]) {
    let still_used =
        node.fills.iter().any(|f| f.color == *from) || node.texts.iter().any(|t| t.color == *from);
    if !still_used {
        node.colors.retain(|c| c != from);
    }
    if !node.colors.contains(to) {
        node.colors.push(*to);
    }
}

/// Recolors fills or texts at `path` whose color equals `from` — shared
/// driver for [`FixOp::RecolorFill`] / [`FixOp::RecolorText`].
fn recolor(scene: &mut LintScene, path: &str, from: &[u8; 4], to: &[u8; 4], text: bool) -> bool {
    let mut changed = false;
    for node in scene_nodes_mut(scene, path) {
        let mut hit = false;
        if text {
            for t in &mut node.texts {
                if t.color == *from {
                    t.color = *to;
                    hit = true;
                }
            }
        } else {
            for f in &mut node.fills {
                if f.color == *from {
                    f.color = *to;
                    hit = true;
                }
            }
        }
        if hit {
            changed = true;
            sync_colors(node, from, to);
        }
    }
    changed
}

/// Applies one [`FixOp`] to the preview scene — `true` when it changed
/// something (idempotent ops report `false`, which is how convergence is
/// judged).
fn apply_fix_op(scene: &mut LintScene, op: &FixOp) -> bool {
    match op {
        FixOp::SetGap { parent, gap } => {
            let mut changed = false;
            for node in scene_nodes_mut(scene, parent) {
                changed |= set_gap(node, *gap);
            }
            changed
        }
        FixOp::SnapGapsToGrid { parent, grid } => {
            let mut changed = false;
            for node in scene_nodes_mut(scene, parent) {
                changed |= snap_gaps(node, *grid);
            }
            changed
        }
        FixOp::AlignSiblings { parent, edge } => {
            let mut changed = false;
            for node in scene_nodes_mut(scene, parent) {
                changed |= align_siblings(node, *edge);
            }
            changed
        }
        FixOp::RecolorFill { path, from, to } => recolor(scene, path, from, to, false),
        FixOp::RecolorText { path, from, to } => recolor(scene, path, from, to, true),
        FixOp::SetFontSize { path, from, to } => {
            let mut changed = false;
            for node in scene_nodes_mut(scene, path) {
                let mut hit = false;
                for t in &mut node.texts {
                    if (t.size - from).abs() < 0.5 {
                        t.size = *to;
                        hit = true;
                        changed = true;
                    }
                }
                if hit {
                    node.font_sizes.retain(|s| (*s - from).abs() >= 0.5);
                    if !node.font_sizes.iter().any(|s| (*s - to).abs() < 0.5) {
                        node.font_sizes.push(*to);
                    }
                }
            }
            changed
        }
        FixOp::GrowBounds { path, min_w, min_h } => {
            let mut changed = false;
            for node in scene_nodes_mut(scene, path) {
                let mut r = node.bounds;
                let nx1 = r.x1.max(r.x0 + min_w);
                let ny1 = r.y1.max(r.y0 + min_h);
                if nx1 > r.x1 || ny1 > r.y1 {
                    r.x1 = nx1;
                    r.y1 = ny1;
                    node.bounds = r;
                    // Fills clipped against the old bounds may now
                    // legitimately cover more — refresh painted area so
                    // coverage rules see the grown surface.
                    let bounds = node.bounds;
                    let painted = node
                        .fills
                        .iter()
                        .map(|f: &FillStat| {
                            let v = f.rect.intersect(bounds);
                            v.width().max(0.0) * v.height().max(0.0)
                        })
                        .sum::<f64>();
                    node.painted_area = painted.min(node.area());
                    changed = true;
                }
            }
            changed
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use martensite_design_lint::NodeKind;

    /// A bare lint node with the given path and bounds (avoiding a direct
    /// `kurbo` dependency — bounds start as `Default` and get their fields
    /// assigned individually).
    fn leaf(path: &str, x0: f64, y0: f64, x1: f64, y1: f64) -> LintNode {
        let mut n = LintNode {
            name: path.rsplit('/').next().unwrap_or(path).to_string(),
            full_name: path.rsplit('/').next().unwrap_or(path).to_string(),
            path: path.to_string(),
            bounds: Default::default(),
            widget_id: None,
            kind: NodeKind::Container,
            allows: Vec::new(),
            own_allows: Vec::new(),
            display_level: None,
            markers: Vec::new(),
            font_sizes: Vec::new(),
            colors: Vec::new(),
            texts: Vec::new(),
            fills: Vec::new(),
            painted_area: 0.0,
            children: Vec::new(),
        };
        n.bounds.x0 = x0;
        n.bounds.y0 = y0;
        n.bounds.x1 = x1;
        n.bounds.y1 = y1;
        n
    }

    /// A scene with `width`-sized children `gap` apart under `App/Panel`.
    fn cramped_scene(gap: f64, count: usize) -> LintScene {
        let mut panel = leaf("App/Panel", 0.0, 0.0, 800.0, 100.0);
        for i in 0..count {
            let x = i as f64 * (50.0 + gap);
            let mut child = leaf("App/Panel/Label", x, 10.0, x + 50.0, 40.0);
            child.name = "Label".to_string();
            panel.children.push(child);
        }
        let mut root = leaf("App", 0.0, 0.0, 800.0, 600.0);
        root.children.push(panel);
        LintScene {
            roots: vec![root],
            frame: None,
            scale_factor: 1.0,
        }
    }

    #[test]
    fn set_gap_spreads_cramped_siblings() {
        let mut scene = cramped_scene(2.0, 4);
        let changed = apply_fix_op(
            &mut scene,
            &FixOp::SetGap {
                parent: "App/Panel".into(),
                gap: 8.0,
            },
        );
        assert!(changed, "cramped siblings should spread");
        let panel = scene.node("App/Panel").expect("panel exists");
        for w in panel.children.windows(2) {
            let gap = w[1].bounds.x0 - w[0].bounds.x1;
            assert!(gap >= 7.9, "gap {gap} below the 8px floor");
        }
    }

    #[test]
    fn set_gap_is_idempotent() {
        let mut scene = cramped_scene(8.0, 3);
        let changed = apply_fix_op(
            &mut scene,
            &FixOp::SetGap {
                parent: "App/Panel".into(),
                gap: 8.0,
            },
        );
        assert!(!changed, "already-compliant siblings must not move");
    }

    #[test]
    fn grow_bounds_expands_minimum() {
        let mut scene = cramped_scene(4.0, 2);
        let changed = apply_fix_op(
            &mut scene,
            &FixOp::GrowBounds {
                path: "App/Panel/Label".into(),
                min_w: 60.0,
                min_h: 50.0,
            },
        );
        assert!(changed);
        let node = scene.node("App/Panel/Label").expect("label exists");
        assert!(node.bounds.width() >= 60.0);
        assert!(node.bounds.height() >= 50.0);
    }

    #[test]
    fn missing_path_is_noop() {
        let mut scene = cramped_scene(2.0, 2);
        let changed = apply_fix_op(
            &mut scene,
            &FixOp::SetGap {
                parent: "App/Nowhere".into(),
                gap: 8.0,
            },
        );
        assert!(!changed);
    }

    #[test]
    fn severity_floor_parsing() {
        assert_eq!(severity_floor(Some("warn")).expect("warn"), Severity::Warn);
        assert_eq!(
            severity_floor(Some("forbid")).expect("forbid"),
            Severity::Forbid
        );
        assert_eq!(severity_floor(Some("off")).expect("off"), Severity::Info);
        assert_eq!(severity_floor(None).expect("none"), Severity::Info);
        assert!(severity_floor(Some("bogus")).is_err());
    }

    #[test]
    fn standard_aliases() {
        assert_eq!(
            standard_filter(Some("gestalt")).expect("gestalt alias"),
            Some(Standard::InfoDesign)
        );
        assert_eq!(
            standard_filter(Some("fitts")).expect("fitts alias"),
            Some(Standard::HciLaws)
        );
        assert_eq!(
            standard_filter(Some("wcag")).expect("wcag"),
            Some(Standard::Wcag)
        );
        assert_eq!(standard_filter(Some("all")).expect("all"), None);
        assert_eq!(standard_filter(None).expect("none"), None);
        assert!(standard_filter(Some("bogus-standard")).is_err());
    }
}
