use core::{
    fmt::{Debug, Display},
    sync::atomic::{AtomicUsize, Ordering},
};

/// temporary id allocator
/// todo: use [idalloc::IdAllocator] instead of this
static ID_ALLOC: AtomicUsize = AtomicUsize::new(0);

unsafe fn alloc_tid_raw() -> usize {
    ID_ALLOC.fetch_add(1, Ordering::SeqCst)
}

unsafe fn free_tid_raw(id: usize) {
    // do nothing for now
}

pub struct Tid(usize);

impl Tid {
    #[inline(always)]
    pub fn new(id: usize) -> Self {
        Self(id)
    }

    #[inline(always)]
    pub fn get(&self) -> usize {
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

pub struct TidHandle(usize);

impl Debug for TidHandle {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.write_fmt(format_args!("task #{}", self.0))
    }
}

impl TidHandle {
    pub fn get(&self) -> usize {
        self.0
    }
}

impl Drop for TidHandle {
    fn drop(&mut self) {
        unsafe {
            free_tid_raw(self.0);
        }
    }
}

pub fn alloc_tid() -> TidHandle {
    let id = unsafe { alloc_tid_raw() };
    TidHandle(id)
}
