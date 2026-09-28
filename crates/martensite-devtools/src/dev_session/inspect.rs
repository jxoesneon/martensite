//! Inspection-domain handlers: widget tree, layout chain, overflow,
//! accessibility tree, paint audit, node capture, inspector select.
//!
//! These handlers are thin envelopes over [`ArenaProbe`](super::ArenaProbe):
//! the app supplies live truth; this module validates params and shapes the
//! wire payload. `lint_pull`/`lint_apply` live here too since they read the
//! session's [`LintBridge`](crate::lint_bridge::LintBridge) rather than the
//! probe.

use serde_json::{json, Value};

use martensite_design_lint::{AlignEdge, FixOp, FixOptions, FixSafety};

use crate::lint_bridge::{
    LintDump, LintNode, LintReport, LintScene, SerializedLintReport, SerializedLintScene,
};

use super::{DevSession, SessionResult};

impl DevSession {
    /// `tree_snapshot` — full/subtree snapshot via the app probe.
    ///
    /// Params (`root_id`, `max_depth`, ...) pass through verbatim — the
    /// app owns the query vocabulary.
    pub fn tree_snapshot(&self, params: &Value) -> SessionResult {
        self.probe
            .lock()
            .expect("probe mutex")
            .probe_tree_snapshot(params)
    }

    /// `tree_node` — detailed single-node inspection. `node_id`
    /// (u64 or numeric string) is required.
    pub fn tree_node(&self, params: &Value) -> SessionResult {
        let node_id = required_node_id(params)?;
        self.probe
            .lock()
            .expect("probe mutex")
            .probe_node_detail(node_id)
    }

    /// `layout_chain` — Taffy constraint chain for a node.
    /// `node_id` (u64 or numeric string) is required.
    pub fn layout_chain(&self, params: &Value) -> SessionResult {
        let node_id = required_node_id(params)?;
        self.probe
            .lock()
            .expect("probe mutex")
            .probe_layout_chain(node_id)
    }

    /// `overflow_scan` — protrusion/clipping diagnostics. `node_id`
    /// scopes the scan to a subtree; omitted scans the whole tree.
    pub fn overflow_scan(&self, params: &Value) -> SessionResult {
        let node_id = optional_node_id(params)?;
        self.probe
            .lock()
            .expect("probe mutex")
            .probe_overflow_scan(node_id)
    }

    /// `a11y_tree` — AccessKit semantic tree. `root_id`,
    /// `role_filter`, and `include_ignored` pass straight through.
    pub fn a11y_tree(&self, params: &Value) -> SessionResult {
        self.probe
            .lock()
            .expect("probe mutex")
            .probe_a11y_tree(params)
    }

    /// `audit_paint` — paint-order audit of the current frame;
    /// `node_id` optionally scopes the audit to a subtree.
    pub fn audit_paint(&self, params: &Value) -> SessionResult {
        let node_id = optional_node_id(params)?;
        self.probe
            .lock()
            .expect("probe mutex")
            .probe_audit_paint(node_id)
    }

    /// `capture_node` — rasterize a node subtree to an image payload.
    ///
    /// Validates `format` (`"png"` default, or `"jpeg"`/`"jpg"`) and
    /// `scale` (positive finite f64, default `1.0`) before hitting the
    /// probe; the probe result passes through verbatim.
    pub fn capture_node(&self, params: &Value) -> SessionResult {
        let node_id = required_node_id(params)?;
        let format = match params.get("format") {
            None | Some(Value::Null) => "png",
            Some(v) => {
                let s = v.as_str().ok_or_else(|| {
                    "invalid param: format must be \"png\" or \"jpeg\"".to_string()
                })?;
                if s.eq_ignore_ascii_case("png") {
                    "png"
                } else if s.eq_ignore_ascii_case("jpeg") || s.eq_ignore_ascii_case("jpg") {
                    "jpeg"
                } else {
                    return Err(format!(
                        "invalid param: unsupported capture format `{s}` \
                         (expected \"png\" or \"jpeg\")"
                    ));
                }
            }
        };
        let scale = match params.get("scale") {
            None | Some(Value::Null) => 1.0,
            Some(v) => v
                .as_f64()
                .ok_or_else(|| "invalid param: scale must be a number".to_string())?,
        };
        if !scale.is_finite() || scale <= 0.0 {
            return Err("invalid param: scale must be a positive finite number".to_string());
        }
        self.probe
            .lock()
            .expect("probe mutex")
            .probe_capture_node(node_id, format, scale)
    }

