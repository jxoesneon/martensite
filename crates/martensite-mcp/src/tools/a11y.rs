//! Category D — Accessibility & Semantic Contract tool (spec §3.7).
//!
//! Live-only (ADR-0039, invariant D9 — semantic honesty): the AccessKit
//! semantic mirror (`SemanticTreeSync` in `martensite-access`) exists only
//! inside a running application, so this tool forwards over the ADR-0038
//! dev channel and surfaces the offline-mode IPC error when no
//! `cargo martensite dev` session answers. The server never synthesizes an
//! accessibility tree offline.

use std::borrow::Cow;

use rmcp::handler::server::router::tool::{AsyncTool, ToolBase};
use rmcp::model::ToolAnnotations;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use serde_json::json;

use crate::error::McpError;
use crate::server::{MartensiteMcp, ServerMode};
use crate::types::A11yNodeDescriptor;

/// Parameters for `martensite_inspect_a11y_tree`.
///
/// # Examples
///
/// ```
/// use martensite_mcp::tools::a11y::InspectA11yTreeParams;
///
/// let p = InspectA11yTreeParams {
///     role_filter: Some("button".to_string()),
///     ..InspectA11yTreeParams::default()
/// };
/// assert!(p.validate().is_ok());
/// assert!(!p.include_ignored());
/// ```
#[derive(Debug, Clone, Default, Deserialize, JsonSchema)]
#[serde(default)]
pub struct InspectA11yTreeParams {
    /// Target `NodeId` or widget anchor to scope the subtree.
    pub root_id: Option<String>,
    /// Filter by ARIA role (`button`, `heading`, `list`, `dialog`, ...).
    pub role_filter: Option<String>,
    /// Include presentation-only / unmapped nodes (default false).
    pub include_ignored: Option<bool>,
}

impl InspectA11yTreeParams {
    /// Effective `include_ignored` flag.
    #[must_use]
    pub fn include_ignored(&self) -> bool {
        self.include_ignored.unwrap_or(false)
    }

    /// Validates parameters client-side before the dev-channel round trip.
    ///
    /// # Errors
    ///
    /// [`McpError::InvalidParameter`] when `root_id` is blank or
    /// `role_filter` names no known AccessKit role.
    pub fn validate(&self) -> Result<(), McpError> {
        if let Some(root) = &self.root_id {
            if root.trim().is_empty() {
                return Err(McpError::InvalidParameter(
                    "`root_id` must be a non-empty node anchor".to_string(),
                ));
            }
        }
        if let Some(role) = &self.role_filter {
            if !is_known_role(role) {
                return Err(McpError::InvalidParameter(format!(
                    "`role_filter` `{role}` is not a known ARIA/AccessKit role \
                     (examples: `button`, `heading`, `list`, `dialog`, `tab`, \
                     `slider`, `text_input`)"
                )));
            }
        }
        Ok(())
    }
}

/// Result payload of `martensite_inspect_a11y_tree`.
///
/// # Examples
///
/// ```
/// use martensite_mcp::tools::a11y::InspectA11yTreeOutput;
///
/// let out = InspectA11yTreeOutput {
///     root: None,
///     apg_findings: vec!["7 (button): missing accessible name".to_string()],
///     mode: "live".to_string(),
/// };
/// assert!(out.root.is_none());
/// ```
#[derive(Debug, Clone, Serialize, JsonSchema)]
pub struct InspectA11yTreeOutput {
    /// Root of the (possibly filtered) semantic subtree; `None` when the
    /// anchor resolved to no node or the filters emptied the subtree.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub root: Option<A11yNodeDescriptor>,
    /// APG pattern-conformance findings, attributed by node id and role.
    #[serde(default)]
    pub apg_findings: Vec<String>,
    /// Server mode that produced the answer (`live` on success).
    pub mode: String,
}

/// `martensite_inspect_a11y_tree`: hierarchical AccessKit semantic tree.
pub struct InspectA11yTreeTool;

impl ToolBase for InspectA11yTreeTool {
    type Parameter = InspectA11yTreeParams;
    type Output = InspectA11yTreeOutput;
    type Error = McpError;

