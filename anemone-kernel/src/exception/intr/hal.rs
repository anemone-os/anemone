pub trait IntrArchTrait: Sized {
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
