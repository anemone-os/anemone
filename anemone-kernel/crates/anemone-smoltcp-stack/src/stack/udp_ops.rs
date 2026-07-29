//! Dormant aggregate UDP operations owned by the protocol Stack.

use anemone_net_api::InterfaceId;
use smoltcp::wire::{EthernetFrame, IpEndpoint, Ipv4Address};

use crate::udp::{EndpointCreateError, EndpointId, RetireError, SendError};

use super::Stack;

#[cfg(feature = "kunit")]
use alloc::vec::Vec;
#[cfg(feature = "kunit")]
use anemone_net_api::Ipv4Address as ApiIpv4Address;
#[cfg(feature = "kunit")]
use smoltcp::wire::IpAddress;

/// Opaque conditional bridge used only by kernel owner KUnit.
///
/// Stage 3 must delete every operation replaced by the real Endpoint consumer.
#[cfg(feature = "kunit")]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct KunitEndpointId(EndpointId);

#[cfg(feature = "kunit")]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum KunitEndpointCreateError {
    InvalidPort,
    PortInUse,
}

#[cfg(feature = "kunit")]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum KunitSendError {
    UnknownEndpoint,
    UnknownInterface,
    UnsupportedSource,
    InvalidDestination,
    Oversize { maximum: usize },
    TxFull,
}

#[cfg(feature = "kunit")]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum KunitRetireError {
    UnknownEndpoint,
}

#[cfg(feature = "kunit")]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct KunitReceivedDatagram {
    pub payload: Vec<u8>,
    pub source_address: ApiIpv4Address,
    pub source_port: u16,
}

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

    /// Conditional operation bridge for the production-path kernel KUnit.
    /// Delete it when Stage 3's real Endpoint owner calls the private aggregate
    /// operations directly through its accepted narrow capability.
    #[cfg(feature = "kunit")]
    pub fn create_udp_endpoint_for_kunit(
        &mut self,
        port: u16,
        receive_packet_capacity: usize,
        engine_payload_capacity: usize,
    ) -> Result<KunitEndpointId, KunitEndpointCreateError> {
        self.create_udp_endpoint(port, receive_packet_capacity, engine_payload_capacity)
            .map(KunitEndpointId)
            .map_err(|error| match error {
                EndpointCreateError::InvalidPort => KunitEndpointCreateError::InvalidPort,
                EndpointCreateError::PortInUse => KunitEndpointCreateError::PortInUse,
            })
    }

    /// Conditional operation bridge for the production-path kernel KUnit.
    /// Route and source selection remain outside Stack and are supplied by the
    /// real control-plane owner.
    #[cfg(feature = "kunit")]
    pub fn send_udp_for_kunit(
        &mut self,
        endpoint: KunitEndpointId,
        interface: InterfaceId,
        source: ApiIpv4Address,
        destination: ApiIpv4Address,
        destination_port: u16,
        payload: &[u8],
    ) -> Result<(), KunitSendError> {
        self.send_udp(
            endpoint.0,
            Some(interface),
            Ipv4Address::from_octets(source.octets()),
            IpEndpoint::new(
                IpAddress::Ipv4(Ipv4Address::from_octets(destination.octets())),
                destination_port,
            ),
            payload,
        )
        .map_err(|error| match error {
            SendError::UnknownEndpoint => KunitSendError::UnknownEndpoint,
            SendError::MissingSelection => unreachable!("KUnit always supplies a selection"),
            SendError::UnknownInterface => KunitSendError::UnknownInterface,
            SendError::UnsupportedSource => KunitSendError::UnsupportedSource,
            SendError::InvalidDestination => KunitSendError::InvalidDestination,
            SendError::Oversize { maximum } => KunitSendError::Oversize { maximum },
            SendError::TxFull => KunitSendError::TxFull,
        })
    }

    #[cfg(feature = "kunit")]
    pub fn receive_udp_for_kunit(
        &mut self,
        endpoint: KunitEndpointId,
    ) -> Option<KunitReceivedDatagram> {
        let datagram = self.udp.receive(endpoint.0)?;
        let IpAddress::Ipv4(source_address) = datagram.source.addr;
        Some(KunitReceivedDatagram {
            payload: datagram.payload,
            source_address: ApiIpv4Address::new(source_address.octets()),
            source_port: datagram.source.port,
        })
    }

    #[cfg(feature = "kunit")]
    pub fn retire_udp_endpoint_for_kunit(
        &mut self,
        endpoint: KunitEndpointId,
    ) -> Result<(), KunitRetireError> {
        self.retire_udp_endpoint(endpoint.0)
            .map_err(|error| match error {
                RetireError::UnknownEndpoint => KunitRetireError::UnknownEndpoint,
            })
    }
}
