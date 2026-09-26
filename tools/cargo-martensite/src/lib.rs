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
/// Dev channel IPC client, server, and wire protocol (ADR-0038).
pub mod dev_channel;
/// Environment diagnosis and readiness checks.
pub mod doctor;
/// Hot-reload coordination framework (file watching, build triggering, timing).
pub mod hot_reload;
/// Headless widget inspector attached to running dev app.
pub mod inspect;
/// Design standard linting in offline and dev-channel attach modes.
pub mod lint;
/// Project scaffolding and embedded templates.
pub mod scaffold;
/// Live property tweaks inspection and source patch application (W5).
pub mod tweak;
/// Cryptographically signed updates and self-update command (v0.19.0 §4.3).
pub mod update;

pub use check::{run_check, CheckError, CheckOptions, CheckReport, LegResult};
pub use cli::{parse_args, run_command, CliError, Command, DEFAULT_DEV_PORT};
#[cfg(unix)]
pub use dev_channel::DevServer;
pub use dev_channel::{
    discover_dev_sessions, discover_socket, find_dev_socket, DevChannelError, DevClient, DevError,
    DevRequest, DevResponse, InspectSelectData, LayoutStep, TreeSnapshotData, WireTreeNode,
    MARTENSITE_VERSION, PROTOCOL_VERSION,
};
pub use doctor::{
    check_accessibility, check_design_lint, check_gpu, check_text_and_fonts, check_toolchain,
    check_version_parity, run_doctor, CheckResult, CheckStatus, DoctorOptions, DoctorReport,
    WORKSPACE_MSRV,
};
pub use hot_reload::{
    build_guest_crate, is_within_reload_budget, reload_cycle, versioned_library_path, FileWatcher,
    HotReloadConfig, HotReloadState, ReloadError, RELOAD_BUDGET_MS,
};
pub use inspect::{run_inspect, InspectError, InspectOptions};
pub use lint::{load_scene_from_file, run_lint, LintError, LintOptions, LintSummary, OutputFormat};
pub use scaffold::{
    get_template_files, init_project, render_template, scaffold_project, validate_project_name,
    FileInitStatus, InitOptions, ScaffoldError, ScaffoldOptions, TemplateFile, TemplateKind,
    DEFAULT_MARTENSITE_VERSION,
};
pub use tweak::{
    apply_patches, parse_patch_line, parse_patches, run_tweak, ParsedPatch, PatchResult,
    TweakAction, TweakError, TweakOptions, TweakReport,
};
pub use update::{
    check_min_version, compare_versions, current_target, fetch_url_or_path, generate_keypair,
    keypair_from_seed, replace_executable, resolve_public_key, run_self_update, sign_asset,
    sign_manifest, verify_asset, verify_manifest, ReleaseAsset, SelfUpdateOptions, UpdateError,
    UpdateManifest, VersionStatus, CURRENT_TARGET, DEFAULT_PUBLIC_KEY_HEX, OFFICIAL_RELEASE_SEED,
};
