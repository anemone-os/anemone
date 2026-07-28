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
