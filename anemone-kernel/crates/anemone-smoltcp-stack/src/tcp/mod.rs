//! The sole TCP namespace, endpoint, stream, and reclaim owner.

mod facts;
mod listener;
mod namespace;
mod reclaim;
mod stream;

use alloc::{vec, vec::Vec};

use anemone_net_api::{
    InterfaceId,
    tcp::{
        TcpEndpointId, TcpEndpointInvalidation, TcpLocalBinding, TcpPeer, TcpPendingError,
        TcpReceiveMode, TcpReceiveReservationId, TcpReleaseReason,
    },
};
use smoltcp::{
    iface::{SocketHandle, SocketSet},
    socket::{self, tcp},
    wire::IpListenEndpoint,
};

/// Immutable TCP resource policy supplied by the kernel at Stack construction.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct TcpPolicy {
    endpoint_capacity: usize,
    engine_timer_capacity: usize,
    listener_completed_capacity: usize,
    rx_buffer_bytes: usize,
    tx_buffer_bytes: usize,
    deferred_reclaim_capacity: usize,
    connect_timeout_ms: usize,
    orphan_timeout_ms: usize,
    ephemeral_port_first: u16,
    ephemeral_port_last: u16,
}

impl TcpPolicy {
    #[allow(clippy::too_many_arguments)]
    pub const fn new(
        endpoint_capacity: usize,
        engine_timer_capacity: usize,
        listener_completed_capacity: usize,
        rx_buffer_bytes: usize,
        tx_buffer_bytes: usize,
        deferred_reclaim_capacity: usize,
        connect_timeout_ms: usize,
        orphan_timeout_ms: usize,
        ephemeral_port_first: u16,
        ephemeral_port_last: u16,
    ) -> Self {
        Self {
            endpoint_capacity,
            engine_timer_capacity,
            listener_completed_capacity,
            rx_buffer_bytes,
            tx_buffer_bytes,
            deferred_reclaim_capacity,
            connect_timeout_ms,
            orphan_timeout_ms,
            ephemeral_port_first,
            ephemeral_port_last,
        }
    }

    pub(crate) const fn listener_completed_capacity(self) -> usize {
        self.listener_completed_capacity
    }

    pub(crate) const fn connect_timeout_ms(self) -> usize {
        self.connect_timeout_ms
    }
}

pub(crate) struct EndpointSlot {
    id: Option<TcpEndpointId>,
    pub(crate) reuse_address: bool,
    pub(crate) no_delay: bool,
    pub(crate) role: EndpointRole,
}

pub(crate) enum EndpointRole {
    Vacant,
    Idle,
    Bound(TcpLocalBinding),
    Listener(Listener),
    Connection(Connection),
    Reclaiming {
        remaining: usize,
        binding: Option<TcpLocalBinding>,
        tuple: Option<ConnectionTuple>,
    },
}

pub(crate) struct Listener {
    pub(crate) interface: InterfaceId,
    pub(crate) binding: TcpLocalBinding,
    pub(crate) backlog: usize,
    pub(crate) slots: Vec<ListenerSlot>,
}

pub(crate) struct ListenerSlot {
    pub(crate) generation: u64,
    pub(crate) handle: Option<SocketHandle>,
    pub(crate) claimed: bool,
    /// Engine-derived protocol reservation used only while this slot owns the
    /// engine. Stack progression refreshes it before another admission can run.
    pub(crate) tuple: Option<ConnectionTuple>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum ConnectionPhase {
    Connecting,
    Connected,
    Failed,
}

pub(crate) struct Connection {
    pub(crate) interface: InterfaceId,
    pub(crate) handle: SocketHandle,
    /// Namespace reservation may remain wildcard while the connected tuple
    /// has a concrete source selected by the control-plane owner.
    pub(crate) binding: TcpLocalBinding,
    pub(crate) local: TcpLocalBinding,
    pub(crate) peer: TcpPeer,
    /// Protocol phase drives connection operations and terminal classification.
    /// Consumable asynchronous delivery is owned separately by `pending_error`;
    /// callers cannot use this field as a second error source.
    pub(crate) phase: ConnectionPhase,
    /// Sticky protocol history used to distinguish connect failure from an
    /// established-stream reset after the engine has entered `Closed`.
    pub(crate) was_connected: bool,
    /// The sole consumable asynchronous error truth for every operation.
    pub(crate) pending_error: Option<TcpPendingError>,
    pub(crate) read_shutdown: bool,
    pub(crate) write_shutdown: bool,
    pub(crate) reservation: Option<OutstandingReceive>,
    /// Deferred lifecycle action while the exactly-once receive capability is
    /// outstanding. It is consumed when that reservation resolves.
    pub(crate) release_requested: Option<TcpReleaseReason>,
}

/// Exact connection reservation moved from `Connection` into reclaim state.
/// The tuple exists in only one role at a time and disappears with the engine.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct ConnectionTuple {
    pub(crate) local: TcpLocalBinding,
    pub(crate) peer: TcpPeer,
}

#[derive(Clone, Copy)]
pub(crate) struct OutstandingReceive {
    pub(crate) id: TcpReceiveReservationId,
    pub(crate) offered: usize,
    pub(crate) mode: TcpReceiveMode,
}

pub(crate) struct DeferredReclaim {
    pub(crate) interface: InterfaceId,
    pub(crate) handle: SocketHandle,
    /// Exact reservation moved from a listener slot while its engine emits
    /// final protocol work. It disappears with this deferred engine.
    pub(crate) tuple: Option<ConnectionTuple>,
    pub(crate) action: ReclaimAction,
}

pub(crate) enum ReclaimAction {
    RearmListener {
        listener: TcpEndpointId,
        slot: usize,
        generation: u64,
    },
    ReleaseEndpoint(TcpEndpointId),
}

/// Aggregate owner for every TCP fact and protocol engine in one Stack.
pub(crate) struct TcpEndpoints {
    policy: TcpPolicy,
    endpoints: Vec<EndpointSlot>,
    next_endpoint_id: u64,
    next_reservation_id: u64,
    engine_count: usize,
    deferred: Vec<DeferredReclaim>,
    /// Bounded, coalesced recheck hints. Endpoint facts remain authoritative
    /// in the role/engine owners and no readiness payload is stored here.
    pending_invalidations: Vec<TcpEndpointInvalidation>,
}

impl TcpEndpoints {
    pub(crate) fn new(policy: TcpPolicy) -> Self {
        assert!(
            policy.deferred_reclaim_capacity >= policy.engine_timer_capacity,
            "TCP deferred reclaim storage must cover every protocol engine"
        );
        let mut endpoints = Vec::with_capacity(policy.endpoint_capacity);
        for _ in 0..policy.endpoint_capacity {
            endpoints.push(EndpointSlot {
                id: None,
                reuse_address: false,
                no_delay: false,
                role: EndpointRole::Vacant,
            });
        }
        Self {
            policy,
            endpoints,
            next_endpoint_id: 1,
            next_reservation_id: 1,
            engine_count: 0,
            // Construction reserves every possible retiring engine. Normal
            // cleanup therefore never allocates and cannot be rejected after
            // a capability has crossed into the kernel.
            deferred: Vec::with_capacity(policy.deferred_reclaim_capacity),
            pending_invalidations: Vec::with_capacity(policy.endpoint_capacity),
        }
    }

    pub(crate) fn policy(&self) -> TcpPolicy {
        self.policy
    }

    pub(crate) fn reservation_interface(
        &self,
        reservation: TcpReceiveReservationId,
    ) -> Option<InterfaceId> {
        self.endpoints.iter().find_map(|slot| match &slot.role {
            EndpointRole::Connection(connection)
                if connection
                    .reservation
                    .is_some_and(|entry| entry.id == reservation) =>
            {
                Some(connection.interface)
            },
            _ => None,
        })
    }

