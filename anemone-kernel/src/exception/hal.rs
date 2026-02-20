// TODO: add docs and comments.

pub use crate::prelude::*;

pub trait ExceptionArch: Sized {
    /// IRQ flags representing the state with interrupts enabled.
    const ENABLED_IRQ_FLAGS: IrqFlags;

    /// IRQ flags representing the state with interrupts disabled.
    const DISABLED_IRQ_FLAGS: IrqFlags;

    /// Get the current IRQ flags.
    fn current_irq_flags() -> IrqFlags;

    /// Restore local interrupt state from flags.
    ///
    /// This is a low-level primitive and must not change any software counter.
    unsafe fn restore_local_intr(flags: IrqFlags);

    /// Enable local interrupts.
    ///
    /// This is a low-level primitive and must not change any software counter.
    unsafe fn local_intr_enable() {
        unsafe {
            Self::restore_local_intr(Self::ENABLED_IRQ_FLAGS);
        }
    }

    /// Disable local interrupts.
    ///
    /// This is a low-level primitive and must not change any software counter.
    unsafe fn local_intr_disable() {
        unsafe {
            Self::restore_local_intr(Self::DISABLED_IRQ_FLAGS);
        }
    }
}

/// Interrupt flags for the current CPU. The exact representation is
/// architecture-specific.
///
/// This type should be treated as opaque, and users should not rely on its
/// internal structure.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct IrqFlags(u64);

impl IrqFlags {
    /// Create a new IrqFlags with the given raw value.
    pub const fn new(raw: u64) -> Self {
        Self(raw)
    }

    /// Get the raw value of this IrqFlags.
    pub const fn raw(&self) -> u64 {
        self.0
    }
}

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
