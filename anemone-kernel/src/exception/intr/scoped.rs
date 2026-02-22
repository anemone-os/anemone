use crate::prelude::*;

/// An RAII guard that restores the previous IRQ flags when dropped.
///
/// When dropped, it will also decrement the hardirq count.
///
/// For most cases, users should use [IntrGuard] instead, and this one is only
/// intended for use in hardirq handlers where we need to track hardirq count.
#[derive(Debug)]
pub struct TrackedIntrGuard {
    flags: IrqFlags,
}

impl TrackedIntrGuard {
    /// Create a new TrackedIntrGuard by disabling local interrupts and
    /// incrementing the hardirq count.
    pub fn new() -> Self {
        let prev_flags = CurExceptionArch::current_irq_flags();

        // order matters: disable interrupts first and then increment the counter.
        unsafe {
            CurExceptionArch::local_intr_disable();
        }
        with_core_local_mut(|core_local| {
            core_local.preempt_counter_mut().increment_hardirq_count();
        });

        Self { flags: prev_flags }
    }
}

impl Drop for TrackedIntrGuard {
    fn drop(&mut self) {
        unsafe {
            with_core_local_mut(|core_local| {
                core_local.preempt_counter_mut().decrement_hardirq_count();
            });
            CurExceptionArch::restore_local_intr(self.flags);
        }
    }
}

/// An RAII guard that restores the previous IRQ flags when dropped.
#[derive(Debug)]
pub struct IntrGuard {
    flags: IrqFlags,
}

impl IntrGuard {
    /// Create a new IntrGuard by disabling local interrupts.
    pub fn new() -> Self {
        let prev_flags = CurExceptionArch::current_irq_flags();

        unsafe {
            CurExceptionArch::local_intr_disable();
        }

        Self { flags: prev_flags }
    }
}

impl Drop for IntrGuard {
    fn drop(&mut self) {
        unsafe {
            CurExceptionArch::restore_local_intr(self.flags);
        }
    }
}