    pub(crate) fn endpoint(&self, id: TcpEndpointId) -> Option<&EndpointSlot> {
        self.endpoints.iter().find(|slot| slot.id == Some(id))
    }

    pub(crate) fn endpoint_mut(&mut self, id: TcpEndpointId) -> Option<&mut EndpointSlot> {
        self.endpoints.iter_mut().find(|slot| slot.id == Some(id))
    }

    pub(crate) fn endpoint_index(&self, id: TcpEndpointId) -> Option<usize> {
        self.endpoints.iter().position(|slot| slot.id == Some(id))
    }

    pub(crate) fn connection(&self, id: TcpEndpointId) -> Option<&Connection> {
        let EndpointRole::Connection(connection) = &self.endpoint(id)?.role else {
            return None;
        };
        Some(connection)
    }

    pub(crate) fn connection_mut(&mut self, id: TcpEndpointId) -> Option<&mut Connection> {
        let EndpointRole::Connection(connection) = &mut self.endpoint_mut(id)?.role else {
            return None;
        };
        Some(connection)
    }

    pub(crate) fn listener(&self, id: TcpEndpointId) -> Option<&Listener> {
        let EndpointRole::Listener(listener) = &self.endpoint(id)?.role else {
            return None;
        };
        Some(listener)
    }

    pub(crate) fn listener_mut(&mut self, id: TcpEndpointId) -> Option<&mut Listener> {
        let EndpointRole::Listener(listener) = &mut self.endpoint_mut(id)?.role else {
            return None;
        };
        Some(listener)
    }

    pub(crate) fn ensure_engine_capacity(&self, additional: usize) -> bool {
        self.engine_count
            .checked_add(additional)
            .is_some_and(|count| count <= self.policy.engine_timer_capacity)
    }

    pub(crate) fn add_listener_engine(
        &mut self,
        sockets: &mut SocketSet<'static>,
        binding: TcpLocalBinding,
    ) -> SocketHandle {
        let mut socket = tcp_socket(self.policy);
        socket
            .listen(to_smoltcp_listen(binding))
            .expect("owner-validated TCP listener binding must be valid");
        let handle = sockets.add(socket);
        self.engine_count += 1;
        assert!(self.engine_count <= self.policy.engine_timer_capacity);
        handle
    }

    pub(crate) fn remove_engine(&mut self, sockets: &mut SocketSet<'static>, handle: SocketHandle) {
        match sockets.remove(handle) {
            socket::Socket::Tcp(_) => {},
            _ => unreachable!("TCP owner only records TCP engine handles"),
        }
        self.engine_count = self
            .engine_count
            .checked_sub(1)
            .expect("TCP engine count underflow");
    }

    pub(crate) fn queue_reclaim(&mut self, reclaim: DeferredReclaim) {
        self.deferred.push(reclaim);
        // Every engine contributes at most one entry, and construction has
        // already reserved at least this many slots. Assert after publishing
        // cleanup so a bookkeeping bug cannot turn final release into a leak.
        assert!(self.deferred.len() <= self.engine_count);
        assert!(self.deferred.len() <= self.policy.deferred_reclaim_capacity);
    }
}

pub(crate) fn tcp_socket(policy: TcpPolicy) -> tcp::Socket<'static> {
    tcp::Socket::new(
        tcp::SocketBuffer::new(vec![0; policy.rx_buffer_bytes]),
        tcp::SocketBuffer::new(vec![0; policy.tx_buffer_bytes]),
    )
}

pub(crate) fn completed_child_state(state: tcp::State) -> bool {
    matches!(state, tcp::State::Established | tcp::State::CloseWait)
}

pub(crate) fn engine_connection_tuple(socket: &tcp::Socket<'_>) -> Option<ConnectionTuple> {
    let local = socket.local_endpoint()?;
    let remote = socket.remote_endpoint()?;
    let smoltcp::wire::IpAddress::Ipv4(local_address) = local.addr;
    let smoltcp::wire::IpAddress::Ipv4(remote_address) = remote.addr;
    Some(ConnectionTuple {
        local: TcpLocalBinding::from_owner_commit(
            anemone_net_api::Ipv4Address::new(local_address.octets()),
            local.port,
        ),
        peer: TcpPeer::new(
            anemone_net_api::Ipv4Address::new(remote_address.octets()),
            remote.port,
        ),
    })
}

pub(crate) fn to_smoltcp_listen(binding: TcpLocalBinding) -> IpListenEndpoint {
    let address = (!binding.address().is_unspecified())
        .then(|| smoltcp::wire::IpAddress::Ipv4(binding.address().octets().into()));
    IpListenEndpoint {
        addr: address,
        port: binding.port(),
    }
}

#[cfg(test)]
mod tests {
    use alloc::vec::Vec;

    use anemone_net_api::{
        Instant, Ipv4Address, Ipv4Cidr, Ipv4EgressSelection,
        icmp_raw::IcmpRawNamespacePolicy,
        tcp::{
            TcpBindError, TcpBindRequest, TcpConnectFact, TcpConnectResult, TcpEndpointFacts,
            TcpListenBacklog, TcpListenError, TcpPeer, TcpPendingError, TcpQueryError,
            TcpReceiveMode, TcpReceiveResolveError, TcpReleaseReason, TcpShutdownDirection,
            TcpShutdownOutcome, TcpStreamReceiveError, TcpStreamReceiveOutcome, TcpStreamSendError,
        },
        udp::UdpNamespacePolicy,
    };
    use smoltcp::{
        iface::PollResult,
        phy::{FaultInjector, Loopback, Medium},
        time::{Duration as SmoltcpDuration, Instant as SmoltcpInstant},
    };

    use crate::{
        PumpBudget,
        stack::{Stack, StackPolicy},
    };

    use super::*;

    const LOCAL: Ipv4Address = Ipv4Address::new([127, 0, 0, 1]);
    const LISTEN_PORT: u16 = 2345;
    const POLICY: TcpPolicy = TcpPolicy::new(64, 64, 10, 32, 32, 64, 60_000, 60_000, 40000, 40063);

    fn test_stack(policy: TcpPolicy) -> (Stack, InterfaceId) {
        let mut stack = Stack::with_policy(StackPolicy::new(
            UdpNamespacePolicy::new(4, 30000, 30003),
            IcmpRawNamespacePolicy::new(4),
            policy,
        ));
        let interface = stack
            .add_local_ipv4(Ipv4Cidr::new(LOCAL, 8).unwrap(), 128, 1500, Instant::ZERO)
            .unwrap();
        (stack, interface)
    }

    fn drive(stack: &mut Stack, interface: InterfaceId, first_tick: i64) {
        for tick in first_tick..first_tick + 128 {
            let _ = stack
                .pump_local(
                    interface,
                    Instant::from_micros(tick * 1_000),
                    PumpBudget::new(32, 32),
                )
                .unwrap();
        }
    }

    fn connected_pair(
        stack: &mut Stack,
        interface: InterfaceId,
        port: u16,
        first_tick: i64,
    ) -> (TcpEndpointId, TcpEndpointId, TcpEndpointId) {
        let listener = stack.create_tcp_endpoint().unwrap();
        stack
            .bind_tcp_endpoint(listener, TcpBindRequest::new(LOCAL, port))
            .unwrap();
        stack
            .listen_tcp_endpoint_with_backlog(
                listener,
                interface,
                Ipv4Address::UNSPECIFIED,
                TcpListenBacklog::new(1),
            )
            .unwrap();
        let client = stack.create_tcp_endpoint().unwrap();
        let _ = stack
            .start_tcp_connect(
                client,
                Ipv4EgressSelection::new(interface, LOCAL),
                TcpPeer::new(LOCAL, port),
            )
            .unwrap();
        drive(stack, interface, first_tick);
        let child = stack
            .claim_tcp_pending_child(listener)
            .unwrap()
            .expect("connected pair must publish one completed child");
        let accepted = stack.take_tcp_child(child).unwrap();
        (listener, client, accepted)
    }

