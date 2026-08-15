use core::sync::atomic::{AtomicU32, Ordering};

use super::{
    super::{
        DwmacDeviceControl,
        frame::{DwmacFrameQueue, RxFrameReservation},
    },
    owner::Dwmac1000Owner,
    ring::{RingError, RxReservation},
};
use crate::{device::net::RecheckWake, prelude::*, utils::any_opaque::AnyOpaque};

const IRQ_DIAGNOSTIC_BUDGET: u32 = 128;
const RX_DIAGNOSTIC_BUDGET: u32 = 128;

/// Gate 3 runtime context. It is an extension of the Gate 2 owner, not a
/// second MMIO/ring/device-cause owner; the IRQ descriptor and device state
/// both retain this same context until shutdown or power-off.
pub(super) struct Dwmac1000IrqContext {
    owner: Arc<Dwmac1000Owner>,
    pending: RecheckSignal,
    /// Gate 3 acceptance-only log budgets. These counters are diagnostic
    /// state and never participate in IRQ, worker, or descriptor decisions;
    /// remove or reduce the logs after the traffic proof is closed.
    irq_diagnostic_budget: AtomicU32,
    rx_diagnostic_budget: AtomicU32,
}

#[derive(Opaque)]
struct Dwmac1000IrqPrivate {
    context: Arc<Dwmac1000IrqContext>,
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
            "DWMAC1000 recheck wake installed twice"
        );
        self.wake.call_once(|| wake);
    }

    fn publish(&self, causes: u32) -> (u32, bool) {
        assert_ne!(causes, 0);
        let previous = self.causes.fetch_or(causes, Ordering::Release);
        let mut woke = false;
        if previous == 0
            && let Some(wake) = self.wake.get().and_then(Weak::upgrade)
        {
            wake.wake();
            woke = true;
        }
        (previous, woke)
    }

    fn requested(&self) -> bool {
        self.causes.load(Ordering::Acquire) != 0
    }

    fn snapshot(&self) -> u32 {
        self.causes.load(Ordering::Acquire)
    }

    fn take(&self) -> u32 {
        self.causes.swap(0, Ordering::AcqRel)
    }
}

pub(super) static IRQ_HANDLER: IrqHandler = IrqHandler::new(handle_irq);

impl Dwmac1000IrqContext {
    pub(super) fn new(owner: Arc<Dwmac1000Owner>) -> Arc<Self> {
        Arc::new(Self {
            owner,
            pending: RecheckSignal::new(),
            irq_diagnostic_budget: AtomicU32::new(IRQ_DIAGNOSTIC_BUDGET),
            rx_diagnostic_budget: AtomicU32::new(RX_DIAGNOSTIC_BUDGET),
        })
    }

