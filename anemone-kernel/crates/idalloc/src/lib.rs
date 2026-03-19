//! A crate for allocating unique IDs, which can be used for various purposes
//! such as device IDs, process IDs, etc.
//!
//! We use [`u64`] as the ID type, which allows for various strategies for ID
//! allocation, such as bitmaps, generational IDs, or simple counters.

#![cfg_attr(not(test), no_std)]
#![allow(unused)]

extern crate alloc;

use core::marker::PhantomData;

/// Strategy interface for allocating and freeing `u64` IDs.
pub trait AllocStrategy: Sized {
    /// Allocate one ID, or return `None` when exhausted.
    fn alloc(&mut self) -> Option<u64>;
    /// Release a previously allocated ID.
    fn dealloc(&mut self, id: u64);

    // TODO: add some methods to query the state of the allocator.
}

pub trait AllocStrategyWithReserve: AllocStrategy {
    fn try_reserve(&mut self, id: u64) -> Result<(), ()>;
}

/// Converts between raw `u64` IDs and domain-specific types.
///
/// Note that [`Bijection`] is not equal to From + Into.
pub trait Bijection {
    type X;
    type Y;

    /// Map from raw ID to domain type.
    fn forward(x: Self::X) -> Self::Y;
    /// Map from domain type back to raw ID.
    fn backward(y: Self::Y) -> Self::X;
}

/// Type-safe wrapper around an allocation strategy plus a bijection.
#[derive(Debug)]
pub struct IdAllocator<S, B>
where
    S: AllocStrategy,
    B: Bijection<X = u64>,
{
    strategy: S,
    _marker: PhantomData<B>,
}

impl<S, B> IdAllocator<S, B>
where
    S: AllocStrategy,
    B: Bijection<X = u64>,
{
    /// Create a new allocator using the given strategy.
    pub fn new(strategy: S) -> Self {
        Self {
            strategy,
            _marker: PhantomData,
        }
    }

    /// Allocate one ID in the domain type.
    pub fn alloc(&mut self) -> Option<B::Y> {
        self.strategy.alloc().map(B::forward)
    }

    /// Deallocate one ID in the domain type.
    pub fn dealloc(&mut self, id: B::Y) {
        let raw_id = B::backward(id);
        self.strategy.dealloc(raw_id);
    }
}

/// Type-safe wrapper around an allocation strategy with reservation support
/// plus a bijection.
#[derive(Debug)]
pub struct IdAllocatorWithReserve<S, B>
where
    S: AllocStrategyWithReserve,
    B: Bijection<X = u64>,
{
    strategy: S,
    _marker: PhantomData<B>,
}

impl<S, B> IdAllocatorWithReserve<S, B>
where
    S: AllocStrategyWithReserve,
    B: Bijection<X = u64>,
{
    pub fn new(strategy: S) -> Self {
        Self {
            strategy,
            _marker: PhantomData,
        }
    }

    pub fn alloc(&mut self) -> Option<B::Y> {
        self.strategy.alloc().map(B::forward)
    }

    pub fn dealloc(&mut self, id: B::Y) {
        let raw_id = B::backward(id);
        self.strategy.dealloc(raw_id);
    }

    /// Try to reserve the given ID in the domain type. Returns `Err(())` if the
    /// ID is already allocated or reserved, or if the ID is out of range.
    pub fn try_reserve(&mut self, id: B::Y) -> Result<(), ()> {
        let raw_id = B::backward(id);
        self.strategy.try_reserve(raw_id)
    }
}

/// To use this predefined bijection, ensure there indeed exists a lossless
/// conversion between the domain type and `u64`.
#[derive(Debug)]
pub struct IdentityBijection<T: Clone + Copy + Into<u64> + From<u64>>(PhantomData<T>);

impl<T: Clone + Copy + Into<u64> + From<u64>> Bijection for IdentityBijection<T> {
    type X = u64;
    type Y = T;

    fn forward(x: Self::X) -> Self::Y {
        T::from(x)
    }

    fn backward(y: Self::Y) -> Self::X {
        y.into()
    }
}

mod stack;
pub use stack::StackedAlloc;
mod bitmap;
pub use bitmap::BitmapAlloc;
mod oneshot;
pub use oneshot::{OneShotAlloc, OneShotAllocWithReserve};

#[cfg(test)]
mod tests {
    use crate::stack::StackedAlloc;

    use super::*;

    #[derive(Clone, Copy, PartialEq, Eq, Debug)]
    struct EntityId(u64);

    struct EIdBijection;

    impl Bijection for EIdBijection {
        type X = u64;
        type Y = EntityId;

        fn forward(x: Self::X) -> Self::Y {
            EntityId(x)
        }

        fn backward(y: Self::Y) -> Self::X {
            y.0
        }
    }

    #[test]
    fn test_id_allocator() {
        let mut alloc = IdAllocator::<StackedAlloc, EIdBijection>::new(StackedAlloc::new(0));
        let id1 = alloc.alloc().unwrap();
        let id2 = alloc.alloc().unwrap();
        assert_eq!(id1, EntityId(0));
        assert_eq!(id2, EntityId(1));
    }
}
