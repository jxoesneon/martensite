//! Serializable descriptor types shared across MCP tools, resources, and the
//! dev-channel bridge.
//!
//! These types are the semantic ground-truth payloads returned by the
//! `martensite_*` tool suite (D9): every field derives directly from the
//! authoritative `WidgetArena`, `LayoutEngine`, reactive runtime, or design
//! lint state — never from pixel heuristics.

use std::collections::BTreeMap;

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

/// Version of the MCP protocol implemented by this crate.
pub const MCP_PROTOCOL_VERSION: u32 = 1;

/// Wire protocol version of the ADR-0038 dev channel.
pub const DEV_CHANNEL_PROTOCOL_VERSION: u32 = 1;

/// Embedded crate version for version-parity verification (D1).
pub const MARTENSITE_VERSION: &str = env!("CARGO_PKG_VERSION");

/// Token-safe pagination envelope shared by list-returning tools.
///
/// # Examples
///
/// ```
/// use martensite_mcp::types::Pagination;
///
/// let page = Pagination::new(120, 50, 0);
/// assert_eq!(page.next_offset, Some(50));
/// assert!(!page.is_last_page());
/// ```
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct Pagination {
    /// Maximum entries requested for this page.
    pub limit: usize,
    /// Offset into the full result set.
    pub offset: usize,
    /// Total entries available before pagination.
    pub total: usize,
    /// Offset to request for the next page, or `null` when exhausted.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub next_offset: Option<usize>,
}

impl Pagination {
    /// Builds a pagination envelope for `total` items given a page request.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite_mcp::types::Pagination;
    ///
    /// let page = Pagination::new(200, 50, 100);
    /// assert_eq!(page.next_offset, Some(150));
    /// ```
    #[must_use]
    pub fn new(total: usize, limit: usize, offset: usize) -> Self {
        let next_offset = (offset + limit < total).then_some(offset + limit);
        Self {
            limit,
            offset,
            total,
            next_offset,
        }
    }

    /// Whether this page is the final page of the result set.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite_mcp::types::Pagination;
    ///
    /// assert!(Pagination::new(10, 50, 0).is_last_page());
    /// ```
    #[must_use]
    pub fn is_last_page(&self) -> bool {
        self.next_offset.is_none()
    }
}

/// A node in the hierarchical widget tree returned by `martensite_inspect_tree`.
///
/// # Examples
///
/// ```
/// use martensite_mcp::types::TreeNodeDescriptor;
///
/// let node = TreeNodeDescriptor::leaf(7, "Text", [10.0, 20.0, 100.0, 30.0]);
/// assert_eq!(node.id, 7);
/// ```
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct TreeNodeDescriptor {
    /// Widget arena identifier.
    pub id: u64,
    /// Debug name registered on the widget, if any.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub debug_name: Option<String>,
    /// Node kind classification (e.g. `Container`, `Text`, `Flex`).
    #[serde(default)]
    pub kind: String,
    /// Logical bounding rectangle `[x, y, width, height]`.
    #[serde(default)]
    pub bounds: [f32; 4],
    /// Total number of direct children (before `child_limit` clamping).
    #[serde(default)]
    pub child_count: usize,
    /// Depth rank in the hierarchy (0 = root).
    #[serde(default)]
    pub depth: usize,
    /// State badges (`has_lint_warnings`, `signal_dirty`, ...).
    #[serde(default)]
    pub badges: Vec<String>,
    /// Count of active reactive signals bound to this widget.
    #[serde(default)]
    pub active_signal_count: usize,
    /// Number of virtualized rows elided per D7 (`+N virtualized rows`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub virtualized_rows: Option<usize>,
    /// Materialized children after depth/breadth clamping.
    #[serde(default)]
    pub children: Vec<TreeNodeDescriptor>,
}

impl TreeNodeDescriptor {
    /// Creates a leaf descriptor with the given id, kind, and bounds.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite_mcp::types::TreeNodeDescriptor;
    ///
    /// let leaf = TreeNodeDescriptor::leaf(1, "Button", [0.0, 0.0, 64.0, 32.0]);
    /// assert!(leaf.children.is_empty());
    /// ```
    #[must_use]
    pub fn leaf(id: u64, kind: impl Into<String>, bounds: [f32; 4]) -> Self {
        Self {
            id,
            debug_name: None,
            kind: kind.into(),
            bounds,
            child_count: 0,
            depth: 0,
            badges: Vec::new(),
            active_signal_count: 0,
            virtualized_rows: None,
            children: Vec::new(),
        }
    }
}

