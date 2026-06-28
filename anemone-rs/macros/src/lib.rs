use proc_macro::TokenStream;

mod entry;
mod signal;

/// Defines the entry point of an Anemone application.
///
/// Expects a function with the signature `(pub) fn main() -> Result<(),
/// Errno>`.
#[proc_macro_attribute]
pub fn main(attr: TokenStream, item: TokenStream) -> TokenStream {
    entry::main_impl(attr, item)
}

/// Defines a signal handler function.
///
/// The function must have signature
/// - `(pub) fn (signo: SigNo) -> ()` if `siginfo` is not required, or
/// - `(pub) fn (signo: SigNo, siginfo: *const SigInfo, ucontext: *const
///   UContext) -> ()` if `siginfo` is required.
#[proc_macro_attribute]
pub fn signal_handler(attr: TokenStream, item: TokenStream) -> TokenStream {
    signal::signal_handler_impl(attr, item)
}
