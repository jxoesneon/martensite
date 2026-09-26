//! Developer CLI entry point for the Martensite GUI framework toolchain.
//!
//! Invoked as `cargo martensite <subcommand>`. The heavy lifting lives in the
//! [`cargo_martensite`] library crate; this binary simply collects the
//! arguments, parses them, and dispatches to [`run_command`].

#![forbid(unsafe_code)]

use cargo_martensite::cli::{parse_args, run_command};

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let cmd = match parse_args(&args) {
        Ok(cmd) => cmd,
        Err(err) => {
            eprintln!("cargo-martensite: {err}");
            eprintln!("Run `cargo martensite help` for usage.");
            std::process::exit(err.exit_code());
        }
    };

    if let Err(err) = run_command(cmd) {
        eprintln!("cargo-martensite: {err}");
        std::process::exit(err.exit_code());
    }
}

/// Smoke test verifying the CLI crate compiles and links correctly.
#[cfg(test)]
#[test]
fn cli_smoke_test() {
    // Compilation + test execution is the smoke test.
    let output = format!("Martensite CLI v{}", env!("CARGO_PKG_VERSION"));
    assert_eq!(
        output,
        format!("Martensite CLI v{}", env!("CARGO_PKG_VERSION"))
    );
}

/// The CLI surface re-exports the public API for documentation discoverability.
#[cfg(test)]
mod api_surface {
    use cargo_martensite::{
        build_guest_crate, get_template_files, init_project, is_within_reload_budget, reload_cycle,
        render_template, scaffold_project, validate_project_name, versioned_library_path, CliError,
        Command, FileInitStatus, FileWatcher, HotReloadConfig, HotReloadState, InitOptions,
        ReloadError, ScaffoldError, ScaffoldOptions, TemplateFile, TemplateKind, DEFAULT_DEV_PORT,
        DEFAULT_MARTENSITE_VERSION, RELOAD_BUDGET_MS,
    };

    #[test]
    fn public_api_compiles() {
        // Touch each re-exported item to ensure the facade stays in sync.
        let _ = DEFAULT_DEV_PORT;
        let _ = DEFAULT_MARTENSITE_VERSION;
        let _ = RELOAD_BUDGET_MS;
        let _ = CliError::MissingCommand;
        let _ = Command::Help;
        let _ = HotReloadConfig {
            guest_crate: "g".to_string(),
            output_dir: std::path::PathBuf::from("."),
            watch_paths: vec![],
            poll_interval_ms: 100,
        };
        let _ = HotReloadState::new();
        let _ = FileWatcher::new(vec![], std::time::Duration::from_millis(1));
        let _ = ReloadError::InvalidConfig("x".to_string());
        let _ = versioned_library_path(std::path::Path::new("."), "g", 1);
        let _ = is_within_reload_budget(10);
        let _ = build_guest_crate as fn(_, _) -> _;
        let _ = reload_cycle as fn(_, _) -> _;

        // Scaffold surface
        let _ = TemplateKind::App;
        let _ = TemplateFile {
            path: "p",
            content: "c",
        };
        let _ = ScaffoldOptions::new("demo");
        let _ = InitOptions::new();
        let _ = FileInitStatus::Created(std::path::PathBuf::from("p"));
        let _ = ScaffoldError::UnknownTemplate("t".to_string());
        let _ = validate_project_name as fn(_) -> _;
        let _ = render_template as fn(_, _, _) -> _;
        let _ = scaffold_project as fn(_) -> _;
        let _ = init_project as fn(_, _) -> _;
        let _ = get_template_files as fn(_) -> _;

        // Doctor surface
        let _ = cargo_martensite::WORKSPACE_MSRV;
        let _ = cargo_martensite::DoctorOptions::default();
        let _ = cargo_martensite::DoctorReport::new();
        let _ = cargo_martensite::CheckStatus::Pass;
        let _ = cargo_martensite::CheckResult::pass("t", "d");
        let _ = cargo_martensite::run_doctor as fn(_) -> _;
        let _ = cargo_martensite::check_gpu as fn() -> _;
        let _ = cargo_martensite::check_text_and_fonts as fn() -> _;
        let _ = cargo_martensite::check_accessibility as fn() -> _;

        // Check surface
        let _ = cargo_martensite::CheckOptions::default();
        let _ = cargo_martensite::CheckReport::new();
        let _ = cargo_martensite::LegResult::pass("l", "c");
        let _ = cargo_martensite::run_check as fn(_) -> _;

        // Lint surface
        let _ = cargo_martensite::LintOptions::default();
        let _ = cargo_martensite::LintSummary::default();
        let _ = cargo_martensite::OutputFormat::Text;
        let _ = cargo_martensite::run_lint as fn(_) -> _;
        let _ = cargo_martensite::load_scene_from_file as fn(_) -> _;

        // Inspect surface
        let _ = cargo_martensite::InspectOptions::default();
        let _ = cargo_martensite::run_inspect as fn(_) -> _;

        // Dev channel surface
        let _ = cargo_martensite::PROTOCOL_VERSION;
        let _ = cargo_martensite::MARTENSITE_VERSION;
        let _ = cargo_martensite::discover_socket as fn(_) -> _;
    }
}
