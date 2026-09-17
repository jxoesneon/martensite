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

mod app;
mod headless;
mod model;
mod panels;
mod text;

fn main() {
    if std::env::args().any(|a| a == "--headless") {
        headless::run();
        return;
    }
    if let Err(err) = app::run() {
        eprintln!("industrial_dashboard: {err}");
        std::process::exit(1);
    }
}
