//! Category A — Widget Arena & Hierarchy Inspection tools (spec §3.1–3.2).

use std::borrow::Cow;

use martensite_design_lint::{LintNode, LintScene};
use rmcp::handler::server::router::tool::{AsyncTool, ToolBase};
use rmcp::model::ToolAnnotations;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::client::{TreeSnapshotData, WireTreeNode};
use crate::error::McpError;
use crate::server::MartensiteMcp;
use crate::types::{NodeDescriptor, Pagination, TreeNodeDescriptor};

/// High bit marking synthetic ids assigned to scene-dump nodes that carry
/// no `widget_id`, keeping them disjoint from real arena identifiers.
pub(crate) const SYNTHETIC_ID_BASE: u64 = 1 << 63;

/// Sentinel id of the `+N virtualized rows` placeholder node (D7) — never
/// a real arena id.
pub(crate) const PLACEHOLDER_ID: u64 = u64::MAX;

/// Sentinel id of the synthetic `LintScene` wrapper root emitted when an
/// offline scene dump holds several top-level scopes.
pub(crate) const SCENE_WRAPPER_ID: u64 = u64::MAX - 1;

/// Minimum protrusion (px) reported as overflow — sub-pixel noise from
/// fractional layout is ignored.
pub(crate) const OVERFLOW_EPSILON: f32 = 0.01;

/// Whether a tool response was answered by the live dev channel or by
/// offline static analysis (D9 semantic honesty: callers must always be
/// able to distinguish runtime truth from scene-dump inference).
///
/// # Examples
///
/// ```
/// use martensite_mcp::tools::inspect::Provenance;
///
/// assert_eq!(serde_json::to_string(&Provenance::Live).unwrap(), "\"live\"");
/// ```
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum Provenance {
    /// Live dev-channel response from a running application.
    Live,
    /// Offline response derived from a `--scene` dump.
    Offline,
}

/// Whether `err` is the dev channel's `-32601` method-not-found mapping —
/// i.e. the running app predates this tool's wire method and a snapshot
/// fallback should be attempted.
pub(crate) fn is_method_missing(err: &McpError) -> bool {
    matches!(err, McpError::Ipc(msg)
        if msg.contains("does not implement") || msg.contains("method not found"))
}

/// Serializes a `node_id` argument for the wire: numeric `WidgetId`s go as
/// integers, `debug_name` paths as strings.
pub(crate) fn wire_node_id(id: &str) -> serde_json::Value {
    match id.parse::<u64>() {
        Ok(n) => serde_json::Value::from(n),
        Err(_) => serde_json::Value::from(id),
    }
}

/// Strips the leading `@` and whitespace from a `filter_marker` argument.
fn marker_key(marker: &str) -> &str {
    marker.strip_prefix('@').unwrap_or(marker).trim()
}

/// Structured error for tools with no offline evaluation path.
pub(crate) fn requires_live(tool: &str, why: &str) -> McpError {
    McpError::Ipc(format!(
        "{tool} requires a live Martensite dev session ({why}); \
         start `cargo martensite dev` or pass --socket <path>"
    ))
}

/// `@`-suffix sections of a `debug_name` that are semantic markers;
/// `@lint:` allow specifiers are excluded.
fn name_markers(debug_name: &str) -> Vec<String> {
    debug_name
        .split('@')
        .skip(1)
        .map(|s| s.trim().to_ascii_lowercase())
        .filter(|s| !s.is_empty() && !s.starts_with("lint:"))
        .collect()
}

/// Whether a wire node's `debug_name` markers or badges carry `marker`.
fn wire_has_marker(node: &WireTreeNode, marker: &str) -> bool {
    node.debug_name
        .as_deref()
        .is_some_and(|n| name_markers(n).iter().any(|m| m == marker))
        || node
            .badges
            .iter()
            .any(|b| b.trim_start_matches('@').eq_ignore_ascii_case(marker))
}

/// Whether `node` or any descendant carries `marker` — the pruning rule
/// for `filter_marker` (ancestors of a match are retained as context).
fn wire_subtree_has_marker(node: &WireTreeNode, marker: &str) -> bool {
    wire_has_marker(node, marker)
        || node
            .children
            .iter()
            .any(|c| wire_subtree_has_marker(c, marker))
}

/// Whether a wire node's `debug_name` matches `id_or_path` (exact or last
/// `::` segment, marker suffixes ignored).
fn wire_name_matches(node: &WireTreeNode, id_or_path: &str) -> bool {
    let Some(name) = node.debug_name.as_deref() else {
        return false;
    };
    let clean = name.split('@').next().unwrap_or(name).trim();
    name == id_or_path || clean == id_or_path || clean.rsplit("::").next() == Some(id_or_path)
}