/// Comprehensive record for a single widget node (`martensite_get_node`).
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct NodeDescriptor {
    /// Widget arena identifier.
    pub id: u64,
    /// Generational index of the arena slot, if reported by the host.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub generation: Option<u32>,
    /// Node kind classification (e.g. `Flex`, `Container`, `Text`, `Button`).
    #[serde(default)]
    pub kind: String,
    /// Debug name registered on the widget, if any.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub debug_name: Option<String>,
    /// Computed logical bounds `[x, y, width, height]`.
    #[serde(default)]
    pub logical_bounds: [f32; 4],
    /// Physical pixel bounds `[x, y, width, height]`.
    #[serde(default)]
    pub physical_bounds: [f32; 4],
    /// Clip rectangle `[x, y, width, height]`, if clipping applies.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub clip_rect: Option<[f32; 4]>,
    /// Paint-order z index.
    #[serde(default)]
    pub z_index: i32,
    /// Semantic markers attached to the node (`@alarm`, `@kpi`, ...).
    #[serde(default)]
    pub markers: Vec<String>,
    /// Live flag-derived state badges (`dirty_paint`, `hovered`,
    /// `loading`, ...), when the probe reports them.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub badges: Vec<String>,
    /// Effective loading state (`NodeFlags::LOADING` override or the
    /// widget's own `is_loading` declaration, ADR-0040), when the host
    /// reports it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub loading: Option<bool>,
    /// ARIA role for the node, when exposed to AccessKit.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub aria_role: Option<String>,
    /// Registered AccessKit properties (name, description, enabled, ...).
    #[serde(default)]
    pub accesskit: BTreeMap<String, String>,
    /// Source span when compiled with `devtools-source-spans`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source_span: Option<SourceSpan>,
}

/// A Rust source-code span (`file:line:col`) for widget origin attribution.
///
/// # Examples
///
/// ```
/// use martensite_mcp::types::SourceSpan;
///
/// let span = SourceSpan { file: "src/main.rs".into(), line: 12, column: 5 };
/// assert_eq!(span.to_string(), "src/main.rs:12:5");
/// ```
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct SourceSpan {
    /// Workspace-relative source file path.
    pub file: String,
    /// 1-based line number.
    pub line: u32,
    /// 1-based column number.
    pub column: u32,
}

impl std::fmt::Display for SourceSpan {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}:{}:{}", self.file, self.line, self.column)
    }
}

/// One step in a widget's layout constraint chain (`martensite_diagnose_layout`).
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct LayoutStepDescriptor {
    /// Step description (widget name or constraint origin).
    pub name: String,
    /// Textual constraint summary (e.g. `min: [0, 0], max: [800, 600]`).
    #[serde(default)]
    pub constraints: String,
    /// Resulting dimensions `[width, height]` at this step.
    #[serde(default)]
    pub result_size: [f32; 2],
    /// Constraint violations or warnings observed at this step.
    #[serde(default)]
    pub violations: Vec<String>,
}

/// Full layout chain diagnostic for a node (`martensite_diagnose_layout`).
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct LayoutChainDescriptor {
    /// The node whose layout chain is described.
    pub node_id: u64,
    /// Inbound constraints received from the parent.
    #[serde(default)]
    pub inbound_constraints: String,
    /// Taffy style definitions applied to the node.
    #[serde(default)]
    pub style: BTreeMap<String, String>,
    /// Intrinsic measure closure output `(min_content, max_content)`.
    #[serde(default)]
    pub intrinsic_measure: [f32; 2],
    /// Final resolved layout bounds `[x, y, width, height]`.
    #[serde(default)]
    pub resolved_bounds: [f32; 4],
    /// Ancestry constraint propagation chain, root-first.
    #[serde(default)]
    pub chain: Vec<LayoutStepDescriptor>,
}

/// Axis along which content overflows its allocated bounds.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum OverflowAxis {
    /// Horizontal (x-axis) overflow.
    Horizontal,
    /// Vertical (y-axis) overflow.
    Vertical,
    /// Overflow on both axes.
    Both,
}

