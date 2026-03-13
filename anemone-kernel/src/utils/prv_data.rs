use core::{any::Any, fmt::Debug};

/// `PrvData` trait is used to represent private data associated with various
/// kernel objects. It allows for type-erased storage of private data, enabling
/// different types of data to be associated with kernel objects without
/// requiring a common base type.
///
/// However, current implementation is based on the `Any` trait, which has 2
/// limitations:
/// - a `dyn Any` trait object can't be cast to another `dyn SomeTrait` trait
///   object, even if the underlying type implements `SomeTrait`.
/// - In C, we can just use a 8-byte pointer to store any type of data. But in
///   Rust we must use a double-sized fat pointer to store a trait object, which
///   is less efficient.
///
///   You might also argue that `Any` isn't a zero-cost abstraction, as it
///   introduces some runtime overhead for type checking and downcasting.
/// Actually this is not true. `Any` indeed provides some unsafe unchecked
/// downcasting methods without runtime type checking, which, in turn, are the
/// same as using raw pointers in C.
pub trait PrvData: Any + Sync + Send {}

impl dyn PrvData {
    pub fn cast<T: PrvData>(&self) -> Option<&T> {
        todo!()
    }

    pub fn cast_mut<T: PrvData>(&mut self) -> Option<&mut T> {
        todo!()
    }

    pub unsafe fn cast_unchecked<T: PrvData>(&self) -> &T {
        todo!()
    }

    pub unsafe fn cast_unchecked_mut<T: PrvData>(&mut self) -> &mut T {
        todo!()
    }
}

impl Debug for dyn PrvData {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        write!(f, "dyn PrvData")
    }
}