    /// `inspector_select` — arm select mode and await a user pick.
    ///
    /// `arm` (default `true`) toggles select mode; `wait` (default
    /// `true`) blocks until the user clicks. Extra params pass through.
    pub fn inspector_select(&self, params: &Value) -> SessionResult {
        let arm = bool_param(params, "arm", true)?;
        let wait = bool_param(params, "wait", true)?;
        let mut wire = params.as_object().cloned().unwrap_or_default();
        wire.insert("arm".to_string(), json!(arm));
        wire.insert("wait".to_string(), json!(wait));
        self.probe
            .lock()
            .expect("probe mutex")
            .probe_inspector_select(&Value::Object(wire))
    }

    /// `node_set_loading` — force or clear the node's
    /// `NodeFlags::LOADING` override (ADR-0040 phase 3).
    ///
    /// `node_id` (u64 or numeric string; `node` is an accepted alias)
    /// and `loading` (bool) are required. The mutation runs through
    /// [`ArenaProbe::probe_node_set_loading`](super::ArenaProbe::probe_node_set_loading)
    /// against the real arena; the response carries `applied`, the
    /// canonical `node_id`, the *effective* `loading` state (a widget's
    /// own `is_loading` declaration keeps it `true` even after the
    /// override is cleared), and a bumped mutation `revision`.
    ///
    /// ```
    /// use martensite_devtools::dev_session::{DevSession, NullProbe};
    /// use serde_json::json;
    ///
    /// let session = DevSession::new(Box::new(NullProbe));
    /// // No app probe backing → an honest capability gap.
    /// let err = session
    ///     .node_set_loading(&json!({"node_id": 7, "loading": true}))
    ///     .unwrap_err();
    /// assert_eq!(err, "not_implemented:node_set_loading");
    /// ```
    pub fn node_set_loading(&self, params: &Value) -> SessionResult {
        let node_id = match params.get("node_id").or_else(|| params.get("node")) {
            None | Some(Value::Null) => {
                return Err("invalid param: node_id required".to_string());
            }
            Some(v) => parse_node_id(v)?,
        };
        let loading = match params.get("loading") {
            None | Some(Value::Null) => {
                return Err("invalid param: loading must be a boolean".to_string());
            }
            Some(v) => v
                .as_bool()
                .ok_or_else(|| "invalid param: loading must be a boolean".to_string())?,
        };
        let mut out = self
            .probe
            .lock()
            .expect("probe mutex")
            .probe_node_set_loading(node_id, loading)?;
        if let Some(obj) = out.as_object_mut() {
            obj.insert("revision".to_string(), json!(self.bump_revision()));
        }
        Ok(out)
    }

    /// `lint_pull` — serialized scene + report from the lint bridge,
    /// emitted as a full [`LintDump`] JSON document.
    ///
    /// Errors with `"no lint frame captured yet"` until the app feeds a
    /// frame through [`DevSession::on_frame`].
    pub fn lint_pull(&self, params: &Value) -> SessionResult {
        let _ = params;
        let bridge = self.lint.lock().expect("lint mutex");
        let (Some(scene), Some(report)) = (bridge.scene(), bridge.report()) else {
            return Err("no lint frame captured yet".to_string());
        };
        serde_json::to_value(LintDump::new(scene, report))
            .map_err(|e| format!("lint_pull serialization failed: {e}"))
    }

