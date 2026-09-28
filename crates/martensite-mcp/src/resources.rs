//! MCP dynamic resource provider (spec §4).
//!
//! Exposes scoped, low-overhead `martensite://` resources backed by the live
//! dev channel or offline context. Full-arena dumps are deliberately replaced
//! by summary + scoped URIs (council efficiency requirement).

use rmcp::model::{ReadResourceResult, Resource, ResourceContents, ResourceTemplate};
use serde_json::{json, Value};

use martensite_design_lint::{LintConfig, Severity};

use crate::error::McpError;
use crate::server::MartensiteMcp;

/// Canonical static resource URIs advertised via `resources/list`.
pub const RESOURCE_URIS: &[&str] = &[
    "martensite://app/tree/summary",
    "martensite://app/a11y",
    "martensite://app/lint",
    "martensite://app/signals",
    "martensite://app/events/ledger",
    "martensite://app/tweaks",
    "martensite://theme/tokens",
];

/// URI templates advertised via `resources/templates/list`.
pub const RESOURCE_TEMPLATES: &[(&str, &str)] = &[
    (
        "martensite://app/tree/{root_id}",
        "Scoped widget subtree JSON hierarchy for a target node",
    ),
    (
        "martensite://standards/{standard}",
        "Markdown documentation and citations for a design standard",
    ),
];

/// Design-standard slugs resolvable via `martensite://standards/{standard}`.
const STANDARD_SLUGS: &[&str] = &["wcag", "isa-101", "isa-18.2", "gestalt", "fitts", "tufte"];

/// Lists the canonical resources exposed by the server.
#[must_use]
pub fn list_resources() -> Vec<Resource> {
    RESOURCE_URIS
        .iter()
        .map(|uri| {
            Resource::new(*uri, uri.trim_start_matches("martensite://"))
                .with_description(resource_description(uri))
                .with_mime_type("application/json")
        })
        .collect()
}

/// Lists the parameterized resource templates.
#[must_use]
pub fn list_resource_templates() -> Vec<ResourceTemplate> {
    RESOURCE_TEMPLATES
        .iter()
        .map(|(tpl, desc)| {
            ResourceTemplate::new(*tpl, tpl.trim_start_matches("martensite://"))
                .with_description(*desc)
        })
        .collect()
}

fn resource_description(uri: &str) -> String {
    match uri {
        "martensite://app/tree/summary" => {
            "Lightweight structural summary (node count, depth, root ids)".to_string()
        }
        "martensite://app/a11y" => "Full hierarchical AccessKit semantic tree".to_string(),
        "martensite://app/lint" => "Latest design lint report across all standards".to_string(),
        "martensite://app/signals" => "Active signal registry summary and dirty bitset".to_string(),
        "martensite://app/events/ledger" => "Tail of the last 100 routed events".to_string(),
        "martensite://app/tweaks" => "Current state of all registered live tweaks".to_string(),
        "martensite://theme/tokens" => {
            "Active theme palette, Oklab colors, and token bindings".to_string()
        }
        _ => "Martensite runtime resource".to_string(),
    }
}

/// Reads a `martensite://` resource, bridging to the live dev channel or the
/// offline context.
///
/// Live-backed resources resolve through the ADR-0038 dev channel; when no
/// dev session is reachable they fail with [`McpError::ResourceNotFound`]
/// carrying an offline hint — except `martensite://app/lint`, which falls
/// back to the `--scene` dump evaluated by `martensite-design-lint`, and
/// `martensite://standards/{standard}`, which is always static.
///
/// # Errors
///
/// * [`McpError::ResourceNotFound`] when the URI matches no known resource,
///   or a live-only resource is read with no dev session attached.
/// * [`McpError::Ipc`] / [`McpError::LintEngine`] on dev-channel or lint
///   engine failures.
/// * [`McpError::Json`] on serialization failure.
pub fn read_resource(server: &MartensiteMcp, uri: &str) -> Result<ReadResourceResult, McpError> {
    match uri {
        "martensite://app/tree/summary" => tree_summary_resource(server, uri),
        "martensite://app/a11y" => live_json_resource(server, "a11y_tree", json!({}), uri),
        "martensite://app/lint" => lint_resource(server, uri),
        "martensite://app/signals" => {
            live_json_resource(server, "signals_list", json!({ "limit": 200 }), uri)
        }
        "martensite://app/events/ledger" => {
            live_json_resource(server, "event_ledger", json!({ "limit": 100 }), uri)
        }
        "martensite://app/tweaks" => live_json_resource(server, "tweaks_list", json!({}), uri),
        "martensite://theme/tokens" => live_json_resource(server, "theme_get", json!({}), uri),
        _ => {
            if let Some(root_id) = uri.strip_prefix("martensite://app/tree/") {
                return tree_subtree_resource(server, uri, root_id);
            }
            if let Some(standard) = uri.strip_prefix("martensite://standards/") {
                return standard_resource(uri, standard);
            }
            Err(McpError::ResourceNotFound(uri.to_string()))
        }
    }
}

