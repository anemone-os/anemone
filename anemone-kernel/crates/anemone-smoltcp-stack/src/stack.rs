use alloc::vec::Vec;

use anemone_net_api::{EthernetAddress, FrameProvider, Instant, InterfaceId};
use smoltcp::{
    iface::{Config, Interface, SocketSet},
    wire::{EthernetAddress as SmoltcpEthernetAddress, HardwareAddress},
};

use crate::{
    adapter::FrameDevice,
    local_link::LocalPort,
    udp::{EndpointCreateError, EndpointId, RetireError, SendError, UdpEndpoints},
};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum PumpError {
    UnknownInterface(InterfaceId),
}

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
    // This owner-local cursor chooses only the next software admission order.
    // It is not queue, link, resource, or deadline truth and never bypasses
    // either direction's finite PumpBudget.
    pub(crate) next_pump_order: PumpOrder,
}

/// Owns the private smoltcp interface resources and their opaque ID mapping.
///
/// `&mut Stack` is the unique pump capability. The kernel wiring owner may put
/// the stack behind its chosen synchronization primitive, but admission,
/// contention, and requeue policy stay outside this protocol-state owner.
#[derive(Default)]
pub struct Stack {
    pub(crate) interfaces: Vec<InterfaceEntry>,
    #[allow(dead_code)]
    pub(crate) local: Option<LocalPort>,
    pub(crate) udp: UdpEndpoints,
    pub(crate) next_interface_id: u32,
}

impl Stack {
    pub const fn new() -> Self {
        Self {
            interfaces: Vec::new(),
            local: None,
            udp: UdpEndpoints::new(),
            next_interface_id: 0,
        }
    }

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
        self.udp.add_interface(id, &mut sockets);
        self.interfaces.push(InterfaceEntry {
            id,
            frame_capacity,
            interface,
            sockets,
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
        self.udp.remove_interface(id, &mut entry.sockets);
        Ok(())
    }

    #[allow(dead_code)]
    fn interface_ipv4_and_mtu(
        &self,
        id: InterfaceId,
        source: smoltcp::wire::Ipv4Address,
    ) -> Option<(bool, usize)> {
        if let Some(entry) = self.interfaces.iter().find(|entry| entry.id == id) {
            let ip_mtu = entry
                .frame_capacity
                .checked_sub(smoltcp::wire::EthernetFrame::<&[u8]>::header_len())?;
            return Some((entry.interface.has_ip_addr(source), ip_mtu));
        }
        self.local
            .as_ref()
            .filter(|local| local.id == id)
            .map(|local| (local.interface.has_ip_addr(source), local.ip_mtu()))
    }

    #[allow(dead_code)]
    fn create_udp_endpoint(
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
    fn send_udp(
        &mut self,
        endpoint: EndpointId,
        selected_interface: Option<InterfaceId>,
        source: smoltcp::wire::Ipv4Address,
        destination: smoltcp::wire::IpEndpoint,
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
    fn retire_udp_endpoint(&mut self, id: EndpointId) -> Result<(), RetireError> {
        // Withdraw the aggregate owner before touching private engine objects.
        let endpoint = self.udp.withdraw(id)?;
        for engine in endpoint.engines() {
            if let Some(entry) = self
                .interfaces
                .iter_mut()
                .find(|entry| entry.id == engine.interface())
            {
                let held_receive = entry
                    .sockets
                    .get::<smoltcp::socket::udp::Socket>(engine.handle())
                    .can_recv();
                entry.sockets.remove(engine.handle());
                if held_receive {
                    self.udp.clear_blocked_receive(engine.interface());
                }
                continue;
            }
            if let Some(local) = self
                .local
                .as_mut()
                .filter(|local| local.id == engine.interface())
            {
                let held_receive = local
                    .sockets
                    .get::<smoltcp::socket::udp::Socket>(engine.handle())
                    .can_recv();
                local.sockets.remove(engine.handle());
                if held_receive {
                    self.udp.clear_blocked_receive(engine.interface());
                }
            }
        }
        if let Some(local) = &mut self.local {
            local.link.remove_owner(endpoint.id());
        }
        Ok(())
    }

    pub(crate) fn interface_mut(
        &mut self,
        id: InterfaceId,
    ) -> Result<&mut InterfaceEntry, PumpError> {
        self.interfaces
            .iter_mut()
            .find(|entry| entry.id == id)
            .ok_or(PumpError::UnknownInterface(id))
    }

    /// Installs an IPv4 address solely for the deterministic host fixture.
    ///
    /// `host-test` is absent from the kernel dependency, so this control cannot
    /// become a production address API. Remove it when the fixture can stay
    /// crate-private or an accepted control-plane owner replaces it.
    #[cfg(feature = "host-test")]
    pub fn configure_ipv4_for_host_validation(
        &mut self,
        id: InterfaceId,
        address: [u8; 4],
        prefix_len: u8,
    ) -> Result<(), PumpError> {
        use smoltcp::wire::{IpAddress, IpCidr, Ipv4Address};

        let entry = self.interface_mut(id)?;
        let cidr = IpCidr::new(
            IpAddress::Ipv4(Ipv4Address::from_octets(address)),
            prefix_len,
        );
        entry.interface.update_ip_addrs(|addresses| {
            addresses.clear();
            assert!(addresses.push(cidr).is_ok());
        });
        Ok(())
    }

    /// Queues complete IPv4 packets solely for the deterministic host fixture.
    ///
    /// The socket and its handle stay private to this stack owner. `host-test`
    /// is absent from the kernel dependency, so this cannot become a
    /// production packet-injection or control-plane API.
    #[cfg(feature = "host-test")]
    pub fn queue_ipv4_for_host_validation(
        &mut self,
        id: InterfaceId,
        packets: &[&[u8]],
    ) -> Result<(), PumpError> {
        use alloc::vec;
        use smoltcp::{socket::raw, wire::IpVersion};

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
}

#[cfg(feature = "host-test")]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct HostEndpointId(u32);

#[cfg(feature = "host-test")]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct HostSelection {
    pub interface: InterfaceId,
    pub source: [u8; 4],
}

#[cfg(feature = "host-test")]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum HostEndpointCreateError {
    InvalidPort,
    PortInUse,
}

#[cfg(feature = "host-test")]
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

#[cfg(feature = "host-test")]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum HostRetireError {
    UnknownEndpoint,
}

#[cfg(feature = "host-test")]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct HostReceivedDatagram {
    pub payload: Vec<u8>,
    pub source_address: [u8; 4],
    pub source_port: u16,
}