    fn name() -> Cow<'static, str> {
        "martensite_inspect_a11y_tree".into()
    }

    fn description() -> Option<Cow<'static, str>> {
        Some(
            "Evaluate the AccessKit accessibility tree independently of layout: \
             accessible names, roles, states, live regions, and APG pattern \
             conformance findings. `role_filter` retains matching nodes plus \
             their ancestor chain. Requires a live `cargo martensite dev` \
             session."
                .into(),
        )
    }

    fn annotations() -> Option<ToolAnnotations> {
        crate::tools::read_only_annotations()
    }
}

impl AsyncTool<MartensiteMcp> for InspectA11yTreeTool {
    async fn invoke(
        service: &MartensiteMcp,
        param: Self::Parameter,
    ) -> Result<Self::Output, Self::Error> {
        param.validate()?;

        // Live-only per spec: `live_call` produces the offline IPC error.
        let resp = service.live_call(
            "a11y_tree",
            json!({
                "root_id": param.root_id,
                "role_filter": param.role_filter,
                "include_ignored": param.include_ignored(),
            }),
        )?;
        let (mut root, wire_findings) = parse_a11y_response(resp)?;

        // Client-side filters: idempotent when the app already applied them
        // and keeps the returned tree honest when it did not (D9).
        if !param.include_ignored() {
            if let Some(root) = root.as_mut() {
                prune_ignored(root);
            }
        }
        if let Some(role) = param.role_filter.as_deref() {
            let needle = normalize_role(role);
            root = root.and_then(|mut r| filter_by_role(&mut r, &needle).then_some(r));
        }

        // App-provided conformance findings are authoritative when present;
        // otherwise aggregate the per-node findings of the surviving tree.
        let apg_findings = if wire_findings.is_empty() {
            root.as_ref().map(collect_apg_findings).unwrap_or_default()
        } else {
            wire_findings
        };

        Ok(InspectA11yTreeOutput {
            root,
            apg_findings,
            mode: mode_str(service).to_string(),
        })
    }
}

/// Decodes the `a11y_tree` payload: a `{ "root": node }` envelope, a bare
/// [`A11yNodeDescriptor`] object, an optional top-level `apg_findings`
/// string array, or `null` for an unresolvable anchor.
fn parse_a11y_response(
    value: serde_json::Value,
) -> Result<(Option<A11yNodeDescriptor>, Vec<String>), McpError> {
    let mut wire_findings = Vec::new();
    let node_value = match value {
        serde_json::Value::Null => return Ok((None, wire_findings)),
        serde_json::Value::Object(mut map) => {
            if let Some(findings) = map
                .get("apg_findings")
                .and_then(serde_json::Value::as_array)
            {
                wire_findings = findings
                    .iter()
                    .filter_map(serde_json::Value::as_str)
                    .map(str::to_string)
                    .collect();
            }
            match map.remove("root") {
                Some(inner) => inner,
                // A map carrying node fields directly is the bare-node shape;
                // an envelope without any node keys decodes as an empty tree.
                None if map.contains_key("id")
                    || map.contains_key("role")
                    || map.contains_key("children") =>
                {
                    serde_json::Value::Object(map)
                }
                None => serde_json::Value::Null,
            }
        }
        other => other,
    };

    if node_value.is_null() {
        return Ok((None, wire_findings));
    }
    let root: A11yNodeDescriptor = serde_json::from_value(node_value)
        .map_err(|e| McpError::Ipc(format!("malformed `a11y_tree` response payload: {e}")))?;
    Ok((Some(root), wire_findings))
}

/// Removes `ignored` descendants, splicing their children into the parent
/// slot — presentation-only wrappers are transparent to assistive
/// technology. The scoped root is kept even when ignored because it is the
/// anchor the caller asked for.
fn prune_ignored(node: &mut A11yNodeDescriptor) {
    let mut kept = Vec::with_capacity(node.children.len());
    for mut child in std::mem::take(&mut node.children) {
        prune_ignored(&mut child);
        if child.ignored {
            kept.extend(child.children);
        } else {
            kept.push(child);
        }
    }
    node.children = kept;
}

