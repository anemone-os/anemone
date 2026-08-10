use anemone_net_api::{
    EthernetAddress, FrameCapabilities, FrameProvider, FrameSizeError, Instant, LinkState,
    ReceiveOutcome, RxToken, TransmitOutcome, TxToken,
};

use crate::{
    device::net::{NetdevFrameProvider, RecheckWake},
    prelude::*,
};

use super::irq::GmacIrqContext;

/// Per-node frame capability. The IRQ context is the sole owner of
/// rings and MMIO, while this provider owns the frame-token protocol surface.
pub(super) struct JH7110GmacProvider {
    context: Arc<GmacIrqContext>,
    /// Immutable publication-time fact parsed from this node's
    /// `local-mac-address`; it never participates in queue admission.
    mac: EthernetAddress,
}

impl JH7110GmacProvider {
    pub(super) fn new(context: Arc<GmacIrqContext>, mac: [u8; 6]) -> Self {
        Self {
            context,
            mac: EthernetAddress::new(mac),
        }
    }

    pub(super) const fn ethernet_address(&self) -> EthernetAddress {
        self.mac
    }

    fn reclaim_tx(&self) {
        loop {
            match self.context.reclaim_tx() {
                Ok(Some(_completion)) => {},
                Ok(None) => break,
                Err(error) => panic!(
                    "JH7110 TX reclaim reservation invariant failed: {:?}",
                    error
                ),
            }
        }
    }
}

pub(super) struct JH7110RxToken<'a> {
    context: &'a GmacIrqContext,
    index: usize,
    consumed: bool,
}

impl RxToken for JH7110RxToken<'_> {
    fn consume<R, F>(mut self, consume: F) -> R
    where
        F: FnOnce(&[u8]) -> R,
    {
        let result = self
            .context
            .consume_rx(self.index, consume)
            .unwrap_or_else(|error| panic!("JH7110 RX reservation disappeared: {:?}", error));
        self.consumed = true;
        result
    }
}

impl Drop for JH7110RxToken<'_> {
    fn drop(&mut self) {
        if !self.consumed {
            self.context.cancel_rx(self.index).unwrap_or_else(|error| {
                panic!("JH7110 RX reservation cancellation failed: {:?}", error)
            });
        }
    }
}

pub(super) struct JH7110TxToken<'a> {
    context: &'a GmacIrqContext,
    index: usize,
    consumed: bool,
}

impl TxToken for JH7110TxToken<'_> {
    fn capacity(&self) -> usize {
        self.context.frame_capacity()
    }

    fn consume<R, F>(mut self, length: usize, fill: F) -> Result<R, FrameSizeError>
    where
        F: FnOnce(&mut [u8]) -> R,
    {
        let capacity = self.capacity();
        if length == 0 || length > capacity {
            self.context.cancel_tx(self.index).unwrap_or_else(|error| {
                panic!("JH7110 TX reservation cancellation failed: {:?}", error)
            });
            self.consumed = true;
            return Err(FrameSizeError::new(length, capacity));
        }
        let result = self
            .context
            .commit_tx_with(self.index, length, fill)
            .unwrap_or_else(|error| panic!("JH7110 TX reservation commit failed: {:?}", error));
        self.consumed = true;
        Ok(result)
    }
}

impl Drop for JH7110TxToken<'_> {
    fn drop(&mut self) {
        if !self.consumed {
            self.context.cancel_tx(self.index).unwrap_or_else(|error| {
                panic!("JH7110 TX reservation cancellation failed: {:?}", error)
            });
        }
    }
}

impl FrameProvider for JH7110GmacProvider {
    type RxToken<'a> = JH7110RxToken<'a>;
    type TxToken<'a> = JH7110TxToken<'a>;

    fn receive(&mut self, _now: Instant) -> ReceiveOutcome<Self::RxToken<'_>, Self::TxToken<'_>> {
        self.reclaim_tx();
        for _ in 0..self.context.ring_size() {
            let Some(reservation) = self.context.reserve_rx() else {
                return ReceiveOutcome::Empty;
            };
            if reservation.length.is_none() {
                self.context
                    .discard_rx(reservation.index)
                    .unwrap_or_else(|error| {
                        panic!("JH7110 malformed RX discard failed: {:?}", error)
                    });
                continue;
            }
            let Some(tx_index) = self.context.reserve_tx() else {
                self.context
                    .cancel_rx(reservation.index)
                    .unwrap_or_else(|error| {
                        panic!("JH7110 RX reservation cancellation failed: {:?}", error)
                    });
                return ReceiveOutcome::TransmitExhausted;
            };
            return ReceiveOutcome::Ready {
                rx: JH7110RxToken {
                    context: self.context.as_ref(),
                    index: reservation.index,
                    consumed: false,
                },
                tx: JH7110TxToken {
                    context: self.context.as_ref(),
                    index: tx_index,
                    consumed: false,
                },
            };
        }
        ReceiveOutcome::Empty
    }

    fn transmit(&mut self, _now: Instant) -> TransmitOutcome<Self::TxToken<'_>> {
        self.reclaim_tx();
        let Some(index) = self.context.reserve_tx() else {
            return TransmitOutcome::Exhausted;
        };
        TransmitOutcome::Ready(JH7110TxToken {
            context: self.context.as_ref(),
            index,
            consumed: false,
        })
    }

    fn capabilities(&self) -> FrameCapabilities {
        FrameCapabilities {
            max_frame_len: self.context.frame_capacity(),
        }
    }

    fn link_state(&self) -> LinkState {
        LinkState::Unknown
    }
}

impl NetdevFrameProvider for JH7110GmacProvider {
    fn install_recheck_wake(&self, wake: Weak<dyn RecheckWake>) {
        self.context.install_recheck_wake(wake);
    }

    fn recheck_requested(&self) -> bool {
        self.context.recheck_requested()
    }

    fn take_recheck_requested(&self) -> bool {
        self.context.take_pending() != 0
    }
}