/// Mathematical cause and clipping lineage of a layout overflow
/// (`martensite_explain_overflow`).
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct OverflowDiagnostic {
    /// Node whose content exceeds its allocated bounds.
    pub offending_node: u64,
    /// Debug name of the offending node, when known.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub offending_name: Option<String>,
    /// Axis on which the overflow occurs.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub overflow_axis: Option<OverflowAxis>,
    /// Exact floating-point pixel magnitude of the overflow.
    #[serde(default)]
    pub overflow_pixels: f32,
    /// Parent container enforcing the bounds and its scroll capability.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub clipping_ancestor: Option<String>,
    /// Actionable remediation suggestions.
    #[serde(default)]
    pub suggested_remediations: Vec<String>,
}

/// Tolerant deserializer for fields the app may report as explicit
/// `null` (capability unknown) rather than omitting — maps `null` to
/// `T::default` so absent detail never fails the decode.
fn null_default<'de, D, T>(d: D) -> Result<T, D::Error>
where
    D: serde::Deserializer<'de>,
    T: serde::Deserialize<'de> + Default,
{
    Option::<T>::deserialize(d).map(Option::unwrap_or_default)
}

/// Descriptor for one reactive signal (`martensite_inspect_signals`).
///
/// The dev session emits `null` for graph details the reactive runtime
/// cannot enumerate (subscriber counts, dependency edges, rank) — those
/// surface as absent here rather than fabricated zeros. `name`,
/// `writable`, and `poisoned` appear when the app registers
/// `SignalAdapter`s or the runtime exposes them.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct SignalDescriptor {
    /// Signal identifier in the reactive registry.
    pub id: String,
    /// Human-facing name when the app registered a `SignalAdapter`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    /// Current value serialized to JSON, when readable.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub value: Option<serde_json::Value>,
    /// Number of subscribers (consumers) of this signal — absent when
    /// the runtime does not expose subscriber bookkeeping.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub subscriber_count: Option<usize>,
    /// Producer signal dependencies feeding this signal. The session
    /// wire names this `dependencies`.
    #[serde(default, alias = "dependencies", deserialize_with = "null_default")]
    pub producers: Vec<String>,
    /// Consumer nodes/signals invalidated by this signal. The session
    /// wire names this `dependents`.
    #[serde(default, alias = "dependents", deserialize_with = "null_default")]
    pub consumers: Vec<String>,
    /// Topological scheduler rank in the push-pull DAG — absent when
    /// the runtime does not expose scheduler metadata.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub scheduler_rank: Option<usize>,
    /// Whether the signal is flagged dirty this evaluation tick.
    #[serde(default)]
    pub dirty: bool,
    /// Whether the signal's last evaluation poisoned its dependents.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub poisoned: Option<bool>,
    /// Whether a registered adapter makes this signal writable via
    /// `martensite_set_signal`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub writable: Option<bool>,
}

/// One design-lint violation (`martensite_lint_scene`).
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct LintFindingDescriptor {
    /// Stable finding identifier usable with `martensite_apply_lint_fix`.
    pub finding_id: String,
    /// Lint rule identifier (e.g. `wcag.contrast.minimum`).
    #[serde(default)]
    pub rule_id: String,
    /// Design standard the rule belongs to (`wcag`, `isa-101`, ...).
    #[serde(default)]
    pub standard: String,
    /// Severity (`info`, `warn`, `forbid`).
    #[serde(default)]
    pub severity: String,
    /// Widget node the finding is attributed to, when applicable.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub node_id: Option<u64>,
    /// `debug_name` lineage path of the node, when applicable.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub node_path: Option<String>,
    /// Human-readable violation description.
    #[serde(default)]
    pub message: String,
    /// Standard citation URL backing the rule.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub citation_url: Option<String>,
    /// Whether an autofix payload is attached to the finding.
    #[serde(default)]
    pub has_autofix: bool,
}

/// Aggregate lint finding counts for a scene.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct LintSummaryCounts {
    /// Active (non-suppressed) findings.
    pub active: usize,
    /// Suppressed findings.
    #[serde(default)]
    pub suppressed: usize,
    /// Findings at `forbid` severity.
    #[serde(default)]
    pub forbid: usize,
    /// Findings at `warn` severity.
    #[serde(default)]
    pub warn: usize,
    /// Findings at `info` severity.
    #[serde(default)]
    pub info: usize,
}

