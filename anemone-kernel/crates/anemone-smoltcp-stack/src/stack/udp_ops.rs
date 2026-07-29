//! Dormant aggregate UDP operations owned by the protocol Stack.

use anemone_net_api::InterfaceId;
use smoltcp::wire::{EthernetFrame, IpEndpoint, Ipv4Address};

use crate::udp::{EndpointCreateError, EndpointId, RetireError, SendError};

use super::Stack;

impl Stack {
    // These private operations now live under the production global Stack
    // owner, but remain dormant until Stage 3 introduces a real Endpoint
    // consumer. The host-only facade continues to exercise them meanwhile.
    #[allow(dead_code)]
    pub(super) fn interface_ipv4_and_mtu(
        &self,
        id: InterfaceId,
        source: Ipv4Address,
    ) -> Option<(bool, usize)> {
        if let Some(entry) = self.interfaces.iter().find(|entry| entry.id == id) {
            let ip_mtu = entry
                .frame_capacity
                .checked_sub(EthernetFrame::<&[u8]>::header_len())?;
            return Some((entry.interface.has_ip_addr(source), ip_mtu));
        }
        self.local
            .as_ref()
            .filter(|local| local.id == id)
            .map(|local| (local.interface.has_ip_addr(source), local.ip_mtu()))
    }

    #[allow(dead_code)]
    pub(super) fn create_udp_endpoint(
        &mut self,
        port: u16,
        receive_packet_capacity: usize,
        engine_payload_capacity: usize,
    ) -> Result<EndpointId, EndpointCreateError> {
        let mut endpoint =
            self.udp
                .prepare_endpoint(port, receive_packet_capacity, engine_payload_capacity)?;
        for entry in &mut self.interfaces {
            endpoint.add_engine(entry.id, &mut entry.sockets);
        }
        if let Some(local) = &mut self.local {
            endpoint.add_engine(local.id, &mut local.sockets);
        }
        Ok(self.udp.publish_endpoint(endpoint))
    }

    #[allow(dead_code)]
    pub(super) fn send_udp(
        &mut self,
        endpoint: EndpointId,
        selected_interface: Option<InterfaceId>,
        source: Ipv4Address,
        destination: IpEndpoint,
        payload: &[u8],
    ) -> Result<(), SendError> {
        let selected = selected_interface.ok_or(SendError::MissingSelection)?;
        let (source_supported, ip_mtu) = self
            .interface_ipv4_and_mtu(selected, source)
            .ok_or(SendError::UnknownInterface)?;
        if !source_supported {
            return Err(SendError::UnsupportedSource);
        }
        self.udp.queue_send(
            endpoint,
            Some(selected),
            source,
            destination,
            payload,
            ip_mtu,
        )
    }

    #[allow(dead_code)]
    pub(super) fn retire_udp_endpoint(&mut self, id: EndpointId) -> Result<(), RetireError> {
        // Withdraw the aggregate owner before touching private engine objects.
        let endpoint = self.udp.withdraw(id)?;
        for engine in endpoint.engines() {
            if let Some(entry) = self
                .interfaces
                .iter_mut()
                .find(|entry| entry.id == engine.interface())
            {
                entry.sockets.remove(engine.handle());
                continue;
            }
            if let Some(local) = self
                .local
                .as_mut()
                .filter(|local| local.id == engine.interface())
            {
                local.sockets.remove(engine.handle());
            }
        }
        if let Some(local) = &mut self.local {
            local.link.remove_owner(endpoint.id());
        }
        Ok(())
    }
}