/// Wraps a JSON payload as an `application/json` resource body.
fn json_resource(uri: &str, body: &Value) -> Result<ReadResourceResult, McpError> {
    let text = serde_json::to_string_pretty(body)?;
    Ok(ReadResourceResult::new(vec![ResourceContents::text(
        text, uri,
    )
    .with_mime_type("application/json")]))
}

/// The canonical offline-mode hint for live-only resources.
fn offline_required_error(uri: &str) -> McpError {
    McpError::ResourceNotFound(format!(
        "`{uri}` requires a live Martensite dev session; start \
         `cargo martensite dev` or pass --socket <path>"
    ))
}

/// Forwards a dev-channel method as a JSON resource; offline →
/// [`McpError::ResourceNotFound`] with the offline hint.
fn live_json_resource(
    server: &MartensiteMcp,
    method: &str,
    params: Value,
    uri: &str,
) -> Result<ReadResourceResult, McpError> {
    match server.try_live_call(method, params)? {
        Some(v) => json_resource(uri, &v),
        None => Err(offline_required_error(uri)),
    }
}

/// `martensite://app/tree/summary`: reduces `tree_snapshot` to
/// `{node_count, max_depth, root_ids}` — never a full dump.
fn tree_summary_resource(
    server: &MartensiteMcp,
    uri: &str,
) -> Result<ReadResourceResult, McpError> {
    let Some(snapshot) = server.try_live_call("tree_snapshot", json!({}))? else {
        return Err(offline_required_error(uri));
    };
    json_resource(uri, &summarize_tree(&snapshot))
}

/// Counts nodes and maximum depth over a `children`-recursive JSON tree.
fn summarize_tree(snapshot: &Value) -> Value {
    fn walk(node: &Value, depth: usize, count: &mut usize, max_depth: &mut usize) {
        *count += 1;
        *max_depth = (*max_depth).max(depth);
        if let Some(children) = node.get("children").and_then(Value::as_array) {
            for child in children {
                walk(child, depth + 1, count, max_depth);
            }
        }
    }

    // The snapshot nests the hierarchy under `root`; tolerate a bare node.
    let root = snapshot.get("root").unwrap_or(snapshot);
    let mut node_count = 0usize;
    let mut max_depth = 0usize;
    walk(root, 0, &mut node_count, &mut max_depth);

    let root_ids: Vec<Value> = root.get("id").cloned().into_iter().collect();

    json!({
        "node_count": node_count,
        "max_depth": max_depth,
        "root_ids": root_ids,
    })
}

/// `martensite://app/tree/{root_id}`: scoped subtree via `tree_snapshot`
/// parameterized by `root_id`.
fn tree_subtree_resource(
    server: &MartensiteMcp,
    uri: &str,
    root_id: &str,
) -> Result<ReadResourceResult, McpError> {
    if root_id.is_empty() || root_id.contains('/') {
        return Err(McpError::ResourceNotFound(uri.to_string()));
    }
    live_json_resource(server, "tree_snapshot", json!({ "root_id": root_id }), uri)
}

