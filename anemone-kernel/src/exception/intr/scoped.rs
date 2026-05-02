use crate::prelude::*;

/// An RAII guard that restores the previous IRQ flags when dropped.
#[derive(Debug)]
pub struct IntrGuard {
    prev: IrqFlags,
}

impl IntrGuard {
    /// Create a new IntrGuard by disabling local interrupts.
    pub fn new() -> Self {
        let prev_flags = IntrArch::current_irq_flags();

        unsafe { IntrArch::local_intr_disable() };

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

/// Run a closure with local interrupts disabled, restoring the previous state
/// afterwards.
pub fn with_intr_disabled<F: FnOnce() -> R, R>(f: F) -> R {
    let guard = IntrGuard::new();
    let res = f();
    drop(guard);
    res
}
