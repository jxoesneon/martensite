//! Comprehensive tests for the `cargo martensite tweak` subcommand.
//!
//! Validates:
//! 1. CLI argument parsing for `tweak dump`, `tweak apply`, `--dry-run`, `--file`, etc.
//! 2. Patch line parsing with various formats and comments.
//! 3. Applying patches in-place to files on disk.
//! 4. Dry-run preview mode (verifying no disk writes occur).
//! 5. Error recovery when files or target expressions are missing.

#![forbid(unsafe_code)]

use std::fs;
use std::path::PathBuf;
use tempfile::tempdir;

use cargo_martensite::cli::{parse_args, CliError, Command};
use cargo_martensite::tweak::{
    apply_patches, parse_patch_line, parse_patches, ParsedPatch, TweakAction, TweakError,
};

#[test]
fn test_cli_parsing_tweak_subcommand() {
    // 1. Default `cargo martensite tweak` defaults to Dump
    let args = vec!["cargo-martensite".to_string(), "tweak".to_string()];
    let cmd = parse_args(&args).expect("parse tweak default");
    assert_eq!(
        cmd,
        Command::Tweak {
            action: TweakAction::Dump,
            socket: None,
            allow_version_mismatch: false,
            file: None,
            dry_run: false,
        }
    );

    // 2. Explicit dump
    let args = vec![
        "cargo-martensite".to_string(),
        "tweak".to_string(),
        "dump".to_string(),
    ];
    let cmd = parse_args(&args).expect("parse tweak dump");
    assert!(matches!(
        cmd,
        Command::Tweak {
            action: TweakAction::Dump,
            ..
        }
    ));

    // 3. Explicit apply with --dry-run
    let args = vec![
        "cargo-martensite".to_string(),
        "tweak".to_string(),
        "apply".to_string(),
        "--dry-run".to_string(),
    ];
    let cmd = parse_args(&args).expect("parse tweak apply dry-run");
    assert_eq!(
        cmd,
        Command::Tweak {
            action: TweakAction::Apply,
            socket: None,
            allow_version_mismatch: false,
            file: None,
            dry_run: true,
        }
    );

    // 4. Tweak with --file and --socket
    let args = vec![
        "cargo-martensite".to_string(),
        "tweak".to_string(),
        "apply".to_string(),
        "--file".to_string(),
        "tweaks.patch".to_string(),
        "--socket".to_string(),
        "/tmp/dev.sock".to_string(),
        "--allow-version-mismatch".to_string(),
    ];
    let cmd = parse_args(&args).expect("parse tweak with file & socket");
    assert_eq!(
        cmd,
        Command::Tweak {
            action: TweakAction::Apply,
            socket: Some("/tmp/dev.sock".to_string()),
            allow_version_mismatch: true,
            file: Some("tweaks.patch".to_string()),
            dry_run: false,
        }
    );

    // 5. Cargo forwarding simulation (`martensite tweak`)
    let args = vec![
        "martensite".to_string(),
        "tweak".to_string(),
        "--dry-run".to_string(),
    ];
    let cmd = parse_args(&args).expect("parse forwarded tweak");
    assert_eq!(
        cmd,
        Command::Tweak {
            action: TweakAction::Dump,
            socket: None,
            allow_version_mismatch: false,
            file: None,
            dry_run: true,
        }
    );
}

#[test]
fn test_cli_parsing_invalid_flags() {
    let args = vec![
        "cargo-martensite".to_string(),
        "tweak".to_string(),
        "--unknown-flag".to_string(),
    ];
    assert!(matches!(
        parse_args(&args),
        Err(CliError::InvalidArgument { .. })
    ));

    let args = vec![
        "cargo-martensite".to_string(),
        "tweak".to_string(),
        "invalid_action".to_string(),
    ];
    assert!(matches!(
        parse_args(&args),
        Err(CliError::InvalidArgument { .. })
    ));

    let args = vec![
        "cargo-martensite".to_string(),
        "tweak".to_string(),
        "--file".to_string(),
    ];
    assert!(matches!(
        parse_args(&args),
        Err(CliError::InvalidArgument { .. })
    ));
}

#[test]
fn test_patch_line_parsing() {
    // Canonical format: `file:line: .prop(old) -> .prop(new)`
    let line1 = "src/ui.rs:142: .padding(12.0) -> .padding(16.0)";
    let p1 = parse_patch_line(line1).expect("parse line1");
    assert_eq!(p1.file, PathBuf::from("src/ui.rs"));
    assert_eq!(p1.line, 142);
    assert_eq!(p1.old_text, ".padding(12.0)");
    assert_eq!(p1.new_text, ".padding(16.0)");
    assert_eq!(p1.format_canonical(), line1);

    // Format with col: `file:line:col: .prop(old) -> .prop(new)`
    let line2 = "crates/app/src/header.rs:25:8: .gap(4.0) -> .gap(8.0)";
    let p2 = parse_patch_line(line2).expect("parse line2");
    assert_eq!(p2.file, PathBuf::from("crates/app/src/header.rs"));
    assert_eq!(p2.line, 25);
    assert_eq!(p2.old_text, ".gap(4.0)");
    assert_eq!(p2.new_text, ".gap(8.0)");

    // Raw value replacement: `file:line: old -> new`
    let line3 = "src/config.rs:10: 100u32 -> 200u32";
    let p3 = parse_patch_line(line3).expect("parse line3");
    assert_eq!(p3.file, PathBuf::from("src/config.rs"));
    assert_eq!(p3.line, 10);
    assert_eq!(p3.old_text, "100u32");
    assert_eq!(p3.new_text, "200u32");

    // Comments and empty lines ignored
    assert!(parse_patch_line("").is_none());
    assert!(parse_patch_line("   ").is_none());
    assert!(parse_patch_line("# Comment line").is_none());
    assert!(parse_patch_line("// Another comment").is_none());
    assert!(parse_patch_line("not a valid patch format").is_none());
}

