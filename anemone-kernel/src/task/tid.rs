//! Task ID management.

use core::fmt::{Debug, Display};

use idalloc::{Bijection, BitmapAlloc, IdAllocator};

use crate::prelude::*;

struct TidBijection;

impl Bijection for TidBijection {
    type X = u64;
    type Y = Tid;

    fn forward(x: Self::X) -> Self::Y {
        debug_assert!(x <= u32::MAX as u64);
        Tid(x as u32)
    }

    fn backward(y: Self::Y) -> Self::X {
        y.0 as u64
    }
}

static ID_ALLOC: Lazy<SpinLock<IdAllocator<BitmapAlloc, TidBijection>>> =
    Lazy::new(|| SpinLock::new(IdAllocator::new(BitmapAlloc::new(1, MAX_PROCESSES))));

#[repr(transparent)]
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Tid(u32);

impl Tid {
    pub const IDLE: Self = Self(0);
    pub const INVALID: Self = {
        const_assert!(
            MAX_PROCESSES < u32::MAX as u64,
            "wrong kconfig: MAX_PROCESSES is too large"
        );
        Self(u32::MAX)
    };
    pub const INIT: Self = Self(1);

    #[inline(always)]
    pub fn new(id: u32) -> Self {
        Self(id)
    }

    #[inline(always)]
    pub fn get(&self) -> u32 {
        self.0
    }
}

impl Debug for Tid {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.write_fmt(format_args!("task #{}", self.0))
    }
}

impl Display for Tid {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.write_fmt(format_args!("task #{}", self.0))
    }
}

pub struct TidHandle(u32);

impl Debug for TidHandle {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.write_fmt(format_args!("task #{}", self.0))
    }
}

impl TidHandle {
    pub const IDLE: Self = Self(0);
    pub const INVALID: Self = {
        const_assert!(
            MAX_PROCESSES < u32::MAX as u64,
            "wrong kconfig: MAX_PROCESSES is too large"
        );
        Self(u32::MAX)
    };

    pub fn get(&self) -> u32 {
        self.0
    }
}

impl Drop for TidHandle {
    fn drop(&mut self) {
        if self.0 != 0 {
            ID_ALLOC.lock_irqsave().dealloc(Tid(self.0));
        } else {
            panic!("dropping idle task's TidHandle");
        }
    }
}

/// Allocate a new [TidHandle]. Returns `None` if the maximum number of
/// processes has been reached. (which is [`MAX_PROCESSES`])
pub fn alloc_tid() -> Option<TidHandle> {
    ID_ALLOC.lock().alloc().map(|tid| TidHandle(tid.get()))
}