    fn tcp_invalidations(stack: &mut Stack) -> Vec<anemone_net_api::tcp::TcpEndpointInvalidation> {
        stack.take_invalidations().into_parts().2
    }

    #[test]
    fn endpoint_facts_and_invalidations_cover_listener_stream_shutdown_and_retire() {
        let (mut stack, interface) = test_stack(POLICY);
        let listener = stack.create_tcp_endpoint().unwrap();
        stack
            .bind_tcp_endpoint(listener, TcpBindRequest::new(LOCAL, LISTEN_PORT))
            .unwrap();
        stack
            .listen_tcp_endpoint_with_backlog(
                listener,
                interface,
                Ipv4Address::UNSPECIFIED,
                TcpListenBacklog::new(1),
            )
            .unwrap();
        assert!(
            tcp_invalidations(&mut stack)
                .iter()
                .any(|entry| entry.endpoint() == listener)
        );
        assert_eq!(
            stack.tcp_endpoint_facts(listener),
            Ok(TcpEndpointFacts::Listener {
                has_pending_child: false
            })
        );
        assert!(tcp_invalidations(&mut stack).is_empty());

        let client = stack.create_tcp_endpoint().unwrap();
        let _ = stack
            .start_tcp_connect(
                client,
                Ipv4EgressSelection::new(interface, LOCAL),
                TcpPeer::new(LOCAL, LISTEN_PORT),
            )
            .unwrap();
        let TcpEndpointFacts::Connection(connecting) = stack.tcp_endpoint_facts(client).unwrap()
        else {
            panic!("active open did not publish connection facts");
        };
        assert_eq!(connecting.connect(), TcpConnectFact::Connecting);
        assert!(
            tcp_invalidations(&mut stack)
                .iter()
                .any(|entry| entry.endpoint() == client)
        );

        drive(&mut stack, interface, 0);
        let invalidations = tcp_invalidations(&mut stack);
        assert_eq!(
            invalidations
                .iter()
                .filter(|entry| entry.endpoint() == listener)
                .count(),
            1
        );
        assert_eq!(
            invalidations
                .iter()
                .filter(|entry| entry.endpoint() == client)
                .count(),
            1
        );
        assert_eq!(
            stack.tcp_endpoint_facts(listener),
            Ok(TcpEndpointFacts::Listener {
                has_pending_child: true
            })
        );
        let TcpEndpointFacts::Connection(connected) = stack.tcp_endpoint_facts(client).unwrap()
        else {
            panic!("completed active open lost connection facts");
        };
        assert_eq!(connected.connect(), TcpConnectFact::Connected);
        assert!(connected.send_capacity() > 0);

        let child = stack
            .claim_tcp_pending_child(listener)
            .unwrap()
            .expect("completed child must be claimable");
        assert_eq!(
            stack.tcp_endpoint_facts(listener),
            Ok(TcpEndpointFacts::Listener {
                has_pending_child: false
            })
        );
        let accepted = stack.take_tcp_child(child).unwrap();
        assert!(
            tcp_invalidations(&mut stack)
                .iter()
                .any(|entry| entry.endpoint() == accepted)
        );

        assert_eq!(stack.send_tcp_stream(client, &[0x5a; 32]).unwrap().0, 32);
        let TcpEndpointFacts::Connection(full) = stack.tcp_endpoint_facts(client).unwrap() else {
            panic!("sender lost connection facts");
        };
        assert_eq!(full.send_capacity(), 0);
        drive(&mut stack, interface, 128);
        let TcpEndpointFacts::Connection(receiving) = stack.tcp_endpoint_facts(accepted).unwrap()
        else {
            panic!("receiver lost connection facts");
        };
        assert_eq!(receiving.received_bytes(), 32);

        assert_eq!(
            stack
                .shutdown_tcp_endpoint(accepted, TcpShutdownDirection::Read)
                .unwrap()
                .0,
            TcpShutdownOutcome::Changed
        );
        let TcpEndpointFacts::Connection(shut_read) = stack.tcp_endpoint_facts(accepted).unwrap()
        else {
            panic!("shutdown receiver lost connection facts");
        };
        assert!(shut_read.is_local_read_shutdown());
        assert!(
            tcp_invalidations(&mut stack)
                .iter()
                .any(|entry| entry.endpoint() == accepted)
        );

        stack
            .release_tcp_endpoint(accepted, TcpReleaseReason::AcceptedChildRollback)
            .unwrap();
        assert_eq!(
            stack.tcp_endpoint_facts(accepted),
            Err(TcpQueryError::UnknownEndpoint)
        );
        assert!(
            tcp_invalidations(&mut stack)
                .iter()
                .any(|entry| entry.endpoint() == accepted)
        );
    }

