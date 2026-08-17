use crate::{prelude::SysError, utils::identity::AnyIdentity};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum TtyParity {
    None,
    Odd,
    Even,
}

/// Immutable, owner-neutral view of the line configuration applied at boot.
///
/// The physical driver remains authoritative for hardware state. A Terminal
/// copies this stable snapshot exactly once while the endpoint is unpublished;
/// runtime register reads must not replace it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct TtyLineSnapshot {
    pub(crate) baud: u32,
    pub(crate) parity: TtyParity,
    pub(crate) data_bits: u8,
}

/// One ordered receive condition transferred from a physical port to TTY.
///
/// The physical driver owns hardware-status classification, while `Terminal`
/// owns termios policy. Keeping the condition attached to its byte prevents a
/// sideband error stream from becoming a second RX truth.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum TtyRxUnit {
    Byte(u8),
    Break,
    FaultedByte(u8),
}

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

impl TtyPortId {
    pub(crate) fn as_str(&self) -> &str {
        self.0.as_str()
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

    /// Dequeue up to `dst.len()` receive units in FIFO order.
    ///
    /// TTY is the only RX consumer, so a true `rx_pending()` observation must
    /// make progress here unless another TTY worker already drained the units.
    fn dequeue_rx(&self, dst: &mut [TtyRxUnit]) -> usize;

    /// Discard RX units already admitted to the port-owned software queue.
    ///
    /// Hardware samples not yet classified by the IRQ owner may arrive after
    /// this operation. Worker-local units are retired by the TTY attachment's
    /// flush generation rather than by the physical owner.
    fn discard_rx(&self);

    /// Submit bytes through the port owner's bounded TX serialization and
    /// return the number accepted before timeout or backpressure.
    fn submit_tx(&self, src: &[u8]) -> usize;

    /// Observe whether the physical transmitter has completely drained.
    ///
    /// This is a snapshot, not a completion notification. Callers must pair it
    /// with their own register-plus-recheck protocol.
    fn tx_idle(&self) -> bool;
}
