//! Procedural macros for Martensite.
use proc_macro::TokenStream;

#[proc_macro]
pub fn widget(_item: TokenStream) -> TokenStream {
    TokenStream::new()
}
