//! Category B — Layout & Constraint Diagnostics tools (spec §3.3–3.4).

use std::borrow::Cow;
use std::collections::HashSet;

use martensite_design_lint::LintNode;
use rmcp::handler::server::router::tool::{AsyncTool, ToolBase};
use rmcp::model::ToolAnnotations;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::client::{LayoutStep, TreeSnapshotData, WireTreeNode};
use crate::error::McpError;
use crate::server::MartensiteMcp;
use crate::tools::inspect::{
    find_scene_node, find_wire_node, is_method_missing, requires_live, scene_node_id, wire_node_id,
    Provenance, OVERFLOW_EPSILON,
};
use crate::types::{
    LayoutChainDescriptor, LayoutStepDescriptor, OverflowAxis, OverflowDiagnostic, Pagination,
};

/// Parameters for `martensite_diagnose_layout`.
#[derive(Debug, Clone, Default, Deserialize, JsonSchema)]
#[serde(default)]
pub struct DiagnoseLayoutParams {
    /// Target `WidgetId` to diagnose.
    pub node_id: String,
}

/// Output of `martensite_diagnose_layout`.
///
/// # Examples
///
/// ```
/// use martensite_mcp::tools::inspect::Provenance;
/// use martensite_mcp::tools::layout::DiagnoseLayoutOutput;
/// use martensite_mcp::types::LayoutChainDescriptor;
///
/// let out = DiagnoseLayoutOutput {
///     chain: LayoutChainDescriptor::default(),
///     mode: Provenance::Live,
///     source: "dev-channel:layout_chain".into(),
/// };
/// assert!(out.chain.chain.is_empty());
/// ```
#[derive(Debug, Clone, Serialize, JsonSchema)]
pub struct DiagnoseLayoutOutput {
    /// The node's Taffy constraint-resolution chain.
    pub chain: LayoutChainDescriptor,
    /// `"live"` vs `"offline"` provenance (D9).
    pub mode: Provenance,
    /// Concrete data source (`dev-channel:layout_chain` or the
    /// `dev-channel:tree_snapshot` selected-node fallback).
    pub source: String,
}

/// `martensite_diagnose_layout`: Taffy constraint-resolution chain for a node.
pub struct DiagnoseLayoutTool;

impl ToolBase for DiagnoseLayoutTool {
    type Parameter = DiagnoseLayoutParams;
    type Output = DiagnoseLayoutOutput;
    type Error = McpError;

    fn name() -> Cow<'static, str> {
        "martensite_diagnose_layout".into()
    }

    fn description() -> Option<Cow<'static, str>> {
        Some(
            "Return the exact Taffy layout chain for a widget: inbound \
             constraints, style definitions, intrinsic measure output, and \
             resolved bounds with ancestry propagation."
                .into(),
        )
    }

    fn annotations() -> Option<ToolAnnotations> {
        crate::tools::read_only_annotations()
    }
}

impl AsyncTool<MartensiteMcp> for DiagnoseLayoutTool {
    async fn invoke(
        service: &MartensiteMcp,
        param: Self::Parameter,
    ) -> Result<Self::Output, Self::Error> {
        let wire = serde_json::json!({ "node_id": wire_node_id(&param.node_id) });
        match service.try_live_call("layout_chain", wire) {
            Ok(Some(val)) => Ok(DiagnoseLayoutOutput {
                chain: parse_layout_chain(val, &param.node_id)?,
                mode: Provenance::Live,
                source: "dev-channel:layout_chain".to_string(),
            }),
            Ok(None) => Err(requires_live(
                "martensite_diagnose_layout",
                "Taffy constraint chains are not recorded in scene dumps",
            )),
            Err(e) if is_method_missing(&e) => diagnose_from_snapshot(service, &param.node_id),
            Err(e) => Err(e),
        }
    }
}

