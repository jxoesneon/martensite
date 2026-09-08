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
            std::process::exit(2);
        }
    };

    if let Err(err) = run_command(cmd) {
        eprintln!("cargo-martensite: {err}");
        std::process::exit(1);
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
        build_guest_crate, is_within_reload_budget, reload_cycle, versioned_library_path, CliError,
        Command, FileWatcher, HotReloadConfig, HotReloadState, ReloadError, DEFAULT_DEV_PORT,
        RELOAD_BUDGET_MS,
    };

    #[test]
    fn public_api_compiles() {
        // Touch each re-exported item to ensure the facade stays in sync.
        let _ = DEFAULT_DEV_PORT;
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
    }
}
