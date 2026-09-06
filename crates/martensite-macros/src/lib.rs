//! Procedural macros for Martensite.
use proc_macro::TokenStream;

/// Attribute macro for declaring Martensite widgets.
#[proc_macro]
pub fn widget(_item: TokenStream) -> TokenStream {
    TokenStream::new()
}
