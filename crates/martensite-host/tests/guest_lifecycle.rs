//! Guest lifecycle integration tests for `martensite-host`.
//!
//! These tests build a minimal C guest cdylib at runtime (exporting the
//! `martensite_render` symbol) and exercise the full `GuestLibrary` /
//! `HostApp` lifecycle against it: `load`, `get_symbol`, `reload`, and
//! `tick`. They require a C compiler (`cc`, `clang`, or `gcc`) to be
//! available; if none is found the tests skip gracefully at runtime
//! (they are **not** `#[ignore]`-gated).

#![forbid(unsafe_code)]

use martensite_host::{GuestLibrary, HostApp, HostError};
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
use tempfile::tempdir;

/// C source for a guest cdylib that exports `martensite_render`.
const RENDER_SOURCE: &str = r#"
#if defined(_WIN32) || defined(__CYGWIN__)
#define EXPORT __declspec(dllexport)
#else
#define EXPORT
#endif
EXPORT void martensite_render(void) {}
"#;

/// C source for a guest cdylib that does NOT export `martensite_render`
/// (used to verify `HostApp::tick` reports `MissingRenderSymbol`).
const NO_RENDER_SOURCE: &str = r#"
#if defined(_WIN32) || defined(__CYGWIN__)
#define EXPORT __declspec(dllexport)
#else
#define EXPORT
#endif
EXPORT void some_other_symbol(void) {}
"#;

/// Searches for an available C compiler, returning its name if one can
/// be invoked successfully. The tests use this to skip gracefully when
/// no toolchain is present instead of failing.
fn find_compiler() -> Option<String> {
    for compiler in ["cc", "clang", "gcc"] {
        let ok = Command::new(compiler)
            .arg("--version")
            .output()
            .map(|o| o.status.success())
            .unwrap_or(false);
        if ok {
            return Some(compiler.to_string());
        }
    }
    None
}

/// Returns the compiler to use, or prints a skip message and `None` if
/// no C compiler is available.
fn require_compiler() -> Option<String> {
    match find_compiler() {
        Some(c) => Some(c),
        None => {
            eprintln!(
                "skipping guest_lifecycle tests: no C compiler \
                 (cc/clang/gcc) found on PATH"
            );
            None
        }
    }
}

/// Compiles `source` into a cdylib named `name` inside `dir` and returns
/// the resulting artifact path. The output filename uses the platform's
/// `DLL_PREFIX`/`DLL_EXTENSION` so it loads correctly via `dlopen`/
/// `LoadLibrary`.
fn build_cdylib(compiler: &str, dir: &Path, name: &str, source: &str) -> PathBuf {
    let src_path = dir.join(format!("{name}.c"));
    fs::write(&src_path, source).expect("failed to write C source");

    let prefix = std::env::consts::DLL_PREFIX;
    let ext = std::env::consts::DLL_EXTENSION;
    let out = dir.join(format!("{prefix}{name}.{ext}"));

    let mut cmd = Command::new(compiler);
    // macOS uses `-dynamiclib`; other Unix uses `-shared -fPIC`; Windows
    // (MinGW gcc) uses `-shared`.
    #[cfg(target_os = "macos")]
    {
        cmd.arg("-dynamiclib");
    }
    #[cfg(all(unix, not(target_os = "macos")))]
    {
        cmd.arg("-shared").arg("-fPIC");
    }
    #[cfg(windows)]
    {
        cmd.arg("-shared");
    }
    cmd.arg("-o").arg(&out).arg(&src_path);

    let output = cmd.output().expect("failed to spawn C compiler");
    assert!(
        output.status.success(),
        "C compilation failed for {name}:\n--- stdout ---\n{}\n--- stderr ---\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr),
    );
    assert!(
        out.exists(),
        "cdylib artifact not found at {}",
        out.display()
    );
    out
}

#[test]
fn guest_library_load_and_path_matches() {
    let compiler = match require_compiler() {
        Some(c) => c,
        None => return,
    };
    let dir = tempdir().expect("failed to create temp dir");
    let lib_path = build_cdylib(&compiler, dir.path(), "guest_v1", RENDER_SOURCE);

    let library = GuestLibrary::load(&lib_path).expect("failed to load guest cdylib");
    assert_eq!(library.path(), lib_path);
}

