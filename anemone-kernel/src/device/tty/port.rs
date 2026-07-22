use crate::{prelude::SysError, utils::identity::AnyIdentity};

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub(crate) struct TtyPortId(AnyIdentity);

impl TryFrom<&str> for TtyPortId {
    type Error = SysError;

    fn try_from(value: &str) -> Result<Self, Self::Error> {
        AnyIdentity::try_from(value)
            .map(Self)
            .map_err(|_| SysError::NameTooLong)
    }
}

impl core::fmt::Display for TtyPortId {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        self.0.fmt(f)
    }
}

/// Narrow transport capability implemented by a physical serial-port owner.
///
/// The port remains the only owner of raw RX storage and TX serialization. The
/// TTY layer may observe the durable RX predicate, dequeue in FIFO order, and
/// submit bounded TX work, but cannot reach registers, locks, or raw storage.
pub(crate) trait TtyPort: Send + Sync {
    fn id(&self) -> &TtyPortId;

    fn rx_pending(&self) -> bool;

    /// Dequeue up to `dst.len()` bytes in FIFO order.
    ///
    /// TTY is the only RX consumer, so a true `rx_pending()` observation must
    /// make progress here unless another TTY worker already drained the bytes.
    fn dequeue_rx(&self, dst: &mut [u8]) -> usize;

    /// Submit bytes through the port owner's bounded TX serialization and
    /// return the number accepted before timeout or backpressure.
    fn submit_tx(&self, src: &[u8]) -> usize;
}
