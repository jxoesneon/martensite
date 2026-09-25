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

/// Composite pre-commit check command executing format, clippy, and design lint.
pub mod check;
/// Command-line interface parsing and dispatch.
pub mod cli;
/// Environment diagnosis and readiness checks.
pub mod doctor;
/// Hot-reload coordination framework (file watching, build triggering, timing).
pub mod hot_reload;
/// Project scaffolding and embedded templates.
pub mod scaffold;

pub use check::{run_check, CheckError, CheckOptions, CheckReport, LegResult};
pub use cli::{parse_args, run_command, CliError, Command, DEFAULT_DEV_PORT};
pub use doctor::{
    check_accessibility, check_design_lint, check_gpu, check_text_and_fonts, check_toolchain,
    check_version_parity, run_doctor, CheckResult, CheckStatus, DoctorOptions, DoctorReport,
    WORKSPACE_MSRV,
};
pub use hot_reload::{
    build_guest_crate, is_within_reload_budget, reload_cycle, versioned_library_path, FileWatcher,
    HotReloadConfig, HotReloadState, ReloadError, RELOAD_BUDGET_MS,
};
pub use scaffold::{
    get_template_files, init_project, render_template, scaffold_project, validate_project_name,
    FileInitStatus, InitOptions, ScaffoldError, ScaffoldOptions, TemplateFile, TemplateKind,
    DEFAULT_MARTENSITE_VERSION,
};