    pub(super) fn private(self: &Arc<Self>) -> AnyOpaque {
        AnyOpaque::new(Dwmac1000IrqPrivate {
            context: self.clone(),
        })
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

    fn take_diagnostic_slot(budget: &AtomicU32) -> bool {
        budget
            .fetch_update(Ordering::Relaxed, Ordering::Relaxed, |remaining| {
                remaining.checked_sub(1)
            })
            .is_ok()
    }

    fn take_irq_diagnostic_slot(&self) -> bool {
        Self::take_diagnostic_slot(&self.irq_diagnostic_budget)
    }

    fn take_rx_diagnostic_slot(&self) -> bool {
        Self::take_diagnostic_slot(&self.rx_diagnostic_budget)
    }

    fn log_rx_state(&self, event: &str, requested_index: Option<usize>, outcome: &str) {
        if !self.take_rx_diagnostic_slot() {
            return;
        }
        let snapshot = self.owner.rx_diagnostic_snapshot();
        let runtime = self.owner.runtime_snapshot();
        let descriptor = snapshot.descriptor;
        kalertln!(
            "dwmac1000 stage=gate3-rx event={} outcome={} requested-index={:?} rx-index={} reservation={:?} des0={:#x} own={} error={} first={} last={} wire-length={} des1={:#x} des2={:#x} csr5={:#x} csr6={:#x} csr7={:#x} mac-control={:#x} mac-interrupt-mask={:#x} mac-high={:#x} mac-low={:#x} frame-filter={:#x}",
            event,
            outcome,
            requested_index,
            snapshot.index,
            snapshot.reservation,
            descriptor.des0,
            descriptor.des0 & (1 << 31) != 0,
            descriptor.des0 & (1 << 15) != 0,
            descriptor.des0 & (1 << 9) != 0,
            descriptor.des0 & (1 << 8) != 0,
            (descriptor.des0 >> 16) & 0x3fff,
            descriptor.des1,
            descriptor.des2,
            runtime.status,
            runtime.dma_control,
            runtime.interrupt_enable,
            runtime.mac_control,
            runtime.mac_interrupt_mask,
            runtime.mac_address_high,
            runtime.mac_address_low,
            runtime.frame_filter,
        );
    }

    pub(super) fn frame_capacity(&self) -> usize {
        self.owner.frame_capacity()
    }

    pub(super) fn ring_size(&self) -> usize {
        self.owner.ring_size()
    }

    pub(super) fn reserve_tx(&self) -> Option<usize> {
        self.owner.reserve_tx()
    }

    pub(super) fn cancel_tx(&self, index: usize) -> Result<(), RingError> {
        self.owner.cancel_tx(index)
    }

    pub(super) fn commit_tx_with<R>(
        &self,
        index: usize,
        length: usize,
        fill: impl FnOnce(&mut [u8]) -> R,
    ) -> Result<R, RingError> {
        self.owner.commit_tx_with(index, length, fill)
    }

    pub(super) fn reclaim_tx(&self) -> Result<bool, RingError> {
        self.owner
            .reclaim_tx()
            .map(|completion| completion.is_some())
    }

    pub(super) fn reserve_rx(&self) -> Option<RxReservation> {
        let reservation = self.owner.reserve_rx();
        let outcome = match reservation {
            None => "empty",
            Some(RxReservation {
                length: Some(_), ..
            }) => "ready",
            Some(RxReservation { length: None, .. }) => "malformed",
        };
        self.log_rx_state("reserve", reservation.map(|value| value.index), outcome);
        reservation
    }

    pub(super) fn cancel_rx(&self, index: usize) -> Result<(), RingError> {
        self.owner.cancel_rx(index)
    }

    pub(super) fn discard_rx(&self, index: usize) -> Result<(), RingError> {
        let result = self.owner.discard_rx(index);
        self.log_rx_state(
            "discard",
            Some(index),
            if result.is_ok() { "refilled" } else { "error" },
        );
        result
    }

    pub(super) fn consume_rx<R>(
        &self,
        index: usize,
        consume: impl FnOnce(&[u8]) -> R,
    ) -> Result<R, RingError> {
        let result = self.owner.consume_rx(index, consume);
        self.log_rx_state(
            "consume",
            Some(index),
            if result.is_ok() { "refilled" } else { "error" },
        );
        result
    }
}

impl DwmacDeviceControl for Dwmac1000IrqContext {
    fn suppress_device(&self) {
        self.owner.suppress_device();
    }

    fn start_device(&self) {
        self.owner.start_device();
        let runtime = self.owner.runtime_snapshot();
        kalertln!(
            "dwmac1000 stage=gate3-runtime event=start csr5={:#x} csr7={:#x} csr6={:#x} mac-control={:#x} mac-interrupt-mask={:#x}",
            runtime.status,
            runtime.interrupt_enable,
            runtime.dma_control,
            runtime.mac_control,
            runtime.mac_interrupt_mask,
        );
    }
}

impl DwmacFrameQueue for Dwmac1000IrqContext {
    type Error = RingError;

    fn frame_capacity(&self) -> usize {
        Dwmac1000IrqContext::frame_capacity(self)
    }

    fn ring_size(&self) -> usize {
        Dwmac1000IrqContext::ring_size(self)
    }

    fn reserve_tx(&self) -> Option<usize> {
        Dwmac1000IrqContext::reserve_tx(self)
    }

    fn cancel_tx(&self, index: usize) -> Result<(), Self::Error> {
        Dwmac1000IrqContext::cancel_tx(self, index)
    }

    fn commit_tx_with<R>(
        &self,
        index: usize,
        length: usize,
        fill: impl FnOnce(&mut [u8]) -> R,
    ) -> Result<R, Self::Error> {
        Dwmac1000IrqContext::commit_tx_with(self, index, length, fill)
    }

    fn reclaim_tx(&self) -> Result<bool, Self::Error> {
        Dwmac1000IrqContext::reclaim_tx(self)
    }

    fn reserve_rx(&self) -> Option<RxFrameReservation> {
        Dwmac1000IrqContext::reserve_rx(self).map(|reservation| RxFrameReservation {
            index: reservation.index,
            frame_ready: reservation.length.is_some(),
        })
    }

    fn cancel_rx(&self, index: usize) -> Result<(), Self::Error> {
        Dwmac1000IrqContext::cancel_rx(self, index)
    }

    fn discard_rx(&self, index: usize) -> Result<(), Self::Error> {
        Dwmac1000IrqContext::discard_rx(self, index)
    }

    fn consume_rx<R>(
        &self,
        index: usize,
        consume: impl FnOnce(&[u8]) -> R,
    ) -> Result<R, Self::Error> {
        Dwmac1000IrqContext::consume_rx(self, index, consume)
    }