/// Locates a node by numeric `WidgetId` or `debug_name` (first match in
/// document order).
pub(crate) fn find_wire_node<'a>(
    node: &'a WireTreeNode,
    id_or_name: &str,
) -> Option<&'a WireTreeNode> {
    if let Ok(id) = id_or_name.parse::<u64>() {
        if node.id == id {
            return Some(node);
        }
    }
    if wire_name_matches(node, id_or_name) {
        return Some(node);
    }
    node.children
        .iter()
        .find_map(|c| find_wire_node(c, id_or_name))
}

/// Deterministic id for a scene node: the dumped `widget_id` when the dump
/// carried one, else `SYNTHETIC_ID_BASE | hash(path)` so ids stay stable
/// across `inspect_tree`/`get_node`/`explain_overflow` calls without
/// pretending to be arena ids (D9).
pub(crate) fn scene_node_id(node: &LintNode) -> u64 {
    node.widget_id.unwrap_or_else(|| {
        use std::hash::{Hash, Hasher};
        let mut h = std::collections::hash_map::DefaultHasher::new();
        node.path.hash(&mut h);
        SYNTHETIC_ID_BASE | (h.finish() & (SYNTHETIC_ID_BASE - 1))
    })
}

/// Logical `[x, y, w, h]` bounds of a scene node — scene geometry is in
/// device px; descriptors report logical units (`bounds / scale_factor`).
fn scene_logical_bounds(node: &LintNode, scale: f32) -> [f32; 4] {
    let s = f64::from(scale).max(1e-6);
    let b = &node.bounds;
    [
        (b.x0 / s) as f32,
        (b.y0 / s) as f32,
        ((b.x1 - b.x0) / s) as f32,
        ((b.y1 - b.y0) / s) as f32,
    ]
}

/// Physical `[x, y, w, h]` device-pixel bounds of a scene node.
fn scene_physical_bounds(node: &LintNode) -> [f32; 4] {
    let b = &node.bounds;
    [
        b.x0 as f32,
        b.y0 as f32,
        (b.x1 - b.x0) as f32,
        (b.y1 - b.y0) as f32,
    ]
}

/// Whether a scene node carries `marker` (`@alarm`, `@level:N`, ...).
fn lint_has_marker(node: &LintNode, marker: &str) -> bool {
    if let Some(level) = marker.strip_prefix("level:") {
        return level.trim().parse::<u8>().ok() == node.display_level;
    }
    node.has_marker(marker)
}

/// Whether `node` or any descendant carries `marker` (see
/// [`wire_subtree_has_marker`]).
fn lint_subtree_has_marker(node: &LintNode, marker: &str) -> bool {
    lint_has_marker(node, marker)
        || node
            .children
            .iter()
            .any(|c| lint_subtree_has_marker(c, marker))
}

/// Whether a scene node matches `id_or_path`: numeric ids compare against
/// [`scene_node_id`]; strings match `path`, `full_name` (marker suffixes
/// ignored), the short `name`, or a `/`-path suffix.
fn scene_node_matches(node: &LintNode, id_or_path: &str) -> bool {
    if let Ok(id) = id_or_path.parse::<u64>() {
        if scene_node_id(node) == id {
            return true;
        }
    }
    let clean = node
        .full_name
        .split('@')
        .next()
        .unwrap_or(&node.full_name)
        .trim();
    node.path == id_or_path
        || node.full_name == id_or_path
        || node.name == id_or_path
        || clean == id_or_path
        || node.path.ends_with(&format!("/{id_or_path}"))
}

/// Resolves a `node_id` argument against a scene (first match in document
/// order).
pub(crate) fn find_scene_node<'a>(scene: &'a LintScene, id_or_path: &str) -> Option<&'a LintNode> {
    scene.walk().find(|n| scene_node_matches(n, id_or_path))
}

/// Depth/breadth/filter clamping shared by the live and offline tree
/// builders.
struct ClampOpts {
    max_depth: usize,
    child_limit: usize,
    offset: usize,
    filter: Option<String>,
}

