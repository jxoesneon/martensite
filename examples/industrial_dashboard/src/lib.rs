//! Industrial Workstation — Martensite's flagship dogfooding example.
//!
//! Two modes over one shared model:
//!
//! - **Windowed** (default): the full production assembly — winit +
//!   `RenderOrchestrator`, four real `Widget` panels over the blessed
//!   models (virtualized 1M-row `DataTable`, `Signal`-driven `Chart`,
//!   `CodeEditor` with live highlighting, `MediaView` over a mock NV12
//!   surface), `DockTree` geometry, `FocusManager` traversal, a live
//!   AccessKit tree, and the advisory paint-compliance audit running
//!   against its own output.
//! - **`--headless`**: the original v0.18.0 CI composition — every
//!   subsystem exercised through model APIs with no display server,
//!   printing a verification report. Kept byte-for-byte behavior so the
//!   two modes can be diffed.
//!
//! `F1`–`F16` API-friction notes live in `headless.rs`; `F17`–`F19`
//! (windowed-path findings) live in `panels.rs`.

// Example crate — the lib exists so sibling bins share modules, not
// to publish an API; per-item docs stay where they were written.
#![allow(missing_docs)]

pub mod app;
pub mod domain;
pub mod headless;
pub mod lint_sweep;
pub mod media_stream;
pub mod menu;
pub mod model;
pub mod overlays;
pub mod panels;
// Verified widget constructors kept as inventory for the separate
// widget catalog — the operational dashboard mounts widgets through
// `zones/` (model-bound), not the catalog's card format.
pub mod showcase;
pub mod statusbar;
pub mod subwindow;
pub mod text;
pub mod toolbar;
pub mod zone;
pub mod zones;

/// The windowed/headless entry — `src/main.rs` is a thin shell over
/// this so sibling bins (`design-lint`) can share the crate.
pub fn run_cli() {
    if std::env::args().any(|a| a == "--headless") {
        headless::run();
        return;
    }
    // `--theme <dark|light|system>` — boots settled into the mode,
    // overriding the persisted preference (which restores when the
    // flag is absent; the store falls back to dark). Verification
    // needs a non-animated start so the paint audit measures final
    // colors, not transition frames.
    let theme = {
        let mut args = std::env::args().skip_while(|a| a != "--theme").skip(1);
        match args.next().as_deref() {
            Some("light") => Some(app::ThemeChoice::Light),
            Some("system") => Some(app::ThemeChoice::System),
            Some("dark") => Some(app::ThemeChoice::Dark),
            _ => None,
        }
    };
    // `--audit-locale` — opt the paint audit into the `MissingLocale`
    // lint (user-visible strings without a shipped FTL translation).
    let audit_locale = std::env::args().any(|a| a == "--audit-locale");
    if let Err(err) = app::run(theme, audit_locale) {
        eprintln!("industrial_dashboard: {err}");
        std::process::exit(1);
    }
}
