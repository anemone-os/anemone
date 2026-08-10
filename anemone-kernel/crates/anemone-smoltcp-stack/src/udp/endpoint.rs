use alloc::{collections::VecDeque, vec, vec::Vec};

use anemone_net_api::{
    InterfaceId,
    udp::{
        UdpEndpointFacts, UdpEndpointId, UdpEndpointLimits, UdpErrorCause, UdpErrorRecord,
        UdpLocalBinding, UdpPeer,
    },
};
use smoltcp::{
    iface::{SocketHandle, SocketSet},
    socket::udp,
    wire::{IpAddress, IpListenEndpoint, Ipv4Address},
};

use super::datagram::{PendingDatagram, ReceivedDatagram, TxPhase};

#[derive(Clone, Copy)]
pub(crate) struct EngineResource {
    pub(super) interface: InterfaceId,
    pub(super) handle: SocketHandle,
}

pub(crate) struct Endpoint {
    pub(super) id: UdpEndpointId,
    /// Sole committed binding truth. Per-interface smoltcp bindings are
    /// private engine projections and never decide conflicts or allocation.
    pub(super) binding: Option<UdpLocalBinding>,
    /// Sole persistent peer truth. Socket/front consumers only query it through
    /// the Stack capability; engine metadata remains an ingress observation.
    pub(super) peer: Option<UdpPeer>,
    engines: Vec<EngineResource>,
    pub(super) tx: TxPhase,
    pub(super) received: VecDeque<ReceivedDatagram>,
    /// Sole opt-in truth for ICMP-origin extended errors.
    pub(super) receive_errors: bool,
    /// Ordered records consumed only by `MSG_ERRQUEUE`.
    pub(super) errors: VecDeque<UdpErrorRecord>,
    /// Latest ordinary error, consumed once by I/O or `SO_ERROR`.
    pub(super) pending_error: Option<UdpErrorCause>,
    pub(super) limits: UdpEndpointLimits,
}

impl Endpoint {
    pub(super) fn new(id: UdpEndpointId, limits: UdpEndpointLimits) -> Self {
        assert!(limits.tx_datagram_capacity() > 0);
        assert!(limits.rx_datagram_capacity() > 0);
        assert!(limits.error_record_capacity() > 0);
        assert!(limits.max_payload_bytes() > 0);
        Self {
            id,
            binding: None,
            peer: None,
            engines: Vec::new(),
            tx: TxPhase::Idle,
            received: VecDeque::with_capacity(limits.rx_datagram_capacity()),
            receive_errors: false,
            errors: VecDeque::with_capacity(limits.error_record_capacity()),
            pending_error: None,
            limits,
        }
    }

    pub(crate) fn add_engine(&mut self, interface: InterfaceId, sockets: &mut SocketSet<'static>) {
        assert!(
            self.engines
                .iter()
                .all(|engine| engine.interface != interface),
            "an Endpoint cannot have two engine resources for one interface"
        );
        let tx_bytes = self
            .limits
            .tx_datagram_capacity()
            .checked_mul(self.limits.max_payload_bytes())
            .expect("validated UDP TX storage size overflowed");
        let rx_bytes = self
            .limits
            .rx_datagram_capacity()
            .checked_mul(self.limits.max_payload_bytes())
            .expect("validated UDP RX storage size overflowed");
        let mut socket = udp::Socket::new(
            udp::PacketBuffer::new(
                vec![udp::PacketMetadata::EMPTY; self.limits.rx_datagram_capacity()],
                vec![0; rx_bytes],
            ),
            udp::PacketBuffer::new(
                vec![udp::PacketMetadata::EMPTY; self.limits.tx_datagram_capacity()],
                vec![0; tx_bytes],
            ),
        );
        if let Some(binding) = self.binding {
            bind_engine(&mut socket, binding);
        }
        let handle = sockets.add(socket);
        self.engines.push(EngineResource { interface, handle });
    }

    pub(crate) fn bind_engine_projection(
        &self,
        interface: InterfaceId,
        sockets: &mut SocketSet<'static>,
        binding: UdpLocalBinding,
    ) {
        let engine = self
            .engine(interface)
            .expect("published interface must have one UDP engine projection");
        bind_engine(sockets.get_mut::<udp::Socket>(engine.handle), binding);
    }

    pub(super) fn commit_binding(&mut self, binding: UdpLocalBinding) {
        assert!(
            self.binding.is_none(),
            "UDP endpoint binding committed twice"
        );
        self.binding = Some(binding);
    }

    pub(super) fn commit_peer(&mut self, peer: UdpPeer) {
        self.peer = Some(peer);
    }

    pub(super) fn remove_engine(
        &mut self,
        interface: InterfaceId,
        sockets: &mut SocketSet<'static>,
    ) -> bool {
        let Some(index) = self
            .engines
            .iter()
            .position(|engine| engine.interface == interface)
        else {
            return false;
        };
        let engine = self.engines.remove(index);
        sockets.remove(engine.handle);
        let released_tx = matches!(
            self.tx,
            TxPhase::Queued(PendingDatagram {
                selected_interface,
                ..
            }) | TxPhase::EngineOwned {
                interface: selected_interface,
            } if selected_interface == interface
        );
        if released_tx {
            self.tx = TxPhase::Idle;
        }
        released_tx
    }

    pub(super) fn engine(&self, interface: InterfaceId) -> Option<EngineResource> {
        self.engines
            .iter()
            .copied()
            .find(|engine| engine.interface == interface)
    }

    pub(crate) fn id(&self) -> UdpEndpointId {
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

    pub(crate) fn facts(&self) -> UdpEndpointFacts {
        UdpEndpointFacts::from_owner_snapshot(
            !self.received.is_empty(),
            matches!(self.tx, TxPhase::Idle),
            self.pending_error.is_some() || !self.errors.is_empty(),
        )
    }
}

fn bind_engine(socket: &mut udp::Socket<'static>, binding: UdpLocalBinding) {
    let address = binding.address();
    let endpoint = IpListenEndpoint {
        addr: (!address.is_unspecified())
            .then(|| IpAddress::Ipv4(Ipv4Address::from_octets(address.octets()))),
        port: binding.port(),
    };
    socket
        .bind(endpoint)
        .expect("validated unbound UDP engine must accept committed binding");
}

impl EngineResource {
    pub(crate) fn interface(self) -> InterfaceId {
        self.interface
    }

    pub(crate) fn handle(self) -> SocketHandle {
        self.handle
    }
}