    #[test]
    fn active_passive_namespace_stream_and_generation_share_one_owner() {
        let (mut stack, interface) = test_stack(POLICY);
        let listener = stack.create_tcp_endpoint().unwrap();
        let binding = stack
            .bind_tcp_endpoint(listener, TcpBindRequest::new(LOCAL, LISTEN_PORT))
            .unwrap();
        assert_eq!(binding.port(), LISTEN_PORT);
        let _ = stack
            .listen_tcp_endpoint(listener, interface, Ipv4Address::UNSPECIFIED)
            .unwrap();

        let conflict = stack.create_tcp_endpoint().unwrap();
        assert!(matches!(
            stack.bind_tcp_endpoint(conflict, TcpBindRequest::new(LOCAL, LISTEN_PORT)),
            Err(anemone_net_api::tcp::TcpBindError::PortInUse)
        ));

        let mut clients = Vec::new();
        for _ in 0..10 {
            let client = stack.create_tcp_endpoint().unwrap();
            let _ = stack
                .start_tcp_connect(
                    client,
                    Ipv4EgressSelection::new(interface, LOCAL),
                    TcpPeer::new(LOCAL, LISTEN_PORT),
                )
                .unwrap();
            assert!(matches!(
                stack.tcp_connect_result(client).unwrap(),
                TcpConnectResult::Connecting { .. }
            ));
            clients.push(client);
        }
        drive(&mut stack, interface, 0);
        for client in &clients {
            assert!(matches!(
                stack.tcp_connect_result(*client).unwrap(),
                TcpConnectResult::Connected { .. }
            ));
        }

        let overflow = stack.create_tcp_endpoint().unwrap();
        let _ = stack
            .start_tcp_connect(
                overflow,
                Ipv4EgressSelection::new(interface, LOCAL),
                TcpPeer::new(LOCAL, LISTEN_PORT),
            )
            .unwrap();
        drive(&mut stack, interface, 128);
        assert!(matches!(
            stack.tcp_connect_result(overflow).unwrap(),
            TcpConnectResult::Failed(TcpPendingError::ConnectionRefused)
        ));
        stack
            .release_tcp_endpoint(overflow, TcpReleaseReason::CreationRollback)
            .unwrap();

        let child = stack
            .claim_tcp_pending_child(listener)
            .unwrap()
            .expect("one completed child must be claimable");
        let accepted = stack.take_tcp_child(child).unwrap();
        assert!(matches!(
            stack.tcp_connect_result(accepted).unwrap(),
            TcpConnectResult::Connected { .. }
        ));
        assert!(matches!(
            stack.cancel_tcp_child(child),
            Err(anemone_net_api::tcp::TcpChildError::StaleChild)
        ));

        let accepted_prefix = stack.send_tcp_stream(clients[0], &[0x5a; 64]).unwrap().0;
        assert_eq!(accepted_prefix, POLICY.tx_buffer_bytes);
        drive(&mut stack, interface, 256);

        let TcpStreamReceiveOutcome::Data(reservation) = stack
            .receive_tcp_stream(accepted, 64, TcpReceiveMode::Consume)
            .unwrap()
        else {
            panic!("buffered bytes must precede end of stream");
        };
        assert_eq!(reservation.bytes(), &[0x5a; 32]);
        let (reservation_id, _) = reservation.into_owner_parts();
        stack.resolve_tcp_receive(reservation_id, 7).unwrap();

        let TcpStreamReceiveOutcome::Data(rolled_back) = stack
            .receive_tcp_stream(accepted, 64, TcpReceiveMode::Consume)
            .unwrap()
        else {
            panic!("buffered bytes must precede end of stream");
        };
        assert_eq!(rolled_back.bytes(), &[0x5a; 25]);
        let (rolled_back_id, _) = rolled_back.into_owner_parts();
        stack.resolve_tcp_receive(rolled_back_id, 0).unwrap();
        let TcpStreamReceiveOutcome::Data(repeated) = stack
            .receive_tcp_stream(accepted, 64, TcpReceiveMode::Consume)
            .unwrap()
        else {
            panic!("rolled-back bytes must remain available");
        };
        assert_eq!(repeated.bytes(), &[0x5a; 25]);
        let (repeated_id, _) = repeated.into_owner_parts();
        assert_eq!(
            stack.resolve_tcp_receive(repeated_id, 26),
            Err(TcpReceiveResolveError::InvalidPrefix)
        );
        stack.resolve_tcp_receive(repeated_id, 25).unwrap();

        let _ = stack.send_tcp_stream(accepted, b"final").unwrap().1;
        drive(&mut stack, interface, 384);
        let TcpStreamReceiveOutcome::Data(outstanding) = stack
            .receive_tcp_stream(clients[0], 5, TcpReceiveMode::Consume)
            .unwrap()
        else {
            panic!("buffered bytes must precede end of stream");
        };
        assert_eq!(outstanding.bytes(), b"final");
        let (outstanding_id, _) = outstanding.into_owner_parts();
        let retiring_handle = stack.protocols.tcp.connection(clients[0]).unwrap().handle;
        stack
            .release_tcp_endpoint(clients[0], TcpReleaseReason::FinalRelease)
            .unwrap();
        assert!(matches!(
            stack.tcp_connect_result(clients[0]).unwrap(),
            TcpConnectResult::Connected { .. }
        ));
        assert_eq!(
            stack
                .protocols
                .tcp
                .connection(clients[0])
                .unwrap()
                .release_requested,
            Some(TcpReleaseReason::FinalRelease)
        );
        assert!(matches!(
            stack
                .local
                .as_ref()
                .unwrap()
                .sockets
                .get::<tcp::Socket>(retiring_handle)
                .state(),
            tcp::State::FinWait1 | tcp::State::LastAck
        ));
        stack.resolve_tcp_receive(outstanding_id, 3).unwrap();
        drive(&mut stack, interface, 512);
        assert_eq!(
            stack.tcp_connect_result(clients[0]),
            Err(TcpQueryError::UnknownEndpoint)
        );

        let cancelled = stack
            .claim_tcp_pending_child(listener)
            .unwrap()
            .expect("another completed child must remain");
        let _ = stack.cancel_tcp_child(cancelled).unwrap();
        drive(&mut stack, interface, 640);
        assert!(stack.claim_tcp_pending_child(listener).unwrap().is_some());
    }

    #[test]
    fn every_engine_can_enter_preallocated_reclaim_without_cleanup_failure() {
        const CAPACITY: usize = 12;
        let policy = TcpPolicy::new(
            16, CAPACITY, 10, 16, 16, CAPACITY, 60_000, 60_000, 41000, 41015,
        );
        let (mut stack, interface) = test_stack(policy);
        let mut endpoints = Vec::new();
        for port in 42000..42000 + CAPACITY as u16 {
            let endpoint = stack.create_tcp_endpoint().unwrap();
            let _ = stack
                .start_tcp_connect(
                    endpoint,
                    Ipv4EgressSelection::new(interface, LOCAL),
                    TcpPeer::new(LOCAL, port),
                )
                .unwrap();
            endpoints.push(endpoint);
        }
        assert_eq!(stack.protocols.tcp.engine_count, CAPACITY);
        for endpoint in endpoints {
            assert!(
                stack
                    .release_tcp_endpoint(endpoint, TcpReleaseReason::CreationRollback)
                    .unwrap()
                    .is_some()
            );
        }
        assert_eq!(stack.protocols.tcp.deferred.len(), CAPACITY);
        let while_reclaiming = stack.create_tcp_endpoint().unwrap();
        assert!(matches!(
            stack.bind_tcp_endpoint(
                while_reclaiming,
                TcpBindRequest::new(LOCAL, policy.ephemeral_port_first),
            ),
            Err(anemone_net_api::tcp::TcpBindError::PortInUse)
        ));
        drive(&mut stack, interface, 0);
        assert_eq!(stack.protocols.tcp.engine_count, 0);
        assert!(stack.protocols.tcp.deferred.is_empty());
        assert!(
            stack
                .bind_tcp_endpoint(
                    while_reclaiming,
                    TcpBindRequest::new(LOCAL, policy.ephemeral_port_first),
                )
                .is_ok()
        );
    }

    #[test]
    fn endpoint_engine_and_ephemeral_capacity_fail_typed_and_recover() {
        let policy = TcpPolicy::new(2, 1, 1, 16, 16, 1, 60_000, 60_000, 43000, 43000);
        let (mut stack, interface) = test_stack(policy);
        let first = stack.create_tcp_endpoint().unwrap();
        let second = stack.create_tcp_endpoint().unwrap();
        assert_eq!(
            stack.create_tcp_endpoint(),
            Err(anemone_net_api::tcp::TcpCreateError::EndpointCapacity)
        );

        stack
            .bind_tcp_endpoint(first, TcpBindRequest::new(LOCAL, 0))
            .unwrap();
        assert_eq!(
            stack.bind_tcp_endpoint(second, TcpBindRequest::new(LOCAL, 0)),
            Err(anemone_net_api::tcp::TcpBindError::EphemeralPortsExhausted)
        );
        stack
            .release_tcp_endpoint(first, TcpReleaseReason::CreationRollback)
            .unwrap();
        stack
            .bind_tcp_endpoint(second, TcpBindRequest::new(LOCAL, 0))
            .unwrap();
        let recovered = stack.create_tcp_endpoint().unwrap();
        stack
            .bind_tcp_endpoint(recovered, TcpBindRequest::new(LOCAL, 43001))
            .unwrap();

        let _ = stack
            .start_tcp_connect(
                second,
                Ipv4EgressSelection::new(interface, LOCAL),
                TcpPeer::new(LOCAL, 44000),
            )
            .unwrap();
        assert_eq!(
            stack.start_tcp_connect(
                recovered,
                Ipv4EgressSelection::new(interface, LOCAL),
                TcpPeer::new(LOCAL, 44001),
            ),
            Err(anemone_net_api::tcp::TcpConnectError::EngineCapacity)
        );
        stack
            .release_tcp_endpoint(second, TcpReleaseReason::CreationRollback)
            .unwrap();
        drive(&mut stack, interface, 0);
        assert!(
            stack
                .start_tcp_connect(
                    recovered,
                    Ipv4EgressSelection::new(interface, LOCAL),
                    TcpPeer::new(LOCAL, 44001),
                )
                .is_ok()
        );
    }

