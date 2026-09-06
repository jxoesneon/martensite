//! Criterion benchmark suites for Martensite.
#![forbid(unsafe_code)]

/// Smoke test verifying the bench suite crate compiles correctly.
#[cfg(test)]
#[test]
fn bench_suite_smoke_test() {
    // Compilation + test execution is the smoke test.
    let name = "bench_suite";
    assert_eq!(name, "bench_suite");
}
