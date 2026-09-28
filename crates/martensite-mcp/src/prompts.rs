//! Standard MCP prompt templates (spec §5).
//!
//! Pre-packaged workflows guiding agents through multi-step UI engineering:
//! `audit_screen`, `fix_overflow`, `refactor_reactive_widget`, and
//! `implement_accessible_pattern`. Each template expands into an ordered
//! [`PromptMessage`] sequence that names the concrete `martensite_*` tools to
//! invoke at every step.

use rmcp::model::{GetPromptResult, JsonObject, Prompt, PromptArgument, PromptMessage, Role};

use crate::error::McpError;
use crate::server::MartensiteMcp;

/// Prompt names exposed via `prompts/list`, in spec order.
pub const PROMPT_NAMES: &[&str] = &[
    "audit_screen",
    "fix_overflow",
    "refactor_reactive_widget",
    "implement_accessible_pattern",
];

/// Lists the bundled prompt templates.
#[must_use]
pub fn list_prompts() -> Vec<Prompt> {
    PROMPT_NAMES
        .iter()
        .map(|name| {
            Prompt::new(
                *name,
                Some(prompt_description(name)),
                Some(prompt_arguments(name)),
            )
        })
        .collect()
}

fn prompt_description(name: &str) -> String {
    match name {
        "audit_screen" => {
            "Audit the current screen against WCAG 2.2 AAA, text clipping, \
             and contrast; list prioritized remediations."
        }
        "fix_overflow" => {
            "Diagnose a reported layout overflow via the Taffy constraint \
             chain, propose minimal code edits, and verify via live tweak \
             preview."
        }
        "refactor_reactive_widget" => {
            "Inspect an imperative widget, find static-state bottlenecks, and \
             refactor to fine-grained Signal/Memo primitives."
        }
        "implement_accessible_pattern" => {
            "Validate a composite widget against the W3C ARIA APG and emit \
             the required AccessKit node bindings."
        }
        _ => "Martensite prompt template",
    }
    .to_string()
}

fn prompt_arguments(name: &str) -> Vec<PromptArgument> {
    match name {
        "audit_screen" => vec![
            prompt_arg(
                "scope",
                "WidgetId or debug_name path to scope the audit; \
                 defaults to the entire visible screen.",
                false,
            ),
            prompt_arg(
                "standard",
                "Design standard filter (`wcag`, `isa101`, `gestalt`, \
                 `hick-fitts`); defaults to all 51 rules.",
                false,
            ),
        ],
        "fix_overflow" => vec![
            prompt_arg(
                "node_id",
                "WidgetId of the overflowing node; when omitted the whole \
                 scene is scanned for active overflows.",
                false,
            ),
            prompt_arg(
                "finding_id",
                "Design-lint finding id from `martensite_lint_scene` \
                 associated with the overflow, if any.",
                false,
            ),
        ],
        "refactor_reactive_widget" => vec![
            prompt_arg(
                "node_id",
                "WidgetId or debug_name path of the imperative widget to \
                 refactor.",
                true,
            ),
            prompt_arg(
                "source_path",
                "Workspace-relative source file implementing the widget, \
                 used to scope the edit.",
                false,
            ),
        ],
        "implement_accessible_pattern" => vec![prompt_arg(
            "widget_path",
            "Workspace-relative source path or debug_name of the \
                 composite widget (tabs, combobox, dialog, …) to validate.",
            true,
        )],
        _ => Vec::new(),
    }
}

fn prompt_arg(name: &str, description: &str, required: bool) -> PromptArgument {
    PromptArgument::new(name)
        .with_description(description)
        .with_required(required)
}

/// Extracts a string argument from a `prompts/get` arguments object.
fn prompt_arg_value<'a>(arguments: Option<&'a JsonObject>, key: &str) -> Option<&'a str> {
    arguments
        .and_then(|map| map.get(key))
        .and_then(|v| v.as_str())
}

