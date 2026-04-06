use core::{any::Any, fmt::Debug};

use alloc::boxed::Box;
use kernel_macros::Opaque;

/// `Opaque` trait is used to represent private data associated with various
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
/// You might also argue that `Any` isn't a zero-cost abstraction, as it
/// introduces some runtime overhead for type checking and downcasting. Actually
/// this is not true. `Any` indeed provides some unsafe unchecked downcasting
/// methods without runtime type checking, which, in turn, are the same as using
/// raw pointers in C.
pub trait Opaque: Any + Sync + Send {}

impl dyn Opaque {
    fn cast<T: Opaque>(&self) -> Option<&T> {
        (self as &dyn Any).downcast_ref::<T>()
    }

    fn cast_mut<T: Opaque>(&mut self) -> Option<&mut T> {
        (self as &mut dyn Any).downcast_mut::<T>()
    }

    unsafe fn cast_unchecked<T: Opaque>(&self) -> &T {
        unsafe { (self as &dyn Any).downcast_unchecked_ref() }
    }

    unsafe fn cast_unchecked_mut<T: Opaque>(&mut self) -> &mut T {
        unsafe { (self as &mut dyn Any).downcast_unchecked_mut() }
    }
}

impl Debug for dyn Opaque {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        write!(f, "dyn Opaque")
    }
}

/// Represents a type-erased opaque data. This is used to store private data for
/// kernel objects without requiring a common base type.
///
/// The name comes from Zig's `anyopaque` type, which serves a similar purpose.
///
/// See [Opaque] for more details.
#[derive(Debug)]
pub struct AnyOpaque(Box<dyn Opaque>);

impl AnyOpaque {
    pub fn new<T: Opaque>(data: T) -> Self {
        Self(Box::new(data))
    }

    pub fn cast<T: Opaque>(&self) -> Option<&T> {
        self.0.cast::<T>()
    }

    pub fn cast_mut<T: Opaque>(&mut self) -> Option<&mut T> {
        self.0.cast_mut::<T>()
    }

    pub unsafe fn cast_unchecked<T: Opaque>(&self) -> &T {
        unsafe { self.0.cast_unchecked::<T>() }
    }

    pub unsafe fn cast_unchecked_mut<T: Opaque>(&mut self) -> &mut T {
        unsafe { self.0.cast_unchecked_mut::<T>() }
    }
}

/// You just need a placeholder, and don't want any actual data? Use this.
#[derive(Debug, Opaque)]
pub struct NilOpaque(());

impl NilOpaque {
    pub fn new() -> AnyOpaque {
        AnyOpaque::new(NilOpaque(()))
    }
}
