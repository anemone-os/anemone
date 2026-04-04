use core::{
    fmt::{Debug, Display},
    sync::atomic::{AtomicU32, Ordering},
};

/// temporary id allocator
/// todo: use [idalloc::IdAllocator] instead of this
static ID_ALLOC: AtomicU32 = AtomicU32::new(1);

unsafe fn alloc_tid_raw() -> u32 {
    ID_ALLOC.fetch_add(1, Ordering::SeqCst)
}

unsafe fn free_tid_raw(id: u32) {
    // do nothing for now
}

#[repr(transparent)]
pub struct Tid(u32);

pub const TID_IDLE: TidHandle = TidHandle(0);

impl Tid {
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
    pub fn get(&self) -> u32 {
        self.0
    }
}

impl Drop for TidHandle {
    fn drop(&mut self) {
        if self.0 != 0 {
            unsafe {
                free_tid_raw(self.0);
            }
        }
    }
}

pub fn alloc_tid() -> TidHandle {
    let id = unsafe { alloc_tid_raw() };
    TidHandle(id)
}