/// Resolves a prompt by name into a `GetPromptResult` message sequence.
///
/// # Errors
///
/// * [`McpError::ResourceNotFound`] for unknown prompt names.
/// * [`McpError::InvalidParameter`] when a required prompt argument is
///   absent or not a string.
pub fn get_prompt(
    _server: &MartensiteMcp,
    name: &str,
    arguments: Option<&JsonObject>,
) -> Result<GetPromptResult, McpError> {
    match name {
        "audit_screen" => Ok(audit_screen(arguments)),
        "fix_overflow" => Ok(fix_overflow(arguments)),
        "refactor_reactive_widget" => refactor_reactive_widget(arguments),
        "implement_accessible_pattern" => implement_accessible_pattern(arguments),
        _ => Err(McpError::ResourceNotFound(format!("prompt `{name}`"))),
    }
}

fn require_arg<'a>(
    arguments: Option<&'a JsonObject>,
    prompt: &str,
    key: &str,
) -> Result<&'a str, McpError> {
    prompt_arg_value(arguments, key).ok_or_else(|| {
        McpError::InvalidParameter(format!(
            "prompt `{prompt}` requires a string argument `{key}`"
        ))
    })
}

/// `audit_screen`: WCAG 2.2 AAA + clipping + contrast audit workflow.
fn audit_screen(arguments: Option<&JsonObject>) -> GetPromptResult {
    let scope = prompt_arg_value(arguments, "scope").unwrap_or("the visible screen root");
    let standard = prompt_arg_value(arguments, "standard").unwrap_or("all");

    GetPromptResult::new(vec![
        PromptMessage::new_text(
            Role::User,
            format!(
                "Audit {scope} against design standard `{standard}`. Work only \
                 through the `martensite_*` tools; never guess layout or \
                 accessibility state (invariant D9).\n\n\
                 Step 1 — Structure: call `martensite_inspect_tree` with \
                 `root_id` = \"{scope}\" (omit `root_id` when auditing the \
                 whole screen) and `include_internal` = true to obtain the \
                 authoritative widget hierarchy."
            ),
        ),
        PromptMessage::new_text(
            Role::User,
            format!(
                "Step 2 — Findings: call `martensite_lint_scene` with \
                 `standard` = \"{standard}\" (omit for all standards) to \
                 collect the 51-rule findings. Then call \
                 `martensite_inspect_a11y_tree` for the AccessKit semantic \
                 tree (roles, names, states) and `martensite_audit_paint` \
                 for text clipping, contrast ratios, and keyline violations."
            ),
        ),
        PromptMessage::new_text(
            Role::User,
            "Step 3 — Drill-down: for each finding, call \
             `martensite_get_node` on the reported `node_id` to gather \
             bounds, clip rects, semantic annotations, and the source span \
             so every remediation cites concrete evidence."
                .to_string(),
        ),
        PromptMessage::new_text(
            Role::User,
            "Step 4 — Remediate and report: apply only Safe fixes via \
             `martensite_apply_lint_fix` (one `finding_id` per call), then \
             re-run `martensite_lint_scene` to confirm. Produce a prioritized \
             remediation list ordered by severity → user impact, citing \
             `node_id`, rule id, and the exact suggested change for every \
             remaining finding."
                .to_string(),
        ),
    ])
    .with_description(format!(
        "WCAG 2.2 / clipping / contrast audit of {scope} (standard: {standard})"
    ))
}