/// Retains nodes whose role matches `needle` plus the ancestor chain needed
/// to reach them, pruning the rest. Returns whether `node` survives.
fn filter_by_role(node: &mut A11yNodeDescriptor, needle: &str) -> bool {
    let matches = normalize_role(&node.role) == needle;
    node.children
        .retain_mut(|child| filter_by_role(child, needle));
    matches || !node.children.is_empty()
}

/// Aggregates per-node APG conformance findings into a flat list attributed
/// by node id and role.
fn collect_apg_findings(root: &A11yNodeDescriptor) -> Vec<String> {
    fn walk(node: &A11yNodeDescriptor, out: &mut Vec<String>) {
        for finding in &node.apg_findings {
            out.push(format!("{} ({}): {}", node.id, node.role, finding));
        }
        for child in &node.children {
            walk(child, out);
        }
    }
    let mut out = Vec::new();
    walk(root, &mut out);
    out
}

/// Normalizes a role token for comparison — lowercase ASCII alphanumeric
/// only, so `ListBox`, `list_box`, and `listbox` all match.
fn normalize_role(role: &str) -> String {
    role.chars()
        .filter(|c| c.is_ascii_alphanumeric())
        .map(|c| c.to_ascii_lowercase())
        .collect()
}

/// Whether `role` plausibly names an `accesskit::Role` (normalized form).
fn is_known_role(role: &str) -> bool {
    let norm = normalize_role(role);
    !norm.is_empty() && KNOWN_ROLES.contains(&norm.as_str())
}

/// `SemanticAction` names accepted by `martensite_invoke_accessibility_action`
/// — snake_case spellings of every [`martensite_core::SemanticAction`]
/// variant, matching the host `A11yAction` contract.
const A11Y_ACTIONS: &[&str] = &[
    "click",
    "focus",
    "blur",
    "set_value",
    "increment",
    "decrement",
    "expand",
    "collapse",
    "show_tooltip",
    "hide_tooltip",
    "show_context_menu",
    "scroll_up",
    "scroll_down",
    "scroll_left",
    "scroll_right",
    "scroll_into_view",
    "scroll_to_point",
    "set_scroll_offset",
];

/// Parameters for `martensite_invoke_accessibility_action`.
///
/// # Examples
///
/// ```
/// use martensite_mcp::tools::a11y::InvokeA11yActionParams;
///
/// let p = InvokeA11yActionParams {
///     node_id: Some("7".to_string()),
///     action: "click".to_string(),
///     ..InvokeA11yActionParams::default()
/// };
/// assert!(p.validate().is_ok());
/// ```
#[derive(Debug, Clone, Default, Deserialize, JsonSchema)]
#[serde(default)]
pub struct InvokeA11yActionParams {
    /// Target widget id (number or decimal string). When omitted the
    /// session dispatches to the currently focused node.
    pub node_id: Option<String>,
    /// `SemanticAction` name, snake_case: `click`, `focus`, `blur`,
    /// `set_value`, `increment`, `decrement`, `expand`, `collapse`,
    /// `show_tooltip`, `hide_tooltip`, `show_context_menu`, `scroll_up`,
    /// `scroll_down`, `scroll_left`, `scroll_right`, `scroll_into_view`,
    /// `scroll_to_point`, `set_scroll_offset`.
    pub action: String,
    /// Payload for `set_value` — ignored for other actions.
    pub value: Option<String>,
    /// `[x, y]` logical point for `scroll_to_point` / `set_scroll_offset`.
    pub point: Option<[f32; 2]>,
}

