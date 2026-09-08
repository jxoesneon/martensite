//! Integration tests for `cargo-martensite` hot-reload and CLI APIs.

use std::time::Duration;

#[test]
fn cli_version_command_outputs_version() {
    let args = vec!["martensite".to_string(), "--version".to_string()];
    let cmd = cargo_martensite::cli::parse_args(&args).unwrap();
    assert!(matches!(cmd, cargo_martensite::cli::Command::Version));
}

#[test]
fn cli_help_command_parses() {
    let args = vec!["martensite".to_string(), "help".to_string()];
    let cmd = cargo_martensite::cli::parse_args(&args).unwrap();
    assert!(matches!(cmd, cargo_martensite::cli::Command::Help));
}

#[test]
fn cli_dev_command_parses_with_options() {
    let args = vec![
        "martensite".to_string(),
        "dev".to_string(),
        "--port".to_string(),
        "8080".to_string(),
    ];
    let cmd = cargo_martensite::cli::parse_args(&args).unwrap();
    match cmd {
        cargo_martensite::cli::Command::Dev { port, .. } => {
            assert_eq!(port, 8080);
        }
        _ => panic!("expected Dev command"),
    }
}

#[test]
fn cli_build_command_parses_release() {
    let args = vec![
        "martensite".to_string(),
        "build".to_string(),
        "--release".to_string(),
    ];
    let cmd = cargo_martensite::cli::parse_args(&args).unwrap();
    match cmd {
        cargo_martensite::cli::Command::Build { release, .. } => {
            assert!(release);
        }
        _ => panic!("expected Build command"),
    }
}

#[test]
fn cli_unknown_command_returns_error() {
    let args = vec!["martensite".to_string(), "nonexistent".to_string()];
    assert!(cargo_martensite::cli::parse_args(&args).is_err());
}

#[test]
fn versioned_library_path_is_platform_correct() {
    let dir = std::path::Path::new("/tmp/martensite-test");
    let path = cargo_martensite::hot_reload::versioned_library_path(dir, "myapp", 1);
    let path_str = path.to_string_lossy();
    assert!(
        path_str.contains("myapp"),
        "path should contain crate name: {path_str}"
    );
    assert!(
        path_str.contains('1'),
        "path should contain version: {path_str}"
    );
}

#[test]
fn is_within_reload_budget_accepts_fast_reloads() {
    assert!(cargo_martensite::hot_reload::is_within_reload_budget(100));
    assert!(cargo_martensite::hot_reload::is_within_reload_budget(349));
}

#[test]
fn is_within_reload_budget_rejects_slow_reloads() {
    assert!(!cargo_martensite::hot_reload::is_within_reload_budget(350));
    assert!(!cargo_martensite::hot_reload::is_within_reload_budget(500));
}

#[test]
fn file_watcher_detects_new_files() {
    let temp = tempfile::tempdir().unwrap();
    let watch_path = temp.path().join("watch_test.txt");
    std::fs::write(&watch_path, "initial").unwrap();

    let mut watcher = cargo_martensite::hot_reload::FileWatcher::new(
        vec![watch_path.clone()],
        Duration::from_millis(1),
    );

    // First check establishes baseline — no changes.
    let changes = watcher.check_for_changes();
    assert!(changes.is_empty(), "first check should show no changes");

    // Modify the file.
    std::thread::sleep(Duration::from_millis(10));
    std::fs::write(&watch_path, "modified").unwrap();

    // Second check should detect the change.
    let changes = watcher.check_for_changes();
    assert!(!changes.is_empty(), "should detect file modification");
}
