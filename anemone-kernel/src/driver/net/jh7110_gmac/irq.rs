use core::sync::atomic::{AtomicU32, Ordering};

use super::{
    regs::GmacRegs,
    ring::{GmacRings, RingError, RxReservation, TxCompletion},
};
use crate::{device::net::RecheckWake, prelude::*, utils::any_opaque::AnyOpaque};

/// Per-node IRQ state. The rings are retained here because the IRQ core has
/// no `free_irq()`; a registered handler can outlive a failed probe or attach
/// attempt until device reset/power-off. `pending` is the durable recheck fact
/// published after the device cause has been acknowledged.
pub(super) struct GmacIrqContext {
    regs: Arc<GmacRegs>,
    rings: SpinLock<GmacRings>,
    pending: RecheckSignal,
}

#[derive(Opaque)]
struct GmacIrqPrivate {
    context: Arc<GmacIrqContext>,
}

struct RecheckSignal {
    causes: AtomicU32,
    wake: spin::Once<Weak<dyn RecheckWake>>,
}

impl RecheckSignal {
    fn new() -> Self {
        Self {
            causes: AtomicU32::new(0),
            wake: spin::Once::new(),
        }
    }

    fn install_wake(&self, wake: Weak<dyn RecheckWake>) {
        assert!(
            self.wake.get().is_none(),
            "JH7110 GMAC recheck wake installed twice"
        );
        self.wake.call_once(|| wake);
    }

    fn publish(&self, causes: u32) {
        assert_ne!(causes, 0);
        let previous = self.causes.fetch_or(causes, Ordering::Release);
        if previous == 0
            && let Some(wake) = self.wake.get().and_then(Weak::upgrade)
        {
            wake.wake();
        }
    }

    fn requested(&self) -> bool {
        self.causes.load(Ordering::Acquire) != 0
    }

    fn take(&self) -> u32 {
        self.causes.swap(0, Ordering::AcqRel)
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
            pending: RecheckSignal::new(),
        })
    }

    pub(super) fn private(self: &Arc<Self>) -> AnyOpaque {
        AnyOpaque::new(GmacIrqPrivate {
            context: self.clone(),
        })
    }

    pub(super) fn suppress_device(&self) {
        self.regs.disable_device_interrupts();
        self.regs.acknowledge_dma_causes();
        self.regs.stop_dma();
    }

    pub(super) fn start_device(&self) {
        self.regs.start_dma();
        self.regs.enable_dma_interrupts();
    }

    pub(super) fn take_pending(&self) -> u32 {
        self.pending.take()
    }

    pub(super) fn install_recheck_wake(&self, wake: Weak<dyn RecheckWake>) {
        self.pending.install_wake(wake);
    }

    pub(super) fn recheck_requested(&self) -> bool {
        self.pending.requested()
    }

    pub(super) fn frame_capacity(&self) -> usize {
        self.rings.lock_irqsave().frame_capacity()
    }

    pub(super) fn ring_size(&self) -> usize {
        self.rings.lock_irqsave().ring_size()
    }

    pub(super) fn reserve_tx(&self) -> Option<usize> {
        self.rings.lock_irqsave().reserve_tx()
    }

    pub(super) fn cancel_tx(&self, index: usize) -> Result<(), RingError> {
        self.rings.lock_irqsave().cancel_tx(index)
    }

    pub(super) fn commit_tx_with<R>(
        &self,
        index: usize,
        length: usize,
        fill: impl FnOnce(&mut [u8]) -> R,
    ) -> Result<R, RingError> {
        let (ptr, length) = {
            let mut rings = self.rings.lock_irqsave();
            rings.tx_frame_parts(index, length)?
        };
        // The reservation is the exclusive owner of this stable DMA backing.
        // No ring lock may cross the protocol callback: paired RX can consume
        // its TX token from inside that callback.
        // SAFETY: `tx_frame_parts` validates the live reservation and returns
        // the corresponding stable allocation; commit remains the only path
        // that publishes this descriptor to the device.
        let result = fill(unsafe { core::slice::from_raw_parts_mut(ptr, length) });
        let tail = self.rings.lock_irqsave().commit_tx(index, length)?;
        // `commit_tx()` orders descriptor OWN after payload/metadata. The
        // MMIO write adds the RISC-V memory-to-device fence before doorbell.
        self.regs.update_tx_tail(tail);
        Ok(result)
    }

    pub(super) fn reclaim_tx(&self) -> Result<Option<TxCompletion>, RingError> {
        self.rings.lock_irqsave().reclaim_tx()
    }

    pub(super) fn reserve_rx(&self) -> Option<RxReservation> {
        self.rings.lock_irqsave().reserve_rx()
    }

    pub(super) fn cancel_rx(&self, index: usize) -> Result<(), RingError> {
        self.rings.lock_irqsave().cancel_rx(index)
    }

    pub(super) fn discard_rx(&self, index: usize) -> Result<(), RingError> {
        let tail = self.rings.lock_irqsave().discard_rx(index)?;
        // Refill publishes OWN before the MMIO tail update.
        self.regs.update_rx_tail(tail);
        Ok(())
    }

    pub(super) fn consume_rx<R>(
        &self,
        index: usize,
        consume: impl FnOnce(&[u8]) -> R,
    ) -> Result<R, RingError> {
        let (ptr, length) = {
            let mut rings = self.rings.lock_irqsave();
            rings.rx_frame_parts(index)?
        };
        // OWN was cleared before reservation. The callback runs without the
        // ring lock so a paired TX token can acquire it and commit a response.
        // SAFETY: `rx_frame_parts` validates the completed reservation; refill
        // is deferred until after the callback returns.
        let result = consume(unsafe { core::slice::from_raw_parts(ptr, length) });
        let tail = self.rings.lock_irqsave().complete_rx(index)?;
        // Refill publishes OWN before the MMIO tail update.
        self.regs.update_rx_tail(tail);
        Ok(result)
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
    use core::sync::atomic::AtomicUsize;

    use super::*;

    struct RecordingWake(AtomicUsize);

    impl RecheckWake for RecordingWake {
        fn wake(&self) {
            self.0.fetch_add(1, Ordering::Relaxed);
        }
    }

    #[kunit]
    fn pending_causes_are_durable_coalesced_and_consumed_once() {
        let pending = RecheckSignal::new();
        let recording = Arc::new(RecordingWake(AtomicUsize::new(0)));
        let wake: Arc<dyn RecheckWake> = recording.clone();
        pending.install_wake(Arc::downgrade(&wake));
        pending.publish(1 << 6);
        pending.publish(1 << 12);
        assert_eq!(recording.0.load(Ordering::Relaxed), 1);
        assert!(pending.requested());
        assert_eq!(pending.take(), (1 << 6) | (1 << 12));
        pending.publish(1 << 2);
        assert_eq!(recording.0.load(Ordering::Relaxed), 2);
        assert_eq!(pending.take(), 1 << 2);
        assert_eq!(pending.take(), 0);
    }
}
