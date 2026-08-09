use core::sync::atomic::{AtomicU32, Ordering};

use super::{
    regs::GmacRegs,
    ring::{GmacRings, RingError, RxCompletion, TxCompletion},
};
use crate::{prelude::*, utils::any_opaque::AnyOpaque};

/// Per-node IRQ state. The rings are retained here because the IRQ core has
/// no `free_irq()`; a registered handler can outlive a failed probe or attach
/// attempt until device reset/power-off. `pending` is the durable recheck fact
/// published after the device cause has been acknowledged.
pub(super) struct GmacIrqContext {
    regs: Arc<GmacRegs>,
    rings: SpinLock<GmacRings>,
    pending: PendingCauses,
}

#[derive(Opaque)]
struct GmacIrqPrivate {
    context: Arc<GmacIrqContext>,
}

struct PendingCauses(AtomicU32);

impl PendingCauses {
    const fn new() -> Self {
        Self(AtomicU32::new(0))
    }

    fn publish(&self, causes: u32) {
        self.0.fetch_or(causes, Ordering::Release);
    }

    fn take(&self) -> u32 {
        self.0.swap(0, Ordering::Acquire)
    }
}

pub(super) static IRQ_HANDLER: IrqHandler = IrqHandler::new(handle_irq);

impl GmacIrqContext {
    pub(super) fn prepare(regs: Arc<GmacRegs>, rings: GmacRings) -> Arc<Self> {
        regs.disable_device_interrupts();
        regs.acknowledge_dma_causes();
        Arc::new(Self {
            regs,
            rings: SpinLock::new(rings),
            pending: PendingCauses::new(),
        })
    }

    pub(super) fn private(self: &Arc<Self>) -> AnyOpaque {
        AnyOpaque::new(GmacIrqPrivate {
            context: self.clone(),
        })
    }

    pub(super) fn enable(&self) {
        self.regs.enable_dma_interrupts();
    }

    pub(super) fn suppress_device_causes(&self) {
        self.regs.disable_device_interrupts();
        self.regs.acknowledge_dma_causes();
    }

    pub(super) fn take_pending(&self) -> u32 {
        self.pending.take()
    }

    pub(super) fn submit_tx(&self, frame: &[u8]) -> Result<(), RingError> {
        let mut rings = self.rings.lock_irqsave();
        let tail = rings.submit_tx(frame)?;
        // `submit_tx()` orders descriptor OWN after payload/metadata. The
        // MMIO write adds the RISC-V memory-to-device fence before doorbell.
        self.regs.update_tx_tail(tail);
        Ok(())
    }

    pub(super) fn reclaim_tx(&self) -> Result<Option<TxCompletion>, RingError> {
        self.rings.lock_irqsave().reclaim_tx()
    }

    pub(super) fn with_rx_frame<R>(
        &self,
        consume: impl FnOnce(&[u8]) -> R,
    ) -> Option<RxCompletion<R>> {
        let mut rings = self.rings.lock_irqsave();
        let completion = rings.with_rx_frame(consume)?;
        // Refill publishes OWN before the MMIO tail update.
        self.regs.update_rx_tail(completion.tail);
        Some(completion)
    }
}

fn handle_irq(private: &AnyOpaque) {
    let context = private
        .cast::<GmacIrqPrivate>()
        .expect("JH7110 GMAC IRQ received invalid private data");
    let causes = context.context.regs.take_enabled_dma_causes();
    if causes != 0 {
        context.context.pending.publish(causes);
    }
}

#[cfg(feature = "kunit")]
mod kunits {
    use super::*;

    #[kunit]
    fn pending_causes_are_durable_coalesced_and_consumed_once() {
        let pending = PendingCauses::new();
        pending.publish(1 << 6);
        pending.publish(1 << 12);
        assert_eq!(pending.take(), (1 << 6) | (1 << 12));
        assert_eq!(pending.take(), 0);
    }
}
