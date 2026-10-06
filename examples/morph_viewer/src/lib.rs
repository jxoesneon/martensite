//! morph_viewer — the native icon-morph showcase.
//!
//! Streams the free stroke-icon packs (`fetch_icons` bin: lucide ISC,
//! tabler/feather/heroicons/iconoir MIT) into [`MorphIcon`](martensite::widgets::MorphIcon) widgets —
//! a hero card morphing the last selection beside a living grid that
//! ripples through the pack on per-cell timers.
//!
//! `MARTENSITE_DEV_CHANNEL=1` serves the dev socket so
//! `cargo martensite mcp` (or any MCP client) can list/trigger the
//! `pack`, `page`, `paused`, and `select` signals and capture frames
//! live. `--live-headless` runs the same tree with no window or GPU.

// Example crate — the lib exists so the bins share modules.
#![allow(missing_docs)]

pub mod app;
pub mod icons;
pub mod viewer;

/// `src/main.rs` is a thin shell over this.
pub fn run_cli() {
    if std::env::args().any(|a| a == "--live-headless") {
        if let Err(err) = app::run_live_headless() {
            eprintln!("morph_viewer: {err}");
            std::process::exit(1);
        }
        return;
    }
    if let Err(err) = app::run() {
        eprintln!("morph_viewer: {err}");
        std::process::exit(1);
    }
}
