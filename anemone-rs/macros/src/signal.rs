use proc_macro::TokenStream;
use quote::{format_ident, quote};
use syn::{
    Error, FnArg, Ident, ItemFn, PatType, ReturnType, Type, TypePtr, parse_macro_input,
    spanned::Spanned,
};

pub fn signal_handler_impl(attr: TokenStream, item: TokenStream) -> TokenStream {
    let handler_kind = match parse_handler_kind(attr) {
        Ok(kind) => kind,
        Err(error) => return error.to_compile_error().into(),
    };

    let function = parse_macro_input!(item as ItemFn);

    if let Err(error) = validate_signal_signature(&function, handler_kind) {
        return error.to_compile_error().into();
    }

    expand_signal_handler(function, handler_kind).into()
}

#[derive(Clone, Copy)]
enum HandlerKind {
    SignoOnly,
    SigInfo,
}

fn parse_handler_kind(attr: TokenStream) -> Result<HandlerKind, Error> {
    if attr.is_empty() {
        return Ok(HandlerKind::SignoOnly);
    }

    let ident = syn::parse::<Ident>(attr)?;
    if ident == "siginfo" {
        Ok(HandlerKind::SigInfo)
    } else {
        Err(Error::new(
            ident.span(),
            "#[anemone_rs::signal_handler] only accepts no arguments or `(siginfo)`",
        ))
    }
}

fn validate_signal_signature(function: &ItemFn, handler_kind: HandlerKind) -> Result<(), Error> {
    if function.sig.asyncness.is_some() {
        return Err(Error::new(
            function.sig.asyncness.span(),
            "#[anemone_rs::signal_handler] does not support async functions",
        ));
    }

    if function.sig.constness.is_some() {
        return Err(Error::new(
            function.sig.constness.span(),
            "#[anemone_rs::signal_handler] does not support const functions",
        ));
    }

    if function.sig.abi.is_some() {
        return Err(Error::new(
            function.sig.abi.span(),
            "#[anemone_rs::signal_handler] expects a plain Rust function and will provide the C ABI shim itself",
        ));
    }

    if function.sig.variadic.is_some() {
        return Err(Error::new(
            function.sig.variadic.span(),
            "#[anemone_rs::signal_handler] does not support variadic functions",
        ));
    }

    if !function.sig.generics.params.is_empty() || function.sig.generics.where_clause.is_some() {
        return Err(Error::new(
            function.sig.generics.span(),
            "#[anemone_rs::signal_handler] does not support generics",
        ));
    }

    if !matches_unit_return(&function.sig.output) {
        return Err(Error::new(
            function.sig.output.span(),
            "#[anemone_rs::signal_handler] requires a function returning `()`",
        ));
    }

    let expected_args = match handler_kind {
        HandlerKind::SignoOnly => 1,
        HandlerKind::SigInfo => 3,
    };
    if function.sig.inputs.len() != expected_args {
        let message = match handler_kind {
            HandlerKind::SignoOnly => {
                "#[anemone_rs::signal_handler] requires `(pub) fn handler(signo: SigNo) -> ()`"
            },
            HandlerKind::SigInfo => {
                "#[anemone_rs::signal_handler(siginfo)] requires `(pub) fn handler(signo: SigNo, siginfo: *const SigInfo, ucontext: *const UContext) -> ()`"
            },
        };
        return Err(Error::new(function.sig.inputs.span(), message));
    }

    let mut inputs = function.sig.inputs.iter();
    validate_arg_type(inputs.next(), "SigNo")?;
    if matches!(handler_kind, HandlerKind::SigInfo) {
        validate_const_ptr_arg_type(inputs.next(), "SigInfo")?;
        validate_const_ptr_arg_type(inputs.next(), "UContext")?;
    }

    Ok(())
}

fn matches_unit_return(output: &ReturnType) -> bool {
    match output {
        ReturnType::Default => true,
        ReturnType::Type(_, ty) => {
            matches!(ty.as_ref(), Type::Tuple(tuple) if tuple.elems.is_empty())
        },
    }
}