impl InvokeA11yActionParams {
    /// Validates parameters client-side before the dev-channel round trip.
    ///
    /// # Errors
    ///
    /// [`McpError::InvalidAction`] when `action` names no known
    /// `SemanticAction`; [`McpError::InvalidParameter`] when `node_id` is
    /// blank, `set_value` lacks `value`, or `scroll_to_point` /
    /// `set_scroll_offset` lack `point`.
    pub fn validate(&self) -> Result<(), McpError> {
        if !A11Y_ACTIONS.contains(&self.action.as_str()) {
            return Err(McpError::InvalidAction(format!(
                "action `{}` is invalid; expected one of: {}",
                self.action,
                A11Y_ACTIONS.join(", ")
            )));
        }
        if let Some(node) = &self.node_id {
            if node.trim().is_empty() {
                return Err(McpError::InvalidParameter(
                    "`node_id` must be a non-empty widget id".to_string(),
                ));
            }
        }
        if self.action == "set_value" && self.value.is_none() {
            return Err(McpError::InvalidParameter(
                "action `set_value` requires `value`".to_string(),
            ));
        }
        if matches!(
            self.action.as_str(),
            "scroll_to_point" | "set_scroll_offset"
        ) && self.point.is_none()
        {
            return Err(McpError::InvalidParameter(format!(
                "action `{}` requires `point` ([x, y] logical coordinates)",
                self.action
            )));
        }
        Ok(())
    }
}

/// Result payload of `martensite_invoke_accessibility_action`.
///
/// # Examples
///
/// ```
/// use martensite_mcp::tools::a11y::InvokeA11yActionOutput;
///
/// let out = InvokeA11yActionOutput {
///     action: "click".to_string(),
///     response: "Handled".to_string(),
///     target: Some("7".to_string()),
///     hit_path: vec![1, 7],
///     mode: "live".to_string(),
/// };
/// assert_eq!(out.response, "Handled");
/// ```
#[derive(Debug, Clone, Serialize, JsonSchema)]
pub struct InvokeA11yActionOutput {
    /// Semantic action that was dispatched (echoed for correlation).
    pub action: String,
    /// Response produced by the app's dispatch path (`Handled`, `Ignored`,
    /// `RequestFocus`, ...).
    pub response: String,
    /// Resolved target node id — the focused node when `node_id` was
    /// omitted.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub target: Option<String>,
    /// Dispatch path of widget ids, root-first, when reported.
    #[serde(default)]
    #[schemars(schema_with = "crate::types::schema_strip::u64v")]
    pub hit_path: Vec<u64>,
    /// Server mode that produced the answer (`live` on success).
    pub mode: String,
}

/// `martensite_invoke_accessibility_action`: dispatch a `SemanticAction` to
/// a widget — the same event assistive technology emits.
pub struct InvokeA11yActionTool;

impl ToolBase for InvokeA11yActionTool {
    type Parameter = InvokeA11yActionParams;
    type Output = InvokeA11yActionOutput;
    type Error = McpError;

    fn name() -> Cow<'static, str> {
        "martensite_invoke_accessibility_action".into()
    }

    fn description() -> Option<Cow<'static, str>> {
        Some(
            "Invoke an accessibility SemanticAction (click, focus, set_value, \
             scroll_to_point, ...) on a widget node — or the focused node \
             when `node_id` is omitted — as assistive technology would; \
             returns the dispatch response and hit path. Requires a live \
             `cargo martensite dev` session."
                .into(),
        )
    }

    fn annotations() -> Option<ToolAnnotations> {
        crate::tools::mutation_annotations()
    }
}

impl AsyncTool<MartensiteMcp> for InvokeA11yActionTool {
    async fn invoke(
        service: &MartensiteMcp,
        param: Self::Parameter,
    ) -> Result<Self::Output, Self::Error> {
        param.validate()?;

        // Live-only per spec: `live_call` produces the offline IPC error.
        let resp = service.live_call(
            "a11y_action",
            json!({
                "node_id": param.node_id.as_deref(),
                "action": param.action.as_str(),
                "value": param.value.as_deref(),
                "point": param.point,
            }),
        )?;
        Ok(a11y_action_output(&param.action, &resp, mode_str(service)))
    }
}

