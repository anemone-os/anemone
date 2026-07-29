use alloc::{collections::VecDeque, vec, vec::Vec};

use anemone_net_api::InterfaceId;
use smoltcp::{
    iface::{SocketHandle, SocketSet},
    socket::udp,
};

use super::datagram::{PendingDatagram, ReceivedDatagram, TxPhase};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct EndpointId(pub(super) u32);

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

#[derive(Clone, Copy)]
pub(crate) struct EngineResource {
    pub(super) interface: InterfaceId,
    pub(super) handle: SocketHandle,
}

pub(crate) struct Endpoint {
    pub(super) id: EndpointId,
    // This is the sole provisional binding truth. Per-interface smoltcp
    // bindings below are private engine projections and never decide conflict.
    pub(super) port: u16,
    engines: Vec<EngineResource>,
    pub(super) tx: TxPhase,
    pub(super) received: VecDeque<ReceivedDatagram>,
    pub(super) receive_packet_capacity: usize,
    pub(super) engine_payload_capacity: usize,
}

impl Endpoint {
    pub(super) fn new(
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

    pub(super) fn remove_engine(
        &mut self,
        interface: InterfaceId,
        sockets: &mut SocketSet<'static>,
    ) {
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

    pub(super) fn engine(&self, interface: InterfaceId) -> Option<EngineResource> {
        self.engines
            .iter()
            .copied()
            .find(|engine| engine.interface == interface)
    }

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
