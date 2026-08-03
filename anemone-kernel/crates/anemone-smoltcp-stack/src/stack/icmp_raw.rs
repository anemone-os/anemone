//! Aggregate IPv4 ICMP raw operations owned by the protocol Stack.

use alloc::vec::Vec;

use anemone_net_api::{
    InterfaceId, Ipv4Address,
    icmp_raw::{
        IcmpRawAssociation, IcmpRawCreateError, IcmpRawDropDiagnostics, IcmpRawEgressPolicy,
        IcmpRawEgressSelection, IcmpRawEndpointConfig, IcmpRawEndpointFacts, IcmpRawEndpointId,
        IcmpRawEndpointInvalidation, IcmpRawEndpointLimits, IcmpRawMutationError,
        IcmpRawQueryError, IcmpRawReceiveError, IcmpRawReceivedPacket, IcmpRawRetireError,
        IcmpRawSendError, IcmpRawTypeFilter,
    },
};
use smoltcp::wire::{IpCidr, Ipv4Address as SmoltcpIpv4Address};

use super::Stack;

impl Stack {
    pub fn create_icmp_raw_endpoint(
        &mut self,
        limits: IcmpRawEndpointLimits,
    ) -> Result<IcmpRawEndpointId, IcmpRawCreateError> {
        self.icmp_raw.create(limits)
    }

    pub fn set_icmp_raw_association(
        &mut self,
        endpoint: IcmpRawEndpointId,
        association: IcmpRawAssociation,
    ) -> Result<(), IcmpRawMutationError> {
        self.icmp_raw.set_association(endpoint, association)
    }

    pub fn set_icmp_raw_filter(
        &mut self,
        endpoint: IcmpRawEndpointId,
        filter: IcmpRawTypeFilter,
    ) -> Result<(), IcmpRawMutationError> {
        self.icmp_raw.set_filter(endpoint, filter)
    }

    pub fn icmp_raw_endpoint_config(
        &self,
        endpoint: IcmpRawEndpointId,
    ) -> Result<IcmpRawEndpointConfig, IcmpRawQueryError> {
        self.icmp_raw.config(endpoint)
    }

    pub fn icmp_raw_endpoint_facts(
        &self,
        endpoint: IcmpRawEndpointId,
    ) -> Result<IcmpRawEndpointFacts, IcmpRawQueryError> {
        self.icmp_raw.facts(endpoint)
    }

    pub fn icmp_raw_endpoint_diagnostics(
        &self,
        endpoint: IcmpRawEndpointId,
    ) -> Result<IcmpRawDropDiagnostics, IcmpRawQueryError> {
        self.icmp_raw.diagnostics(endpoint)
    }

    pub fn take_icmp_raw_endpoint_invalidations(&mut self) -> Vec<IcmpRawEndpointInvalidation> {
        self.icmp_raw.take_invalidations()
    }

    pub fn send_icmp_raw_endpoint(
        &mut self,
        endpoint: IcmpRawEndpointId,
        selection: IcmpRawEgressSelection,
        destination: Ipv4Address,
        policy: IcmpRawEgressPolicy,
        message: &[u8],
    ) -> Result<(), IcmpRawSendError> {
        let source = SmoltcpIpv4Address::from_octets(selection.source().octets());
        let (source_supported, destination_allowed, ip_mtu) = self
            .icmp_raw_interface_facts(selection.interface(), source, destination)
            .ok_or(IcmpRawSendError::UnknownInterface)?;
        if !source_supported {
            return Err(IcmpRawSendError::UnsupportedSource);
        }
        if !destination_allowed {
            return Err(IcmpRawSendError::InvalidDestination);
        }
        self.icmp_raw.queue_send(
            endpoint,
            selection.interface(),
            selection.source(),
            destination,
            policy,
            message,
            ip_mtu,
        )
    }

    pub fn receive_icmp_raw_endpoint(
        &mut self,
        endpoint: IcmpRawEndpointId,
        peek: bool,
    ) -> Result<IcmpRawReceivedPacket, IcmpRawReceiveError> {
        self.icmp_raw.receive(endpoint, peek)
    }

    pub fn retire_icmp_raw_endpoint(
        &mut self,
        endpoint: IcmpRawEndpointId,
    ) -> Result<(), IcmpRawRetireError> {
        let reset_interfaces = self.icmp_raw.retire(endpoint)?;
        if let Some(local) = &mut self.local {
            local.link.remove_icmp_raw_owner(endpoint);
        }
        for interface in reset_interfaces {
            if let Some(entry) = self
                .interfaces
                .iter_mut()
                .find(|entry| entry.id == interface)
            {
                self.icmp_raw
                    .replace_engine(&mut entry.icmp_raw_engine, &mut entry.sockets);
            } else if let Some(local) = self.local.as_mut().filter(|local| local.id == interface) {
                self.icmp_raw
                    .replace_engine(&mut local.icmp_raw_engine, &mut local.sockets);
            }
        }
        Ok(())
    }

    fn icmp_raw_interface_facts(
        &self,
        id: InterfaceId,
        source: SmoltcpIpv4Address,
        destination: Ipv4Address,
    ) -> Option<(bool, bool, usize)> {
        let destination = SmoltcpIpv4Address::from_octets(destination.octets());
        if let Some(entry) = self.interfaces.iter().find(|entry| entry.id == id) {
            let ip_mtu = entry
                .frame_capacity
                .checked_sub(smoltcp::wire::EthernetFrame::<&[u8]>::header_len())?;
            return Some((
                entry.interface.has_ip_addr(source),
                destination_is_unicast_for_interface(&entry.interface, destination),
                ip_mtu,
            ));
        }
        self.local
            .as_ref()
            .filter(|local| local.id == id)
            .map(|local| {
                (
                    local.interface.has_ip_addr(source),
                    destination_is_unicast_for_interface(&local.interface, destination),
                    local.ip_mtu(),
                )
            })
    }
}

fn destination_is_unicast_for_interface(
    interface: &smoltcp::iface::Interface,
    destination: SmoltcpIpv4Address,
) -> bool {
    if !Ipv4Address::new(destination.octets()).is_unicast() {
        return false;
    }
    !interface.ip_addrs().iter().any(|cidr| match *cidr {
        IpCidr::Ipv4(cidr) => cidr
            .broadcast()
            .is_some_and(|broadcast| broadcast == destination),
        #[allow(unreachable_patterns)]
        _ => false,
    })
}