/// Converts a wire subtree to a [`TreeNodeDescriptor`], re-applying the
/// request clamps client-side (defensive: a dev app may ignore the
/// request params). `is_root` marks the query anchor — the pagination
/// `offset` applies only to its direct children.
///
/// Rows the app withheld beyond our breadth clamp (virtualized
/// `DataGrid`/`ScrollView` containers) surface as a `+N virtualized rows`
/// placeholder child instead of being silently dropped (D7).
fn wire_to_descriptor(
    node: &WireTreeNode,
    depth: usize,
    opts: &ClampOpts,
    is_root: bool,
) -> TreeNodeDescriptor {
    let mut desc = TreeNodeDescriptor {
        id: node.id,
        debug_name: node.debug_name.clone(),
        kind: node.kind.clone(),
        bounds: node.screen_bounds,
        child_count: node.child_count,
        depth,
        badges: node.badges.clone(),
        active_signal_count: node.active_signal_count,
        virtualized_rows: None,
        children: Vec::new(),
    };
    if depth < opts.max_depth {
        let skip = if is_root { opts.offset } else { 0 };
        for child in node.children.iter().skip(skip).take(opts.child_limit) {
            if let Some(marker) = &opts.filter {
                if !wire_subtree_has_marker(child, marker) {
                    continue;
                }
            }
            desc.children
                .push(wire_to_descriptor(child, depth + 1, opts, false));
        }
        let elided = node.child_count.saturating_sub(node.children.len());
        if elided > 0 && node.children.len() < opts.child_limit {
            desc.virtualized_rows = Some(elided);
            let mut placeholder =
                TreeNodeDescriptor::leaf(PLACEHOLDER_ID, "virtualized_rows", [0.0; 4]);
            placeholder.debug_name = Some(format!("+{elided} virtualized rows"));
            placeholder.depth = depth + 1;
            placeholder.virtualized_rows = Some(elided);
            desc.children.push(placeholder);
        }
    }
    desc
}

/// Converts a scene node to a [`TreeNodeDescriptor`], applying the same
/// depth/breadth/filter clamps as the live path.
fn lint_to_descriptor(
    node: &LintNode,
    depth: usize,
    opts: &ClampOpts,
    is_root: bool,
    scale: f32,
) -> TreeNodeDescriptor {
    let mut desc = TreeNodeDescriptor {
        id: scene_node_id(node),
        debug_name: Some(node.full_name.clone()),
        kind: node.name.clone(),
        bounds: scene_logical_bounds(node, scale),
        child_count: node.children.len(),
        depth,
        badges: Vec::new(),
        active_signal_count: 0,
        virtualized_rows: None,
        children: Vec::new(),
    };
    if depth < opts.max_depth {
        let skip = if is_root { opts.offset } else { 0 };
        for child in node.children.iter().skip(skip).take(opts.child_limit) {
            if let Some(marker) = &opts.filter {
                if !lint_subtree_has_marker(child, marker) {
                    continue;
                }
            }
            desc.children
                .push(lint_to_descriptor(child, depth + 1, opts, false, scale));
        }
    }
    desc
}

/// Total children of a wire query root before breadth clamping — the
/// `total` of the pagination envelope. With `filter_marker` active it is
/// the count of matching (or match-containing) materialized children.
fn wire_children_total(root: &WireTreeNode, filter: Option<&str>) -> usize {
    match filter {
        Some(m) => root
            .children
            .iter()
            .filter(|c| wire_subtree_has_marker(c, m))
            .count(),
        None => root.child_count,
    }
}

/// [`wire_children_total`] for scene nodes.
fn lint_children_total(root: &LintNode, filter: Option<&str>) -> usize {
    match filter {
        Some(m) => root
            .children
            .iter()
            .filter(|c| lint_subtree_has_marker(c, m))
            .count(),
        None => root.children.len(),
    }
}

/// Full [`NodeDescriptor`] for a scene node — only fields the dump
/// actually carries are populated (D9: no fabricated clip rects,
/// AccessKit state, or source spans).
fn lint_node_descriptor(node: &LintNode, scale: f32) -> NodeDescriptor {
    let mut markers = node.markers.clone();
    if let Some(level) = node.display_level {
        markers.push(format!("level:{level}"));
    }
    NodeDescriptor {
        id: scene_node_id(node),
        generation: None,
        kind: node.name.clone(),
        debug_name: Some(node.full_name.clone()),
        logical_bounds: scene_logical_bounds(node, scale),
        physical_bounds: scene_physical_bounds(node),
        clip_rect: None,
        z_index: 0,
        markers,
        badges: Vec::new(),
        loading: None,
        aria_role: None,
        accesskit: Default::default(),
        source_span: None,
    }
}

/// [`NodeDescriptor`] recovered from a snapshot [`WireTreeNode`] — the
/// fallback when the app does not implement `tree_node`. `local_bounds`
/// map to `logical_bounds` and `screen_bounds` to `physical_bounds`.
fn wire_node_descriptor(node: &WireTreeNode) -> NodeDescriptor {
    NodeDescriptor {
        id: node.id,
        generation: None,
        kind: node.kind.clone(),
        debug_name: node.debug_name.clone(),
        logical_bounds: node.local_bounds,
        physical_bounds: node.screen_bounds,
        clip_rect: None,
        z_index: 0,
        markers: node
            .debug_name
            .as_deref()
            .map(name_markers)
            .unwrap_or_default(),
        badges: node.badges.clone(),
        // Older hosts carry no `loading` field on snapshot nodes; the
        // flag-derived badge is the only honest signal there.
        loading: node.badges.iter().any(|b| b == "loading").then_some(true),
        aria_role: None,
        accesskit: Default::default(),
        source_span: None,
    }
}

