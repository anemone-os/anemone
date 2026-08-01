/// Protocol-domain IPv4 address shared by the kernel control plane and Stack.
///
/// This value carries no route, interface, socket, or Linux ABI policy.
#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
#[repr(transparent)]
pub struct Ipv4Address([u8; 4]);

impl Ipv4Address {
    pub const UNSPECIFIED: Self = Self::new([0, 0, 0, 0]);
    pub const LOOPBACK: Self = Self::new([127, 0, 0, 1]);
    pub const LIMITED_BROADCAST: Self = Self::new([255, 255, 255, 255]);

    pub const fn new(octets: [u8; 4]) -> Self {
        Self(octets)
    }

    pub const fn octets(self) -> [u8; 4] {
        self.0
    }

    pub const fn to_bits(self) -> u32 {
        u32::from_be_bytes(self.0)
    }

    pub const fn is_unspecified(self) -> bool {
        self.to_bits() == 0
    }

    pub const fn is_loopback(self) -> bool {
        self.0[0] == 127
    }

    pub const fn is_multicast(self) -> bool {
        self.0[0] >= 224 && self.0[0] <= 239
    }

    pub const fn is_limited_broadcast(self) -> bool {
        self.to_bits() == Self::LIMITED_BROADCAST.to_bits()
    }

    pub const fn is_unicast(self) -> bool {
        !self.is_unspecified() && !self.is_multicast() && !self.is_limited_broadcast()
    }
}

/// Checked IPv4 address/prefix pair.
///
/// The address is preserved exactly; callers decide whether it represents an
/// interface address, a route prefix, or another protocol-domain fact.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub struct Ipv4Cidr {
    address: Ipv4Address,
    prefix_len: u8,
}

impl Ipv4Cidr {
    pub const fn new(address: Ipv4Address, prefix_len: u8) -> Option<Self> {
        if prefix_len > 32 {
            return None;
        }
        Some(Self {
            address,
            prefix_len,
        })
    }

    pub const fn address(self) -> Ipv4Address {
        self.address
    }

    pub const fn prefix_len(self) -> u8 {
        self.prefix_len
    }

    pub const fn network_bits(self) -> u32 {
        self.address.to_bits() & self.netmask_bits()
    }

    pub const fn broadcast_bits(self) -> Option<u32> {
        if self.prefix_len >= 31 {
            return None;
        }
        Some(self.network_bits() | !self.netmask_bits())
    }

    pub const fn contains(self, address: Ipv4Address) -> bool {
        address.to_bits() & self.netmask_bits() == self.network_bits()
    }

    const fn netmask_bits(self) -> u32 {
        if self.prefix_len == 0 {
            0
        } else {
            u32::MAX << (32 - self.prefix_len)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cidr_preserves_address_and_checks_membership() {
        let cidr = Ipv4Cidr::new(Ipv4Address::new([10, 0, 2, 15]), 24).unwrap();
        assert_eq!(cidr.address().octets(), [10, 0, 2, 15]);
        assert_eq!(cidr.network_bits(), u32::from_be_bytes([10, 0, 2, 0]));
        assert_eq!(
            cidr.broadcast_bits(),
            Some(u32::from_be_bytes([10, 0, 2, 255]))
        );
        assert!(cidr.contains(Ipv4Address::new([10, 0, 2, 200])));
        assert!(!cidr.contains(Ipv4Address::new([10, 0, 3, 1])));
        assert!(Ipv4Cidr::new(Ipv4Address::LOOPBACK, 33).is_none());
    }

    #[test]
    fn address_classes_are_mechanical_protocol_facts() {
        assert!(Ipv4Address::UNSPECIFIED.is_unspecified());
        assert!(Ipv4Address::new([127, 3, 2, 1]).is_loopback());
        assert!(Ipv4Address::new([239, 1, 2, 3]).is_multicast());
        assert!(Ipv4Address::LIMITED_BROADCAST.is_limited_broadcast());
        assert!(Ipv4Address::new([10, 0, 2, 15]).is_unicast());
    }
}