/// Converts a wire [`LayoutStep`] to its descriptor form.
fn step_to_descriptor(step: &LayoutStep) -> LayoutStepDescriptor {
    LayoutStepDescriptor {
        name: step.name.clone(),
        constraints: step.constraints.clone(),
        result_size: step.result_size,
        violations: step.violations.clone(),
    }
}

/// Parses the `layout_chain` result: a [`LayoutChainDescriptor`], a
/// `{"chain"}`/`{"layout_chain"}` envelope, or a bare `Vec<LayoutStep>`.
fn parse_layout_chain(
    val: serde_json::Value,
    requested: &str,
) -> Result<LayoutChainDescriptor, McpError> {
    if let Ok(chain) = serde_json::from_value::<LayoutChainDescriptor>(val.clone()) {
        return Ok(chain);
    }
    for key in ["chain", "layout_chain"] {
        if let Some(inner) = val.get(key) {
            if let Ok(steps) = serde_json::from_value::<Vec<LayoutStep>>(inner.clone()) {
                return Ok(chain_from_steps(steps, requested, [0.0; 4]));
            }
        }
    }
    if let Ok(steps) = serde_json::from_value::<Vec<LayoutStep>>(val) {
        return Ok(chain_from_steps(steps, requested, [0.0; 4]));
    }
    Err(McpError::Ipc(
        "unrecognized `layout_chain` payload (expected LayoutChainDescriptor)".to_string(),
    ))
}

/// Builds a [`LayoutChainDescriptor`] envelope around bare wire steps —
/// the fields the wire cannot supply stay at their defaults rather than
/// being fabricated (D9).
fn chain_from_steps(
    steps: Vec<LayoutStep>,
    requested: &str,
    resolved_bounds: [f32; 4],
) -> LayoutChainDescriptor {
    LayoutChainDescriptor {
        node_id: requested.parse().unwrap_or(0),
        resolved_bounds,
        chain: steps.iter().map(step_to_descriptor).collect(),
        ..LayoutChainDescriptor::default()
    }
}

/// `diagnose_layout` fallback for dev apps that predate the `layout_chain`
/// method: `tree_snapshot` still carries the chain of the *selected* node.
/// Per D9 the chain is reused only when the request targets that node —
/// it is never reattributed to another widget.
fn diagnose_from_snapshot(
    service: &MartensiteMcp,
    node_id: &str,
) -> Result<DiagnoseLayoutOutput, McpError> {
    let val = service.live_call("tree_snapshot", serde_json::json!({}))?;
    let snap: TreeSnapshotData = serde_json::from_value(val)
        .map_err(|e| McpError::Ipc(format!("invalid `tree_snapshot` payload: {e}")))?;

    let requested = find_wire_node(&snap.root, node_id)
        .ok_or_else(|| McpError::NodeNotFound(node_id.to_string()))?;
    let Some(selected_id) = snap.selected_id else {
        return Err(McpError::LayoutEngine(format!(
            "no layout chain for `{node_id}`: this dev app predates the \
             `layout_chain` method and nothing is selected in the inspector"
        )));
    };
    let Some(selected) = find_wire_node(&snap.root, &selected_id.to_string()) else {
        return Err(McpError::LayoutEngine(format!(
            "no layout chain for `{node_id}`: the app's selected node \
             {selected_id} is absent from its snapshot"
        )));
    };
    if requested.id != selected.id {
        return Err(McpError::LayoutEngine(format!(
            "no layout chain for `{node_id}`: this dev app predates the \
             `layout_chain` method and only reports the selected node's chain"
        )));
    }

    Ok(DiagnoseLayoutOutput {
        chain: chain_from_steps(snap.layout_chain, node_id, selected.screen_bounds),
        mode: Provenance::Live,
        source: "dev-channel:tree_snapshot".to_string(),
    })
}

