use anemone_net_api::{
    FrameProvider, Instant, ReceiveOutcome, RxToken as NetRxToken, TransmitOutcome,
    TxToken as NetTxToken,
};
use smoltcp::{
    phy::{
        Device, DeviceCapabilities, Medium, RxToken as SmoltcpRxToken, TxToken as SmoltcpTxToken,
    },
    time::Instant as SmoltcpInstant,
};

pub(crate) struct FrameDevice<'a, P> {
    provider: &'a mut P,
    blocked_work: bool,
}

impl<'a, P> FrameDevice<'a, P> {
    pub(crate) fn new(provider: &'a mut P) -> Self {
        Self {
            provider,
            blocked_work: false,
        }
    }

    pub(crate) fn blocked_work(&self) -> bool {
        self.blocked_work
    }
}

pub(crate) fn to_smoltcp_instant(now: Instant) -> SmoltcpInstant {
    SmoltcpInstant::from_micros(now.total_micros())
}

pub(crate) fn from_smoltcp_instant(now: SmoltcpInstant) -> Instant {
    Instant::from_micros(now.total_micros())
}

pub(crate) struct FrameRxToken<T>(T);

impl<T: NetRxToken> SmoltcpRxToken for FrameRxToken<T> {
    fn consume<R, F>(self, f: F) -> R
    where
        F: FnOnce(&[u8]) -> R,
    {
        self.0.consume(f)
    }
}

pub(crate) struct FrameTxToken<T>(T);

impl<T: NetTxToken> SmoltcpTxToken for FrameTxToken<T> {
    fn consume<R, F>(self, len: usize, f: F) -> R
    where
        F: FnOnce(&mut [u8]) -> R,
    {
        match self.0.consume(len, f) {
            Ok(value) => value,
            Err(error) => panic!(
                "smoltcp requested frame length {} beyond provider capacity {}",
                error.requested(),
                error.capacity()
            ),
        }
    }
}

impl<P: FrameProvider> Device for FrameDevice<'_, P> {
    type RxToken<'a>
        = FrameRxToken<P::RxToken<'a>>
    where
        Self: 'a;
    type TxToken<'a>
        = FrameTxToken<P::TxToken<'a>>
    where
        Self: 'a;

    fn receive(
        &mut self,
        timestamp: SmoltcpInstant,
    ) -> Option<(Self::RxToken<'_>, Self::TxToken<'_>)> {
        match self.provider.receive(from_smoltcp_instant(timestamp)) {
            ReceiveOutcome::Ready { rx, tx } => Some((FrameRxToken(rx), FrameTxToken(tx))),
            ReceiveOutcome::TransmitExhausted => {
                self.blocked_work = true;
                None
            },
            ReceiveOutcome::Empty | ReceiveOutcome::LinkUnavailable => None,
        }
    }

    fn transmit(&mut self, timestamp: SmoltcpInstant) -> Option<Self::TxToken<'_>> {
        match self.provider.transmit(from_smoltcp_instant(timestamp)) {
            TransmitOutcome::Ready(tx) => Some(FrameTxToken(tx)),
            TransmitOutcome::Exhausted => {
                self.blocked_work = true;
                None
            },
            TransmitOutcome::LinkUnavailable => None,
        }
    }

    fn capabilities(&self) -> DeviceCapabilities {
        let mut capabilities = DeviceCapabilities::default();
        capabilities.medium = Medium::Ethernet;
        capabilities.max_transmission_unit = self.provider.capabilities().max_frame_len;
        capabilities
    }
}
