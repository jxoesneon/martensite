//! Hot-reload latency gate test for `martensite-host`.
//!
//! This integration test measures the full `file change -> build -> load ->
//! reload` pipeline latency and asserts it is below the 350 ms milestone
//! budget. It is `#[ignore]` by default because it spawns real `cargo`
//! builds and requires a working C toolchain.

use martensite_host::GuestLibrary;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::Instant;

/// The hot-reload latency budget from the v0.9.0 milestone spec.
const RELOAD_BUDGET_MS: u64 = 350;

/// Creates a minimal temporary guest cdylib crate with the given source
/// counter baked into a `martensite_render` symbol.
fn create_guest_crate(counter: u32) -> tempfile::TempDir {
    let dir = tempfile::tempdir().expect("failed to create temp dir");

    let cargo_toml = r#"[package]
name = "guest_latency"
version = "0.1.0"
edition = "2021"

[lib]
crate-type = ["cdylib"]
"#;
    fs::write(dir.path().join("Cargo.toml"), cargo_toml).expect("failed to write Cargo.toml");
    fs::create_dir_all(dir.path().join("src")).expect("failed to create src dir");

    let lib_rs = format!(
        r#"#[no_mangle]
pub extern "C" fn martensite_render() {{
    let _ = {counter}u32;
}}
"#
    );
    fs::write(dir.path().join("src/lib.rs"), lib_rs).expect("failed to write lib.rs");

    dir
}

/// Builds the guest crate as a cdylib and returns the artifact path.
fn build_guest_dylib(guest_dir: &Path, target_dir: &Path) -> PathBuf {
    let output = Command::new("cargo")
        .arg("build")
        .arg("--lib")
        .arg("--target-dir")
        .arg(target_dir)
        .current_dir(guest_dir)
        .output()
        .expect("failed to spawn cargo");

    assert!(
        output.status.success(),
        "guest build failed:\n--- stdout ---\n{}\n--- stderr ---\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr),
    );

    let debug_dir = target_dir.join("debug");
    let prefix = std::env::consts::DLL_PREFIX;
    let ext = std::env::consts::DLL_EXTENSION;
    let artifact = debug_dir.join(format!("{prefix}guest_latency.{ext}"));
    assert!(
        artifact.exists(),
        "cdylib artifact not found at {}",
        artifact.display()
    );
    artifact
}

/// Returns a deterministic versioned cdylib path.
fn versioned_path(target_dir: &Path, version: u64) -> PathBuf {
    let prefix = std::env::consts::DLL_PREFIX;
    let ext = std::env::consts::DLL_EXTENSION;
    target_dir.join(format!("{prefix}guest_latency_v{version}.{ext}"))
}

/// End-to-end <350 ms hot-reload gate test.
///
/// Builds a guest cdylib, loads it, modifies the source, rebuilds, and
/// reloads into `GuestLibrary`, measuring the full pipeline from the file
/// write to the new dylib being live.
#[test]
#[ignore = "performance: requires cargo build environment and C toolchain"]
fn file_change_to_dylib_reload_within_350ms() {
    let guest = create_guest_crate(1);
    let target_dir = tempfile::tempdir().expect("failed to create target dir");
    let target_path = target_dir.path();

    let artifact_v1 = build_guest_dylib(guest.path(), target_path);
    let versioned_v1 = versioned_path(target_path, 1);
    fs::copy(&artifact_v1, &versioned_v1).expect("failed to copy v1");

    let mut library = GuestLibrary::load(&versioned_v1).expect("failed to load v1");

    // Modify the guest source so the rebuild produces a different dylib.
    fs::write(
        guest.path().join("src/lib.rs"),
        r#"#[no_mangle]
pub extern "C" fn martensite_render() {
    let _ = 2u32;
}
"#,
    )
    .expect("failed to modify guest source");

    // Measure from the file change to the new dylib being loaded.
    let start = Instant::now();
    let artifact_v2 = build_guest_dylib(guest.path(), target_path);
    let versioned_v2 = versioned_path(target_path, 2);
    fs::copy(&artifact_v2, &versioned_v2).expect("failed to copy v2");
    library.reload(&versioned_v2).expect("failed to reload v2");
    let elapsed_ms = start.elapsed().as_millis() as u64;

    assert!(
        elapsed_ms < RELOAD_BUDGET_MS,
        "hot-reload pipeline latency was {elapsed_ms} ms, exceeding the {RELOAD_BUDGET_MS} ms budget"
    );

    eprintln!("hot-reload pipeline latency: {elapsed_ms} ms");
}
