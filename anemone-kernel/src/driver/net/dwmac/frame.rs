use core::fmt::Debug;

use anemone_net_api::{
    EthernetAddress, FrameCapabilities, FrameProvider, FrameSizeError, Instant, LinkState,
    ReceiveOutcome, RxToken, TransmitOutcome, TxToken,
};

use crate::{
    device::net::{NetdevFrameProvider, RecheckWake},
    prelude::*,
};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) struct RxFrameReservation {
    pub(super) index: usize,
    pub(super) frame_ready: bool,
}

pub(super) trait DwmacFrameQueue: Send + Sync + 'static {
    type Error: Debug;

    fn frame_capacity(&self) -> usize;
    fn ring_size(&self) -> usize;
    fn reserve_tx(&self) -> Option<usize>;
    fn cancel_tx(&self, index: usize) -> Result<(), Self::Error>;
    fn commit_tx_with<R>(
        &self,
        index: usize,
        length: usize,
        fill: impl FnOnce(&mut [u8]) -> R,
    ) -> Result<R, Self::Error>;
    fn reclaim_tx(&self) -> Result<bool, Self::Error>;
    fn reserve_rx(&self) -> Option<RxFrameReservation>;
    fn cancel_rx(&self, index: usize) -> Result<(), Self::Error>;
    fn discard_rx(&self, index: usize) -> Result<(), Self::Error>;
    fn consume_rx<R>(
        &self,
        index: usize,
        consume: impl FnOnce(&[u8]) -> R,
    ) -> Result<R, Self::Error>;
    fn install_recheck_wake(&self, wake: Weak<dyn RecheckWake>);
    fn recheck_requested(&self) -> bool;
    fn take_recheck_requested(&self) -> bool;
}

/// Common frame/progression adapter. Concrete backends retain register,
/// descriptor, DMA-address, and device-cause ownership behind
/// `DwmacFrameQueue`.
pub(super) struct DwmacFrameProvider<Q: DwmacFrameQueue> {
    queue: Arc<Q>,
    /// Immutable publication-time fact parsed from this node's firmware data;
    /// it never participates in queue admission.
    mac: EthernetAddress,
    link_state: LinkState,
}

/// Boot-time placeholder for a DWMAC node whose PHY exists but has no
/// resolved carrier. It preserves the stable netdev identity without owning
/// MMIO, DMA backing, an IRQ, or a path that can become available later.
/// Remove it when DWMAC1000 gains a runtime PHY owner that can safely turn a
/// post-boot carrier into a configured MAC and live frame provider.
pub(super) struct UnavailableDwmacProvider {
    frame_capacity: usize,
}

#[derive(Debug, Eq, PartialEq)]
pub(super) enum UnavailableDwmacToken {}

impl RxToken for UnavailableDwmacToken {
    fn consume<R, F>(self, _consume: F) -> R
    where
        F: FnOnce(&[u8]) -> R,
    {
        match self {}
    }
}

impl TxToken for UnavailableDwmacToken {
    fn capacity(&self) -> usize {
        match *self {}
    }

    fn consume<R, F>(self, _length: usize, _fill: F) -> Result<R, FrameSizeError>
    where
        F: FnOnce(&mut [u8]) -> R,
    {
        match self {}
    }
}

impl UnavailableDwmacProvider {
    pub(super) const fn new(frame_capacity: usize) -> Self {
        Self { frame_capacity }
    }
}

impl FrameProvider for UnavailableDwmacProvider {
    type RxToken<'a> = UnavailableDwmacToken;
    type TxToken<'a> = UnavailableDwmacToken;

    fn receive(&mut self, _now: Instant) -> ReceiveOutcome<Self::RxToken<'_>, Self::TxToken<'_>> {
        ReceiveOutcome::LinkUnavailable
    }

    fn transmit(&mut self, _now: Instant) -> TransmitOutcome<Self::TxToken<'_>> {
        TransmitOutcome::LinkUnavailable
    }

    fn capabilities(&self) -> FrameCapabilities {
        FrameCapabilities {
            max_frame_len: self.frame_capacity,
        }
    }

    fn link_state(&self) -> LinkState {
        LinkState::Down
    }
}

impl NetdevFrameProvider for UnavailableDwmacProvider {
    fn install_recheck_wake(&self, _wake: Weak<dyn RecheckWake>) {
        // This compatibility placeholder is permanently unavailable. A cable
        // inserted after boot cannot produce a provider edge; rebooting with
        // carrier is the only transition to the real DWMAC owner.
    }

    fn recheck_requested(&self) -> bool {
        false
    }

    fn take_recheck_requested(&self) -> bool {
        false
    }
}

impl<Q: DwmacFrameQueue> DwmacFrameProvider<Q> {
    pub(super) fn new(queue: Arc<Q>, mac: [u8; 6]) -> Self {
        Self::with_link_state(queue, mac, LinkState::Unknown)
    }

    pub(super) fn with_link_state(queue: Arc<Q>, mac: [u8; 6], link_state: LinkState) -> Self {
        Self {
            queue,
            mac: EthernetAddress::new(mac),
            link_state,
        }
    }

    pub(super) const fn ethernet_address(&self) -> EthernetAddress {
        self.mac
    }

    fn reclaim_tx(&self) {
        loop {
            match self.queue.reclaim_tx() {
                Ok(true) => {},
                Ok(false) => break,
                Err(error) => panic!("DWMAC TX reclaim invariant failed: {:?}", error),
            }
        }
    }
}