/// Parses a `tree_node` result: a [`NodeDescriptor`], a `{"node": …}`
/// envelope, or a bare [`WireTreeNode`] for older dev apps.
fn parse_tree_node(val: serde_json::Value, requested: &str) -> Result<NodeDescriptor, McpError> {
    if val.is_null() {
        return Err(McpError::NodeNotFound(requested.to_string()));
    }
    for candidate in [val.clone(), val.get("node").cloned().unwrap_or_default()] {
        if let Ok(desc) = serde_json::from_value::<NodeDescriptor>(candidate.clone()) {
            return Ok(desc);
        }
        if let Ok(wire) = serde_json::from_value::<WireTreeNode>(candidate) {
            return Ok(wire_node_descriptor(&wire));
        }
    }
    Err(McpError::Ipc(
        "unrecognized `tree_node` payload (expected NodeDescriptor or WireTreeNode)".to_string(),
    ))
}

/// Offline `inspect_tree`: builds a clamped tree from the `--scene` dump.
fn inspect_tree_offline(
    service: &MartensiteMcp,
    param: &InspectTreeParams,
    opts: &ClampOpts,
) -> Result<InspectTreeOutput, McpError> {
    let Some(scene) = service.offline().load_scene()? else {
        return Err(requires_live(
            "martensite_inspect_tree",
            "no --scene file configured",
        ));
    };
    let scale = scene.scale_factor;
    let (root, total) = match &param.root_id {
        Some(rid) => {
            let node =
                find_scene_node(&scene, rid).ok_or_else(|| McpError::NodeNotFound(rid.clone()))?;
            let total = lint_children_total(node, opts.filter.as_deref());
            (lint_to_descriptor(node, 0, opts, true, scale), total)
        }
        None if scene.roots.len() == 1 => {
            let node = &scene.roots[0];
            let total = lint_children_total(node, opts.filter.as_deref());
            (lint_to_descriptor(node, 0, opts, true, scale), total)
        }
        None => {
            // Multi-window / manually built dump: wrap the roots in a
            // synthetic `LintScene` node (sentinel id, zero bounds).
            let mut wrapper = TreeNodeDescriptor {
                id: SCENE_WRAPPER_ID,
                debug_name: Some("LintScene".to_string()),
                kind: "LintScene".to_string(),
                bounds: [0.0; 4],
                child_count: scene.roots.len(),
                depth: 0,
                badges: Vec::new(),
                active_signal_count: 0,
                virtualized_rows: None,
                children: Vec::new(),
            };
            for r in scene.roots.iter().skip(opts.offset).take(opts.child_limit) {
                if let Some(m) = &opts.filter {
                    if !lint_subtree_has_marker(r, m) {
                        continue;
                    }
                }
                wrapper
                    .children
                    .push(lint_to_descriptor(r, 1, opts, false, scale));
            }
            let total = match &opts.filter {
                Some(m) => scene
                    .roots
                    .iter()
                    .filter(|r| lint_subtree_has_marker(r, m))
                    .count(),
                None => scene.roots.len(),
            };
            (wrapper, total)
        }
    };
    Ok(InspectTreeOutput {
        root,
        pagination: Pagination::new(total, opts.child_limit, opts.offset),
        mode: Provenance::Offline,
        source: "offline:scene".to_string(),
    })
}
/// Parameters for `martensite_inspect_tree`.
///
/// # Examples
///
/// ```
/// use martensite_mcp::tools::inspect::InspectTreeParams;
///
/// let p = InspectTreeParams::default();
/// assert_eq!(p.max_depth(), 4);
/// ```
#[derive(Debug, Clone, Default, Deserialize, JsonSchema)]
#[serde(default)]
pub struct InspectTreeParams {
    /// Target `WidgetId` to scope subtree inspection; defaults to arena root.
    pub root_id: Option<String>,
    /// Maximum vertical recursion depth (default 4, max 32).
    pub max_depth: Option<u32>,
    /// Maximum direct children returned per parent (default 50, max 200).
    pub child_limit: Option<u32>,
    /// Child pagination offset (default 0).
    pub offset: Option<u32>,
    /// Filter nodes by semantic marker (e.g. `@alarm`, `@kpi`, `@level:1`).
    pub filter_marker: Option<String>,
    /// Expand widget-internal child-protocol nodes (`child_count`/`child`).
    pub include_internal: Option<bool>,
}

impl InspectTreeParams {
    /// Effective depth limit, clamped to the spec maximum.
    #[must_use]
    pub fn max_depth(&self) -> u32 {
        self.max_depth.unwrap_or(4).min(32)
    }

    /// Effective breadth clamp, bounded to the spec maximum.
    #[must_use]
    pub fn child_limit(&self) -> u32 {
        self.child_limit.unwrap_or(50).min(200)
    }

