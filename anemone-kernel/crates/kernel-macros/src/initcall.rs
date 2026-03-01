use proc_macro::TokenStream;
use quote::{format_ident, quote};
use syn::Error;

pub fn initcall_impl(attr: TokenStream, item: TokenStream) -> TokenStream {
    if attr.is_empty() {
        return Error::new_spanned(
            proc_macro2::TokenStream::from(attr),
            "#[initcall] requires an argument specifying the initcall level",
        )
        .to_compile_error()
        .into();
    }

    let input = syn::parse_macro_input!(item as syn::ItemFn);
    if input.sig.asyncness.is_some() {
        return Error::new_spanned(&input.sig.asyncness, "initcall function cannot be async")
            .to_compile_error()
            .into();
    }
    if input.sig.unsafety.is_some() {
        return Error::new_spanned(&input.sig.unsafety, "initcall function cannot be unsafe")
            .to_compile_error()
            .into();
    }
    if input.sig.abi.is_some() {
        return Error::new_spanned(&input.sig.abi, "initcall function cannot specify an ABI")
            .to_compile_error()
            .into();
    }
    if input.sig.variadic.is_some() {
        return Error::new_spanned(&input.sig.variadic, "initcall function cannot be variadic")
            .to_compile_error()
            .into();
    }

    // parse level argument
    let level_string = attr.to_string();
    let level_str = level_string.trim();
    let level = match level_str {
        "driver" => quote!(crate::initcall::InitCallLevel::Driver),
        _ => {
            return Error::new_spanned(
                proc_macro2::TokenStream::from(attr),
                format!("invalid initcall level: {}", level_str),
            )
            .to_compile_error()
            .into();
        },
    };

    let link_section = format!(".initcall.{}", level_str);

    let name = &input.sig.ident;
    let initcall_name = format_ident!("__INITCALL_{}", name.to_string().to_uppercase());

    let output = quote! {
        #input

        #[used]
        #[unsafe(link_section = #link_section)]
        static #initcall_name: crate::initcall::InitCall = crate::initcall::InitCall {
            name: concat!(module_path!(), "::", stringify!(#name)),
            init_fn: #name,
            level: #level,
        };
    };

    output.into()
}