    fn install_recheck_wake(&self, wake: Weak<dyn RecheckWake>) {
        Dwmac1000IrqContext::install_recheck_wake(self, wake);
    }

    fn recheck_requested(&self) -> bool {
        Dwmac1000IrqContext::recheck_requested(self)
    }

    fn take_recheck_requested(&self) -> bool {
        Dwmac1000IrqContext::take_pending(self) != 0
    }
}

fn handle_irq(private: &AnyOpaque) {
    let context = private
        .cast::<Dwmac1000IrqPrivate>()
        .expect("DWMAC1000 IRQ received invalid private data");
    let context = &context.context;
    if context.take_irq_diagnostic_slot() {
        kalertln!(
            "dwmac1000 stage=gate3-irq event=enter pending={:#x}",
            context.pending.snapshot(),
        );
    }
    let service = context.owner.service_irq();
    if context.take_irq_diagnostic_slot() {
        let rx = context.owner.rx_diagnostic_snapshot();
        let descriptor = rx.descriptor;
        kalertln!(
            "dwmac1000 stage=gate3-irq event=sample csr5-raw={:#x} csr5-legal={:#x} csr5-raw-after={:#x} csr5-after={:#x} mac-status={:#x} mac-status-after={:#x} rx-index={} rx-reservation={:?} rx-des0={:#x} rx-own={} rx-error={} rx-first={} rx-last={} rx-wire-length={}",
            service.csr5_raw,
            service.csr5,
            service.csr5_raw_after,
            service.csr5_after,
            service.mac_status,
            service.mac_status_after,
            rx.index,
            rx.reservation,
            descriptor.des0,
            descriptor.des0 & (1 << 31) != 0,
            descriptor.des0 & (1 << 15) != 0,
            descriptor.des0 & (1 << 9) != 0,
            descriptor.des0 & (1 << 8) != 0,
            (descriptor.des0 >> 16) & 0x3fff,
        );
    }
    if service.csr5_after != 0 || service.mac_status_after != 0 {
        kerrln!(
            "dwmac1000 stage=gate3-irq result=fail csr5-raw={:#x} csr5={:#x} csr5-raw-after={:#x} csr5-after={:#x} mac-status={:#x} mac-status-after={:#x} action=quiesce",
            service.csr5_raw,
            service.csr5,
            service.csr5_raw_after,
            service.csr5_after,
            service.mac_status,
            service.mac_status_after,
        );
        context.owner.suppress_device();
        // Do not wake the worker after a failed W1C/read-to-clear proof: the
        // owner has been quiesced and no new frame capability may be minted.
        return;
    }
    let causes = service.csr5 | service.mac_status;
    if causes != 0 {
        kdebugln!(
            "dwmac1000 stage=gate3-irq result=handled csr5-raw={:#x} csr5={:#x} csr5-after={:#x} mac-status={:#x} mac-status-after={:#x}",
            service.csr5_raw,
            service.csr5,
            service.csr5_after,
            service.mac_status,
            service.mac_status_after,
        );
        let (pending_before, wake_called) = context.pending.publish(causes);
        if context.take_irq_diagnostic_slot() {
            kalertln!(
                "dwmac1000 stage=gate3-irq event=pending causes={:#x} pending-before={:#x} pending-after={:#x} wake-capability-called={}",
                causes,
                pending_before,
                context.pending.snapshot(),
                wake_called,
            );
        }
    } else if context.take_irq_diagnostic_slot() {
        kalertln!(
            "dwmac1000 stage=gate3-irq event=idle csr5-raw={:#x} mac-status={:#x} pending={:#x}",
            service.csr5_raw,
            service.mac_status,
            context.pending.snapshot(),
        );
    }
}

#[cfg(feature = "kunit")]
mod kunits {
    use core::sync::atomic::{AtomicUsize, Ordering};

    use super::*;

    struct RecordingWake(AtomicUsize);

    impl RecheckWake for RecordingWake {
        fn wake(&self) {
            self.0.fetch_add(1, Ordering::Relaxed);
        }
    }

    #[kunit]
    fn pending_causes_are_coalesced_and_consumed_once() {
        let pending = RecheckSignal::new();
        let recording = Arc::new(RecordingWake(AtomicUsize::new(0)));
        let wake: Arc<dyn RecheckWake> = recording.clone();
        pending.install_wake(Arc::downgrade(&wake));
        pending.publish(1 << 6);
        pending.publish(1 << 0);
        assert_eq!(recording.0.load(Ordering::Relaxed), 1);
        assert_eq!(pending.take(), (1 << 6) | 1);
        assert!(!pending.requested());
        pending.publish(1 << 1);
        assert_eq!(recording.0.load(Ordering::Relaxed), 2);
        assert_eq!(pending.take(), 1 << 1);
    }
}
