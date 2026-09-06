//! Integration tests for the `widget!` procedural macro.

use martensite_macros::widget;

#[test]
fn widget_macro_accepts_empty_token_stream() {
    // The widget! macro should accept an empty token stream and expand
    // (the current implementation expands to nothing). If expansion failed,
    // this test would not compile.
    widget!();
}

#[test]
fn widget_macro_accepts_arbitrary_tokens() {
    // The macro is currently a no-op that accepts any token stream,
    // so providing tokens must also expand without error.
    widget!(some tokens here);
}
