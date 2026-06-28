use proc_macro::TokenStream;
use quote::quote;
use syn::{
    Attribute, Error, GenericArgument, ItemFn, PathArguments, ReturnType, Type, parse_macro_input,
    parse_quote, spanned::Spanned,
};

pub fn main_impl(attr: TokenStream, item: TokenStream) -> TokenStream {
    if !attr.is_empty() {
        return Error::new(
            proc_macro2::Span::call_site(),
            "#[anemone_rs::main] does not accept arguments",
        )
        .to_compile_error()
        .into();
    }

    let mut function = parse_macro_input!(item as ItemFn);

    if let Err(error) = validate_main_signature(&function) {
        return error.to_compile_error().into();
    }

    function
        .attrs
        .push(parse_quote!(#[unsafe(export_name = "anemone_main")]));

    quote!(#function).into()
}

fn validate_main_signature(function: &ItemFn) -> Result<(), Error> {
    if !function.sig.inputs.is_empty() {
        return Err(Error::new(
            function.sig.inputs.span(),
            "#[anemone_rs::main] requires `(pub) fn main() -> Result<(), Errno>` without arguments",
        ));
    }

    if function.sig.asyncness.is_some() {
        return Err(Error::new(
            function.sig.asyncness.span(),
            "#[anemone_rs::main] does not support async functions",
        ));
    }

    if function.sig.constness.is_some() {
        return Err(Error::new(
            function.sig.constness.span(),
            "#[anemone_rs::main] does not support const functions",
        ));
    }

    if function.sig.abi.is_some() {
        return Err(Error::new(
            function.sig.abi.span(),
            "#[anemone_rs::main] expects a plain Rust function",
        ));
    }

    if !function.sig.generics.params.is_empty() || function.sig.generics.where_clause.is_some() {
        return Err(Error::new(
            function.sig.generics.span(),
            "#[anemone_rs::main] does not support generics",
        ));
    }

    if has_export_control_attr(&function.attrs) {
        return Err(Error::new(
            function.span(),
            "#[anemone_rs::main] cannot be combined with #[no_mangle] or #[export_name = ...]",
        ));
    }

    if !matches_main_return(&function.sig.output) {
        return Err(Error::new(
            function.sig.output.span(),
            "#[anemone_rs::main] requires the signature `(pub) fn main() -> Result<(), Errno>`",
        ));
    }

    Ok(())
}

fn has_export_control_attr(attrs: &[Attribute]) -> bool {
    attrs
        .iter()
        .any(|attr| attr.path().is_ident("no_mangle") || attr.path().is_ident("export_name"))
}

fn matches_main_return(output: &ReturnType) -> bool {
    let ReturnType::Type(_, ty) = output else {
        return false;
    };

    let Type::Path(type_path) = ty.as_ref() else {
        return false;
    };

    if type_path.qself.is_some() {
        return false;
    }

    let Some(result_segment) = type_path.path.segments.last() else {
        return false;
    };
    if result_segment.ident != "Result" {
        return false;
    }

    let PathArguments::AngleBracketed(arguments) = &result_segment.arguments else {
        return false;
    };
    if arguments.args.len() != 2 {
        return false;
    }

    let mut arguments = arguments.args.iter();
    let Some(GenericArgument::Type(ok_ty)) = arguments.next() else {
        return false;
    };
    let Some(GenericArgument::Type(err_ty)) = arguments.next() else {
        return false;
    };
    matches!(ok_ty, Type::Tuple(tuple) if tuple.elems.is_empty()) && matches_errno_type(err_ty)
}

fn matches_errno_type(ty: &Type) -> bool {
    let Type::Path(type_path) = ty else {
        return false;
    };

    type_path.qself.is_none()
        && type_path
            .path
            .segments
            .last()
            .is_some_and(|segment| segment.ident == "Errno")
}
