//! Developer CLI entry point for the Martensite GUI framework toolchain.
fn main() {
    println!("Martensite CLI v0.0.1");
}

/// Smoke test verifying the CLI crate compiles and links correctly.
#[cfg(test)]
#[test]
fn cli_smoke_test() {
    // Compilation + test execution is the smoke test.
    let output = "Martensite CLI v0.0.1";
    assert_eq!(output, "Martensite CLI v0.0.1");
}
