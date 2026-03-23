use proc_macro::TokenStream;
use quote::{format_ident, quote};
use syn::{
    Error, Ident, ItemFn, ReturnType, Token, parse_macro_input, punctuated::Punctuated,
};

pub fn kunit_impl(attr: TokenStream, item: TokenStream) -> TokenStream {
    let attr_parser = Punctuated::<Ident, Token![,]>::parse_terminated;
    let args = parse_macro_input!(attr with attr_parser);

    let mut percpu = false;
    for arg in args {
        if arg == "percpu" {
            if percpu {
                return Error::new_spanned(arg, "duplicate argument: percpu")
                    .to_compile_error()
                    .into();
            }
            percpu = true;
        } else {
            return Error::new_spanned(arg, "unknown argument, expected: percpu")
                .to_compile_error()
                .into();
        }
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
    let kind = if percpu {
        quote!(crate::debug::kunit::KUnitKind::PerCpu)
    } else {
        quote!(crate::debug::kunit::KUnitKind::Plain)
    };

    let new_item = quote! {
        #[cfg(feature = "kunit")]
        #[used]
        #[unsafe(link_section = ".kunit")]
        static #kunit_name: crate::debug::kunit::KUnit = crate::debug::kunit::KUnit {
            name: concat!(module_path!(), "::", stringify!(#name)),
            test_fn: #name,
            kind: #kind,
        };
    };
    let output = quote! {
        #new_item
        #input
    };
    output.into()
}