#[test]
fn test_parse_multiple_patches() {
    let multi = r#"
# Live Tweaks Patches
src/ui/header.rs:10: .padding(12.0) -> .padding(16.0)
// Spacing adjustment
src/ui/header.rs:20: .gap(4.0) -> .gap(8.0)

src/ui/theme.rs:5: [255, 0, 0, 255] -> [0, 255, 0, 255]
"#;

    let patches = parse_patches(multi);
    assert_eq!(patches.len(), 3);
    assert_eq!(patches[0].file, PathBuf::from("src/ui/header.rs"));
    assert_eq!(patches[0].line, 10);
    assert_eq!(patches[1].line, 20);
    assert_eq!(patches[2].file, PathBuf::from("src/ui/theme.rs"));
}

#[test]
fn test_apply_patches_in_place_success() {
    let dir = tempdir().expect("create temp dir");
    let file_a = dir.path().join("header.rs");
    let initial_content = r#"use martensite::prelude::*;

pub fn header() -> impl Widget {
    Container::new()
        .padding(12.0)
        .gap(4.0)
        .build()
}
"#;
    fs::write(&file_a, initial_content).expect("write header.rs");

    let patch1 = ParsedPatch::new(&file_a, 5, ".padding(12.0)", ".padding(18.0)");
    let patch2 = ParsedPatch::new(&file_a, 6, ".gap(4.0)", ".gap(8.5)");

    let report = apply_patches(&[patch1, patch2], false).expect("apply patches");
    assert_eq!(report.patches_processed, 2);
    assert_eq!(report.patches_applied, 2);
    assert_eq!(report.files_modified, 1);
    assert!(!report.dry_run);

    let updated = fs::read_to_string(&file_a).expect("read updated header.rs");
    let expected = r#"use martensite::prelude::*;

pub fn header() -> impl Widget {
    Container::new()
        .padding(18.0)
        .gap(8.5)
        .build()
}
"#;
    assert_eq!(updated, expected);
}

#[test]
fn test_apply_patches_dry_run_leaves_file_untouched() {
    let dir = tempdir().expect("create temp dir");
    let file_path = dir.path().join("view.rs");
    let content = "let width = 100.0;\nlet height = 50.0;\n";
    fs::write(&file_path, content).expect("write file");

    let patch = ParsedPatch::new(&file_path, 1, "100.0", "150.0");
    let report = apply_patches(&[patch], true).expect("apply dry-run");

    assert_eq!(report.patches_applied, 1);
    assert_eq!(report.files_modified, 1);
    assert!(report.dry_run);
    assert!(report.details[0].contains("[DRY RUN]"));

    // File on disk MUST remain untouched
    let disk_content = fs::read_to_string(&file_path).expect("read file");
    assert_eq!(disk_content, content);
}

#[test]
fn test_apply_patches_tolerant_line_search() {
    let dir = tempdir().expect("create temp dir");
    let file_path = dir.path().join("shifted.rs");
    // Line was recorded at 2, but has moved to line 4 due to added comments
    let content = "// Comment 1\n// Comment 2\n\nlet size = 32.0;\n";
    fs::write(&file_path, content).expect("write shifted file");

    let patch = ParsedPatch::new(&file_path, 2, "32.0", "48.0");
    let report = apply_patches(&[patch], false).expect("apply shifted patch");
    assert_eq!(report.patches_applied, 1);

    let updated = fs::read_to_string(&file_path).expect("read updated");
    assert_eq!(updated, "// Comment 1\n// Comment 2\n\nlet size = 48.0;\n");
}

#[test]
fn test_apply_patches_error_cases() {
    let dir = tempdir().expect("create temp dir");
    let non_existent = dir.path().join("missing.rs");

    // Missing file error
    let patch = ParsedPatch::new(&non_existent, 1, "a", "b");
    let res = apply_patches(&[patch], false);
    match res {
        Err(TweakError::PatchApply { file, reason, .. }) => {
            assert!(file.contains("missing.rs"));
            assert!(reason.contains("does not exist"));
        }
        other => panic!(
            "expected PatchApply error for missing file, got {:?}",
            other
        ),
    }

    // Missing target value in existing file
    let file_path = dir.path().join("exists.rs");
    fs::write(&file_path, "let x = 10;\n").expect("write exists.rs");
    let patch_missing_val = ParsedPatch::new(&file_path, 1, "non_existent_target", "new_val");
    let res = apply_patches(&[patch_missing_val], false);
    match res {
        Err(TweakError::PatchApply { file, reason, .. }) => {
            assert!(file.contains("exists.rs"));
            assert!(reason.contains("could not find old value"));
        }
        other => panic!(
            "expected PatchApply error for missing target, got {:?}",
            other
        ),
    }
}