/// A registered live-tweakable parameter (`martensite_list_tweaks`).
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct TweakDescriptor {
    /// Tweak identifier registered via `#[tweak]` or `TweakRegistry`.
    pub name: String,
    /// Value kind (`f32`, `Color`, `Spring`, `String`, `bool`, ...).
    #[serde(default)]
    pub kind: String,
    /// Current value serialized to JSON.
    #[serde(default, alias = "value")]
    pub current_value: serde_json::Value,
    /// Compile-time default value serialized to JSON.
    #[serde(default)]
    pub default_value: serde_json::Value,
    /// Workspace-relative source file declaring the tweak.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source_file: Option<String>,
    /// 1-based line of the tweak declaration.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source_line: Option<u32>,
    /// Optimistic concurrency token for `martensite_sync_tweaks_to_source`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub revision_token: Option<String>,
    /// Whether the live value differs from the default.
    #[serde(default, alias = "dirty")]
    pub modified: bool,
}

/// One routed-event record in the event ledger (`martensite_get_event_ledger`).
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct EventRecordDescriptor {
    /// Monotonic sequence number in the ledger.
    pub seq: u64,
    /// ISO 8601 timestamp of dispatch.
    #[serde(default)]
    pub timestamp: String,
    /// Event type (`pointer_click`, `key_press`, `scroll`, ...).
    #[serde(default)]
    pub event_type: String,
    /// Hit-test target widget id, when resolved.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub target_id: Option<u64>,
    /// Bubbling ancestry of widget ids, target-first.
    #[serde(default)]
    pub ancestry: Vec<u64>,
    /// Response produced by the router (`Handled`, `Ignored`, ...).
    #[serde(default)]
    pub response: String,
    /// Rejection reason when the event was dropped.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub rejection_reason: Option<String>,
}

/// One node of the AccessKit semantic tree (`martensite_inspect_a11y_tree`).
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct A11yNodeDescriptor {
    /// AccessKit node identifier.
    pub id: String,
    /// ARIA role (`button`, `heading`, `list`, `dialog`, ...).
    #[serde(default)]
    pub role: String,
    /// Accessible name.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    /// Accessible description.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    /// Accessibility states (`expanded`, `selected`, `checked`, `disabled`).
    #[serde(default)]
    pub states: Vec<String>,
    /// Live-region attribute (`polite`, `assertive`), when set.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub live_region: Option<String>,
    /// Whether the node is presentation-only / unmapped.
    #[serde(default)]
    pub ignored: bool,
    /// APG conformance findings attached to this node.
    #[serde(default)]
    pub apg_findings: Vec<String>,
    /// Child accessibility nodes.
    #[serde(default)]
    pub children: Vec<A11yNodeDescriptor>,
}

/// One structured runtime error record (`martensite_runtime_errors`).
///
/// Covers captured panics (`install_dev_panic_hook`), active `ErrorSurface`
/// diagnostics, and reactive-runtime evaluation errors.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct RuntimeErrorDescriptor {
    /// Monotonic sequence number of the record (assigned by array index when
    /// the wire payload omits it).
    #[serde(default)]
    pub seq: u64,
    /// Severity (`info`, `warning`, `error`, `critical`, `fatal`).
    #[serde(default, alias = "level")]
    pub severity: String,
    /// Producing subsystem (`panic`, `error_surface`, `reactive`, ...).
    #[serde(default, alias = "kind", alias = "origin")]
    pub source: String,
    /// Human-readable error summary.
    #[serde(default, alias = "msg", alias = "one_line_cause")]
    pub message: String,
    /// Widget node the error is attributed to, when applicable.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub node_id: Option<u64>,
    /// ISO 8601 capture timestamp, when reported.
    #[serde(
        default,
        alias = "time",
        alias = "ts",
        skip_serializing_if = "Option::is_none"
    )]
    pub timestamp: Option<String>,
}