/// Builds the typed `a11y_action` result from the wire payload: `response`
/// carries the router verdict, `target` the resolved node id, and
/// `hit_path` the dispatch lineage.
fn a11y_action_output(
    action: &str,
    resp: &serde_json::Value,
    mode: &str,
) -> InvokeA11yActionOutput {
    let response = match resp.get("response") {
        Some(serde_json::Value::String(s)) => s.clone(),
        Some(other) => other.to_string(),
        None => resp
            .as_str()
            .map(str::to_string)
            .unwrap_or_else(|| resp.to_string()),
    };
    let target = resp.get("target").and_then(|v| match v {
        serde_json::Value::String(s) => Some(s.clone()),
        serde_json::Value::Number(n) => Some(n.to_string()),
        _ => None,
    });
    let hit_path = resp
        .get("hit_path")
        .and_then(serde_json::Value::as_array)
        .map(|arr| arr.iter().filter_map(serde_json::Value::as_u64).collect())
        .unwrap_or_default();
    InvokeA11yActionOutput {
        action: action.to_string(),
        response,
        target,
        hit_path,
        mode: mode.to_string(),
    }
}

/// Current server mode as a stable wire string (`live` / `offline`).
fn mode_str(service: &MartensiteMcp) -> &'static str {
    match service.mode() {
        ServerMode::Live { .. } => "live",
        ServerMode::Offline => "offline",
    }
}

/// Every `accesskit::Role` variant (AccessKit 0.25) in normalized form —
/// lowercase ASCII alphanumeric, separators stripped — used for
/// `role_filter` plausibility validation.
const KNOWN_ROLES: &[&str] = &[
    "unknown",
    "textrun",
    "cell",
    "label",
    "image",
    "link",
    "row",
    "listitem",
    "listmarker",
    "treeitem",
    "listboxoption",
    "menuitem",
    "menulistoption",
    "paragraph",
    "genericcontainer",
    "checkbox",
    "radiobutton",
    "textinput",
    "button",
    "defaultbutton",
    "pane",
    "rowheader",
    "columnheader",
    "rowgroup",
    "list",
    "table",
    "layouttablecell",
    "layouttablerow",
    "layouttable",
    "switch",
    "menu",
    "multilinetextinput",
    "searchinput",
    "dateinput",
    "datetimeinput",
    "weekinput",
    "monthinput",
    "timeinput",
    "emailinput",
    "numberinput",
    "passwordinput",
    "phonenumberinput",
    "urlinput",
    "abbr",
    "alert",
    "alertdialog",
    "application",
    "article",
    "audio",
    "banner",
    "blockquote",
    "canvas",
    "caption",
    "caret",
    "code",
    "colorwell",
    "combobox",
    "editablecombobox",
    "complementary",
    "comment",
    "contentdeletion",
    "contentinsertion",
    "contentinfo",
    "definition",
    "descriptionlist",
    "details",
    "dialog",
    "disclosuretriangle",
    "document",
    "embeddedobject",
    "emphasis",
    "feed",
    "figurecaption",
    "figure",
    "footer",
    "form",
    "grid",
    "gridcell",
    "group",
    "header",
    "heading",
    "iframe",
    "iframepresentational",
    "imecandidate",
    "keyboard",
    "legend",
    "linebreak",
    "listbox",
    "log",
    "main",
    "mark",
    "marquee",
    "math",
    "menubar",
    "menuitemcheckbox",
    "menuitemradio",
    "menulistpopup",
    "meter",
    "navigation",
    "note",
    "pluginobject",
    "progressindicator",
    "radiogroup",
    "region",
    "rootwebarea",
    "ruby",
    "rubyannotation",
    "scrollbar",
    "scrollview",
    "search",
    "section",
    "sectionfooter",
    "sectionheader",
    "slider",
    "spinbutton",
    "splitter",
    "status",
    "strong",
    "suggestion",
    "svgroot",
    "tab",
    "tablist",
    "tabpanel",
    "term",
    "time",
    "timer",
    "titlebar",
    "toolbar",
    "tooltip",
    "tree",
    "treegrid",
    "video",
    "webview",
    "window",
    "pdfactionablehighlight",
    "pdfroot",
    "graphicsdocument",
    "graphicsobject",
    "graphicssymbol",
    "docabstract",
    "docacknowledgements",
    "docafterword",
    "docappendix",
    "docbacklink",
    "docbiblioentry",
    "docbibliography",
    "docbiblioref",
    "docchapter",
    "doccolophon",
    "docconclusion",
    "doccover",
    "doccredit",
    "doccredits",
    "docdedication",
    "docendnote",
    "docendnotes",
    "docepigraph",
    "docepilogue",
    "docerrata",
    "docexample",
    "docfootnote",
    "docforeword",
    "docglossary",
    "docglossref",
    "docindex",
    "docintroduction",
    "docnoteref",
    "docnotice",
    "docpagebreak",
    "docpagefooter",
    "docpageheader",
    "docpagelist",
    "docpart",
    "docpreface",
    "docprologue",
    "docpullquote",
    "docqna",
    "docsubtitle",
    "doctip",
    "doctoc",
    "listgrid",
    "terminal",
];

