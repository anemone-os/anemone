//! Task ID management.

use core::{
    fmt::{Debug, Display},
    sync::atomic::{AtomicBool, Ordering},
};

use idalloc::{Bijection, BitmapAlloc, IdAllocator};

use crate::{prelude::*, syscall::handler::TryFromSyscallArg};

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

const ORDINARY_TID_START: u64 = 3;
const ORDINARY_TID_CAPACITY: u64 = MAX_PROCESSES - ORDINARY_TID_START + 1;

static ID_ALLOC: Lazy<SpinLock<IdAllocator<BitmapAlloc, TidBijection>>> = Lazy::new(|| {
    SpinLock::new(IdAllocator::new(BitmapAlloc::new(
        ORDINARY_TID_START,
        ORDINARY_TID_CAPACITY,
    )))
});

static INIT_TID_CONSUMED: AtomicBool = AtomicBool::new(false);
static KTHREADD_TID_CONSUMED: AtomicBool = AtomicBool::new(false);

#[repr(transparent)]
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Tid(u32);

impl Tid {
    pub const IDLE: Self = Self(0);
    pub const INVALID: Self = {
        const_assert!(
            MAX_PROCESSES >= ORDINARY_TID_START,
            "wrong kconfig: MAX_PROCESSES must include fixed kernel TIDs"
        );
        const_assert!(
            MAX_PROCESSES < u32::MAX as u64,
            "wrong kconfig: MAX_PROCESSES is too large"
        );
        Self(u32::MAX)
    };
    pub const INIT: Self = Self(1);
    pub const KTHREADD: Self = Self(2);

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

impl TryFromSyscallArg for Tid {
    fn try_from_syscall_arg(raw: u64) -> Result<Self, SysError> {
        let raw = u32::try_from_syscall_arg(raw)?;
        Ok(Self(raw))
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

    pub fn get_typed(&self) -> Tid {
        Tid(self.0)
    }

    pub fn get(&self) -> u32 {
        self.0
    }
}

impl Drop for TidHandle {
    fn drop(&mut self) {
        if self.0 == 0 {
            panic!("dropping idle task's TidHandle");
        }

        if self.0 == Tid::INIT.get() || self.0 == Tid::KTHREADD.get() {
            return;
        }

        // With reuse disabled, allocation bits intentionally remain set so a
        // numeric TID identifies at most one task during the whole boot. This
        // diagnostic policy trades reuse for eventual exhaustion at
        // MAX_PROCESSES; enable `tid_reuse` to restore ordinary recycling.
        if cfg!(feature = "tid_reuse") {
            ID_ALLOC.lock_irqsave().dealloc(Tid(self.0));
        }
    }
}

/// Allocate a new [TidHandle]. Returns `None` if the maximum number of
/// processes has been reached. (which is [`MAX_PROCESSES`])
pub fn alloc_tid() -> Option<TidHandle> {
    ID_ALLOC.lock().alloc().map(|tid| TidHandle(tid.get()))
}

/// Allocate the boot init task's fixed TID.
///
/// TID 1 is outside the ordinary allocator. Only the BSP bootstrap path should
/// consume this handle; secondary init-thread members use ordinary TIDs while
/// joining TGID 1.
pub(crate) fn alloc_init_tid() -> TidHandle {
    assert!(
        !INIT_TID_CONSUMED.swap(true, Ordering::AcqRel),
        "init fixed TID handle consumed more than once"
    );
    TidHandle(Tid::INIT.get())
}

/// Allocate `kthreadd`'s fixed TID.
///
/// TID 2 is reserved for `kthreadd` and never enters the ordinary allocator.
/// This deliberately does not generalize into `reserve_tid(Tid)`.
pub(in crate::task) fn alloc_kthreadd_tid() -> TidHandle {
    assert!(
        !KTHREADD_TID_CONSUMED.swap(true, Ordering::AcqRel),
        "kthreadd fixed TID handle consumed more than once"
    );
    TidHandle(Tid::KTHREADD.get())
}