    #[test]
    fn listener_retirement_subsumes_pending_child_rearm_before_releasing_port() {
        let (mut stack, interface) = test_stack(POLICY);
        let listener = stack.create_tcp_endpoint().unwrap();
        stack
            .bind_tcp_endpoint(listener, TcpBindRequest::new(LOCAL, LISTEN_PORT))
            .unwrap();
        stack
            .listen_tcp_endpoint(listener, interface, Ipv4Address::UNSPECIFIED)
            .unwrap();
        let client = stack.create_tcp_endpoint().unwrap();
        let _ = stack
            .start_tcp_connect(
                client,
                Ipv4EgressSelection::new(interface, LOCAL),
                TcpPeer::new(LOCAL, LISTEN_PORT),
            )
            .unwrap();
        drive(&mut stack, interface, 0);

        let child = stack
            .claim_tcp_pending_child(listener)
            .unwrap()
            .expect("completed child must be claimable");
        assert!(stack.cancel_tcp_child(child).unwrap().is_some());
        assert_eq!(stack.protocols.tcp.deferred.len(), 1);
        assert!(
            stack
                .release_tcp_endpoint(listener, TcpReleaseReason::ListenerWithdrawal)
                .unwrap()
                .is_none()
        );

        let contender = stack.create_tcp_endpoint().unwrap();
        assert!(matches!(
            stack.bind_tcp_endpoint(contender, TcpBindRequest::new(LOCAL, LISTEN_PORT)),
            Err(anemone_net_api::tcp::TcpBindError::PortInUse)
        ));
        drive(&mut stack, interface, 128);
        assert!(
            stack
                .bind_tcp_endpoint(contender, TcpBindRequest::new(LOCAL, LISTEN_PORT))
                .is_ok()
        );
    }

    #[test]
    fn claimed_child_cancel_does_not_depend_on_completed_state_remaining_stable() {
        let (mut stack, interface) = test_stack(POLICY);
        let listener = stack.create_tcp_endpoint().unwrap();
        stack
            .bind_tcp_endpoint(listener, TcpBindRequest::new(LOCAL, LISTEN_PORT))
            .unwrap();
        stack
            .listen_tcp_endpoint(listener, interface, Ipv4Address::UNSPECIFIED)
            .unwrap();
        let client = stack.create_tcp_endpoint().unwrap();
        let _ = stack
            .start_tcp_connect(
                client,
                Ipv4EgressSelection::new(interface, LOCAL),
                TcpPeer::new(LOCAL, LISTEN_PORT),
            )
            .unwrap();
        drive(&mut stack, interface, 0);

        let child = stack
            .claim_tcp_pending_child(listener)
            .unwrap()
            .expect("completed child must be claimable");
        let (_, slot, _) = child.owner_parts();
        let handle = stack.protocols.tcp.listener(listener).unwrap().slots[slot]
            .handle
            .unwrap();
        stack
            .local
            .as_mut()
            .unwrap()
            .sockets
            .get_mut::<tcp::Socket>(handle)
            .abort();
        let _ = stack.cancel_tcp_child(child).unwrap();
        drive(&mut stack, interface, 128);
        assert!(stack.protocols.tcp.deferred.is_empty());
        let slot = &stack.protocols.tcp.listener(listener).unwrap().slots[slot];
        assert!(slot.handle.is_some());
        assert!(!slot.claimed);
    }

    #[test]
    fn connecting_timeout_is_preserved_as_a_typed_owner_observation() {
        let policy = TcpPolicy::new(64, 64, 10, 32, 32, 64, 1, 60_000, 40000, 40063);
        let (mut stack, interface) = test_stack(policy);
        let endpoint = stack.create_tcp_endpoint().unwrap();
        let _ = stack
            .start_tcp_connect(
                endpoint,
                Ipv4EgressSelection::new(interface, LOCAL),
                TcpPeer::new(Ipv4Address::new([127, 0, 0, 2]), 80),
            )
            .unwrap();
        let handle = stack.protocols.tcp.connection(endpoint).unwrap().handle;
        assert_eq!(
            stack
                .local
                .as_ref()
                .unwrap()
                .sockets
                .get::<tcp::Socket>(handle)
                .timeout(),
            Some(SmoltcpDuration::from_millis(1))
        );

        let mut device = FaultInjector::new(Loopback::new(Medium::Ip), 1);
        device.set_drop_chance(100);
        let local = stack.local.as_mut().unwrap();
        local.interface.poll(
            SmoltcpInstant::from_millis(0),
            &mut device,
            &mut local.sockets,
        );
        local.interface.poll(
            SmoltcpInstant::from_millis(2),
            &mut device,
            &mut local.sockets,
        );
        assert!(
            stack
                .observe_tcp_stream(endpoint)
                .unwrap()
                .has_pending_error()
        );
        let TcpEndpointFacts::Connection(timed_out) = stack.tcp_endpoint_facts(endpoint).unwrap()
        else {
            panic!("timed-out active open lost connection facts");
        };
        assert_eq!(timed_out.connect(), TcpConnectFact::Failed);
        assert!(timed_out.has_pending_error());
        assert!(timed_out.is_terminal());
        assert!(
            tcp_invalidations(&mut stack)
                .iter()
                .any(|entry| entry.endpoint() == endpoint)
        );
        assert_eq!(
            stack.tcp_connect_result(endpoint).unwrap(),
            TcpConnectResult::Failed(TcpPendingError::TimedOut)
        );
        assert_eq!(stack.consume_tcp_pending_error(endpoint).unwrap(), None);
    }

    #[test]
    fn normalized_backlog_is_the_only_listener_admission_limit() {
        let (mut stack, interface) = test_stack(POLICY);
        let listener = stack.create_tcp_endpoint().unwrap();
        stack
            .bind_tcp_endpoint(listener, TcpBindRequest::new(LOCAL, LISTEN_PORT))
            .unwrap();

        for expected in [0, 1, 10, 1, 10] {
            stack
                .listen_tcp_endpoint_with_backlog(
                    listener,
                    interface,
                    Ipv4Address::UNSPECIFIED,
                    TcpListenBacklog::new(expected),
                )
                .unwrap();
            let owner = stack.protocols.tcp.listener(listener).unwrap();
            assert_eq!(owner.backlog, expected);
            assert_eq!(
                owner
                    .slots
                    .iter()
                    .filter(|slot| slot.handle.is_some())
                    .count(),
                expected
            );
        }
        assert_eq!(
            stack.listen_tcp_endpoint_with_backlog(
                listener,
                interface,
                Ipv4Address::UNSPECIFIED,
                TcpListenBacklog::new(11),
            ),
            Err(TcpListenError::EngineCapacity)
        );
        assert_eq!(stack.protocols.tcp.listener(listener).unwrap().backlog, 10);
    }

    #[test]
    fn reuse_requires_both_owners_and_never_allows_duplicate_listener() {
        let (mut stack, interface) = test_stack(POLICY);
        let wildcard = stack.create_tcp_endpoint().unwrap();
        stack.set_tcp_reuse_address(wildcard, true).unwrap();
        stack
            .bind_tcp_endpoint(
                wildcard,
                TcpBindRequest::new(Ipv4Address::UNSPECIFIED, LISTEN_PORT),
            )
            .unwrap();

        let specific = stack.create_tcp_endpoint().unwrap();
        assert_eq!(
            stack.bind_tcp_endpoint(specific, TcpBindRequest::new(LOCAL, LISTEN_PORT)),
            Err(TcpBindError::PortInUse)
        );
        stack.set_tcp_reuse_address(specific, true).unwrap();
        stack
            .bind_tcp_endpoint(specific, TcpBindRequest::new(LOCAL, LISTEN_PORT))
            .unwrap();
        stack
            .listen_tcp_endpoint_with_backlog(
                wildcard,
                interface,
                Ipv4Address::UNSPECIFIED,
                TcpListenBacklog::new(1),
            )
            .unwrap();
        assert_eq!(
            stack.listen_tcp_endpoint_with_backlog(
                specific,
                interface,
                Ipv4Address::UNSPECIFIED,
                TcpListenBacklog::new(1),
            ),
            Err(TcpListenError::PortInUse)
        );
    }