/// `martensite://app/lint`: live `lint_pull`, or the offline `--scene` dump
/// evaluated by `martensite-design-lint`.
fn lint_resource(server: &MartensiteMcp, uri: &str) -> Result<ReadResourceResult, McpError> {
    if let Some(v) = server.try_live_call("lint_pull", json!({}))? {
        return json_resource(uri, &v);
    }

    let Some(scene) = server.offline().load_scene()? else {
        return Err(McpError::ResourceNotFound(format!(
            "`{uri}` has no offline report — pass --scene <path> to evaluate \
             a scene dump, or start `cargo martensite dev` for the live report"
        )));
    };

    let config_path = server.offline().workspace_root().join("design-lint.toml");
    let config = LintConfig::from_file(&config_path)
        .map_err(|e| McpError::LintEngine(format!("`{}`: {e}", config_path.display())))?
        .unwrap_or_default();

    let report = martensite_design_lint::lint(&scene, &config);
    let findings: Vec<Value> = report
        .findings
        .iter()
        .map(|f| {
            json!({
                "rule_id": f.rule,
                "severity": f.severity.config_key(),
                "confidence": format!("{:?}", f.confidence),
                "node_path": f.path,
                "message": f.message,
                "citation": f.citation,
                "doc": f.doc,
                "has_autofix": f.fix.is_some(),
            })
        })
        .collect();

    let body = json!({
        "source": "offline_scene",
        "findings": findings,
        "summary": {
            "active": report.findings.len(),
            "suppressed": report.suppressed.len(),
            "forbid": report.count_at(Severity::Forbid),
            "error": report.count_at(Severity::Error),
            "warn": report.count_at(Severity::Warn),
            "info": report.count_at(Severity::Info),
        },
        "unused_allows": report.unused_allows,
    });
    json_resource(uri, &body)
}

/// `martensite://standards/{standard}`: static normative summary + citation.
fn standard_resource(uri: &str, standard: &str) -> Result<ReadResourceResult, McpError> {
    let body = standard_doc(standard).ok_or_else(|| {
        McpError::ResourceNotFound(format!(
            "`{uri}` — unknown standard `{standard}` (known: {})",
            STANDARD_SLUGS.join(", ")
        ))
    })?;
    Ok(ReadResourceResult::new(vec![ResourceContents::text(
        body, uri,
    )
    .with_mime_type("text/markdown")]))
}