/// Parameters for `martensite_explain_overflow`.
#[derive(Debug, Clone, Default, Deserialize, JsonSchema)]
#[serde(default)]
pub struct ExplainOverflowParams {
    /// Target `WidgetId` to diagnose; omitted scans the whole scene.
    pub node_id: Option<String>,
    /// Maximum diagnostics returned (default 50, max 250).
    pub limit: Option<u32>,
    /// Pagination offset (default 0).
    pub offset: Option<u32>,
}

impl ExplainOverflowParams {
    /// Effective result limit, clamped to `[1, 250]`.
    #[must_use]
    pub fn limit(&self) -> usize {
        self.limit.unwrap_or(50).clamp(1, 250) as usize
    }

    /// Effective pagination offset.
    #[must_use]
    pub fn offset(&self) -> usize {
        self.offset.unwrap_or(0) as usize
    }
}

/// Output of `martensite_explain_overflow`.
///
/// # Examples
///
/// ```
/// use martensite_mcp::tools::inspect::Provenance;
/// use martensite_mcp::tools::layout::ExplainOverflowOutput;
/// use martensite_mcp::types::Pagination;
///
/// let out = ExplainOverflowOutput {
///     diagnostics: vec![],
///     pagination: Pagination::new(0, 50, 0),
///     mode: Provenance::Live,
///     source: "dev-channel:overflow_scan".into(),
/// };
/// assert!(out.diagnostics.is_empty());
/// ```
#[derive(Debug, Clone, Serialize, JsonSchema)]
pub struct ExplainOverflowOutput {
    /// Overflow diagnostics on this page.
    pub diagnostics: Vec<OverflowDiagnostic>,
    /// Pagination envelope over the full diagnostic set.
    pub pagination: Pagination,
    /// `"live"` vs `"offline"` provenance (D9).
    pub mode: Provenance,
    /// Concrete data source (`dev-channel:overflow_scan`,
    /// `dev-channel:tree_snapshot` bounds comparison, or `offline:scene`).
    pub source: String,
}

/// `martensite_explain_overflow`: pinpoints overflow cause + clipping lineage.
pub struct ExplainOverflowTool;

impl ToolBase for ExplainOverflowTool {
    type Parameter = ExplainOverflowParams;
    type Output = ExplainOverflowOutput;
    type Error = McpError;

    fn name() -> Cow<'static, str> {
        "martensite_explain_overflow".into()
    }

    fn description() -> Option<Cow<'static, str>> {
        Some(
            "Pinpoint the mathematical cause and clipping lineage of a layout \
             overflow: offending node, axis, pixel magnitude, clipping \
             ancestor, and suggested remediations."
                .into(),
        )
    }

    fn annotations() -> Option<ToolAnnotations> {
        crate::tools::read_only_annotations()
    }
}

impl AsyncTool<MartensiteMcp> for ExplainOverflowTool {
    async fn invoke(
        service: &MartensiteMcp,
        param: Self::Parameter,
    ) -> Result<Self::Output, Self::Error> {
        let mut wire = serde_json::Map::new();
        if let Some(id) = &param.node_id {
            wire.insert("node_id".to_string(), wire_node_id(id));
        }
        match service.try_live_call("overflow_scan", serde_json::Value::Object(wire)) {
            Ok(Some(val)) => {
                let (diagnostics, pagination) = paginate(parse_overflow_scan(val)?, &param);
                Ok(ExplainOverflowOutput {
                    diagnostics,
                    pagination,
                    mode: Provenance::Live,
                    source: "dev-channel:overflow_scan".to_string(),
                })
            }
            Ok(None) => explain_overflow_offline(service, &param),
            Err(e) if is_method_missing(&e) => explain_overflow_from_snapshot(service, &param),
            Err(e) => Err(e),
        }
    }
}