    /// Effective pagination offset.
    #[must_use]
    pub fn offset(&self) -> u32 {
        self.offset.unwrap_or(0)
    }

    /// Effective `include_internal` flag.
    #[must_use]
    pub fn include_internal(&self) -> bool {
        self.include_internal.unwrap_or(false)
    }
}

/// Output of `martensite_inspect_tree`.
///
/// # Examples
///
/// ```
/// use martensite_mcp::tools::inspect::{InspectTreeOutput, Provenance};
/// use martensite_mcp::types::{Pagination, TreeNodeDescriptor};
///
/// let out = InspectTreeOutput {
///     root: TreeNodeDescriptor::leaf(0, "App", [0.0, 0.0, 800.0, 600.0]),
///     pagination: Pagination::new(0, 50, 0),
///     mode: Provenance::Offline,
///     source: "offline:scene".into(),
/// };
/// assert!(out.pagination.is_last_page());
/// ```
#[derive(Debug, Clone, Serialize, JsonSchema)]
pub struct InspectTreeOutput {
    /// Root of the returned (depth- and breadth-clamped) subtree.
    pub root: TreeNodeDescriptor,
    /// Pagination envelope over the root's direct children.
    pub pagination: Pagination,
    /// `"live"` dev channel vs `"offline"` scene dump (D9).
    pub mode: Provenance,
    /// Concrete data source, e.g. `dev-channel:tree_snapshot` or
    /// `offline:scene`.
    pub source: String,
}

/// `martensite_inspect_tree`: hierarchical widget tree with breadth clamping.
pub struct InspectTreeTool;

impl ToolBase for InspectTreeTool {
    type Parameter = InspectTreeParams;
    type Output = InspectTreeOutput;
    type Error = McpError;

    fn name() -> Cow<'static, str> {
        "martensite_inspect_tree".into()
    }

    fn description() -> Option<Cow<'static, str>> {
        Some(
            "Query the live widget tree with depth limits, horizontal breadth \
             clamping, marker filtering, and virtualized-row placeholders (D7)."
                .into(),
        )
    }

    fn annotations() -> Option<ToolAnnotations> {
        crate::tools::read_only_annotations()
    }
}

impl AsyncTool<MartensiteMcp> for InspectTreeTool {
    async fn invoke(
        service: &MartensiteMcp,
        param: Self::Parameter,
    ) -> Result<Self::Output, Self::Error> {
        let opts = ClampOpts {
            max_depth: param.max_depth() as usize,
            child_limit: param.child_limit() as usize,
            offset: param.offset() as usize,
            filter: param
                .filter_marker
                .as_deref()
                .map(marker_key)
                .filter(|s| !s.is_empty())
                .map(str::to_ascii_lowercase),
        };

        // The wire `root_id` is numeric; a `debug_name` path is resolved
        // client-side against the returned snapshot.
        let mut wire = serde_json::json!({
            "max_depth": opts.max_depth,
            "child_limit": opts.child_limit,
            "offset": opts.offset,
            "include_internal": param.include_internal(),
        });
        if let Some(root) = param.root_id.as_deref().and_then(|s| s.parse::<u64>().ok()) {
            wire["root_id"] = serde_json::Value::from(root);
        }
        if let Some(marker) = &opts.filter {
            wire["filter_marker"] = serde_json::Value::from(marker.clone());
        }

        if let Some(val) = service.try_live_call("tree_snapshot", wire)? {
            let snap: TreeSnapshotData = serde_json::from_value(val)
                .map_err(|e| McpError::Ipc(format!("invalid `tree_snapshot` payload: {e}")))?;
            let root_wire = match &param.root_id {
                Some(rid) => find_wire_node(&snap.root, rid)
                    .ok_or_else(|| McpError::NodeNotFound(rid.clone()))?,
                None => &snap.root,
            };
            let total = wire_children_total(root_wire, opts.filter.as_deref());
            let root = wire_to_descriptor(root_wire, 0, &opts, true);
            return Ok(InspectTreeOutput {
                root,
                pagination: Pagination::new(total, opts.child_limit, opts.offset),
                mode: Provenance::Live,
                source: "dev-channel:tree_snapshot".to_string(),
            });
        }

        inspect_tree_offline(service, &param, &opts)
    }
}

/// Parameters for `martensite_get_node`.
#[derive(Debug, Clone, Default, Deserialize, JsonSchema)]
#[serde(default)]
pub struct GetNodeParams {
    /// Unique `WidgetId` or `debug_name` path of the target node.
    pub node_id: String,
}

