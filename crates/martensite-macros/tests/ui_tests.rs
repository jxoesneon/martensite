//! Compile-fail tests for the `widget!` macro using `trybuild`.
//!
//! These tests verify that invalid `widget!` invocations produce clear
//! `compile_error!` diagnostics at compile time.

#[test]
fn ui() {
    let t = trybuild::TestCases::new();
    t.compile_fail("tests/ui/*.rs");
    t.pass("tests/ui/pass/*.rs");
}