/// Parses the `overflow_scan` result: a bare `Vec<OverflowDiagnostic>` or
/// a `{"diagnostics"}`/`{"overflows"}` envelope.
fn parse_overflow_scan(val: serde_json::Value) -> Result<Vec<OverflowDiagnostic>, McpError> {
    if let Ok(diags) = serde_json::from_value::<Vec<OverflowDiagnostic>>(val.clone()) {
        return Ok(diags);
    }
    for key in ["diagnostics", "overflows"] {
        if let Some(inner) = val.get(key) {
            if let Ok(diags) = serde_json::from_value::<Vec<OverflowDiagnostic>>(inner.clone()) {
                return Ok(diags);
            }
        }
    }
    Err(McpError::Ipc(
        "unrecognized `overflow_scan` payload (expected Vec<OverflowDiagnostic>)".to_string(),
    ))
}

/// Splits `items` into the requested page plus its envelope.
fn paginate<T>(items: Vec<T>, param: &ExplainOverflowParams) -> (Vec<T>, Pagination) {
    let (limit, offset) = (param.limit(), param.offset());
    let pagination = Pagination::new(items.len(), limit, offset);
    (
        items.into_iter().skip(offset).take(limit).collect(),
        pagination,
    )
}

/// `explain_overflow` fallback for dev apps that predate `overflow_scan`:
/// derives a bounds-overflow pass by comparing each child's allocated
/// rect against its parent's — painted-ink overflow is not observable on
/// the wire, so only bounds overflow is reported (D9).
fn explain_overflow_from_snapshot(
    service: &MartensiteMcp,
    param: &ExplainOverflowParams,
) -> Result<ExplainOverflowOutput, McpError> {
    let val = service.live_call("tree_snapshot", serde_json::json!({}))?;
    let snap: TreeSnapshotData = serde_json::from_value(val)
        .map_err(|e| McpError::Ipc(format!("invalid `tree_snapshot` payload: {e}")))?;

    // Diagnostics are filtered to the requested subtree (a node's own
    // protrusion counts as inside it).
    let scope: Option<HashSet<u64>> = match &param.node_id {
        Some(id) => {
            let node =
                find_wire_node(&snap.root, id).ok_or_else(|| McpError::NodeNotFound(id.clone()))?;
            let mut ids = HashSet::new();
            collect_wire_ids(node, &mut ids);
            Some(ids)
        }
        None => None,
    };
    let mut diags = Vec::new();
    collect_wire_overflow(&snap.root, &scope, &mut diags);
    let (diagnostics, pagination) = paginate(diags, param);
    Ok(ExplainOverflowOutput {
        diagnostics,
        pagination,
        mode: Provenance::Live,
        source: "dev-channel:tree_snapshot".to_string(),
    })
}

/// Offline `explain_overflow`: same bounds-overflow pass over the
/// `--scene` dump's `LintNode` geometry.
fn explain_overflow_offline(
    service: &MartensiteMcp,
    param: &ExplainOverflowParams,
) -> Result<ExplainOverflowOutput, McpError> {
    let Some(scene) = service.offline().load_scene()? else {
        return Err(requires_live(
            "martensite_explain_overflow",
            "no --scene file configured",
        ));
    };
    let scope: Option<HashSet<u64>> = match &param.node_id {
        Some(id) => {
            let node =
                find_scene_node(&scene, id).ok_or_else(|| McpError::NodeNotFound(id.clone()))?;
            Some(node.walk().map(scene_node_id).collect())
        }
        None => None,
    };
    let mut diags = Vec::new();
    for root in &scene.roots {
        collect_scene_overflow(root, &scope, &mut diags);
    }
    let (diagnostics, pagination) = paginate(diags, param);
    Ok(ExplainOverflowOutput {
        diagnostics,
        pagination,
        mode: Provenance::Offline,
        source: "offline:scene".to_string(),
    })
}

/// Collects every wire node id in a subtree.
fn collect_wire_ids(root: &WireTreeNode, out: &mut HashSet<u64>) {
    out.insert(root.id);
    for child in &root.children {
        collect_wire_ids(child, out);
    }
}