/// Output of `martensite_get_node`.
///
/// # Examples
///
/// ```
/// use martensite_mcp::tools::inspect::{GetNodeOutput, Provenance};
/// use martensite_mcp::types::NodeDescriptor;
///
/// let out = GetNodeOutput {
///     node: NodeDescriptor::default(),
///     mode: Provenance::Live,
///     source: "dev-channel:tree_node".into(),
/// };
/// assert_eq!(out.node.id, 0);
/// ```
#[derive(Debug, Clone, Serialize, JsonSchema)]
pub struct GetNodeOutput {
    /// The resolved node record.
    pub node: NodeDescriptor,
    /// `"live"` vs `"offline"` provenance (D9).
    pub mode: Provenance,
    /// Concrete data source (`dev-channel:tree_node`,
    /// `dev-channel:tree_snapshot`, or `offline:scene`).
    pub source: String,
}

/// `martensite_get_node`: comprehensive single-widget record.
pub struct GetNodeTool;

impl ToolBase for GetNodeTool {
    type Parameter = GetNodeParams;
    type Output = GetNodeOutput;
    type Error = McpError;

    fn name() -> Cow<'static, str> {
        "martensite_get_node".into()
    }

    fn description() -> Option<Cow<'static, str>> {
        Some(
            "Retrieve the complete record for one widget node: generational \
             index, kind, logical/physical bounds, clip rect, markers, ARIA \
             role, AccessKit properties, and source span."
                .into(),
        )
    }

    fn annotations() -> Option<ToolAnnotations> {
        crate::tools::read_only_annotations()
    }
}

impl AsyncTool<MartensiteMcp> for GetNodeTool {
    async fn invoke(
        service: &MartensiteMcp,
        param: Self::Parameter,
    ) -> Result<Self::Output, Self::Error> {
        let wire = serde_json::json!({ "node_id": wire_node_id(&param.node_id) });
        match service.try_live_call("tree_node", wire) {
            Ok(Some(val)) => Ok(GetNodeOutput {
                node: parse_tree_node(val, &param.node_id)?,
                mode: Provenance::Live,
                source: "dev-channel:tree_node".to_string(),
            }),
            Ok(None) => get_node_offline(service, &param),
            Err(e) if is_method_missing(&e) => {
                // The running app predates `tree_node` — fall back to a
                // snapshot walk for the id/`debug_name`.
                let val = service.live_call("tree_snapshot", serde_json::json!({}))?;
                let snap: TreeSnapshotData = serde_json::from_value(val)
                    .map_err(|e| McpError::Ipc(format!("invalid `tree_snapshot` payload: {e}")))?;
                let node = find_wire_node(&snap.root, &param.node_id)
                    .ok_or_else(|| McpError::NodeNotFound(param.node_id.clone()))?;
                Ok(GetNodeOutput {
                    node: wire_node_descriptor(node),
                    mode: Provenance::Live,
                    source: "dev-channel:tree_snapshot".to_string(),
                })
            }
            Err(e) => Err(e),
        }
    }
}

/// Offline `get_node`: resolves the id/`debug_name` path against the
/// `--scene` dump.
fn get_node_offline(
    service: &MartensiteMcp,
    param: &GetNodeParams,
) -> Result<GetNodeOutput, McpError> {
    let Some(scene) = service.offline().load_scene()? else {
        return Err(requires_live(
            "martensite_get_node",
            "no --scene file configured",
        ));
    };
    let node = find_scene_node(&scene, &param.node_id)
        .ok_or_else(|| McpError::NodeNotFound(param.node_id.clone()))?;
    Ok(GetNodeOutput {
        node: lint_node_descriptor(node, scene.scale_factor),
        mode: Provenance::Offline,
        source: "offline:scene".to_string(),
    })
}

/// Parameters for `martensite_set_loading` (ADR-0040 phase 3).
///
/// # Examples
///
/// ```
/// use martensite_mcp::tools::inspect::SetLoadingParams;
///
/// let p = SetLoadingParams {
///     node_id: "42".to_string(),
///     loading: Some(true),
/// };
/// assert_eq!(p.loading, Some(true));
/// ```
#[derive(Debug, Clone, Default, Deserialize, JsonSchema)]
#[serde(default)]
pub struct SetLoadingParams {
    /// Target `WidgetId` (numeric id as returned by
    /// `martensite_inspect_tree`/`martensite_get_node`).
    pub node_id: String,
    /// New loading state: `true` forces the skeleton placeholder,
    /// `false` clears the instance override (a widget's own
    /// `is_loading` declaration still applies). Required.
    pub loading: Option<bool>,
}

