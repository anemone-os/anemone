use anemone_smoltcp_stack::udp_probe;

use super::*;

impl DomainStack {
    pub(in crate::net) fn create_udp_probe(
        &self,
        port: u16,
    ) -> Result<udp_probe::EndpointId, udp_probe::CreateError> {
        udp_probe::create(&mut self.stack.lock(), port, 4, NET_LOCAL_LINK_MTU_BYTES)
    }

    pub(in crate::net) fn send_udp_probe(
        &self,
        endpoint: udp_probe::EndpointId,
        interface: InterfaceId,
        source: Ipv4Address,
        destination: Ipv4Address,
        destination_port: u16,
        payload: &[u8],
    ) -> Result<(), udp_probe::SendError> {
        udp_probe::send(
            &mut self.stack.lock(),
            endpoint,
            interface,
            source,
            destination,
            destination_port,
            payload,
        )
    }

    pub(in crate::net) fn receive_udp_probe(
        &self,
        endpoint: udp_probe::EndpointId,
    ) -> Option<udp_probe::ReceivedDatagram> {
        udp_probe::receive(&mut self.stack.lock(), endpoint)
    }

    pub(in crate::net) fn retire_udp_probe(
        &self,
        endpoint: udp_probe::EndpointId,
    ) -> Result<(), udp_probe::RetireError> {
        udp_probe::retire(&mut self.stack.lock(), endpoint)
    }
}
