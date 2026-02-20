#![no_std]

use proc_macro::TokenStream;

mod percpu;

/// Defines a per-CPU variable.
///
/// The variable must be a static item.
#[proc_macro_attribute]
pub fn percpu(attr: TokenStream, item: TokenStream) -> TokenStream {
    percpu::percpu_impl(attr, item)
}