/// Result payload of `martensite_set_loading`.
///
/// # Examples
///
/// ```
/// use martensite_mcp::tools::inspect::{Provenance, SetLoadingOutput};
///
/// let out = SetLoadingOutput {
///     node_id: "42".to_string(),
///     applied: true,
///     loading: Some(true),
///     revision: Some(3),
///     mode: Provenance::Live,
/// };
/// assert!(out.applied);
/// ```
#[derive(Debug, Clone, Serialize, JsonSchema)]
pub struct SetLoadingOutput {
    /// Node the mutation targeted (echoed for correlation).
    pub node_id: String,
    /// Whether the dev app applied the override.
    pub applied: bool,
    /// Effective loading state after the write — the instance override
    /// OR'd with the widget's own `is_loading` declaration (ADR-0040).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub loading: Option<bool>,
    /// Mutation revision stamped by the dev session, when reported.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub revision: Option<u64>,
    /// Server mode that produced the answer (`live` on success).
    pub mode: Provenance,
}

/// `martensite_set_loading`: force a node's `LOADING` flag on a live app.
pub struct SetLoadingTool;

impl ToolBase for SetLoadingTool {
    type Parameter = SetLoadingParams;
    type Output = SetLoadingOutput;
    type Error = McpError;

    fn name() -> Cow<'static, str> {
        "martensite_set_loading".into()
    }

    fn description() -> Option<Cow<'static, str>> {
        Some(
            "Force or clear a widget node's loading state (ADR-0040): the \
             skeleton placeholder replaces the node's subtree, the a11y node \
             reports `busy` with the sanitized \"Loading\" label, and input \
             is covered. Verify via martensite_inspect_a11y_tree and \
             martensite_capture_node. Requires a live `cargo martensite dev` \
             session."
                .into(),
        )
    }

    fn annotations() -> Option<ToolAnnotations> {
        crate::tools::mutation_annotations()
    }
}

impl AsyncTool<MartensiteMcp> for SetLoadingTool {
    async fn invoke(
        service: &MartensiteMcp,
        param: Self::Parameter,
    ) -> Result<Self::Output, Self::Error> {
        let node_id = param.node_id.trim();
        if node_id.is_empty() {
            return Err(McpError::InvalidParameter(
                "`node_id` must be a non-empty widget id".to_string(),
            ));
        }
        let Some(loading) = param.loading else {
            return Err(McpError::InvalidParameter(
                "`loading` is required (true forces the placeholder, false \
                 clears the override)"
                    .to_string(),
            ));
        };

        // Live-only: a loading override mutates a running arena, so there
        // is no offline equivalent — `live_call` reports the honest IPC
        // error when no dev session answers (D9).
        let resp = service.live_call(
            "node_set_loading",
            serde_json::json!({ "node_id": wire_node_id(node_id), "loading": loading }),
        )?;
        Ok(set_loading_output(node_id, &resp))
    }
}

