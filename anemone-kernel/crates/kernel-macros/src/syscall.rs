use proc_macro::TokenStream;
use quote::{format_ident, quote};
use syn::{Error, Expr, FnArg, ItemFn, Pat, Path};

pub fn syscall_impl(attr: TokenStream, item: TokenStream) -> TokenStream {
    let sysno_expr = syn::parse_macro_input!(attr as Expr);

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
            if !attr.path().is_ident("validate_with") {
                continue;
            }

            if validate_with.is_some() {
                return Error::new_spanned(attr, "duplicate #[validate_with(...)] attribute")
                    .to_compile_error()
                    .into();
            }

            match attr.parse_args::<Path>() {
                Ok(path) => validate_with = Some(path),
                Err(err) => return err.to_compile_error().into(),
            }
        }
        arg.attrs.retain(|attr| !attr.path().is_ident("validate_with"));

        arg_bindings.push(match validate_with {
            Some(validate_with) => quote! {
                let #arg_name: #arg_ty = #validate_with(regs.args[#arg_index])?;
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

    let expanded = quote! {
        #input

        fn #wrapper_name(
            regs: &crate::syscall::handler::SyscallRegs,
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