pub(super) struct DwmacRxToken<'a, Q: DwmacFrameQueue> {
    queue: &'a Q,
    index: usize,
    consumed: bool,
}

impl<Q: DwmacFrameQueue> RxToken for DwmacRxToken<'_, Q> {
    fn consume<R, F>(mut self, consume: F) -> R
    where
        F: FnOnce(&[u8]) -> R,
    {
        let result = self
            .queue
            .consume_rx(self.index, consume)
            .unwrap_or_else(|error| panic!("DWMAC RX reservation disappeared: {:?}", error));
        self.consumed = true;
        result
    }
}

impl<Q: DwmacFrameQueue> Drop for DwmacRxToken<'_, Q> {
    fn drop(&mut self) {
        if !self.consumed {
            self.queue.cancel_rx(self.index).unwrap_or_else(|error| {
                panic!("DWMAC RX reservation cancellation failed: {:?}", error)
            });
        }
    }
}

pub(super) struct DwmacTxToken<'a, Q: DwmacFrameQueue> {
    queue: &'a Q,
    index: usize,
    consumed: bool,
}

impl<Q: DwmacFrameQueue> TxToken for DwmacTxToken<'_, Q> {
    fn capacity(&self) -> usize {
        self.queue.frame_capacity()
    }

    fn consume<R, F>(mut self, length: usize, fill: F) -> Result<R, FrameSizeError>
    where
        F: FnOnce(&mut [u8]) -> R,
    {
        let capacity = self.capacity();
        if length == 0 || length > capacity {
            self.queue.cancel_tx(self.index).unwrap_or_else(|error| {
                panic!("DWMAC TX reservation cancellation failed: {:?}", error)
            });
            self.consumed = true;
            return Err(FrameSizeError::new(length, capacity));
        }
        let result = self
            .queue
            .commit_tx_with(self.index, length, fill)
            .unwrap_or_else(|error| panic!("DWMAC TX reservation commit failed: {:?}", error));
        self.consumed = true;
        Ok(result)
    }
}

impl<Q: DwmacFrameQueue> Drop for DwmacTxToken<'_, Q> {
    fn drop(&mut self) {
        if !self.consumed {
            self.queue.cancel_tx(self.index).unwrap_or_else(|error| {
                panic!("DWMAC TX reservation cancellation failed: {:?}", error)
            });
        }
    }
}

impl<Q: DwmacFrameQueue> FrameProvider for DwmacFrameProvider<Q> {
    type RxToken<'a> = DwmacRxToken<'a, Q>;
    type TxToken<'a> = DwmacTxToken<'a, Q>;

    fn receive(&mut self, _now: Instant) -> ReceiveOutcome<Self::RxToken<'_>, Self::TxToken<'_>> {
        self.reclaim_tx();
        for _ in 0..self.queue.ring_size() {
            let Some(reservation) = self.queue.reserve_rx() else {
                return ReceiveOutcome::Empty;
            };
            if !reservation.frame_ready {
                self.queue
                    .discard_rx(reservation.index)
                    .unwrap_or_else(|error| {
                        panic!("DWMAC malformed RX discard failed: {:?}", error)
                    });
                continue;
            }
            let Some(tx_index) = self.queue.reserve_tx() else {
                self.queue
                    .cancel_rx(reservation.index)
                    .unwrap_or_else(|error| {
                        panic!("DWMAC RX reservation cancellation failed: {:?}", error)
                    });
                return ReceiveOutcome::TransmitExhausted;
            };
            return ReceiveOutcome::Ready {
                rx: DwmacRxToken {
                    queue: self.queue.as_ref(),
                    index: reservation.index,
                    consumed: false,
                },
                tx: DwmacTxToken {
                    queue: self.queue.as_ref(),
                    index: tx_index,
                    consumed: false,
                },
            };
        }
        ReceiveOutcome::Empty
    }

    fn transmit(&mut self, _now: Instant) -> TransmitOutcome<Self::TxToken<'_>> {
        self.reclaim_tx();
        let Some(index) = self.queue.reserve_tx() else {
            return TransmitOutcome::Exhausted;
        };
        TransmitOutcome::Ready(DwmacTxToken {
            queue: self.queue.as_ref(),
            index,
            consumed: false,
        })
    }

    fn capabilities(&self) -> FrameCapabilities {
        FrameCapabilities {
            max_frame_len: self.queue.frame_capacity(),
        }
    }

    fn link_state(&self) -> LinkState {
        self.link_state
    }
}

impl<Q: DwmacFrameQueue> NetdevFrameProvider for DwmacFrameProvider<Q> {
    fn install_recheck_wake(&self, wake: Weak<dyn RecheckWake>) {
        self.queue.install_recheck_wake(wake);
    }

    fn recheck_requested(&self) -> bool {
        self.queue.recheck_requested()
    }

    fn take_recheck_requested(&self) -> bool {
        self.queue.take_recheck_requested()
    }
}

#[cfg(feature = "kunit")]
mod kunits {
    use super::*;

    #[kunit]
    fn unavailable_provider_is_down_and_never_mints_frame_tokens() {
        let mut provider = UnavailableDwmacProvider::new(1536);
        assert_eq!(provider.link_state(), LinkState::Down);
        assert_eq!(provider.capabilities().max_frame_len, 1536);
        assert_eq!(
            provider.receive(Instant::ZERO),
            ReceiveOutcome::LinkUnavailable
        );
        assert_eq!(
            provider.transmit(Instant::ZERO),
            TransmitOutcome::LinkUnavailable
        );
        assert!(!provider.recheck_requested());
        assert!(!provider.take_recheck_requested());
    }
}