/// Builds the typed `node_set_loading` result from the app's wire
/// response, tolerating `{applied|ok|success, loading, revision}` objects
/// and bare `true`/`false` verdicts.
fn set_loading_output(node_id: &str, resp: &serde_json::Value) -> SetLoadingOutput {
    let (applied, loading, revision) = match resp {
        serde_json::Value::Bool(b) => (*b, Some(*b), None),
        serde_json::Value::Object(map) => {
            let applied = ["applied", "ok", "success"]
                .iter()
                .find_map(|key| map.get(*key))
                .and_then(serde_json::Value::as_bool)
                .unwrap_or(true);
            let loading = map.get("loading").and_then(serde_json::Value::as_bool);
            let revision = map
                .get("revision")
                .and_then(|v| v.as_u64().or_else(|| v.as_str()?.parse().ok()));
            (applied, loading, revision)
        }
        _ => (true, None, None),
    };
    SetLoadingOutput {
        node_id: node_id.to_string(),
        applied,
        loading,
        revision,
        mode: Provenance::Live,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn wire(id: u64, name: &str, children: Vec<WireTreeNode>) -> WireTreeNode {
        WireTreeNode {
            id,
            debug_name: Some(name.to_string()),
            kind: "Container".to_string(),
            screen_bounds: [0.0, 0.0, 10.0, 10.0],
            local_bounds: [0.0, 0.0, 10.0, 10.0],
            depth: 0,
            child_count: children.len(),
            badges: Vec::new(),
            active_signal_count: 0,
            children,
        }
    }

    fn opts(max_depth: usize, child_limit: usize, offset: usize) -> ClampOpts {
        ClampOpts {
            max_depth,
            child_limit,
            offset,
            filter: None,
        }
    }

    #[test]
    fn wire_descriptor_clamps_depth_and_breadth() {
        let tree = wire(
            1,
            "Root",
            vec![
                wire(2, "A", vec![wire(4, "A1", vec![])]),
                wire(3, "B", vec![]),
            ],
        );
        let d = wire_to_descriptor(&tree, 0, &opts(1, 1, 0), true);
        assert_eq!(d.children.len(), 1);
        assert!(d.children[0].children.is_empty(), "depth clamp failed");
        let d = wire_to_descriptor(&tree, 0, &opts(4, 50, 0), true);
        assert_eq!(d.children.len(), 2);
        assert_eq!(d.children[0].children.len(), 1);
    }

    #[test]
    fn wire_descriptor_marks_elided_rows() {
        let mut tree = wire(1, "DataGrid", vec![wire(2, "Row", vec![])]);
        tree.child_count = 500; // app withheld 499 rows
        let d = wire_to_descriptor(&tree, 0, &opts(4, 50, 0), true);
        assert_eq!(d.virtualized_rows, Some(499));
        let placeholder = d.children.last().expect("placeholder child");
        assert_eq!(placeholder.id, PLACEHOLDER_ID);
        assert_eq!(placeholder.virtualized_rows, Some(499));
    }

    #[test]
    fn filter_marker_prunes_unmatched_subtrees() {
        let tree = wire(
            1,
            "Root",
            vec![
                wire(2, "Panel@alarm", vec![]),
                wire(3, "Plain", vec![wire(4, "Leaf@kpi", vec![])]),
                wire(5, "Other", vec![]),
            ],
        );
        let mut o = opts(4, 50, 0);
        o.filter = Some("kpi".to_string());
        let d = wire_to_descriptor(&tree, 0, &o, true);
        assert_eq!(d.children.len(), 1);
        assert_eq!(d.children[0].id, 3);
        assert_eq!(wire_children_total(&tree, Some("kpi")), 1);
        o.filter = Some("alarm".to_string());
        let d = wire_to_descriptor(&tree, 0, &o, true);
        assert_eq!(d.children.len(), 1);
        assert_eq!(d.children[0].id, 2);
    }

    #[test]
    fn find_wire_node_by_id_and_name() {
        let tree = wire(1, "App::Root", vec![wire(7, "widgets::Tabs", vec![])]);
        assert_eq!(find_wire_node(&tree, "7").map(|n| n.id), Some(7));
        assert_eq!(find_wire_node(&tree, "Tabs").map(|n| n.id), Some(7));
        assert_eq!(
            find_wire_node(&tree, "widgets::Tabs").map(|n| n.id),
            Some(7)
        );
        assert!(find_wire_node(&tree, "missing").is_none());
    }

    #[test]
    fn scene_node_id_is_stable_and_synthetic() {
        let node = LintNode {
            name: "Tabs".into(),
            full_name: "widgets::Tabs".into(),
            path: "App/Tabs".into(),
            bounds: Default::default(),
            widget_id: None,
            kind: martensite_design_lint::NodeKind::Navigation,
            allows: vec![],
            own_allows: vec![],
            display_level: None,
            markers: vec![],
            font_sizes: vec![],
            colors: vec![],
            texts: vec![],
            fills: vec![],
            painted_area: 0.0,
            children: vec![],
        };
        let id = scene_node_id(&node);
        assert_eq!(id, scene_node_id(&node));
        assert!(id >= SYNTHETIC_ID_BASE);
        let mut with_id = node.clone();
        with_id.widget_id = Some(42);
        assert_eq!(scene_node_id(&with_id), 42);
    }

    #[test]
    fn set_loading_output_parses_envelope() {
        let out = set_loading_output(
            "7",
            &serde_json::json!({"applied": true, "loading": true, "revision": 4}),
        );
        assert!(out.applied);
        assert_eq!(out.node_id, "7");
        assert_eq!(out.loading, Some(true));
        assert_eq!(out.revision, Some(4));
        assert_eq!(out.mode, Provenance::Live);

        // Bare verdict + tolerant field absence.
        let out = set_loading_output("9", &serde_json::json!(false));
        assert!(!out.applied);
        assert_eq!(out.loading, Some(false));
        assert_eq!(out.revision, None);
    }

    #[tokio::test]
    async fn set_loading_validates_and_requires_live_session() {
        use crate::server::McpServerOptions;

        let server = MartensiteMcp::new(McpServerOptions::offline());

        // Param validation runs before any IPC attempt.
        let res = SetLoadingTool::invoke(&server, SetLoadingParams::default()).await;
        assert!(matches!(res, Err(McpError::InvalidParameter(_))));
        let res = SetLoadingTool::invoke(
            &server,
            SetLoadingParams {
                node_id: "7".to_string(),
                loading: None,
            },
        )
        .await;
        assert!(matches!(res, Err(McpError::InvalidParameter(_))));

        // Offline: an honest IPC error — never a fabricated mutation (D9).
        let res = SetLoadingTool::invoke(
            &server,
            SetLoadingParams {
                node_id: "7".to_string(),
                loading: Some(true),
            },
        )
        .await;
        match res {
            Err(McpError::Ipc(msg)) => {
                assert!(
                    msg.contains("requires a live Martensite dev session"),
                    "{msg}"
                );
            }
            other => panic!("expected offline IPC hint, got {other:?}"),
        }
    }
}