/// `fix_overflow`: Taffy overflow diagnosis + live-verified minimal fix.
fn fix_overflow(arguments: Option<&JsonObject>) -> GetPromptResult {
    let node_id = prompt_arg_value(arguments, "node_id");
    let finding_id = prompt_arg_value(arguments, "finding_id");
    let target = node_id.unwrap_or("the whole scene");
    let node_step = match node_id {
        Some(id) => format!("call `martensite_explain_overflow` with `node_id` = \"{id}\""),
        None => "call `martensite_explain_overflow` without `node_id` to scan \
             the scene for active overflows, then pick the highest-magnitude \
             `offending_node`"
            .to_string(),
    };
    let finding_step = match finding_id {
        Some(id) => format!(
            "Because lint finding `{id}` is associated with this overflow, \
             prefer `martensite_apply_lint_fix` with `finding_id` = \"{id}\" \
             when the tool reports a Safe fix for it."
        ),
        None => "If the fix maps to a design-lint finding, apply it via \
             `martensite_apply_lint_fix`; otherwise edit the source directly."
            .to_string(),
    };

    GetPromptResult::new(vec![
        PromptMessage::new_text(
            Role::User,
            format!(
                "Diagnose and fix the layout overflow reported on {target}.\n\n\
                 Step 1 — Localize: {node_step}. Record `offending_node`, \
                 `overflow_axis`, `overflow_pixels`, and `clipping_ancestor`."
            ),
        ),
        PromptMessage::new_text(
            Role::User,
            "Step 2 — Constraint chain: call `martensite_diagnose_layout` on \
             the `offending_node` to obtain the inbound Taffy constraints, \
             flex parameters, and measure-closure results. Call \
             `martensite_get_node` for its source span. Identify which \
             constraint (missing `flex_shrink`, fixed width, unclamped text) \
             mathematically produces `overflow_pixels`."
                .to_string(),
        ),
        PromptMessage::new_text(
            Role::User,
            format!(
                "Step 3 — Minimal edit: choose the smallest change that \
                 removes the overflow (e.g. wrap in `ScrollView`, \
                 `flex_shrink(1.0)`, or a clamped font size). {finding_step}"
            ),
        ),
        PromptMessage::new_text(
            Role::User,
            "Step 4 — Live verification: preview the change non-destructively \
             with `martensite_set_tweak`, then confirm via \
             `martensite_explain_overflow` (expect zero `overflow_pixels`) and \
             `martensite_capture_node` on the offending subtree. When the \
             preview is correct, persist it with \
             `martensite_sync_tweaks_to_source` — first `dry_run` = true, then \
             `confirmed` = true only after the diff is reviewed."
                .to_string(),
        ),
    ])
    .with_description(format!("Overflow diagnosis and verified fix for {target}"))
}

/// `refactor_reactive_widget`: imperative → fine-grained Signal/Memo refactor.
fn refactor_reactive_widget(arguments: Option<&JsonObject>) -> Result<GetPromptResult, McpError> {
    let node_id = require_arg(arguments, "refactor_reactive_widget", "node_id")?;
    let source_path = prompt_arg_value(arguments, "source_path")
        .map(|p| format!(" Limit source edits to `{p}`."))
        .unwrap_or_default();

    Ok(GetPromptResult::new(vec![
        PromptMessage::new_text(
            Role::User,
            format!(
                "Refactor the imperative widget `{node_id}` into fine-grained \
                 `Signal`/`Memo` primitives.{source_path}\n\n\
                 Step 1 — Baseline: call `martensite_get_node` with \
                 `node_id` = \"{node_id}\" to capture its kind, bounds, \
                 semantic annotations, and source span, then call \
                 `martensite_inspect_signals` with `node_id` = \"{node_id}\" \
                 to enumerate the signals it reads, writes, and broadcasts."
            ),
        ),
        PromptMessage::new_text(
            Role::User,
            "Step 2 — Bottleneck analysis: call \
             `martensite_inspect_signals` again with `only_dirty` = true and \
             inspect the subscriber sets and dirty bitsets. Flag state that \
             is coarse-grained (one signal invalidating unrelated \
             subscribers), stale manual invalidation, and imperative \
             `set_dirty` calls that a `Memo` should replace."
                .to_string(),
        ),
        PromptMessage::new_text(
            Role::User,
            "Step 3 — Refactor: rewrite the widget so each independently \
             changing input is a `Signal` and each derived value is a \
             `Memo`; remove manual dirty propagation. Keep the public \
             widget API and visual output identical. Use \
             `martensite_list_tweaks` / `martensite_set_tweak` to \
             preserve any tweakable values across the refactor."
                .to_string(),
        ),
        PromptMessage::new_text(
            Role::User,
            format!(
                "Step 4 — Verify equivalence: exercise the refactored widget \
                 with `martensite_dispatch_event` (pointer/keyboard), \
                 `martensite_trigger_signal`, and `martensite_step_timemachine` \
                 replay; diff `martensite_get_event_ledger` and \
                 `martensite_capture_node` output for `{node_id}` against the \
                 Step-1 baseline."
            ),
        ),
    ])
    .with_description(format!("Signal/Memo refactor plan for `{node_id}`")))
}

