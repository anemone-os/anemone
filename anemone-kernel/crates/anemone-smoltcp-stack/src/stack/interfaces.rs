//! External-interface mapping owned by the protocol Stack.

use alloc::vec::Vec;

use anemone_net_api::{
    EthernetAddress, FrameProvider, Instant, InterfaceId, Ipv4Address, Ipv4Cidr,
};
use smoltcp::{
    iface::{Config, Interface, SocketSet},
    wire::{
        EthernetAddress as SmoltcpEthernetAddress, HardwareAddress, IpAddress, IpCidr,
        Ipv4Address as SmoltcpIpv4Address,
    },
};

use crate::{
    adapter::{FrameDevice, to_smoltcp_instant},
    local_link::LocalPort,
};

use super::{InterfaceProtocols, Ipv4ConfigError, PumpError, Stack};

#[derive(Clone, Copy)]
pub(crate) enum PumpOrder {
    IngressFirst,
    EgressFirst,
}

impl PumpOrder {
    pub(crate) const fn next(self) -> Self {
        match self {
            Self::IngressFirst => Self::EgressFirst,
            Self::EgressFirst => Self::IngressFirst,
        }
    }
}

pub(crate) struct InterfaceEntry {
    pub(crate) id: InterfaceId,
    // Stable attach-time snapshot required by smoltcp. The provider remains
    // the truth source; every pump asserts that this snapshot is not stale.
    pub(crate) frame_capacity: usize,
    pub(crate) interface: Interface,
    pub(crate) sockets: SocketSet<'static>,
    pub(crate) protocols: InterfaceProtocols,
    // This owner-local cursor chooses only the next software admission order.
    // It is not queue, link, resource, or deadline truth and never bypasses
    // either direction's finite PumpBudget.
    pub(crate) next_pump_order: PumpOrder,
}

impl Stack {
    pub fn add_interface<P: FrameProvider>(
        &mut self,
        provider: &mut P,
        ethernet_address: EthernetAddress,
        now: Instant,
    ) -> InterfaceId {
        let raw_id = self.next_interface_id;
        self.next_interface_id = raw_id
            .checked_add(1)
            .expect("InterfaceId namespace exhausted");
        let id = InterfaceId::from_index(raw_id);
        let frame_capacity = provider.capabilities().max_frame_len;
        let mut device = FrameDevice::new(provider);
        let hardware_address = HardwareAddress::Ethernet(SmoltcpEthernetAddress::from_bytes(
            &ethernet_address.octets(),
        ));
        let interface = Interface::new(
            Config::new(hardware_address),
            &mut device,
            crate::adapter::to_smoltcp_instant(now),
        );

        let mut sockets = SocketSet::new(Vec::new());
        let protocols = self.protocols.attach_interface(id, &mut sockets);
        self.interfaces.push(InterfaceEntry {
            id,
            frame_capacity,
            interface,
            sockets,
            protocols,
            next_pump_order: PumpOrder::IngressFirst,
        });
        id
    }

    /// Withdraws a transaction-local mapping before active publication.
    ///
    /// Interface IDs remain monotonic and are not reused. Runtime detach is not
    /// part of R0; the kernel attach authority only uses this for rollback when
    /// worker/wake/time preparation fails.
    pub fn remove_interface(&mut self, id: InterfaceId) -> Result<(), PumpError> {
        let Some(index) = self.interfaces.iter().position(|entry| entry.id == id) else {
            return Err(PumpError::UnknownInterface(id));
        };
        let mut entry = self.interfaces.remove(index);
        self.protocols
            .detach_interface(id, entry.protocols, &mut entry.sockets);
        Ok(())
    }

    /// Installs the one IP-medium local protocol port.
    ///
    /// Route/source policy remains in the kernel control-plane owner. AnyIP is
    /// only a protocol projection for packets that owner has already selected
    /// onto this bounded local path.
    pub fn add_local_ipv4(
        &mut self,
        loopback: Ipv4Cidr,
        packet_capacity: usize,
        mtu: usize,
        now: Instant,
    ) -> Result<InterfaceId, Ipv4ConfigError> {
        if self.local.is_some() {
            return Err(Ipv4ConfigError::LocalInterfaceAlreadyExists);
        }
        let raw_id = self.next_interface_id;
        self.next_interface_id = raw_id
            .checked_add(1)
            .expect("InterfaceId namespace exhausted");
        let id = InterfaceId::from_index(raw_id);
        let mut local = LocalPort::new(
            id,
            to_smoltcp_instant(now),
            packet_capacity,
            mtu,
            &mut self.protocols,
        );
        let cidr = to_smoltcp_cidr(loopback);
        local.interface.update_ip_addrs(|addresses| {
            assert!(
                addresses.push(cidr).is_ok(),
                "local IPv4 projection is full"
            );
        });
        local.interface.set_any_ip(true);
        self.local = Some(local);
        Ok(id)
    }

    /// Installs the external interface's protocol projection once at boot.
    pub fn configure_external_ipv4(
        &mut self,
        id: InterfaceId,
        cidr: Ipv4Cidr,
        default_gateway: Option<Ipv4Address>,
    ) -> Result<(), Ipv4ConfigError> {
        let Some(entry) = self.interfaces.iter_mut().find(|entry| entry.id == id) else {
            return Err(Ipv4ConfigError::UnknownInterface(id));
        };
        let cidr = to_smoltcp_cidr(cidr);
        entry.interface.update_ip_addrs(|addresses| {
            addresses.clear();
            assert!(
                addresses.push(cidr).is_ok(),
                "external IPv4 projection is full"
            );
        });
        if let Some(gateway) = default_gateway {
            let previous = entry
                .interface
                .routes_mut()
                .add_default_ipv4_route(to_smoltcp_address(gateway))
                .expect("one default IPv4 route must fit");
            assert!(previous.is_none(), "external default route installed twice");
        }
        Ok(())
    }

    /// Adds a /32 local-delivery projection for one configured local address.
    pub fn add_local_delivery_ipv4(&mut self, address: Ipv4Address) -> Result<(), Ipv4ConfigError> {
        let Some(local) = self.local.as_mut() else {
            return Err(Ipv4ConfigError::MissingLocalInterface);
        };
        let cidr = to_smoltcp_cidr(Ipv4Cidr::new(address, 32).expect("/32 is valid"));
        local.interface.update_ip_addrs(|addresses| {
            assert!(
                addresses.iter().all(|existing| *existing != cidr),
                "local IPv4 projection installed twice"
            );
            assert!(
                addresses.push(cidr).is_ok(),
                "local IPv4 projection is full"
            );
        });
        Ok(())
    }

    #[cfg(any(test, feature = "host-test"))]
    pub(crate) fn interface_mut(
        &mut self,
        id: InterfaceId,
    ) -> Result<&mut InterfaceEntry, PumpError> {
        self.interfaces
            .iter_mut()
            .find(|entry| entry.id == id)
            .ok_or(PumpError::UnknownInterface(id))
    }
}

fn to_smoltcp_address(address: Ipv4Address) -> SmoltcpIpv4Address {
    SmoltcpIpv4Address::from_octets(address.octets())
}

fn to_smoltcp_cidr(cidr: Ipv4Cidr) -> IpCidr {
    IpCidr::new(
        IpAddress::Ipv4(to_smoltcp_address(cidr.address())),
        cidr.prefix_len(),
    )
}
