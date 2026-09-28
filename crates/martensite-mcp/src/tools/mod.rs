//! The 28 `martensite_*` MCP tools, one struct per tool implementing
//! `rmcp`'s [`ToolBase`] + [`AsyncTool`] traits.
//!
//! Each module is a self-contained work packet; [`tool_router`] merges them
//! into a single [`ToolRouter`] mounted on the server.

pub mod a11y;
pub mod events;
pub mod inspect;
pub mod layout;
pub mod lifecycle;
pub mod lint;
pub mod reactive;
pub mod runtime;
pub mod tweaks;
pub mod visual;

use rmcp::handler::server::router::tool::ToolRouter;
use rmcp::model::ToolAnnotations;

use crate::server::MartensiteMcp;

/// `ToolAnnotations` for read-only inspection/query tools: the tool does not
/// modify the running app or the host environment, is safe to repeat, and
/// only talks to the local dev session (closed world).
///
/// Returned from each tool's
/// [`ToolBase::annotations`](rmcp::handler::server::router::tool::ToolBase::annotations)
/// override.
pub(crate) fn read_only_annotations() -> Option<ToolAnnotations> {
    Some(
        ToolAnnotations::new()
            .read_only(true)
            .destructive(false)
            .idempotent(true)
            .open_world(false),
    )
}

/// `ToolAnnotations` for live-state mutation tools (event dispatch, a11y
/// actions, signal/tweak/theme writes, lint fixes, hot reload, timemachine):
/// non-read-only, destructive to prior app state, and non-idempotent.
pub(crate) fn mutation_annotations() -> Option<ToolAnnotations> {
    Some(
        ToolAnnotations::new()
            .read_only(false)
            .destructive(true)
            .idempotent(false)
            .open_world(false),
    )
}

/// `ToolAnnotations` for additive artifact/file writes that never mutate the
/// running app itself (node captures, headless renders, widget scaffolding):
/// non-read-only (they write to disk) but non-destructive.
pub(crate) fn artifact_annotations() -> Option<ToolAnnotations> {
    Some(
        ToolAnnotations::new()
            .read_only(false)
            .destructive(false)
            .idempotent(false)
            .open_world(false),
    )
}

/// Assembles the full 28-tool router (spec §3, categories A–I plus the
/// ADR-0040 loading mutation).
///
/// Order follows the spec: arena (3), layout (2), reactivity (3), a11y (2),
/// lint (3), tweaks (4), events/replay (3), visuals (2), lifecycle (4),
/// runtime diagnostics (2).
#[must_use]
pub fn tool_router() -> ToolRouter<MartensiteMcp> {
    ToolRouter::new()
        // Category A: Widget Arena & Hierarchy Inspection
        .with_async_tool::<inspect::InspectTreeTool>()
        .with_async_tool::<inspect::GetNodeTool>()
        .with_async_tool::<inspect::SetLoadingTool>()
        // Category B: Layout & Constraint Diagnostics
        .with_async_tool::<layout::DiagnoseLayoutTool>()
        .with_async_tool::<layout::ExplainOverflowTool>()
        // Category C: Reactive State & Signal DAG
        .with_async_tool::<reactive::InspectSignalsTool>()
        .with_async_tool::<reactive::TriggerSignalTool>()
        .with_async_tool::<reactive::SetSignalTool>()
        // Category D: Accessibility & Semantic Contract
        .with_async_tool::<a11y::InspectA11yTreeTool>()
        .with_async_tool::<a11y::InvokeA11yActionTool>()
        // Category E: Design Standards & Lint Enforcement
        .with_async_tool::<lint::LintSceneTool>()
        .with_async_tool::<lint::ApplyLintFixTool>()
        .with_async_tool::<lint::AuditPaintTool>()
        // Category F: Live Tweaks & Source Synchronization
        .with_async_tool::<tweaks::ListTweaksTool>()
        .with_async_tool::<tweaks::SetTweakTool>()
        .with_async_tool::<tweaks::SetThemeTool>()
        .with_async_tool::<tweaks::SyncTweaksTool>()
        // Category G: Interaction, Replay & Visual Verification
        .with_async_tool::<events::DispatchEventTool>()
        .with_async_tool::<events::EventLedgerTool>()
        .with_async_tool::<events::TimemachineTool>()
        .with_async_tool::<visual::CaptureNodeTool>()
        .with_async_tool::<visual::RenderHeadlessTool>()
        // Category H: Toolchain, Lifecycle & Scaffolding
        .with_async_tool::<lifecycle::DoctorTool>()
        .with_async_tool::<lifecycle::ReloadStatusTool>()
        .with_async_tool::<lifecycle::HotReloadTool>()
        .with_async_tool::<lifecycle::ScaffoldWidgetTool>()
        // Category I: Runtime Diagnostics
        .with_async_tool::<runtime::RuntimeErrorsTool>()
        .with_async_tool::<runtime::LogsTool>()
}
