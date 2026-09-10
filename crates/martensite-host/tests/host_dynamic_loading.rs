//! Dynamic-loading integration tests for `martensite-host`.
//!
//! These tests exercise `GuestLibrary::load`, `get_symbol`, `reload`, and
//! `HostApp::tick` against real system libraries so they require no C
//! toolchain to build a guest cdylib. They are platform-gated: libc.so.6 on
//! Linux, libSystem.dylib on macOS, and kernel32.dll on Windows.

#![forbid(unsafe_code)]

use martensite_host::{GuestLibrary, HostApp, HostError};
use std::path::Path;
use tempfile::tempdir;

/// The canonical system C library path for the current platform.
fn system_c_library_path() -> &'static str {
    #[cfg(target_os = "linux")]
    {
        "libc.so.6"
    }
    #[cfg(target_os = "macos")]
    {
        "libSystem.dylib"
    }
    #[cfg(target_os = "windows")]
    {
        "kernel32.dll"
    }
    #[cfg(not(any(target_os = "linux", target_os = "macos", target_os = "windows")))]
    {
        compile_error!("host_dynamic_loading tests require linux, macos, or windows");
    }
}

/// A second distinct system library path used to verify `reload`.
fn second_system_library_path() -> &'static str {
    #[cfg(target_os = "linux")]
    {
        "libm.so.6"
    }
    #[cfg(target_os = "macos")]
    {
        "libm.dylib"
    }
    #[cfg(target_os = "windows")]
    {
        "msvcrt.dll"
    }
}

/// The well-known symbol name resolved from the system C library.
fn well_known_symbol_name() -> &'static str {
    #[cfg(unix)]
    {
        "getpid"
    }
    #[cfg(windows)]
    {
        "GetCurrentProcessId"
    }
}

/// Loads the system C library and returns the `GuestLibrary`.
fn load_system_library() -> GuestLibrary {
    GuestLibrary::load(Path::new(system_c_library_path())).expect("failed to load system library")
}

#[cfg(unix)]
type WellKnownSymbol = extern "C" fn() -> i32;

#[cfg(windows)]
type WellKnownSymbol = extern "system" fn() -> u32;

#[test]
fn load_and_get_symbol_from_system_library() {
    let library = load_system_library();
    let symbol = library
        .get_symbol::<WellKnownSymbol>(well_known_symbol_name())
        .expect("failed to get well-known symbol");
    let pid = (*symbol)();
    assert!(
        pid > 0,
        "well-known symbol returned a non-positive result: {pid}"
    );
}

#[test]
fn get_symbol_missing_returns_symbol_not_found() {
    let library = load_system_library();
    let result = library.get_symbol::<extern "C" fn()>("definitely_not_a_real_symbol_xyzzy");
    assert!(result.is_err());
    match result.unwrap_err() {
        HostError::SymbolNotFound(name) => {
            assert_eq!(name, "definitely_not_a_real_symbol_xyzzy");
        }
        other => panic!("expected SymbolNotFound, got {other:?}"),
    }
}

#[test]
fn host_app_tick_missing_render_symbol() {
    let library = load_system_library();
    let app = HostApp::new(library);
    let result = app.tick();
    assert!(result.is_err());
    match result.unwrap_err() {
        HostError::MissingRenderSymbol => {}
        other => panic!("expected MissingRenderSymbol, got {other:?}"),
    }
}

#[test]
fn guest_library_reloads_to_another_valid_library() {
    let mut library = load_system_library();
    let original_path = library.path().to_path_buf();

    library
        .reload(Path::new(second_system_library_path()))
        .expect("failed to reload to second system library");

    assert_ne!(
        library.path(),
        &original_path,
        "path did not change after reload"
    );
    assert_eq!(
        library.path(),
        Path::new(second_system_library_path()),
        "path does not match the reloaded library"
    );
}

#[test]
fn load_invalid_bytes_returns_load_failed() {
    let dir = tempdir().expect("failed to create temp dir");
    let suffix = std::env::consts::DLL_EXTENSION;
    let bogus = dir.path().join(format!("not_a_library.{suffix}"));
    std::fs::write(&bogus, b"not a library").expect("failed to write bogus file");

    let result = GuestLibrary::load(&bogus);
    assert!(result.is_err());
    match result.unwrap_err() {
        HostError::LoadFailed(path, msg) => {
            assert_eq!(path, bogus);
            assert!(!msg.is_empty(), "LoadFailed message should be non-empty");
        }
        other => panic!("expected LoadFailed, got {other:?}"),
    }
}
