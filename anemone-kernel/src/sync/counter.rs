use core::sync::atomic::{AtomicUsize, Ordering};

use crate::prelude::*;

pub struct CpuSync {
    inner: AtomicUsize,
    name: &'static str,
}

impl CpuSync {
    pub const fn new(name: &'static str) -> Self {
        Self {
            inner: AtomicUsize::new(0),
            name,
        }
    }
    /// Synchronize all CPUs to ensure they have all executed the code up to
    /// this point.
    #[inline(never)]
    pub unsafe fn sync_with_counter(&self) {
        let ncpus = CpuArch::ncpus();
        if self.inner.fetch_add(1, Ordering::SeqCst) + 1 == ncpus {
            knoticeln!("Counter '{}' synchronized", self.name);
        }

        while self.inner.load(Ordering::SeqCst) < ncpus {
            core::hint::spin_loop();
        }
    }
}