    #[test]
    fn implicit_connect_skips_an_exact_reused_tuple() {
        let policy = TcpPolicy::new(8, 8, 1, 32, 32, 8, 60_000, 60_000, 40000, 40001);
        let (mut stack, interface) = test_stack(policy);
        let listener = stack.create_tcp_endpoint().unwrap();
        stack
            .bind_tcp_endpoint(listener, TcpBindRequest::new(LOCAL, 25020))
            .unwrap();
        stack
            .listen_tcp_endpoint_with_backlog(
                listener,
                interface,
                Ipv4Address::UNSPECIFIED,
                TcpListenBacklog::new(1),
            )
            .unwrap();
        let peer = TcpPeer::new(LOCAL, 25020);

        let first = stack.create_tcp_endpoint().unwrap();
        stack.set_tcp_reuse_address(first, true).unwrap();
        let _ = stack
            .start_tcp_connect(first, Ipv4EgressSelection::new(interface, LOCAL), peer)
            .unwrap();
        drive(&mut stack, interface, 0);
        let first_connection = stack.protocols.tcp.connection(first).unwrap();
        assert_eq!(first_connection.binding.address(), Ipv4Address::UNSPECIFIED);
        assert_eq!(first_connection.local.address(), LOCAL);
        assert_eq!(
            stack
                .tcp_endpoint_binding(first)
                .unwrap()
                .unwrap()
                .address(),
            LOCAL
        );
        assert_eq!(
            stack.tcp_endpoint_binding(first).unwrap().unwrap().port(),
            40000
        );

        let second = stack.create_tcp_endpoint().unwrap();
        stack.set_tcp_reuse_address(second, true).unwrap();
        let _ = stack
            .start_tcp_connect(second, Ipv4EgressSelection::new(interface, LOCAL), peer)
            .unwrap();
        assert_eq!(
            stack.tcp_endpoint_binding(second).unwrap().unwrap().port(),
            40001
        );
    }

    #[test]
    fn listener_and_deferred_child_reserve_their_complete_tuple() {
        let (mut stack, interface) = test_stack(POLICY);
        let listener = stack.create_tcp_endpoint().unwrap();
        let contender = stack.create_tcp_endpoint().unwrap();
        for endpoint in [listener, contender] {
            stack.set_tcp_reuse_address(endpoint, true).unwrap();
            stack
                .bind_tcp_endpoint(endpoint, TcpBindRequest::new(LOCAL, 25012))
                .unwrap();
        }
        stack
            .listen_tcp_endpoint_with_backlog(
                listener,
                interface,
                Ipv4Address::UNSPECIFIED,
                TcpListenBacklog::new(1),
            )
            .unwrap();

        let inbound = stack.create_tcp_endpoint().unwrap();
        stack
            .bind_tcp_endpoint(inbound, TcpBindRequest::new(LOCAL, 25013))
            .unwrap();
        let _ = stack
            .start_tcp_connect(
                inbound,
                Ipv4EgressSelection::new(interface, LOCAL),
                TcpPeer::new(LOCAL, 25012),
            )
            .unwrap();
        drive(&mut stack, interface, 0);

        let exact_peer = TcpPeer::new(LOCAL, 25013);
        assert_eq!(
            stack.start_tcp_connect(
                contender,
                Ipv4EgressSelection::new(interface, LOCAL),
                exact_peer,
            ),
            Err(anemone_net_api::tcp::TcpConnectError::PortInUse)
        );

        stack
            .release_tcp_endpoint(listener, TcpReleaseReason::ListenerWithdrawal)
            .unwrap();
        assert!(stack.protocols.tcp.deferred.iter().any(|entry| {
            entry
                .tuple
                .is_some_and(|tuple| tuple.local.port() == 25012 && tuple.peer == exact_peer)
        }));
        assert_eq!(
            stack.start_tcp_connect(
                contender,
                Ipv4EgressSelection::new(interface, LOCAL),
                exact_peer,
            ),
            Err(anemone_net_api::tcp::TcpConnectError::PortInUse)
        );
    }

    #[test]
    fn pending_error_has_one_consumer_across_query_and_stream_operation() {
        let (mut stack, interface) = test_stack(POLICY);
        let (_, client, accepted) = connected_pair(&mut stack, interface, 25001, 0);
        stack
            .release_tcp_endpoint(accepted, TcpReleaseReason::AcceptedChildRollback)
            .unwrap();
        drive(&mut stack, interface, 128);
        assert_eq!(
            stack.send_tcp_stream(client, b"x"),
            Err(TcpStreamSendError::ConnectionReset)
        );
        assert_eq!(
            stack.shutdown_tcp_endpoint(client, TcpShutdownDirection::Write),
            Err(anemone_net_api::tcp::TcpShutdownError::NotConnected)
        );
        assert_eq!(stack.consume_tcp_pending_error(client).unwrap(), None);

        let (_, client, accepted) = connected_pair(&mut stack, interface, 25002, 256);
        stack
            .release_tcp_endpoint(accepted, TcpReleaseReason::AcceptedChildRollback)
            .unwrap();
        drive(&mut stack, interface, 384);
        assert_eq!(
            stack.consume_tcp_pending_error(client).unwrap(),
            Some(TcpPendingError::ConnectionReset)
        );
        assert_eq!(stack.consume_tcp_pending_error(client).unwrap(), None);
        assert_eq!(
            stack.send_tcp_stream(client, b"x"),
            Err(TcpStreamSendError::BrokenStream)
        );

        let (_, client, accepted) = connected_pair(&mut stack, interface, 25009, 512);
        stack
            .release_tcp_endpoint(accepted, TcpReleaseReason::AcceptedChildRollback)
            .unwrap();
        drive(&mut stack, interface, 640);
        assert_eq!(
            stack.tcp_connect_result(client).unwrap(),
            TcpConnectResult::Failed(TcpPendingError::ConnectionReset)
        );
        assert_eq!(stack.consume_tcp_pending_error(client).unwrap(), None);
        assert!(matches!(
            stack.tcp_connect_result(client).unwrap(),
            TcpConnectResult::Bound(_)
        ));

        let refused = stack.create_tcp_endpoint().unwrap();
        let _ = stack
            .start_tcp_connect(
                refused,
                Ipv4EgressSelection::new(interface, LOCAL),
                TcpPeer::new(LOCAL, 25999),
            )
            .unwrap();
        drive(&mut stack, interface, 768);
        assert_eq!(
            stack.consume_tcp_pending_error(refused).unwrap(),
            Some(TcpPendingError::ConnectionRefused)
        );
        assert_eq!(
            stack.tcp_connect_result(refused).unwrap(),
            TcpConnectResult::Terminal
        );
        let TcpConnectResult::Bound(retained) = stack.tcp_connect_result(refused).unwrap() else {
            panic!("terminal connect consumption must rearm the retained binding");
        };
        assert_eq!(retained.address(), Ipv4Address::UNSPECIFIED);

        let alternate = Ipv4Address::new([127, 0, 0, 2]);
        let _ = stack
            .start_tcp_connect(
                refused,
                Ipv4EgressSelection::new(interface, alternate),
                TcpPeer::new(LOCAL, 25998),
            )
            .unwrap();
        let retried = stack.protocols.tcp.connection(refused).unwrap();
        assert_eq!(retried.binding, retained);
        assert_eq!(retried.local.address(), alternate);
    }