/// Protrusion of `child` beyond `parent` bounds `[x, y, w, h]` as
/// `(dx, dy)` pixels (0 when contained).
fn bounds_overflow(parent: [f32; 4], child: [f32; 4]) -> (f32, f32) {
    let dx = (child[0] + child[2] - (parent[0] + parent[2])).max(parent[0] - child[0]);
    let dy = (child[1] + child[3] - (parent[1] + parent[3])).max(parent[1] - child[1]);
    (dx.max(0.0), dy.max(0.0))
}

/// Axis classification for a `(dx, dy)` protrusion.
fn overflow_axis(dx: f32, dy: f32) -> OverflowAxis {
    match (dx > OVERFLOW_EPSILON, dy > OVERFLOW_EPSILON) {
        (true, true) => OverflowAxis::Both,
        (true, false) => OverflowAxis::Horizontal,
        _ => OverflowAxis::Vertical,
    }
}

/// Whether a container reads as scrollable by kind/`debug_name`.
fn is_scrollable(kind: &str, name: Option<&str>) -> bool {
    kind.to_ascii_lowercase().contains("scroll")
        || name.is_some_and(|n| n.to_ascii_lowercase().contains("scroll"))
}

/// Human label for a clipping ancestor: `name (scrollable)` or
/// `name (non-scrolling)`.
fn clip_label(kind: &str, name: Option<&str>) -> String {
    let label = name.unwrap_or(kind);
    let capability = if is_scrollable(kind, name) {
        "scrollable"
    } else {
        "non-scrolling"
    };
    format!("{label} ({capability})")
}

/// Remediation suggestions for an overflow (axis-aware, scroll-aware).
fn remediations(horizontal: bool, vertical: bool, parent_scrollable: bool) -> Vec<String> {
    let mut out = Vec::new();
    if horizontal {
        out.push(
            "reduce the child's fixed width or set `flex_shrink(1.0)` so it yields space"
                .to_string(),
        );
        out.push("wrap or truncate long text content".to_string());
    }
    if vertical {
        out.push("reduce the child's fixed height or set `flex_shrink(1.0)`".to_string());
    }
    if !parent_scrollable {
        out.push("wrap the subtree in `ScrollView` to clip and scroll the overflow".to_string());
    }
    out
}

/// Bounds-overflow pass over a wire subtree: every child protruding past
/// its parent's allocated rect produces a diagnostic, filtered to `scope`
/// when a `node_id` was requested.
fn collect_wire_overflow(
    parent: &WireTreeNode,
    scope: &Option<HashSet<u64>>,
    out: &mut Vec<OverflowDiagnostic>,
) {
    for child in &parent.children {
        let (dx, dy) = bounds_overflow(parent.screen_bounds, child.screen_bounds);
        if (dx > OVERFLOW_EPSILON || dy > OVERFLOW_EPSILON)
            && scope.as_ref().is_none_or(|s| s.contains(&child.id))
        {
            out.push(OverflowDiagnostic {
                offending_node: child.id,
                offending_name: child.debug_name.clone(),
                overflow_axis: Some(overflow_axis(dx, dy)),
                overflow_pixels: dx.max(dy),
                clipping_ancestor: Some(clip_label(&parent.kind, parent.debug_name.as_deref())),
                suggested_remediations: remediations(
                    dx > OVERFLOW_EPSILON,
                    dy > OVERFLOW_EPSILON,
                    is_scrollable(&parent.kind, parent.debug_name.as_deref()),
                ),
            });
        }
        collect_wire_overflow(child, scope, out);
    }
}

