use crate::prelude::*;

/// An RAII guard that restores the previous IRQ flags when dropped.
#[derive(Debug)]
pub struct IntrGuard {
    flags: IrqFlags,
}

impl IntrGuard {
    /// Create a new IntrGuard by disabling local interrupts.
    pub fn new() -> Self {
        let prev_flags = IntrArch::current_irq_flags();

        unsafe {
            IntrArch::local_intr_disable();
        }

        Self { flags: prev_flags }
    }
}

impl Drop for IntrGuard {
    fn drop(&mut self) {
        unsafe {
            IntrArch::restore_local_intr(self.flags);
        }
    }
}