    /// `lint_apply` — evaluate fix ops against a **copy** of the last
    /// lint scene; the live arena and the bridge's cached frame are
    /// never mutated (ADR-0038 read-only-evaluation invariant).
    ///
    /// Params:
    /// - `ops` — explicit [`FixOp`]s as tagged objects
    ///   (`{"op": "set_gap", "parent": ..., "gap": ...}`, ...), or
    ///   finding fingerprints / `{"finding_id": ...}` entries resolved
    ///   against the cached report.
    /// - `finding_id` — shorthand for `ops: ["<fingerprint>"]`.
    /// - `force` (default `false`) — permit `Risky` ops.
    /// - `recursive` (default `true`) — re-apply until an idempotent
    ///   fixpoint; `false` caps at a single pass.
    /// - `max_recursiveness` (default `8`) — pass ceiling.
    ///
    /// With no `ops`/`finding_id`, the whole cached report is run
    /// through [`martensite_design_lint::autofix`] — the
    /// `cargo martensite lint --fix` attach path.
    ///
    /// Returns `{converged, applied_ops_count, skipped_risky_ops,
    /// remaining_findings, scene, report}` — `scene`/`report` are the
    /// [`SerializedLintScene`]/[`SerializedLintReport`] pair both wire
    /// decoders (cargo-martensite, martensite-mcp) accept.
    pub fn lint_apply(&self, params: &Value) -> SessionResult {
        let force = params
            .get("force")
            .and_then(Value::as_bool)
            .unwrap_or(false);
        let recursive = params
            .get("recursive")
            .and_then(Value::as_bool)
            .unwrap_or(true);
        let max_depth = params
            .get("max_recursiveness")
            .and_then(Value::as_u64)
            .and_then(|v| usize::try_from(v).ok())
            .unwrap_or(8);

        let bridge = self.lint.lock().expect("lint mutex");
        let Some(scene) = bridge.scene() else {
            return Err("no lint frame captured yet".to_string());
        };
        let mut preview = scene.clone();

        let (report, converged, applied, skipped_risky) =
            match resolve_fix_ops(params, bridge.report(), force)? {
                Some(ops) => {
                    let (applied, skipped, converged) =
                        apply_explicit_ops(&mut preview, &ops, force, recursive, max_depth);
                    (
                        martensite_design_lint::lint(&preview, bridge.config()),
                        converged,
                        applied,
                        skipped,
                    )
                }
                None => {
                    let opts = FixOptions {
                        force,
                        recursive,
                        max_depth,
                    };
                    let fix = martensite_design_lint::autofix(&mut preview, bridge.config(), &opts);
                    let applied = fix.applied_count();
                    let skipped = fix.gated_count();
                    let converged = fix.converged;
                    (fix.report, converged, applied, skipped)
                }
            };

        Ok(json!({
            "converged": converged,
            "applied_ops_count": applied,
            "skipped_risky_ops": skipped_risky,
            "remaining_findings": report.findings.len(),
            "scene": SerializedLintScene::from(&preview),
            "report": SerializedLintReport::from(&report),
        }))
    }
}

// ---------------------------------------------------------------------------
// Param parsing
// ---------------------------------------------------------------------------

/// Parses a `node_id` value — a u64 or a decimal string (`"42"`).
fn parse_node_id(value: &Value) -> Result<u64, String> {
    match value {
        Value::Number(n) => n
            .as_u64()
            .ok_or_else(|| "invalid param: node_id must be a non-negative integer".to_string()),
        Value::String(s) => s
            .trim()
            .parse::<u64>()
            .map_err(|_| format!("invalid param: node_id `{s}` is not a u64")),
        _ => Err("invalid param: node_id must be a u64 or numeric string".to_string()),
    }
}

/// Required `node_id` param — missing or null errors out.
fn required_node_id(params: &Value) -> Result<u64, String> {
    match params.get("node_id") {
        None | Some(Value::Null) => Err("invalid param: node_id required".to_string()),
        Some(v) => parse_node_id(v),
    }
}

/// Optional `node_id` param — missing or null becomes `None`.
fn optional_node_id(params: &Value) -> Result<Option<u64>, String> {
    match params.get("node_id") {
        None | Some(Value::Null) => Ok(None),
        Some(v) => parse_node_id(v).map(Some),
    }
}

/// Optional bool param — `default` when missing or null, error when
/// present with a non-boolean value.
fn bool_param(params: &Value, key: &str, default: bool) -> Result<bool, String> {
    match params.get(key) {
        None | Some(Value::Null) => Ok(default),
        Some(v) => v
            .as_bool()
            .ok_or_else(|| format!("invalid param: {key} must be a boolean")),
    }
}

// ---------------------------------------------------------------------------
// lint_apply — op resolution
// ---------------------------------------------------------------------------