#[cfg(test)]
mod tests {
    use super::*;
    use crate::server::McpServerOptions;

    fn node(id: &str, role: &str, children: Vec<A11yNodeDescriptor>) -> A11yNodeDescriptor {
        A11yNodeDescriptor {
            id: id.to_string(),
            role: role.to_string(),
            children,
            ..A11yNodeDescriptor::default()
        }
    }

    #[test]
    fn role_normalization_matches_encodings() {
        assert_eq!(normalize_role("ListBox"), "listbox");
        assert_eq!(normalize_role("list_box"), "listbox");
        assert_eq!(normalize_role("text-input"), "textinput");
        assert!(is_known_role("button"));
        assert!(is_known_role("AlertDialog"));
        assert!(is_known_role("doc-chapter"));
        assert!(!is_known_role("buton"));
        assert!(!is_known_role("'; DROP TABLE"));
        assert!(!is_known_role(""));
    }

    #[test]
    fn validate_rejects_implausible_role() {
        let p = InspectA11yTreeParams {
            role_filter: Some("not-a-real-role!!".to_string()),
            ..InspectA11yTreeParams::default()
        };
        assert!(matches!(p.validate(), Err(McpError::InvalidParameter(_))));

        let p = InspectA11yTreeParams {
            root_id: Some("  ".to_string()),
            ..InspectA11yTreeParams::default()
        };
        assert!(matches!(p.validate(), Err(McpError::InvalidParameter(_))));
    }

    #[test]
    fn prune_ignored_hoists_children() {
        let mut root = node(
            "root",
            "genericcontainer",
            vec![
                node(
                    "wrap",
                    "genericcontainer",
                    vec![node("b", "button", vec![])],
                ),
                node("leaf", "label", vec![]),
            ],
        );
        root.children[0].ignored = true;
        prune_ignored(&mut root);
        // The ignored wrapper is gone; its button child is hoisted.
        assert_eq!(root.children.len(), 2);
        assert_eq!(root.children[0].id, "b");
        assert_eq!(root.children[1].id, "leaf");
    }

    #[test]
    fn role_filter_preserves_ancestor_chain() {
        let mut root = node(
            "root",
            "genericcontainer",
            vec![
                node(
                    "nav",
                    "navigation",
                    vec![node("logo", "image", vec![]), node("go", "button", vec![])],
                ),
                node("aside", "complementary", vec![]),
            ],
        );
        assert!(filter_by_role(&mut root, "button"));
        // Root kept (ancestor), nav kept (ancestor), logo pruned, aside pruned.
        assert_eq!(root.children.len(), 1);
        assert_eq!(root.children[0].id, "nav");
        assert_eq!(root.children[0].children.len(), 1);
        assert_eq!(root.children[0].children[0].id, "go");
    }

    #[test]
    fn role_filter_empties_without_match() {
        let mut root = node("root", "genericcontainer", vec![node("t", "label", vec![])]);
        assert!(!filter_by_role(&mut root, "dialog"));
    }

    #[test]
    fn parses_envelope_bare_node_and_null() {
        // Envelope with findings.
        let (root, findings) = parse_a11y_response(serde_json::json!({
            "root": {"id": "1", "role": "button"},
            "apg_findings": ["1 (button): missing accessible name"],
        }))
        .expect("envelope");
        assert_eq!(root.expect("root").id, "1");
        assert_eq!(findings.len(), 1);

        // Bare node object.
        let (root, findings) =
            parse_a11y_response(serde_json::json!({"id": "9", "role": "list"})).expect("bare node");
        assert_eq!(root.expect("root").role, "list");
        assert!(findings.is_empty());

        // Null anchor.
        let (root, _) = parse_a11y_response(serde_json::Value::Null).expect("null");
        assert!(root.is_none());
    }