/// Test-only observation. These fields never participate in owner decisions.
#[cfg(feature = "host-test")]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct HostEndpointObservation {
    pub engine_resources: usize,
    pub pending_tx: bool,
    pub received_datagrams: usize,
}

/// Test-only observation. Occupancy is not an admission input outside owner
/// code.
#[cfg(feature = "host-test")]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct HostLocalLinkObservation {
    pub occupied_packets: usize,
}

#[cfg(feature = "host-test")]
impl Stack {
    /// Installs the production-shaped IP-medium port only for deterministic
    /// validation. The kernel dependency excludes `host-test`; remove this
    /// facade when an accepted control-plane owner supplies the same input.
    pub fn add_local_ipv4_for_host_validation(
        &mut self,
        address: [u8; 4],
        prefix_len: u8,
        packet_capacity: usize,
        mtu: usize,
        now: Instant,
    ) -> InterfaceId {
        assert!(
            self.local.is_none(),
            "the provisional domain has one local port"
        );
        let raw_id = self.next_interface_id;
        self.next_interface_id = raw_id
            .checked_add(1)
            .expect("InterfaceId namespace exhausted");
        let id = InterfaceId::from_index(raw_id);
        let mut local = LocalPort::new(
            id,
            crate::adapter::to_smoltcp_instant(now),
            packet_capacity,
            mtu,
        );
        let cidr = smoltcp::wire::IpCidr::new(
            smoltcp::wire::IpAddress::Ipv4(smoltcp::wire::Ipv4Address::from_octets(address)),
            prefix_len,
        );
        local.interface.update_ip_addrs(|addresses| {
            assert!(addresses.push(cidr).is_ok());
        });
        self.udp.add_interface(id, &mut local.sockets);
        self.local = Some(local);
        id
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
                smoltcp::wire::Ipv4Address::from_octets(selection.source),
            ),
            None => (None, smoltcp::wire::Ipv4Address::UNSPECIFIED),
        };
        self.send_udp(
            EndpointId::from_raw(endpoint.0),
            selected_interface,
            source,
            smoltcp::wire::IpEndpoint::new(
                smoltcp::wire::IpAddress::Ipv4(smoltcp::wire::Ipv4Address::from_octets(
                    destination_address,
                )),
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
        let smoltcp::wire::IpAddress::Ipv4(source_address) = datagram.source.addr;
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
}

#[cfg(feature = "host-test")]
impl From<EndpointCreateError> for HostEndpointCreateError {
    fn from(error: EndpointCreateError) -> Self {
        match error {
            EndpointCreateError::InvalidPort => Self::InvalidPort,
            EndpointCreateError::PortInUse => Self::PortInUse,
        }
    }
}

#[cfg(feature = "host-test")]
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

#[cfg(feature = "host-test")]
impl From<RetireError> for HostRetireError {
    fn from(error: RetireError) -> Self {
        match error {
            RetireError::UnknownEndpoint => Self::UnknownEndpoint,
        }
    }
}