/// Resolves the caller's explicit op selection into `(FixOp, FixSafety)`
/// pairs, or `None` when the request carries no selection (the whole-
/// report [`martensite_design_lint::autofix`] path).
///
/// Accepted shapes: top-level `finding_id`, `ops` entries that are
/// finding-fingerprint strings, `{"finding_id": ...}` objects, or
/// tagged serialized [`FixOp`]s. [`FixSafety`] is declared by the
/// finding's fix for resolved ops, and derived from the op kind for
/// serialized ops (spacing/alignment = `Safe`; color/font/geometry =
/// `Risky`) — matching the `LintFix` declarations in
/// `martensite_design_lint::rules`.
fn resolve_fix_ops(
    params: &Value,
    report: Option<&LintReport>,
    force: bool,
) -> Result<Option<Vec<(FixOp, FixSafety)>>, String> {
    let mut ops: Vec<(FixOp, FixSafety)> = Vec::new();
    let mut explicit = false;

    if let Some(id) = params.get("finding_id").and_then(Value::as_str) {
        explicit = true;
        ops.extend(finding_fix_ops(id, report, force)?);
    }

    if let Some(ops_val) = params.get("ops").filter(|v| !v.is_null()) {
        let arr = ops_val
            .as_array()
            .ok_or_else(|| "invalid param: ops must be an array".to_string())?;
        explicit = true;
        for (i, item) in arr.iter().enumerate() {
            match item {
                Value::String(id) => ops.extend(
                    finding_fix_ops(id, report, force)
                        .map_err(|e| format!("invalid param: ops[{i}]: {e}"))?,
                ),
                Value::Object(_) => {
                    if let Some(id) = item.get("finding_id").and_then(Value::as_str) {
                        ops.extend(
                            finding_fix_ops(id, report, force)
                                .map_err(|e| format!("invalid param: ops[{i}]: {e}"))?,
                        );
                    } else {
                        let op = parse_fix_op(item)
                            .map_err(|e| format!("invalid param: ops[{i}]: {e}"))?;
                        let safety = op_safety(&op);
                        ops.push((op, safety));
                    }
                }
                _ => {
                    return Err(format!(
                        "invalid param: ops[{i}] must be a fix-op object or finding id"
                    ))
                }
            }
        }
    }

    Ok(explicit.then_some(ops))
}

/// Resolves a `finding_id` (a [`Finding::fingerprint`](martensite_design_lint::Finding)
/// emitted by `lint_pull`) to the ops of the fix the cached report
/// attached to that finding.
fn finding_fix_ops(
    id: &str,
    report: Option<&LintReport>,
    force: bool,
) -> Result<Vec<(FixOp, FixSafety)>, String> {
    let Some(report) = report else {
        return Err("no lint frame captured yet".to_string());
    };
    let finding = report
        .findings
        .iter()
        .find(|f| f.fingerprint() == id)
        .ok_or_else(|| {
            format!("unknown finding_id `{id}` — pull a fresh lint scene for current fingerprints")
        })?;
    let Some(fix) = &finding.fix else {
        return Err(format!(
            "finding `{id}` ({} @ {}) carries no autofix",
            finding.rule, finding.path
        ));
    };
    if fix.safety == FixSafety::Risky && !force {
        return Err(format!(
            "fix for `{id}` is risky ({}); pass `force: true` to apply it",
            fix.summary
        ));
    }
    Ok(fix.ops.iter().cloned().map(|op| (op, fix.safety)).collect())
}

/// The [`FixSafety`] an op kind carries — mirrors the `LintFix`
/// declarations in `martensite_design_lint::rules` (spacing/alignment
/// nudges are `Safe`; recolors, font changes, and geometry growth are
/// `Risky`).
fn op_safety(op: &FixOp) -> FixSafety {
    match op {
        FixOp::SetGap { .. } | FixOp::SnapGapsToGrid { .. } | FixOp::AlignSiblings { .. } => {
            FixSafety::Safe
        }
        FixOp::RecolorFill { .. }
        | FixOp::RecolorText { .. }
        | FixOp::SetFontSize { .. }
        | FixOp::GrowBounds { .. } => FixSafety::Risky,
    }
}