/// One captured tracing event (`martensite_logs`).
///
/// # Examples
///
/// ```
/// use martensite_mcp::types::LogRecordDescriptor;
///
/// let rec = LogRecordDescriptor {
///     seq: 7,
///     level: "WARN".to_string(),
///     target: "martensite::layout".to_string(),
///     message: "constraint cycle".to_string(),
///     file: None,
///     line: None,
///     timestamp: "2025-01-01T00:00:00Z".to_string(),
/// };
/// assert_eq!(rec.level, "WARN");
/// ```
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct LogRecordDescriptor {
    /// Monotonic capture sequence in the session log ring.
    #[serde(default)]
    pub seq: u64,
    /// Event level (`TRACE`/`DEBUG`/`INFO`/`WARN`/`ERROR`).
    #[serde(default)]
    pub level: String,
    /// `tracing` target (module path).
    #[serde(default)]
    pub target: String,
    /// Formatted log message.
    #[serde(default)]
    pub message: String,
    /// Source file, when available.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub file: Option<String>,
    /// Source line, when available.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub line: Option<u32>,
    /// RFC 3339 capture timestamp.
    #[serde(default)]
    pub timestamp: String,
}

/// Structured audit record emitted for every disk mutation (spec §6.4).
///
/// # Examples
///
/// ```
/// use martensite_mcp::types::AuditRecord;
///
/// let rec = AuditRecord::new("sync_tweaks_to_source", "src/theme.rs", 12, 14);
/// assert_eq!(rec.operation, "sync_tweaks_to_source");
/// ```
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct AuditRecord {
    /// ISO 8601 timestamp of the mutation.
    pub timestamp: String,
    /// Operation performed (`sync_tweaks_to_source`, `scaffold_widget`, ...).
    pub operation: String,
    /// Workspace-relative file path written.
    pub file_path: String,
    /// First modified line (1-based).
    pub span_start: u32,
    /// Last modified line (1-based, inclusive).
    pub span_end: u32,
    /// Unified diff applied to the file.
    #[serde(default)]
    pub diff: String,
    /// Optimistic revision token used for the write, when applicable.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub revision_token: Option<String>,
}

impl AuditRecord {
    /// Creates an audit record stamped with the current UTC time.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite_mcp::types::AuditRecord;
    ///
    /// let rec = AuditRecord::new("op", "src/lib.rs", 1, 3);
    /// assert_eq!(rec.file_path, "src/lib.rs");
    /// ```
    #[must_use]
    pub fn new(
        operation: impl Into<String>,
        file_path: impl Into<String>,
        span_start: u32,
        span_end: u32,
    ) -> Self {
        Self {
            timestamp: iso_now(),
            operation: operation.into(),
            file_path: file_path.into(),
            span_start,
            span_end,
            diff: String::new(),
            revision_token: None,
        }
    }
}

/// Returns the current time as an ISO 8601 UTC string without pulling in a
/// datetime dependency (second precision is sufficient for audit records).
#[must_use]
pub fn iso_now() -> String {
    let secs = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    let (days, rem) = (secs / 86_400, secs % 86_400);
    let (h, m, s) = (rem / 3600, (rem % 3600) / 60, rem % 60);
    // Days since 1970-01-01 → civil date (Howard Hinnant's algorithm).
    let z = days + 719_468;
    let era = z / 146_097;
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let mo = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = if mo <= 2 { y + 1 } else { y };
    format!("{y:04}-{mo:02}-{d:02}T{h:02}:{m:02}:{s:02}Z")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pagination_envelope() {
        let p = Pagination::new(200, 50, 0);
        assert_eq!(p.next_offset, Some(50));
        assert!(!p.is_last_page());
        let last = Pagination::new(200, 50, 150);
        assert!(last.is_last_page());
        assert_eq!(last.next_offset, None);
    }

    #[test]
    fn iso_now_format() {
        let ts = iso_now();
        assert_eq!(ts.len(), 20);
        assert!(ts.ends_with('Z'));
        assert_eq!(&ts[4..5], "-");
    }

    #[test]
    fn descriptors_serialize() {
        let node = TreeNodeDescriptor::leaf(3, "Text", [0.0, 0.0, 10.0, 10.0]);
        let json = serde_json::to_string(&node).expect("serialize");
        assert!(json.contains("\"id\":3"));
        let finding = LintFindingDescriptor {
            finding_id: "f-1".to_string(),
            ..Default::default()
        };
        let _ = serde_json::to_value(&finding).expect("serialize finding");
    }
}
