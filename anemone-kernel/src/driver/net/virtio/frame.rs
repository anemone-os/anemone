use core::sync::atomic::Ordering;

use anemone_net_api::{
    EthernetAddress, FrameCapabilities, FrameProvider, FrameSizeError, Instant, LinkState,
    ReceiveOutcome, RxToken, TransmitOutcome, TxToken,
};
use virtio_drivers::{Error as VirtIOError, transport::SomeTransport};

use crate::{
    device::net::{NetdevFrameProvider, RecheckWake},
    prelude::*,
};

use super::{
    BACKING_CAPACITY, FRAME_CAPACITY, HEADER_RESERVE, RX_SLOT_COUNT, TX_SLOT_COUNT,
    device::VirtIONetDevice,
};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) enum RxOwnership {
    Unqueued,
    Device { queue_token: u16 },
    Ready { frame_offset: usize, len: usize },
    Reserved { frame_offset: usize, len: usize },
    RequeuePending,
}

pub(super) struct RxSlot {
    pub(super) backing: Box<[u8]>,
    pub(super) ownership: RxOwnership,
}

impl RxSlot {
    fn new() -> Self {
        Self {
            backing: vec![0; BACKING_CAPACITY].into_boxed_slice(),
            ownership: RxOwnership::Unqueued,
        }
    }

    fn reserve(&mut self) -> bool {
        let RxOwnership::Ready { frame_offset, len } = self.ownership else {
            return false;
        };
        self.ownership = RxOwnership::Reserved { frame_offset, len };
        true
    }

    fn cancel_reservation(&mut self) {
        let RxOwnership::Reserved { frame_offset, len } = self.ownership else {
            panic!("RX token cancellation requires a reserved slot")
        };
        self.ownership = RxOwnership::Ready { frame_offset, len };
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) enum TxOwnership {
    Available,
    Reserved,
    Device { queue_token: u16, total_len: usize },
}

pub(super) struct TxSlot {
    pub(super) backing: Box<[u8]>,
    pub(super) ownership: TxOwnership,
}

impl TxSlot {
    fn new() -> Self {
        Self {
            backing: vec![0; BACKING_CAPACITY].into_boxed_slice(),
            ownership: TxOwnership::Available,
        }
    }

    fn reserve(&mut self) -> bool {
        if self.ownership != TxOwnership::Available {
            return false;
        }
        self.ownership = TxOwnership::Reserved;
        true
    }

    fn cancel_reservation(&mut self) {
        assert_eq!(self.ownership, TxOwnership::Reserved);
        self.ownership = TxOwnership::Available;
    }
}

/// Sole owner of frame slots and their queue-token lifecycle.
///
/// The IRQ handler only touches `device.raw` and an edge bit. Therefore the
/// exclusive `&mut FrameProvider` borrow is sufficient to expose one slot to a
/// protocol callback without a provider-global or IRQ-off guard.
pub(crate) struct VirtIONetProvider {
    // Declared first so pre-IRQ initialization failures drop RawNet (and unset
    // both queues) before releasing the following backing slots. Once an IRQ
    // is registered, R0 never drops this provider: publication failure retains
    // it, and success keeps it alive until power-off. Runtime removal must first
    // prevent Weak upgrades and quiesce/reset the queues before allowing drop.
    pub(super) device: Arc<VirtIONetDevice>,
    rx_slots: Box<[RxSlot]>,
    tx_slots: Box<[TxSlot]>,
}

impl VirtIONetProvider {
    pub(super) fn new(
        transport: SomeTransport<'static>,
    ) -> Result<(Self, EthernetAddress), VirtIOError> {
        let (device, mac) = VirtIONetDevice::new(transport)?;
        let mut provider = Self {
            device,
            rx_slots: (0..RX_SLOT_COUNT)
                .map(|_| RxSlot::new())
                .collect::<Vec<_>>()
                .into_boxed_slice(),
            tx_slots: (0..TX_SLOT_COUNT)
                .map(|_| TxSlot::new())
                .collect::<Vec<_>>()
                .into_boxed_slice(),
        };
        for slot in &mut provider.rx_slots {
            provider.device.submit_rx(slot)?;
        }
        Ok((provider, mac))
    }

