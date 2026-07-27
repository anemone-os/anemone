use core::sync::atomic::{AtomicBool, Ordering};

use anemone_net_api::EthernetAddress;
use virtio_drivers::{Error as VirtIOError, device::net::VirtIONetRaw, transport::SomeTransport};

use crate::{device::net::RecheckWake, driver::virtio::VirtIOHalImpl, prelude::*};

use super::{
    HEADER_RESERVE, QUEUE_SIZE,
    frame::{RxOwnership, RxSlot, TxOwnership, TxSlot},
};

type RawNet = VirtIONetRaw<VirtIOHalImpl, SomeTransport<'static>, QUEUE_SIZE>;

/// Driver-private durable recheck predicate with an optional stateless wake.
///
/// `publish()` commits the predicate before invoking the wake capability.
/// Repeated publications may coalesce in the predicate, and `take()` clears
/// only the committed fact; queue and link truth remain owned by the device.
struct RecheckLatch {
    requested: AtomicBool,
    wake: spin::Once<Weak<dyn RecheckWake>>,
}

impl RecheckLatch {
    fn new() -> Self {
        Self {
            requested: AtomicBool::new(false),
            wake: spin::Once::new(),
        }
    }

    fn install_wake(&self, wake: Weak<dyn RecheckWake>) {
        assert!(
            self.wake.get().is_none(),
            "VirtIO-Net recheck wake installed twice"
        );
        self.wake.call_once(|| wake);
    }

    fn publish(&self) {
        self.requested.store(true, Ordering::Release);
        if let Some(wake) = self.wake.get().and_then(Weak::upgrade) {
            wake.wake();
        }
    }

    fn requested(&self) -> bool {
        self.requested.load(Ordering::Acquire)
    }

    fn take(&self) -> bool {
        self.requested.swap(false, Ordering::AcqRel)
    }
}

/// IRQ-shared device facts. Frame slots are deliberately absent: the provider
/// owns them exclusively, so protocol callbacks need no lock and cannot race
/// the IRQ path.
pub(super) struct VirtIONetDevice {
    pub(super) raw: SpinLock<RawNet>,
    /// Owner-local predicate plus edge-only wake. Queue truth stays in RawNet.
    recheck: RecheckLatch,
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
                recheck: RecheckLatch::new(),
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
        Ok(())
    }

    pub(super) fn handle_irq(&self) {
        if !self.raw.lock_irqsave().ack_interrupt().is_empty() {
            self.recheck.publish();
        }
    }

    pub(super) fn install_recheck_wake(&self, wake: Weak<dyn RecheckWake>) {
        self.recheck.install_wake(wake);
    }

    pub(super) fn recheck_requested(&self) -> bool {
        self.recheck.requested()
    }

    pub(super) fn take_recheck_requested(&self) -> bool {
        self.recheck.take()
    }

    pub(super) fn enable_interrupts(&self) {
        self.raw.lock_irqsave().enable_interrupts();
    }
    pub(super) fn disable_interrupts(&self) {
        self.raw.lock_irqsave().disable_interrupts();
    }
}

#[cfg(feature = "kunit")]
mod kunits {
    use super::*;

    struct WakeProbe(AtomicUsize);

    impl RecheckWake for WakeProbe {
        fn wake(&self) {
            self.0.fetch_add(1, Ordering::Relaxed);
        }
    }

    #[kunit]
    fn recheck_latch_preserves_fact_without_wake_and_coalesces_publications() {
        let latch = RecheckLatch::new();
        latch.publish();
        assert!(latch.requested());

        let wake = Arc::new(WakeProbe(AtomicUsize::new(0)));
        let wake_capability: Arc<dyn RecheckWake> = wake.clone();
        latch.install_wake(Arc::downgrade(&wake_capability));
        drop(wake_capability);
        assert!(latch.requested());
        assert_eq!(wake.0.load(Ordering::Relaxed), 0);
        assert!(latch.take());
        assert!(!latch.take());

        latch.publish();
        latch.publish();
        assert_eq!(wake.0.load(Ordering::Relaxed), 2);
        assert!(latch.take());
        assert!(!latch.take());

        latch.publish();
        assert_eq!(wake.0.load(Ordering::Relaxed), 3);
        assert!(latch.take());
    }
}
