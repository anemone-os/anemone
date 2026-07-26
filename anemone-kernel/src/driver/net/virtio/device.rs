use core::sync::atomic::{AtomicBool, AtomicUsize, Ordering};

use anemone_net_api::EthernetAddress;
use virtio_drivers::{Error as VirtIOError, device::net::VirtIONetRaw, transport::SomeTransport};

use crate::{device::net::RecheckWake, driver::virtio::VirtIOHalImpl, prelude::*};

use super::{
    HEADER_RESERVE, QUEUE_SIZE,
    frame::{RxOwnership, RxSlot, TxOwnership, TxSlot},
};

type RawNet = VirtIONetRaw<VirtIOHalImpl, SomeTransport<'static>, QUEUE_SIZE>;

/// Diagnostic-only provider counters. They never drive queue or worker state.
#[cfg(feature = "kunit")]
#[derive(Clone, Copy, Debug)]
pub(crate) struct VirtIONetStats {
    rx_completions: usize,
    tx_submissions: usize,
    tx_completions: usize,
    irq_rechecks: usize,
    queue_full: usize,
    live_mappings: usize,
    mapping_high_water: usize,
}

#[cfg(feature = "kunit")]
impl VirtIONetStats {
    pub(crate) const fn rx_completions(self) -> usize {
        self.rx_completions
    }
    pub(crate) const fn tx_submissions(self) -> usize {
        self.tx_submissions
    }
    pub(crate) const fn tx_completions(self) -> usize {
        self.tx_completions
    }
    pub(crate) const fn irq_rechecks(self) -> usize {
        self.irq_rechecks
    }
    pub(crate) const fn queue_full(self) -> usize {
        self.queue_full
    }
    pub(crate) const fn live_mappings(self) -> usize {
        self.live_mappings
    }
    pub(crate) const fn mapping_high_water(self) -> usize {
        self.mapping_high_water
    }
}

pub(super) struct VirtIONetDiagnostics {
    pub(super) rx_completions: AtomicUsize,
    pub(super) tx_submissions: AtomicUsize,
    pub(super) tx_completions: AtomicUsize,
    pub(super) irq_rechecks: AtomicUsize,
    pub(super) queue_full: AtomicUsize,
    live_mappings: AtomicUsize,
    mapping_high_water: AtomicUsize,
}

impl VirtIONetDiagnostics {
    const fn new() -> Self {
        Self {
            rx_completions: AtomicUsize::new(0),
            tx_submissions: AtomicUsize::new(0),
            tx_completions: AtomicUsize::new(0),
            irq_rechecks: AtomicUsize::new(0),
            queue_full: AtomicUsize::new(0),
            live_mappings: AtomicUsize::new(0),
            mapping_high_water: AtomicUsize::new(0),
        }
    }

    pub(super) fn mapping_opened(&self) {
        let live = self.live_mappings.fetch_add(1, Ordering::Relaxed) + 1;
        self.mapping_high_water.fetch_max(live, Ordering::Relaxed);
    }

    pub(super) fn mapping_closed(&self) {
        let previous = self.live_mappings.fetch_sub(1, Ordering::Relaxed);
        assert!(previous > 0, "VirtIO-Net mapping counter underflow");
    }

    #[cfg(feature = "kunit")]
    fn snapshot(&self) -> VirtIONetStats {
        VirtIONetStats {
            rx_completions: self.rx_completions.load(Ordering::Relaxed),
            tx_submissions: self.tx_submissions.load(Ordering::Relaxed),
            tx_completions: self.tx_completions.load(Ordering::Relaxed),
            irq_rechecks: self.irq_rechecks.load(Ordering::Relaxed),
            queue_full: self.queue_full.load(Ordering::Relaxed),
            live_mappings: self.live_mappings.load(Ordering::Relaxed),
            mapping_high_water: self.mapping_high_water.load(Ordering::Relaxed),
        }
    }
}

