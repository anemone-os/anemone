//! Aggregate UDP operations owned by the protocol Stack.

use anemone_net_api::{
    InterfaceId,
    udp::{
        UdpBindError, UdpBindRequest, UdpCreateError, UdpEndpointId, UdpEndpointLimits,
        UdpLocalBinding, UdpQueryError, UdpRetireError,
    },
};
use smoltcp::wire::{EthernetFrame, IpEndpoint, Ipv4Address};

use crate::udp::SendError;

use super::Stack;

impl Stack {
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

    pub fn create_udp_endpoint(
        &mut self,
        limits: UdpEndpointLimits,
    ) -> Result<UdpEndpointId, UdpCreateError> {
        let mut endpoint = self.udp.prepare_endpoint(limits)?;
        for entry in &mut self.interfaces {
            endpoint.add_engine(entry.id, &mut entry.sockets);
        }
        if let Some(local) = &mut self.local {
            endpoint.add_engine(local.id, &mut local.sockets);
        }
        Ok(self.udp.publish_endpoint(endpoint))
    }

    /// Atomically selects/reserves a port, projects the binding into every
    /// private engine, and commits the endpoint's sole binding truth.
    pub fn bind_udp_endpoint(
        &mut self,
        id: UdpEndpointId,
        request: UdpBindRequest,
        ephemeral_first: u16,
        ephemeral_last: u16,
    ) -> Result<UdpLocalBinding, UdpBindError> {
        let binding = self
            .udp
            .prepare_binding(id, request, ephemeral_first, ephemeral_last)?;
        {
            let endpoint = self
                .udp
                .endpoint(id)
                .expect("prepared UDP endpoint disappeared before projection");
            for entry in &mut self.interfaces {
                endpoint.bind_engine_projection(entry.id, &mut entry.sockets, binding);
            }
            if let Some(local) = &mut self.local {
                endpoint.bind_engine_projection(local.id, &mut local.sockets, binding);
            }
        }
        self.udp.commit_binding(id, binding);
        Ok(binding)
    }

    pub fn udp_endpoint_binding(
        &self,
        id: UdpEndpointId,
    ) -> Result<Option<UdpLocalBinding>, UdpQueryError> {
        self.udp.binding(id)
    }

    #[allow(dead_code)]
    pub(super) fn send_udp(
        &mut self,
        endpoint: UdpEndpointId,
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

    pub fn retire_udp_endpoint(&mut self, id: UdpEndpointId) -> Result<(), UdpRetireError> {
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