fn validate_arg_type(arg: Option<&FnArg>, expected_ident: &str) -> Result<(), Error> {
    let Some(FnArg::Typed(PatType { ty, .. })) = arg else {
        return Err(Error::new(
            proc_macro2::Span::call_site(),
            "#[anemone_rs::signal_handler] only supports plain function arguments",
        ));
    };

    if matches_type_path_ident(ty, expected_ident) {
        Ok(())
    } else {
        Err(Error::new(
            ty.span(),
            format!(
                "#[anemone_rs::signal_handler] expects argument type `{}` here",
                expected_ident
            ),
        ))
    }
}

fn validate_const_ptr_arg_type(arg: Option<&FnArg>, expected_ident: &str) -> Result<(), Error> {
    let Some(FnArg::Typed(PatType { ty, .. })) = arg else {
        return Err(Error::new(
            proc_macro2::Span::call_site(),
            "#[anemone_rs::signal_handler] only supports plain function arguments",
        ));
    };

    let Type::Ptr(TypePtr {
        mutability, elem, ..
    }) = ty.as_ref()
    else {
        return Err(Error::new(
            ty.span(),
            format!(
                "#[anemone_rs::signal_handler] expects `*const {}` here",
                expected_ident
            ),
        ));
    };

    if mutability.is_some() || !matches_type_path_ident(elem, expected_ident) {
        return Err(Error::new(
            ty.span(),
            format!(
                "#[anemone_rs::signal_handler] expects `*const {}` here",
                expected_ident
            ),
        ));
    }

    Ok(())
}

fn matches_type_path_ident(ty: &Type, expected_ident: &str) -> bool {
    let Type::Path(type_path) = ty else {
        return false;
    };

    type_path.qself.is_none()
        && type_path
            .path
            .segments
            .last()
            .is_some_and(|segment| segment.ident == expected_ident)
}

fn expand_signal_handler(function: ItemFn, handler_kind: HandlerKind) -> proc_macro2::TokenStream {
    let attrs = function.attrs.clone();
    let vis = function.vis.clone();
    let output = function.sig.output.clone();
    let wrapper_ident = function.sig.ident.clone();
    let inner_ident = format_ident!("__signal_handler_wrapper_{}", wrapper_ident);

    let mut inner_function = function;
    inner_function.attrs.clear();
    inner_function.vis = syn::Visibility::Inherited;
    inner_function.sig.ident = inner_ident.clone();

    let call = match handler_kind {
        HandlerKind::SignoOnly => {
            let invoke = invoke_inner(
                &inner_function.sig.unsafety,
                &inner_ident,
                quote!(::anemone_rs::os::linux::process::signal::SigNo::new(
                    __raw_signo
                )),
            );
            quote! {
                #(#attrs)*
                #vis extern "C" fn #wrapper_ident(__raw_signo: usize) #output {
                    #invoke
                }

                #inner_function
            }
        },
        HandlerKind::SigInfo => {
            let invoke = invoke_inner(
                &inner_function.sig.unsafety,
                &inner_ident,
                quote!(
                    ::anemone_rs::os::linux::process::signal::SigNo::new(__raw_signo),
                    __siginfo,
                    __ucontext
                ),
            );
            quote! {
                #(#attrs)*
                #vis extern "C" fn #wrapper_ident(
                    __raw_signo: usize,
                    __siginfo: *const ::anemone_rs::abi::process::linux::signal::SigInfo,
                    __ucontext: *const ::anemone_rs::abi::process::linux::ucontext::UContext,
                ) #output {
                    #invoke
                }

                #inner_function
            }
        },
    };

    call
}

fn invoke_inner(
    unsafety: &Option<syn::token::Unsafe>,
    inner_ident: &Ident,
    args: proc_macro2::TokenStream,
) -> proc_macro2::TokenStream {
    if unsafety.is_some() {
        quote!(unsafe { #inner_ident(#args) })
    } else {
        quote!(#inner_ident(#args))
    }
}