/// IRQ-shared device facts. Frame slots are deliberately absent: the provider
/// owns them exclusively, so protocol callbacks need no lock and cannot race
/// the IRQ path.
pub(super) struct VirtIONetDevice {
    pub(super) raw: SpinLock<RawNet>,
    /// Edge-only wake hint. Queue completion remains the durable truth.
    recheck_requested: AtomicBool,
    recheck_wake: spin::Once<Weak<dyn RecheckWake>>,
    /// Diagnostic-only mirrors of owner transitions; never behavior inputs.
    pub(super) diagnostics: VirtIONetDiagnostics,
}

impl VirtIONetDevice {
    pub(super) fn new(
        transport: SomeTransport<'static>,
    ) -> Result<(Arc<Self>, EthernetAddress), VirtIOError> {
        let raw = RawNet::new(transport)?;
        let mac = EthernetAddress::new(raw.mac_address());
        Ok((
            Arc::new(Self {
                raw: SpinLock::new(raw),
                recheck_requested: AtomicBool::new(false),
                recheck_wake: spin::Once::new(),
                diagnostics: VirtIONetDiagnostics::new(),
            }),
            mac,
        ))
    }

    pub(super) fn submit_rx(&self, slot: &mut RxSlot) -> Result<(), VirtIOError> {
        assert!(matches!(
            slot.ownership,
            RxOwnership::Unqueued | RxOwnership::RequeuePending
        ));
        let mut raw = self.raw.lock_irqsave();
        // SAFETY: this exact stable boxed backing remains owned by `slot` and
        // inaccessible to CPU frame consumers until the matching queue token
        // is observed and `receive_complete` unshares it. On error no request
        // was committed, so the provider retains CPU ownership.
        let queue_token = unsafe { raw.receive_begin(&mut slot.backing)? };
        slot.ownership = RxOwnership::Device { queue_token };
        self.diagnostics.mapping_opened();
        Ok(())
    }

    pub(super) fn submit_tx(&self, slot: &mut TxSlot, len: usize) -> Result<(), VirtIOError> {
        assert_eq!(slot.ownership, TxOwnership::Reserved);
        let mut raw = self.raw.lock_irqsave();
        let header_len = raw.fill_buffer_header(&mut slot.backing)?;
        if header_len != HEADER_RESERVE {
            slot.backing
                .copy_within(HEADER_RESERVE..HEADER_RESERVE + len, header_len);
        }
        let total_len = header_len + len;
        // SAFETY: the stable boxed prefix contains the initialized header and
        // frame. After the queue commit the slot becomes Device-owned and no
        // CPU path accesses it until the matching completion is harvested. An
        // error leaves the slot Reserved and therefore CPU-owned.
        let queue_token = unsafe { raw.transmit_begin(&slot.backing[..total_len])? };
        slot.ownership = TxOwnership::Device {
            queue_token,
            total_len,
        };
        self.diagnostics
            .tx_submissions
            .fetch_add(1, Ordering::Relaxed);
        self.diagnostics.mapping_opened();
        Ok(())
    }

    pub(super) fn handle_irq(&self) {
        if !self.raw.lock_irqsave().ack_interrupt().is_empty() {
            self.recheck_requested.store(true, Ordering::Release);
            self.diagnostics
                .irq_rechecks
                .fetch_add(1, Ordering::Relaxed);
            if let Some(wake) = self.recheck_wake.get().and_then(Weak::upgrade) {
                wake.wake();
            }
        }
    }

    pub(super) fn install_recheck_wake(&self, wake: Weak<dyn RecheckWake>) {
        assert!(
            self.recheck_wake.get().is_none(),
            "VirtIO-Net recheck wake installed twice"
        );
        self.recheck_wake.call_once(|| wake);
    }

    pub(super) fn recheck_requested(&self) -> bool {
        self.recheck_requested.load(Ordering::Acquire)
    }

    pub(super) fn take_recheck_requested(&self) -> bool {
        self.recheck_requested.swap(false, Ordering::AcqRel)
    }

    pub(super) fn enable_interrupts(&self) {
        self.raw.lock_irqsave().enable_interrupts();
    }
    pub(super) fn disable_interrupts(&self) {
        self.raw.lock_irqsave().disable_interrupts();
    }
    #[cfg(feature = "kunit")]
    pub(super) fn stats(&self) -> VirtIONetStats {
        self.diagnostics.snapshot()
    }
}
