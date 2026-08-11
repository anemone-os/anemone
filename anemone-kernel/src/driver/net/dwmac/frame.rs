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
}

impl<Q: DwmacFrameQueue> DwmacFrameProvider<Q> {
    pub(super) fn new(queue: Arc<Q>, mac: [u8; 6]) -> Self {
        Self {
            queue,
            mac: EthernetAddress::new(mac),
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
        LinkState::Unknown
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
