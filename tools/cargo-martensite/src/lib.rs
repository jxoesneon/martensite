//! Library facade for the `cargo-martensite` developer toolchain.
//!
//! This crate provides the command-line parsing and hot-reload coordination
//! logic used by the `cargo martensite` subcommand. The actual dynamic-library
//! loading (`dlopen`/`dlsym`) is intentionally **not** performed here: it
//! requires `unsafe` code and is delegated to the lightweight host binary that
//! links the guest cdylib. This crate is `#![forbid(unsafe_code)]`, so it only
//! coordinates file watching, build triggering, versioned-path generation, and
//! reload-timing measurement.
#![forbid(unsafe_code)]

/// Command-line interface parsing and dispatch.
pub mod cli;
/// Hot-reload coordination framework (file watching, build triggering, timing).
pub mod hot_reload;

pub use cli::{parse_args, run_command, CliError, Command, DEFAULT_DEV_PORT};
pub use hot_reload::{
    build_guest_crate, is_within_reload_budget, reload_cycle, versioned_library_path, FileWatcher,
    HotReloadConfig, HotReloadState, ReloadError, RELOAD_BUDGET_MS,
};
