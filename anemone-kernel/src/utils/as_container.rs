/// A trait for defining a container type that contains an inner type.
/// This is useful for those types that have only one 'inner' field, and we want
/// to be able to convert between the container and the inner type easily.
///
/// However, for those types that have multiple inner fields with the same type,
/// this trait is not suitable. This trait is not intended to be used for those
/// types.

pub unsafe trait AsContainer<Inner> {
    unsafe fn from_inner(inner: *const Inner) -> *const Self;
    unsafe fn from_inner_mut(inner: *mut Inner) -> *mut Self;
}

macro_rules! impl_as_container {
    ($container:ty, $inner_name:ident => $inner_type:ty) => {
        unsafe impl AsContainer<$inner_type> for $container {
            unsafe fn from_inner(inner: *const $inner_type) -> *const Self {
                let offset = core::mem::offset_of!($container, $inner_name);
                inner.cast::<u8>().wrapping_sub(offset).cast()
            }
            unsafe fn from_inner_mut(inner: *mut $inner_type) -> *mut Self {
                let offset = core::mem::offset_of!($container, $inner_name);
                inner.cast::<u8>().wrapping_sub(offset).cast()
            }
        }
    };
}
