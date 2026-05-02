use proc_macro::TokenStream;
use quote::quote;
use syn::{
    Error, Ident, ItemStatic, StaticMutability, Token, parse_macro_input, punctuated::Punctuated,
};

pub fn percpu_impl(attr: TokenStream, item: TokenStream) -> TokenStream {
    let input = parse_macro_input!(item as ItemStatic);

    if let StaticMutability::Mut(mut_token) = input.mutability {
        return Error::new_spanned(
            mut_token,
            "#[percpu] does not support static mut; use interior mutability instead",
        )
        .to_compile_error()
        .into();
    }

    let attr_parser = Punctuated::<Ident, Token![,]>::parse_terminated;
    let args = parse_macro_input!(attr with attr_parser);

    // currently only one argument is supported: core_local, which put the variable
    // in .percpu.core_local section, located at the beginning of the per-CPU data
    // segment.
    let mut core_local = false;
    for arg in args {
        if arg == "core_local" {
            if core_local {
                return Error::new_spanned(arg, "duplicate argument: core_local")
                    .to_compile_error()
                    .into();
            }
            core_local = true;
        } else {
            return Error::new_spanned(arg, "unknown argument, expected: core_local")
                .to_compile_error()
                .into();
        }
    }

    let section = if core_local {
        ".percpu.core_local"
    } else {
        ".percpu"
    };

    let name = &input.ident;
    let ty = &input.ty;
    let vis = &input.vis;
    let init = &input.expr;
    let attrs = &input.attrs;

    let new_item = quote! {
        #[unsafe(link_section = #section)]
        #[used]
        #(#attrs)*
        #vis static #name: crate::percpu::PerCpu<#ty> = crate::percpu::PerCpu::new(#init);
    };

    TokenStream::from(new_item)
}
