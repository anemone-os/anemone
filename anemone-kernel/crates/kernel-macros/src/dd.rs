//! Proc macros related to device driver model.

use proc_macro::TokenStream;
use quote::quote;
use syn::{Data, DataStruct, DeriveInput, Error, Fields, parse_macro_input};

macro_rules! base_impl {
    ($([$derive_fn:ident, $derive:ident, $attr_str:literal, $trait:path, $base_ty:path],)*) => {
        $(
            pub fn $derive_fn(input: TokenStream) -> TokenStream {
                let input = parse_macro_input!(input as DeriveInput);
                let name = &input.ident;
                let (impl_generics, ty_generics, where_clause) = input.generics.split_for_impl();

                // find base field
                if let Data::Struct(DataStruct {
                    fields: Fields::Named(fields),
                    ..
                }) = &input.data
                {
                    let mut base_fields = fields
                        .named
                        .iter()
                        .filter(|f| f.attrs.iter().any(|attr| attr.path().is_ident($attr_str)));
                    let first = base_fields.next();
                    let second = base_fields.next();
                    match (first, second) {
                        (None, None) => {
                            return Error::new_spanned(
                                name,
                                format!("struct must have a field with #[{}] attribute", $attr_str),
                            )
                            .to_compile_error()
                            .into();
                        },
                        (Some(_), Some(_)) => {
                            return Error::new_spanned(
                                name,
                                format!("struct must have only one field with #[{}] attribute", $attr_str),
                            )
                            .to_compile_error()
                            .into();
                        },
                        (Some(base_field), None) => {
                            let ident = base_field.ident.as_ref().unwrap();
                            let expanded = quote! {
                                impl #impl_generics $trait for #name #ty_generics #where_clause {
                                    fn base(&self) -> &$base_ty {
                                        &self.#ident
                                    }
                                }
                            };
                            return expanded.into();
                        },
                        _ => unreachable!(),
                    }
                }

                Error::new_spanned(&input.ident, "expected struct with named fields")
                    .to_compile_error()
                    .into()
            }
        )*
    };
}

base_impl!(
    [
        kobject_impl,
        KObject,
        "kobject",
        crate::device::kobject::KObjectData,
        crate::device::kobject::KObjectBase
    ],
    [
        device_impl,
        Device,
        "device",
        crate::device::DeviceData,
        crate::device::DeviceBase
    ],
    [
        driver_impl,
        Driver,
        "driver",
        crate::driver::DriverData,
        crate::driver::DriverBase
    ],
);

pub fn drv_state_impl(input: TokenStream) -> TokenStream {
    let input = parse_macro_input!(input as DeriveInput);
    let name = &input.ident;
    let expanded = quote! {
        impl crate::driver::DriverState for #name {}
    };
    expanded.into()
}
