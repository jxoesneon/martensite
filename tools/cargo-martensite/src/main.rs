//! Developer CLI entry point for the Martensite GUI framework toolchain.
fn main() {
    println!("Martensite CLI v{}", env!("CARGO_PKG_VERSION"));
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