    #[test]
    fn engine_phase_distinguishes_a_same_round_established_reset() {
        let (mut stack, interface) = test_stack(POLICY);
        let listener = stack.create_tcp_endpoint().unwrap();
        stack
            .bind_tcp_endpoint(listener, TcpBindRequest::new(LOCAL, 25021))
            .unwrap();
        stack
            .listen_tcp_endpoint_with_backlog(
                listener,
                interface,
                Ipv4Address::UNSPECIFIED,
                TcpListenBacklog::new(1),
            )
            .unwrap();
        let client = stack.create_tcp_endpoint().unwrap();
        let _ = stack
            .start_tcp_connect(
                client,
                Ipv4EgressSelection::new(interface, LOCAL),
                TcpPeer::new(LOCAL, 25021),
            )
            .unwrap();
        let listener_handle = stack.protocols.tcp.listener(listener).unwrap().slots[0]
            .handle
            .unwrap();
        let client_handle = stack.protocols.tcp.connection(client).unwrap().handle;

        let mut device = Loopback::new(Medium::Ip);
        let local = stack.local.as_mut().unwrap();
        for tick in 0..8 {
            local.interface.poll(
                SmoltcpInstant::from_millis(tick),
                &mut device,
                &mut local.sockets,
            );
        }
        assert_eq!(
            local.sockets.get::<tcp::Socket>(client_handle).state(),
            tcp::State::Established
        );
        local
            .sockets
            .get_mut::<tcp::Socket>(listener_handle)
            .abort();
        for tick in 8..16 {
            local.interface.poll(
                SmoltcpInstant::from_millis(tick),
                &mut device,
                &mut local.sockets,
            );
        }
        assert_eq!(
            local.sockets.get::<tcp::Socket>(client_handle).state(),
            tcp::State::Closed
        );
        assert!(
            !stack
                .protocols
                .tcp
                .connection(client)
                .unwrap()
                .was_connected
        );
        assert_eq!(
            stack.tcp_connect_result(client).unwrap(),
            TcpConnectResult::Failed(TcpPendingError::ConnectionReset)
        );
        assert_eq!(
            stack.shutdown_tcp_endpoint(client, TcpShutdownDirection::Write),
            Err(anemone_net_api::tcp::TcpShutdownError::NotConnected)
        );
    }

    #[test]
    fn peek_fin_reset_and_shutdown_share_one_receive_truth() {
        let (mut stack, interface) = test_stack(POLICY);
        let (_, client, accepted) = connected_pair(&mut stack, interface, 25003, 0);
        stack.send_tcp_stream(client, b"hello").unwrap();
        stack
            .shutdown_tcp_endpoint(client, TcpShutdownDirection::Write)
            .unwrap();
        drive(&mut stack, interface, 128);

        let TcpEndpointFacts::Connection(fin) = stack.tcp_endpoint_facts(accepted).unwrap() else {
            panic!("FIN receiver lost connection facts");
        };
        assert_eq!(fin.received_bytes(), 5);
        assert!(fin.is_peer_receive_closed());
        assert!(!fin.is_terminal());

        let peek = match stack
            .receive_tcp_stream(accepted, 5, TcpReceiveMode::Peek)
            .unwrap()
        {
            TcpStreamReceiveOutcome::Data(reservation) => reservation,
            TcpStreamReceiveOutcome::EndOfStream => panic!("buffered bytes precede FIN"),
        };
        assert_eq!(peek.bytes(), b"hello");
        let (peek_id, _) = peek.into_owner_parts();
        stack.resolve_tcp_receive(peek_id, 5).unwrap();

        let received = match stack
            .receive_tcp_stream(accepted, 5, TcpReceiveMode::Consume)
            .unwrap()
        {
            TcpStreamReceiveOutcome::Data(reservation) => reservation,
            TcpStreamReceiveOutcome::EndOfStream => panic!("peek must not consume"),
        };
        assert_eq!(received.bytes(), b"hello");
        let (received_id, _) = received.into_owner_parts();
        stack.resolve_tcp_receive(received_id, 5).unwrap();
        let eof = stack.receive_tcp_stream(accepted, 5, TcpReceiveMode::Consume);
        assert!(
            matches!(eof, Ok(TcpStreamReceiveOutcome::EndOfStream)),
            "buffered FIN must become EOF after consume, got {eof:?}"
        );

        let (_, client, accepted) = connected_pair(&mut stack, interface, 25004, 256);
        stack.send_tcp_stream(client, b"bytes").unwrap();
        drive(&mut stack, interface, 384);
        stack
            .release_tcp_endpoint(client, TcpReleaseReason::AcceptedChildRollback)
            .unwrap();
        drive(&mut stack, interface, 512);
        let TcpEndpointFacts::Connection(reset) = stack.tcp_endpoint_facts(accepted).unwrap()
        else {
            panic!("reset receiver lost connection facts");
        };
        assert_eq!(reset.received_bytes(), 5);
        assert!(reset.has_pending_error());
        assert!(reset.is_terminal());
        let observation = stack.observe_tcp_stream(accepted).unwrap();
        assert!(observation.has_received_bytes());
        assert!(observation.has_pending_error());
        assert!(!observation.end_of_stream());
        let received = match stack
            .receive_tcp_stream(accepted, 5, TcpReceiveMode::Consume)
            .unwrap()
        {
            TcpStreamReceiveOutcome::Data(reservation) => reservation,
            TcpStreamReceiveOutcome::EndOfStream => panic!("buffered bytes precede reset"),
        };
        let (received_id, _) = received.into_owner_parts();
        stack.resolve_tcp_receive(received_id, 5).unwrap();
        let observation = stack.observe_tcp_stream(accepted).unwrap();
        assert!(!observation.has_received_bytes());
        assert!(observation.has_pending_error());
        assert!(!observation.end_of_stream());
        assert_eq!(
            stack.receive_tcp_stream(accepted, 5, TcpReceiveMode::Consume),
            Err(TcpStreamReceiveError::ConnectionReset)
        );
        let TcpEndpointFacts::Connection(consumed_reset) =
            stack.tcp_endpoint_facts(accepted).unwrap()
        else {
            panic!("consumed reset lost connection facts");
        };
        assert!(!consumed_reset.has_pending_error());
        assert!(!consumed_reset.is_peer_receive_closed());
        assert!(stack.observe_tcp_stream(accepted).unwrap().end_of_stream());

        let (_, client, accepted) = connected_pair(&mut stack, interface, 25005, 640);
        stack.send_tcp_stream(client, b"r").unwrap();
        drive(&mut stack, interface, 768);
        assert_eq!(
            stack
                .shutdown_tcp_endpoint(accepted, TcpShutdownDirection::Read)
                .unwrap()
                .0,
            TcpShutdownOutcome::Changed
        );
        let observation = stack.observe_tcp_stream(accepted).unwrap();
        assert!(observation.has_received_bytes());
        assert!(!observation.end_of_stream());
        let received = match stack
            .receive_tcp_stream(accepted, 1, TcpReceiveMode::Consume)
            .unwrap()
        {
            TcpStreamReceiveOutcome::Data(reservation) => reservation,
            TcpStreamReceiveOutcome::EndOfStream => panic!("buffered bytes precede SHUT_RD EOF"),
        };
        let (received_id, _) = received.into_owner_parts();
        stack.resolve_tcp_receive(received_id, 1).unwrap();
        assert!(stack.observe_tcp_stream(accepted).unwrap().end_of_stream());
        assert_eq!(
            stack
                .shutdown_tcp_endpoint(accepted, TcpShutdownDirection::Read)
                .unwrap()
                .0,
            TcpShutdownOutcome::Unchanged
        );
        assert!(matches!(
            stack.receive_tcp_stream(accepted, 1, TcpReceiveMode::Consume),
            Ok(TcpStreamReceiveOutcome::EndOfStream)
        ));
        assert_eq!(
            stack
                .shutdown_tcp_endpoint(accepted, TcpShutdownDirection::ReadWrite)
                .unwrap()
                .0,
            TcpShutdownOutcome::Changed
        );
        assert_eq!(
            stack
                .shutdown_tcp_endpoint(accepted, TcpShutdownDirection::Write)
                .unwrap()
                .0,
            TcpShutdownOutcome::Unchanged
        );
        assert_eq!(
            stack.send_tcp_stream(accepted, b"x"),
            Err(TcpStreamSendError::BrokenStream)
        );
        // The peer remains owner-live until its own release.
        assert!(stack.tcp_connect_result(client).is_ok());
    }

