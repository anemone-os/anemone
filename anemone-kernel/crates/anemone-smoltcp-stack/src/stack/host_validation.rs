use alloc::{vec, vec::Vec};

use anemone_net_api::{
    Instant, InterfaceId, Ipv4Address as ApiIpv4Address, Ipv4Cidr as ApiIpv4Cidr,
    Ipv4EgressSelection,
    udp::{
        UdpBindError, UdpBindRequest, UdpCreateError, UdpEndpointFacts, UdpEndpointId,
        UdpEndpointLimits, UdpLocalBinding, UdpPeer, UdpQueryError, UdpSendError,
    },
};
use smoltcp::{socket::raw, wire::IpVersion};

use crate::{
    pump::PumpBudget,
    stack::{Ipv4ConfigError, PumpError, Stack},
};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct HostEndpointId(UdpEndpointId);

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct HostSelection {
    pub interface: InterfaceId,
    pub source: [u8; 4],
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum HostEndpointCreateError {
    InvalidPort,
    EndpointCapacity,
    PortInUse,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum HostSendError {
    UnknownEndpoint,
    UnboundEndpoint,
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
        if port == 0 {
            return Err(HostEndpointCreateError::InvalidPort);
        }
        let id = self
            .create_udp_endpoint(UdpEndpointLimits::new(
                1,
                receive_packet_capacity,
                engine_payload_capacity,
            ))
            .map(HostEndpointId)
            .map_err(HostEndpointCreateError::from)?;
        let result =
            self.bind_udp_endpoint(id.0, UdpBindRequest::new(ApiIpv4Address::UNSPECIFIED, port));
        match result {
            Ok(_) => Ok(id),
            Err(error) => {
                self.retire_udp_endpoint(id.0)
                    .expect("failed host create-and-bind must retire its fresh endpoint");
                Err(match error {
                    UdpBindError::PortInUse => HostEndpointCreateError::PortInUse,
                    UdpBindError::UnknownEndpoint
                    | UdpBindError::AlreadyBound
                    | UdpBindError::EphemeralPortsExhausted => {
                        unreachable!("fresh fixed-port host endpoint must be bindable")
                    },
                })
            },
        }
    }

    pub fn create_unbound_udp_endpoint_for_host_validation(
        &mut self,
        limits: UdpEndpointLimits,
    ) -> Result<HostEndpointId, UdpCreateError> {
        self.create_udp_endpoint(limits).map(HostEndpointId)
    }

    pub fn bind_udp_endpoint_for_host_validation(
        &mut self,
        endpoint: HostEndpointId,
        request: UdpBindRequest,
    ) -> Result<UdpLocalBinding, UdpBindError> {
        self.bind_udp_endpoint(endpoint.0, request)
    }

    pub fn udp_binding_for_host_validation(
        &self,
        endpoint: HostEndpointId,
    ) -> Result<Option<UdpLocalBinding>, UdpQueryError> {
        self.udp_endpoint_binding(endpoint.0)
    }

    pub fn send_udp_for_host_validation(
        &mut self,
        endpoint: HostEndpointId,
        selection: Option<HostSelection>,
        destination_address: [u8; 4],
        destination_port: u16,
        payload: &[u8],
    ) -> Result<(), HostSendError> {
        let selection = selection.ok_or(HostSendError::MissingSelection)?;
        self.send_udp_endpoint(
            endpoint.0,
            Ipv4EgressSelection::new(selection.interface, ApiIpv4Address::new(selection.source)),
            UdpPeer::new(ApiIpv4Address::new(destination_address), destination_port),
            payload,
        )
        .map_err(Into::into)
    }

    pub fn receive_udp_for_host_validation(
        &mut self,
        endpoint: HostEndpointId,
    ) -> Option<HostReceivedDatagram> {
        let datagram = self.receive_udp_endpoint(endpoint.0).ok()?;
        Some(HostReceivedDatagram {
            payload: datagram.payload().to_vec(),
            source_address: datagram.peer().address().octets(),
            source_port: datagram.peer().port(),
        })
    }

    pub fn retire_udp_endpoint_for_host_validation(
        &mut self,
        endpoint: HostEndpointId,
    ) -> Result<(), HostRetireError> {
        self.retire_udp_endpoint(endpoint.0).map_err(Into::into)
    }

    pub fn udp_endpoint_observation_for_host_validation(
        &self,
        endpoint: HostEndpointId,
    ) -> Option<HostEndpointObservation> {
        let endpoint = self.protocols.udp.endpoint(endpoint.0)?;
        Some(HostEndpointObservation {
            engine_resources: endpoint.engines().len(),
            pending_tx: endpoint.has_pending_tx(),
            received_datagrams: endpoint.received_len(),
        })
    }

    pub fn udp_endpoint_facts_for_host_validation(
        &self,
        endpoint: HostEndpointId,
    ) -> Result<UdpEndpointFacts, UdpQueryError> {
        self.udp_endpoint_facts(endpoint.0)
    }

    pub fn take_udp_invalidations_for_host_validation(&mut self) -> Vec<HostEndpointId> {
        self.take_invalidations()
            .into_parts()
            .0
            .into_iter()
            .map(|invalidation| HostEndpointId(invalidation.endpoint()))
            .collect()
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

impl From<UdpCreateError> for HostEndpointCreateError {
    fn from(error: UdpCreateError) -> Self {
        match error {
            UdpCreateError::EndpointCapacity => Self::EndpointCapacity,
        }
    }
}

impl From<UdpSendError> for HostSendError {
    fn from(error: UdpSendError) -> Self {
        match error {
            UdpSendError::UnknownEndpoint => Self::UnknownEndpoint,
            UdpSendError::UnboundEndpoint => Self::UnboundEndpoint,
            UdpSendError::UnknownInterface => Self::UnknownInterface,
            UdpSendError::UnsupportedSource => Self::UnsupportedSource,
            UdpSendError::InvalidDestination => Self::InvalidDestination,
            UdpSendError::MessageTooLong { maximum } => Self::Oversize { maximum },
            UdpSendError::WouldBlock => Self::TxFull,
        }
    }
}

impl From<anemone_net_api::udp::UdpRetireError> for HostRetireError {
    fn from(error: anemone_net_api::udp::UdpRetireError) -> Self {
        match error {
            anemone_net_api::udp::UdpRetireError::UnknownEndpoint => Self::UnknownEndpoint,
        }
    }
}
