use alloc::{vec, vec::Vec};

use anemone_net_api::{
    Instant, InterfaceId, Ipv4Address as ApiIpv4Address, Ipv4Cidr as ApiIpv4Cidr,
};
use smoltcp::{
    socket::raw,
    wire::{IpAddress, IpVersion, Ipv4Address},
};

use crate::{
    pump::PumpBudget,
    stack::{Ipv4ConfigError, PumpError, Stack},
    udp::{EndpointCreateError, EndpointId, RetireError, SendError},
};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct HostEndpointId(u32);

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct HostSelection {
    pub interface: InterfaceId,
    pub source: [u8; 4],
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum HostEndpointCreateError {
    InvalidPort,
    PortInUse,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum HostSendError {
    UnknownEndpoint,
    MissingSelection,
    UnknownInterface,
    UnsupportedSource,
    InvalidDestination,
    Oversize { maximum: usize },
    TxFull,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum HostRetireError {
    UnknownEndpoint,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct HostReceivedDatagram {
    pub payload: Vec<u8>,
    pub source_address: [u8; 4],
    pub source_port: u16,
}

/// Test-only observation. These fields never participate in owner decisions.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct HostEndpointObservation {
    pub engine_resources: usize,
    pub pending_tx: bool,
    pub received_datagrams: usize,
}

/// Test-only observation. Occupancy is not an admission input outside owner
/// code.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct HostLocalLinkObservation {
    pub occupied_packets: usize,
}

impl Stack {
    // These conditional methods remain public only because the long-term host
    // matrices are integration tests. Keep this facade limited to host-owned
    // orchestration and observation; `host-test` is absent from the kernel
    // dependency and must not become a production capability surface.

    /// Compatibility wrapper for existing deterministic host fixtures.
    pub fn configure_ipv4_for_host_validation(
        &mut self,
        id: InterfaceId,
        address: [u8; 4],
        prefix_len: u8,
    ) -> Result<(), PumpError> {
        let cidr = ApiIpv4Cidr::new(ApiIpv4Address::new(address), prefix_len)
            .expect("host fixture prefix must be valid");
        self.configure_external_ipv4(id, cidr, None)
            .map_err(|error| match error {
                Ipv4ConfigError::UnknownInterface(id) => PumpError::UnknownInterface(id),
                Ipv4ConfigError::LocalInterfaceAlreadyExists
                | Ipv4ConfigError::MissingLocalInterface => {
                    unreachable!("external projection cannot report a local-interface error")
                },
            })
    }

    /// Queues complete IPv4 packets solely for the deterministic host fixture.
    ///
    /// The socket and its handle stay private to this stack owner. `host-test`
    /// is absent from the kernel dependency, so this cannot become a
    /// production packet-injection or control-plane API.
    pub fn queue_ipv4_for_host_validation(
        &mut self,
        id: InterfaceId,
        packets: &[&[u8]],
    ) -> Result<(), PumpError> {
        let entry = self.interface_mut(id)?;
        let payload_capacity = packets
            .iter()
            .try_fold(0usize, |total, packet| total.checked_add(packet.len()))
            .expect("host validation packet storage capacity overflow");
        let mut socket = raw::Socket::new(
            Some(IpVersion::Ipv4),
            None,
            raw::PacketBuffer::new(vec![raw::PacketMetadata::EMPTY], vec![0; 1]),
            raw::PacketBuffer::new(
                vec![raw::PacketMetadata::EMPTY; packets.len()],
                vec![0; payload_capacity],
            ),
        );
        for packet in packets {
            socket
                .send_slice(packet)
                .expect("sized host validation socket must accept every packet");
        }
        entry.sockets.add(socket);
        Ok(())
    }

    /// Compatibility wrapper for existing deterministic host fixtures.
    pub fn add_local_ipv4_for_host_validation(
        &mut self,
        address: [u8; 4],
        prefix_len: u8,
        packet_capacity: usize,
        mtu: usize,
        now: Instant,
    ) -> InterfaceId {
        let cidr = ApiIpv4Cidr::new(ApiIpv4Address::new(address), prefix_len)
            .expect("host fixture prefix must be valid");
        self.add_local_ipv4(cidr, packet_capacity, mtu, now)
            .expect("host fixture must create exactly one local interface")
    }

    pub fn create_udp_endpoint_for_host_validation(
        &mut self,
        port: u16,
        receive_packet_capacity: usize,
        engine_payload_capacity: usize,
    ) -> Result<HostEndpointId, HostEndpointCreateError> {
        self.create_udp_endpoint(port, receive_packet_capacity, engine_payload_capacity)
            .map(|id| HostEndpointId(id.raw()))
            .map_err(Into::into)
    }

    pub fn send_udp_for_host_validation(
        &mut self,
        endpoint: HostEndpointId,
        selection: Option<HostSelection>,
        destination_address: [u8; 4],
        destination_port: u16,
        payload: &[u8],
    ) -> Result<(), HostSendError> {
        let (selected_interface, source) = match selection {
            Some(selection) => (
                Some(selection.interface),
                Ipv4Address::from_octets(selection.source),
            ),
            None => (None, Ipv4Address::UNSPECIFIED),
        };
        self.send_udp(
            EndpointId::from_raw(endpoint.0),
            selected_interface,
            source,
            smoltcp::wire::IpEndpoint::new(
                IpAddress::Ipv4(Ipv4Address::from_octets(destination_address)),
                destination_port,
            ),
            payload,
        )
        .map_err(Into::into)
    }

    pub fn receive_udp_for_host_validation(
        &mut self,
        endpoint: HostEndpointId,
    ) -> Option<HostReceivedDatagram> {
        let datagram = self.udp.receive(EndpointId::from_raw(endpoint.0))?;
        let IpAddress::Ipv4(source_address) = datagram.source.addr;
        Some(HostReceivedDatagram {
            payload: datagram.payload,
            source_address: source_address.octets(),
            source_port: datagram.source.port,
        })
    }

    pub fn retire_udp_endpoint_for_host_validation(
        &mut self,
        endpoint: HostEndpointId,
    ) -> Result<(), HostRetireError> {
        self.retire_udp_endpoint(EndpointId::from_raw(endpoint.0))
            .map_err(Into::into)
    }

    pub fn udp_endpoint_observation_for_host_validation(
        &self,
        endpoint: HostEndpointId,
    ) -> Option<HostEndpointObservation> {
        let endpoint = self.udp.endpoint(EndpointId::from_raw(endpoint.0))?;
        Some(HostEndpointObservation {
            engine_resources: endpoint.engines().len(),
            pending_tx: endpoint.has_pending_tx(),
            received_datagrams: endpoint.received_len(),
        })
    }

    pub fn local_link_observation_for_host_validation(&self) -> Option<HostLocalLinkObservation> {
        Some(HostLocalLinkObservation {
            occupied_packets: self.local.as_ref()?.link.occupied(),
        })
    }

    /// Conditional facade for the deterministic host fixture. The local pump
    /// itself remains ordinary `no_std + alloc` owner logic.
    pub fn pump_local_for_host_validation(
        &mut self,
        id: InterfaceId,
        now: Instant,
        budget: PumpBudget,
    ) -> Result<anemone_net_api::PumpOutcome, PumpError> {
        self.pump_local(id, now, budget)
    }
}

impl From<EndpointCreateError> for HostEndpointCreateError {
    fn from(error: EndpointCreateError) -> Self {
        match error {
            EndpointCreateError::InvalidPort => Self::InvalidPort,
            EndpointCreateError::PortInUse => Self::PortInUse,
        }
    }
}

impl From<SendError> for HostSendError {
    fn from(error: SendError) -> Self {
        match error {
            SendError::UnknownEndpoint => Self::UnknownEndpoint,
            SendError::MissingSelection => Self::MissingSelection,
            SendError::UnknownInterface => Self::UnknownInterface,
            SendError::UnsupportedSource => Self::UnsupportedSource,
            SendError::InvalidDestination => Self::InvalidDestination,
            SendError::Oversize { maximum } => Self::Oversize { maximum },
            SendError::TxFull => Self::TxFull,
        }
    }
}

impl From<RetireError> for HostRetireError {
    fn from(error: RetireError) -> Self {
        match error {
            RetireError::UnknownEndpoint => Self::UnknownEndpoint,
        }
    }
}
