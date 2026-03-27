use proc_macro::TokenStream;
use quote::{format_ident, quote};
use syn::{Error, Expr, FnArg, ItemFn, Pat, Path, ReturnType, parse::Parser};

pub fn syscall_impl(attr: TokenStream, item: TokenStream) -> TokenStream {
    let mut sysno_expr: Option<Expr> = None;
    let parser = syn::meta::parser(|meta| {
        if meta.path.is_ident("no") {
            if sysno_expr.is_some() {
                return Err(meta.error("duplicate `no` argument"));
            }

            sysno_expr = Some(meta.value()?.parse::<Expr>()?);
            Ok(())
        } else {
            Err(meta.error("unsupported syscall attribute argument"))
        }
    });

    let attr_ts = proc_macro2::TokenStream::from(attr.clone());
    if let Err(err) = parser.parse2(attr_ts) {
        return err.to_compile_error().into();
    }

    let Some(sysno_expr) = sysno_expr else {
        return Error::new(
            proc_macro2::Span::call_site(),
            "missing `no = ...` in #[syscall(...)]",
        )
        .to_compile_error()
        .into();
    };

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

        let mut validate_with: Option<Path> = None;
        for attr in &arg.attrs {
            if !attr.path().is_ident("sysarg") {
                continue;
            }

            let parse_result = attr.parse_nested_meta(|meta| {
                if meta.path.is_ident("validate_with") {
                    if validate_with.is_some() {
                        return Err(meta.error("duplicate `validate_with` argument"));
                    }

                    validate_with = Some(meta.value()?.parse::<Path>()?);
                    Ok(())
                } else {
                    Err(meta.error("unsupported sysarg attribute argument"))
                }
            });

            if let Err(err) = parse_result {
                return err.to_compile_error().into();
            }
        }
        arg.attrs.retain(|attr| !attr.path().is_ident("sysarg"));

        arg_bindings.push(match validate_with {
			Some(validate_with) => quote! {
				let #arg_name: #arg_ty = #validate_with(regs.args[#arg_index] as u64, ctx)?;
			},
			None => quote! {
				let #arg_name = <#arg_ty as crate::syscall::handler::TryFromSyscallArg>::try_from_syscall_arg(
					regs.args[#arg_index] as u64,
					ctx,
				)?;
			},
		});
        arg_names.push(arg_name.clone());
    }

    let nargs = arg_names.len();

    let expanded = quote! {
        #input

        fn #wrapper_name(
            regs: &crate::syscall::handler::SyscallRegs,
            ctx: &crate::syscall::handler::SyscallCtx,
        ) -> core::result::Result<u64, crate::syserror::SysError> {
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
