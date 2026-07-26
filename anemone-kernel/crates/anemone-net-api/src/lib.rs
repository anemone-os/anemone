#![no_std]

use core::fmt;

/// Opaque identity assigned by the concrete protocol-stack owner.
///
/// The numeric value is only for equality, maps, and diagnostics. It cannot be
/// used to recover a smoltcp object or a network-device identity.
#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
#[repr(transparent)]
pub struct InterfaceId(u32);

impl InterfaceId {
    pub const fn from_index(index: u32) -> Self {
        Self(index)
    }

    pub const fn index(self) -> u32 {
        self.0
    }
}

#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
#[repr(transparent)]
pub struct EthernetAddress([u8; 6]);

impl EthernetAddress {
    pub const fn new(octets: [u8; 6]) -> Self {
        Self(octets)
    }

    pub const fn octets(self) -> [u8; 6] {
        self.0
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum LinkState {
    Unknown,
    Down,
    Up,
}

/// Stable facts normalized by the network-device owner.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct InterfaceFacts {
    pub ethernet_address: Option<EthernetAddress>,
    pub max_frame_len: usize,
    pub link_state: LinkState,
}

/// Capacity owned by a concrete frame provider.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct FrameCapabilities {
    pub max_frame_len: usize,
}

/// Monotonic time relative to an arbitrary boot-local epoch.
#[derive(Clone, Copy, Debug, Default, Eq, Ord, PartialEq, PartialOrd)]
#[repr(transparent)]
pub struct Instant(i64);

impl Instant {
    pub const ZERO: Self = Self::from_micros(0);

    pub const fn from_micros(micros: i64) -> Self {
        Self(micros)
    }

    pub const fn total_micros(self) -> i64 {
        self.0
    }
}

/// Non-negative monotonic duration.
#[derive(Clone, Copy, Debug, Default, Eq, Ord, PartialEq, PartialOrd)]
#[repr(transparent)]
pub struct Duration(u64);

impl Duration {
    pub const ZERO: Self = Self::from_micros(0);

    pub const fn from_micros(micros: u64) -> Self {
        Self(micros)
    }

    pub const fn total_micros(self) -> u64 {
        self.0
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Recheck {
    Idle,
    Immediate,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct PumpOutcome {
    pub work_remaining: bool,
    pub recheck: Recheck,
    pub next_deadline: Option<Instant>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct FrameSizeError {
    requested: usize,
    capacity: usize,
}

impl FrameSizeError {
    pub const fn new(requested: usize, capacity: usize) -> Self {
        Self {
            requested,
            capacity,
        }
    }

    pub const fn requested(self) -> usize {
        self.requested
    }

    pub const fn capacity(self) -> usize {
        self.capacity
    }
}

impl fmt::Display for FrameSizeError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "frame length {} exceeds token capacity {}",
            self.requested, self.capacity
        )
    }
}

/// One-shot access to a received Ethernet frame.
///
/// The slice cannot escape the callback:
///
/// ```compile_fail
/// use anemone_net_api::RxToken;
///
/// fn leak<T: RxToken>(token: T) -> &'static [u8] {
///     token.consume(|frame| frame)
/// }
/// ```
pub trait RxToken {
    fn consume<R, F>(self, f: F) -> R
    where
        F: FnOnce(&[u8]) -> R;
}

/// One-shot access to a transmit backing owned by the provider.
///
/// The slice cannot escape the callback:
///
/// ```compile_fail
/// use anemone_net_api::TxToken;
///
/// fn leak<T: TxToken>(token: T) -> &'static mut [u8] {
///     token.consume(1, |frame| frame).unwrap()
/// }
/// ```
pub trait TxToken {
    fn capacity(&self) -> usize;

    fn consume<R, F>(self, len: usize, f: F) -> Result<R, FrameSizeError>
    where
        F: FnOnce(&mut [u8]) -> R;
}

#[derive(Debug, Eq, PartialEq)]
pub enum ReceiveOutcome<R, T> {
    Ready { rx: R, tx: T },
    Empty,
    TransmitExhausted,
    LinkUnavailable,
}

#[derive(Debug, Eq, PartialEq)]
pub enum TransmitOutcome<T> {
    Ready(T),
    Exhausted,
    LinkUnavailable,
}

/// Frame-level capability shared by test and production providers.
///
/// Associated tokens keep concrete backing, queue, and DMA identity inside the
/// provider. Implementations must restore an unconsumed reservation from the
/// token's `Drop` path.
pub trait FrameProvider {
    type RxToken<'a>: RxToken
    where
        Self: 'a;
    type TxToken<'a>: TxToken
    where
        Self: 'a;

    fn receive(&mut self, now: Instant) -> ReceiveOutcome<Self::RxToken<'_>, Self::TxToken<'_>>;

    fn transmit(&mut self, now: Instant) -> TransmitOutcome<Self::TxToken<'_>>;

    fn capabilities(&self) -> FrameCapabilities;

    fn link_state(&self) -> LinkState;
}
