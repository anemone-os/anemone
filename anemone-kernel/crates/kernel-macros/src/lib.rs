use proc_macro::TokenStream;

mod dd;
mod initcall;
mod kunit;
mod percpu;

/// Defines a per-CPU variable.
///
/// The variable must be a static item.
#[proc_macro_attribute]
pub fn percpu(attr: TokenStream, item: TokenStream) -> TokenStream {
    percpu::percpu_impl(attr, item)
}

/// Defines a KUnit test case.
///
/// The test function must have the signature `fn()`.
#[proc_macro_attribute]
pub fn kunit(attr: TokenStream, item: TokenStream) -> TokenStream {
    kunit::kunit_impl(attr, item)
}

/// Defines an initcall.
///
/// The function must have the signature `fn()`.
///
/// Currently supported levels are:
/// - `driver`
#[proc_macro_attribute]
pub fn initcall(attr: TokenStream, item: TokenStream) -> TokenStream {
    initcall::initcall_impl(attr, item)
}

/// Derives the `KObjectData` trait for a struct.
///
/// The struct must have exactly one field with the `#[kobject]` attribute,
/// which is used as the base of the kobject. The field must be of type
/// `KObjectBase`.
#[proc_macro_derive(KObject, attributes(kobject))]
pub fn kobject_derive(input: TokenStream) -> TokenStream {
    dd::kobject_impl(input)
}

/// Derives the `DeviceData` trait for a struct.
///
/// The struct must have exactly one field with the `#[device]` attribute, which
/// is used as the base of the device. The field must be of type `DeviceBase`.
#[proc_macro_derive(Device, attributes(device))]
pub fn device_derive(input: TokenStream) -> TokenStream {
    dd::device_impl(input)
}

/// Derives the `DriverData` trait for a struct.
///
/// The struct must have exactly one field with the `#[driver]` attribute, which
/// is used as the base of the driver. The field must be of type `DriverBase`.
#[proc_macro_derive(Driver, attributes(driver))]
pub fn driver_impl(input: TokenStream) -> TokenStream {
    dd::driver_impl(input)
}

/// Derives the `DriverState` trait for a struct, which allows it to be used as
/// the state of a device driver.
#[proc_macro_derive(DriverState)]
pub fn drv_state_derive(input: TokenStream) -> TokenStream {
    dd::drv_state_impl(input)
}
