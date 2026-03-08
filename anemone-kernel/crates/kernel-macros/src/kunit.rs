use proc_macro::TokenStream;
use quote::{format_ident, quote};
use syn::{Error, ItemFn, ReturnType, parse_macro_input};

pub fn kunit_impl(attr: TokenStream, item: TokenStream) -> TokenStream {
    if !attr.is_empty() {
        return Error::new_spanned(
            proc_macro2::TokenStream::from(attr),
            "#[kunit] does not accept any arguments",
        )
        .to_compile_error()
        .into();
    }

    let input = parse_macro_input!(item as ItemFn);

    if input.sig.constness.is_some() {
        return Error::new_spanned(&input.sig.constness, "kunit test function cannot be const")
            .to_compile_error()
            .into();
    }

    if input.sig.asyncness.is_some() {
        return Error::new_spanned(&input.sig.asyncness, "kunit test function cannot be async")
            .to_compile_error()
            .into();
    }

    if input.sig.unsafety.is_some() {
        return Error::new_spanned(&input.sig.unsafety, "kunit test function cannot be unsafe")
            .to_compile_error()
            .into();
    }

    if input.sig.abi.is_some() {
        return Error::new_spanned(&input.sig.abi, "kunit test function cannot specify an ABI")
            .to_compile_error()
            .into();
    }

    if !input.sig.generics.params.is_empty() || input.sig.generics.where_clause.is_some() {
        return Error::new_spanned(
            &input.sig.generics,
            "kunit test function cannot have generics",
        )
        .to_compile_error()
        .into();
    }

    if !input.sig.inputs.is_empty() {
        return Error::new_spanned(
            &input.sig.inputs,
            "kunit test function must have signature fn()",
        )
        .to_compile_error()
        .into();
    }

    if input.sig.variadic.is_some() {
        return Error::new_spanned(
            &input.sig.variadic,
            "kunit test function cannot be variadic",
        )
        .to_compile_error()
        .into();
    }

    if !matches!(input.sig.output, ReturnType::Default) {
        return Error::new_spanned(
            &input.sig.output,
            "kunit test function must have no return value",
        )
        .to_compile_error()
        .into();
    }

    let name = &input.sig.ident;
    let kunit_name = format_ident!("__KUNIT_{}", input.sig.ident.to_string().to_uppercase());

    let new_item = quote! {
        #[cfg(feature = "kunit")]
        #[used]
        #[unsafe(link_section = ".kunit")]
        static #kunit_name: crate::debug::kunit::KUnit = crate::debug::kunit::KUnit {
            name: concat!(module_path!(), "::", stringify!(#name)),
            test_fn: #name,
        };
    };
    let output = quote! {
        #new_item
        #input
    };
    output.into()
}
