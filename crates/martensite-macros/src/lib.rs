//! Procedural macros for Martensite.
use proc_macro::TokenStream;

/// Declares a Martensite widget, expanding to a unit-type struct that
/// implements [`Default`].
///
/// # Example
///
/// ```ignore
/// widget!(MyButton);
/// ```
///
/// Expands to:
///
/// ```ignore
/// #[derive(Default)]
/// pub struct MyButton;
/// ```
#[proc_macro]
pub fn widget(item: TokenStream) -> TokenStream {
    // Parse the input as a single identifier (the widget name).
    let name = item.to_string();
    let name = name.trim();
    if name.is_empty() {
        return "compile_error!(\"widget! requires a widget name\")"
            .parse()
            .unwrap_or_default();
    }
    let expanded = format!(
        "/// Auto-generated Martensite widget struct.\n\
         #[derive(Default)]\n\
         pub struct {name};"
    );
    expanded.parse().unwrap_or_default()
}