/// [`collect_wire_overflow`] for scene geometry (device px).
fn collect_scene_overflow(
    parent: &LintNode,
    scope: &Option<HashSet<u64>>,
    out: &mut Vec<OverflowDiagnostic>,
) {
    let eps = f64::from(OVERFLOW_EPSILON);
    for child in &parent.children {
        let (pb, cb) = (&parent.bounds, &child.bounds);
        let dx = (cb.x1 - pb.x1).max(pb.x0 - cb.x0).max(0.0);
        let dy = (cb.y1 - pb.y1).max(pb.y0 - cb.y0).max(0.0);
        if (dx > eps || dy > eps)
            && scope
                .as_ref()
                .is_none_or(|s| s.contains(&scene_node_id(child)))
        {
            let (dx, dy) = (dx as f32, dy as f32);
            out.push(OverflowDiagnostic {
                offending_node: scene_node_id(child),
                offending_name: Some(child.full_name.clone()),
                overflow_axis: Some(overflow_axis(dx, dy)),
                overflow_pixels: dx.max(dy),
                clipping_ancestor: Some(clip_label(&parent.name, Some(parent.full_name.as_str()))),
                suggested_remediations: remediations(
                    dx > OVERFLOW_EPSILON,
                    dy > OVERFLOW_EPSILON,
                    is_scrollable(&parent.name, Some(parent.full_name.as_str())),
                ),
            });
        }
        collect_scene_overflow(child, scope, out);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn wire(id: u64, bounds: [f32; 4], children: Vec<WireTreeNode>) -> WireTreeNode {
        WireTreeNode {
            id,
            debug_name: None,
            kind: "Container".to_string(),
            screen_bounds: bounds,
            local_bounds: bounds,
            depth: 0,
            child_count: children.len(),
            badges: Vec::new(),
            active_signal_count: 0,
            children,
        }
    }

    #[test]
    fn bounds_overflow_reports_protrusion() {
        let parent = [0.0, 0.0, 100.0, 100.0];
        assert_eq!(
            bounds_overflow(parent, [10.0, 10.0, 20.0, 20.0]),
            (0.0, 0.0)
        );
        assert_eq!(
            bounds_overflow(parent, [90.0, 0.0, 20.0, 50.0]),
            (10.0, 0.0)
        );
        assert_eq!(
            bounds_overflow(parent, [0.0, 90.0, 10.0, 20.0]),
            (0.0, 10.0)
        );
        assert_eq!(bounds_overflow(parent, [-5.0, 0.0, 10.0, 10.0]), (5.0, 0.0));
    }

    #[test]
    fn wire_overflow_scan_finds_protruding_children() {
        let tree = wire(
            1,
            [0.0, 0.0, 100.0, 100.0],
            vec![
                wire(2, [10.0, 10.0, 50.0, 50.0], vec![]),
                wire(3, [90.0, 90.0, 30.0, 20.0], vec![]), // +20 x, +10 y
            ],
        );
        let mut diags = Vec::new();
        collect_wire_overflow(&tree, &None, &mut diags);
        assert_eq!(diags.len(), 1);
        assert_eq!(diags[0].offending_node, 3);
        assert_eq!(diags[0].overflow_axis, Some(OverflowAxis::Both));
        assert!((diags[0].overflow_pixels - 20.0).abs() < f32::EPSILON);
    }

    #[test]
    fn wire_overflow_scope_filters_to_subtree() {
        let tree = wire(
            1,
            [0.0, 0.0, 100.0, 100.0],
            vec![wire(
                2,
                [0.0, 0.0, 50.0, 50.0],
                vec![wire(3, [40.0, 40.0, 30.0, 30.0], vec![])],
            )],
        );
        // Node 3 overflows node 2 but not node 1's rect — scoping to the
        // root yields the diagnostic, scoping to an empty subtree none.
        let mut ids = HashSet::new();
        collect_wire_ids(&tree.children[0], &mut ids);
        let mut diags = Vec::new();
        collect_wire_overflow(&tree, &Some(ids), &mut diags);
        assert_eq!(diags.len(), 1);
        assert_eq!(diags[0].offending_node, 3);
    }
}