/// Decodes a serialized [`FixOp`] — fixes are *data* (`fix.rs`), so the
/// wire form mirrors the enum: `{"op": "set_gap", "parent": "...", "gap": 8.0}`.
/// Tags accept the variant name or snake_case (`"SetGap"`/`"set_gap"`).
fn parse_fix_op(v: &Value) -> Result<FixOp, String> {
    let tag = v
        .get("op")
        .or_else(|| v.get("type"))
        .or_else(|| v.get("kind"))
        .and_then(Value::as_str)
        .ok_or_else(|| "missing `op` tag".to_string())?;
    let norm = tag.to_ascii_lowercase().replace(['_', '-'], "");

    let str_field = |key: &str| -> Result<String, String> {
        v.get(key)
            .and_then(Value::as_str)
            .map(str::to_string)
            .ok_or_else(|| format!("`{key}` must be a string"))
    };
    let num_field = |keys: &[&str]| -> Result<f64, String> {
        keys.iter()
            .find_map(|k| v.get(*k).and_then(Value::as_f64))
            .ok_or_else(|| format!("expected one of {keys:?} to be a number"))
    };
    let rgba_field = |key: &str| -> Result<[u8; 4], String> {
        let arr = v
            .get(key)
            .and_then(Value::as_array)
            .ok_or_else(|| format!("`{key}` must be an [r, g, b, a] array"))?;
        if arr.len() != 4 {
            return Err(format!("`{key}` must have exactly 4 channels"));
        }
        let mut out = [0u8; 4];
        for (i, ch) in arr.iter().enumerate() {
            let n = ch
                .as_u64()
                .ok_or_else(|| format!("`{key}[{i}]` must be a channel value 0-255"))?;
            out[i] = u8::try_from(n).map_err(|_| format!("`{key}[{i}]` out of range (0-255)"))?;
        }
        Ok(out)
    };

    match norm.as_str() {
        "setgap" => Ok(FixOp::SetGap {
            parent: str_field("parent")?,
            gap: num_field(&["gap", "amount", "value"])?,
        }),
        "snapgapstogrid" => Ok(FixOp::SnapGapsToGrid {
            parent: str_field("parent")?,
            grid: num_field(&["grid", "step", "value"])?,
        }),
        "alignsiblings" => Ok(FixOp::AlignSiblings {
            parent: str_field("parent")?,
            edge: parse_align_edge(v.get("edge"))?,
        }),
        "recolorfill" => Ok(FixOp::RecolorFill {
            path: str_field("path")?,
            from: rgba_field("from")?,
            to: rgba_field("to")?,
        }),
        "recolortext" => Ok(FixOp::RecolorText {
            path: str_field("path")?,
            from: rgba_field("from")?,
            to: rgba_field("to")?,
        }),
        "setfontsize" => Ok(FixOp::SetFontSize {
            path: str_field("path")?,
            from: num_field(&["from", "from_size"])? as f32,
            to: num_field(&["to", "to_size"])? as f32,
        }),
        "growbounds" => Ok(FixOp::GrowBounds {
            path: str_field("path")?,
            min_w: num_field(&["min_w", "minW", "min_width", "w"])?,
            min_h: num_field(&["min_h", "minH", "min_height", "h"])?,
        }),
        other => Err(format!(
            "unknown fix op `{other}` — expected set_gap, snap_gaps_to_grid, \
             align_siblings, recolor_fill, recolor_text, set_font_size, or grow_bounds"
        )),
    }
}

/// Parses an [`AlignEdge`] wire value (`"left"|"top"|"right"|"bottom"`).
fn parse_align_edge(v: Option<&Value>) -> Result<AlignEdge, String> {
    let Some(v) = v else {
        return Err("`edge` is required (left|top|right|bottom)".to_string());
    };
    let s = v
        .as_str()
        .ok_or_else(|| "`edge` must be a string".to_string())?;
    match s.to_ascii_lowercase().as_str() {
        "left" => Ok(AlignEdge::Left),
        "top" => Ok(AlignEdge::Top),
        "right" => Ok(AlignEdge::Right),
        "bottom" => Ok(AlignEdge::Bottom),
        other => Err(format!(
            "unknown edge `{other}` — expected left|top|right|bottom"
        )),
    }
}

/// Applies explicit `ops` to `preview` until a pass changes nothing
/// (idempotent fixpoint — all engine ops are idempotent) or `max_depth`
/// passes elapse. `recursive: false` caps at one pass. `Risky` ops are
/// skipped unless `force`, matching `autofix`'s `--force` gate.
///
/// `converged` mirrors [`FixReport::converged`](martensite_design_lint::FixReport):
/// `true` when a pass applied nothing; `false` means the run stopped
/// with eligible ops still applicable (single-pass or depth-limited).
/// Returns `(applied, skipped_risky, converged)`.
fn apply_explicit_ops(
    preview: &mut LintScene,
    ops: &[(FixOp, FixSafety)],
    force: bool,
    recursive: bool,
    max_depth: usize,
) -> (usize, usize, bool) {
    let max_passes = if recursive { max_depth.max(1) } else { 1 };
    let mut applied = 0usize;
    let mut skipped_risky = 0usize;
    let mut converged = false;
    for _pass in 0..max_passes {
        let mut changed = 0usize;
        skipped_risky = 0;
        for (op, safety) in ops {
            if *safety == FixSafety::Risky && !force {
                skipped_risky += 1;
                continue;
            }
            if apply_fix_op(preview, op) {
                changed += 1;
            }
        }
        applied += changed;
        if changed == 0 {
            converged = true;
            break;
        }
    }
    (applied, skipped_risky, converged)
}

