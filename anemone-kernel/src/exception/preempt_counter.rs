use crate::prelude::*;

/// Preempt counter for tracking preemption state in the kernel.
#[derive(Debug)]
#[repr(transparent)]
pub struct PreemptCounter(AtomicUsize);

impl PreemptCounter {
    pub const ZEROED: PreemptCounter = PreemptCounter(AtomicUsize::new(0));
    pub unsafe fn increase(&self) -> usize {
        self.0.fetch_add(1, Ordering::SeqCst) + 1
    }

    pub unsafe fn decrease(&self) -> usize {
        let val = self.0.fetch_sub(1, Ordering::SeqCst).wrapping_sub(1);
        if val == usize::MAX {
            panic!("try to decrease a already cleared preempt counter");
        }
        val
    }

    pub fn disable_preempt_with<F: Fn() -> R, R>(&self, f: F) -> R {
        unsafe { self.increase() };
        let res = f();
        unsafe { self.decrease() };
        res
    }

    pub fn allow(&self) -> bool {
        self.0.load(Ordering::SeqCst) == 0
    }
}

#[derive(Debug)]
pub struct PreemptGuard;

impl PreemptGuard {
    pub fn new() -> Self {
        unsafe {
            unsafe_with_core_local(|local| local.preempt_counter().increase());
        }
        Self
    }
}

impl Drop for PreemptGuard {
    fn drop(&mut self) {
        with_intr_disabled(|| {
            if unsafe {
                unsafe_with_core_local(|local| local.preempt_counter().decrease() == 0)
                    && fetch_clear_resched_flag()
            } {
                unsafe {
                    schedule();
                }
            }
        });
    }
}