#[test]
fn guest_library_get_symbol_render_is_callable() {
    let compiler = match require_compiler() {
        Some(c) => c,
        None => return,
    };
    let dir = tempdir().expect("failed to create temp dir");
    let lib_path = build_cdylib(&compiler, dir.path(), "guest_render", RENDER_SOURCE);

    let library = GuestLibrary::load(&lib_path).expect("failed to load guest cdylib");
    let render = library
        .get_symbol::<extern "C" fn()>("martensite_render")
        .expect("failed to get martensite_render symbol");
    // Calling the symbol should not panic.
    (*render)();
}

#[test]
fn guest_library_get_symbol_nonexistent_returns_symbol_not_found() {
    let compiler = match require_compiler() {
        Some(c) => c,
        None => return,
    };
    let dir = tempdir().expect("failed to create temp dir");
    let lib_path = build_cdylib(&compiler, dir.path(), "guest_sym", RENDER_SOURCE);

    let library = GuestLibrary::load(&lib_path).expect("failed to load guest cdylib");
    let result = library.get_symbol::<extern "C" fn()>("nonexistent");
    assert!(result.is_err());
    match result.unwrap_err() {
        HostError::SymbolNotFound(name) => assert_eq!(name, "nonexistent"),
        other => panic!("expected SymbolNotFound, got {other:?}"),
    }
}

#[test]
fn host_app_new_and_tick_succeeds() {
    let compiler = match require_compiler() {
        Some(c) => c,
        None => return,
    };
    let dir = tempdir().expect("failed to create temp dir");
    let lib_path = build_cdylib(&compiler, dir.path(), "guest_tick", RENDER_SOURCE);

    let library = GuestLibrary::load(&lib_path).expect("failed to load guest cdylib");
    let app = HostApp::new(library);
    app.tick()
        .expect("tick should succeed when render symbol is present");
}

#[test]
fn host_app_tick_missing_render_symbol_returns_error() {
    let compiler = match require_compiler() {
        Some(c) => c,
        None => return,
    };
    let dir = tempdir().expect("failed to create temp dir");
    let lib_path = build_cdylib(&compiler, dir.path(), "guest_no_render", NO_RENDER_SOURCE);

    let library = GuestLibrary::load(&lib_path).expect("failed to load guest cdylib");
    let app = HostApp::new(library);
    let result = app.tick();
    assert!(result.is_err());
    match result.unwrap_err() {
        HostError::MissingRenderSymbol => {}
        other => panic!("expected MissingRenderSymbol, got {other:?}"),
    }
}

#[test]
fn guest_library_reload_updates_path() {
    let compiler = match require_compiler() {
        Some(c) => c,
        None => return,
    };
    let dir = tempdir().expect("failed to create temp dir");
    let lib_v1 = build_cdylib(&compiler, dir.path(), "guest_reload_v1", RENDER_SOURCE);
    let lib_v2 = build_cdylib(&compiler, dir.path(), "guest_reload_v2", RENDER_SOURCE);

    let mut library = GuestLibrary::load(&lib_v1).expect("failed to load guest cdylib v1");
    assert_eq!(library.path(), lib_v1);

    library
        .reload(&lib_v2)
        .expect("failed to reload to second guest cdylib");
    assert_eq!(library.path(), lib_v2, "path did not update after reload");
    assert_ne!(library.path(), lib_v1);

    // The reloaded library should still be usable.
    let render = library
        .get_symbol::<extern "C" fn()>("martensite_render")
        .expect("failed to get martensite_render after reload");
    (*render)();
}

#[test]
fn guest_library_load_bogus_file_returns_load_failed() {
    let dir = tempdir().expect("failed to create temp dir");
    let suffix = std::env::consts::DLL_EXTENSION;
    let bogus = dir.path().join(format!("not_a_library.{suffix}"));
    fs::write(&bogus, b"not a library").expect("failed to write bogus file");

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