/// Static markdown summary and citation for a design standard.
fn standard_doc(standard: &str) -> Option<&'static str> {
    match standard {
        "wcag" => Some(
            "# WCAG 2.2 — Web Content Accessibility Guidelines\n\
             \n\
             Normative perceivable/operable/understandable/robust criteria.\n\
             Rules most relevant to widget surfaces:\n\
             \n\
             - **1.4.3 Contrast (Minimum)** — text ≥ 4.5:1 (3:1 for large\n\
             \x20 text ≥ 18pt / 14pt bold).\n\
             - **1.4.11 Non-text Contrast** — UI components and graphical\n\
             \x20 objects ≥ 3:1 against adjacent colors.\n\
             - **2.5.8 Target Size (Minimum)** — pointer targets ≥ 24×24 CSS\n\
             \x20 px (AAA: 44×44).\n\
             - **4.1.2 Name, Role, Value** — every interactive element must\n\
             \x20 expose role, accessible name, and state to AT (AccessKit\n\
             \x20 nodes in Martensite).\n\
             \n\
             Citation: <https://www.w3.org/TR/WCAG22/>\n",
        ),
        "isa-101" => Some(
            "# ISA-101 — Human Machine Interfaces for Process Automation\n\
             \n\
             High-performance HMI style guide: gray-scale backgrounds, color\n\
             reserved for abnormal states and actionable data, strict alarm\n\
             color semantics (reserved hues, never repurpose alarm colors\n\
             for decoration), and situational-awareness hierarchy\n\
             (overview → unit → detail → diagnostic levels).\n\
             \n\
             Citation: <https://www.isa.org/standards-and-publications/\
             isa-standards/isa-standards-committees/isa101>\n",
        ),
        "isa-18.2" => Some(
            "# ISA-18.2 — Management of Alarm Systems\n\
             \n\
             Alarm lifecycle and presentation norms: every alarm needs a\n\
             defined operator response, severity-ranked prioritization,\n\
             distinct unacknowledged indication (flashing/bold until ACKed),\n\
             alarm flood suppression, and shelving/state transitions that\n\
             are auditable. Alarm indication must never rely on color alone.\n\
             \n\
             Citation: <https://www.isa.org/standards-and-publications/\
             isa-standards/isa-standards-committees/isa18>\n",
        ),
        "gestalt" => Some(
            "# Gestalt Principles of Perceptual Organization\n\
             \n\
             Grouping laws governing how users parse visual structure:\n\
             proximity (nearby items read as a group), similarity (shared\n\
             shape/color implies shared kind), common region (enclosure\n\
             groups stronger than spacing), uniform connectedness, closure,\n\
             and figure/ground separation. Layout spacing and panel chrome\n\
             must reinforce — not contradict — the logical grouping.\n\
             \n\
             Citation: <https://en.wikipedia.org/wiki/Principles_of_grouping>\n",
        ),
        "fitts" => Some(
            "# Fitts's Law — Predictive Model of Pointing\n\
             \n\
             MT = a + b·log₂(1 + D/W): movement time scales with distance\n\
             and inversely with target width. Consequences for widget\n\
             design: enlarge interactive hit targets beyond their visual\n\
             bounds, place frequent/destructive actions to minimize travel,\n\
             exploit screen edges/corners (infinite effective width), and\n\
             never shrink primary actions below the WCAG 24px floor.\n\
             \n\
             Citation: <https://en.wikipedia.org/wiki/Fitts%27s_law>\n\
             (Fitts, 1954 — doi:10.1037/h0055392)\n",
        ),
        "tufte" => Some(
            "# Tufte — Data-Ink Maximization\n\
             \n\
             Maximize the data-ink ratio: erase non-data ink, erase\n\
             redundant data ink, and prefer direct labeling over legends.\n\
             Chartjunk (gratuitous gridlines, 3-D effects, heavy borders,\n\
             vibrating fills) obscures the data. Small multiples outperform\n\
             decoration-dense single charts. Applies to Martensite chart and\n\
             indicator widgets: every painted element must carry data.\n\
             \n\
             Citation: Tufte, *The Visual Display of Quantitative\n\
             Information* — <https://www.edwardtufte.com/book/\
             the-visual-display-of-quantitative-information>\n",
        ),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::server::McpServerOptions;

    #[test]
    fn list_resources_covers_spec_table() {
        let uris: Vec<String> = list_resources().iter().map(|r| r.uri.clone()).collect();
        for expected in RESOURCE_URIS {
            assert!(uris.contains(&expected.to_string()));
        }
    }

    #[test]
    fn templates_cover_scoped_uris() {
        let tpls: Vec<String> = list_resource_templates()
            .iter()
            .map(|t| t.uri_template.clone())
            .collect();
        assert!(tpls.contains(&"martensite://app/tree/{root_id}".to_string()));
        assert!(tpls.contains(&"martensite://standards/{standard}".to_string()));
    }

    #[test]
    fn unknown_uri_is_not_found() {
        let server = MartensiteMcp::new(McpServerOptions::offline());
        let err = read_resource(&server, "martensite://app/nope").unwrap_err();
        assert!(matches!(err, McpError::ResourceNotFound(_)));
    }

    #[test]
    fn live_resources_error_offline_with_hint() {
        let server = MartensiteMcp::new(McpServerOptions::offline());
        // In test env no dev socket is reachable; if one were, these return Ok.
        for uri in [
            "martensite://app/tree/summary",
            "martensite://app/a11y",
            "martensite://app/signals",
            "martensite://app/events/ledger",
            "martensite://app/tweaks",
            "martensite://theme/tokens",
        ] {
            match read_resource(&server, uri) {
                Err(McpError::ResourceNotFound(msg)) => {
                    assert!(msg.contains("live Martensite dev session"));
                }
                Ok(_) => {}
                Err(e) => panic!("unexpected error for {uri}: {e}"),
            }
        }
    }

    #[test]
    fn standards_resolve_static_docs() {
        let server = MartensiteMcp::new(McpServerOptions::offline());
        for slug in STANDARD_SLUGS {
            let uri = format!("martensite://standards/{slug}");
            let res = read_resource(&server, &uri).expect(slug);
            assert_eq!(res.contents.len(), 1);
        }
        let err = read_resource(&server, "martensite://standards/bogus").unwrap_err();
        assert!(matches!(err, McpError::ResourceNotFound(_)));
    }

    #[test]
    fn tree_template_rejects_malformed_ids() {
        let server = MartensiteMcp::new(McpServerOptions::offline());
        let err = read_resource(&server, "martensite://app/tree/").unwrap_err();
        assert!(matches!(err, McpError::ResourceNotFound(_)));
        let err = read_resource(&server, "martensite://app/tree/a/b").unwrap_err();
        assert!(matches!(err, McpError::ResourceNotFound(_)));
    }

    #[test]
    fn tree_summary_reduction() {
        let snapshot = json!({
            "root": {
                "id": 1,
                "children": [
                    { "id": 2, "children": [{ "id": 4 }] },
                    { "id": 3 }
                ]
            }
        });
        let summary = summarize_tree(&snapshot);
        assert_eq!(summary["node_count"], 4);
        assert_eq!(summary["max_depth"], 2);
        assert_eq!(summary["root_ids"], json!([1]));
    }
}
