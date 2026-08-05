//! Aggregate IPv4 ICMP raw operations owned by the protocol Stack.

use anemone_net_api::{
    InterfaceId, Ipv4Address, Ipv4EgressSelection,
    icmp_raw::{
        IcmpRawCreateError, IcmpRawDropDiagnostics, IcmpRawEgressPolicy, IcmpRawEndpointConfig,
        IcmpRawEndpointFacts, IcmpRawEndpointId, IcmpRawEndpointLimits, IcmpRawMutationError,
        IcmpRawQueryError, IcmpRawReceiveError, IcmpRawReceivedPacket, IcmpRawRetireError,
        IcmpRawSendError, IcmpRawTypeFilter,
    },
};
use smoltcp::wire::{IpCidr, Ipv4Address as SmoltcpIpv4Address};

use super::{ProtocolProgression, Stack};

impl Stack {
    pub fn create_icmp_raw_endpoint(
        &mut self,
        limits: IcmpRawEndpointLimits,
    ) -> Result<IcmpRawEndpointId, IcmpRawCreateError> {
        self.protocols.icmp_raw.create(limits)
    }

    pub fn bind_icmp_raw_endpoint(
        &mut self,
        endpoint: IcmpRawEndpointId,
        local: Option<Ipv4Address>,
    ) -> Result<(), IcmpRawMutationError> {
        self.protocols.icmp_raw.bind(endpoint, local)
    }

    pub fn connect_icmp_raw_endpoint(
        &mut self,
        endpoint: IcmpRawEndpointId,
        selected_source: Ipv4Address,
        peer: Ipv4Address,
    ) -> Result<(), IcmpRawMutationError> {
        self.protocols
            .icmp_raw
            .connect(endpoint, selected_source, peer)
    }

    pub fn disconnect_icmp_raw_endpoint(
        &mut self,
        endpoint: IcmpRawEndpointId,
    ) -> Result<(), IcmpRawMutationError> {
        self.protocols.icmp_raw.disconnect(endpoint)
    }

    pub fn set_icmp_raw_filter(
        &mut self,
        endpoint: IcmpRawEndpointId,
        filter: IcmpRawTypeFilter,
    ) -> Result<(), IcmpRawMutationError> {
        self.protocols.icmp_raw.set_filter(endpoint, filter)
    }

    pub fn icmp_raw_endpoint_config(
        &self,
        endpoint: IcmpRawEndpointId,
    ) -> Result<IcmpRawEndpointConfig, IcmpRawQueryError> {
        self.protocols.icmp_raw.config(endpoint)
    }

    pub fn icmp_raw_endpoint_facts(
        &self,
        endpoint: IcmpRawEndpointId,
    ) -> Result<IcmpRawEndpointFacts, IcmpRawQueryError> {
        self.protocols.icmp_raw.facts(endpoint)
    }

    pub fn icmp_raw_endpoint_diagnostics(
        &self,
        endpoint: IcmpRawEndpointId,
    ) -> Result<IcmpRawDropDiagnostics, IcmpRawQueryError> {
        self.protocols.icmp_raw.diagnostics(endpoint)
    }

    pub fn send_icmp_raw_endpoint(
        &mut self,
        endpoint: IcmpRawEndpointId,
        selection: Ipv4EgressSelection,
        destination: Ipv4Address,
        policy: IcmpRawEgressPolicy,
        message: &[u8],
    ) -> Result<ProtocolProgression, IcmpRawSendError> {
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
        self.protocols.icmp_raw.queue_send(
            endpoint,
            selection.interface(),
            selection.source(),
            destination,
            policy,
            message,
            ip_mtu,
        )?;
        // ICMP raw owns the successful queue transition and therefore the
        // decision that this interface now has protocol progression work.
        Ok(ProtocolProgression::committed(selection.interface()))
    }

    pub fn receive_icmp_raw_endpoint(
        &mut self,
        endpoint: IcmpRawEndpointId,
        peek: bool,
    ) -> Result<IcmpRawReceivedPacket, IcmpRawReceiveError> {
        self.protocols.icmp_raw.receive(endpoint, peek)
    }

    pub fn retire_icmp_raw_endpoint(
        &mut self,
        endpoint: IcmpRawEndpointId,
    ) -> Result<(), IcmpRawRetireError> {
        let reset_interfaces = self.protocols.icmp_raw.retire(endpoint)?;
        if let Some(local) = &mut self.local {
            local.link.remove_icmp_raw_owner(endpoint);
        }
        for interface in reset_interfaces {
            if let Some(entry) = self
                .interfaces
                .iter_mut()
                .find(|entry| entry.id == interface)
            {
                self.protocols.icmp_raw.replace_egress_engine(
                    &mut entry.protocols.icmp_raw_egress,
                    &mut entry.sockets,
                );
            } else if let Some(local) = self.local.as_mut().filter(|local| local.id == interface) {
                self.protocols.icmp_raw.replace_egress_engine(
                    &mut local.protocols.icmp_raw_egress,
                    &mut local.sockets,
                );
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
