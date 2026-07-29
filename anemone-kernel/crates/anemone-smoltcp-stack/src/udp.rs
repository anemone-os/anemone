use alloc::{collections::VecDeque, vec, vec::Vec};

use anemone_net_api::InterfaceId;
use smoltcp::{
    iface::{SocketHandle, SocketSet},
    socket::udp,
    wire::{IpAddress, IpEndpoint, Ipv4Address},
};

const IPV4_HEADER_LEN: usize = 20;
const UDP_HEADER_LEN: usize = 8;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct EndpointId(u32);

impl EndpointId {
    #[cfg(feature = "host-test")]
    pub(crate) const fn from_raw(raw: u32) -> Self {
        Self(raw)
    }

    #[cfg(feature = "host-test")]
    pub(crate) const fn raw(self) -> u32 {
        self.0
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum EndpointCreateError {
    InvalidPort,
    PortInUse,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum SendError {
    UnknownEndpoint,
    MissingSelection,
    UnknownInterface,
    UnsupportedSource,
    InvalidDestination,
    Oversize { maximum: usize },
    TxFull,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum RetireError {
    UnknownEndpoint,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct ReceivedDatagram {
    pub(crate) payload: Vec<u8>,
    pub(crate) source: IpEndpoint,
}

#[derive(Clone, Copy)]
pub(crate) struct EngineResource {
    interface: InterfaceId,
    handle: SocketHandle,
}

struct PendingDatagram {
    selected_interface: InterfaceId,
    source: Ipv4Address,
    destination: IpEndpoint,
    payload: Vec<u8>,
}

enum TxPhase {
    Idle,
    Queued(PendingDatagram),
    EngineOwned { interface: InterfaceId },
}

pub(crate) struct Endpoint {
    id: EndpointId,
    // This is the sole provisional binding truth. Per-interface smoltcp
    // bindings below are private engine projections and never decide conflict.
    port: u16,
    engines: Vec<EngineResource>,
    tx: TxPhase,
    received: VecDeque<ReceivedDatagram>,
    receive_packet_capacity: usize,
    engine_payload_capacity: usize,
}

impl Endpoint {
    fn new(
        id: EndpointId,
        port: u16,
        receive_packet_capacity: usize,
        engine_payload_capacity: usize,
    ) -> Self {
        assert!(receive_packet_capacity > 0);
        assert!(engine_payload_capacity > 0);
        Self {
            id,
            port,
            engines: Vec::new(),
            tx: TxPhase::Idle,
            received: VecDeque::with_capacity(receive_packet_capacity),
            receive_packet_capacity,
            engine_payload_capacity,
        }
    }

    pub(crate) fn add_engine(&mut self, interface: InterfaceId, sockets: &mut SocketSet<'static>) {
        assert!(
            self.engines
                .iter()
                .all(|engine| engine.interface != interface),
            "an Endpoint cannot have two engine resources for one interface"
        );
        let mut socket = udp::Socket::new(
            udp::PacketBuffer::new(
                vec![udp::PacketMetadata::EMPTY],
                vec![0; self.engine_payload_capacity],
            ),
            udp::PacketBuffer::new(
                vec![udp::PacketMetadata::EMPTY],
                vec![0; self.engine_payload_capacity],
            ),
        );
        socket
            .bind(self.port)
            .expect("a non-zero provisional binding must be accepted");
        let handle = sockets.add(socket);
        self.engines.push(EngineResource { interface, handle });
    }

    fn remove_engine(&mut self, interface: InterfaceId, sockets: &mut SocketSet<'static>) {
        let Some(index) = self
            .engines
            .iter()
            .position(|engine| engine.interface == interface)
        else {
            return;
        };
        let engine = self.engines.remove(index);
        sockets.remove(engine.handle);
        if matches!(
            self.tx,
            TxPhase::Queued(PendingDatagram {
                selected_interface,
                ..
            }) | TxPhase::EngineOwned {
                interface: selected_interface,
            } if selected_interface == interface
        ) {
            self.tx = TxPhase::Idle;
        }
    }

    fn engine(&self, interface: InterfaceId) -> Option<EngineResource> {
        self.engines
            .iter()
            .copied()
            .find(|engine| engine.interface == interface)
    }
}

/// Owns the provisional domain-wide Endpoint namespace and aggregate state.
///
/// Engine sockets remain inside their `SocketSet`; the mapping here is the
/// only authority that can bind, select, drain, or retire them as one Endpoint.
pub(crate) struct UdpEndpoints {
    endpoints: Vec<Endpoint>,
    next_id: u32,
    next_egress_endpoint: usize,
}

impl UdpEndpoints {
    pub(crate) const fn new() -> Self {
        Self {
            endpoints: Vec::new(),
            next_id: 0,
            next_egress_endpoint: 0,
        }
    }

    pub(crate) fn prepare_endpoint(
        &mut self,
        port: u16,
        receive_packet_capacity: usize,
        engine_payload_capacity: usize,
    ) -> Result<Endpoint, EndpointCreateError> {
        if port == 0 {
            return Err(EndpointCreateError::InvalidPort);
        }
        if self.endpoints.iter().any(|endpoint| endpoint.port == port) {
            return Err(EndpointCreateError::PortInUse);
        }
        let raw = self.next_id;
        self.next_id = raw.checked_add(1).expect("EndpointId namespace exhausted");
        Ok(Endpoint::new(
            EndpointId(raw),
            port,
            receive_packet_capacity,
            engine_payload_capacity,
        ))
    }

    pub(crate) fn publish_endpoint(&mut self, endpoint: Endpoint) -> EndpointId {
        let id = endpoint.id;
        self.endpoints.push(endpoint);
        id
    }

    pub(crate) fn add_interface(
        &mut self,
        interface: InterfaceId,
        sockets: &mut SocketSet<'static>,
    ) {
        for endpoint in &mut self.endpoints {
            endpoint.add_engine(interface, sockets);
        }
    }

    pub(crate) fn remove_interface(
        &mut self,
        interface: InterfaceId,
        sockets: &mut SocketSet<'static>,
    ) {
        // Withdraw the owner mapping before removing the private engine object.
        for endpoint in &mut self.endpoints {
            endpoint.remove_engine(interface, sockets);
        }
    }

    pub(crate) fn queue_send(
        &mut self,
        endpoint_id: EndpointId,
        selected_interface: Option<InterfaceId>,
        source: Ipv4Address,
        destination: IpEndpoint,
        payload: &[u8],
        interface_ip_mtu: usize,
    ) -> Result<(), SendError> {
        let selected_interface = selected_interface.ok_or(SendError::MissingSelection)?;
        if destination.addr.is_unspecified() || destination.port == 0 {
            return Err(SendError::InvalidDestination);
        }
        let endpoint = self
            .endpoints
            .iter_mut()
            .find(|endpoint| endpoint.id == endpoint_id)
            .ok_or(SendError::UnknownEndpoint)?;
        // Admission owns both limits that the private engine must satisfy.
        // Accepting against MTU alone would defer an engine-buffer failure to
        // pump time, after the operation has already reported success.
        let Some(interface_payload_capacity) =
            interface_ip_mtu.checked_sub(IPV4_HEADER_LEN + UDP_HEADER_LEN)
        else {
            // A zero-length UDP payload still needs both headers. Reporting a
            // zero payload maximum as admissible here would defer failure to
            // the device after the operation had already committed success.
            return Err(SendError::Oversize { maximum: 0 });
        };
        let maximum = interface_payload_capacity.min(endpoint.engine_payload_capacity);
        if payload.len() > maximum {
            return Err(SendError::Oversize { maximum });
        }
        if endpoint.engine(selected_interface).is_none() {
            return Err(SendError::UnknownInterface);
        }
        if !matches!(endpoint.tx, TxPhase::Idle) {
            return Err(SendError::TxFull);
        }
        endpoint.tx = TxPhase::Queued(PendingDatagram {
            selected_interface,
            source,
            destination,
            payload: payload.to_vec(),
        });
        Ok(())
    }

    /// Moves at most one Endpoint's datagram into an interface engine.
    ///
    /// A previous engine-owned datagram has priority so a blocked provider
    /// cannot be bypassed by another logical datagram on the same interface.
    pub(crate) fn prepare_egress(
        &mut self,
        interface: InterfaceId,
        sockets: &mut SocketSet<'static>,
    ) -> Option<EndpointId> {
        if let Some((index, endpoint)) = self
            .endpoints
            .iter()
            .enumerate()
            .find(|(_, endpoint)| {
                matches!(endpoint.tx, TxPhase::EngineOwned { interface: owner } if owner == interface)
            })
        {
            self.next_egress_endpoint = (index + 1) % self.endpoints.len();
            return Some(endpoint.id);
        }
        if self.endpoints.is_empty() {
            return None;
        }

        let len = self.endpoints.len();
        for offset in 0..len {
            let index = (self.next_egress_endpoint + offset) % len;
            let endpoint = &mut self.endpoints[index];
            let selected = matches!(
                &endpoint.tx,
                TxPhase::Queued(pending) if pending.selected_interface == interface
            );
            if !selected {
                continue;
            }
            let TxPhase::Queued(pending) = core::mem::replace(&mut endpoint.tx, TxPhase::Idle)
            else {
                unreachable!();
            };
            let engine = endpoint
                .engine(interface)
                .expect("selected interface was validated before commit");
            let result = sockets.get_mut::<udp::Socket>(engine.handle).send_slice(
                &pending.payload,
                udp::UdpMetadata {
                    endpoint: pending.destination,
                    local_address: Some(IpAddress::Ipv4(pending.source)),
                    meta: Default::default(),
                },
            );
            assert!(
                result.is_ok(),
                "owner admission must reserve enough private engine capacity"
            );
            endpoint.tx = TxPhase::EngineOwned { interface };
            self.next_egress_endpoint = (index + 1) % len;
            return Some(endpoint.id);
        }
        None
    }

    pub(crate) fn complete_egress(
        &mut self,
        endpoint_id: Option<EndpointId>,
        interface: InterfaceId,
        sockets: &SocketSet<'static>,
    ) -> bool {
        let Some(endpoint_id) = endpoint_id else {
            return false;
        };
        let endpoint = self
            .endpoints
            .iter_mut()
            .find(|endpoint| endpoint.id == endpoint_id)
            .expect("active engine owner must remain published during pump");
        let engine = endpoint
            .engine(interface)
            .expect("active engine mapping must remain published during pump");
        if sockets.get::<udp::Socket>(engine.handle).send_queue() == 0 {
            endpoint.tx = TxPhase::Idle;
            false
        } else {
            true
        }
    }

    pub(crate) fn drain_ingress(
        &mut self,
        interface: InterfaceId,
        sockets: &mut SocketSet<'static>,
    ) {
        for endpoint in &mut self.endpoints {
            let Some(engine) = endpoint.engine(interface) else {
                continue;
            };
            let socket = sockets.get_mut::<udp::Socket>(engine.handle);
            while endpoint.received.len() < endpoint.receive_packet_capacity && socket.can_recv() {
                let (payload, metadata) = socket
                    .recv()
                    .expect("can_recv must imply one engine-owned datagram");
                endpoint.received.push_back(ReceivedDatagram {
                    payload: payload.to_vec(),
                    source: metadata.endpoint,
                });
            }
            // A full aggregate queue leaves this Endpoint's oldest engine
            // datagram in place. It must not gate other Endpoints or the whole
            // interface: later packets for this full UDP Endpoint may be
            // dropped by its bounded engine queue while unrelated sockets keep
            // making normal ingress progress.
        }
    }

    pub(crate) fn receive(&mut self, id: EndpointId) -> Option<ReceivedDatagram> {
        self.endpoints
            .iter_mut()
            .find(|endpoint| endpoint.id == id)?
            .received
            .pop_front()
    }

    pub(crate) fn withdraw(&mut self, id: EndpointId) -> Result<Endpoint, RetireError> {
        let index = self
            .endpoints
            .iter()
            .position(|endpoint| endpoint.id == id)
            .ok_or(RetireError::UnknownEndpoint)?;
        let endpoint = self.endpoints.remove(index);
        if self.next_egress_endpoint > self.endpoints.len() {
            self.next_egress_endpoint = 0;
        }
        Ok(endpoint)
    }

    pub(crate) fn endpoint(&self, id: EndpointId) -> Option<&Endpoint> {
        self.endpoints.iter().find(|endpoint| endpoint.id == id)
    }
}

impl Default for UdpEndpoints {
    fn default() -> Self {
        Self::new()
    }
}

impl Endpoint {
    pub(crate) fn id(&self) -> EndpointId {
        self.id
    }

    pub(crate) fn engines(&self) -> &[EngineResource] {
        &self.engines
    }

    pub(crate) fn has_pending_tx(&self) -> bool {
        !matches!(self.tx, TxPhase::Idle)
    }

    pub(crate) fn received_len(&self) -> usize {
        self.received.len()
    }
}

impl EngineResource {
    pub(crate) fn interface(self) -> InterfaceId {
        self.interface
    }

    pub(crate) fn handle(self) -> SocketHandle {
        self.handle
    }
}
