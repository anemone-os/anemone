use alloc::sync::Arc;

use super::regs::GmacRegs;

/// Device-local IRQ capability kept separate from the generic controller flow.
/// Gate 0 constructs only the disabled baseline; Gate 1 becomes its first
/// production consumer after ring ownership and handler cause handling exist.
pub(super) struct GmacIrqContext {
    regs: Arc<GmacRegs>,
}

impl GmacIrqContext {
    pub(super) fn prepare(regs: Arc<GmacRegs>) -> Self {
        regs.disable_device_interrupts();
        regs.acknowledge_dma_causes();
        Self { regs }
    }

    #[allow(dead_code)]
    // Gate 1 uses this after a real IRQ registration; Gate 0 establishes the
    // same baseline during construction and never registers the interrupt.
    pub(super) fn disable(&self) {
        self.regs.disable_device_interrupts();
    }

    #[allow(dead_code)]
    // Gate 1 uses this from the device-cause cleanup path.
    pub(super) fn acknowledge(&self) {
        self.regs.acknowledge_dma_causes();
    }

    #[allow(dead_code)]
    // Gate 1 removes this allowance when the non-empty handler is registered.
    pub(super) fn enable(&self, mac_mask: u32, dma_mask: u32) {
        self.regs.enable_device_interrupts(mac_mask, dma_mask);
    }
}
