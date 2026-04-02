use proc_macro::TokenStream;

mod entry;

/// Defines the entry point of an Anemone application.
///
/// Expects a function with the signature `(pub) fn main() -> Result<(),
/// Errno>`.
#[proc_macro_attribute]
pub fn main(attr: TokenStream, item: TokenStream) -> TokenStream {
    entry::main_impl(attr, item)
}