    #[test]
    fn aggregates_per_node_findings() {
        let mut root = node(
            "root",
            "genericcontainer",
            vec![node("b", "button", vec![])],
        );
        root.apg_findings.push("unlabelled group".to_string());
        root.children[0]
            .apg_findings
            .push("missing accessible name".to_string());
        let findings = collect_apg_findings(&root);
        assert_eq!(findings.len(), 2);
        assert!(findings[1].contains("b (button)"), "{:?}", findings);
    }

    #[tokio::test]
    async fn inspect_a11y_tree_requires_live_session() {
        // Without `cargo martensite dev` the tool must fail with the
        // offline IPC hint rather than fabricating a semantic tree (D9).
        let server = MartensiteMcp::new(McpServerOptions::offline());
        let res = InspectA11yTreeTool::invoke(&server, InspectA11yTreeParams::default()).await;
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

    #[test]
    fn a11y_action_validate_rejects_bad_input() {
        // Unknown action → InvalidAction.
        let p = InvokeA11yActionParams {
            action: "teleport".to_string(),
            ..InvokeA11yActionParams::default()
        };
        assert!(matches!(p.validate(), Err(McpError::InvalidAction(_))));

        // Blank node_id → InvalidParameter.
        let p = InvokeA11yActionParams {
            node_id: Some("  ".to_string()),
            action: "click".to_string(),
            ..InvokeA11yActionParams::default()
        };
        assert!(matches!(p.validate(), Err(McpError::InvalidParameter(_))));

        // `set_value` without a payload.
        let p = InvokeA11yActionParams {
            action: "set_value".to_string(),
            ..InvokeA11yActionParams::default()
        };
        assert!(matches!(p.validate(), Err(McpError::InvalidParameter(_))));

        // `scroll_to_point` without coordinates.
        let p = InvokeA11yActionParams {
            action: "scroll_to_point".to_string(),
            ..InvokeA11yActionParams::default()
        };
        assert!(matches!(p.validate(), Err(McpError::InvalidParameter(_))));

        // Fully populated forms pass.
        let p = InvokeA11yActionParams {
            node_id: Some("7".to_string()),
            action: "set_value".to_string(),
            value: Some("42".to_string()),
            ..InvokeA11yActionParams::default()
        };
        assert!(p.validate().is_ok());
        let p = InvokeA11yActionParams {
            action: "scroll_to_point".to_string(),
            point: Some([10.0, 20.0]),
            ..InvokeA11yActionParams::default()
        };
        assert!(p.validate().is_ok());
    }

    #[test]
    fn a11y_action_output_parses_wire_payload() {
        let out = a11y_action_output(
            "click",
            &serde_json::json!({
                "response": "Handled",
                "target": 7,
                "hit_path": [1, 7],
            }),
            "live",
        );
        assert_eq!(out.response, "Handled");
        assert_eq!(out.target.as_deref(), Some("7"));
        assert_eq!(out.hit_path, vec![1, 7]);

        // Bare string response.
        let out = a11y_action_output("focus", &serde_json::json!("Ignored"), "live");
        assert_eq!(out.response, "Ignored");
        assert!(out.target.is_none());
    }

    #[tokio::test]
    async fn invoke_a11y_action_requires_live_session() {
        let server = MartensiteMcp::new(McpServerOptions::offline());
        let res = InvokeA11yActionTool::invoke(
            &server,
            InvokeA11yActionParams {
                action: "click".to_string(),
                ..InvokeA11yActionParams::default()
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

    #[test]
    fn tool_annotations_classify_a11y_tools() {
        let ro = InspectA11yTreeTool::annotations().expect("annotations");
        assert_eq!(ro.read_only_hint, Some(true));

        let mut_ann = InvokeA11yActionTool::annotations().expect("annotations");
        assert_eq!(mut_ann.read_only_hint, Some(false));
        assert_eq!(mut_ann.destructive_hint, Some(true));
        assert_eq!(mut_ann.idempotent_hint, Some(false));
    }
}