    fn harvest_rx(&mut self) {
        let mut raw = self.device.raw.lock_irqsave();
        let Some(queue_token) = raw.poll_receive() else {
            return;
        };
        let slot = self
            .rx_slots
            .iter_mut()
            .find(|slot| slot.ownership == (RxOwnership::Device { queue_token }))
            .expect("VirtIO-Net RX completion must have one owner slot");
        // SAFETY: the queue token and exact backing were stored together by
        // `submit_rx`; completion unshares/syncs before Ready becomes visible.
        let (frame_offset, len) = unsafe {
            raw.receive_complete(queue_token, &mut slot.backing)
                .unwrap_or_else(|error| panic!("VirtIO-Net RX completion failed: {error}"))
        };
        self.device.diagnostics.mapping_closed();
        self.device
            .diagnostics
            .rx_completions
            .fetch_add(1, Ordering::Relaxed);
        assert!(frame_offset + len <= slot.backing.len());
        slot.ownership = RxOwnership::Ready { frame_offset, len };
    }

    fn harvest_tx(&mut self) {
        loop {
            let mut raw = self.device.raw.lock_irqsave();
            let Some(queue_token) = raw.poll_transmit() else {
                return;
            };
            let slot = self.tx_slots.iter_mut().find(|slot| {
                matches!(slot.ownership, TxOwnership::Device { queue_token: owned, .. } if owned == queue_token)
            }).expect("VirtIO-Net TX completion must have one owner slot");
            let TxOwnership::Device { total_len, .. } = slot.ownership else {
                unreachable!()
            };
            // SAFETY: this is the same stable backing prefix and matching token
            // committed by `submit_tx`; completion precedes CPU reuse.
            unsafe {
                raw.transmit_complete(queue_token, &slot.backing[..total_len])
                    .unwrap_or_else(|error| panic!("VirtIO-Net TX completion failed: {error}"));
            }
            self.device.diagnostics.mapping_closed();
            self.device
                .diagnostics
                .tx_completions
                .fetch_add(1, Ordering::Relaxed);
            slot.ownership = TxOwnership::Available;
        }
    }
}

impl NetdevFrameProvider for VirtIONetProvider {
    fn install_recheck_wake(&self, wake: Weak<dyn RecheckWake>) {
        self.device.install_recheck_wake(wake);
    }
    fn recheck_requested(&self) -> bool {
        self.device.recheck_requested()
    }
    fn take_recheck_requested(&self) -> bool {
        self.device.take_recheck_requested()
    }
}

pub(crate) struct VirtIORxToken<'a> {
    device: &'a VirtIONetDevice,
    slot: &'a mut RxSlot,
    consumed: bool,
}

impl RxToken for VirtIORxToken<'_> {
    fn consume<R, F>(mut self, f: F) -> R
    where
        F: FnOnce(&[u8]) -> R,
    {
        let RxOwnership::Reserved { frame_offset, len } = self.slot.ownership else {
            panic!("RX consume requires a reserved slot")
        };
        let result = f(&self.slot.backing[frame_offset..frame_offset + len]);
        self.slot.ownership = RxOwnership::RequeuePending;
        // Commit before refill so unwinding cannot attempt Reserved-only cancel.
        self.consumed = true;
        self.device
            .submit_rx(self.slot)
            .unwrap_or_else(|error| panic!("VirtIO-Net RX refill failed: {error}"));
        result
    }
}

impl Drop for VirtIORxToken<'_> {
    fn drop(&mut self) {
        if !self.consumed {
            self.slot.cancel_reservation();
        }
    }
}

pub(crate) struct VirtIOTxToken<'a> {
    device: &'a VirtIONetDevice,
    slot: &'a mut TxSlot,
    consumed: bool,
}

