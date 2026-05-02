use proc_macro::TokenStream;
use quote::{format_ident, quote};
use syn::{
    Error, Expr, FnArg, Ident, ItemFn, Pat, Token,
    parse::{Parse, ParseStream},
};

struct SyscallAttr {
    sysno: Expr,
    preparse: Option<Expr>,
}

impl Parse for SyscallAttr {
    fn parse(input: ParseStream<'_>) -> syn::Result<Self> {
        let sysno = input.parse()?;
        let mut preparse = None;

        while !input.is_empty() {
            input.parse::<Token![,]>()?;
            if input.is_empty() {
                break;
            }

            let key: Ident = input.parse()?;
            input.parse::<Token![=]>()?;

            match key.to_string().as_str() {
                "preparse" => {
                    if preparse.is_some() {
                        return Err(Error::new_spanned(
                            key,
                            "duplicate `preparse` syscall attribute",
                        ));
                    }
                    preparse = Some(input.parse()?);
                },
                _ => {
                    return Err(Error::new_spanned(
                        key,
                        "unsupported syscall attribute; expected `preparse = ...`",
                    ));
                },
            }
        }

        Ok(Self { sysno, preparse })
    }
}

pub fn syscall_impl(attr: TokenStream, item: TokenStream) -> TokenStream {
    let SyscallAttr {
        sysno: sysno_expr,
        preparse,
    } = syn::parse_macro_input!(attr as SyscallAttr);

    let mut input = syn::parse_macro_input!(item as ItemFn);

    if input.sig.constness.is_some() {
        return Error::new_spanned(&input.sig.constness, "syscall function cannot be const")
            .to_compile_error()
            .into();
    }
    if input.sig.asyncness.is_some() {
        return Error::new_spanned(&input.sig.asyncness, "syscall function cannot be async")
            .to_compile_error()
            .into();
    }
    if input.sig.unsafety.is_some() {
        return Error::new_spanned(&input.sig.unsafety, "syscall function cannot be unsafe")
            .to_compile_error()
            .into();
    }
    if input.sig.abi.is_some() {
        return Error::new_spanned(&input.sig.abi, "syscall function cannot specify an ABI")
            .to_compile_error()
            .into();
    }
    if input.sig.variadic.is_some() {
        return Error::new_spanned(&input.sig.variadic, "syscall function cannot be variadic")
            .to_compile_error()
            .into();
    }
    if !input.sig.generics.params.is_empty() {
        return Error::new_spanned(&input.sig.generics, "syscall function cannot be generic")
            .to_compile_error()
            .into();
    }

    let name = &input.sig.ident;
    let wrapper_name = format_ident!("__sys_wrap_{}", name);
    let static_name = format_ident!("__SYSCALL_{}", name.to_string().to_uppercase());

    let mut arg_bindings = Vec::new();
    let mut arg_names = Vec::new();

    for (index, arg) in input.sig.inputs.iter_mut().enumerate() {
        if index >= 6 {
            return Error::new_spanned(arg, "syscall function cannot have more than 6 arguments")
                .to_compile_error()
                .into();
        }

        let FnArg::Typed(arg) = arg else {
            return Error::new_spanned(arg, "syscall function cannot have a receiver")
                .to_compile_error()
                .into();
        };

        let Pat::Ident(pat_ident) = &*arg.pat else {
            return Error::new_spanned(
                &arg.pat,
                "syscall parameters must use simple identifier patterns",
            )
            .to_compile_error()
            .into();
        };

        let arg_name = &pat_ident.ident;
        let arg_ty = &arg.ty;
        let arg_index = syn::Index::from(index);

        let mut validate_with: Option<Expr> = None;
        for attr in &arg.attrs {
            if !attr.path().is_ident("validate_with") {
                continue;
            }

            if validate_with.is_some() {
                return Error::new_spanned(attr, "duplicate #[validate_with(...)] attribute")
                    .to_compile_error()
                    .into();
            }

            match attr.parse_args::<Expr>() {
                Ok(expr) => validate_with = Some(expr),
                Err(err) => return err.to_compile_error().into(),
            }
        }
        arg.attrs
            .retain(|attr| !attr.path().is_ident("validate_with"));

        arg_bindings.push(match validate_with {
            Some(validate_with) => quote! {
                let __validate_with = (#validate_with);
                let #arg_name: #arg_ty = __validate_with(regs.args[#arg_index])?;
            },
            None => quote! {
                let #arg_name = <#arg_ty as crate::syscall::handler::TryFromSyscallArg>::try_from_syscall_arg(
                    regs.args[#arg_index],
                )?;
            },
        });
        arg_names.push(arg_name.clone());
    }

    let nargs = arg_names.len();
    let raw_arg_exprs: Vec<_> = (0..nargs)
        .map(|index| {
            let arg_index = syn::Index::from(index);
            quote! { regs.args[#arg_index] }
        })
        .collect();
    let preparse_call = match preparse {
        Some(preparse) => quote! {
            {
                let __preparse = (#preparse);
                let _ = __preparse(#(#raw_arg_exprs),*);
            }
        },
        None => quote! {},
    };

    let expanded = quote! {
        #input

        fn #wrapper_name(
            regs: &crate::syscall::handler::SyscallRegs,
        ) -> core::result::Result<u64, crate::syserror::SysError> {
            #preparse_call
            #(#arg_bindings)*

            #name(#(#arg_names),*)
                .map_err(core::convert::Into::into)
        }

        #[used]
        #[unsafe(link_section = ".syscall")]
        static #static_name: crate::syscall::handler::SyscallHandler = crate::syscall::handler::SyscallHandler {
            sysno: (#sysno_expr) as usize,
            nargs: #nargs,
            name: concat!(module_path!(), "::", stringify!(#name)),
            handler: #wrapper_name,
        };
    };

    expanded.into()
}
