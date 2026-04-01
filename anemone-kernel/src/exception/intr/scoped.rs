use crate::prelude::*;

/// An RAII guard that restores the previous IRQ flags when dropped.
#[derive(Debug)]
pub struct IntrGuard {
    prev: IrqFlags,
}

impl IntrGuard {
    /// Create a new IntrGuard by disabling local interrupts.
    pub fn new(enable: bool) -> Self {
        let prev_flags = IntrArch::current_irq_flags();

        if enable {
            unsafe { IntrArch::local_intr_enable() };
        } else {
            unsafe { IntrArch::local_intr_disable() };
        }

        Self { prev: prev_flags }
    }
}

impl Drop for IntrGuard {
    fn drop(&mut self) {
        unsafe {
            IntrArch::restore_local_intr(self.prev);
        }
    }
}

pub fn with_intr_disabled<F: FnOnce(bool) -> R, R>(f: F) -> R {
    let guard = IntrGuard::new(false);
    let res = f(guard.prev == IntrArch::ENABLED_IRQ_FLAGS);
    drop(guard);
    res
}

pub fn with_intr_enabled<F: FnOnce(bool) -> R, R>(f: F) -> R {
    let guard = IntrGuard::new(true);
    let res = f(guard.prev == IntrArch::ENABLED_IRQ_FLAGS);
    drop(guard);
    res
}