// ---------------------------------------------------------------------------
// FixOp application on the preview copy
//
// `martensite_design_lint` exposes `autofix` (a whole-report `cargo fix`
// loop) but its single-op entry point is crate-private, so explicit
// `ops`/`finding_id` requests apply through this port — same semantics
// as `fix.rs`: spacing fixes only spread, alignment works inside band
// clusters, and recolors keep shared colors honest. (The identical port
// lives in `martensite-mcp::tools::lint` for its offline preview path.)
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
/// via [`LintNode::translate`].
fn shift_tree(n: &mut LintNode, axis: Axis, d: f64) {
    n.translate(match axis {
        Axis::X => kurbo::Vec2::new(d, 0.0),
        Axis::Y => kurbo::Vec2::new(0.0, d),
    });
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
/// something (idempotent ops report `false`, which is how convergence
/// is judged).
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
                    // legitimately cover more — refresh the stat so
                    // coverage rules see the grown surface.
                    let bounds = node.bounds;
                    let painted = node
                        .fills
                        .iter()
                        .map(|f| {
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
    use kurbo::Rect;
    use martensite_core::PaintList;
    use serde_json::json;

    use super::super::{ArenaProbe, NullProbe};
    use super::*;

    fn session() -> DevSession {
        DevSession::new(Box::new(NullProbe))
    }

    /// Probe that echoes the invoked method plus its normalized args.
    struct EchoProbe;

    impl ArenaProbe for EchoProbe {
        fn probe_tree_snapshot(&mut self, params: &Value) -> SessionResult {
            Ok(json!({"m": "tree_snapshot", "p": params.clone()}))
        }
        fn probe_node_detail(&mut self, node_id: u64) -> SessionResult {
            Ok(json!({"m": "tree_node", "id": node_id}))
        }
        fn probe_layout_chain(&mut self, node_id: u64) -> SessionResult {
            Ok(json!({"m": "layout_chain", "id": node_id}))
        }
        fn probe_overflow_scan(&mut self, node_id: Option<u64>) -> SessionResult {
            Ok(json!({"m": "overflow_scan", "id": node_id}))
        }
        fn probe_a11y_tree(&mut self, params: &Value) -> SessionResult {
            Ok(json!({"m": "a11y_tree", "p": params.clone()}))
        }
        fn probe_audit_paint(&mut self, node_id: Option<u64>) -> SessionResult {
            Ok(json!({"m": "audit_paint", "id": node_id}))
        }
        fn probe_capture_node(&mut self, node_id: u64, format: &str, scale: f64) -> SessionResult {
            Ok(json!({"m": "capture_node", "id": node_id, "f": format, "s": scale}))
        }
        fn probe_inspector_select(&mut self, params: &Value) -> SessionResult {
            Ok(json!({"m": "inspector_select", "p": params.clone()}))
        }
        fn probe_node_set_loading(&mut self, node_id: u64, loading: bool) -> SessionResult {
            Ok(json!({"m": "node_set_loading", "id": node_id, "loading": loading}))
        }
    }

    /// A paint list whose four `App/Panel/Label` siblings sit 2px apart —
    /// `whitespace` flags it and attaches a Safe `SetGap` autofix.
    fn cramped_list() -> PaintList {
        let mut list = PaintList::new();
        list.push_scope(None, "App", Rect::new(0.0, 0.0, 800.0, 600.0));
        list.push_scope(None, "Panel", Rect::new(0.0, 0.0, 800.0, 100.0));
        for i in 0..4 {
            let x = i as f64 * 52.0;
            list.push_scope(None, "Label", Rect::new(x, 10.0, x + 50.0, 40.0));
            list.pop_scope();
        }
        list.pop_scope();
        list.pop_scope();
        list
    }

    #[test]
    fn tree_node_requires_node_id() {
        let s = session();
        let err = s.tree_node(&json!({})).expect_err("missing node_id");
        assert!(err.starts_with("invalid param: node_id"), "{err}");
        assert!(s.tree_node(&json!({"node_id": -1})).is_err());
        assert!(s.tree_node(&json!({"node_id": 4.5})).is_err());
        assert!(s.tree_node(&json!({"node_id": "abc"})).is_err());
        assert!(s.layout_chain(&json!({})).is_err());
        assert!(s.capture_node(&json!({})).is_err());
    }

    #[test]
    fn node_id_accepts_u64_or_numeric_string() {
        let s = DevSession::new(Box::new(EchoProbe));
        assert_eq!(s.tree_node(&json!({"node_id": 7})).unwrap()["id"], 7);
        assert_eq!(s.tree_node(&json!({"node_id": "9"})).unwrap()["id"], 9);
        assert_eq!(s.layout_chain(&json!({"node_id": "5"})).unwrap()["id"], 5);
    }

    #[test]
    fn probe_methods_delegate_and_normalize() {
        let s = DevSession::new(Box::new(EchoProbe));

        // tree_snapshot passes params through verbatim.
        let v = s.tree_snapshot(&json!({"max_depth": 3})).unwrap();
        assert_eq!(v["m"], "tree_snapshot");
        assert_eq!(v["p"]["max_depth"], 3);

        // Optional node scoping.
        assert!(s.overflow_scan(&json!({})).unwrap()["id"].is_null());
        assert_eq!(s.overflow_scan(&json!({"node_id": 3})).unwrap()["id"], 3);
        assert_eq!(s.audit_paint(&json!({"node_id": "8"})).unwrap()["id"], 8);

        // a11y params pass straight through.
        let v = s
            .a11y_tree(&json!({"role_filter": "button", "include_ignored": true}))
            .unwrap();
        assert_eq!(v["p"]["role_filter"], "button");
        assert_eq!(v["p"]["include_ignored"], true);

        // inspector_select defaults arm/wait to true, honors explicit values.
        let v = s.inspector_select(&json!({})).unwrap();
        assert_eq!(v["p"], json!({"arm": true, "wait": true}));
        let v = s
            .inspector_select(&json!({"arm": false, "wait": false}))
            .unwrap();
        assert_eq!(v["p"], json!({"arm": false, "wait": false}));
        assert!(s.inspector_select(&json!({"arm": 1})).is_err());
    }

    #[test]
    fn node_set_loading_validates_params_and_stamps_revision() {
        let s = DevSession::new(Box::new(EchoProbe));
        assert!(s.node_set_loading(&json!({})).is_err());
        assert!(s.node_set_loading(&json!({"loading": true})).is_err());
        assert!(s.node_set_loading(&json!({"node_id": 3})).is_err());
        assert!(s
            .node_set_loading(&json!({"node_id": 3, "loading": "yes"}))
            .is_err());

        let v = s
            .node_set_loading(&json!({"node_id": 7, "loading": true}))
            .expect("set loading");
        assert_eq!(v["m"], "node_set_loading");
        assert_eq!(v["id"], 7);
        assert_eq!(v["loading"], true);
        assert_eq!(v["revision"], 1);

        // The `node` alias and numeric-string ids resolve identically.
        let v = s
            .node_set_loading(&json!({"node": "9", "loading": false}))
            .expect("alias");
        assert_eq!(v["id"], 9);
        assert_eq!(v["loading"], false);
        assert_eq!(v["revision"], 2);
    }

    #[test]
    fn node_set_loading_propagates_probe_errors() {
        let s = session();
        let err = s
            .node_set_loading(&json!({"node_id": 4, "loading": true}))
            .expect_err("NullProbe is not implemented");
        assert_eq!(err, "not_implemented:node_set_loading");
    }

    #[test]
    fn capture_node_validates_format_and_scale() {
        let s = DevSession::new(Box::new(EchoProbe));
        let v = s.capture_node(&json!({"node_id": 4})).unwrap();
        assert_eq!(v["f"], "png");
        assert_eq!(v["s"], 1.0);

        let v = s
            .capture_node(&json!({"node_id": 4, "format": "jpeg", "scale": 2.0}))
            .unwrap();
        assert_eq!(v["f"], "jpeg");
        assert_eq!(v["s"], 2.0);

        assert!(s
            .capture_node(&json!({"node_id": 4, "format": "webp"}))
            .is_err());
        assert!(s.capture_node(&json!({"node_id": 4, "format": 5})).is_err());
        assert!(s
            .capture_node(&json!({"node_id": 4, "scale": 0.0}))
            .is_err());
        assert!(s
            .capture_node(&json!({"node_id": 4, "scale": -1.0}))
            .is_err());
        assert!(s
            .capture_node(&json!({"node_id": 4, "scale": "big"}))
            .is_err());
    }

    #[test]
    fn probe_errors_propagate_unchanged() {
        let s = session();
        for res in [
            s.tree_snapshot(&json!({})),
            s.tree_node(&json!({"node_id": 1})),
            s.layout_chain(&json!({"node_id": 1})),
            s.overflow_scan(&json!({})),
            s.a11y_tree(&json!({})),
            s.audit_paint(&json!({})),
            s.capture_node(&json!({"node_id": 1})),
            s.inspector_select(&json!({})),
        ] {
            let err = res.expect_err("NullProbe is not implemented");
            assert!(err.starts_with("not_implemented:"), "{err}");
        }
    }

    #[test]
    fn lint_pull_without_frame_errors() {
        let s = session();
        let err = s.lint_pull(&json!({})).expect_err("no frame captured");
        assert_eq!(err, "no lint frame captured yet");
    }

    #[test]
    fn lint_pull_returns_lint_dump() {
        let s = session();
        s.on_frame(&cramped_list());
        let v = s.lint_pull(&json!({})).expect("lint_pull");
        assert_eq!(v["version"], 1);
        assert!(v["scene"]["roots"].is_array());
        assert!(v["report"]["findings"].is_array());
        // Round-trips through the decoder consumers use.
        let dump: LintDump = serde_json::from_value(v).expect("LintDump wire shape");
        assert_eq!(dump.scene.roots.len(), 1);
    }

    #[test]
    fn lint_apply_without_frame_errors() {
        let s = session();
        let err = s.lint_apply(&json!({})).expect_err("no frame captured");
        assert_eq!(err, "no lint frame captured yet");
    }

    #[test]
    fn lint_apply_autofix_never_mutates_live_scene() {
        let s = session();
        s.on_frame(&cramped_list());
        let before = s.lint_pull(&json!({})).unwrap()["report"]["findings"]
            .as_array()
            .map_or(0, Vec::len);

        let v = s.lint_apply(&json!({})).expect("autofix");
        assert_eq!(v["converged"], true);
        assert!(v["applied_ops_count"].as_u64().unwrap_or(0) > 0);
        assert!(v["scene"]["roots"].is_array());
        assert!(v["report"]["findings"].is_array());

        // The bridge's cached frame is untouched — a fresh pull still
        // reports the pre-fix findings (ADR-0038).
        let after = s.lint_pull(&json!({})).unwrap()["report"]["findings"]
            .as_array()
            .map_or(0, Vec::len);
        assert_eq!(before, after);
    }

    #[test]
    fn lint_apply_explicit_ops_and_finding_id() {
        let s = session();
        s.on_frame(&cramped_list());

        // Tagged serialized op.
        let v = s
            .lint_apply(&json!({
                "ops": [{"op": "set_gap", "parent": "App/Panel", "gap": 8.0}]
            }))
            .expect("explicit set_gap");
        assert!(v["applied_ops_count"].as_u64().unwrap_or(0) >= 1);
        assert_eq!(v["converged"], true);

        // Unrecognized ops are invalid-param errors, not silent skips.
        let err = s
            .lint_apply(&json!({"ops": [{"op": "explode"}]}))
            .expect_err("unknown op must fail");
        assert!(err.starts_with("invalid param"), "{err}");
        assert!(s.lint_apply(&json!({"ops": {"op": "set_gap"}})).is_err());
        assert!(s
            .lint_apply(&json!({"finding_id": "deadbeefdeadbeef"}))
            .is_err());

        // `finding_id` resolves against the cached report's fix payloads.
        let fid = {
            let bridge = s.lint.lock().expect("lint mutex");
            bridge
                .report()
                .and_then(|r| {
                    r.findings
                        .iter()
                        .find(|f| f.fix.as_ref().is_some_and(|x| x.safety == FixSafety::Safe))
                        .map(|f| f.fingerprint())
                })
                .expect("a fixable safe finding")
        };
        let v = s
            .lint_apply(&json!({"finding_id": fid}))
            .expect("finding_id apply");
        assert!(v["applied_ops_count"].as_u64().unwrap_or(0) >= 1);
    }

    #[test]
    fn lint_apply_gates_risky_ops_on_force() {
        let s = session();
        s.on_frame(&cramped_list());

        let grow = json!({
            "ops": [{"op": "grow_bounds", "path": "App/Panel/Label",
                     "min_w": 200.0, "min_h": 60.0}]
        });
        let v = s.lint_apply(&grow).expect("unforced apply");
        assert_eq!(v["applied_ops_count"], 0);
        assert_eq!(v["skipped_risky_ops"], 1);

        let v = s
            .lint_apply(&json!({
                "force": true,
                "ops": [{"op": "grow_bounds", "path": "App/Panel/Label",
                         "min_w": 200.0, "min_h": 60.0}]
            }))
            .expect("forced apply");
        assert!(v["applied_ops_count"].as_u64().unwrap_or(0) >= 1);
        assert_eq!(v["skipped_risky_ops"], 0);
    }
}