/// `implement_accessible_pattern`: ARIA APG validation + AccessKit bindings.
fn implement_accessible_pattern(
    arguments: Option<&JsonObject>,
) -> Result<GetPromptResult, McpError> {
    let widget_path = require_arg(arguments, "implement_accessible_pattern", "widget_path")?;

    Ok(GetPromptResult::new(vec![
        PromptMessage::new_text(
            Role::User,
            format!(
                "Validate the composite widget at `{widget_path}` against the \
                 W3C ARIA Authoring Practices Guide and emit the AccessKit \
                 node bindings it is missing.\n\n\
                 Step 1 — Locate and inspect: find the widget's runtime node \
                 with `martensite_inspect_tree` (`filter_marker` or \
                 `debug_name` matching `{widget_path}`), then call \
                 `martensite_inspect_a11y_tree` with `root_id` scoped to it \
                 to capture current roles, names, states, and focus order."
            ),
        ),
        PromptMessage::new_text(
            Role::User,
            "Step 2 — APG gap analysis: call `martensite_lint_scene` with \
             `standard` = \"wcag\" and `node_path_filter` targeting the \
             widget. Compare the AccessKit tree against the APG pattern for \
             its role (e.g. `tablist` keyboard contract, `combobox` \
             `aria-expanded`/`aria-activedescendant`, `dialog` focus trap). \
             List every missing role, name, state, property, and keyboard \
             interaction."
                .to_string(),
        ),
        PromptMessage::new_text(
            Role::User,
            format!(
                "Step 3 — Emit bindings: edit `{widget_path}` so every \
                 semantic node publishes the required AccessKit properties \
                 (role, accessible name, value ranges, expanded/selected \
                 states) and the widget handles the APG keyboard contract. \
                 For a greenfield widget, bootstrap with \
                 `martensite_scaffold_widget` and integrate the bindings there."
            ),
        ),
        PromptMessage::new_text(
            Role::User,
            "Step 4 — Verify: re-run `martensite_inspect_a11y_tree` and \
             `martensite_lint_scene` (`standard` = \"wcag\") expecting zero \
             findings; drive the keyboard contract end-to-end with \
             `martensite_dispatch_event` (Tab, arrows, Enter, Escape) and \
             confirm focus and state transitions in \
             `martensite_get_event_ledger`."
                .to_string(),
        ),
    ])
    .with_description(format!("APG conformance workflow for `{widget_path}`")))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn args_with(pairs: &[(&str, &str)]) -> JsonObject {
        pairs
            .iter()
            .map(|(k, v)| ((*k).to_string(), serde_json::Value::from(*v)))
            .collect()
    }

    #[test]
    fn list_prompts_advertises_all_templates() {
        let prompts = list_prompts();
        let names: Vec<&str> = prompts.iter().map(|p| p.name.as_str()).collect();
        assert_eq!(names, PROMPT_NAMES);
        for p in &prompts {
            assert!(p.description.is_some());
            assert!(p.arguments.is_some());
        }
    }

    #[test]
    fn get_prompt_rejects_unknown_name() {
        let server = MartensiteMcp::new(crate::server::McpServerOptions::offline());
        let err = get_prompt(&server, "nope", None).unwrap_err();
        assert!(matches!(err, McpError::ResourceNotFound(_)));
    }

    #[test]
    fn audit_screen_uses_defaults() {
        let server = MartensiteMcp::new(crate::server::McpServerOptions::offline());
        let res = get_prompt(&server, "audit_screen", None).unwrap();
        assert_eq!(res.messages.len(), 4);
    }

    #[test]
    fn refactor_requires_node_id() {
        let server = MartensiteMcp::new(crate::server::McpServerOptions::offline());
        let err = get_prompt(&server, "refactor_reactive_widget", None).unwrap_err();
        assert!(matches!(err, McpError::InvalidParameter(_)));

        let args = args_with(&[("node_id", "node-7")]);
        let res = get_prompt(&server, "refactor_reactive_widget", Some(&args)).unwrap();
        assert!(res.messages.len() >= 3);
    }

    #[test]
    fn accessible_pattern_requires_widget_path() {
        let server = MartensiteMcp::new(crate::server::McpServerOptions::offline());
        let err = get_prompt(&server, "implement_accessible_pattern", None).unwrap_err();
        assert!(matches!(err, McpError::InvalidParameter(_)));
    }
}