    #[test]
    fn nodelay_and_release_reason_drive_engine_policy_and_cleanup() {
        let (mut stack, interface) = test_stack(POLICY);
        let listener = stack.create_tcp_endpoint().unwrap();
        stack.set_tcp_no_delay(listener, true).unwrap();
        stack
            .bind_tcp_endpoint(listener, TcpBindRequest::new(LOCAL, 25006))
            .unwrap();
        stack
            .listen_tcp_endpoint_with_backlog(
                listener,
                interface,
                Ipv4Address::UNSPECIFIED,
                TcpListenBacklog::new(1),
            )
            .unwrap();
        let client = stack.create_tcp_endpoint().unwrap();
        stack.set_tcp_no_delay(client, true).unwrap();
        let _ = stack
            .start_tcp_connect(
                client,
                Ipv4EgressSelection::new(interface, LOCAL),
                TcpPeer::new(LOCAL, 25006),
            )
            .unwrap();
        drive(&mut stack, interface, 0);
        let child = stack.claim_tcp_pending_child(listener).unwrap().unwrap();
        let accepted = stack.take_tcp_child(child).unwrap();
        assert!(stack.tcp_no_delay(accepted).unwrap());
        let handle = stack.protocols.tcp.connection(client).unwrap().handle;
        assert_eq!(
            stack
                .local
                .as_ref()
                .unwrap()
                .sockets
                .get::<tcp::Socket>(handle)
                .timeout(),
            None
        );
        assert!(
            !stack
                .local
                .as_ref()
                .unwrap()
                .sockets
                .get::<tcp::Socket>(handle)
                .nagle_enabled()
        );
        assert!(stack.set_tcp_no_delay(client, false).unwrap().is_some());
        assert!(
            stack
                .local
                .as_ref()
                .unwrap()
                .sockets
                .get::<tcp::Socket>(handle)
                .nagle_enabled()
        );

        let graceful_handle = stack.protocols.tcp.connection(client).unwrap().handle;
        assert!(
            stack
                .release_tcp_endpoint(client, TcpReleaseReason::FinalRelease)
                .unwrap()
                .is_some()
        );
        assert!(matches!(
            stack
                .local
                .as_ref()
                .unwrap()
                .sockets
                .get::<tcp::Socket>(graceful_handle)
                .state(),
            tcp::State::FinWait1 | tcp::State::LastAck
        ));

        let rollback_handle = stack.protocols.tcp.connection(accepted).unwrap().handle;
        assert!(
            stack
                .release_tcp_endpoint(accepted, TcpReleaseReason::CreationRollback)
                .unwrap()
                .is_some()
        );
        assert_eq!(
            stack
                .local
                .as_ref()
                .unwrap()
                .sockets
                .get::<tcp::Socket>(rollback_handle)
                .state(),
            tcp::State::Closed
        );
    }

    #[test]
    fn final_release_timeout_is_engine_owned_and_reclaims_an_orphan() {
        let policy = TcpPolicy::new(64, 64, 10, 32, 32, 64, 60_000, 1, 40000, 40063);
        let (mut stack, interface) = test_stack(policy);
        let (_, client, _accepted) = connected_pair(&mut stack, interface, 25010, 0);
        let handle = stack.protocols.tcp.connection(client).unwrap().handle;

        // Move the interface clock far beyond the last peer activity while no
        // timeout policy is installed. Final release must start a fresh orphan
        // interval rather than inherit that old receive timestamp.
        stack
            .pump_local(
                interface,
                Instant::from_micros(10_000_000),
                PumpBudget::new(32, 32),
            )
            .unwrap();

        assert!(
            stack
                .release_tcp_endpoint(client, TcpReleaseReason::FinalRelease)
                .unwrap()
                .is_some()
        );
        assert_eq!(
            stack
                .local
                .as_ref()
                .unwrap()
                .sockets
                .get::<tcp::Socket>(handle)
                .timeout(),
            Some(SmoltcpDuration::from_millis(1))
        );

        let mut device = FaultInjector::new(Loopback::new(Medium::Ip), 1);
        device.set_drop_chance(100);
        {
            let local = stack.local.as_mut().unwrap();
            let release_time = SmoltcpInstant::from_millis(10_000);
            local.interface.poll_maintenance(release_time);
            for _ in 0..64 {
                if local
                    .interface
                    .poll_egress(release_time, &mut device, &mut local.sockets)
                    == PollResult::None
                {
                    break;
                }
            }
            assert_eq!(
                local.sockets.get::<tcp::Socket>(handle).state(),
                tcp::State::FinWait1
            );

            let expired = SmoltcpInstant::from_millis(10_001);
            local.interface.poll_maintenance(expired);
            for _ in 0..64 {
                if local
                    .interface
                    .poll_egress(expired, &mut device, &mut local.sockets)
                    == PollResult::None
                {
                    break;
                }
            }
        }
        {
            let (protocols, local) = (&mut stack.protocols, stack.local.as_mut().unwrap());
            protocols
                .tcp
                .reclaim_interface(interface, &mut local.sockets);
        }
        assert!(stack.protocols.tcp.endpoint(client).is_none());
        assert!(stack.protocols.tcp.deferred.is_empty());
    }

    #[test]
    fn time_wait_reservation_rebind_requires_both_reuse_intents() {
        let (mut stack, interface) = test_stack(POLICY);
        let listener = stack.create_tcp_endpoint().unwrap();
        stack
            .bind_tcp_endpoint(listener, TcpBindRequest::new(LOCAL, 25007))
            .unwrap();
        stack
            .listen_tcp_endpoint_with_backlog(
                listener,
                interface,
                Ipv4Address::UNSPECIFIED,
                TcpListenBacklog::new(1),
            )
            .unwrap();
        let client = stack.create_tcp_endpoint().unwrap();
        stack.set_tcp_reuse_address(client, true).unwrap();
        stack
            .bind_tcp_endpoint(client, TcpBindRequest::new(LOCAL, 25008))
            .unwrap();
        let _ = stack
            .start_tcp_connect(
                client,
                Ipv4EgressSelection::new(interface, LOCAL),
                TcpPeer::new(LOCAL, 25007),
            )
            .unwrap();
        drive(&mut stack, interface, 0);

        let duplicate = stack.create_tcp_endpoint().unwrap();
        stack.set_tcp_reuse_address(duplicate, true).unwrap();
        stack
            .bind_tcp_endpoint(duplicate, TcpBindRequest::new(LOCAL, 25008))
            .unwrap();
        assert_eq!(
            stack.start_tcp_connect(
                duplicate,
                Ipv4EgressSelection::new(interface, LOCAL),
                TcpPeer::new(LOCAL, 25007),
            ),
            Err(anemone_net_api::tcp::TcpConnectError::PortInUse)
        );
        stack
            .release_tcp_endpoint(duplicate, TcpReleaseReason::CreationRollback)
            .unwrap();
        stack
            .release_tcp_endpoint(client, TcpReleaseReason::FinalRelease)
            .unwrap();

        let contender = stack.create_tcp_endpoint().unwrap();
        assert_eq!(
            stack.bind_tcp_endpoint(contender, TcpBindRequest::new(LOCAL, 25008)),
            Err(TcpBindError::PortInUse)
        );
        stack.set_tcp_reuse_address(contender, true).unwrap();
        stack
            .bind_tcp_endpoint(contender, TcpBindRequest::new(LOCAL, 25008))
            .unwrap();
        assert_eq!(
            stack.start_tcp_connect(
                contender,
                Ipv4EgressSelection::new(interface, LOCAL),
                TcpPeer::new(LOCAL, 25007),
            ),
            Err(anemone_net_api::tcp::TcpConnectError::PortInUse)
        );
        assert!(
            stack
                .start_tcp_connect(
                    contender,
                    Ipv4EgressSelection::new(interface, LOCAL),
                    TcpPeer::new(LOCAL, 25011),
                )
                .is_ok()
        );
    }
}