impl TxToken for VirtIOTxToken<'_> {
    fn capacity(&self) -> usize {
        FRAME_CAPACITY
    }
    fn consume<R, F>(mut self, len: usize, f: F) -> Result<R, FrameSizeError>
    where
        F: FnOnce(&mut [u8]) -> R,
    {
        if len > FRAME_CAPACITY {
            self.slot.cancel_reservation();
            self.consumed = true;
            return Err(FrameSizeError::new(len, FRAME_CAPACITY));
        }
        let result = f(&mut self.slot.backing[HEADER_RESERVE..HEADER_RESERVE + len]);
        self.device
            .submit_tx(self.slot, len)
            .unwrap_or_else(|error| {
                panic!("reserved VirtIO-Net TX descriptors disappeared before submit: {error}")
            });
        self.consumed = true;
        Ok(result)
    }
}

impl Drop for VirtIOTxToken<'_> {
    fn drop(&mut self) {
        if !self.consumed {
            self.slot.cancel_reservation();
        }
    }
}

impl FrameProvider for VirtIONetProvider {
    type RxToken<'a> = VirtIORxToken<'a>;
    type TxToken<'a> = VirtIOTxToken<'a>;

    fn receive(&mut self, _now: Instant) -> ReceiveOutcome<Self::RxToken<'_>, Self::TxToken<'_>> {
        self.harvest_tx();
        self.harvest_rx();
        let Some(rx_index) = self.rx_slots.iter_mut().position(RxSlot::reserve) else {
            return ReceiveOutcome::Empty;
        };
        if !self.device.raw.lock_irqsave().can_send() {
            self.device
                .diagnostics
                .queue_full
                .fetch_add(1, Ordering::Relaxed);
            self.rx_slots[rx_index].cancel_reservation();
            return ReceiveOutcome::TransmitExhausted;
        }
        let Some(tx_index) = self.tx_slots.iter_mut().position(TxSlot::reserve) else {
            self.device
                .diagnostics
                .queue_full
                .fetch_add(1, Ordering::Relaxed);
            self.rx_slots[rx_index].cancel_reservation();
            return ReceiveOutcome::TransmitExhausted;
        };
        ReceiveOutcome::Ready {
            rx: VirtIORxToken {
                device: &self.device,
                slot: &mut self.rx_slots[rx_index],
                consumed: false,
            },
            tx: VirtIOTxToken {
                device: &self.device,
                slot: &mut self.tx_slots[tx_index],
                consumed: false,
            },
        }
    }

    fn transmit(&mut self, _now: Instant) -> TransmitOutcome<Self::TxToken<'_>> {
        self.harvest_tx();
        if !self.device.raw.lock_irqsave().can_send() {
            self.device
                .diagnostics
                .queue_full
                .fetch_add(1, Ordering::Relaxed);
            return TransmitOutcome::Exhausted;
        }
        let Some(index) = self.tx_slots.iter_mut().position(TxSlot::reserve) else {
            self.device
                .diagnostics
                .queue_full
                .fetch_add(1, Ordering::Relaxed);
            return TransmitOutcome::Exhausted;
        };
        TransmitOutcome::Ready(VirtIOTxToken {
            device: &self.device,
            slot: &mut self.tx_slots[index],
            consumed: false,
        })
    }

    fn capabilities(&self) -> FrameCapabilities {
        FrameCapabilities {
            max_frame_len: FRAME_CAPACITY,
        }
    }
    fn link_state(&self) -> LinkState {
        LinkState::Unknown
    }
}

#[cfg(feature = "kunit")]
mod kunits {
    use super::*;

    #[kunit]
    fn owner_local_slot_cancellation_restores_the_prior_state() {
        let mut rx = RxSlot::new();
        rx.ownership = RxOwnership::Ready {
            frame_offset: 12,
            len: 64,
        };
        assert!(rx.reserve());
        rx.cancel_reservation();
        assert_eq!(
            rx.ownership,
            RxOwnership::Ready {
                frame_offset: 12,
                len: 64
            }
        );

        let mut tx = TxSlot::new();
        assert!(tx.reserve());
        tx.cancel_reservation();
        assert_eq!(tx.ownership, TxOwnership::Available);
    }
}
